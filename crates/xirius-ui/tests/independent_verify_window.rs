//! UI-337: the independent windows, in the real window. The app runs `--script device-verify`
//! (`src/device_window/verify_shots.rs`), which asks the windows with plain window messages and
//! `PrintWindow` (no OS input, nothing in front), and writes `verify.json`. This reads it.
//!
//! - the Channel Editor, the Macro Controls and a built-in instrument's window are owned by the
//!   main window and stay in front of it; they go with it when it is minimised and come back;
//! - the empty bar is the caption and the buttons and content are the client; a double click on
//!   the bar does not maximise;
//! - the Channel Editor opens at its content's size and bar; the least width is 520 without
//!   layers and 720 with; the Macro Controls cannot be resized;
//! - a window that is closed and opened again comes back where it was, at the size it had.
//!
//! The last test is a normal launch (not parked): the windows show on the screen briefly.
//!
//! This opens real windows: it is not part of `scripts/ci_tests.py`'s runs (its name ends in
//! `_window`).

use serde_json::Value;
use std::process::Command;

mod support;

fn run() -> (Value, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-ui-verify-window-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    support::sample_audio(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent", "--script", "device-verify", "--shot"])
        .arg(&dir)
        .current_dir(&dir)
        .env("XIRIUS_EDITOR_OFFSCREEN", "1")
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .expect("the app runs");
    let err = String::from_utf8_lossy(&out.stderr);
    if let Ok(keep) = std::env::var("XIRIUS_KEEP_VERIFY") {
        // The report and the pictures, for a look.
        let _ = std::fs::create_dir_all(&keep);
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".bmp") || name.starts_with("verify.") {
                let _ = std::fs::copy(entry.path(), std::path::Path::new(&keep).join(&*name));
            }
        }
    }
    assert!(out.status.success(), "the run failed: {err}");
    assert!(!err.contains("panicked"), "{err}");
    let text = std::fs::read_to_string(dir.join("verify.json")).expect("the probe wrote it");
    (serde_json::from_str(&text).unwrap(), tmp)
}

/// A normal launch (not parked): the windows are on the screen for a few seconds.
fn run_placed() -> (Value, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-ui-place-window-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    support::sample_audio(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args([
            "--at",
            "100,100",
            "--silent",
            "--script",
            "device-place",
            "--shot",
        ])
        .arg(&dir)
        .current_dir(&dir)
        .env("XIRIUS_CRASH_DIR", dir.join("crashes"))
        .output()
        .expect("the app runs");
    let err = String::from_utf8_lossy(&out.stderr);
    if let Ok(keep) = std::env::var("XIRIUS_KEEP_VERIFY") {
        let _ = std::fs::create_dir_all(&keep);
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".bmp") || name.starts_with("verify.") {
                let _ = std::fs::copy(entry.path(), std::path::Path::new(&keep).join(&*name));
            }
        }
    }
    assert!(out.status.success(), "the run failed: {err}");
    let text = std::fs::read_to_string(dir.join("verify.json")).expect("the probe wrote it");
    (serde_json::from_str(&text).unwrap(), tmp)
}

fn rows<'a>(report: &'a Value, key: &str) -> &'a [Value] {
    report[key].as_array().map_or(&[], |v| v.as_slice())
}

#[test]
fn the_windows_are_owned_by_the_main_window_and_go_with_it() {
    let (report, _tmp) = run();
    let owners = rows(&report, "owner");
    assert_eq!(owners.len(), 3, "{report}");
    for o in owners {
        assert_eq!(o["owner_is_main"], true, "{o}");
    }
    assert_eq!(report["z_before"]["all_above_main"], true, "{report}");
    assert_eq!(report["z_after_raise"]["all_above_main"], true, "{report}");
    let m = &report["minimise"];
    assert_eq!(m["hidden_with_main"], true, "{m}");
    assert_eq!(m["restored_with_main"], true, "{m}");
}

#[test]
fn the_bar_is_the_caption_and_the_rest_is_the_client() {
    let (report, _tmp) = run();
    let hits = rows(&report, "hits");
    for window in ["built-in instrument", "channel editor", "macro controls"] {
        let mine: Vec<&Value> = hits.iter().filter(|h| h["window"] == window).collect();
        assert!(mine.len() >= 4, "{window} was asked too little: {mine:?}");
        let bad: Vec<String> = mine
            .iter()
            .filter(|h| h["ok"] != true)
            .map(|h| format!("{}: {}", h["label"], h["answer"]))
            .collect();
        assert!(bad.is_empty(), "{window}: {}", bad.join(", "));
    }
    for p in rows(&report, "press") {
        assert_eq!(p["first_goes_on"], true, "{p}");
        assert_eq!(p["second_goes_on"], false, "{p}");
    }
    for d in rows(&report, "double_click") {
        assert_eq!(d["maximised"], false, "{d}");
        assert_eq!(d["size_same"], true, "{d}");
    }
}

#[test]
fn a_closed_window_opens_again_where_and_as_big_as_it_was() {
    let (report, _tmp) = run();
    let again = rows(&report, "reopen");
    assert_eq!(again.len(), 3, "{report}");
    for r in again {
        assert_eq!(r["reopened"], true, "{r}");
        assert_eq!(r["same_place"], true, "{r}");
        assert_eq!(r["same_size"], true, "{r}");
        assert_eq!(r["owner_is_main"], true, "{r}");
    }
}

#[test]
fn the_channel_editor_opens_at_its_content_and_never_below_its_least_width() {
    let (report, _tmp) = run();
    let o = &report["opening"];
    let (w, h) = (
        o["ce_client_logical"][0].as_f64().unwrap(),
        o["ce_client_logical"][1].as_f64().unwrap(),
    );
    let (nw, nh) = (
        o["natural_with_bar"][0].as_f64().unwrap(),
        o["natural_with_bar"][1].as_f64().unwrap(),
    );
    assert!((w - nw).abs() <= 1.0 && (h - nh).abs() <= 1.0, "{o}");
    for l in rows(&report, "least_width") {
        let got = l["got_width_logical"].as_f64().unwrap();
        assert!(got >= l["least_logical"].as_f64().unwrap() - 1.0, "{l}");
    }
    let multi = rows(&report, "multi");
    assert!(!multi.is_empty(), "{report}");
    for m in multi {
        assert!(m["got_logical"][0].as_f64().unwrap() >= 719.0, "{m}");
        assert_eq!(m["keys"]["gap"].as_f64(), Some(4.0), "{m}");
    }
    let macros = rows(&report, "sizes")
        .iter()
        .find(|s| s["window"] == "macro controls")
        .unwrap();
    assert_eq!(macros["thick_frame"], false, "{macros}");
}

#[test]
fn in_a_normal_launch_the_windows_open_where_and_as_big_as_asked_and_come_back() {
    let (report, _tmp) = run_placed();
    let opening = rows(&report, "opening");
    assert_eq!(opening.len(), 3, "{report}");
    for o in opening {
        assert_eq!(o["size_ok"], true, "{o}");
        assert_eq!(o["origin_ok"], true, "{o}");
    }
    let moved = rows(&report, "moved");
    assert_eq!(moved.len(), 3, "{report}");
    for m in moved {
        assert_eq!(m["reopened"], true, "{m}");
        assert_eq!(m["same_place"], true, "{m}");
        assert_eq!(m["same_size"], true, "{m}");
    }
}
