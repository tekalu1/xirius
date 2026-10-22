//! Select all five kinds from the lane head's lists through the window.
use crate::{
    arrange::{LaneButton, LaneParts},
    popup::{Action, PopupKind},
    shot::{Act, Play, Shot, Target},
    studio::{Geometry, Studio},
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_core::{auto::AutoTarget, id::TrackId};
use xirius_design::Layout;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "targets01-list",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(setup), 0),
            Act::Press(Target::Track("Keys")),
            Act::Key("a"),
            Act::Stage(Play::Window(play), 0),
        ],
    },
    Shot {
        name: "targets02-lanes",
        acts: &[
            Act::Stage(Play::Window(play), 1),
            Act::Wheel {
                at: Target::Track("Keys"),
                dx: 0.0,
                dy: -400.0,
                ctrl: false,
            },
        ],
    },
    Shot {
        name: "targets03-change",
        acts: &[Act::Stage(Play::Window(play), 2)],
    },
    Shot {
        name: "targets04-volume",
        acts: &[Act::Stage(Play::Window(play), 3)],
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
fn setup(s: &mut Studio, _: u8) {
    let t = track(s);
    s.drawer_open = false;
    s.run_command("track.add.bus", json!({"name":"Room"}));
    let bus = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Room")
        .unwrap()
        .id;
    s.run_command("send.add", json!({"ids":[t],"to":bus,"level":0}));
    s.run_command("plugin.add", json!({"track":t,"name":"EQ"}));
    s.run_command(
        "auto.point.add",
        json!({"track":t,"target":"volume","at":15360,"value":-12}),
    );
    s.scroll.1 = s.tree().row_of(RowKind::Track(t)).unwrap().y;
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn head(s: &Studio, g: &Geometry, lane: usize, button: LaneButton) -> (f32, f32) {
    let tree = s.tree();
    let row = tree.row_of(RowKind::Track(track(s))).unwrap();
    let parts = LaneParts::of(row, lane == 0);
    let r = match button {
        LaneButton::Add => parts.add,
        LaneButton::Param => parts.param,
        LaneButton::Remove => parts.remove,
        _ => unreachable!(),
    };
    (
        g.column.x + r.x + r.w / 2.0,
        s.lane_top(g, row, lane) + r.y + r.h / 2.0,
    )
}
fn item(s: &Studio, g: &Geometry, to: &str) -> (f32, f32) {
    let cell = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .into_iter()
        .find(|c| matches!(&c.action, Some(Action::AutomationTarget(_, _, value)) if value == to))
        .unwrap();
    (
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    )
}
fn click(window: &mut Window, cx: &mut App, (x, y): (f32, f32)) {
    let position = point(px(x), px(y + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}
fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let coords = |window: &Window, lane, button| {
        let s = studio.borrow();
        head(&s, &crate::screen::geometry(&s, window), lane, button)
    };
    if n == 0 {
        let at = coords(window, 0, LaneButton::Add);
        click(window, cx, at);
        let s = studio.borrow();
        let p = s.popup.as_ref().unwrap();
        assert_eq!(p.kind, PopupKind::AutomationTargets(track(&s), false));
        assert!(p.choices.iter().any(|c| c.to.starts_with("param:")));
        assert!(p.choices.iter().any(|c| c.to.starts_with("send:")));
    } else if n == 1 {
        let targets = {
            let s = studio.borrow();
            let p = s.project();
            let t = &p.tracks[&track(&s)];
            vec![
                AutoTarget::Pan,
                AutoTarget::Mute,
                AutoTarget::Send(t.sends[0].id),
                crate::arrange::targets_of(t)
                    .into_iter()
                    .find(|t| matches!(t, AutoTarget::Param(..)))
                    .unwrap(),
            ]
        };
        for (i, target) in targets.iter().enumerate() {
            if i > 0 {
                let at = coords(window, 0, LaneButton::Add);
                click(window, cx, at);
            }
            let at = {
                let s = studio.borrow();
                item(
                    &s,
                    &crate::screen::geometry(&s, window),
                    &target.to_string(),
                )
            };
            click(window, cx, at);
        }
        let s = studio.borrow();
        let p = s.project();
        let lanes = s.arrange.lanes_of(&p.tracks[&track(&s)]);
        assert_eq!(lanes.len(), 5, "{:?}", s.error);
    } else if n == 2 {
        let at = coords(window, 1, LaneButton::Param);
        click(window, cx, at);
        let to = {
            let s = studio.borrow();
            let p = s.project();
            let t = &p.tracks[&track(&s)];
            crate::arrange::targets_of(t)
                .into_iter()
                .find(|target| {
                    matches!(target, AutoTarget::Param(..))
                        && !t.automation.iter().any(|l| l.target == *target)
                })
                .unwrap()
        };
        let at = {
            let s = studio.borrow();
            item(&s, &crate::screen::geometry(&s, window), &to.to_string())
        };
        click(window, cx, at);
        let s = studio.borrow();
        assert_eq!(s.project().tracks[&track(&s)].automation[1].target, to);
        assert!(s.error.is_none(), "{:?}", s.error);
    } else {
        let before = studio.borrow().project().tracks[&track(&studio.borrow())].automation[0]
            .points
            .clone();
        let at = coords(window, 0, LaneButton::Remove);
        click(window, cx, at);
        let at = coords(window, 0, LaneButton::Add);
        click(window, cx, at);
        let at = {
            let s = studio.borrow();
            item(&s, &crate::screen::geometry(&s, window), "volume")
        };
        click(window, cx, at);
        let s = studio.borrow();
        let p = s.project();
        let lane = p.tracks[&track(&s)]
            .automation
            .iter()
            .find(|l| l.target == AutoTarget::Volume)
            .unwrap();
        assert!(!lane.hidden);
        assert_eq!(lane.points, before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
        s.arrange_down(g, at.0, at.1, crate::tools::Mods::default());
        s.arrange_up(g, at.0, at.1, crate::tools::Mods::default());
    }
    fn pick(s: &mut Studio, g: &Geometry, to: &AutoTarget) {
        let at = item(s, g, &to.to_string());
        let hit = s.hit(g, at.0, at.1);
        assert!(s.press(g, hit));
    }
    #[test]
    fn plus_adds_each_kind_and_the_name_retargets_one_lane_in_one_step() {
        let mut s = Studio::sample();
        setup(&mut s, 0);
        let g = s.geometry(1680.0, 1000.0);
        let t = track(&s);
        s.toggle_lanes(&g, t);
        let targets = crate::arrange::targets_of(&s.project().tracks[&t]);
        let required = [
            AutoTarget::Pan,
            AutoTarget::Mute,
            targets
                .iter()
                .find(|t| matches!(t, AutoTarget::Send(..)))
                .unwrap()
                .clone(),
            targets
                .iter()
                .find(|t| matches!(t, AutoTarget::Param(..)))
                .unwrap()
                .clone(),
        ];
        for target in required {
            let at = head(&s, &g, 0, LaneButton::Add);
            press(&mut s, &g, at);
            pick(&mut s, &g, &target);
        }
        assert_eq!(s.arrange.lanes_of(&s.project().tracks[&t]).len(), 5);
        let before = s.project();
        let to = targets
            .into_iter()
            .find(|target| {
                matches!(target, AutoTarget::Param(..))
                    && !before.tracks[&t]
                        .automation
                        .iter()
                        .any(|l| l.target == *target)
            })
            .unwrap();
        let at = head(&s, &g, 1, LaneButton::Param);
        press(&mut s, &g, at);
        let popup = s.popup.as_ref().unwrap();
        assert_eq!(popup.current, "pan");
        assert!(popup.choices.iter().any(|c| c.to == "off:volume"));
        pick(&mut s, &g, &to);
        assert_eq!(s.project().tracks[&t].automation[1].target, to);
        assert!(s.song.undo());
        assert_eq!(s.project(), before);
        let volume = before.tracks[&t].automation[0].points.clone();
        s.lane_button(&g, t, &AutoTarget::Volume, LaneButton::Remove);
        let at = head(&s, &g, 0, LaneButton::Add);
        press(&mut s, &g, at);
        assert!(
            s.popup
                .as_ref()
                .unwrap()
                .choices
                .iter()
                .any(|c| c.to == "volume")
        );
        pick(&mut s, &g, &AutoTarget::Volume);
        assert_eq!(s.project().tracks[&t].automation[0].points, volume);
    }
}
