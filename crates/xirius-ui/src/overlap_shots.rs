//! `--script overlap`: the two "no overlap" rows of the settings' edit section (EDT-137 /
//! EDT-138), turned on with the pointer; then a note dragged onto the one before it in the roll
//! cuts that one's end, and a clip dropped into the middle of another splits it in two. Every
//! press and drag goes through the window's own input.
use gpui::{App, Modifiers, MouseButton, Window};
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use crate::feel_shots::{button_down, button_up, drag_to, pointer};
use crate::settings::{Do, Section};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Hit, Studio};
use xirius_core::id::{ClipId, TrackId};

const NOTES: &str = "edit.note.overlap";
const CLIPS: &str = "edit.clip.no-overlap";

const fn window(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "ov01-settings-off",
        acts: &[Act::Reset, Act::Stage(Play::Studio(prepare), 0), window(0)],
    },
    Shot {
        name: "ov02-settings-on",
        acts: &[window(1)],
    },
    Shot {
        name: "ov03-roll-before",
        acts: &[window(2)],
    },
    Shot {
        name: "ov04-roll-trimmed",
        acts: &[window(3)],
    },
    Shot {
        name: "ov05-arrange-before",
        acts: &[window(4)],
    },
    Shot {
        name: "ov06-arrange-split",
        acts: &[window(5)],
    },
];

fn dir() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("overlap-shots-{}", std::process::id()))
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
    clips.sort_by_key(|c| c.start);
    (t.id, clips)
}

/// Where the long clip starts: a bar after the clips of the sample.
fn home(s: &Studio) -> i64 {
    let bar = 4 * i64::from(s.project().ppq);
    let (_, clips) = keys(s);
    clips.iter().map(|c| c.start + c.length).max().unwrap_or(0) + bar
}

/// Fresh settings (both rows off) and, after the clips of Keys, a long clip of four bars with
/// two C4s (two beats at its head, one beat at its second bar) and a short clip of one bar two
/// bars after its end.
fn prepare(s: &mut Studio, _: u8) {
    s.popup = None;
    s.drawer_open = false;
    s.song.load_settings(&dir());
    s.settings_reload();
    let ppq = i64::from(s.project().ppq);
    let bar = 4 * ppq;
    let at = home(s);
    let (t, _) = keys(s);
    for (start, length) in [(at, 4 * bar), (at + 6 * bar, bar)] {
        assert!(s.run_command(
            "clip.add.midi",
            json!({"track": t.to_string(), "at": start, "length": length}),
        ));
    }
    let long = long_clip(s);
    assert!(s.run_command(
        "note.put",
        json!({"clip": long.to_string(), "notes": [
            {"at": 0, "pitch": 60, "length": 2 * ppq},
            {"at": bar, "pitch": 60, "length": ppq},
        ]}),
    ));
    assert!(!pref(s, |p| p.bool(CLIPS)));
}

/// A setting as the song's commands read it.
fn pref<T>(s: &Studio, read: impl Fn(&xirius_prefs::Prefs) -> T) -> T {
    let mut guard = s.song.settings().lock().unwrap_or_else(|e| e.into_inner());
    guard.reload_if_changed();
    read(&guard.prefs)
}

fn long_clip(s: &Studio) -> ClipId {
    let at = home_of(s);
    let (_, clips) = keys(s);
    clips
        .iter()
        .find(|c| c.start == at)
        .expect("the long clip")
        .id
}

/// `home` once the two clips are in: the long one is the second from the end.
fn home_of(s: &Studio) -> i64 {
    let (_, clips) = keys(s);
    clips[clips.len() - 2].start
}

fn notes(s: &Studio) -> Vec<(i64, i64)> {
    let id = long_clip(s);
    let (_, clips) = keys(s);
    let c = clips.iter().find(|c| c.id == id).unwrap();
    let mut v: Vec<(i64, i64)> = c.notes().iter().map(|n| (n.start, n.length)).collect();
    v.sort_unstable();
    v
}

