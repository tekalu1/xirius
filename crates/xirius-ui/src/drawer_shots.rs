//! `--script drawer`: the drawer's top edge (UI-095) and the wheel and middle
//! button (EDT-068), played as real pointer events on the window. The edge's
//! cursor does not show in a window picture; the tests in `drawer_edge` hold it.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px, size,
};
use xirius_design::Layout;

use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Rect, Studio};

/// Stage `n` of this script, played by [`play`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const DRAWER: &[Shot] = &[
    Shot {
        name: "d01-opened",
        acts: &[],
    },
    Shot {
        name: "d02-edge-dragged-up",
        acts: &[staged(0)],
    },
    Shot {
        name: "d03-edge-at-most",
        acts: &[staged(1)],
    },
    Shot {
        name: "d04-edge-at-least",
        acts: &[staged(2)],
    },
    Shot {
        name: "d05-ctrl-wheel-key-rows",
        acts: &[staged(3)],
    },
    Shot {
        name: "d06-ctrl-wheel-track-rows",
        acts: &[staged(4)],
    },
    Shot {
        name: "d07-wheel-on-ruler-zooms",
        acts: &[staged(5)],
    },
    Shot {
        name: "d08-middle-drag-lanes",
        acts: &[staged(6)],
    },
    Shot {
        name: "d09-middle-drag-roll",
        acts: &[staged(7)],
    },
    Shot {
        name: "d10-double-click-default",
        acts: &[staged(8)],
    },
    Shot {
        name: "d11-edge-dragged-again",
        acts: &[staged(9)],
    },
    Shot {
        name: "d12-mixer-edge-dragged",
        acts: &[staged(10)],
    },
    Shot {
        name: "d13-small-window-mixer",
        acts: &[staged(11)],
    },
    Shot {
        name: "d14-small-window-roll",
        acts: &[staged(12)],
    },
    Shot {
        name: "d15-window-back",
        acts: &[staged(13)],
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

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn down(button: MouseButton, p: Point<Pixels>, clicks: usize) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position: p,
        modifiers: Modifiers::default(),
        click_count: clicks,
        first_mouse: false,
    })
}

fn moved(p: Point<Pixels>, held: Option<MouseButton>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: p,
        pressed_button: held,
        modifiers: Modifiers::default(),
    })
}

fn up(button: MouseButton, p: Point<Pixels>, clicks: usize) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position: p,
        modifiers: Modifiers::default(),
        click_count: clicks,
    })
}

/// Press at `from`, move to `to` in four steps with the button held, let go.
fn drag(button: MouseButton, from: (f32, f32), to: (f32, f32), window: &mut Window, cx: &mut App) {
    let start = at(from.0, from.1);
    window.dispatch_event(moved(start, None), cx);
    window.dispatch_event(down(button, start, 1), cx);
    let mut last = start;
    for i in 1..=4 {
        let t = i as f32 / 4.0;
        last = at(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        window.dispatch_event(moved(last, Some(button)), cx);
    }
    window.dispatch_event(up(button, last, 1), cx);
}

fn click(p: (f32, f32), clicks: usize, window: &mut Window, cx: &mut App) {
    let p = at(p.0, p.1);
    window.dispatch_event(moved(p, None), cx);
    for n in 1..=clicks {
        window.dispatch_event(down(MouseButton::Left, p, n), cx);
        window.dispatch_event(up(MouseButton::Left, p, n), cx);
    }
}

fn wheel(p: (f32, f32), dy: f32, ctrl: bool, window: &mut Window, cx: &mut App) {
    let p = at(p.0, p.1);
    window.dispatch_event(moved(p, None), cx);
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: p,
            delta: ScrollDelta::Pixels(point(px(0.0), px(dy))),
            modifiers: Modifiers {
                control: ctrl,
                ..Modifiers::default()
            },
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
}

/// Drags the drawer's top edge by `dy`, the pointer staying in the window (a
/// move outside it reaches no element).
fn edge(dy: f32, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let from = read(studio, window, |s, g| {
        let to = (2.0 - Layout::TOP_BAR_H, g.size.1 - 2.0);
        s.drawer_edge(g).map(|r| (mid(r), to))
    });
    match from {
        Some((from, (top, bottom))) => {
            let to = (from.1 + dy).clamp(top, bottom);
            drag(MouseButton::Left, from, (from.0, to), window, cx)
        }
        None => eprintln!("the drawer is shut"),
    }
}

/// A point on the roll's grid, `dx` and `dy` in from its top left.
fn grid(studio: &Rc<RefCell<Studio>>, window: &Window, dx: f32, dy: f32) -> Option<(f32, f32)> {
    read(studio, window, |s, g| {
        let ctx = s.roll_ctx(g)?;
        let grid = s.roll.frame(ctx.body).grid;
        Some((grid.x + dx.min(grid.w / 2.0), grid.y + dy.min(grid.h / 2.0)))
    })
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match stage {
        0 => edge(-200.0, studio, window, cx),
        1 => edge(-2000.0, studio, window, cx),
        2 => edge(2000.0, studio, window, cx),
        // Room for the roll again, then two notches of Ctrl + wheel on it.
        3 => {
            edge(-150.0, studio, window, cx);
            if let Some(p) = grid(studio, window, 300.0, 60.0) {
                wheel(p, 120.0, true, window, cx);
            }
        }
        4 => {
            let p = read(studio, window, |_, g| (g.lanes.x + 300.0, g.lanes.y + 30.0));
            wheel(p, 120.0, true, window, cx);
        }
        5 => {
            let p = read(studio, window, |_, g| (g.ruler.x + 200.0, mid(g.ruler).1));
            wheel(p, 180.0, false, window, cx);
        }
        6 => {
            let from = read(studio, window, |_, g| mid(g.lanes));
            drag(
                MouseButton::Middle,
                from,
                (from.0 - 300.0, from.1 - 60.0),
                window,
                cx,
            );
        }
        7 => {
            if let Some(from) = grid(studio, window, 300.0, 60.0) {
                drag(
                    MouseButton::Middle,
                    from,
                    (from.0 - 80.0, from.1 - 120.0),
                    window,
                    cx,
                );
            }
        }
        8 => {
            if let Some(p) = read(studio, window, |s, g| s.drawer_edge(g).map(mid)) {
                click(p, 2, window, cx);
            }
        }
        9 => edge(-150.0, studio, window, cx),
        10 => {
            let tab = read(studio, window, |_, g| mid(g.tab(DrawerTab::Mixer)));
            click(tab, 1, window, cx);
            edge(-40.0, studio, window, cx);
        }
        11 => window.resize(size(px(960.0), px(600.0))),
        12 => {
            let tab = read(studio, window, |_, g| mid(g.tab(DrawerTab::Roll)));
            click(tab, 1, window, cx);
        }
        13 => window.resize(size(px(1440.0), px(860.0))),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_has_its_own_name_and_a_known_number() {
        let mut names: Vec<_> = DRAWER.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), DRAWER.len());
        for shot in DRAWER {
            for act in shot.acts {
                assert!(matches!(act, Act::Stage(_, n) if *n <= 13), "{act:?}");
            }
        }
    }
}
