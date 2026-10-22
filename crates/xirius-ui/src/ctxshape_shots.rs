//! CTX-034: a controller point's "次のポイントまでの形" row, pressed through the offscreen window's
//! pointer handlers; the lane draws the stretch as a slanted line, and one undo takes it back.
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use std::cell::RefCell;
use std::rc::Rc;
use xirius_core::model::{CtrlKind, CtrlShape};
use xirius_design::Layout;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const CTXSHAPE: &[Shot] = &[
    Shot {
        name: "01-point-menu",
        acts: &[Act::Reset, stage(0), stage(1)],
    },
    Shot {
        name: "02-shape-rows",
        acts: &[stage(2)],
    },
    Shot {
        name: "03-line",
        acts: &[stage(3)],
    },
    Shot {
        name: "04-undone",
        acts: &[stage(4)],
    },
];

fn press(p: (f32, f32), button: MouseButton, window: &mut Window, cx: &mut App) {
    let p = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
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
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let layout = s.popup.as_ref().expect("menu").layout(g.size);
        let cell = layout
            .cells
            .iter()
            .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
            .unwrap_or_else(|| panic!("missing row {label}"));
        assert!(cell.live(), "disabled row {label}");
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    };
    press(at, MouseButton::Left, window, cx);
}

fn shapes(s: &Studio, window: &Window) -> Vec<CtrlShape> {
    let g = crate::screen::geometry(s, window);
    let c = s.roll_ctx(&g).expect("the roll is open");
    s.project()
        .clip(c.clip.id)
        .expect("the clip")
        .1
        .controllers
        .iter()
        .find(|l| l.kind == CtrlKind::Cc(1))
        .map(|l| l.points.iter().map(|p| p.shape).collect())
        .unwrap_or_default()
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            let c = s.roll_ctx(&g).expect("the roll is open");
            s.song
                .run(
                    "cc.replace",
                    serde_json::json!({ "clip": c.clip.id.to_string(), "lane": "cc1",
                        "from": 0, "to": 1920,
                        "points": [{ "tick": 0, "value": 10 }, { "tick": 480, "value": 90 },
                                   { "tick": 960, "value": 40 }] }),
                )
                .expect("the lane is written");
            s.resized(&g);
        }
        1 => {
            let at = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                let c = s.roll_ctx(&g).expect("the roll is open");
                let f = s.roll.frame(c.body);
                let v = s.roll.view(&c);
                let cc = s
                    .roll
                    .lanes
                    .iter()
                    .position(|l| l.kind == crate::roll::LaneKind::Ctrl(CtrlKind::Cc(1)))
                    .expect("the CC1 lane");
                (
                    v.x(480),
                    crate::roll::y_of(s.roll.lanes[cc].kind, &f.lanes[cc].plot, 90),
                )
            };
            press(at, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open(), "the press opens a menu");
        }
        2 => choose(studio, "次のポイントまでの形", window, cx),
        3 => {
            choose(studio, "直線", window, cx);
            assert_eq!(
                shapes(&studio.borrow(), window),
                [CtrlShape::Step, CtrlShape::Linear, CtrlShape::Step]
            );
        }
        _ => {
            let mut s = studio.borrow_mut();
            let g = crate::screen::geometry(&s, window);
            s.undo(&g);
            assert_eq!(shapes(&s, window), [CtrlShape::Step; 3]);
        }
    }
}
