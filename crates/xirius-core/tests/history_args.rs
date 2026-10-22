//! A step keeps a long text of its arguments as its length only: the state the step made
//! holds it, and the index written at every edit stays small.

use std::path::Path;

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::session::{ARG_KEPT_BYTES, RunOptions};
use xirius_core::{Project, Session};

/// A plugin's state as large as a sampler's (base64, 300 KB).
fn big_state() -> String {
    "AAEC".repeat(75_000)
}

fn index_bytes(dir: &Path) -> u64 {
    std::fs::metadata(dir.join("history").join("index.json"))
        .unwrap()
        .len()
}

/// A song with one plugin whose state was set to [`big_state`].
fn song(dir: &Path) -> (Session, TrackId) {
    let mut s = Session::create(dir, Project::new("big state")).unwrap();
    let h = RunOptions::human();
    let track: TrackId = s
        .run_value("track.add.audio", json!({"name": "Audio"}), &h)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let plugin = s
        .run_value(
            "plugin.add",
            json!({"track": track.to_string(), "name": "Sampler", "format": "vst3", "uid": "vendor/sampler"}),
            &h,
        )
        .unwrap()
        .data["id"]
        .clone();
    s.run_value(
        "plugin.state.set",
        json!({"id": plugin, "state": big_state()}),
        &h,
    )
    .unwrap();
    (s, track)
}

fn state_of(s: &Session, track: TrackId) -> Option<String> {
    s.project().tracks[&track].inserts[0].state.clone()
}

#[test]
fn a_long_argument_stays_out_of_the_index_and_the_state_keeps_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    assert_eq!(state_of(&s, track), Some(big_state()));

    let row = s.history().pop().unwrap();
    assert_eq!(row.command, "plugin.state.set");
    let kept = row.args["state"].as_str().unwrap();
    assert!(kept.len() < ARG_KEPT_BYTES, "{kept}");
    assert!(kept.contains(&big_state().len().to_string()), "{kept}");
    assert!(index_bytes(&dir) < 16 * 1024, "{}", index_bytes(&dir));

    // Back and forth over the step, the state comes from the step's own file.
    s.undo(1).unwrap();
    assert_eq!(state_of(&s, track), None);
    s.redo(1).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
    drop(s);

    let mut s = Session::open(&dir).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
    s.undo(1).unwrap();
    s.redo(1).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
}

#[test]
fn a_song_whose_index_carries_a_long_argument_writes_it_short_at_the_next_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (s, track) = song(&dir);
    drop(s);

    // As a song written before: the argument in full.
    let path = dir.join("history").join("index.json");
    let mut index: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let step = index["steps"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|st| st["command"] == "plugin.state.set")
        .unwrap();
    step["args"]["state"] = json!(big_state());
    std::fs::write(&path, serde_json::to_vec(&index).unwrap()).unwrap();
    assert!(index_bytes(&dir) > big_state().len() as u64);

    let mut s = Session::open(&dir).unwrap();
    let row = s.history().pop().unwrap();
    assert!(row.args["state"].as_str().unwrap().len() < ARG_KEPT_BYTES);
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "Renamed"}),
        &RunOptions::human(),
    )
    .unwrap();
    assert!(index_bytes(&dir) < 16 * 1024, "{}", index_bytes(&dir));
    s.undo(1).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
}

#[test]
fn a_short_argument_is_kept_as_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut s = Session::create(&dir, Project::new("short")).unwrap();
    s.run_value(
        "track.add.audio",
        json!({"name": "Vocal"}),
        &RunOptions::human(),
    )
    .unwrap();
    let row = s.history().pop().unwrap();
    assert_eq!(row.args["name"], "Vocal");
}

