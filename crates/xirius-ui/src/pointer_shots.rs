//! `--script pointer`: the pointer over the arrangement for each tool (UI-184), read from the
//! cursor the real (parked, silent) window hands to the system after it has drawn a frame. Each
//! stage puts the pointer on a case, and the next stage (a frame later) reports what the window
//! handed over on stderr (`pointer-check stage=N ok=true`), which `tests/pointer_window.rs`
//! reads. The pointer is moved through the studio's own handlers: no OS input is sent.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, CursorStyle, Modifiers, MouseMoveEvent, PlatformInput, Window, point, px};
use xirius_design::Layout;

use crate::arrange::probe::{clip_mid, row_at, track};
use crate::pointer::Shape;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tools::Tool;
use crate::tree::RowKind;
use xirius_core::model::TrackKind;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

/// Where a case puts the pointer.
#[derive(Clone, Copy)]
enum At {
    Clip,
    EmptyKeys,
    EmptyAudio,
}

/// What the window hands to the system.
#[derive(Clone, Copy)]
enum Want {
    Picture(Shape),
    /// The system's own shape, and no picture of ours.
    Style(CursorStyle),
}

const CASES: &[(&str, Tool, At, Want, bool)] = &[
    (
        "cut-on-clip",
        Tool::Cut,
        At::Clip,
        Want::Picture(Shape::Scissors),
        false,
    ),
    (
        "eraser-on-clip",
        Tool::Eraser,
        At::Clip,
        Want::Picture(Shape::Eraser),
        false,
    ),
    (
        "mute-on-clip",
        Tool::Mute,
        At::Clip,
        Want::Picture(Shape::Mute),
        false,
    ),
    (
        "audition-on-clip",
        Tool::Audition,
        At::Clip,
        Want::Picture(Shape::Audition),
        false,
    ),
    (
        "pencil-on-clip",
        Tool::Draw,
        At::Clip,
        Want::Picture(Shape::Pencil),
        false,
    ),
    (
        "range-on-clip",
        Tool::Range,
        At::Clip,
        Want::Picture(Shape::Range),
        false,
    ),
    (
        "cut-on-empty",
        Tool::Cut,
        At::EmptyKeys,
        Want::Style(CursorStyle::OperationNotAllowed),
        false,
    ),
    (
        "eraser-on-empty",
        Tool::Eraser,
        At::EmptyAudio,
        Want::Style(CursorStyle::OperationNotAllowed),
        false,
    ),
    (
        "mute-on-empty",
        Tool::Mute,
        At::EmptyKeys,
        Want::Style(CursorStyle::OperationNotAllowed),
        false,
    ),
    (
        "audition-on-empty",
        Tool::Audition,
        At::EmptyAudio,
        Want::Style(CursorStyle::OperationNotAllowed),
        false,
    ),
    (
        "pencil-on-audio",
        Tool::Draw,
        At::EmptyAudio,
        Want::Style(CursorStyle::OperationNotAllowed),
        false,
    ),
    (
        "pencil-on-instrument",
        Tool::Draw,
        At::EmptyKeys,
        Want::Picture(Shape::Pencil),
        false,
    ),
    (
        "arrow-on-empty",
        Tool::Select,
        At::EmptyKeys,
        Want::Style(CursorStyle::Arrow),
        false,
    ),
    (
        "crosshair-setting",
        Tool::Cut,
        At::Clip,
        Want::Style(CursorStyle::Crosshair),
        true,
    ),
    (
        "rest",
        Tool::Select,
        At::EmptyKeys,
        Want::Style(CursorStyle::Arrow),
        false,
    ),
];

macro_rules! shots {
    ($($n:literal $name:literal),* $(,)?) => {
        &[$(Shot { name: concat!("pointer-", $name), acts: &[staged($n)] }),*]
    };
}

pub const SHOTS: &[Shot] = shots!(
    0 "01-cut-on-clip",
    1 "02-eraser-on-clip",
    2 "03-mute-on-clip",
    3 "04-audition-on-clip",
    4 "05-pencil-on-clip",
    5 "06-range-on-clip",
    6 "07-cut-on-empty",
    7 "08-eraser-on-empty",
    8 "09-mute-on-empty",
    9 "10-audition-on-empty",
    10 "11-pencil-on-audio",
    11 "12-pencil-on-instrument",
    12 "13-arrow-on-empty",
    13 "14-crosshair-setting",
    14 "15-rest",
    15 "16-end",
);

