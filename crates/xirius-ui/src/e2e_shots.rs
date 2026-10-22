//! `--script e2e`: one song made from the empty screen to the files it is written to, every
//! step through the window's own input (the presses, drags and keys a person makes). Each stage
//! prints one `e2e-check stage=N name=.. ok=..` line with what it found on the screen and in the
//! song; a stage that fails says why and the next ones still run (`tests/e2e_window.rs` reads
//! the lines).
//!
//! The song is kept in `XIRIUS_E2E_DIR` (or `xirius-e2e-shots` in the temp folder): the stage
//! that saves it empties that folder first.

mod finish;
mod make;
mod mix;

use crate::{
    shot::{Act, Play, Shot},
    studio::{Geometry, Hit, Rect, Studio},
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use std::{cell::RefCell, collections::BTreeSet, path::PathBuf, rc::Rc};
use xirius_core::id::TrackId;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "e2e-00-new-song",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "e2e-01-midi-track",
        acts: &[stage(1)],
    },
    Shot {
        name: "e2e-02-tempo",
        acts: &[stage(2)],
    },
    Shot {
        name: "e2e-03-meter",
        acts: &[stage(3)],
    },
    Shot {
        name: "e2e-04-instrument-track",
        acts: &[stage(4)],
    },
    Shot {
        name: "e2e-05-instrument-list",
        acts: &[stage(5)],
    },
    Shot {
        name: "e2e-06-plugin-instrument",
        acts: &[stage(6)],
    },
    Shot {
        name: "e2e-07-midi-part",
        acts: &[stage(7)],
    },
    Shot {
        name: "e2e-08-notes-drawn",
        acts: &[stage(8)],
    },
    Shot {
        name: "e2e-09-note-moved",
        acts: &[stage(9)],
    },
    Shot {
        name: "e2e-10-quantized",
        acts: &[stage(10)],
    },
    Shot {
        name: "e2e-11-drum-view",
        acts: &[stage(11)],
    },
    Shot {
        name: "e2e-12-drum-hits",
        acts: &[stage(12)],
    },
    Shot {
        name: "e2e-13-saved-as",
        acts: &[stage(13)],
    },
    Shot {
        name: "e2e-14-file-dragged",
        acts: &[stage(14)],
    },
    Shot {
        name: "e2e-15-split",
        acts: &[stage(15)],
    },
    Shot {
        name: "e2e-16-loop",
        acts: &[stage(16)],
    },
    Shot {
        name: "e2e-17-fade",
        acts: &[stage(17)],
    },
    Shot {
        name: "e2e-18-fader-pan",
        acts: &[stage(18)],
    },
    Shot {
        name: "e2e-19-insert",
        acts: &[stage(19)],
    },
    Shot {
        name: "e2e-20-send",
        acts: &[stage(20)],
    },
    Shot {
        name: "e2e-21-automation",
        acts: &[stage(21)],
    },
    Shot {
        name: "e2e-22-marker",
        acts: &[stage(22)],
    },
    Shot {
        name: "e2e-23-bounce",
        acts: &[stage(23)],
    },
    Shot {
        name: "e2e-24-freeze",
        acts: &[stage(24)],
    },
    Shot {
        name: "e2e-25-saved",
        acts: &[stage(25)],
    },
    Shot {
        name: "e2e-26-closed",
        acts: &[stage(26)],
    },
    Shot {
        name: "e2e-27-reopened",
        acts: &[stage(27)],
    },
    Shot {
        name: "e2e-28-wav",
        acts: &[stage(28)],
    },
    Shot {
        name: "e2e-29-mp3",
        acts: &[stage(29)],
    },
    Shot {
        name: "e2e-30-dawproject",
        acts: &[stage(30)],
    },
    Shot {
        name: "e2e-31-recorded",
        acts: &[stage(31)],
    },
    Shot {
        name: "e2e-32-undone",
        acts: &[stage(32)],
    },
    Shot {
        name: "e2e-33-redone",
        acts: &[stage(33)],
    },
];

/// What the stages hand on to the later ones.
#[derive(Default)]
struct Memo {
    /// The track 「MIDI を演奏」 made, the one with a plugin, and the audio track.
    keys: Option<TrackId>,
    synth: Option<TrackId>,
    audio: Option<TrackId>,
    /// The song as it was when it was new, before it was closed, and before the undoing.
    empty: Option<serde_json::Value>,
    closed: Option<serde_json::Value>,
    full: Option<serde_json::Value>,
    /// The steps there were to undo.
    steps: usize,
}

