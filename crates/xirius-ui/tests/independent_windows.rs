//! UI-337: the rules of the independent windows, read from the sources (so they run on CI).
//!
//! - one entry opens a window (`independent.rs`); `resize` is asked in few places;
//! - the old "own place and pin" fields do not spread to more structs;
//! - what `draw::paint_overlay` paints is a list of modals, popovers, transient shows and
//!   screen sections (there is no "window" kind: a window is a real window).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// (path under src with `/`, text without the comment lines)
fn all() -> Vec<(String, String)> {
    let mut files = Vec::new();
    sources(&src(), &mut files);
    files
        .into_iter()
        .map(|f| {
            let rel = f
                .strip_prefix(src())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&f).unwrap();
            let code = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            (rel, code)
        })
        .collect()
}

/// Files that may call `cx.open_window`: the main window, the measuring Workspace window and
/// the entry of the independent windows.
const OPEN_WINDOW: &[&str] = &["screen.rs", "app.rs", "independent.rs"];

#[test]
fn only_the_entries_open_a_window() {
    for (name, code) in all() {
        if code.contains(".open_window(") {
            assert!(
                OPEN_WINDOW.contains(&name.as_str()),
                "{name} calls `open_window`: an independent window is opened by \
                 `independent::open` (screen.rs is the main window, app.rs the measuring window)"
            );
        }
    }
}

/// Files that may call `window.resize(`, and why.
const RESIZE: &[(&str, &str)] = &[
    (
        "device_window.rs",
        "the views of the independent windows ask their size",
    ),
    ("independent.rs", "the entry"),
    (
        "device_window/input_shots.rs",
        "test shots of a device window",
    ),
    ("drawer_shots.rs", "test shots: the size of the main window"),
    ("polish_shots.rs", "test shots: the size of the main window"),
];

#[test]
fn window_resize_is_called_in_few_places() {
    for (name, code) in all() {
        if code.contains("window.resize(") {
            assert!(
                RESIZE.iter().any(|(n, _)| *n == name),
                "{name} calls `window.resize(`: the size of an independent window is the \
                 business of `independent.rs` and `device_window.rs` (or list it with a reason)"
            );
        }
    }
}

/// Structs that have both `at: Option<(f32, f32)>` and `pinned: bool` (a window of its own
/// place and pin drawn inside the screen). No window is drawn inside the screen any more, so
/// none is allowed: a window's place is the OS's.
const PLACED_AND_PINNED: &[&str] = &[];

