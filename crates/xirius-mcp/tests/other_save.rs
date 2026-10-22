//! PRJ-024: the MCP server holds its song open across commands, so what another process (the
//! screen) saved to `project.json` meanwhile comes in first, as one step `project.reload`
//! (「ほかのアプリの変更」, UI-289), as a command-line run that opens the song takes it. Each
//! session holds a lane of its own on the folder, as a process does (PRJ-023), so the second
//! session stands for the screen.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use xirius_core::command::Author;
use xirius_core::session::{AiPermission, Confirm, RunOptions};
use xirius_core::store::Store;
use xirius_core::{Project, Session};
use xirius_mcp::gateway::lock;
use xirius_mcp::host::RELOAD_EVENT;
use xirius_mcp::{Gateway, Scope};

fn folder() -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-mcp-other-save-")
        .tempdir()
        .unwrap();
    let dir = base.path().join("song");
    (base, dir)
}

/// The MCP server's session on `dir`, for an AI that may save without asking.
fn server(dir: &Path) -> (Arc<Mutex<Session>>, Gateway) {
    let song = Arc::new(Mutex::new(Session::open(dir).unwrap()));
    let gw = Gateway::new(song.clone(), "claude", Scope::Edit);
    let mut s = lock(&song);
    s.set_ai_permission(AiPermission::Save).unwrap();
    s.set_confirm(Confirm {
        destructive: false,
        save: false,
    });
    drop(s);
    (song, gw)
}

fn call(gw: &Gateway, tool: &str, args: Value) -> Value {
    let (v, error) = gw.call(tool, args.as_object().unwrap()).unwrap();
    assert!(!error, "{tool}: {v}");
    v
}

fn names_of(p: &Project) -> Vec<String> {
    let mut n: Vec<String> = p
        .tracks
        .values()
        .map(|t| t.name.clone())
        .filter(|n| n != "Master")
        .collect();
    n.sort();
    n
}

fn names(s: &Arc<Mutex<Session>>) -> Vec<String> {
    names_of(lock(s).project())
}