#[test]
fn long_strings_in_history_states_are_kept_once_in_blobs_and_stay_small() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    let h = RunOptions::human();
    for i in 0..5 {
        s.run_value(
            "track.rename",
            json!({"id": track.to_string(), "name": format!("Audio {i}")}),
            &h,
        )
        .unwrap();
    }

    let hash = xirius_core::store::sha256_hex(big_state().as_bytes());
    let blob_path = dir.join("history").join("blobs").join(&hash);
    assert!(blob_path.is_file());
    assert_eq!(std::fs::read_to_string(&blob_path).unwrap(), big_state());

    for step in 3..=8 {
        let state_path = dir.join("history").join(format!("{step}.json"));
        let raw = std::fs::read(&state_path).unwrap();
        assert!(
            raw.len() < 8 * 1024,
            "step {step} should reference blob, got {} bytes",
            raw.len()
        );
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            v["format_version"],
            xirius_core::store::HISTORY_FORMAT_VERSION
        );
    }

    // Save, create version, and save-as all work with Shape B.
    s.save().unwrap();
    let ver = s.save_version("v1").unwrap();
    let copy_dir = tmp.path().join("song_copy");
    s.save_as(&copy_dir).unwrap();
    assert!(copy_dir.join("history").join("blobs").join(&hash).is_file());
    let copy_step = std::fs::metadata(copy_dir.join("history").join("8.json"))
        .unwrap()
        .len();
    assert!(copy_step < 8 * 1024, "{copy_step}");

    // Undo past the plugin state, then restore the version.
    s.jump(2).unwrap();
    assert_eq!(state_of(&s, track), None);
    s.restore_version(&ver.id, &xirius_core::command::Author::Human)
        .unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));

    // Simulate a crash after an unsaved edit and verify recovery with Shape B.
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "After Crash"}),
        &h,
    )
    .unwrap();
    s.simulate_crash();

    let recovered = Session::open(&copy_dir).unwrap();
    assert!(recovered.recovery().is_some());
    assert_eq!(recovered.project().tracks[&track].name, "After Crash");
    assert_eq!(state_of(&recovered, track), Some(big_state()));
}

#[test]
fn v1_history_states_are_read_without_rewriting_and_new_steps_write_v2() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    s.save().unwrap();
    let p3 = s.project().clone();
    drop(s);

    // Rewrite history/3.json as format_version 1 with inline state, and remove history/blobs.
    let v1_bytes = serde_json::to_vec(&json!({
        "format_version": 1,
        "project": p3,
    }))
    .unwrap();
    let step3_path = dir.join("history").join("3.json");
    std::fs::write(&step3_path, &v1_bytes).unwrap();
    std::fs::remove_dir_all(dir.join("history").join("blobs")).unwrap();

    // Opening reads v1 without rewriting it or creating history/blobs.
    let mut s = Session::open(&dir).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
    assert!(!dir.join("history").join("blobs").exists());
    let step3_after_open: Value =
        serde_json::from_slice(&std::fs::read(&step3_path).unwrap()).unwrap();
    assert_eq!(step3_after_open["format_version"], 1);

    // A new step writes format_version 2 with blobs, leaving step 3 untouched as v1.
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "Step 4"}),
        &RunOptions::human(),
    )
    .unwrap();
    let step3_after_edit: Value =
        serde_json::from_slice(&std::fs::read(&step3_path).unwrap()).unwrap();
    assert_eq!(step3_after_edit["format_version"], 1);

    let step4_path = dir.join("history").join("4.json");
    let step4_raw = std::fs::read(&step4_path).unwrap();
    assert!(step4_raw.len() < 8 * 1024);
    let step4_val: Value = serde_json::from_slice(&step4_raw).unwrap();
    assert_eq!(
        step4_val["format_version"],
        xirius_core::store::HISTORY_FORMAT_VERSION
    );
    drop(s);

    // Reopen (so step 3 is not cached in memory), undo to v1 step 3, redo to v2 step 4.
    let mut s = Session::open(&dir).unwrap();
    s.undo(1).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
    s.redo(1).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
}

