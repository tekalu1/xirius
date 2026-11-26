//! AI-018: the AI writes over the song file only while it is the file the session knows. Before
//! it writes, a copy of the old file is kept in `backups/` and read back by its SHA-256; a file
//! changed elsewhere stops the save before anything is written.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::error::ErrorCode;
use xirius_core::session::{AiPermission, Confirm, RunOptions, RunResult, Session};
use xirius_core::store::sha256_hex;

fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-overwrite-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn human(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn ai(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::ai("claude"))
}

fn ai_ok(s: &mut Session, name: &str, args: Value) -> RunResult {
    ai(s, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A saved song the AI may save without asking.
fn song(dir: &Path) -> Session {
    let mut s = Session::new("ai");
    s.save_as(dir).unwrap();
    s.set_ai_permission(AiPermission::Save).unwrap();
    s
}

fn project(dir: &Path) -> Vec<u8> {
    fs::read(dir.join("project.json")).unwrap()
}

fn backups(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join("backups"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn the_ais_save_keeps_the_old_file_and_says_both_hashes() {
    let (_tmp, dir) = temp_dir("song");
    let mut s = song(&dir);
    let old = project(&dir);
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    let r = ai_ok(&mut s, "project.save", json!({}));
    assert!(!s.is_dirty());

    let backup = r.data["backup"].as_str().unwrap();
    assert!(backup.starts_with("backups/project-"), "{backup}");
    assert_eq!(
        fs::read(dir.join(backup)).unwrap(),
        old,
        "the copy is the old file"
    );
    assert_eq!(r.data["file_hash"]["before"], sha256_hex(&old));
    assert_eq!(r.data["file_hash"]["after"], sha256_hex(&project(&dir)));
    assert_ne!(r.data["file_hash"]["before"], r.data["file_hash"]["after"]);
    assert!(r.message.contains(backup), "{}", r.message);
}

#[test]
fn a_song_file_changed_elsewhere_stops_the_ais_save() {
    let (_tmp, dir) = temp_dir("song");
    let mut s = song(&dir);
    // Someone else's edit: the same song, one byte more.
    let mut theirs = project(&dir);
    theirs.push(b'\n');
    fs::write(dir.join("project.json"), &theirs).unwrap();

    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    let e = ai(&mut s, "project.save", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(e.message.contains("中止"), "{}", e.message);
    assert_eq!(project(&dir), theirs, "their file is left as it is");
    assert!(backups(&dir).is_empty(), "nothing was copied either");
    assert!(s.is_dirty(), "the AI's edit is still unsaved");

    // A person saving decides; after that the AI may save again.
    human(&mut s, "project.save", json!({}));
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    ai_ok(&mut s, "project.save", json!({}));
    assert_eq!(backups(&dir).len(), 1);
}

#[test]
fn a_save_the_person_answered_for_keeps_the_copy_too() {
    let (_tmp, dir) = temp_dir("song");
    let mut s = song(&dir);
    s.set_ai_permission(AiPermission::Edit).unwrap();
    s.set_confirm(Confirm::OPEN_04);
    let old = project(&dir);
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    let d = ai_ok(&mut s, "project.save", json!({}))
        .decision
        .expect("asks first");
    let r = human(
        &mut s,
        "ai.decide",
        json!({"token": d.token, "answer": "yes"}),
    );
    let backup = r.data["backup"].as_str().unwrap();
    assert_eq!(fs::read(dir.join(backup)).unwrap(), old);
    assert_eq!(r.data["file_hash"]["after"], sha256_hex(&project(&dir)));
}

#[test]
fn a_song_opened_from_its_folder_knows_its_file() {
    let (_tmp, dir) = temp_dir("song");
    drop(song(&dir));
    let mut s = Session::open(&dir).unwrap();
    s.set_ai_permission(AiPermission::Save).unwrap();
    ai_ok(&mut s, "track.add.audio", json!({"name": "A"}));
    ai_ok(&mut s, "project.save", json!({}));
    assert_eq!(backups(&dir).len(), 1);
    drop(s);

    // Changed while it was closed: the next session reads the new file, so that is no clash.
    let mut theirs = project(&dir);
    theirs.push(b'\n');
    fs::write(dir.join("project.json"), &theirs).unwrap();
    let mut s = Session::open(&dir).unwrap();
    ai_ok(&mut s, "track.add.audio", json!({"name": "B"}));
    let r = ai_ok(&mut s, "project.save", json!({}));
    let backup = r.data["backup"].as_str().unwrap();
    assert_eq!(fs::read(dir.join(backup)).unwrap(), theirs);
}

#[test]
fn a_persons_save_makes_no_copies() {
    let (_tmp, dir) = temp_dir("song");
    let mut s = song(&dir);
    human(&mut s, "track.add.audio", json!({"name": "A"}));
    human(&mut s, "project.save", json!({}));
    assert!(backups(&dir).is_empty());
}
