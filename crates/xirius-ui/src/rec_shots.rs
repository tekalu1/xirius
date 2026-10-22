//! `--script rec`: the recording area under a track, pressed as real pointer events on the
//! window. Three takes of a passage hang under their track; a bar pressed in a take row plays
//! that take there; the Comp row folds the rows; arming a track shows its input level and gain,
//! and the monitor button makes the input heard; record pressed with nothing armed rings the arm
//! button. Nothing opens a device: the input is the
//! built-in mock and the output is silent.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use xirius_core::id::TrackId;
use xirius_core::time::Tick;
use xirius_design::Layout;

use crate::rec_area::{COMP_H, RecArea};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::topbar::BarButton;
use crate::tree::RowKind;

const BAR: Tick = 3840;

/// Stage `n` of [`REC`], played through the window.
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const REC: &[Shot] = &[
    // Three takes of two bars, under the Vocal track: the Comp row and a row each.
    Shot {
        name: "r01-three-takes",
        acts: &[Act::Reset, staged(0)],
    },
    // The second bar of the first take pressed: it plays there, and its bar lights.
    Shot {
        name: "r02-bar-of-take-1-pressed",
        acts: &[staged(1)],
    },
    // The first bar of the second take pressed: a comp of two takes.
    Shot {
        name: "r03-bar-of-take-2-pressed",
        acts: &[staged(2)],
    },
    // The Comp row pressed: the take rows fold away.
    Shot {
        name: "r04-comp-folded",
        acts: &[staged(3)],
    },
    // Pressed again: the rows come back.
    Shot {
        name: "r05-comp-unfolded",
        acts: &[staged(4)],
    },
    // Vocal armed: its input level moves and the gain slider shows under it.
    Shot {
        name: "r06-armed-input",
        acts: &[staged(5)],
    },
    // The monitor button pressed: it is lit and the input is on the output.
    Shot {
        name: "r07-monitor-on",
        acts: &[staged(6)],
    },
    // Nothing armed and the record button pressed: it says so and the arm button is ringed.
    Shot {
        name: "r08-record-with-nothing-armed",
        acts: &[Act::Reset, staged(7)],
    },
    // Recording with the Vocal input turned 24 dB up: the take's level is over the frame, and
    // that is said beside the meter. The input is the test tone.
    Shot {
        name: "r09-recording-outside-the-frame",
        acts: &[Act::Reset, staged(8), staged(9)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn click(p: (f32, f32), window: &mut Window, cx: &mut App) {
    let p = at(p);
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: p,
            modifiers: Modifiers::default(),
            click_count: 1,
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

/// Where the middle of bar `bar` (from the first take's start) of take row `j` is.
fn bar_at(s: &Studio, g: &Geometry, t: TrackId, bar: Tick, j: usize) -> Option<(f32, f32)> {
    let row = *s.tree().row_of(RowKind::Track(t))?;
    let area = RecArea::of(&s.project().tracks[&t], s.takes_open.contains(&t));
    let start = s.project().time_map().bar_start(40);
    Some((
        s.tick_x(g, start + bar * BAR + BAR / 2),
        s.row_y(g, &row) + (row.rec_y() - row.y) + area.take_y(j) + area.take_h() / 2.0,
    ))
}

/// Scrolls so that the track's rows and the takes are on screen.
fn show(s: &mut Studio, t: TrackId) {
    if let Some(row) = s.tree().row_of(RowKind::Track(t)).copied() {
        s.scroll.1 = row.y;
    }
    let start = s.project().time_map().bar_start(40);
    s.scroll.0 = (s.ticks_to_px(start) - 40.0).max(0.0);
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let (g, t) = {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        let Some(t) = vocal(&s) else {
            eprintln!("no Vocal track");
            return;
        };
        if stage == 0 {
            let start = s.project().time_map().bar_start(40);
            let takes: Vec<_> = (1..=3)
                .map(|i| {
                    json!({"track": t.to_string(), "start": start, "length": 2 * BAR,
                           "file": format!("Recordings/Vocal-{i}.wav")})
                })
                .collect();
            s.drawer_open = false;
            s.resized(&g);
            s.command(|song| {
                song.run("record.take.add", json!({ "takes": takes }))
                    .map(|_| ())
            });
            show(&mut s, t);
            return;
        }
        if stage == 8 {
            let dir = std::env::temp_dir().join("xirius-rec-shots");
            let _ = std::fs::remove_dir_all(&dir);
            s.command(|song| song.save_as(&dir));
            let bar = s.project().time_map().bar_start(3);
            s.seek(bar);
            s.command(|song| song.toggle_arm(t));
            s.command(|song| {
                song.run(
                    "track.input.gain",
                    json!({ "ids": [t.to_string()], "db": 24.0 }),
                )
                .map(|_| ())
            });
            show(&mut s, t);
            return;
        }
        show(&mut s, t);
        (g, t)
    };
    let s = studio.borrow();
    let area = |s: &Studio| RecArea::of(&s.project().tracks[&t], s.takes_open.contains(&t));
    let comp = |s: &Studio| {
        let row = *s.tree().row_of(RowKind::Track(t))?;
        let a = area(s);
        Some((
            g.column.x + row.content_x() + 6.0,
            s.row_y(&g, &row) + (row.rec_y() - row.y) + a.comp_y()? + COMP_H / 2.0,
        ))
    };
    let p = match stage {
        1 => bar_at(&s, &g, t, 1, 0),
        2 => bar_at(&s, &g, t, 0, 1),
        3 | 4 => comp(&s),
        5 => s.center_of(&g, Hit::Arm(t)),
        6 => s.center_of(&g, Hit::Monitor(t)),
        7 | 9 => s.center_of(&g, Hit::Bar(BarButton::Record)),
        _ => None,
    };
    drop(s);
    match p {
        Some(p) => click(p, window, cx),
        None => eprintln!("rec stage {stage}: nothing to press"),
    }
    // Recording runs a while, so that the meter has a take's worth of level to judge.
    if stage == 9 {
        crate::shot::run_for(&mut studio.borrow_mut(), 1.5, |_| false);
    }
}
