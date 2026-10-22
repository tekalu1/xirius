//! `--script history-time`: the history lists each step with the time it was made (PRJ-011).
//! The list is opened through the window's own input path (no OS input).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use serde_json::json;
use xirius_design::Layout;

use crate::popup::{Look, PopupKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use crate::topbar::BarButton;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // Three steps, newest first, each with its time on the right; the opening state with none.
    Shot {
        name: "h01-history-times",
        acts: &[Act::Reset, stage(0)],
    },
];

fn press(p: (f32, f32), window: &mut Window, cx: &mut App) {
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
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n != 0 {
        return;
    }
    {
        let mut s = studio.borrow_mut();
        let ids: Vec<String> = s
            .project()
            .tracks
            .values()
            .filter(|t| t.kind != xirius_core::model::TrackKind::Master)
            .take(2)
            .map(|t| t.id.to_string())
            .collect();
        assert!(s.run_command("track.mute", json!({ "ids": [ids[0]] })));
        assert!(s.run_command("track.solo", json!({ "ids": [ids[1]] })));
        assert!(s.run_command("track.volume.set", json!({ "ids": [ids[0]], "db": -3.0 })));
    }
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        s.center_of(&g, Hit::Bar(BarButton::History))
            .expect("the history button is on the bar")
    };
    press(at, window, cx);
    let s = studio.borrow();
    let popup = s.popup.as_ref().expect("the history is open");
    assert_eq!(popup.kind, PopupKind::History);
    let g = crate::screen::geometry(&s, window);
    let hints: Vec<String> = popup
        .layout(g.size)
        .cells
        .iter()
        .filter(|c| matches!(c.look, Look::Step { .. }))
        .map(|c| c.hint.clone())
        .collect();
    assert_eq!(hints.len(), 4, "three steps and the opening state");
    assert!(hints[..3].iter().all(|h| h.contains(':')), "{hints:?}");
    assert!(hints[3].is_empty(), "the opening state has no time");
}
