use crate::{
    arrange::{self, Preview},
    shot::{Act, Play, Shot, Target},
    studio::{Geometry, Studio},
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    ScrollDelta, ScrollWheelEvent, TouchPhase, Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_core::{
    auto::{AutoLane, AutoTarget},
    id::TrackId,
};
use xirius_design::Layout;
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "point01-push",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(setup), 0),
            Act::Press(Target::Track("Keys")),
            Act::Key("a"),
            Act::Stage(Play::Window(drag), 0),
        ],
    },
    Shot {
        name: "point02-return",
        acts: &[Act::Stage(Play::Window(drag), 1)],
    },
    Shot {
        name: "point03-wheel",
        acts: &[Act::Stage(Play::Window(wheel), 0)],
    },
    Shot {
        name: "point04-time",
        acts: &[
            Act::Key("alt-right"),
            Act::Stage(Play::Studio(check_nudge), 0),
        ],
    },
    Shot {
        name: "point05-time-back",
        acts: &[
            Act::Key("alt-left"),
            Act::Stage(Play::Studio(check_back), 0),
        ],
    },
    Shot {
        name: "point06-axis",
        acts: &[Act::Stage(Play::Window(precise), 0)],
    },
    Shot {
        name: "point07-fine",
        acts: &[Act::Stage(Play::Window(precise), 1)],
    },
];
fn track(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id
}
fn lane(s: &Studio) -> AutoLane {
    s.project().tracks[&track(s)]
        .automation
        .iter()
        .find(|l| l.target == AutoTarget::Volume)
        .unwrap()
        .clone()
}
fn setup(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    s.run_command("auto.lane.add", json!({"track":track(s),"target":"volume"}));
    for (at, value) in [(15360, -24.0), (17280, -12.0), (19200, -6.0)] {
        s.run_command(
            "auto.point.add",
            json!({"track":track(s),"target":"volume","at":at,"value":value}),
        );
    }
    s.scroll.1 = s.tree().row_of(RowKind::Track(track(s))).unwrap().y;
}
fn position(s: &Studio, g: &Geometry) -> (f32, f32) {
    let l = lane(s);
    let p = l.points[1];
    let tree = s.tree();
    let row = tree.row_of(RowKind::Track(track(s))).unwrap();
    (
        g.lanes.x + s.ticks_to_px(p.tick) - s.scroll.0,
        s.lane_top(g, row, 0) + arrange::lane_value_y(&l.target, p.value),
    )
}
fn moved(x: f32, y: f32, button: Option<MouseButton>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position: point(px(x), px(y + Layout::TOP_BAR_H)),
        pressed_button: button,
        modifiers: Modifiers::default(),
    })
}
fn drag(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let (x, y, dx) = {
        let s = s.borrow();
        let g = crate::screen::geometry(&s, w);
        let (x, y) = position(&s, &g);
        (x, y, s.ticks_to_px(3840))
    };
    if n == 0 {
        w.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y + Layout::TOP_BAR_H)),
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    }
    w.dispatch_event(
        moved(
            x + if n == 0 { dx } else { 0.0 },
            y,
            Some(MouseButton::Left),
        ),
        cx,
    );
    let state = s.borrow();
    let g = crate::screen::geometry(&state, w);
    let plan = state.plan(&g, state.arrange.drag.as_ref().unwrap());
    let mut preview = lane(&state);
    if let Some(plan) = plan {
        if let Preview::Point {
            index, tick, value, ..
        } = plan.preview
        {
            preview.push_point(index, tick, value);
        }
    } else {
        assert_eq!(n, 1);
    }
    assert_eq!(preview.points[2].tick, if n == 0 { 21121 } else { 19200 });
    drop(state);
    if n == 1 {
        w.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: point(px(x), px(y + Layout::TOP_BAR_H)),
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
        assert_eq!(lane(&s.borrow()).points[2].tick, 19200);
    }
}
fn wheel(_: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let (x, y) = {
        let s = s.borrow();
        position(&s, &crate::screen::geometry(&s, w))
    };
    w.dispatch_event(moved(x, y, None), cx);
    w.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: point(px(x), px(y + Layout::TOP_BAR_H)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(40.0))),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::default()
            },
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
    let s = s.borrow();
    assert!(lane(&s).points[1].value > -12.0);
    assert_eq!(lane(&s).points[1].tick, 17280);
}
fn check_nudge(s: &mut Studio, _: u8) {
    assert!(lane(s).points[1].tick > 17280);
    assert!(s.error.is_none(), "{:?}", s.error);
}

