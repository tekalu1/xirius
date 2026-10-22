//! `--script caption`: the window's minimise, maximise and close buttons at rest and under the
//! pointer (UI-005). The pointer is moved through the window's own input path, so the hover is
//! the buttons' own and no OS input is sent.

use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use gpui::{App, Modifiers, MouseMoveEvent, PlatformInput, Window, point, px};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const CAPTION: &[Shot] = &[
    Shot {
        name: "caption-01-rest",
        acts: &[],
    },
    Shot {
        name: "caption-02-hover-minimise",
        acts: &[staged(0)],
    },
    Shot {
        name: "caption-03-hover-maximise",
        acts: &[staged(1)],
    },
    Shot {
        name: "caption-04-hover-close",
        acts: &[staged(2)],
    },
];

/// Stage `n` puts the pointer on the middle of button `n` (0 minimise, 1 maximise, 2 close).
fn play(n: u8, _studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let width = f32::from(window.viewport_size().width);
    let x = width - Layout::CAPTION_BUTTON_W * (2.5 - f32::from(n));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x), px(Layout::TOP_BAR_H / 2.0)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}
