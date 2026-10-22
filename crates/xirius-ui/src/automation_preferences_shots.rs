use crate::{
    settings::{Do, Section},
    shot::{Act, Play, Shot, Target},
    studio::{Geometry, Studio},
};
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, PlatformInput, ScrollDelta,
    ScrollWheelEvent, TouchPhase, Window, point, px,
};
use std::{cell::RefCell, path::PathBuf, rc::Rc};
use xirius_design::Layout;
const FOLLOW: &str = "edit.automation.follows-clips";
const LOCK: &str = "edit.automation.locks-clips";
pub const SHOTS: &[Shot] = &[
    Shot {
        name: "prefs01-on",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Track("Keys")),
            Act::Key("a"),
            Act::Stage(Play::Window(gear), 0),
            Act::Stage(Play::Window(edit), 0),
        ],
    },
    Shot {
        name: "prefs02-off",
        acts: &[Act::Stage(Play::Window(change), 0)],
    },
    Shot {
        name: "prefs03-reopen",
        acts: &[
            Act::Stage(Play::Studio(reopen), 0),
            Act::Stage(Play::Window(gear), 0),
            Act::Stage(Play::Window(edit), 0),
        ],
    },
];
fn dir() -> PathBuf {
    std::env::current_dir()
        .unwrap()
        .join("target")
        .join(format!("automation-prefs-native-{}", std::process::id()))
}
fn prepare(s: &mut Studio, _: u8) {
    s.drawer_open = false;
    s.song.load_settings(&dir());
    s.settings_reload();
    assert!(s.arrange.follows_clips && s.arrange.locks_clips);
}
fn visible(s: &Studio, g: &Geometry, act: &Do) -> Option<(f32, f32)> {
    let scene = s.settings_scene(g);
    scene.items.iter().filter(|i| i.act == *act).find_map(|i| {
        let p = (i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0);
        (scene.item_at(p.0, p.1).map(|i| &i.act) == Some(act)).then_some(p)
    })
}
pub(crate) fn aim(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, act: &Do) -> (f32, f32) {
    for _ in 0..40 {
        let s = s.borrow();
        let g = crate::screen::geometry(&s, w);
        if let Some(p) = visible(&s, &g, act) {
            return p;
        }
        let scene = s.settings_scene(&g);
        let item = scene.find(act).expect("settings item");
        let y = item.r.y + item.r.h / 2.0;
        let dy = if y > scene.body.bottom() {
            -100.0
        } else {
            100.0
        };
        let pos = point(
            px(scene.body.x + scene.body.w / 2.0),
            px(scene.body.y + scene.body.h / 2.0 + Layout::TOP_BAR_H),
        );
        drop(s);
        w.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: pos,
                delta: ScrollDelta::Pixels(point(px(0.0), px(dy))),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    }
    panic!("settings item is off screen: {act:?}");
}
fn click(s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App, act: Do) {
    let (x, y) = aim(s, w, cx, &act);
    press_at(x, y, w, cx);
}
fn gear(_: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    let (x, y) = {
        let s = s.borrow();
        s.center_of(
            &crate::screen::geometry(&s, w),
            crate::studio::Hit::RailSettings,
        )
        .unwrap()
    };
    press_at(x, y, w, cx);
}
fn press_at(x: f32, y: f32, w: &mut Window, cx: &mut App) {
    let position = point(px(x), px(y + Layout::TOP_BAR_H));
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}
fn edit(_: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    click(s, w, cx, Do::Nav(Section::Edit));
    let on = s.borrow().arrange.follows_clips;
    aim(s, w, cx, &Do::Pref(FOLLOW.into(), (!on).to_string()));
    aim(s, w, cx, &Do::Pref(LOCK.into(), (!on).to_string()));
}
fn change(_: u8, s: &Rc<RefCell<Studio>>, w: &mut Window, cx: &mut App) {
    click(s, w, cx, Do::Pref(FOLLOW.into(), "false".into()));
    click(s, w, cx, Do::Pref(LOCK.into(), "false".into()));
    let mut s = s.borrow_mut();
    s.settings_reload();
    assert!(!s.arrange.follows_clips && !s.arrange.locks_clips);
    assert!(dir().join("prefs.toml").is_file());
}
fn reopen(s: &mut Studio, _: u8) {
    s.close_settings();
    s.song.load_settings(&dir());
    s.settings_reload();
    assert!(!s.arrange.follows_clips && !s.arrange.locks_clips);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        arrange::{Spot, probe},
        tools::Mods,
        tree::RowKind,
    };
    use xirius_core::auto::AutoTarget;
    fn press(s: &mut Studio, g: &Geometry, key: &str, on: bool) {
        let act = Do::Pref(key.into(), on.to_string());
        for _ in 0..40 {
            if let Some((x, y)) = visible(s, g, &act) {
                assert!(s.settings_press(g, x, y));
                s.settings_reload();
                return;
            }
            let scene = s.settings_scene(g);
            let r = scene.find(&act).expect("switch").r;
            let dy = if r.y + r.h / 2.0 > scene.body.bottom() {
                -60.0
            } else {
                60.0
            };
            s.settings_wheel(
                g,
                scene.body.x + scene.body.w / 2.0,
                scene.body.y + scene.body.h / 2.0,
                dy,
            );
        }
        panic!("switch not visible: {key}={on}");
    }
    fn move_clip(s: &mut Studio, g: &Geometry) -> i64 {
        let from = probe::clip_mid(s, g, "Bass", 0);
        let to = (from.0 + s.ticks_to_px(3840), from.1);
        probe::hold(s, g, from, to, Mods::default());
        s.arrange_up(g, to.0, to.1, Mods::default());
        3840
    }
    #[test]
    fn switches_persist_and_control_the_clip_drag_after_a_fresh_launch() {
        let path = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!("automation-prefs-test-{}", std::process::id()));
        let mut s = Studio::sample();
        s.song.load_settings(&path);
        s.settings_reload();
        // The clip the test drags selects its track; the instrument-input arm
        // that would follow is not what this test checks (REC-024).
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let g = s.geometry(1600.0, 900.0);
        let t = probe::two_lanes(&mut s, &g, "Bass");
        let original = s.project();
        let from = probe::clip_mid(&s, &g, "Bass", 0);
        assert!(s.arrange.follows_clips && s.arrange.locks_clips);
        assert!(!matches!(
            s.arrange_spot(&g, from.0, from.1),
            Spot::Clip { .. }
        ));
        s.open_settings(Section::Edit);
        press(&mut s, &g, LOCK, false);
        assert!(!s.arrange.locks_clips);
        assert!(s.arrange.follows_clips);
        s.close_settings();
        let by = move_clip(&mut s, &g);
        assert_eq!(
            s.project().tracks[&t].clips[0].start,
            original.tracks[&t].clips[0].start + by
        );
        let points = s.project().tracks[&t]
            .automation
            .iter()
            .find(|l| l.target == AutoTarget::Volume)
            .unwrap()
            .points
            .clone();
        assert!(
            points
                .iter()
                .any(|p| p.tick == original.tracks[&t].automation[0].points[0].tick + by)
        );
        assert!(s.undo(&g));
        assert_eq!(s.project(), original);
        s.open_settings(Section::Edit);
        press(&mut s, &g, FOLLOW, false);
        s.close_settings();
        move_clip(&mut s, &g);
        assert_eq!(
            s.project().tracks[&t].automation,
            original.tracks[&t].automation
        );
        assert!(s.undo(&g));
        drop(s);
        let mut s = Studio::sample();
        s.song.load_settings(&path);
        s.settings_reload();
        assert!(!s.arrange.follows_clips && !s.arrange.locks_clips);
        let g = s.geometry(1600.0, 900.0);
        let t = probe::two_lanes(&mut s, &g, "Bass");
        let original = s.project();
        move_clip(&mut s, &g);
        assert_eq!(
            s.project().tracks[&t].automation,
            original.tracks[&t].automation
        );
        assert!(s.undo(&g));
        s.open_settings(Section::Edit);
        press(&mut s, &g, FOLLOW, true);
        press(&mut s, &g, LOCK, true);
        s.close_settings();
        assert!(s.arrange.follows_clips && s.arrange.locks_clips);
        let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
        assert!(row.lanes > 0);
        let from = probe::clip_mid(&s, &g, "Bass", 0);
        assert!(!matches!(
            s.arrange_spot(&g, from.0, from.1),
            Spot::Clip { .. }
        ));
        let _ = std::fs::remove_dir_all(path);
    }
}
