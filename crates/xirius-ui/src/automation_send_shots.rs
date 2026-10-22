//! Draw the send's points through the window's mouse path.
use crate::{
    arrange,
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    tools::Tool,
    topbar::BarButton,
    tree::RowKind,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_core::{auto::AutoTarget, id::TrackId};
use xirius_design::Layout;

pub const SHOTS: &[Shot] = &[Shot {
    name: "send01-points",
    acts: &[
        Act::Reset,
        Act::Stage(Play::Studio(prepare), 0),
        Act::Press(Target::Track("Keys")),
        Act::Key("a"),
        Act::Press(Target::Top(BarButton::Tool(Tool::Draw))),
        Act::Stage(Play::Window(draw), 0),
    ],
}];

fn track(s: &Studio) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id
}
fn target(s: &Studio) -> AutoTarget {
    AutoTarget::Send(s.project().tracks[&track(s)].sends.last().unwrap().id)
}
fn prepare(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    s.scroll.1 = 0.0;
    s.run_command("track.add.bus", json!({"name":"Send"}));
    let bus = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Send")
        .unwrap()
        .id;
    s.run_command("send.add", json!({"ids":[track(s)],"to":bus,"level":0}));
    s.run_command(
        "auto.lane.add",
        json!({"track":track(s),"target":target(s).to_string()}),
    );
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn positions(s: &Studio, g: &crate::studio::Geometry) -> Vec<(f32, f32)> {
    let t = track(s);
    let target = target(s);
    let tree = s.tree();
    let row = tree.row_of(RowKind::Track(t)).unwrap();
    let lane = s
        .arrange
        .lanes_of(&s.project().tracks[&t])
        .iter()
        .position(|l| l.target == target)
        .unwrap();
    let top = s.lane_top(g, row, lane);
    [(16320, -40.0), (19200, 0.0)]
        .iter()
        .map(|(tick, value)| {
            (
                g.lanes.x + s.ticks_to_px(*tick) - s.scroll.0,
                top + arrange::lane_value_y(&target, *value),
            )
        })
        .collect()
}
fn check(s: &Studio) {
    let project = s.project();
    let target = target(s);
    let lane = project.tracks[&track(s)]
        .automation
        .iter()
        .find(|l| l.target == target)
        .unwrap();
    assert_eq!(lane.points.len(), 2, "{:?}", s.error);
    assert_eq!(lane.points[0].tick, 16320);
    assert!((lane.points[0].value + 40.0).abs() < 0.01);
    assert_eq!(lane.points[1].tick, 19200);
    assert!(lane.points[1].value.abs() < 0.01);
}
fn draw(_: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let points = {
        let s = studio.borrow();
        positions(&s, &crate::screen::geometry(&s, window))
    };
    for (x, y) in points {
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
    check(&studio.borrow());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{studio::Hit, tools::Mods};
    #[test]
    fn the_send_lane_takes_the_draw_tools_points() {
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        s.toggle_lanes(&g, track(&s));
        s.press(&g, Hit::Bar(BarButton::Tool(Tool::Draw)));
        for (x, y) in positions(&s, &g) {
            assert!(s.arrange_down(&g, x, y, Mods::default()));
            s.arrange_up(&g, x, y, Mods::default());
        }
        check(&s);
    }
}
