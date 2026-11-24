//! The foot of the track column: one M and one S for every track (UI-376). Lit while any
//! track has the switch on, a press turns them all off; pressed again it puts back what it
//! turned off, until a switch is set by hand. Also which tracks only another track's solo
//! keeps quiet, whose M says so without being lit.
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Hit, Rect, Studio};
use std::collections::HashSet;
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_design::{FontSize, Radius, Weight};

/// The foot's height under the rows.
pub const FOOT_H: f32 = 32.0;
const BUTTON: f32 = 20.0;
const TOP: f32 = 6.0;
/// The buttons' left edges from the column's, under the rows' own M and S.
const MUTE_X: f32 = 40.0;
const SOLO_X: f32 = 63.0;

/// The switch a foot button stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Flag {
    Mute,
    Solo,
}

impl Flag {
    pub const ALL: [Flag; 2] = [Flag::Mute, Flag::Solo];

    fn label(self) -> &'static str {
        match self {
            Flag::Mute => "M",
            Flag::Solo => "S",
        }
    }

    /// The command a press runs: turn them off, or put back what was turned off.
    pub fn command(self, restore: bool) -> &'static str {
        match (self, restore) {
            (Flag::Mute, false) => "track.mute.clear",
            (Flag::Mute, true) => "track.mute.restore",
            (Flag::Solo, false) => "track.solo.clear",
            (Flag::Solo, true) => "track.solo.restore",
        }
    }
}

/// What a foot button shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Some track has the switch on: lit, a press turns them all off.
    On,
    /// None is on, and the last press turned some off: a press puts them back.
    Restore,
    /// Nothing to turn off or put back: drawn faint, and a press does nothing.
    Off,
}

pub fn state(p: &Project, flag: Flag) -> State {
    let on = p.tracks.values().any(|t| match flag {
        Flag::Mute => t.mute,
        Flag::Solo => t.solo,
    });
    let kept = match flag {
        Flag::Mute => &p.cleared.mute,
        Flag::Solo => &p.cleared.solo,
    };
    if on {
        State::On
    } else if kept.iter().any(|t| p.tracks.contains_key(t)) {
        State::Restore
    } else {
        State::Off
    }
}

/// The tracks only another track's solo keeps quiet: their own mute is off.
pub fn implied(p: &Project) -> HashSet<TrackId> {
    if !p.tracks.values().any(|t| t.solo) {
        return HashSet::new();
    }
    xirius_session::graph::silenced(p)
        .into_iter()
        .filter(|(t, quiet)| *quiet && p.tracks.get(t).is_some_and(|track| !track.mute))
        .map(|(t, _)| t)
        .collect()
}

/// The foot's band at the bottom of the column; none while the list is folded to a strip.
pub fn rect(g: &Geometry) -> Option<Rect> {
    (!g.panels.folded && g.column.h >= FOOT_H)
        .then(|| Rect::new(g.column.x, g.column.bottom() - FOOT_H, g.column.w, FOOT_H))
}

pub fn button(g: &Geometry, flag: Flag) -> Option<Rect> {
    let foot = rect(g)?;
    let x = match flag {
        Flag::Mute => MUTE_X,
        Flag::Solo => SOLO_X,
    };
    Some(Rect::new(foot.x + x, foot.y + TOP, BUTTON, BUTTON))
}

/// What is under a point of the foot: a button that does something, or nothing (the foot
/// keeps the rows under it from taking the press).
pub fn hit(studio: &Studio, g: &Geometry, x: f32, y: f32) -> Option<Hit> {
    if !rect(g)?.contains(x, y) {
        return None;
    }
    let p = studio.project();
    Some(
        Flag::ALL
            .into_iter()
            .find(|f| button(g, *f).is_some_and(|r| r.contains(x, y)))
            .and_then(|f| match state(&p, f) {
                State::On => Some(Hit::AllOff(f, false)),
                State::Restore => Some(Hit::AllOff(f, true)),
                State::Off => None,
            })
            .unwrap_or(Hit::None),
    )
}

impl Studio {
    pub(crate) fn all_off_press(&mut self, flag: Flag, restore: bool) -> bool {
        let name = flag.command(restore);
        // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
        self.command(|song| song.run(name, serde_json::json!({})).map(|_| ()))
    }
}

pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let Some(foot) = rect(g) else {
        return;
    };
    let c = pen.c;
    pen.round(foot, c.surface_2, Radius::S);
    let p = studio.project();
    for flag in Flag::ALL {
        let Some(r) = button(g, flag) else {
            continue;
        };
        let state = state(&p, flag);
        let hot = state != State::Off && matches!(studio.hover, Hit::AllOff(f, _) if f == flag);
        let lit = match flag {
            Flag::Mute => c.mute_on,
            Flag::Solo => c.solo_on,
        };
        let rest = match state {
            _ if hot => (Some(c.hover), c.ink_strong),
            State::Off => (None, c.ink_disabled),
            _ => (Some(c.surface_3), c.ink),
        };
        let (face, ink) = if state == State::On && !hot {
            (Some(lit.0), lit.1)
        } else {
            rest
        };
        if let Some(face) = face {
            pen.round(r, face, Radius::Clip);
        }
        pen.text(
            flag.label(),
            FontSize::Caption,
            Weight::Semibold,
            ink,
            r,
            Align::Center,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Studio, Geometry, Vec<TrackId>) {
        let s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        let ids: Vec<_> = s
            .project()
            .tracks
            .keys()
            .copied()
            .filter(|t| !t.is_master())
            .collect();
        (s, g, ids)
    }

    fn at(g: &Geometry, flag: Flag) -> (f32, f32) {
        let r = button(g, flag).expect("the foot is shown");
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn a_track_only_another_solo_keeps_quiet_is_implied_and_its_own_mute_is_not() {
        let (mut s, _, ids) = sample();
        assert!(implied(&s.project()).is_empty(), "no solo, nothing implied");
        s.song
            .run(
                "track.solo",
                serde_json::json!({ "ids": [ids[0].to_string()] }),
            )
            .unwrap();
        s.song
            .run(
                "track.mute",
                serde_json::json!({ "ids": [ids[1].to_string()] }),
            )
            .unwrap();
        let quiet = implied(&s.project());
        assert!(!quiet.contains(&ids[0]), "the soloed track is heard");
        assert!(
            !quiet.contains(&ids[1]),
            "a mute of its own is lit, not implied"
        );
        assert!(
            ids[2..].iter().any(|t| quiet.contains(t)),
            "the others are kept quiet by the solo: {quiet:?}"
        );
        assert!(
            quiet.iter().all(|t| !t.is_master()),
            "the master stays heard"
        );
    }

    #[test]
    fn the_foot_turns_every_solo_off_puts_them_back_and_then_has_nothing_to_do() {
        let (mut s, g, ids) = sample();
        assert_eq!(state(&s.project(), Flag::Solo), State::Off);
        let (x, y) = at(&g, Flag::Solo);
        assert_eq!(s.hit(&g, x, y), Hit::None, "nothing to turn off: no press");
        let two = serde_json::json!({ "ids": [ids[0].to_string(), ids[1].to_string()] });
        s.song.run("track.solo", two).unwrap();
        assert_eq!(state(&s.project(), Flag::Solo), State::On);
        assert_eq!(s.hit(&g, x, y), Hit::AllOff(Flag::Solo, false));
        assert!(s.press(&g, Hit::AllOff(Flag::Solo, false)));
        assert!(s.project().tracks.values().all(|t| !t.solo));
        assert_eq!(state(&s.project(), Flag::Solo), State::Restore);
        assert_eq!(s.hit(&g, x, y), Hit::AllOff(Flag::Solo, true));
        assert!(s.press(&g, Hit::AllOff(Flag::Solo, true)));
        let back: Vec<_> = s
            .project()
            .tracks
            .values()
            .filter(|t| t.solo)
            .map(|t| t.id)
            .collect();
        assert_eq!(back.len(), 2);
        assert!(back.contains(&ids[0]) && back.contains(&ids[1]));
        assert_eq!(state(&s.project(), Flag::Solo), State::On);
    }

    #[test]
    fn a_mute_set_by_hand_forgets_what_the_foot_turned_off() {
        let (mut s, g, ids) = sample();
        s.song
            .run(
                "track.mute",
                serde_json::json!({ "ids": [ids[0].to_string()] }),
            )
            .unwrap();
        assert!(s.press(&g, Hit::AllOff(Flag::Mute, false)));
        assert_eq!(state(&s.project(), Flag::Mute), State::Restore);
        assert!(s.press(&g, Hit::Mute(ids[1])));
        assert!(s.press(&g, Hit::Mute(ids[1])));
        assert_eq!(state(&s.project(), Flag::Mute), State::Off);
        let (x, y) = at(&g, Flag::Mute);
        assert_eq!(s.hit(&g, x, y), Hit::None);
    }

    #[test]
    fn the_foot_stands_at_the_bottom_of_the_column_and_keeps_the_rows_under_it() {
        let (s, mut g, _) = sample();
        let foot = rect(&g).unwrap();
        assert_eq!(foot.bottom(), g.column.bottom());
        assert_eq!(foot.h, FOOT_H);
        let m = button(&g, Flag::Mute).unwrap();
        let so = button(&g, Flag::Solo).unwrap();
        assert_eq!(
            (m.x - g.column.x, m.y - foot.y, m.w, m.h),
            (40.0, 6.0, 20.0, 20.0)
        );
        assert_eq!((so.x - g.column.x, so.y - foot.y), (63.0, 6.0));
        // A point of the foot off its buttons is no row's.
        assert_eq!(s.hit(&g, foot.right() - 10.0, foot.y + 16.0), Hit::None);
        // The last row can scroll up out from under it.
        let before = s.max_scroll(&g).1;
        g.panels.folded = true;
        assert!(rect(&g).is_none());
        assert_eq!(s.max_scroll(&g).1, (before - FOOT_H).max(0.0));
    }
}
