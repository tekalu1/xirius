//! A completed Write lap, a stopped drag, and Touch's return through real controls.
use crate::{
    mixer::MixerHit,
    shot::{Act, Play, Shot, Target},
    studio::{DrawerTab, Studio},
    topbar::BarButton,
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};
use xirius_core::{auto::AutoTarget, id::TrackId, model::Project};
use xirius_design::Layout;

thread_local! { static BEFORE:RefCell<Option<Arc<Project>>> = const { RefCell::new(None) }; }
const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "modes01-write-loop",
        acts: &[
            Act::Reset,
            stage(0),
            Act::Press(Target::Track("Keys")),
            Act::Key("a"),
            Act::Press(Target::Top(BarButton::Loop)),
            Act::Press(Target::Top(BarButton::Play)),
            stage(1),
            Act::Press(Target::Top(BarButton::Stop)),
            stage(2),
        ],
    },
    Shot {
        name: "modes02-stopped-drag",
        acts: &[Act::Stage(Play::Window(drag), 0)],
    },
    Shot {
        name: "modes03-touch-return",
        acts: &[
            Act::Reset,
            stage(3),
            Act::Press(Target::Track("Keys")),
            Act::Key("a"),
            Act::Press(Target::Top(BarButton::Play)),
            stage(5),
            Act::Stage(Play::Window(drag), 1),
            Act::Press(Target::Top(BarButton::Stop)),
            stage(4),
        ],
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
fn setup(s: &mut Studio, touch: bool) {
    let t = track(s);
    // The mixer is shown here: pressing its tab again would shut the drawer (UI-015).
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    s.seek(0);
    s.run_command("track.volume.set", json!({"ids":[t],"db":0}));
    s.run_command(
        "auto.mode",
        json!({"ids":[t],"mode":if touch {"touch"}else{"write"}}),
    );
    if touch {
        for (at, value) in [(0, -24), (3840, -12), (7680, -24)] {
            s.run_command(
                "auto.point.add",
                json!({"track":t,"target":"volume","at":at,"value":value}),
            );
        }
    } else {
        s.run_command(
            "auto.point.add",
            json!({"track":t,"target":"volume","at":7680,"value":-30}),
        );
        s.run_command("loop.set", json!({"start":0,"end":1920}));
    }
    BEFORE.with(|p| *p.borrow_mut() = Some(s.project()));
    s.scroll.1 = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap().y;
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn prepare(s: &mut Studio, n: u8) {
    match n {
        0 => setup(s, false),
        1 => {
            let last = Cell::new(s.playhead);
            let laps = Cell::new(0);
            crate::shot::run_for(s, 60.0, |s| {
                if s.playhead < last.get() {
                    laps.set(laps.get() + 1);
                }
                last.set(s.playhead);
                laps.get() >= 2
            });
            assert!(laps.get() >= 2, "{:?}", s.error);
        }
        2 => {
            crate::shot::run_for(s, 60.0, |s| s.arrange.writing.is_none());
            let p = s.project();
            let lane = p.tracks[&track(s)]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Volume)
                .unwrap();
            assert!(
                lane.value_at(1500.0).unwrap().abs() < 1e-8,
                "{:?}",
                lane.points
            );
            assert!((lane.value_at(7680.0).unwrap() + 30.0).abs() < 1e-8);
        }
        3 => setup(s, true),
        4 => {
            crate::shot::run_for(s, 60.0, |s| s.arrange.writing.is_none());
            let p = s.project();
            let t = track(s);
            let lane = p.tracks[&t]
                .automation
                .iter()
                .find(|l| l.target == AutoTarget::Volume)
                .unwrap();
            BEFORE.with(|before| {
                let before = before.borrow();
                let old = &before.as_ref().unwrap().tracks[&t].automation[0];
                let edge = lane
                    .points
                    .iter()
                    .enumerate()
                    .filter(|(_, point)| !old.points.iter().any(|old| old.tick == point.tick))
                    .max_by_key(|(_, point)| point.tick)
                    .unwrap()
                    .0;
                let returned = &lane.points[edge - 1];
                assert!(
                    (returned.value - old.value_at(returned.tick as f64).unwrap()).abs() < 1e-8
                );
                assert_eq!(p.tracks[&t].volume_db, 0.0);
                assert!(
                    lane.points
                        .iter()
                        .any(|p| (p.value - old.value_at(p.tick as f64).unwrap()).abs() > 1.0),
                    "{:?}",
                    lane.points
                );
            });
        }
        5 => {
            crate::shot::run_for(s, 60.0, |s| s.arrange.writing.is_some());
            assert!(s.arrange.writing.is_some(), "{:?}", s.error);
        }
        _ => unreachable!(),
    }
}
fn drag(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let (x, y, before) = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        let (x, y) = s.mixer_center(&g, MixerHit::Fader(track(&s))).unwrap();
        (x, y, s.project())
    };
    let at = |dy| point(px(x), px(y + dy + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(0.0),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    for i in 1..=4 {
        window.dispatch_event(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at(i as f32 * 15.0),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        crate::shot::run_for(&mut studio.borrow_mut(), 0.06, |_| false);
    }
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(60.0),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
    let mut s = studio.borrow_mut();
    if n == 0 {
        let t = track(&s);
        let p = s.project();
        assert_eq!(p.tracks[&t].automation, before.tracks[&t].automation);
        assert!((p.tracks[&t].volume_db - before.tracks[&t].volume_db).abs() > 1.0);
    } else {
        crate::shot::run_for(&mut s, 0.25, |_| false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    #[test]
    fn looping_write_keeps_completed_laps_and_stopped_drag_does_not_write_a_lane() {
        let mut s = Studio::sample();
        s.transport.mode = xirius_session::output::OutputMode::Silent { paced: true };
        setup(&mut s, false);
        let before = s.project();
        s.toggle_loop();
        s.toggle_play();
        prepare(&mut s, 1);
        s.stop_transport();
        prepare(&mut s, 2);
        let written = s.project();
        let t = track(&s);
        let g = s.geometry(1600.0, 1000.0);
        let hit = MixerHit::Fader(t);
        let (_, y) = s.mixer_center(&g, hit).unwrap();
        assert!(s.drag_begin(&g, Hit::Mixer(hit), y));
        s.drag_move(y + 60.0, false);
        s.drag_end();
        assert_eq!(
            s.project().tracks[&t].automation,
            written.tracks[&t].automation
        );
        assert!((s.project().tracks[&t].volume_db - written.tracks[&t].volume_db).abs() > 1.0);
        assert!(s.song.undo());
        assert_eq!(s.project(), written);
        assert!(s.song.undo());
        assert_eq!(s.project(), before);
    }
}
