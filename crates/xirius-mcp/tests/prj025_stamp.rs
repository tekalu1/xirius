//! PRJ-025: before each command the MCP server looks whether another process (the screen)
//! saved `project.json` (PRJ-024). It tells that from the file's size and modification time
//! first, and reads and hashes the file only when they are not what they were: a large song
//! takes long to hash. What it last took in is still compared by hash, so a file written with
//! the same size, or touched with the same bytes, is told right.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use xirius_core::session::{AiPermission, Confirm, RunOptions};
use xirius_core::{Project, Session};
use xirius_mcp::gateway::lock;
use xirius_mcp::host::RELOAD_EVENT;
use xirius_mcp::{Gateway, Scope};

fn folder() -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-mcp-stamp-")
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

fn reloaded(v: &Value) -> bool {
    v["events"]
        .as_array()
        .is_some_and(|e| e.iter().any(|e| e["event"] == RELOAD_EVENT))
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

fn project(dir: &Path) -> PathBuf {
    dir.join("project.json")
}

fn stamp(dir: &Path) -> (u64, SystemTime) {
    let m = std::fs::metadata(project(dir)).unwrap();
    (m.len(), m.modified().unwrap())
}

/// Gives `project.json` the modification time `at`, as a write long ago would have left.
fn set_time(dir: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(project(dir))
        .unwrap()
        .set_modified(at)
        .unwrap();
}

fn hashes(song: &Arc<Mutex<Session>>) -> u64 {
    lock(song).disk_hashes()
}

/// The screen renames its one track and saves: `project.json` keeps its size.
fn rename(screen: &mut Session, from: &str, to: &str) {
    assert_eq!(from.len(), to.len());
    let id = screen
        .project()
        .tracks
        .values()
        .find(|t| t.name == from)
        .unwrap()
        .id;
    screen
        .run_value(
            "track.rename",
            json!({ "id": id.to_string(), "name": to }),
            &RunOptions::human(),
        )
        .unwrap();
    screen.save().unwrap();
}

#[test]
fn an_unchanged_file_is_not_read_again_and_a_changed_one_comes_in_even_at_the_same_size() {
    let (_tmp, dir) = folder();
    let mut screen = Session::create(&dir, Project::new("Stamp")).unwrap();
    screen
        .run_value(
            "track.add.audio",
            json!({ "name": "A" }),
            &RunOptions::human(),
        )
        .unwrap();
    screen.save().unwrap();
    // Written a while ago: its size and time can be trusted.
    let long_ago = SystemTime::now() - Duration::from_secs(60);
    set_time(&dir, long_ago);
    let (song, gw) = server(&dir);

    // The first command reads the file once to know it is the song's own; the next do not.
    let before = hashes(&song);
    let v = call(&gw, "track_list", json!({}));
    assert!(!reloaded(&v), "{v}");
    assert_eq!(hashes(&song), before + 1);
    for _ in 0..5 {
        let v = call(&gw, "track_list", json!({}));
        assert!(!reloaded(&v), "{v}");
    }
    assert_eq!(
        hashes(&song),
        before + 1,
        "the same size and time: not read"
    );

    // Touched, with the same bytes: read once, and it is still the song's own.
    set_time(&dir, long_ago + Duration::from_secs(10));
    let v = call(&gw, "track_list", json!({}));
    assert!(!reloaded(&v), "the same bytes are not a change: {v}");
    assert_eq!(hashes(&song), before + 2);
    call(&gw, "track_list", json!({}));
    assert_eq!(hashes(&song), before + 2);

    // The screen saves another size: it comes in.
    screen
        .run_value(
            "track.add.audio",
            json!({ "name": "B" }),
            &RunOptions::human(),
        )
        .unwrap();
    screen.save().unwrap();
    let v = call(&gw, "track_list", json!({}));
    assert!(reloaded(&v), "{v}");
    assert_eq!(listed(&v), ["A", "B"]);

    // The same size, written now: it comes in.
    let (len, _) = stamp(&dir);
    rename(&mut screen, "B", "C");
    assert_eq!(stamp(&dir).0, len, "the file kept its size");
    let v = call(&gw, "track_list", json!({}));
    assert!(reloaded(&v), "{v}");
    assert_eq!(listed(&v), ["A", "C"]);

    // The same size and the same time, as a write in the same tick of a coarse clock leaves:
    // a file read too soon after it was written is read again, so it still comes in.
    let v = call(&gw, "track_list", json!({}));
    assert!(!reloaded(&v), "{v}");
    let seen = stamp(&dir);
    rename(&mut screen, "C", "D");
    set_time(&dir, seen.1);
    assert_eq!(
        stamp(&dir),
        seen,
        "nothing tells the write from the size and time"
    );
    let v = call(&gw, "track_list", json!({}));
    assert!(reloaded(&v), "{v}");
    assert_eq!(listed(&v), ["A", "D"]);
}

/// The time the check before a command takes on a song whose `project.json` is tens of MB,
/// with nothing changed: from the size and time, against reading and hashing the file.
/// Run by hand: `cargo test -p xirius-mcp --test prj025_stamp -- --ignored --nocapture`.
#[test]
#[ignore = "writes a song of tens of MB"]
fn the_check_before_a_command_on_a_large_song() {
    use xirius_core::model::{ClipBody, Note};
    let (_tmp, dir) = folder();
    let mut s = Session::new("large");
    let t = s
        .run_value("track.add.instrument", json!({}), &RunOptions::human())
        .unwrap();
    let t = t.data["id"].as_str().unwrap().to_string();
    let c = s
        .run_value(
            "clip.add.midi",
            json!({"track": t, "at": 0, "bars": 1000}),
            &RunOptions::human(),
        )
        .unwrap();
    let cid = c.data["id"].as_str().unwrap().parse().unwrap();
    let mut p = (*s.project_arc()).clone();
    let n = 300_000u64;
    let notes: Vec<Note> = (0..n)
        .map(|i| Note {
            id: xirius_core::id::NoteId(p.alloc()),
            start: (i * 37 % 3_000_000) as i64 + (i % 7) as i64,
            length: 240,
            pitch: (36 + i % 60) as u8,
            velocity: 100,
            muted: false,
            ai: false,
        })
        .collect();
    let (_, clip) = p.clip_mut(cid).unwrap();
    clip.length = 3_100_000;
    clip.body = ClipBody::Midi {
        notes: Arc::new(notes),
        transpose: 0,
    };
    drop(Session::create(&dir, p).unwrap());
    set_time(&dir, SystemTime::now() - Duration::from_secs(60));
    let mb = stamp(&dir).0 as f64 / 1e6;
    let (song, gw) = server(&dir);

    // The first check reads and hashes the file.
    let t = Instant::now();
    assert!(lock(&song).catch_up_with_disk().unwrap().is_none());
    let hashed = t.elapsed();
    let before = hashes(&song);

    let rounds = 100u32;
    let t = Instant::now();
    for _ in 0..rounds {
        assert!(lock(&song).catch_up_with_disk().unwrap().is_none());
    }
    let stamped = t.elapsed() / rounds;
    assert_eq!(hashes(&song), before, "not read again");

    let t = Instant::now();
    for _ in 0..rounds {
        call(&gw, "track_list", json!({}));
    }
    let listed = t.elapsed() / rounds;
    assert_eq!(hashes(&song), before);

    let t = Instant::now();
    for _ in 0..5 {
        lock(&song).store().unwrap().project_hash().unwrap();
    }
    let hash = t.elapsed() / 5;
    eprintln!(
        "project.json {mb:.1} MB: check with nothing changed {stamped:?} (first, hashing {hashed:?}; \
         a hash alone {hash:?}); a whole track_list {listed:?}"
    );
}
