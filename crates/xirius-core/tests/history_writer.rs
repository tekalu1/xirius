//! The history written by a thread of its own (PRJ-022): an edit returns before its files
//! reach a slow disk, and what needs the files waits for them.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::model::TrackKind;
use xirius_core::session::RunOptions;
use xirius_core::{Project, Session};

/// Each write waits this long, as a USB drive or a synced folder would.
const SLOW: Duration = Duration::from_millis(300);

fn add_track(s: &mut Session, name: &str) -> String {
    s.run_value(
        "track.add.audio",
        json!({ "name": name }),
        &RunOptions::human(),
    )
    .unwrap()
    .message
}

/// The tracks' names, past the master.
fn names(s: &Session) -> Vec<String> {
    s.project()
        .tracks
        .values()
        .filter(|t| t.kind != TrackKind::Master)
        .map(|t| t.name.clone())
        .collect()
}

fn background_song(dir: &Path) -> Session {
    let mut s = Session::create(dir, Project::new("writer")).unwrap();
    s.write_in_background(true);
    s.simulate_slow_disk(SLOW);
    s
}

#[test]
fn edits_on_a_slow_disk_return_before_their_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = background_song(&dir);

    let t = Instant::now();
    for i in 0..4 {
        add_track(&mut s, &format!("T{i}"));
    }
    let took = t.elapsed();
    // Written on the spot, the four would take four times SLOW.
    assert!(took < SLOW, "{took:?}");
    assert!(s.writes_pending());
    assert!(!dir.join("history").join("4.json").exists());

    s.flush_writes();
    assert!(!s.writes_pending());
    assert!(dir.join("history").join("4.json").is_file());
    assert!(s.take_write_errors().is_empty());
}

#[test]
fn a_session_written_on_the_spot_is_on_the_disk_when_the_edit_returns() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = Session::create(&dir, Project::new("writer")).unwrap();
    assert!(!s.writes_in_background());
    add_track(&mut s, "A");
    assert!(!s.writes_pending());
    assert!(dir.join("history").join("1.json").is_file());
}

#[test]
fn closing_waits_for_the_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = background_song(&dir);
    add_track(&mut s, "A");
    add_track(&mut s, "B");
    s.undo(1).unwrap();
    drop(s);

    // Not saved: the history brings the edits back, with the undo.
    let mut s = Session::open(&dir).unwrap();
    assert_eq!(names(&s), ["A"]);
    s.redo(1).unwrap();
    assert_eq!(names(&s), ["A", "B"]);
}

#[test]
fn saving_waits_for_the_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = background_song(&dir);
    add_track(&mut s, "A");
    add_track(&mut s, "B");
    s.save().unwrap();
    assert!(!s.writes_pending());
    assert!(dir.join("history").join("2.json").is_file());
    let index = std::fs::read_to_string(dir.join("history").join("index.json")).unwrap();
    assert!(index.contains("\"cursor\":2"), "{index}");
}

#[test]
fn saving_as_takes_the_steps_not_yet_written() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let copy = tmp.path().join("copy");
    let mut s = background_song(&dir);
    add_track(&mut s, "A");
    add_track(&mut s, "B");
    s.save_as(&copy).unwrap();
    add_track(&mut s, "C");
    drop(s);

    let mut c = Session::open(&copy).unwrap();
    assert_eq!(names(&c), ["A", "B", "C"]);
    c.undo(2).unwrap();
    assert_eq!(names(&c), ["A"]);
    // The first folder goes back to its saved song, with the steps written as redo.
    let mut o = Session::open(&dir).unwrap();
    assert!(names(&o).is_empty());
    o.redo(2).unwrap();
    assert_eq!(names(&o), ["A", "B"]);
}

#[test]
fn a_failed_write_on_the_thread_is_handed_over() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = background_song(&dir);
    s.simulate_slow_disk(Duration::ZERO);
    add_track(&mut s, "A");
    s.flush_writes();
    // The next state's file cannot be written: a folder has its name.
    std::fs::create_dir(dir.join("history").join("2.json")).unwrap();
    let message = add_track(&mut s, "B");
    assert!(!message.contains("履歴を書き込めませんでした"), "{message}");
    assert_eq!(names(&s), ["A", "B"]);

    s.flush_writes();
    let errors = s.take_write_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("履歴を書き込めませんでした"),
        "{errors:?}"
    );
    // Handed over once.
    assert!(s.take_write_errors().is_empty());
}

#[test]
fn a_failed_write_on_the_spot_comes_back_in_the_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = Session::create(&dir, Project::new("writer")).unwrap();
    add_track(&mut s, "A");
    std::fs::create_dir(dir.join("history").join("2.json")).unwrap();
    let message = add_track(&mut s, "B");
    assert!(message.contains("履歴を書き込めませんでした"), "{message}");
    assert!(s.take_write_errors().is_empty());
}

#[test]
fn a_failed_index_after_an_undo_is_handed_over() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = Session::create(&dir, Project::new("writer")).unwrap();
    add_track(&mut s, "A");
    let index = dir.join("history").join("index.json");
    std::fs::remove_file(&index).unwrap();
    std::fs::create_dir(&index).unwrap();
    s.undo(1).unwrap();
    assert!(names(&s).is_empty());
    let errors = s.take_write_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
}

#[test]
fn an_automatic_save_point_is_written_after_the_steps_before_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = background_song(&dir);
    add_track(&mut s, "A");
    let t = Instant::now();
    let info = s.autosave_if_due(u64::MAX / 2).unwrap().unwrap();
    assert!(t.elapsed() < SLOW, "{:?}", t.elapsed());

    // The list has it while it is on the way, without waiting (the UI reads it per edit).
    let t = Instant::now();
    let versions = s.versions().unwrap();
    assert!(t.elapsed() < SLOW, "{:?}", t.elapsed());
    assert!(s.writes_pending());
    assert!(versions.iter().any(|v| v.id == info.id), "{versions:?}");
    // Reading it waits for it.
    let saved = s.store().unwrap().read_version(&info.id).unwrap();
    assert_eq!(saved.tracks.len(), s.project().tracks.len());
    // A named version after it takes the next number, and the list keeps both.
    s.run_value(
        "version.save",
        json!({ "name": "mix" }),
        &RunOptions::human(),
    )
    .unwrap();
    let versions = s.versions().unwrap();
    assert_eq!(versions.len(), 2, "{versions:?}");
    assert_ne!(versions[1].id, info.id);
}

#[test]
fn undoing_to_a_state_only_on_the_disk_waits_for_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = Session::create(&dir, Project::new("writer")).unwrap();
    s.write_in_background(true);
    // More steps than are kept in memory.
    for i in 0..230 {
        add_track(&mut s, &format!("T{i}"));
    }
    s.simulate_slow_disk(Duration::from_millis(5));
    s.undo(229).unwrap();
    assert_eq!(names(&s), ["T0"]);
    s.redo(229).unwrap();
    assert_eq!(names(&s).len(), 230);
}

#[test]
fn opening_another_song_keeps_writing_in_the_background() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let other = tmp.path().join("other");
    Session::create(&other, Project::new("other")).unwrap();
    let mut s = background_song(&dir);
    add_track(&mut s, "A");
    s.run_value(
        "project.open",
        json!({ "path": other.to_string_lossy() }),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(s.writes_in_background());
    // The song left was written before the other was read.
    assert!(dir.join("history").join("1.json").is_file());

    let t = Instant::now();
    add_track(&mut s, "B");
    assert!(t.elapsed() < SLOW, "{:?}", t.elapsed());
    drop(s);
    assert_eq!(names(&Session::open(&other).unwrap()), ["B"]);
}