#[test]
fn missing_or_corrupt_blob_falls_back_to_saved_project_and_old_reader_keeps_project_v1() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    s.save().unwrap();
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "Unsaved"}),
        &RunOptions::human(),
    )
    .unwrap();
    drop(s);

    // project.json and manifest.toml remain format_version 1 with full inline state,
    // so an older Xirius binary can always read the saved song.
    let proj_json: Value =
        serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap()).unwrap();
    assert_eq!(proj_json["format_version"], 1);
    assert_eq!(
        proj_json["project"]["tracks"][track.to_string()]["inserts"][0]["state"]
            .as_str()
            .unwrap()
            .len(),
        big_state().len()
    );

    // If the blob is deleted or corrupted, Store::read_state reports Corrupt,
    // and Session::open falls back to project.json with a notice.
    let hash = xirius_core::store::sha256_hex(big_state().as_bytes());
    let blob_path = dir.join("history").join("blobs").join(&hash);
    std::fs::write(&blob_path, b"corrupted").unwrap();

    let store = xirius_core::store::Store::new(&dir);
    let err = store.read_state(4).unwrap_err();
    assert_eq!(err.code, xirius_core::ErrorCode::Corrupt);

    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().tracks[&track].name, "Audio");
    assert_eq!(state_of(&s, track), Some(big_state()));
    assert!(
        s.notices
            .iter()
            .any(|n| n.contains("履歴を読み込めなかったため"))
    );
}

#[test]
fn saving_prunes_unreferenced_blobs_and_stray_states() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    let hash_a = xirius_core::store::sha256_hex(big_state().as_bytes());
    let plugin_id = s.project().tracks[&track].inserts[0].id.to_string();

    // Undo step 3 (which introduced hash_a) and replace it with a different large state.
    s.undo(1).unwrap();
    let other_state = "BBFD".repeat(75_000);
    let hash_b = xirius_core::store::sha256_hex(other_state.as_bytes());
    s.run_value(
        "plugin.state.set",
        json!({"id": plugin_id, "state": other_state}),
        &RunOptions::human(),
    )
    .unwrap();

    let blobs_dir = dir.join("history").join("blobs");
    assert!(blobs_dir.join(&hash_a).is_file());
    assert!(blobs_dir.join(&hash_b).is_file());

    // Saving prunes hash_a because no kept step references it anymore.
    s.save().unwrap();
    assert!(!blobs_dir.join(&hash_a).exists());
    assert!(blobs_dir.join(&hash_b).is_file());
    drop(s);

    // A crash in the middle of a step leaves a state the index does not name and its blob.
    // Opening only reads; the next save clears them.
    let orphan = blobs_dir.join("0123456789abcdef");
    std::fs::write(&orphan, b"orphan").unwrap();
    let stray_state = dir.join("history").join("999.json");
    std::fs::write(&stray_state, b"{}").unwrap();

    let mut s = Session::open(&dir).unwrap();
    assert!(orphan.exists());
    assert!(stray_state.exists());
    s.save().unwrap();
    assert!(!orphan.exists());
    assert!(!stray_state.exists());
    assert!(blobs_dir.join(&hash_b).is_file());
    assert_eq!(state_of(&s, track).as_deref(), Some(other_state.as_str()));
}

#[test]
fn a_deleted_blob_fails_the_undo_that_needs_it_and_the_next_edit_writes_it_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    let h = RunOptions::human();
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "Step 4"}),
        &h,
    )
    .unwrap();
    s.save().unwrap();
    drop(s);
    let hash = xirius_core::store::sha256_hex(big_state().as_bytes());
    let blob = dir.join("history").join("blobs").join(&hash);

    // Opened again, only the last state is in memory. Undoing to state 3 reads it from disk,
    // and with its blob gone that fails and leaves the song as it was.
    let mut s = Session::open(&dir).unwrap();
    std::fs::remove_file(&blob).unwrap();
    let err = s.undo(1).unwrap_err();
    assert_eq!(err.code, xirius_core::ErrorCode::Corrupt);
    assert_eq!(s.cursor(), 4);
    assert_eq!(s.project().tracks[&track].name, "Step 4");

    // The next step points to the same text, so it writes the blob again, and the history
    // reads back whole.
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "Step 5"}),
        &h,
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&blob).unwrap(), big_state());
    drop(s);
    let mut s = Session::open(&dir).unwrap();
    assert!(s.notices.is_empty(), "{:?}", s.notices);
    s.undo(2).unwrap();
    assert_eq!(state_of(&s, track), Some(big_state()));
    drop(s);

    // The last state's blob gone: the song opens from project.json and says so.
    std::fs::remove_file(&blob).unwrap();
    let s = Session::open(&dir).unwrap();
    assert!(
        s.notices
            .iter()
            .any(|n| n.contains("履歴を読み込めなかったため"))
    );
    assert_eq!(s.project().tracks[&track].name, "Step 4");
    assert_eq!(state_of(&s, track), Some(big_state()));
}

