//! A run of shots for the one line of an operation (`xirius-ui --shot <dir> --script opline`):
//! the detail opened from a history row for a person's operation, the AI's, and a change of
//! the AI's permission. Each stage is an [`Act::Stage`] played here.

use serde_json::json;
use xirius_core::AiPermission;
use xirius_core::session::RunOptions;

use crate::popup::Action;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::topbar::BarButton;

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const OPLINE: &[Shot] = &[
    // A person's operation: `$ xirius track.mute …`, what it changed, the undo unit.
    Shot {
        name: "o01-person",
        acts: &[Act::Reset, staged(0), staged(1)],
    },
    // The AI's operation at the same place, with the same lines.
    Shot {
        name: "o02-ai",
        acts: &[staged(2)],
    },
    // A change of the AI's permission is a row too, with no undo unit.
    Shot {
        name: "o03-permission",
        acts: &[staged(3)],
    },
];

/// The step the song made last.
fn last_step(s: &Studio) -> u64 {
    s.song.steps() as u64
}

/// The detail of `n` toggled, the history left open.
fn detail(s: &mut Studio, g: &Geometry, n: u64) {
    let open = s.popup.as_ref().and_then(|p| p.open_step);
    if open != Some(n) {
        s.popup_action(g, Action::StepInfo(n));
    }
}

/// Plays stage `stage` of [`OPLINE`].
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    s.drawer_open = false;
    let g = &s.geometry(g.size.0, g.size.1);
    s.resized(g);
    match stage {
        // A person mutes a track, the AI is allowed to edit and moves the notes.
        0 => {
            let drums = s
                .project()
                .tracks
                .values()
                .find(|t| t.name == "Drums")
                .map(|t| t.id);
            if let Some(t) = drums {
                s.press(g, Hit::Mute(t));
            }
            s.ai_act(crate::ai::Act::Level(AiPermission::Edit));
            if let Some(clip) = s.roll_target().map(|t| t.clip.id.to_string()) {
                let raw = json!({"ids": [clip], "by": 2});
                let raw = raw.as_object().cloned().unwrap_or_default();
                if let Err(e) = s.ai_run("note.transpose", &raw, &RunOptions::ai("AI"), false) {
                    eprintln!("note.transpose: {e}");
                }
            }
            s.popup = None;
        }
        1 => {
            s.press(g, Hit::Bar(BarButton::History));
            // The person's step is the first after the opening state.
            detail(s, g, 1);
        }
        2 => {
            detail(s, g, last_step(s));
        }
        3 => {
            let note = s.song.history_notes().first().map(|i| i.n);
            if let Some(n) = note {
                detail(s, g, n);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stages_open_each_detail_in_turn() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        stage(&mut s, &g, 0);
        stage(&mut s, &g, 1);
        let p = s.popup.as_ref().expect("the history");
        assert_eq!(p.open_step, Some(1));
        stage(&mut s, &g, 2);
        let last = s.song.steps() as u64;
        assert_eq!(s.popup.as_ref().and_then(|p| p.open_step), Some(last));
        stage(&mut s, &g, 3);
        let n = s.popup.as_ref().and_then(|p| p.open_step).unwrap();
        assert!(n >= crate::song::NOTE_N);
    }
}