/// The track names a `track.list` answered with.
fn listed(v: &Value) -> Vec<String> {
    let mut n: Vec<String> = v["data"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .filter(|n| n != "Master")
        .collect();
    n.sort();
    n
}

fn reloads(v: &Value) -> Vec<&Value> {
    v["events"]
        .as_array()
        .map(|e| e.iter().filter(|e| e["event"] == RELOAD_EVENT).collect())
        .unwrap_or_default()
}

fn cursor(s: &Arc<Mutex<Session>>) -> u64 {
    lock(s).cursor()
}

fn last_command(s: &Arc<Mutex<Session>>) -> String {
    let rows = lock(s).history();
    let row = rows.iter().find(|r| r.current).unwrap();
    row.command.clone()
}

fn add(s: &mut Session, name: &str) {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap();
}

fn on_disk(dir: &Path) -> Vec<String> {
    names_of(&Store::new(dir).read_project().unwrap())
}

#[test]
fn the_screen_s_save_comes_in_as_one_step_before_the_next_command_and_undo_takes_it_back() {
    let (_tmp, dir) = folder();
    let mut screen = Session::create(&dir, Project::new("Other")).unwrap();
    add(&mut screen, "A");
    screen.save().unwrap();
    let (song, gw) = server(&dir);

    // Its own save is not another process's: no step comes in.
    let v = call(&gw, "track_add_audio", json!({"name": "M"}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert!(!lock(&song).is_dirty(), "an AI with 保存 saves its edit");
    let at = cursor(&song);
    let v = call(&gw, "track_list", json!({}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert_eq!(cursor(&song), at);
    assert_eq!(on_disk(&dir), ["A", "M"]);

    // The screen takes the AI's save in, adds a track and saves.
    assert!(screen.reload_from_disk(&Author::Human).unwrap().is_some());
    add(&mut screen, "S");
    screen.save().unwrap();

    // A command that only looks answers on the song the screen saved, one step up.
    let v = call(&gw, "track_list", json!({}));
    assert_eq!(listed(&v), ["A", "M", "S"]);
    let told = reloads(&v);
    assert_eq!(told.len(), 1, "{v}");
    assert_eq!(told[0]["step"], at + 1, "{v}");
    let msg = told[0]["message"].as_str().unwrap();
    assert!(msg.contains("ほかのアプリの変更"), "{msg}");
    assert_eq!(cursor(&song), at + 1);
    assert_eq!(last_command(&song), "project.reload");
    assert!(!lock(&song).is_dirty(), "the song is the file");
    // Taken once: the next command finds nothing new.
    let v = call(&gw, "track_list", json!({}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert_eq!(cursor(&song), at + 1);

    // The AI's save compares with the file it took in, so it is not refused.
    let v = call(&gw, "project_save", json!({}));
    assert_eq!(v["ok"], true, "{v}");

    // The first undo takes back what came in, and the song is saved so.
    let v = call(&gw, "history_undo", json!({}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert_eq!(names(&song), ["A", "M"]);
    assert_eq!(cursor(&song), at);
    assert_eq!(on_disk(&dir), ["A", "M"]);
    // That save is the server's own: it does not come in again.
    let v = call(&gw, "track_list", json!({}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert_eq!(listed(&v), ["A", "M"]);
}

#[test]
fn a_write_goes_on_from_the_screen_s_save_instead_of_writing_over_it() {
    let (_tmp, dir) = folder();
    // The server opened the song first (the first lane); the screen came after.
    Session::create(&dir, Project::new("Other")).unwrap();
    let (song, gw) = server(&dir);
    call(&gw, "track_add_audio", json!({"name": "M"}));
    let mut screen = Session::open(&dir).unwrap();
    assert_eq!(names_of(screen.project()), ["M"]);
    add(&mut screen, "S");
    screen.save().unwrap();
    drop(screen);

    let at = cursor(&song);
    let v = call(&gw, "track_add_audio", json!({"name": "N"}));
    assert_eq!(reloads(&v).len(), 1, "{v}");
    assert_eq!(
        cursor(&song),
        at + 2,
        "the screen's save, then the AI's track"
    );
    assert_eq!(on_disk(&dir), ["M", "N", "S"], "the screen's track stays");

    // Undo goes back over the AI's track first, then over what came in.
    call(&gw, "history_undo", json!({}));
    assert_eq!(names(&song), ["M", "S"]);
    call(&gw, "history_undo", json!({}));
    assert_eq!(names(&song), ["M"]);
    assert_eq!(on_disk(&dir), ["M"]);
}

#[test]
fn with_unsaved_steps_the_song_stays_and_the_ai_s_save_is_refused() {
    let (_tmp, dir) = folder();
    let mut screen = Session::create(&dir, Project::new("Other")).unwrap();
    screen.save().unwrap();
    let (song, gw) = server(&dir);
    // An AI that may not save leaves its edit unsaved.
    lock(&song).set_ai_permission(AiPermission::Edit).unwrap();
    call(&gw, "track_add_audio", json!({"name": "M"}));
    assert!(lock(&song).is_dirty());

    add(&mut screen, "S");
    screen.save().unwrap();

    let at = cursor(&song);
    let v = call(&gw, "track_list", json!({}));
    assert!(reloads(&v).is_empty(), "{v}");
    assert_eq!(listed(&v), ["M"]);
    assert_eq!(cursor(&song), at);

    // AI-018 as before: the file is not the one the server read, so nothing is written.
    lock(&song).set_ai_permission(AiPermission::Save).unwrap();
    let (v, error) = gw
        .call("project_save", json!({}).as_object().unwrap())
        .unwrap();
    assert!(error, "{v}");
    assert_eq!(v["error"]["code"], "conflict", "{v}");
    assert_eq!(on_disk(&dir), ["S"]);
}
