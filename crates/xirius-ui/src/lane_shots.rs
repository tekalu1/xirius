//! `--script lanes`: the tabs over the roll's controller lanes, pressed as real pointer events
//! on the window. Pressing "Sus" has to show something even when the song has no sustain
//! values: the tab and the lane's head are marked, a folded lane opens, and an empty lane says
//! what it holds and how to add a point.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_core::model::CtrlKind;
use xirius_design::Layout;

use crate::roll::{Lane, LaneKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Rect, Studio};

/// Stage `n` of this script, played by [`play`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const LANES: &[Shot] = &[
    // The velocity lane at work, the sustain lane folded under it.
    Shot {
        name: "l01-sus-folded",
        acts: &[Act::Reset, staged(0)],
    },
    // "Sus" pressed: its tab is lit, its lane opens with a lit head, and the empty lane says so.
    Shot {
        name: "l02-sus-tab-pressed",
        acts: &[staged(1)],
    },
    // Two clicks with the pencil: the pedal down between them, drawn as a band with steps.
    Shot {
        name: "l03-sus-pedal-drawn",
        acts: &[staged(2)],
    },
    // "Vel" pressed again: the mark moves back, the pedal stays drawn.
    Shot {
        name: "l04-vel-tab-pressed",
        acts: &[staged(3)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The roll's frame as the window sees it now.
fn frame(
    studio: &Rc<RefCell<Studio>>,
    window: &Window,
) -> Option<(crate::roll::Frame, crate::roll::View)> {
    let s = studio.borrow();
    let g: Geometry = crate::screen::geometry(&s, window);
    let ctx = s.roll_ctx(&g)?;
    let f = s.roll.frame(ctx.body);
    let v = s.roll.view(&ctx);
    Some((f, v))
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

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if stage == 0 {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        s.select_clip_of("Keys");
        s.drawer_open = true;
        s.tab = DrawerTab::Roll;
        s.resized(&g);
        let mut sus = Lane::new(LaneKind::Ctrl(CtrlKind::Cc(64)));
        sus.folded = true;
        s.roll.lanes.push(sus);
        s.roll.active_lane = 0;
        return;
    }
    let Some((f, v)) = frame(studio, window) else {
        eprintln!("the roll is not open");
        return;
    };
    match stage {
        1 => click(mid(f.tabs[1]), window, cx),
        2 => {
            let plot = f.lanes[1].plot;
            click((v.x(960), plot.y + 6.0), window, cx);
            click((v.x(2880), plot.bottom() - 6.0), window, cx);
        }
        3 => click(mid(f.tabs[0]), window, cx),
        _ => {}
    }
}
