use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use xirius_design::Layout;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "tempo-edit01-before",
        acts: &[Act::Reset, Act::Stage(Play::Studio(prepare), 0)],
    },
    Shot {
        name: "tempo-edit02-drag",
        acts: &[Act::Stage(Play::Window(drag), 0)],
    },
    Shot {
        name: "tempo-edit03-before-point",
        acts: &[
            Act::Press(Target::Bar(6)),
            Act::Stage(Play::Studio(check_before), 0),
        ],
    },
    Shot {
        name: "tempo-edit04-playing",
        acts: &[
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Studio(played), 0),
        ],
    },
    Shot {
        name: "tempo-edit05-undo",
        acts: &[
            Act::Press(Target::Top(BarButton::Stop)),
            Act::Key("ctrl-z"),
            Act::Stage(Play::Studio(undone), 0),
        ],
    },
];
fn prepare(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    assert!(s.run_command("tempo.set", json!({"at":0,"bpm":120})));
    assert!(s.run_command("tempo.set", json!({"at":19200,"bpm":160})));
    assert!(s.run_command("view.snap", json!({"off":true})));
    s.beat_w = 100.0;
    s.scroll.0 = s.ticks_to_px(15360);
    s.seek(19200);
    assert_eq!(s.bar_view().bpm, "160.0");
}
fn drag(_: u8, studio: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let (x, y, dx, steps, id) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, w);
        let t = &s.project().tempo[1];
        let (x, y) = s.tempo_xy(&g, t.tick, t.bpm);
        (
            x,
            y,
            s.ticks_to_px(960),
            s.song.session().steps().len(),
            t.id,
        )
    };
    let at = |x: f32, y: f32| point(px(x), px(y + Layout::TOP_BAR_H));
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(x, y),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(x + dx, y - 20.0),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(x + dx, y - 20.0),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
    let s = studio.borrow();
    assert_eq!(s.song.session().steps().len(), steps + 1);
    assert_eq!(s.project().tempo[1].id, id);
    assert_eq!(
        (s.project().tempo[1].tick, s.project().tempo[1].bpm),
        (20160, 170.0)
    );
    assert_eq!(s.bar_view().bpm, "120.0");
}
fn check_before(s: &mut Studio, _: u8) {
    assert_eq!(s.bar_view().bpm, "120.0");
}
fn played(s: &mut Studio, _: u8) {
    crate::shot::run_for(s, 5.0, |s| s.playhead >= 21120);
    assert!(s.playhead >= 21120, "{:?}", s.error);
    assert_eq!(s.bar_view().bpm, "170.0");
}
fn undone(s: &mut Studio, _: u8) {
    assert_eq!(
        (s.project().tempo[1].tick, s.project().tempo[1].bpm),
        (19200, 160.0)
    );
    assert_eq!(s.bar_view().bpm, "160.0");
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{arrange::Preview, tools::Mods};
    fn gesture(s: &mut Studio, index: usize, delta: (f32, f32)) {
        let g = s.geometry(1600.0, 900.0);
        let t = s.project().tempo[index].clone();
        let (x, y) = s.tempo_xy(&g, t.tick, t.bpm);
        let mods = Mods::default();
        assert!(s.arrange_down(&g, x, y, mods));
        s.arrange_move(&g, x + delta.0, y + delta.1, mods);
        s.arrange_up(&g, x + delta.0, y + delta.1, mods);
    }
    #[test]
    fn a_one_axis_drag_preserves_the_other_axis_and_the_first_point_stays_put() {
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let id = s.project().tempo[1].id;
        assert!(s.run_command("tempo.move", json!({"id":id,"at":19213,"bpm":123.45})));
        let dx = s.ticks_to_px(960);
        gesture(&mut s, 1, (dx, 0.0));
        assert_eq!(
            (s.project().tempo[1].tick, s.project().tempo[1].bpm),
            (20173, 123.45)
        );
        assert!(s.run_command("view.snap", json!({"off":false})));
        gesture(&mut s, 1, (0.0, -20.0));
        assert_eq!(
            (s.project().tempo[1].tick, s.project().tempo[1].bpm),
            (20173, 133.0)
        );
        assert!(s.run_command("tempo.set", json!({"at":0,"bpm":123.45})));
        s.scroll.0 = 0.0;
        let steps = s.song.session().steps().len();
        gesture(&mut s, 0, (50.0, 0.0));
        assert_eq!(s.song.session().steps().len(), steps);
        assert_eq!(
            (s.project().tempo[0].tick, s.project().tempo[0].bpm),
            (0, 123.45)
        );
    }
    #[test]
    fn dragging_a_tempo_point_moves_its_time_and_bpm_in_one_step() {
        let mut s = Studio::sample();
        prepare(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        let t = s.project().tempo[1].clone();
        let steps = s.song.session().steps().len();
        let (x, y) = s.tempo_xy(&g, t.tick, t.bpm);
        let dx = s.ticks_to_px(960);
        let mods = Mods::default();
        assert!(s.arrange_down(&g, x, y, mods));
        assert!(s.arrange_move(&g, x + dx, y - 20.0, mods));
        let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
        assert!(matches!(
            plan.preview,
            Preview::Tempo {
                tick: 20160,
                bpm: 170.0,
                ..
            }
        ));
        assert!(s.arrange_up(&g, x + dx, y - 20.0, mods));
        assert_eq!(s.song.session().steps().len(), steps + 1);
        assert_eq!(s.project().tempo[1].id, t.id);
        assert_eq!(s.project().tempo[1].tick, 20160);
        assert_eq!(s.bar_view().bpm, "120.0");
        s.seek(20160);
        assert_eq!(s.bar_view().bpm, "170.0");
        assert!(s.undo(&g));
        assert_eq!(s.project().tempo[1], t);
        assert_eq!(s.bar_view().bpm, "160.0");
        assert!(s.run_command("tempo.set", json!({"at":0,"bpm":120,"ramp":true})));
        s.seek(9600);
        assert_eq!(s.bar_view().bpm, "140.0");
    }
}
