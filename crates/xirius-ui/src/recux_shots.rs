//! `--script recux`: the recording flow of the approved 1.0 form (ux-v1-rec 01 to 05), played as
//! real pointer and key events on the window. Pre-count in the record button and the red of a
//! take that is being recorded; laps of a loop that become takes; a take row swiped, pressed and
//! moved with the arrow keys; the take rows' right-click menu; many takes in short rows; and the
//! record panel with the punch range on the ruler. Nothing opens a device: the input is the
//! built-in test tone and the output is silent.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;
use xirius_core::id::TrackId;
use xirius_core::time::Tick;
use xirius_design::Layout;

use crate::rec_area::RecArea;
use crate::rec_panel::RecPart;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::topbar::BarButton;
use crate::tree::RowKind;

const BAR: Tick = 3840;
/// Where the passage sits: bar 14, where the sample song's Vocal track has nothing.
const FIRST_BAR: u32 = 14;

/// Stage `n` of [`RECUX`], played through the window.
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const RECUX: &[Shot] = &[
    // Two bars of pre-count are being counted: the record button shows the beats left, and
    // the position stays white.
    Shot {
        name: "u01-count",
        acts: &[Act::Reset, staged(0), staged(1)],
    },
    // The count ends and the take begins: the button, the position and the playhead turn red.
    Shot {
        name: "u02-recording",
        acts: &[staged(2)],
    },
    // Stopped: the take is in the track's colour.
    Shot {
        name: "u03-stopped",
        acts: &[staged(3)],
    },
    // A loop of two bars recorded for a lap and a half, stopped midway: the last whole lap plays
    // and carries the count of takes; the half lap is a take that does not play.
    Shot {
        name: "u04-loop-recording",
        acts: &[Act::Reset, staged(10), staged(11)],
    },
    Shot {
        name: "u05-loop-stopped",
        acts: &[staged(12)],
    },
    // Four takes of one passage; a swipe along the third promotes that stretch.
    Shot {
        name: "u06-comp-swiping",
        acts: &[Act::Reset, staged(20), staged(21)],
    },
    Shot {
        name: "u07-comp-swiped",
        acts: &[staged(22)],
    },
    // The arrow keys: up switches the take over the stretch.
    Shot {
        name: "u08-comp-key-up",
        acts: &[staged(23)],
    },
    // A right-click on a take row.
    Shot {
        name: "u09-comp-menu",
        acts: &[staged(24)],
    },
    // Eight takes: the rows shrink so that all of them are in view.
    Shot {
        name: "u10-comp-many",
        acts: &[staged(25)],
    },
    // The record panel: auto-punch (the loop range is the punch range, red on the ruler),
    // pre-roll, pre-count and input quantize.
    Shot {
        name: "u11-record-panel",
        acts: &[Act::Reset, staged(30)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn mv(p: (f32, f32), held: Option<MouseButton>, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: held,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    mv(p, None, window, cx);
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn click(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    down(p, button, window, cx);
    up(p, button, window, cx);
}

fn key(name: &str, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: Keystroke {
                modifiers: Modifiers::default(),
                key: name.into(),
                key_char: None,
            },
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
}

fn vocal(s: &Studio) -> Option<TrackId> {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .map(|t| t.id)
}

/// Presses the middle of `hit` as it is on the window now.
fn press_hit(studio: &Rc<RefCell<Studio>>, hit: Hit, window: &mut Window, cx: &mut App) {
    let p = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, hit)
    };
    match p {
        Some(p) => click(p, MouseButton::Left, window, cx),
        None => eprintln!("recux: nothing to press for {hit:?}"),
    }
}

/// Where `tick` of take row `j` of the track is.
fn row_at(s: &Studio, g: &Geometry, t: TrackId, tick: Tick, j: usize) -> Option<(f32, f32)> {
    let row = *s.tree().row_of(RowKind::Track(t))?;
    let area = RecArea::of(&s.project().tracks[&t], s.takes_open.contains(&t));
    Some((
        s.tick_x(g, tick),
        s.row_y(g, &row) + (row.rec_y() - row.y) + area.take_y(j) + area.take_h() / 2.0,
    ))
}

/// Scrolls so that the track's rows and the passage are on screen.
fn show(s: &mut Studio, t: TrackId) {
    if let Some(row) = s.tree().row_of(RowKind::Track(t)).copied() {
        s.scroll.1 = row.y;
    }
    let start = s.project().time_map().bar_start(FIRST_BAR);
    s.scroll.0 = (s.ticks_to_px(start) - 40.0).max(0.0);
}

/// `n` takes of the two bars at the first bar, as a recording of laps leaves them.
fn add_takes(s: &mut Studio, t: TrackId, from: usize, n: usize) {
    let start = s.project().time_map().bar_start(FIRST_BAR);
    let takes: Vec<_> = (from..from + n)
        .map(|i| {
            json!({"track": t.to_string(), "start": start, "length": 2 * BAR,
                   "file": format!("Recordings/Vocal-{i}.wav")})
        })
        .collect();
    s.command(|song| {
        song.run("record.take.add", json!({ "takes": takes }))
            .map(|_| ())
    });
}

/// Lets the studio run for `secs`, taking the waveforms made meanwhile, so that a take just
/// recorded is drawn with its waveform.
fn settle(s: &mut Studio, secs: f32) {
    // The run draws nothing, so the peaks are asked for here, as a drawn clip would.
    let dir = s.song().dir();
    let store = xirius_session::peaks::store_dir(dir.as_deref());
    let project = s.project();
    let files: Vec<_> = project
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter_map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, .. } => {
                crate::wave::source(dir.as_deref(), file)
            }
            _ => None,
        })
        .collect();
    crate::shot::run_for(s, secs, |_| {
        for f in &files {
            crate::wave::look(f, &store);
        }
        crate::wave::poll();
        false
    });
}