#[test]
fn the_own_place_and_pin_structs_do_not_grow() {
    let mut found = BTreeSet::new();
    for (_, code) in all() {
        let mut rest = code.as_str();
        while let Some(i) = rest.find("struct ") {
            rest = &rest[i + 7..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let after = &rest[name.len()..];
            if !after.trim_start().starts_with('{') {
                continue;
            }
            let open = after.find('{').unwrap();
            let mut depth = 0;
            let mut end = after.len();
            for (k, c) in after[open..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let body = &after[open..end];
            if body.contains("at: Option<(f32, f32)>") && body.contains("pinned: bool") {
                found.insert(name);
            }
        }
    }
    let allowed: BTreeSet<String> = PLACED_AND_PINNED.iter().map(|s| s.to_string()).collect();
    assert!(
        found.is_subset(&allowed),
        "a struct with `at: Option<(f32, f32)>` and `pinned: bool` is new: {:?} (allowed {:?})",
        found.difference(&allowed).collect::<Vec<_>>(),
        allowed
    );
    assert_eq!(
        found, allowed,
        "a struct no longer has both fields: take it off PLACED_AND_PINNED"
    );
}

#[derive(PartialEq, Debug)]
enum Class {
    Modal,
    Popover,
    Transient,
    Section,
}

use Class::*;

/// Everything `draw::paint_overlay` paints, with its class.
const OVERLAY: &[(&str, Class)] = &[
    ("crate::start::paint", Section),
    ("crate::rec_panel::paint", Section),
    ("paint_takes", Section),
    ("paint_playhead", Section),
    ("paint_head_meters", Section),
    ("crate::arrange::paint::overlay", Section),
    ("crate::track_name::paint", Popover),
    ("crate::zoom_input::paint", Popover),
    ("crate::ai::overlay", Section),
    ("crate::mixer::paint::paint", Section),
    ("crate::fx_chain::paint_headers", Section),
    // No window is painted here (the main screen's frame for every device window is empty, see
    // `the_main_screen_frames_no_device_window` in window_tests.rs); it stays for the drum kit's drag chip, which
    // follows the pointer over this screen.
    ("crate::instrument::window::paint", Transient),
    ("crate::browser_inst::paint", Popover),
    ("crate::fx_chain::paint_drag", Transient),
    ("crate::name_drag::paint", Transient),
    ("crate::insert_drag::paint", Transient),
    ("crate::fx_preset::paint", Popover),
    ("crate::hover_tip::paint", Transient),
    ("crate::gain_tip::paint", Transient),
    ("crate::head_fader::paint", Transient),
    ("crate::home::paint", Section),
    ("crate::cleanup_dialog::paint_veil", Modal),
    ("crate::popup::paint_with", Popover),
    ("crate::cleanup_dialog::paint_ring", Modal),
    ("crate::hover_tip::popup", Transient),
    ("crate::notice::paint", Modal),
    ("crate::midi_assign::paint", Popover),
    ("crate::toast::paint", Transient),
    ("crate::dawproject_ui::paint_drop", Transient),
    ("inline:error", Transient),
    ("crate::palette::paint", Modal),
    ("crate::settings::paint", Modal),
    ("crate::recovery_card::paint", Modal),
    ("crate::prepare_card::paint", Modal),
    ("crate::wait::paint", Modal),
];

fn overlay_surfaces() -> BTreeSet<String> {
    let text = std::fs::read_to_string(src().join("draw.rs")).unwrap();
    let start = text.find("pub fn paint_overlay(").expect("paint_overlay");
    let body = &text[start..];
    let end = body.find("\n}\n").expect("end of paint_overlay");
    let body = &body[..end];
    // The signature is not a surface.
    let body = &body[body
        .find(
            "{
",
        )
        .unwrap()..];
    let b = body.as_bytes();
    let word_byte = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b':';
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < b.len() {
        let before_ok = i == 0 || !(word_byte(b[i - 1]) || b[i - 1] == b'.');
        if before_ok && (b[i].is_ascii_lowercase() || b[i] == b'_') {
            let mut j = i;
            while j < b.len() && word_byte(b[j]) {
                j += 1;
            }
            let word = &body[i..j];
            if j < b.len()
                && b[j] == b'('
                && (word.starts_with("crate::") || word.starts_with("paint_"))
                // A cache scope and a helper of the popup, not surfaces of their own.
                && !word.ends_with("PaintScope::enter")
                && word != "crate::ai::paint_fresh"
            {
                out.insert(word.to_string());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    if body.contains("&studio.error") {
        out.insert("inline:error".into());
    }
    out
}

#[test]
fn the_overlay_paints_only_listed_surfaces() {
    let found = overlay_surfaces();
    let listed: BTreeSet<String> = OVERLAY.iter().map(|(n, _)| n.to_string()).collect();
    assert!(
        found.is_subset(&listed),
        "paint_overlay paints a surface that is not in OVERLAY: {:?}. Classify it as a modal, \
         a popover, a transient show or a section of the screen (a window is a window, not an \
         overlay)",
        found.difference(&listed).collect::<Vec<_>>()
    );
    assert!(
        listed.is_subset(&found),
        "OVERLAY lists a surface paint_overlay no longer paints (remove it): {:?}",
        listed.difference(&found).collect::<Vec<_>>()
    );
}

#[test]
fn the_overlay_list_has_no_duplicates() {
    let names: BTreeSet<_> = OVERLAY.iter().map(|(n, _)| *n).collect();
    assert_eq!(names.len(), OVERLAY.len());
}

/// The Macro Controls are a window of their own: nothing of an in-screen window is left of
/// them (its place, its move grab, its paint in the main screen).
#[test]
fn the_macro_controls_are_not_drawn_or_placed_inside_the_screen() {
    let files = all();
    let get = |n: &str| {
        files
            .iter()
            .find(|(f, _)| f == n)
            .map(|(_, c)| c.clone())
            .unwrap()
    };
    let pad = get("macro_pad/mod.rs");
    assert!(
        !pad.contains("Grab::Move") && !pad.contains("Move {"),
        "the pad has no move grab"
    );
    assert!(
        !pad.contains("at: Option<(f32, f32)>"),
        "the pad has no place of its own"
    );
    let draw = get("draw.rs");
    assert!(
        !draw.contains("macro_pad::"),
        "paint_overlay does not paint the Macro Controls"
    );
    let opened = get("device_window.rs");
    assert!(
        opened.contains("Kind::Macros") && opened.contains("resizable: false"),
        "device_window.rs opens the Macro Controls through `independent::open`, not resizable"
    );
}

/// The Macro Controls' accessibility marks live in their own window's tree (window-local
/// coordinates), not in the main window's.
#[test]
fn the_macro_controls_marks_are_in_their_own_windows_tree() {
    let files = all();
    let get = |n: &str| {
        files
            .iter()
            .find(|(f, _)| f == n)
            .map(|(_, c)| c.clone())
            .unwrap()
    };
    assert!(
        !get("a11y.rs").contains("macro_pad::marks"),
        "the main window's marks do not include the Macro Controls'"
    );
    assert!(
        get("device_window.rs").contains("macro_pad::marks"),
        "the Macro window builds its own marks"
    );
}

/// The Macro Controls' panel is the whole window: no shadow, round corners or margin of an
/// in-screen floating panel are drawn inside it.
#[test]
fn the_macro_controls_panel_fills_its_window() {
    let files = all();
    let paint = files
        .iter()
        .find(|(f, _)| f == "macro_pad/paint.rs")
        .map(|(_, c)| c.clone())
        .unwrap();
    assert!(
        !paint.contains("pen.floating(") && paint.contains("pen.fill(v.r, c.paper)"),
        "the panel is a plain fill of the window's whole surface"
    );
}

/// The Channel Editor's and the devices' accessibility marks live in their own windows' trees
/// (window-local coordinates), not in the main window's.
#[test]
fn the_channel_editors_and_the_devices_marks_are_in_their_own_windows_trees() {
    let files = all();
    let get = |n: &str| {
        files
            .iter()
            .find(|(f, _)| f == n)
            .map(|(_, c)| c.clone())
            .unwrap()
    };
    let a11y = get("a11y.rs");
    assert!(
        !a11y.contains("channel::marks(s, g)") && !a11y.contains("out.extend(device_marks("),
        "the main window's marks include neither the Channel Editor's nor a device's"
    );
    let window = get("device_window.rs");
    assert!(
        window.contains("crate::channel::marks(") && window.contains("crate::a11y::device_marks("),
        "the Channel Editor's and the device's windows build their own marks"
    );
}
