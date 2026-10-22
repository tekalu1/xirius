//! BRW-003, BRW-004: saving gathers the audio files into the song's folder, as the song's
//! media settings say.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xirius_core::model::ClipBody;
use xirius_core::session::{RunOptions, Session};

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-collect-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn text(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The file every audio clip names, in track order.
fn files(s: &Session) -> Vec<String> {
    let p = s.project();
    let mut out = Vec::new();
    for t in p.tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file, .. } = &c.body {
                out.push(file.clone());
            }
        }
    }
    out.sort();
    out
}

fn place(s: &mut Session, file: &str) {
    run(
        s,
        "clip.import.place",
        json!({ "file": file, "length": 1920 }),
    );
}

/// A saved, empty song and a folder of outside files, with the guard of the folder they are in.
fn rig(tag: &str) -> (tempfile::TempDir, Session, PathBuf, PathBuf) {
    let (root_tmp, root) = temp_dir(tag);
    let song = root.join("song");
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let mut s = Session::new("collect");
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    (root_tmp, s, song, outside)
}

#[test]
fn saving_copies_an_outside_file_in_and_points_the_clip_at_it() {
    let (_tmp, mut s, song, outside) = rig("outside");
    let kick = outside.join("Kick.wav");
    fs::write(&kick, b"kick bytes").unwrap();
    place(&mut s, &text(&kick));
    let step = s.cursor();
    let r = s
        .run_value("project.save", json!({}), &RunOptions::human())
        .unwrap();
    assert!(r.message.contains("1 個"), "{}", r.message);
    assert_eq!(
        fs::read(song.join("Media/Imports/Kick.wav")).unwrap(),
        b"kick bytes"
    );
    assert_eq!(files(&s), ["Media/Imports/Kick.wav"]);
    assert_eq!(s.cursor(), step + 1, "one step of the history");
    assert!(!s.is_dirty());
    // Undo goes back to the outside file, which is still there.
    s.undo(1).unwrap();
    assert_eq!(files(&s), [text(&kick)]);
    // Saving again, with the file already gathered, makes no step.
    run(&mut s, "history.redo", json!({}));
    assert_eq!(s.cursor(), step + 1);
    run(&mut s, "project.save", json!({}));
    assert_eq!(s.cursor(), step + 1);
}

#[test]
fn a_file_two_clips_share_is_copied_once_and_both_follow() {
    let (_tmp, mut s, song, outside) = rig("shared");
    let hit = outside.join("Hit.wav");
    fs::write(&hit, b"hit").unwrap();
    place(&mut s, &text(&hit));
    place(&mut s, &text(&hit));
    run(&mut s, "project.save", json!({}));
    assert_eq!(
        files(&s),
        ["Media/Imports/Hit.wav", "Media/Imports/Hit.wav"]
    );
    assert_eq!(fs::read_dir(song.join("Media/Imports")).unwrap().count(), 1);
}

#[test]
fn files_with_one_name_but_other_bytes_each_get_a_place() {
    let (_tmp, mut s, song, outside) = rig("names");
    let a = outside.join("a");
    let b = outside.join("b");
    fs::create_dir_all(&a).unwrap();
    fs::create_dir_all(&b).unwrap();
    fs::write(a.join("Loop.wav"), b"first").unwrap();
    fs::write(b.join("Loop.wav"), b"second").unwrap();
    place(&mut s, &text(&a.join("Loop.wav")));
    place(&mut s, &text(&b.join("Loop.wav")));
    run(&mut s, "project.save", json!({}));
    assert_eq!(
        files(&s),
        ["Media/Imports/Loop 2.wav", "Media/Imports/Loop.wav"]
    );
    assert_eq!(
        fs::read(song.join("Media/Imports/Loop.wav")).unwrap(),
        b"first"
    );
    assert_eq!(
        fs::read(song.join("Media/Imports/Loop 2.wav")).unwrap(),
        b"second"
    );
}

#[test]
fn a_file_in_the_folder_is_named_relative_and_one_that_is_gone_stays_and_is_said() {
    let (_tmp, mut s, song, _) = rig("inside");
    fs::create_dir_all(song.join("Recordings")).unwrap();
    let take = song.join("Recordings/take1.wav");
    fs::write(&take, b"take").unwrap();
    place(&mut s, &text(&fs::canonicalize(&take).unwrap()));
    place(&mut s, "C:/Gone/Nothing.wav");
    let r = s
        .run_value("project.save", json!({}), &RunOptions::human())
        .unwrap();
    assert!(r.message.contains("見つからない"), "{}", r.message);
    assert!(r.message.contains("C:/Gone/Nothing.wav"), "{}", r.message);
    assert_eq!(files(&s), ["C:/Gone/Nothing.wav", "Recordings/take1.wav"]);
    assert!(!song.join("Media/Imports").exists());
}

