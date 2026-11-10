//! UI-337: an independent window (a built-in device's, the channel editor's) moves by the empty
//! part of its bar, in the real window. The app runs `--script device-drag`
//! (`src/device_window/drag_shots.rs`), which asks each window `WM_NCHITTEST` with plain window
//! messages (no OS input, nothing in front), and writes `drag.json`. This reads it.
//!
//! - the empty bar answers `HTCAPTION`; each bar button and the content `HTCLIENT`; the four
//!   edges are the resize edges;
//! - a press on the empty bar goes on the first time and is taken the second (the double
//!   click), and `WM_NCLBUTTONDBLCLK` there does not maximise;
//! - a move and a size given by the system stay.

use serde_json::Value;
use std::process::Command;

mod support;

fn run() -> (Value, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-ui-detached-window-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    support::sample_audio(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "device-drag", "--shot"])
        .arg(&dir)
        .current_dir(&dir)
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .expect("the app runs");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the run failed: {err}");
    assert!(!err.contains("panicked"), "{err}");
    let text = std::fs::read_to_string(dir.join("drag.json")).expect("the probe wrote drag.json");
    if let Ok(keep) = std::env::var("XIRIUS_KEEP_DRAG") {
        let _ = std::fs::write(&keep, &text);
    }
    (serde_json::from_str(&text).unwrap(), tmp)
}

fn rows<'a>(report: &'a Value, key: &str) -> &'a [Value] {
    report[key].as_array().map_or(&[], |v| v.as_slice())
}

#[test]
fn the_bar_moves_the_window_and_the_rest_is_as_before() {
    let (report, _tmp) = run();
    let hits = rows(&report, "hits");
    for window in ["built-in instrument", "channel editor"] {
        let mine: Vec<&Value> = hits.iter().filter(|h| h["window"] == window).collect();
        assert!(mine.len() >= 10, "{window} was asked too little: {mine:?}");
        for label in ["bar, empty stretch", "bar, name", "content", "top edge"] {
            assert!(
                mine.iter().any(|h| h["label"] == label),
                "{window} was not asked {label}"
            );
        }
        let bad: Vec<String> = mine
            .iter()
            .filter(|h| h["ok"] != true)
            .map(|h| {
                format!(
                    "{}: {} expected {} answered {}",
                    h["window"], h["label"], h["expect"], h["answer"]
                )
            })
            .collect();
        assert!(
            bad.is_empty(),
            "wrong hit test answers:\n{}",
            bad.join("\n")
        );
    }
}

#[test]
fn a_double_click_on_the_empty_bar_does_not_maximise() {
    let (report, _tmp) = run();
    let press = rows(&report, "press");
    assert_eq!(press.len(), 2, "{report}");
    for p in press {
        assert_eq!(p["first_goes_on"], true, "the first press must go on: {p}");
        assert_eq!(p["second_goes_on"], false, "the double click is taken: {p}");
    }
    let double = rows(&report, "double_click");
    assert_eq!(double.len(), 2, "{report}");
    for d in double {
        assert_eq!(d["maximised"], false, "maximised by a double click: {d}");
        assert_eq!(d["size_same"], true, "the size changed: {d}");
    }
}

#[test]
fn the_system_can_move_and_size_the_window() {
    let (report, _tmp) = run();
    for m in rows(&report, "move") {
        assert_eq!(m["moved"], true, "{m}");
        assert_eq!(m["size_same"], true, "{m}");
    }
    for r in rows(&report, "resize") {
        assert_eq!(r["sticks"], true, "the frame set the size back: {r}");
    }
}
