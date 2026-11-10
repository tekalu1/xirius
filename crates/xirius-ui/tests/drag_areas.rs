//! UI-337: every window of the app that the app draws its own bar on (`WindowDecorations::Client`)
//! must be movable by that bar on Windows, where `start_window_move` does nothing: it registers
//! the bar's empty parts as `WindowControlArea::Drag`, from the same functions as the press
//! hit test, or it is on the list of windows that are not meant to move.
//!
//! This reads the sources, so it runs on CI (the real-window test is `detached_window.rs`).

use std::path::{Path, PathBuf};

/// Files that open a client-decorated window that is not movable, and why.
const NOT_MOVABLE: &[(&str, &str)] = &[(
    "independent.rs",
    "the one entry of the independent windows: the views it opens (device_window.rs, checked      below) draw the bar and register the Drag areas; the Presets popup has no bar",
)];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn every_client_window_is_movable_or_listed() {
    let mut files = Vec::new();
    sources(&src(), &mut files);
    let mut opens = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let n = text.matches("WindowDecorations::Client").count();
        if n == 0 {
            continue;
        }
        opens += n;
        let name = f.file_name().unwrap().to_str().unwrap();
        let drags = text.contains("WindowControlArea::Drag")
            || text.contains("drag_rects")
            || text.contains("drag_areas");
        let listed = NOT_MOVABLE.iter().any(|(n, _)| *n == name);
        assert!(
            drags || listed,
            "{name} opens a client-decorated window with no Drag area: register the bar's empty \
             parts (`WindowControlArea::Drag`, from `drag_rects`) or list it in NOT_MOVABLE"
        );
    }
    assert!(
        opens > 0,
        "no window is opened by `WindowDecorations::Client`?"
    );
}

#[test]
fn the_independent_windows_register_their_drag_areas() {
    let entry = std::fs::read_to_string(src().join("independent.rs")).unwrap();
    assert!(
        entry.contains("WindowDecorations::Client") && entry.contains("fn open"),
        "independent.rs must be the entry that opens the client-decorated windows"
    );
    let text = std::fs::read_to_string(src().join("device_window.rs")).unwrap();
    assert!(
        text.contains("WindowControlArea::Drag") && text.contains("drag_rects"),
        "the built-in and channel editor windows must register `drag_rects` as Drag areas"
    );
    // A double click on the bar is taken, so the system does not maximise (UI-337).
    assert!(
        text.contains("click_count >= 2") && text.contains("stop_propagation"),
        "a double click on the bar must stop the press"
    );
}

#[test]
fn macos_still_uses_start_window_move() {
    let text = std::fs::read_to_string(src().join("device_window.rs")).unwrap();
    assert!(text.contains("start_window_move"));
}

#[test]
fn the_macro_controls_register_their_drag_areas_from_the_press_hit_test() {
    let pad = std::fs::read_to_string(src().join("macro_pad/mod.rs")).unwrap();
    // the Drag areas and the press are one test: `part_at` says "head" for both
    assert!(
        pad.contains("pub fn drag_rects") && pad.contains("part_at(v, x, y) == Some(Part::Head)"),
        "the Macro Controls' drag areas must come from the same `part_at` head test as the press"
    );
    let text = std::fs::read_to_string(src().join("device_window.rs")).unwrap();
    assert!(
        text.contains("macro_pad::drag_rects"),
        "the Macro Controls window must register `macro_pad::drag_rects` as Drag areas"
    );
    assert!(
        text.contains("macro_pad::Part::Head"),
        "a press on the Macro Controls' head starts the window move"
    );
}