#[test]
fn saving_as_elsewhere_takes_the_recordings_along() {
    let (_tmp, mut s, song, outside) = rig("save-as");
    fs::create_dir_all(song.join("Recordings")).unwrap();
    fs::write(song.join("Recordings/take1.wav"), b"take").unwrap();
    place(&mut s, "Recordings/take1.wav");
    let ext = outside.join("Pad.wav");
    fs::write(&ext, b"pad").unwrap();
    place(&mut s, &text(&ext));
    let other = song.parent().unwrap().join("copy");
    let r = s
        .run_value(
            "project.save-as",
            json!({ "path": text(&other) }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(r.message.contains("集めました"), "{}", r.message);
    assert_eq!(
        fs::read(other.join("Recordings/take1.wav")).unwrap(),
        b"take"
    );
    assert_eq!(
        fs::read(other.join("Media/Imports/Pad.wav")).unwrap(),
        b"pad"
    );
    assert_eq!(files(&s), ["Media/Imports/Pad.wav", "Recordings/take1.wav"]);
    // The copy opens on its own, whatever happens to the first folder.
    drop(s);
    fs::remove_dir_all(&song).unwrap();
    let reopened = Session::open(&other).unwrap();
    assert_eq!(
        files(&reopened),
        ["Media/Imports/Pad.wav", "Recordings/take1.wav"]
    );
}

#[test]
fn a_song_that_copies_nothing_keeps_its_outside_files_where_they_are() {
    let (_tmp, mut s, song, outside) = rig("no-copy");
    run(&mut s, "project.settings", json!({ "copy_imports": false }));
    let kick = outside.join("Kick.wav");
    fs::write(&kick, b"kick").unwrap();
    place(&mut s, &text(&kick));
    let step = s.cursor();
    let r = s
        .run_value("project.save", json!({}), &RunOptions::human())
        .unwrap();
    assert!(!r.message.contains("集めました"), "{}", r.message);
    assert_eq!(files(&s), [text(&kick)]);
    assert_eq!(s.cursor(), step, "no step of the history");
    assert!(!song.join("Media").exists());
    // Turned on again, the next save gathers it.
    run(&mut s, "project.settings", json!({ "copy_imports": true }));
    run(&mut s, "project.save", json!({}));
    assert_eq!(files(&s), ["Media/Imports/Kick.wav"]);
}

#[test]
fn a_media_folder_outside_the_song_holds_the_copies_and_its_files_stay_put() {
    let (_tmp, mut s, song, outside) = rig("outside-media");
    let media = song.parent().unwrap().join("shared");
    run(
        &mut s,
        "project.settings",
        json!({ "media_folder": text(&media) }),
    );
    // A file already in the media folder is the song's: saving leaves it alone.
    fs::create_dir_all(media.join("Recordings")).unwrap();
    let take = media.join("Recordings/take.wav");
    fs::write(&take, b"take").unwrap();
    place(&mut s, &text(&take));
    // A file outside both is copied into the media folder's Imports.
    let pad = outside.join("Pad.wav");
    fs::write(&pad, b"pad").unwrap();
    place(&mut s, &text(&pad));
    run(&mut s, "project.save", json!({}));
    assert_eq!(fs::read(media.join("Imports/Pad.wav")).unwrap(), b"pad");
    let got = files(&s);
    assert!(got.contains(&text(&take)), "{got:?}");
    assert!(
        got.iter()
            .any(|f| f.replace('\\', "/").ends_with("shared/Imports/Pad.wav")),
        "{got:?}"
    );
    assert!(!song.join("Media").exists());
}

#[test]
fn the_media_settings_are_one_step_and_saved_with_the_song() {
    let (_tmp, mut s, song, _) = rig("settings");
    let step = s.cursor();
    let d = run(
        &mut s,
        "project.settings",
        json!({ "media_folder": "Files\\Takes/", "copy_imports": "false" }),
    );
    assert_eq!(d["media_folder"], "Files/Takes");
    assert_eq!(d["copy_imports"], false);
    assert_eq!(s.cursor(), step + 1);
    run(&mut s, "project.save", json!({}));
    let reopened = Session::open(&song).unwrap();
    assert_eq!(reopened.project().media.folder, "Files/Takes");
    assert!(!reopened.project().media.copy_imports);
    s.undo(1).unwrap();
    assert!(s.project().media.is_default());
    let bad = s.run_value(
        "project.settings",
        json!({ "media_folder": "  " }),
        &RunOptions::human(),
    );
    assert!(bad.is_err());
}