/// The song saved in a folder of its own, so that a recording has somewhere to go, with the
/// Vocal armed and the playhead at the passage.
fn ready(s: &mut Studio, t: TrackId) {
    let dir = std::env::temp_dir().join("xirius-recux-shots");
    let _ = std::fs::remove_dir_all(&dir);
    // The folder is made anew, and the files in it take the names the last ones had.
    crate::wave::forget();
    s.command(|song| song.save_as(&dir));
    s.command(|song| song.toggle_arm(t));
    let bar = s.project().time_map().bar_start(FIRST_BAR);
    s.seek(bar);
    show(s, t);
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let t = {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        let Some(t) = vocal(&s) else {
            eprintln!("no Vocal track");
            return;
        };
        if matches!(stage, 0 | 10 | 20 | 30) {
            s.drawer_open = false;
            s.resized(&g);
        }
        t
    };
    let start = studio.borrow().project().time_map().bar_start(FIRST_BAR);
    let bars = |n: Tick| start + n * BAR;
    match stage {
        // The pre-count: two bars, the record button pressed.
        0 => {
            let mut s = studio.borrow_mut();
            ready(&mut s, t);
            let _ = s
                .song
                .set("record.count-in", json!({"on": true, "bars": 2}));
        }
        1 => {
            press_hit(studio, Hit::Bar(BarButton::Record), window, cx);
            let mut s = studio.borrow_mut();
            crate::shot::run_for(&mut s, 6.0, |s| {
                s.bar_view().pre_count.is_some_and(|n| n <= 2)
            });
        }
        2 => {
            let mut s = studio.borrow_mut();
            crate::shot::run_for(&mut s, 6.0, |s| s.bar_view().recording_now());
            crate::shot::run_for(&mut s, 1.2, |_| false);
        }
        3 => {
            press_hit(studio, Hit::Bar(BarButton::Stop), window, cx);
            settle(&mut studio.borrow_mut(), 3.0);
        }
        // Laps of a loop.
        10 => {
            {
                let mut s = studio.borrow_mut();
                ready(&mut s, t);
                let _ = s.song.set("record.count-in", json!({"on": false}));
                s.command(|song| {
                    song.run("loop.set", json!({"start": bars(0), "end": bars(2)}))
                        .map(|_| ())
                });
            }
            press_hit(studio, Hit::Bar(BarButton::Loop), window, cx);
        }
        11 => {
            press_hit(studio, Hit::Bar(BarButton::Record), window, cx);
            // A lap is four seconds, and the first few of the first are lost to the engine's
            // start: one whole lap and half of another.
            crate::shot::run_for(&mut studio.borrow_mut(), 8.6, |_| false);
        }
        12 => {
            press_hit(studio, Hit::Bar(BarButton::Stop), window, cx);
            settle(&mut studio.borrow_mut(), 3.0);
        }
        // The comp.
        20 => {
            let mut s = studio.borrow_mut();
            add_takes(&mut s, t, 1, 4);
            show(&mut s, t);
        }
        21 => {
            let (a, b) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (
                    row_at(&s, &g, t, bars(1) - BAR / 2, 2),
                    row_at(&s, &g, t, bars(1) + BAR / 2, 2),
                )
            };
            if let (Some(a), Some(b)) = (a, b) {
                down(a, MouseButton::Left, window, cx);
                mv(
                    ((a.0 + b.0) / 2.0, a.1),
                    Some(MouseButton::Left),
                    window,
                    cx,
                );
                mv(b, Some(MouseButton::Left), window, cx);
            }
        }
        22 => {
            let b = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                row_at(&s, &g, t, bars(1) + BAR / 2, 2)
            };
            if let Some(b) = b {
                up(b, MouseButton::Left, window, cx);
            }
        }
        23 => key("up", window, cx),
        24 => {
            let p = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                row_at(&s, &g, t, bars(1) + BAR / 4, 1)
            };
            if let Some(p) = p {
                click(p, MouseButton::Right, window, cx);
            }
        }
        25 => {
            key("escape", window, cx);
            let mut s = studio.borrow_mut();
            add_takes(&mut s, t, 5, 4);
            show(&mut s, t);
        }
        // The record panel and the punch range.
        30 => {
            {
                let mut s = studio.borrow_mut();
                s.command(|song| {
                    song.run("loop.set", json!({"start": bars(0), "end": bars(2)}))
                        .map(|_| ())
                });
                show(&mut s, t);
            }
            press_hit(studio, Hit::Bar(BarButton::RecPanel), window, cx);
            for part in [
                RecPart::Punch,
                RecPart::PreRoll,
                RecPart::Count,
                RecPart::Quantize,
            ] {
                press_hit(studio, Hit::Rec(part), window, cx);
            }
        }
        _ => {}
    }
    // The pointer rests on an empty place, so that no tip stays over a button (not in the
    // middle of a drag).
    if stage == 21 {
        return;
    }
    let rest = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        (g.lanes.x + g.lanes.w - 20.0, g.lanes.y + g.lanes.h - 20.0)
    };
    mv(rest, None, window, cx);
}
