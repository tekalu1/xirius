//! PLG-055: a plugin whose process went down is shown only by the colour of the mark on its
//! insert row. The song goes on playing and can be edited, and no line of text says what
//! happened: no notice, no toast, no message, no menu. The plugin really goes down (the
//! fixture's own exit); the sound goes nowhere (`Silent`).
use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{Geometry, Studio};
use crate::topbar::BarButton;
use gpui::{App, Modifiers, MouseMoveEvent, PlatformInput, Window, point, px};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;
use xirius_design::Layout;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-down-while-playing",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Studio(crate::plugin_reload_shots::prepare), 0),
            Act::Press(Target::Top(BarButton::Play)),
            Act::Stage(Play::Window(away), 0),
            Act::Stage(Play::Studio(down), 0),
            Act::Stage(Play::Laid(quiet), 0),
        ],
    },
    Shot {
        name: "02-edited-while-down",
        acts: &[
            Act::Stage(Play::Studio(edit), 0),
            Act::Stage(Play::Laid(quiet), 0),
        ],
    },
];

fn vocal(s: &Studio) -> xirius_core::id::TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Vocal")
        .unwrap()
        .id
}

/// The pointer leaves the play button for the empty end of the mixer, so no tip is up.
fn away(_: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let at = {
        let s = studio.borrow();
        let g = crate::screen::geometry(&s, window);
        // Between the last channel and the master: no strip there.
        let body = g.drawer_body;
        let at = (body.x + body.w * 0.6, body.y + body.h / 2.0);
        assert!(
            s.mixer_strips(&g)
                .iter()
                .all(|x| !x.rect.contains(at.0, at.1))
        );
        (at.0, at.1 + Layout::TOP_BAR_H)
    };
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: point(px(at.0), px(at.1)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    assert_eq!(studio.borrow().hover, crate::studio::Hit::None);
}

/// Plays on for a while: the position moves on, with the plugin still down.
fn plays_on(s: &mut Studio) {
    let id = crate::plugin_reload_shots::id(s);
    let from = s.transport.tick_now().expect("a position");
    crate::shot::run_for(s, 15.0, |s| {
        s.transport.tick_now().is_some_and(|t| t > from + 960)
    });
    assert!(s.transport.playing);
    assert!(s.transport.tick_now().is_some_and(|t| t > from + 960));
    assert!(s.health.crashed(id));
}

pub(crate) fn down(s: &mut Studio, _: u8) {
    crate::plugin_crash_shots::crash(s, 0);
    plays_on(s);
}

/// The fader of the track whose plugin is down moves, while the song plays.
fn edit(s: &mut Studio, _: u8) {
    let t = vocal(s);
    assert!(s.run_command(
        "track.volume.set",
        json!({ "ids": [t.to_string()], "db": -6.0 })
    ));
    assert_eq!(s.project().tracks[&t].volume_db, -6.0);
    plays_on(s);
}

/// Nothing on the screen in words: the row's mark is all, and the row is in sight.
pub(crate) fn quiet(s: &mut Studio, g: &Geometry, _: u8) {
    assert!(s.notice.is_none());
    assert!(s.toast.is_none());
    assert!(s.error.is_none(), "{:?}", s.error);
    assert!(s.popup.is_none());
    let id = crate::plugin_reload_shots::id(s);
    assert!(s.mixer_center(g, MixerHit::Open(vocal(s), id)).is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_that_went_down_leaves_the_song_playing_and_editable_without_a_word() {
        let mut s = Studio::sample();
        crate::plugin_reload_shots::prepare(&mut s, 0);
        assert!(s.toggle_play());
        down(&mut s, 0);
        let g = s.geometry(1600.0, 900.0);
        quiet(&mut s, &g, 0);
        edit(&mut s, 0);
        quiet(&mut s, &g, 0);
        s.transport.pause();
    }
}