/// Drags from `from` to `to` in two moves, so the drag starts before it gets there.
fn drag(from: (f32, f32), to: (f32, f32), w: &mut Window, cx: &mut App) {
    button_down(from, 1, w, cx);
    drag_to(((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0), w, cx);
    drag_to(to, w, cx);
    button_up(to, 1, w, cx);
}

/// 0 opens the settings at the edit section and shows the two rows; 1 turns both on with the
/// pointer; 2 shuts the settings and opens the roll on the long clip; 3 drags its second note
/// three beats left, onto the first; 4 shuts the drawer and zooms out; 5 drags the short clip four bars left,
/// into the middle of the long one.
fn play(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let gear = {
                let st = s.borrow();
                st.center_of(&crate::screen::geometry(&st, w), Hit::RailSettings)
                    .unwrap()
            };
            pointer(gear, Modifiers::default(), Some(MouseButton::Left), w, cx);
            click(s, w, cx, Do::Nav(Section::Edit));
            aim(s, w, cx, &Do::Pref(NOTES.into(), "trim".into()));
            aim(s, w, cx, &Do::Pref(CLIPS.into(), "true".into()));
        }
        1 => {
            click(s, w, cx, Do::Pref(NOTES.into(), "trim".into()));
            click(s, w, cx, Do::Pref(CLIPS.into(), "true".into()));
            let st = s.borrow();
            assert_eq!(pref(&st, |p| p.text(NOTES).to_string()), "trim");
            assert!(pref(&st, |p| p.bool(CLIPS)));
            assert!(dir().join("prefs.toml").is_file());
        }
        2 => {
            let mut st = s.borrow_mut();
            st.close_settings();
            let (t, _) = keys(&st);
            let id = long_clip(&st);
            st.selected_clip = Some((t, id));
            st.drawer_open = true;
            st.tab = DrawerTab::Roll;
            st.choose_editor();
            let g = crate::screen::geometry(&st, w);
            if let Some(c) = st.roll_ctx(&g) {
                st.roll.sync(&c);
                st.roll.center(&c);
            }
        }
        3 => {
            let ppq = i64::from(s.borrow().project().ppq);
            assert_eq!(notes(&s.borrow()), [(0, 2 * ppq), (4 * ppq, ppq)]);
            let (from, to, steps) = {
                let st = s.borrow();
                let g = crate::screen::geometry(&st, w);
                let c = st.roll_ctx(&g).expect("the roll is open");
                let v = st.roll.view(&c);
                let n = c
                    .notes()
                    .iter()
                    .map(|n| st.roll.shown(n))
                    .max_by_key(|n| n.start)
                    .unwrap();
                let from = (
                    v.x(n.start) + v.px(n.length) / 2.0,
                    v.y(n.pitch) + v.row_h / 2.0,
                );
                (from, (from.0 - v.px(3 * ppq), from.1), st.song.steps())
            };
            drag(from, to, w, cx);
            let st = s.borrow();
            assert_eq!(notes(&st), [(0, ppq), (ppq, ppq)]);
            assert_eq!(st.song.steps(), steps + 1);
        }
        4 => {
            let mut st = s.borrow_mut();
            st.drawer_open = false;
            st.choose_editor();
            let (t, clips) = keys(&st);
            st.selected_clip = Some((t, clips[clips.len() - 1].id));
            // Zoomed out so the song and the two clips fit.
            st.beat_w = 14.0;
            st.scroll.0 = 0.0;
        }
        _ => {
            let (from, to, at, bar, steps) = {
                let st = s.borrow();
                let g = crate::screen::geometry(&st, w);
                let bar = 4 * i64::from(st.project().ppq);
                let (t, clips) = keys(&st);
                let short = clips[clips.len() - 1].id;
                let from = st
                    .center_of(&g, Hit::Clip(t, short))
                    .expect("the short clip is on screen");
                let to = (from.0 - st.ticks_to_px(4 * bar), from.1);
                (from, to, home_of(&st), bar, st.song.steps())
            };
            drag(from, to, w, cx);
            let st = s.borrow();
            let spans: Vec<(i64, i64)> = keys(&st)
                .1
                .iter()
                .filter(|c| c.start >= at)
                .map(|c| (c.start - at, c.length))
                .collect();
            assert_eq!(spans, [(0, 2 * bar), (2 * bar, bar), (3 * bar, bar)]);
            assert_eq!(st.song.steps(), steps + 1);
        }
    }
}

fn aim(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, act: &Do) -> (f32, f32) {
    crate::automation_preferences_shots::aim(s, w, cx, act)
}

fn click(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, act: Do) {
    let p = aim(s, w, cx, &act);
    pointer(p, Modifiers::default(), Some(MouseButton::Left), w, cx);
}