thread_local! {
    static MEMO: RefCell<Memo> = RefCell::new(Memo::default());
}

fn memo<T>(f: impl FnOnce(&mut Memo) -> T) -> T {
    MEMO.with(|m| f(&mut m.borrow_mut()))
}

/// Where the song is saved.
fn song_dir() -> PathBuf {
    std::env::var_os("XIRIUS_E2E_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xirius-e2e-shots"))
}

type Studio1 = Rc<RefCell<Studio>>;
type Done = Result<String, String>;

fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) {
    let name = SHOTS
        .get(usize::from(n))
        .map_or("?", |shot| shot.name.trim_start_matches("e2e-"));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let done = match n {
            0..=14 => make::play(n, s, w, cx),
            15..=24 => mix::play(n, s, w, cx),
            _ => finish::play(n, s, w, cx),
        };
        let error = s.borrow_mut().error.take();
        match (done, error) {
            (Ok(_), Some(e)) => Err(format!("the screen says: {e}")),
            (done, _) => done,
        }
    }));
    let done = run.unwrap_or_else(|e| {
        Err(e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panicked".into()))
    });
    let (ok, what) = match done {
        Ok(what) => (true, what),
        Err(what) => (false, what),
    };
    eprintln!(
        "e2e-check stage={n} name={name} ok={ok} {}",
        what.replace(['\n', '\r'], " ")
    );
}

fn check(ok: bool, what: impl Into<String>) -> Result<(), String> {
    if ok { Ok(()) } else { Err(what.into()) }
}

// ------------------------------------------------------------------ the window's input

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn hover(p: (f32, f32), w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(p: (f32, f32), b: MouseButton, m: Modifiers, clicks: usize, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: b,
            position: at(p),
            modifiers: m,
            click_count: clicks,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(p: (f32, f32), b: MouseButton, m: Modifiers, clicks: usize, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: b,
            position: at(p),
            modifiers: m,
            click_count: clicks,
        }),
        cx,
    );
}

fn held(p: (f32, f32), b: MouseButton, m: Modifiers, w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: Some(b),
            modifiers: m,
        }),
        cx,
    );
}

/// Moves there and clicks `n` times.
fn click_n(p: (f32, f32), n: usize, w: &mut Window, cx: &mut App) {
    hover(p, w, cx);
    for clicks in 1..=n {
        down(p, MouseButton::Left, Modifiers::default(), clicks, w, cx);
        up(p, MouseButton::Left, Modifiers::default(), clicks, w, cx);
    }
}

fn click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    click_n(p, 1, w, cx);
}

fn right_click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    hover(p, w, cx);
    down(p, MouseButton::Right, Modifiers::default(), 1, w, cx);
    up(p, MouseButton::Right, Modifiers::default(), 1, w, cx);
}

/// Presses at `a`, moves to `b` in steps with the button held, lets go.
fn drag_m(a: (f32, f32), b: (f32, f32), m: Modifiers, w: &mut Window, cx: &mut App) {
    hover(a, w, cx);
    down(a, MouseButton::Left, m, 1, w, cx);
    for i in 1..=6 {
        let f = i as f32 / 6.0;
        held(
            (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f),
            MouseButton::Left,
            m,
            w,
            cx,
        );
    }
    up(b, MouseButton::Left, m, 1, w, cx);
}

fn drag(a: (f32, f32), b: (f32, f32), w: &mut Window, cx: &mut App) {
    drag_m(a, b, Modifiers::default(), w, cx);
}

fn keys(text: &str, w: &mut Window, cx: &mut App) {
    for key in text.split(' ') {
        w.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
    }
}

// ------------------------------------------------------------------ reading the screen

/// The screen at rest: the drawer and the rows that slide are where they end.
fn rest(s: &Studio1, w: &Window) {
    let s = s.borrow();
    let was = s.motion.enabled.replace(false);
    crate::screen::geometry(&s, w);
    s.motion.enabled.set(was);
}

