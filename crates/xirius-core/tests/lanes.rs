//! One song open in two sessions at once (PRJ-023), as the screen and the `xirius` program
//! have it. Each session holds a lock of its own on the folder, as a process does, so two in
//! this process stand for two processes; `two_processes.rs` in xirius-cli runs real ones.

use std::path::{Path, PathBuf};

use serde_json::json;
use xirius_core::session::{RunOptions, Session};
use xirius_core::{Project, store::VersionKind};

const CRASH: &str = "前回は正常に終了しませんでした";

fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-lanes-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn add(s: &mut Session, name: &str) {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap();
}

fn names(s: &Session) -> Vec<String> {
    let mut n: Vec<String> = s
        .project()
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

fn crashed(s: &Session) -> bool {
    s.notices.iter().any(|n| n.contains(CRASH))
}

fn lane_index(dir: &Path, k: u32) -> PathBuf {
    dir.join("history")
        .join("lanes")
        .join(k.to_string())
        .join("index.json")
}

#[test]
fn the_song_opened_again_in_its_own_session_is_not_taken_for_a_crash() {
    let (_tmp, dir) = temp_dir("again");
    let mut s = Session::create(&dir, Project::new("Again")).unwrap();
    add(&mut s, "A1");
    s.save().unwrap();
    s.run_value(
        "project.open",
        json!({ "path": dir.to_string_lossy() }),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(!crashed(&s), "{:?}", s.notices);
    assert!(s.recovery().is_none());
    assert_eq!(names(&s), list(&["A1"]));

    // It still holds the first history: another session opened now keeps one of its own.
    let mut other = Session::open(&dir).unwrap();
    assert!(!crashed(&other), "{:?}", other.notices);
    add(&mut other, "B1");
    assert!(lane_index(&dir, 1).is_file());
    drop(other);
    drop(s);
    assert!(!dir.join("session.lock").exists());
}

#[test]
fn two_sessions_number_their_versions_apart() {
    let (_tmp, dir) = temp_dir("versions");
    let mut a = Session::create(&dir, Project::new("Versions")).unwrap();
    let mut b = Session::open(&dir).unwrap();
    add(&mut a, "A1");
    add(&mut b, "B1");
    let va = a.save_version("from a").unwrap();
    let vb = b.save_version("from b").unwrap();
    assert_ne!(va.id, vb.id);
    let all: Vec<String> = a.versions().unwrap().into_iter().map(|v| v.id).collect();
    assert_eq!(all, vec![va.id.clone(), vb.id.clone()]);
    // Each version holds the song of the session that made it.
    let mut c = Session::open(&dir).unwrap();
    c.restore_version(&vb.id, &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(names(&c), list(&["B1"]));
    c.restore_version(&va.id, &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(names(&c), list(&["A1"]));
}

#[test]
fn a_second_session_that_went_down_is_picked_up_by_the_next_one() {
    let (_tmp, dir) = temp_dir("second");
    let mut first = Session::create(&dir, Project::new("Second")).unwrap();
    add(&mut first, "A1");
    first.save().unwrap();

    let mut b = Session::open(&dir).unwrap();
    add(&mut b, "B1");
    add(&mut b, "B2");
    b.simulate_crash();
    assert!(
        dir.join("session.lock").is_file(),
        "the first one's lock stays"
    );

    // The next one takes the place it left: its steps are there to undo.
    let mut c = Session::open(&dir).unwrap();
    assert!(crashed(&c), "{:?}", c.notices);
    assert_eq!(c.recovery().map(|r| r.steps), Some(2));
    assert_eq!(names(&c), list(&["A1", "B1", "B2"]));
    c.undo(1).unwrap();
    assert_eq!(names(&c), list(&["A1", "B1"]));
    drop(c);
    assert_eq!(names(&first), list(&["A1"]));
    drop(first);

    // Once no one has the song open, where the other one got to is kept as a version.
    let mut s = Session::open(&dir).unwrap();
    assert!(!crashed(&s), "{:?}", s.notices);
    assert_eq!(names(&s), list(&["A1"]));
    assert!(!lane_index(&dir, 1).exists(), "the lane is closed");
    let kept: Vec<_> = s
        .versions()
        .unwrap()
        .into_iter()
        .filter(|v| v.kind == VersionKind::OtherProcess)
        .collect();
    assert_eq!(kept.len(), 1);
    s.restore_version(&kept[0].id, &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(names(&s), list(&["A1", "B1"]));
}

#[test]
fn a_session_that_only_read_leaves_no_lane_behind() {
    let (_tmp, dir) = temp_dir("read");
    let s = Session::create(&dir, Project::new("Read")).unwrap();
    let other = Session::open(&dir).unwrap();
    assert!(dir.join("history/lanes/1/session.lock").is_file());
    drop(other);
    assert!(!dir.join("history/lanes/1").exists());
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert!(s.versions().unwrap().is_empty());
}

#[test]
fn what_a_crash_left_in_the_blobs_goes_with_the_next_save() {
    let (_tmp, dir) = temp_dir("leftovers");
    let mut s = Session::create(&dir, Project::new("Leftovers")).unwrap();
    add(&mut s, "A1");
    s.save().unwrap();
    // A step cut off between its blob and its state, and a blob cut off in its write.
    let blobs = dir.join("history").join("blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    let stray = blobs.join("ab".repeat(32));
    let half = blobs.join(format!("{}.tmp4242", "cd".repeat(32)));
    std::fs::write(&stray, "x".repeat(5000)).unwrap();
    std::fs::write(&half, "y").unwrap();
    s.simulate_crash();

    let mut s = Session::open(&dir).unwrap();
    assert!(crashed(&s), "{:?}", s.notices);
    s.save().unwrap();
    assert!(!stray.exists());
    assert!(!half.exists());
}