/// A place on the lanes where no track has a part.
fn free_x(s: &Studio, g: &Geometry) -> f32 {
    let bar = 3840;
    let project = s.project();
    (0..60)
        .map(|b| b * bar + bar / 2)
        .find(|t| {
            g.lanes.contains(s.tick_x(g, *t), g.lanes.y + 1.0)
                && project.tracks.values().all(|k| {
                    k.clips
                        .iter()
                        .all(|c| *t < c.start || *t >= c.start + c.length)
                })
        })
        .map(|t| s.tick_x(g, t))
        .expect("a free place in view")
}

/// Scrolls the tracks to the first audio track and gives the middle of its row.
fn audio_row(s: &mut Studio, g: &Geometry) -> f32 {
    let id = s
        .project()
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Audio)
        .expect("an audio track")
        .id;
    let top = s.tree().row_of(RowKind::Track(id)).expect("row").y;
    s.scroll.1 = top - 8.0;
    row_at(s, g, RowKind::Track(id), 0.5)
}

/// Scrolls the tracks so the Keys row is on screen. The body is short enough that, from the top,
/// the row can be below the fold.
fn keys_in_view(s: &mut Studio) {
    let id = track(s, "Keys");
    let top = s.tree().row_of(RowKind::Track(id)).expect("row").y;
    s.scroll.1 = top - 8.0;
}

fn set_crosshair(s: &mut Studio, on: bool) {
    s.song
        .run(
            "prefs.set",
            serde_json::json!({"key": "edit.cursor.crosshair", "value": on.to_string()}),
        )
        .expect("the setting");
    s.apply_edit_prefs();
}

/// Reports what the window hands over for case `n`, drawn since the stage before. The tool's
/// picture is read from the cursor the window holds. The system's own shapes are GPUI's to set,
/// and only for a window the pointer is over (a parked window never is), so there the window
/// must have left the cursor alone and asked GPUI for the shape.
fn check(n: usize, s: &Studio, g: &Geometry) {
    let (name, _, _, want, _) = CASES[n];
    let pointer = crate::pointer::of_studio(s, g);
    #[cfg(windows)]
    let handed = {
        let hwnd = crate::park::own_handle(crate::screen::WINDOW_TITLE).expect("the window");
        crate::pointer_os::handed(hwnd).map(|c| {
            c == crate::pointer_os::cursor(
                pointer.shape.unwrap_or(Shape::Range),
                crate::pointer_os::size_for(hwnd),
            )
        })
    };
    #[cfg(not(windows))]
    let handed = pointer.shape.map(|_| true);
    let ok = match want {
        Want::Picture(shape) => pointer.shape == Some(shape) && handed == Some(true),
        Want::Style(style) => pointer.shape.is_none() && pointer.style == style && handed.is_none(),
    };
    eprintln!("pointer-check stage={n} case={name} ok={ok}");
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let n = usize::from(n);
    if n > 0 {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        check(n - 1, &s, &g);
    }
    let Some(&(_, tool, at, _, crosshair)) = CASES.get(n) else {
        return;
    };
    let (x, y) = {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        if n > 0 && CASES[n - 1].4 && !crosshair {
            set_crosshair(&mut s, false);
        }
        if crosshair {
            set_crosshair(&mut s, true);
        }
        s.scroll.1 = 0.0;
        s.tool = tool;
        match at {
            At::Clip => {
                keys_in_view(&mut s);
                clip_mid(&s, &g, "Keys", 0)
            }
            At::EmptyKeys => {
                keys_in_view(&mut s);
                (
                    free_x(&s, &g),
                    row_at(&s, &g, RowKind::Track(track(&s, "Keys")), 0.5),
                )
            }
            At::EmptyAudio => {
                let y = audio_row(&mut s, &g);
                (free_x(&s, &g), y)
            }
        }
    };
    eprintln!("pointer-at stage={n} x={x} y={}", y + Layout::TOP_BAR_H);
    // Through the window's own input path, so GPUI knows where the pointer is as well.
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(y + Layout::TOP_BAR_H)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}
