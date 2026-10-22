//! KEY-016: a drag up and down on the ruler zooms, through the offscreen window's pointer
//! handlers (the press, the moves and the release arrive as the window gets them).
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "rd01-rest",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "rd02-held-down-zoomed-in",
        acts: &[stage(1)],
    },
    Shot {
        name: "rd03-released",
        acts: &[stage(2)],
    },
    Shot {
        name: "rd04-dragged-up-zoomed-out",
        acts: &[stage(3)],
    },
];

fn at(x: f32, y: f32) -> gpui::Point<gpui::Pixels> {
    point(px(x), px(y + Layout::TOP_BAR_H))
}

fn moved(x: f32, y: f32, down: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(x, y),
            pressed_button: down.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(x: f32, y: f32, window: &mut Window, cx: &mut App) {
    moved(x, y, false, window, cx);
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: at(x, y),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(x: f32, y: f32, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: at(x, y),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// Where the press lands: on the ruler, under the loop band.
fn grip(studio: &Rc<RefCell<Studio>>, window: &Window) -> (f32, f32) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    (g.ruler.x + 360.0, g.ruler.y + g.ruler.h - 4.0)
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let (x, y) = grip(studio, window);
    match n {
        0 => {
            let s = studio.borrow();
            assert!(s.ruler_grab.is_none());
        }
        1 => {
            let before = studio.borrow().beat_w;
            down(x, y, window, cx);
            for step in 1..=5 {
                moved(x, y + 24.0 * step as f32, true, window, cx);
            }
            let s = studio.borrow();
            assert!(s.ruler_grab.as_ref().is_some_and(|g| g.started()));
            assert!(s.beat_w > before, "{} > {before}", s.beat_w);
        }
        2 => {
            up(x, y + 120.0, window, cx);
            let s = studio.borrow();
            assert!(s.ruler_grab.is_none());
        }
        _ => {
            let before = studio.borrow().beat_w;
            down(x, y, window, cx);
            for step in 1..=8 {
                moved(x, y - 8.0 * step as f32, true, window, cx);
            }
            up(x, y - 64.0, window, cx);
            let s = studio.borrow();
            assert!(s.beat_w < before, "{} < {before}", s.beat_w);
        }
    }
}
