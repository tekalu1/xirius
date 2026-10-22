//! Two processes with one song open (PRJ-023): the screen's session in this process, the
//! `xirius` program in others. Each keeps its own history, so neither one's steps are written
//! over, cut off or cleared up by the other, and what the other was doing is still there once
//! the song is opened again. Nothing here opens a device or makes a sound.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

use serde_json::{Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_core::store::Store;

const CRASH: &str = "前回は正常に終了しませんでした";

fn xirius(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_xirius"));
    c.env_remove("XIRIUS_SONG")
        .env("XIRIUS_TRASH_DIR", dir.with_extension("bin"))
        .current_dir(std::env::temp_dir())
        .arg("--song")
        .arg(dir);
    c
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Runs `xirius --song <dir> <args> --json` to its end; it has to succeed.
fn cli(dir: &Path, args: &[&str]) -> Output {
    let out = xirius(dir)
        .args(args)
        .arg("--json")
        .output()
        .expect("run xirius");
    assert!(
        out.status.success(),
        "{args:?} failed: {}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    out
}

/// The track names the program's own view of the song has (the master left out), sorted.
fn cli_names(dir: &Path) -> Vec<String> {
    let out = cli(dir, &["--no-save", "track.list"]);
    let v: Value = serde_json::from_slice(&out.stdout).expect("json output");
    let rows = match &v["data"] {
        Value::Array(a) => a.clone(),
        other => other["tracks"].as_array().cloned().unwrap_or_default(),
    };
    let mut names: Vec<String> = rows
        .iter()
        .filter_map(|r| r["name"].as_str())
        .filter(|n| *n != "Master")
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

fn names(p: &Project) -> Vec<String> {
    let mut n: Vec<String> = p
        .tracks
        .values()
        .map(|t| t.name.clone())
        .filter(|n| n != "Master")
        .collect();
    n.sort();
    n
}

fn list(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn add(s: &mut Session, name: &str) {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap();
}

/// The screen's session: a new song in `dir`, writing its history on a thread of its own as
/// the window's does.
fn screen(dir: &Path) -> Session {
    let mut s = Session::create(dir, Project::new("Two")).unwrap();
    s.write_in_background(true);
    s
}

/// `xirius batch` kept running, one line at a time: a long-lived second process.
struct Batch {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Batch {
    fn start(dir: &Path, flags: &[&str]) -> Batch {
        let mut child = xirius(dir)
            .args(flags)
            .args(["--json", "batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start xirius batch");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Batch {
            child,
            stdin,
            stdout,
        }
    }

    /// Sends one line and waits for its answer.
    fn run(&mut self, line: &str) -> Value {
        writeln!(self.stdin, "{line}").unwrap();
        self.stdin.flush().unwrap();
        let mut answer = String::new();
        self.stdout.read_line(&mut answer).unwrap();
        serde_json::from_str(&answer).unwrap_or_else(|e| panic!("{line}: {e}: {answer}"))
    }

    /// Goes down as a killed process does: nothing is closed properly.
    fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The versions another process's history was kept in when the song was opened again.
fn kept_from_others(s: &Session) -> Vec<String> {
    s.versions()
        .unwrap()
        .into_iter()
        .filter(|v| json!(v.kind) == json!("other_process"))
        .map(|v| v.id)
        .collect()
}

#[test]
fn the_screen_and_the_program_each_undo_their_own_steps() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut g = screen(&dir);
    add(&mut g, "G1");
    g.save().unwrap();

    // The program edits the song the screen has open, and saves (as it does after each command).
    let out = cli(&dir, &["track.add.audio", "--name", "C1"]);
    assert!(!text(&out.stderr).contains(CRASH), "{}", text(&out.stderr));
    assert!(
        dir.join("session.lock").is_file(),
        "the screen's lock outlives the program"
    );

    // The screen goes on: a step undone and replaced, then a save that clears up after it.
    add(&mut g, "G2");
    g.undo(1).unwrap();
    add(&mut g, "G3");
    g.save().unwrap();
    g.flush_writes();

    // The program goes on from what the screen saved, taken in as a step of its own (UI-289).
    cli(&dir, &["track.add.audio", "--name", "C2"]);
    assert_eq!(cli_names(&dir), list(&["C2", "G1", "G3"]));

    // The screen's steps are its own.
    assert_eq!(names(g.project()), list(&["G1", "G3"]));
    g.undo(1).unwrap();
    assert_eq!(names(g.project()), list(&["G1"]));
    g.undo(1).unwrap();
    assert_eq!(names(g.project()), list(&[]));
    g.redo(2).unwrap();
    assert_eq!(names(g.project()), list(&["G1", "G3"]));
    assert!(g.take_write_errors().is_empty());

    // And the program's, from one run to the next, the screen's save among them.
    cli(&dir, &["history.undo"]);
    assert_eq!(cli_names(&dir), list(&["G1", "G3"]));
    cli(&dir, &["history.undo"]);
    assert_eq!(cli_names(&dir), list(&["C1", "G1"]));
    cli(&dir, &["history.undo"]);
    assert_eq!(cli_names(&dir), list(&["G1"]));
    for _ in 0..3 {
        cli(&dir, &["history.redo"]);
    }
    assert_eq!(cli_names(&dir), list(&["C2", "G1", "G3"]));
    assert!(dir.join("session.lock").is_file());

    // The screen closes and the song opens again: the screen's history goes on, and where the
    // program got to is kept as a version that brings it back.
    drop(g);
    assert!(!dir.join("session.lock").exists());
    let mut s = Session::open(&dir).unwrap();
    assert!(s.recovery().is_none(), "{:?}", s.notices);
    assert_eq!(names(s.project()), list(&["G1", "G3"]));
    s.undo(1).unwrap();
    assert_eq!(names(s.project()), list(&["G1"]));
    let kept = kept_from_others(&s);
    assert_eq!(kept.len(), 1, "{:?}", s.versions().unwrap());
    s.restore_version(&kept[0], &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(names(s.project()), list(&["C2", "G1", "G3"]));
}

#[test]
fn a_crash_of_the_first_process_is_recovered_with_the_others_work_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut g = Session::create(&dir, Project::new("Crash")).unwrap();
    g.save().unwrap();
    drop(g);

    // A long-lived process (the screen's stand-in) edits without saving ...
    let mut b = Batch::start(&dir, &["--no-save"]);
    assert_eq!(b.run("track.add.audio --name B1")["ok"], json!(true));
    assert_eq!(b.run("track.add.audio --name B2")["ok"], json!(true));

    // ... while the program edits the same song and closes properly.
    let out = cli(&dir, &["track.add.audio", "--name", "C1"]);
    assert!(!text(&out.stderr).contains(CRASH), "{}", text(&out.stderr));

    // The long-lived process is killed.
    b.kill();

    let mut s = Session::open(&dir).unwrap();
    assert!(
        s.notices.iter().any(|n| n.contains(CRASH)),
        "the crash is told: {:?}",
        s.notices
    );
    assert_eq!(s.recovery().map(|r| r.steps), Some(2));
    assert_eq!(names(s.project()), list(&["B1", "B2"]));
    s.undo(1).unwrap();
    assert_eq!(names(s.project()), list(&["B1"]));
    s.redo(1).unwrap();
    let kept = kept_from_others(&s);
    assert_eq!(kept.len(), 1, "{:?}", s.versions().unwrap());
    s.restore_version(&kept[0], &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(names(s.project()), list(&["B1", "B2", "C1"]));
}

/// A plugin's state past the length a blob starts at, short enough for a command line.
fn big_state(seed: &str) -> String {
    seed.repeat(8000 / seed.len())
}

fn plugin_state(p: &Project, plugin: &str) -> Option<String> {
    p.tracks
        .values()
        .flat_map(|t| t.inserts.iter())
        .find(|i| i.id.to_string() == plugin)
        .and_then(|i| i.state.clone())
}

/// The plugin's state in `project.json` (the program saves after each command).
fn saved_state(dir: &Path, plugin: &str) -> Option<String> {
    plugin_state(&Store::new(dir).read_project().unwrap(), plugin)
}

#[test]
fn clearing_up_after_a_save_leaves_the_other_processs_long_states() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut g = screen(&dir);
    let h = RunOptions::human();
    let track = g
        .run_value("track.add.audio", json!({ "name": "Synth" }), &h)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let plugin = g
        .run_value(
            "plugin.add",
            json!({ "track": track, "name": "Sampler", "format": "vst3", "uid": "vendor/sampler" }),
            &h,
        )
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let set = |s: &mut Session, state: &str| {
        s.run_value(
            "plugin.state.set",
            json!({ "id": plugin, "state": state }),
            &RunOptions::human(),
        )
        .unwrap();
    };
    set(&mut g, &big_state("AAEC"));
    g.save().unwrap();

    // The program puts states of its own in, kept as blobs only its history points to.
    for seed in ["BBED", "CCEE"] {
        let state = big_state(seed);
        cli(
            &dir,
            &["plugin.state.set", "--id", &plugin, "--state", &state],
        );
    }
    assert_eq!(saved_state(&dir, &plugin), Some(big_state("CCEE")));

    // The screen lets go of a state of its own and saves, which clears up the blobs.
    set(&mut g, &big_state("DDEF"));
    g.undo(1).unwrap();
    set(&mut g, &big_state("EEFF"));
    g.save().unwrap();
    g.flush_writes();
    assert!(g.take_write_errors().is_empty());

    // The program still goes back and forth through its own states, read from the disk. It
    // goes on from the screen's save, so the first undo takes that back (UI-289).
    cli(&dir, &["history.undo"]);
    assert_eq!(saved_state(&dir, &plugin), Some(big_state("CCEE")));
    cli(&dir, &["history.undo"]);
    assert_eq!(saved_state(&dir, &plugin), Some(big_state("BBED")));
    cli(&dir, &["history.undo"]);
    assert_eq!(saved_state(&dir, &plugin), Some(big_state("AAEC")));
    cli(&dir, &["history.redo"]);
    cli(&dir, &["history.redo"]);
    assert_eq!(saved_state(&dir, &plugin), Some(big_state("CCEE")));

    // And the screen through its own, from the disk too once it opens the song again.
    assert_eq!(plugin_state(g.project(), &plugin), Some(big_state("EEFF")));
    drop(g);
    let mut s = Session::open(&dir).unwrap();
    assert_eq!(plugin_state(s.project(), &plugin), Some(big_state("EEFF")));
    s.undo(1).unwrap();
    assert_eq!(plugin_state(s.project(), &plugin), Some(big_state("AAEC")));
    let kept = kept_from_others(&s);
    assert_eq!(kept.len(), 1, "{:?}", s.versions().unwrap());
    s.restore_version(&kept[0], &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(plugin_state(s.project(), &plugin), Some(big_state("CCEE")));
}

#[test]
fn the_recordings_are_not_cleared_up_while_another_process_has_the_song_open() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut g = screen(&dir);
    g.save().unwrap();
    let recordings = dir.join("Recordings");
    std::fs::create_dir_all(&recordings).unwrap();
    let take = recordings.join("Vox-1.wav");
    std::fs::write(&take, b"RIFF").unwrap();

    // The list is shown ...
    let out = cli(&dir, &["project.cleanup", "--dry-run"]);
    assert!(
        text(&out.stdout).contains("Vox-1.wav"),
        "{}",
        text(&out.stdout)
    );
    // ... but nothing is moved: the screen may be recording that file.
    let out = xirius(&dir)
        .args(["project.cleanup", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "{}", text(&out.stdout));
    assert!(
        text(&out.stdout).contains("ほかのプロセス")
            || text(&out.stderr).contains("ほかのプロセス"),
        "{}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    assert!(take.is_file());

    // Once the screen has closed, it goes.
    drop(g);
    cli(&dir, &["project.cleanup"]);
    assert!(!take.exists());
}
