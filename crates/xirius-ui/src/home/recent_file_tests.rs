//! UI-350: one bad entry in `recent.json` must not hide the others. The entries are written by
//! hand here (a folder that is gone, one with nothing in it, a song that is cut short, entries
//! of the wrong shape), the way a file edited outside the app or left by a crash looks.

use super::tests::{labels, with_device};
use super::*;
use crate::song::Song;
use serde_json::json;
use std::time::Duration;

/// Runs `body` on a thread and fails when it does not come back: a worker that waits forever
/// must show as a failure, not as a test that never ends.
fn within<T: Send + 'static>(what: &str, body: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(body());
    });
    rx.recv_timeout(Duration::from_secs(20))
        .unwrap_or_else(|_| panic!("{what} did not come back in 20 s"))
}

fn good(path: &std::path::Path) {
    let mut seed = Song::empty();
    seed.save_as(path).unwrap();
}

/// The start screen over the keeper at `root`, after the worker has read `recent.json`.
fn shown(root: &std::path::Path) -> (Studio, Geometry) {
    let keep = crate::keep::Keep::at(root.join("keep"));
    let mut s = Studio::new(Song::empty());
    with_device(&mut s, true);
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home = Some(Home::default());
    s.home_tick();
    let g = s.geometry(1280.0, 720.0);
    (s, g)
}

fn entry(dir: &std::path::Path, name: &str, at: u64) -> serde_json::Value {
    json!({"dir": dir.to_string_lossy(), "name": name, "at_ms": at, "draft": false})
}

#[test]
fn one_bad_entry_leaves_the_other_rows_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("keep")).unwrap();
    let (a, z) = (root.join("a"), root.join("z"));
    good(&a);
    good(&z);
    let gone = root.join("gone");
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let cut = root.join("cut");
    std::fs::create_dir_all(&cut).unwrap();
    std::fs::write(cut.join("project.json"), "{\"name\":\"cut\",\"tra").unwrap();
    let a_file = root.join("a-file");
    std::fs::write(&a_file, "not a folder").unwrap();
    let now = xirius_core::store::now_ms();
    let songs = json!([
        entry(&a, "a", now - 5000),
        entry(&gone, "gone", now - 4000),
        entry(&empty, "empty", now - 3000),
        entry(&cut, "cut", now - 2000),
        entry(&a_file, "a-file", now - 1500),
        // the wrong shape: no name, a number for the folder, nothing at all, a far time
        {"dir": z.to_string_lossy()},
        {"dir": 7, "name": "seven"},
        null,
        {"dir": z.to_string_lossy(), "name": "", "at_ms": u64::MAX},
        entry(&z, "z", now - 1000),
    ]);
    std::fs::write(
        root.join("keep").join("recent.json"),
        json!({"songs": songs}).to_string(),
    )
    .unwrap();
    let r = root.clone();
    let rows = within("the list", move || {
        let (s, _g) = shown(&r);
        let rows = labels(&s);
        let again = {
            // the worker still answers
            s.keep.settle_recent();
            labels(&s)
        };
        assert_eq!(rows, again);
        rows
    });
    // Each entry is kept or left out on its own: the good ones are there whatever stands
    // beside them.
    assert!(rows.contains(&"a".to_string()), "{rows:?}");
    assert!(rows.contains(&"z".to_string()), "{rows:?}");
    assert!(!rows.contains(&"gone".to_string()), "{rows:?}");
    assert!(!rows.contains(&"empty".to_string()), "{rows:?}");
    assert!(
        rows.contains(&crate::song::SAMPLE_NAME.to_string()),
        "{rows:?}"
    );
}

#[test]
fn a_file_that_is_not_an_object_or_has_no_songs_lists_only_the_sample() {
    for text in [
        "",
        "[1,2]",
        "{\"songs\": 5}",
        "{\"songs\": {}}",
        "{",
        "\u{feff}{\"songs\":[]}",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("keep")).unwrap();
        std::fs::write(root.join("keep").join("recent.json"), text).unwrap();
        let r = root.clone();
        let rows = within("the list", move || labels(&shown(&r).0));
        assert_eq!(rows, [crate::song::SAMPLE_NAME], "{text:?}");
    }
}

/// Opening a song writes the list; a file that is not an object is written over, not stuck on.
#[test]
fn a_song_opened_over_a_broken_file_is_remembered_and_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("keep")).unwrap();
    std::fs::write(
        root.join("keep").join("recent.json"),
        "{\"songs\": [ {\"dir\": ",
    )
    .unwrap();
    let a = root.join("a");
    good(&a);
    let r = root.clone();
    let rows = within("remember", move || {
        let (s, _g) = shown(&r);
        s.keep
            .remember(&a, "a", false, xirius_core::store::now_ms());
        s.keep.settle_recent();
        s.keep
            .settled_recent()
            .into_iter()
            .map(|x| x.name)
            .collect::<Vec<_>>()
    });
    assert_eq!(rows, ["a"]);
}

/// A name written in another encoding (one byte that is not UTF-8) is one entry's trouble: the
/// others are listed, and the list can still be written.
#[test]
fn an_entry_with_bytes_that_are_not_utf8_leaves_the_other_rows_listed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("keep")).unwrap();
    let (a, b) = (root.join("a"), root.join("b"));
    good(&a);
    good(&b);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"{\"songs\": [");
    bytes.extend_from_slice(entry(&a, "a", 5000).to_string().as_bytes());
    bytes.extend_from_slice(b", {\"dir\": \"");
    let odd = serde_json::to_string(&root.join("odd").to_string_lossy()).unwrap();
    bytes.extend_from_slice(odd.trim_matches('"').as_bytes());
    // "a katakana letter" in Shift-JIS, cut off in the middle of its pair
    bytes.extend_from_slice(b"\", \"name\": \"Demo\x83\", \"at_ms\": 9000, \"draft\": false}, ");
    bytes.extend_from_slice(entry(&b, "b", 4000).to_string().as_bytes());
    bytes.extend_from_slice(b"]}");
    std::fs::write(root.join("keep").join("recent.json"), bytes).unwrap();
    let r = root.clone();
    let (rows, kept) = within("the list", move || {
        let (s, _g) = shown(&r);
        let rows = labels(&s);
        // Opening a song writes the list again.
        s.keep
            .remember(&a, "a", false, xirius_core::store::now_ms());
        s.keep.settle_recent();
        (rows, s.keep.settled_recent().len())
    });
    assert!(rows.contains(&"a".to_string()), "{rows:?}");
    assert!(rows.contains(&"b".to_string()), "{rows:?}");
    assert_eq!(kept, 2, "remembering a song keeps the others");
}
