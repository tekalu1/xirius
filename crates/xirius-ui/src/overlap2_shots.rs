//! `--script overlap2` (EDT-139): with "no clip overlap" on, the take that plays in a stack of
//! two is dragged two bars right with the pointer. The other take of its stack stays whole
//! under it, and the clip after the stack, which it now covers the head of, is cut. The drag
//! goes through the window's own input.
use gpui::{App, Window};
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use crate::feel_shots::{button_down, button_up, drag_to};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use crate::tree::RowKind;
use xirius_core::id::{ClipId, TrackId};

const CLIPS: &str = "edit.clip.no-overlap";

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "tk01-takes-before",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Stage(Play::Window(play), 0),
        ],
    },
    Shot {
        name: "tk02-take-moved",
        acts: &[Act::Stage(Play::Window(play), 1)],
    },
];

fn dir() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("overlap2-shots-{}", std::process::id()))
}

/// The Keys track and its clips, earliest first.
fn keys(s: &Studio) -> (TrackId, Vec<xirius_core::model::Clip>) {
    let p = s.project();
    let t = p
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .expect("the sample has Keys");
    let mut clips = t.clips.clone();
    clips.sort_by_key(|c| (c.start, c.id.0));
    (t.id, clips)
}

fn bar(s: &Studio) -> i64 {
    4 * i64::from(s.project().ppq)
}

/// Where the takes start: a bar after the clips of the sample.
fn home(s: &Studio) -> i64 {
    let (_, clips) = keys(s);
    clips.iter().map(|c| c.start + c.length).max().unwrap_or(0) + bar(s)
}

/// Fresh settings with "no clip overlap" on and, after the clips of Keys, two recorded takes
/// of four bars (a stack; the later one plays) and a clip of two bars a bar after them.
fn prepare(s: &mut Studio, _: u8) {
    s.popup = None;
    s.drawer_open = false;
    s.song.load_settings(&dir());
    s.settings_reload();
    s.song
        .run("prefs.set", json!({"key": CLIPS, "value": "true"}))
        .expect("the setting");
    let ppq = i64::from(s.project().ppq);
    let bar = bar(s);
    let at = home(s);
    let (t, _) = keys(s);
    let take = json!({"track": t.to_string(), "start": at, "length": 4 * bar,
    "notes": [
        {"start": 0, "length": 2 * ppq, "pitch": 60},
        {"start": bar, "length": 2 * ppq, "pitch": 64},
        {"start": 2 * bar, "length": 2 * ppq, "pitch": 67},
    ]});
    assert!(s.command(|song| {
        song.run("record.take.add", json!({"takes": [take, take]}))
            .map(|_| ())
    }));
    assert!(s.run_command(
        "clip.add.midi",
        json!({"track": t.to_string(), "at": at + 5 * bar, "length": 2 * bar}),
    ));
    let track = &s.project().tracks[&t];
    let stacks = xirius_core::takes::stacks(track);
    assert_eq!(stacks.len(), 1);
    assert_eq!(stacks[0].rows, 2);
}

/// The spans after `at` as (start - at, length, muted), earliest first.
fn spans(s: &Studio, at: i64) -> Vec<(i64, i64, bool)> {
    keys(s)
        .1
        .iter()
        .filter(|c| c.start >= at)
        .map(|c| (c.start - at, c.length, c.muted))
        .collect()
}

/// The take that plays.
fn playing(s: &Studio, at: i64) -> ClipId {
    keys(s)
        .1
        .iter()
        .find(|c| c.start == at && !c.muted)
        .expect("the take that plays")
        .id
}

fn takes_start(s: &Studio) -> i64 {
    keys(s)
        .1
        .iter()
        .filter(|c| c.muted)
        .map(|c| c.start)
        .max()
        .expect("the take that does not play")
}

/// 0 zooms out onto Keys so the stack, its take rows and the clip after it show; 1 drags the
/// take that plays two bars right.
fn play(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let mut st = s.borrow_mut();
            assert!(pref(&st));
            let at = takes_start(&st);
            let bar = bar(&st);
            assert_eq!(
                spans(&st, at),
                [
                    (0, 4 * bar, true),
                    (0, 4 * bar, false),
                    (5 * bar, 2 * bar, false)
                ]
            );
            let (t, _) = keys(&st);
            let take = playing(&st, at);
            st.selected_clip = Some((t, take));
            st.beat_w = 14.0;
            if let Some(row) = st.tree().row_of(RowKind::Track(t)).copied() {
                st.scroll.1 = (row.y - 8.0).max(0.0);
            }
            st.scroll.0 = (st.ticks_to_px(at - 2 * bar)).max(0.0);
        }
        _ => {
            let (from, to, at, bar, steps) = {
                let st = s.borrow();
                let g = crate::screen::geometry(&st, w);
                let at = takes_start(&st);
                let bar = bar(&st);
                let (t, _) = keys(&st);
                let from = st
                    .center_of(&g, Hit::Clip(t, playing(&st, at)))
                    .expect("the take is on screen");
                let to = (from.0 + st.ticks_to_px(2 * bar), from.1);
                (from, to, at, bar, st.song.steps())
            };
            button_down(from, 1, w, cx);
            drag_to(((from.0 + to.0) / 2.0, from.1), w, cx);
            drag_to(to, w, cx);
            button_up(to, 1, w, cx);
            let st = s.borrow();
            // The other take is whole under the moved one; the clip after them lost its head.
            assert_eq!(
                spans(&st, at),
                [
                    (0, 4 * bar, true),
                    (2 * bar, 4 * bar, false),
                    (6 * bar, bar, false)
                ]
            );
            assert_eq!(st.song.steps(), steps + 1);
            let (t, _) = keys(&st);
            let stacks = xirius_core::takes::stacks(&st.project().tracks[&t]);
            assert_eq!(stacks.len(), 1);
            assert_eq!(stacks[0].rows, 2);
        }
    }
}

/// Whether "no clip overlap" is on, as the song's commands read it.
fn pref(s: &Studio) -> bool {
    let mut guard = s.song.settings().lock().unwrap_or_else(|e| e.into_inner());
    guard.reload_if_changed();
    guard.prefs.bool(CLIPS)
}
