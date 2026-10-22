//! `--script toast-hover`: the toast at the foot of the lanes with its button, and the pointer
//! on the button and on the close (EDT-110, EXP-048). Silent; no OS input.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, Modifiers, MouseMoveEvent, PlatformInput, Window, point, px};
use xirius_design::Layout;

use crate::keymap_shots::read;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::toast::{Answer, FROZEN, ToastPart};

const OFFER: Act = Act::Stage(Play::Laid(offer), 0);

pub const SHOTS: &[Shot] = &[
    // The toast as it comes, the pointer away from it.
    Shot {
        name: "th01-toast",
        acts: &[Act::Reset, OFFER],
    },
    // The pointer on the button: the button's face lights.
    Shot {
        name: "th02-button",
        acts: &[Act::Reset, OFFER, Act::Stage(Play::Window(over), 0)],
    },
    // The pointer on the close.
    Shot {
        name: "th03-close",
        acts: &[Act::Reset, OFFER, Act::Stage(Play::Window(over), 1)],
    },
];

fn offer(s: &mut Studio, _: &Geometry, _: u8) {
    s.offer(
        FROZEN.into(),
        "フリーズを解除",
        Answer::Unfreeze(Vec::new()),
    );
}

fn over(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let part = if n == 0 {
        ToastPart::Button
    } else {
        ToastPart::Close
    };
    let Some(at) = read(studio, window, |s, g| s.center_of(g, Hit::Toast(part))) else {
        return;
    };
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(at.0), px(at.1 + Layout::TOP_BAR_H)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}
