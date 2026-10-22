//! `--script wheel`: wheel turns with Shift held, in the shape Windows sends them (sideways:
//! `Lines(x, 0)`, `gpui-pre-windows`'s `handle_mouse_wheel_msg`), through the real (parked,
//! silent) window's own input path (EDT-136). Each stage reports on stderr
//! (`wheel-check stage=N case=… ok=true`), which `tests/wheel_window.rs` reads.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseMoveEvent, PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase,
    Window, point, px,
};
use xirius_design::Layout;

use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Studio};

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "wheel-01-roll",
        acts: &[staged(0)],
    },
    Shot {
        name: "wheel-02-roll-ctrl-shift",
        acts: &[staged(1)],
    },
    Shot {
        name: "wheel-03-ruler-ctrl-shift",
        acts: &[staged(2)],
    },
];

/// One notch turned away from you with Ctrl and Shift held, as Windows sends it (three
/// characters, `wheel_scroll_chars`'s default).
fn ctrl_shift_notch(x: f32, y: f32, window: &mut Window, cx: &mut App) {
    let position = point(px(x), px(y + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Lines(point(3.0, 0.0)),
            modifiers: Modifiers {
                control: true,
                shift: true,
                ..Modifiers::default()
            },
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            s.drawer_open = true;
            s.tab = DrawerTab::Roll;
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
        }
        1 => {
            let (at, before) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let body = s.roll_ctx(&g).expect("the roll").body;
                (
                    (body.x + body.w / 2.0, body.y + body.h / 2.0),
                    s.roll.beat_px,
                )
            };
            ctrl_shift_notch(at.0, at.1, window, cx);
            let after = studio.borrow().roll.beat_px;
            let ok = after > before;
            eprintln!(
                "wheel-check stage=1 case=roll-ctrl-shift before={before} after={after} ok={ok}"
            );
        }
        2 => {
            let (at, before) = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                (
                    (g.ruler.x + g.ruler.w / 2.0, g.ruler.y + g.ruler.h / 2.0),
                    s.beat_w,
                )
            };
            ctrl_shift_notch(at.0, at.1, window, cx);
            let after = studio.borrow().beat_w;
            let ok = after > before;
            eprintln!(
                "wheel-check stage=2 case=ruler-ctrl-shift before={before} after={after} ok={ok}"
            );
        }
        _ => {}
    }
}
