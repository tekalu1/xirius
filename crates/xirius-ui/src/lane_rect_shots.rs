//! KEY-016: a rectangle drawn in the empty part of a controller lane, as real pointer events on
//! the window. While the button is down the rubber band is drawn and the points inside it light;
//! on release they stay selected. The velocity lane takes the bars whose tips lie inside.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use xirius_core::model::CtrlKind;
use xirius_design::Layout;

use crate::roll::{Lane, LaneKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Studio};

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "rect01-modulation-lane",
        acts: &[Act::Reset, staged(0)],
    },
    Shot {
        name: "rect02-band-while-dragging",
        acts: &[staged(1)],
    },
    Shot {
        name: "rect03-points-selected",
        acts: &[staged(2)],
    },
    Shot {
        name: "rect04-velocity-tips",
        acts: &[staged(3)],
    },
];

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn moved(p: (f32, f32), held: bool, window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: held.then_some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(p: (f32, f32), window: &mut Window, cx: &mut App) {
    moved(p, false, window, cx);
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(p: (f32, f32), window: &mut Window, cx: &mut App) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// The corners of a rectangle over `from..to` ticks of lane `lane`, inside its plot.
fn corners(
    studio: &Rc<RefCell<Studio>>,
    window: &Window,
    lane: usize,
    from: i64,
    to: i64,
) -> Option<((f32, f32), (f32, f32))> {
    let s = studio.borrow();
    let g: Geometry = crate::screen::geometry(&s, window);
    let ctx = s.roll_ctx(&g)?;
    let plot = s.roll.frame(ctx.body).lanes.get(lane)?.plot;
    let v = s.roll.view(&ctx);
    Some(((v.x(from), plot.y + 2.0), (v.x(to), plot.bottom() - 2.0)))
}

pub(crate) fn play(stage: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if stage == 0 {
        let mut s = studio.borrow_mut();
        let g = crate::screen::geometry(&s, window);
        s.select_clip_of("Keys");
        s.drawer_open = true;
        s.tab = DrawerTab::Roll;
        s.resized(&g);
        let Some(ctx) = s.roll_ctx(&g) else {
            eprintln!("the roll is not open");
            return;
        };
        let clip = ctx.clip.id.to_string();
        let points: Vec<_> = [
            (0, 20),
            (480, 100),
            (960, 60),
            (1440, 110),
            (1920, 30),
            (2400, 80),
        ]
        .iter()
        .map(|(tick, value)| json!({ "tick": tick, "value": value }))
        .collect();
        s.song
            .run(
                "cc.replace",
                json!({ "clip": clip, "lane": "cc1", "from": 0, "to": 2400, "points": points }),
            )
            .expect("the lane takes its points");
        s.roll
            .lanes
            .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
        s.roll.active_lane = 1;
        s.roll.points.clear();
        return;
    }
    let lane = if stage == 3 { 0 } else { 1 };
    let Some((a, b)) = corners(studio, window, lane, 700, 1700) else {
        eprintln!("the roll is not open");
        return;
    };
    match stage {
        1 => {
            down(a, window, cx);
            moved(b, true, window, cx);
        }
        2 => up(b, window, cx),
        _ => {
            studio.borrow_mut().roll.active_lane = 0;
            let Some((a, b)) = corners(studio, window, 0, 700, 7000) else {
                return;
            };
            down(a, window, cx);
            moved(b, true, window, cx);
            up(b, window, cx);
        }
    }
}