fn read<T>(s: &Studio1, w: &Window, f: impl FnOnce(&Studio, &Geometry) -> T) -> T {
    rest(s, w);
    let s = s.borrow();
    let g = crate::screen::geometry(&s, w);
    f(&s, &g)
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The centre of something the screen shows.
fn spot(s: &Studio1, w: &Window, hit: Hit) -> Result<(f32, f32), String> {
    read(s, w, |s, g| s.center_of(g, hit)).ok_or_else(|| format!("{hit:?} is not on the screen"))
}

fn press(s: &Studio1, hit: Hit, w: &mut Window, cx: &mut App) -> Result<(), String> {
    let p = spot(s, w, hit)?;
    click(p, w, cx);
    Ok(())
}

/// Clicks the open pop-over's cell with this text (not a field that holds the same text).
fn cell(s: &Studio1, text: &str, w: &mut Window, cx: &mut App) -> Result<(), String> {
    cell_where(
        s,
        text,
        |c| c.text == text && c.look != crate::popup::Look::Input,
        w,
        cx,
    )
}

fn cell_where(
    s: &Studio1,
    what: &str,
    f: impl Fn(&crate::popup::Cell) -> bool,
    w: &mut Window,
    cx: &mut App,
) -> Result<(), String> {
    let p = read(s, w, |s, g| {
        let popup = s.popup.as_ref().ok_or("no pop-over is open")?;
        let cells = popup.layout(g.size).cells;
        let found = || cells.iter().filter(|c| f(c));
        let c = found().find(|c| c.live()).or_else(|| found().next());
        let c = c.ok_or_else(|| {
            let texts: Vec<_> = cells
                .iter()
                .filter(|c| !c.text.is_empty())
                .map(|c| c.text.clone())
                .collect();
            format!("no cell {what} in {texts:?}")
        })?;
        if !c.live() {
            return Err(format!("the cell {what} cannot be pressed"));
        }
        Ok(mid(c.rect))
    })?;
    click(p, w, cx);
    Ok(())
}

fn row_of(s: &Studio, t: TrackId) -> Result<crate::tree::Row, String> {
    s.tree()
        .row_of(RowKind::Track(t))
        .copied()
        .ok_or_else(|| format!("no row for {t}"))
}

/// The middle of the lane of `t` at `tick`.
fn lane_at(s: &Studio, g: &Geometry, t: TrackId, tick: i64) -> Result<(f32, f32), String> {
    let row = row_of(s, t)?;
    Ok((s.tick_x(g, tick), s.row_y(g, &row) + row.h / 2.0))
}

/// The rect of the clip of `t` that is `i`th in time.
fn clip_rect(s: &Studio, g: &Geometry, t: TrackId, i: usize) -> Result<Rect, String> {
    let p = s.project();
    let mut clips: Vec<_> = p.tracks.get(&t).ok_or("no track")?.clips.iter().collect();
    clips.sort_by_key(|c| c.start);
    let c = clips.get(i).ok_or_else(|| format!("no clip {i}"))?;
    Ok(s.clip_rect(g, &row_of(s, t)?, c))
}

fn tracks(s: &Studio1) -> BTreeSet<TrackId> {
    s.borrow().project().tracks.keys().copied().collect()
}

/// The one track that is new since `before`.
fn made(s: &Studio1, before: &BTreeSet<TrackId>) -> Result<TrackId, String> {
    let now = tracks(s);
    let new: Vec<_> = now.difference(before).copied().collect();
    match new.as_slice() {
        [t] => Ok(*t),
        _ => Err(format!("{} new tracks", new.len())),
    }
}

fn track_of(f: impl FnOnce(&Memo) -> Option<TrackId>, what: &str) -> Result<TrackId, String> {
    memo(|m| f(m)).ok_or_else(|| format!("no {what} track from an earlier stage"))
}

/// The song as data, to compare.
fn song_json(s: &Studio1) -> serde_json::Value {
    serde_json::to_value(&*s.borrow().project()).unwrap_or_default()
}

/// Where two songs differ, as JSON pointers (the first few).
fn differences(a: &serde_json::Value, b: &serde_json::Value) -> Vec<String> {
    fn walk(a: &serde_json::Value, b: &serde_json::Value, at: String, out: &mut Vec<String>) {
        use serde_json::Value::{Array, Object};
        match (a, b) {
            (Object(x), Object(y)) => {
                let keys: BTreeSet<_> = x.keys().chain(y.keys()).collect();
                for k in keys {
                    match (x.get(k), y.get(k)) {
                        (Some(p), Some(q)) => walk(p, q, format!("{at}/{k}"), out),
                        _ => out.push(format!("{at}/{k}")),
                    }
                }
            }
            (Array(x), Array(y)) if x.len() == y.len() => {
                for (i, (p, q)) in x.iter().zip(y).enumerate() {
                    walk(p, q, format!("{at}/{i}"), out);
                }
            }
            (p, q) if p != q => out.push(at),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(a, b, String::new(), &mut out);
    out
}