#[test]
fn a_parameter_named_like_a_blob_reference_is_kept_as_a_parameter() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    let plugin = s.project().tracks[&track].inserts[0].id.to_string();
    s.run_value(
        "plugin.param.set",
        json!({"id": plugin, "param": "$blob", "value": 0.5}),
        &RunOptions::human(),
    )
    .unwrap();
    drop(s);

    let s = Session::open(&dir).unwrap();
    assert!(s.notices.is_empty(), "{:?}", s.notices);
    assert_eq!(s.project().tracks[&track].inserts[0].params["$blob"], 0.5);
    assert_eq!(state_of(&s, track), Some(big_state()));
}

#[test]
fn a_history_started_afresh_on_open_writes_its_first_state_before_the_index_names_it() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    s.save().unwrap();
    drop(s);
    // The history cannot be read: the next open starts one from the saved song.
    let hash = xirius_core::store::sha256_hex(big_state().as_bytes());
    std::fs::write(dir.join("history").join("blobs").join(&hash), b"corrupted").unwrap();
    let mut s = Session::open(&dir).unwrap();
    assert!(!s.can_undo());
    s.run_value(
        "track.rename",
        json!({"id": track.to_string(), "name": "After"}),
        &RunOptions::human(),
    )
    .unwrap();
    drop(s);

    // Going back reaches the saved song, not the state 0 the unreadable history had.
    let mut s = Session::open(&dir).unwrap();
    assert!(s.notices.is_empty(), "{:?}", s.notices);
    s.undo(1).unwrap();
    assert_eq!(s.project().tracks[&track].name, "Audio");
    assert_eq!(state_of(&s, track), Some(big_state()));
    assert!(!s.can_undo());
}

/// What an older Xirius (history format 1) leaves when it cannot read this history: its own
/// history from the saved song, whose start it does not write, so state 0 stays this one's.
fn written_over_by_an_older_xirius(dir: &Path, cursor: u64) {
    let history = dir.join("history");
    let mut saved: Value =
        serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap()).unwrap();
    for t in saved["project"]["tracks"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        if t["name"] == "Audio" {
            t["name"] = json!("Renamed by the older one");
        }
    }
    std::fs::write(history.join("1.json"), serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut index: Value =
        serde_json::from_slice(&std::fs::read(history.join("index.json")).unwrap()).unwrap();
    let mut step = index["steps"][0].clone();
    step["n"] = json!(1);
    step["command"] = json!("track.rename");
    index["first"] = json!(0);
    index["cursor"] = json!(cursor);
    index["steps"] = json!([step]);
    std::fs::write(
        history.join("index.json"),
        serde_json::to_vec(&index).unwrap(),
    )
    .unwrap();
}

#[test]
fn the_start_an_older_xirius_did_not_write_is_not_gone_back_to() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    s.save().unwrap();
    drop(s);
    written_over_by_an_older_xirius(&dir, 1);

    let mut s = Session::open(&dir).unwrap();
    assert_eq!(s.project().tracks[&track].name, "Renamed by the older one");
    // State 0 is the empty song this history began with, not the one renamed.
    assert!(!s.can_undo());
    assert!(s.undo(1).is_err());
    // The next save lets the old start go.
    s.save().unwrap();
    assert!(!dir.join("history").join("0.json").exists());
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert!(s.notices.is_empty(), "{:?}", s.notices);
    assert_eq!(s.project().tracks[&track].name, "Renamed by the older one");
}

#[test]
fn an_older_xirius_gone_back_to_its_start_opens_the_saved_song() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let (mut s, track) = song(&dir);
    s.save().unwrap();
    drop(s);
    written_over_by_an_older_xirius(&dir, 0);

    // Its cursor names the state it did not write: the saved song, as it had.
    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().tracks[&track].name, "Audio");
    assert_eq!(state_of(&s, track), Some(big_state()));
    assert!(
        s.notices
            .iter()
            .any(|n| n.contains("履歴を読み込めなかったため"))
    );
}
