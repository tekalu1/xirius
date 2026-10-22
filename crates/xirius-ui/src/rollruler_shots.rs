//! `--script rollruler`: the piano roll's ruler (bar numbers, the loop band,
//! a click to seek, the playhead line) and the autoscroll switch (EDT-070),
//! played as real pointer events on the window. No OS input: the events go to
//! the window only, and the output is silent.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, Window, point, px,
};
use xirius_design::Layout;

use crate::shot::{Act, Shot};
use crate::studio::{Geometry, Hit, Rect, Studio};

pub const ROLL_RULER: &[Shot] = &[
    // The roll open on the sample song: bar numbers over the grid, the band
    // above it, the playhead at its start.
    Shot {
        name: "r01-roll-ruler",
        acts: &[],
    },
    // The autoscroll switch at the head of the arrangement column, pressed,
    // then the pointer moved away: it goes off, the notice line says so, and
    // the switch is no longer lit by the hover.
    Shot {
        name: "r02-follow-off",
        acts: &[Act::Reset, Act::RollRuler(2)],
    },
    // The switch in the roll's tool row, pressed after the head's: it is on
    // again. Both switches share one state.
    Shot {
        name: "r03-follow-on",
        acts: &[Act::Reset, Act::RollRuler(2), Act::RollRuler(3)],
    },
    // Playing with following on: the playhead, just inside the right edge,
    // brings the grid back to the left so that it sits at the edge.
    Shot {
        name: "r04-playing-follows",
        acts: &[Act::Reset, Act::RollRuler(4)],
    },
    // Playing with following off, from the left of the grid: the playhead
    // moves across the grid and the grid stays where it was.
    Shot {
        name: "r05-playing-no-follow",
        acts: &[Act::Reset, Act::RollRuler(5)],
    },
    // A press on the ruler below the loop band moves the playhead there.
    Shot {
        name: "r06-click-seeks",
        acts: &[Act::Reset, Act::RollRuler(6)],
    },
    // A drag on the loop band makes a loop: the lit stretch shades the grid.
    Shot {
        name: "r07-loop-dragged",
        acts: &[Act::Reset, Act::RollRuler(7)],
    },
    // The head switch pressed, then the pointer leaves the window without a
    // move: the hover goes too.
    Shot {
        name: "r08-head-switch-left-window",
        acts: &[Act::Reset, Act::RollRuler(8)],
    },
];

/// The part of the screen a stage aims at, read without holding the studio
/// while the events go out.
fn read<T>(
    studio: &Rc<RefCell<Studio>>,
    window: &Window,
    f: impl Fn(&Studio, &Geometry) -> T,
) -> T {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    f(&s, &g)
}

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y + Layout::TOP_BAR_H))
}

fn down(p: Point<Pixels>, clicks: usize) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: p,
        modifiers: Modifiers::default(),
        click_count: clicks,
        first_mouse: false,
    })
}

fn moved(p: Point<Pixels>, held: bool) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: p,
        pressed_button: held.then_some(MouseButton::Left),
        modifiers: Modifiers::default(),
    })
}

fn up(p: Point<Pixels>, clicks: usize) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: p,
        modifiers: Modifiers::default(),
        click_count: clicks,
    })
}

/// A point on the roll's ruler, `dx` and `dy` in from its top left.
fn ruler(studio: &Rc<RefCell<Studio>>, window: &Window, dx: f32, dy: f32) -> Option<(f32, f32)> {
    read(studio, window, |s, g| {
        let ctx = s.roll_ctx(g)?;
        let r = s.roll.frame(ctx.body).ruler;
        Some((r.x + dx, r.y + dy))
    })
}

