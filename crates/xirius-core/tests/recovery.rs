//! A song that was open when the process died (PRJ-001, PRJ-012): the process
//! is really killed, in a temp folder, and the song is opened again.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use xirius_core::recovery;
use xirius_core::session::{RunOptions, Session};

const CHILD: &str = "XIRIUS_RECOVERY_CHILD";

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-recovery-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn run(s: &mut Session, name: &str, args: Value) {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

fn track_names(s: &Session) -> Vec<String> {
    let mut names: Vec<String> = s
        .project()
        .tracks
        .values()
        .map(|t| t.name.clone())
        .collect();
    names.sort();
    names
}

/// Runs only in the child process the tests below start: makes a song, saves
/// it, edits three more times, says so and waits to be killed.
#[test]
fn child_that_waits_to_be_killed() {
    let Ok(dir) = std::env::var(CHILD) else {
        return;
    };
    let draft = dir.ends_with("-draft");
    let mut s = Session::new("落ちる曲");
    if draft {
        run(&mut s, "track.add.audio", json!({"name": "first"}));
        s.keep_as_draft(&dir).unwrap();
    } else {
        run(&mut s, "track.add.audio", json!({"name": "saved"}));
        s.save_as(&dir).unwrap();
    }
    for name in ["a", "b", "c"] {
        run(&mut s, "track.add.audio", json!({"name": name}));
    }
    println!("READY");
    std::thread::sleep(std::time::Duration::from_secs(120));
}

/// Starts the child on `dir`, waits for its edits, and kills it.
fn kill_a_running_session(dir: &PathBuf) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_that_waits_to_be_killed", "--nocapture"])
        .env(CHILD, dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ready = false;
    for line in lines.by_ref() {
        if line.unwrap().trim() == "READY" {
            ready = true;
            break;
        }
    }
    assert!(ready, "the child never got as far as editing");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn a_killed_process_leaves_a_song_that_opens_at_its_last_step_with_a_count() {
    let (_dir_tmp, dir) = temp_dir("killed");
    kill_a_running_session(&dir);
    assert!(dir.join("session.lock").exists());
    assert!(
        recovery::crashed(&dir),
        "the lock names a process that is gone"
    );

    let s = Session::open(&dir).unwrap();
    assert_eq!(track_names(&s), ["Master", "a", "b", "c", "saved"]);
    let r = s.recovery().expect("a crashed song says what came back");
    assert_eq!(r.steps, 3);
    assert_eq!(r.last_command.as_deref(), Some("track.add.audio"));
    assert!(r.last_at_ms > 0);
    let id = r
        .version
        .clone()
        .expect("the saved state is kept as a version");
    assert!(
        s.versions()
            .unwrap()
            .iter()
            .any(|v| v.id == id && format!("{:?}", v.kind).contains("Crash"))
    );
    // An open song holds the lock itself, so it is not a crashed one.
    assert!(!recovery::crashed(&dir));
}

#[test]
fn discarding_goes_back_to_the_save_and_the_steps_stay_as_redo() {
    let (_dir_tmp, dir) = temp_dir("discard");
    kill_a_running_session(&dir);
    let mut s = Session::open(&dir).unwrap();
    s.discard_unsaved().unwrap();
    assert_eq!(track_names(&s), ["Master", "saved"]);
    assert!(s.recovery().is_none());
    s.redo(3).unwrap();
    assert_eq!(track_names(&s), ["Master", "a", "b", "c", "saved"]);
}

#[test]
fn a_draft_comes_back_with_every_step_and_discards_to_where_it_began() {
    let (_dir_tmp, dir) = temp_dir("killed-draft");
    kill_a_running_session(&dir);
    let mut s = Session::open(&dir).unwrap();
    // The first track was added before the song was kept: it is where the
    // history begins, and the three after it are what comes back.
    assert_eq!(s.recovery().unwrap().steps, 4);
    assert_eq!(track_names(&s), ["Master", "a", "b", "c", "first"]);
    assert!(s.is_dirty(), "a draft is never a saved song");
    s.discard_unsaved().unwrap();
    assert_eq!(track_names(&s), ["Master"]);
}

#[test]
fn an_automatic_save_point_does_not_move_where_the_song_was_saved() {
    let (_dir_tmp, dir) = temp_dir("auto");
    let mut s = Session::new("auto");
    run(&mut s, "track.add.audio", json!({"name": "saved"}));
    s.save_as(&dir).unwrap();
    s.auto_interval_ms = 0;
    run(&mut s, "track.add.audio", json!({"name": "later"}));
    let now = xirius_core::store::now_ms() + 1;
    assert!(s.autosave_if_due(now).unwrap().is_some());
    assert_eq!(s.saved_step(), 1);
    s.simulate_crash();
    let s = Session::open(&dir).unwrap();
    assert_eq!(s.recovery().unwrap().steps, 1, "the step since the save");
}

#[test]
fn a_song_closed_properly_says_nothing_about_a_crash() {
    let (_dir_tmp, dir) = temp_dir("clean");
    let mut s = Session::new("clean");
    run(&mut s, "track.add.audio", json!({"name": "x"}));
    s.save_as(&dir).unwrap();
    drop(s);
    assert!(!recovery::crashed(&dir));
    let s = Session::open(&dir).unwrap();
    assert!(s.recovery().is_none());
}
