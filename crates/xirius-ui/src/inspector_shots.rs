//! `--script inspector` (UI-312): the inspector opened from the rail, a track's transpose and
//! velocity marked on its head, and the inspector with nothing chosen. Silent; nothing takes the
//! PC's focus. The song is the sample's.

use serde_json::json;
use xirius_core::id::TrackId;

use crate::arrange::probe::track;
use crate::browser::RailTab;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{Geometry, Studio};

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

const fn inspector() -> Act {
    Act::Press(Target::Rail(RailTab::Inspector))
}

pub const SHOTS: &[Shot] = &[
    // The chosen track and its clip, with a transpose and a velocity of the track's own.
    Shot {
        name: "in01-inspector",
        acts: &[
            Act::Reset,
            laid(0),
            Act::Press(Target::Clip("Keys")),
            inspector(),
        ],
    },
    // The same values marked on the track's head, the panel shut.
    Shot {
        name: "in02-track-mark",
        acts: &[
            Act::Reset,
            laid(0),
            Act::Press(Target::Clip("Keys")),
            inspector(),
            inspector(),
        ],
    },
    // Nothing chosen: the face stays empty (UI-312).
    Shot {
        name: "in03-empty",
        acts: &[Act::Reset, laid(1), inspector()],
    },
    // The track alone: a press on its head takes the clip's part away (UI-313).
    Shot {
        name: "in04-track-only",
        acts: &[
            Act::Reset,
            laid(0),
            Act::Press(Target::Clip("Keys")),
            inspector(),
            Act::Press(Target::Track("Keys")),
        ],
    },
];

fn keys(s: &Studio) -> TrackId {
    track(s, "Keys")
}

/// Plays stage `n` of [`SHOTS`] on the screen's state.
fn stage(s: &mut Studio, _g: &Geometry, n: u8) {
    match n {
        0 => {
            let t = keys(s);
            s.arrange_run(
                "track.transpose",
                json!({ "ids": [t.to_string()], "semis": -12 }),
            );
            s.arrange_run(
                "track.velocity",
                json!({ "ids": [t.to_string()], "percent": 10 }),
            );
        }
        1 => {
            s.selected_row = None;
            s.selected_clip = None;
        }
        _ => unreachable!("inspector stage {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_is_listed() {
        let listed = crate::SCRIPTS.iter().find(|(name, _)| *name == "inspector");
        let names = |shots: &[Shot]| shots.iter().map(|s| s.name).collect::<Vec<_>>();
        assert_eq!(listed.map(|(_, s)| names(s)), Some(names(SHOTS)));
    }

    /// The first shot's song has the values the head's mark and the inspector show.
    #[test]
    fn the_values_are_the_ones_the_shot_is_named_after() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        stage(&mut s, &g, 0);
        let t = &s.project().tracks[&keys(&s)];
        assert_eq!((t.transpose, t.velocity), (-12, 10));
        assert_eq!(crate::draw::track_mark(t).as_deref(), Some("−12 +10%"));
        // The form the head chooses is the whole one too, not the one-value form (UI-312).
        let row = *s
            .tree()
            .row_of(crate::tree::RowKind::Track(keys(&s)))
            .expect("the row");
        let mark = s.row_mark(&row, t);
        assert_eq!(mark.text.as_deref(), Some("−12 +10%"));
    }

    /// The rail's button opens the inspector, and the panel shows the chosen track's two fields
    /// and its clip's.
    #[test]
    fn the_first_shot_opens_the_inspector_on_the_chosen_track_and_clip() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 900.0);
        stage(&mut s, &g, 0);
        let (x, y) = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
        s.arrange_down(&g, x, y, crate::tools::Mods::NONE);
        s.arrange_up(&g, x, y, crate::tools::Mods::NONE);
        s.press(&g, crate::studio::Hit::Rail(RailTab::Inspector));
        assert!(s.inspector_open());
        let l = s.inspector_layout(&g);
        assert_eq!(
            l.rows.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [
                crate::context::fields::Kind::TrackSemis,
                crate::context::fields::Kind::TrackVelocity,
                crate::context::fields::Kind::Semis
            ]
        );
        // A press on the track's head leaves the clip's part out (UI-313).
        s.press(
            &g,
            crate::studio::Hit::Row(crate::tree::RowKind::Track(keys(&s))),
        );
        assert!(s.inspector_layout(&g).clip_header.is_none());
        assert_eq!(
            s.inspector_layout(&g)
                .rows
                .iter()
                .map(|r| r.kind)
                .collect::<Vec<_>>(),
            [
                crate::context::fields::Kind::TrackSemis,
                crate::context::fields::Kind::TrackVelocity
            ]
        );
    }
}