/// Press at `from`, move to `to` in four steps with the button held, let go.
fn drag(from: (f32, f32), to: (f32, f32), window: &mut Window, cx: &mut App) {
    let start = at(from.0, from.1);
    window.dispatch_event(moved(start, false), cx);
    window.dispatch_event(down(start, 1), cx);
    let mut last = start;
    for i in 1..=4 {
        let t = i as f32 / 4.0;
        last = at(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        window.dispatch_event(moved(last, true), cx);
    }
    window.dispatch_event(up(last, 1), cx);
}

fn click(p: (f32, f32), window: &mut Window, cx: &mut App) {
    let p = at(p.0, p.1);
    window.dispatch_event(moved(p, false), cx);
    window.dispatch_event(down(p, 1), cx);
    window.dispatch_event(up(p, 1), cx);
}

/// The autoscroll switch at the head of the arrangement column. Returns the
/// point it was pressed at.
fn head_switch(
    studio: &Rc<RefCell<Studio>>,
    window: &mut Window,
    cx: &mut App,
) -> Option<(f32, f32)> {
    let p = read(studio, window, |s, g| s.center_of(g, Hit::Follow));
    if let Some(p) = p {
        click(p, window, cx);
    }
    p
}

/// The autoscroll switch in the roll's tool row.
fn roll_switch(studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let p = read(studio, window, |s, g| {
        let ctx = s.roll_ctx(g)?;
        let r = s.roll.frame(ctx.body).follow;
        Some((r.x + r.w / 2.0, r.y + r.h / 2.0))
    });
    if let Some(p) = p {
        click(p, window, cx);
    }
}

/// Moves the pointer to an empty part of the browser, away from the switches.
fn leave(window: &mut Window, cx: &mut App) {
    window.dispatch_event(moved(at(200.0, 500.0), false), cx);
}

/// The pointer leaves the window from where it was (no move comes first).
fn exit(p: (f32, f32), window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseExited(MouseExitEvent {
            position: at(p.0, p.1),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

/// Zooms the roll in by 2x, so that the sample clip is wider than the grid
/// and there is room to scroll. At the default zoom the clip fits the grid,
/// so the view has nothing to move to.
fn zoom_roll_in(studio: &Rc<RefCell<Studio>>) {
    let mut s = studio.borrow_mut();
    s.roll.beat_px = (s.roll.beat_px * 2.0).min(crate::roll::BEAT_PX_MAX);
}

/// Zooms in and puts the view back at the clip's start, then starts the song
/// 16 px short of the grid's right edge: the playhead is in the band where
/// following (TurnOver) moves the view on the first frame.
fn play_from_the_edge(studio: &Rc<RefCell<Studio>>, window: &Window) {
    play_from(studio, window, |grid| grid.right() - 16.0);
}

/// The same zoom and view, but the song starts 16 px in from the grid's left
/// edge, so the playhead moves across the grid while the view stays put.
fn play_from_the_left(studio: &Rc<RefCell<Studio>>, window: &Window) {
    play_from(studio, window, |grid| grid.x + 16.0);
}

/// Zooms in, puts the view back at the clip's start, and plays from the
/// window x that `at_px` gives for the grid. The view is put back first so
/// that the playhead starts inside it and not at the clip's end, where the
/// view is clamped.
fn play_from(studio: &Rc<RefCell<Studio>>, window: &Window, at_px: impl Fn(Rect) -> f32) {
    zoom_roll_in(studio);
    studio.borrow_mut().roll.scroll.0 = 0.0;
    let at = read(studio, window, |s, g| {
        let ctx = s.roll_ctx(g)?;
        let grid = s.roll.frame(ctx.body).grid;
        Some(s.roll.ruler_scale(&ctx).tick(at_px(grid)))
    });
    let Some(at) = at else { return };
    pause(studio);
    let mut s = studio.borrow_mut();
    s.seek(at);
    s.toggle_play();
}

fn pause(studio: &Rc<RefCell<Studio>>) {
    let mut s = studio.borrow_mut();
    if s.transport.playing {
        s.toggle_play();
    }
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match stage {
        // The arrangement's switch: following goes off, and the pointer leaves it.
        2 => {
            head_switch(studio, window, cx);
            leave(window, cx);
        }
        // The roll's switch: following goes on again, and the pointer leaves it.
        3 => {
            roll_switch(studio, window, cx);
            leave(window, cx);
        }
        // Following on: play from the edge; the view jumps back on the first frame.
        4 => play_from_the_edge(studio, window),
        // Following off: switch off, then play from the grid's left; the
        // playhead moves across it and nothing scrolls.
        5 => {
            head_switch(studio, window, cx);
            leave(window, cx);
            play_from_the_left(studio, window);
        }
        // Below the band (14 px from the ruler's top): a seek, no loop.
        6 => {
            pause(studio);
            if let Some(p) = ruler(studio, window, 300.0, 22.0) {
                click(p, window, cx);
            }
        }
        // On the band, away from any grip: a new loop from the press to the
        // release, snapped to the grid.
        7 => {
            pause(studio);
            let from = ruler(studio, window, 200.0, 6.0);
            let to = ruler(studio, window, 420.0, 6.0);
            if let (Some(from), Some(to)) = (from, to) {
                drag(from, to, window, cx);
            }
        }
        // The head switch, then the pointer leaves the window.
        8 => {
            if let Some(p) = head_switch(studio, window, cx) {
                exit(p, window, cx);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_its_own_name_and_a_known_number() {
        let mut names: Vec<_> = ROLL_RULER.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ROLL_RULER.len());
        for shot in ROLL_RULER {
            for act in shot.acts {
                if let Act::RollRuler(n) = act {
                    assert!(*n <= 8, "{act:?}");
                }
            }
        }
    }
}