fn check_back(s: &mut Studio, _: u8) {
    assert_eq!(lane(s).points[1].tick, 17280);
}

fn precise(n: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let (x, y, original) = {
        let s = s.borrow();
        let (x, y) = position(&s, &crate::screen::geometry(&s, w));
        (x, y, lane(&s).points[1])
    };
    let (dx, dy, modifiers) = if n == 0 {
        (
            60.0,
            -3.0,
            Modifiers {
                control: true,
                ..Modifiers::default()
            },
        )
    } else {
        (
            0.0,
            -20.0,
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        )
    };
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(x), px(y + Layout::TOP_BAR_H)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(x + dx), px(y + dy + Layout::TOP_BAR_H)),
            pressed_button: Some(MouseButton::Left),
            modifiers,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(x + dx), px(y + dy + Layout::TOP_BAR_H)),
            modifiers,
            click_count: 1,
        }),
        cx,
    );
    let s = s.borrow();
    let p = lane(&s).points[1];
    if n == 0 {
        assert!(p.tick > original.tick);
        assert_eq!(p.value, original.value);
    } else {
        assert_eq!(p.tick, original.tick);
        assert!(p.value > original.value && p.value - original.value < 6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Mods;
    #[test]
    fn push_return_axis_fine_wheel_and_mapped_nudge() {
        let mut s = Studio::sample();
        // Undo here must land exactly on the state the test kept; the
        // instrument-input arm that would follow the selection is not
        // what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        setup(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        s.toggle_lanes(&g, track(&s));
        let (x, y) = position(&s, &g);
        let dx = s.ticks_to_px(3840);
        let original = s.project();
        s.arrange_down(&g, x, y, Mods::default());
        s.arrange_move(&g, x + dx, y, Mods::default());
        let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
        assert!(matches!(plan.preview, Preview::Point { tick: 21120, .. }));
        assert_eq!(s.project(), original);
        s.arrange_move(&g, x + dx / 4.0, y, Mods::default());
        s.arrange_up(&g, x + dx / 4.0, y, Mods::default());
        assert_eq!(lane(&s).points[1].tick, 18240);
        assert_eq!(lane(&s).points[2].tick, 19200);
        assert!(s.song.undo());
        s.after_edit();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        s.arrange_down(&g, x, y, Mods::default());
        s.arrange_move(&g, x + dx, y - 3.0, ctrl);
        assert!(matches!(
            s.plan(&g, s.arrange.drag.as_ref().unwrap())
                .unwrap()
                .preview,
            Preview::Point { value: -12.0, .. }
        ));
        s.arrange_key(&g, "escape", Mods::default());
        let fine = Mods {
            shift: true,
            ..Mods::default()
        };
        s.arrange_down(&g, x, y, Mods::default());
        s.arrange_move(&g, x, y - 20.0, fine);
        s.arrange_up(&g, x, y - 20.0, fine);
        assert!(lane(&s).points[1].value > -12.0 && lane(&s).points[1].value < -6.0);
        assert_eq!(lane(&s).points[1].tick, 17280);
        let (x, y) = position(&s, &g);
        s.arrange.focus = false;
        s.input_map.browser_focus = true;
        assert!(s.arrange_wheel(
            &g,
            x,
            y,
            1.0,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ));
        let before = s.project();
        assert!(s.arrange.focus);
        assert!(!s.input_map.browser_focus);
        assert!(s.mapped_key(
            &g,
            "right",
            None,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ));
        check_nudge(&mut s, 0);
        assert!(s.undo(&g));
        assert_eq!(s.project(), before);
        assert!(s.redo(&g));
        assert!(s.undo(&g));
        assert!(s.mapped_key(
            &g,
            "right",
            None,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ));
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(s.undo(&g));
        s.command(|song| song.set_snap(false));
        let before = lane(&s).points[1].tick;
        assert!(s.mapped_key(
            &g,
            "right",
            None,
            Mods {
                alt: true,
                ..Mods::default()
            }
        ));
        assert_eq!(lane(&s).points[1].tick, before + 2);
    }
}
