//! A fader or a pan knob of a selected track moves every selected track (MIX-050).
//!
//! Each track moves by as much as the dragged one, so the differences between them stay. A track
//! that reaches the end of its range stops there alone, and its difference comes back when the
//! drag turns around, since every value is measured from where its track was when the drag began.
//! A fader is -∞ at its floor and below: a track taken there is silent, the dragged fader at the
//! bottom takes every track there, and a track at -∞ stays, since silence has no difference to
//! keep (a drag of a fader at -∞ moves its track alone).
//! Alt at the press moves the dragged track alone, as does a press on a track outside the
//! selection. Let go, the move is one `--by` command: one step of the history.

use xirius_core::id::TrackId;
use xirius_core::model::{FADER_FLOOR_DB, MAX_DB, MIN_DB, Project, pan_moved, volume_moved};

use super::{Drag, MixerHit};
use crate::studio::Studio;

impl Drag {
    /// How far the dragged control moved from where it began: what every selected track moves by.
    /// A fader at the bottom is -∞, so it moves the others by all the range: to -∞ too.
    pub(crate) fn by(&self) -> f64 {
        match self.hit {
            MixerHit::Fader(_) if self.value <= FADER_FLOOR_DB => MIN_DB - MAX_DB,
            _ => self.value - self.start,
        }
    }

    /// The dragged fader began at -∞: there is no difference to keep, so it moves alone.
    fn starts_silent(&self) -> bool {
        matches!(self.hit, MixerHit::Fader(_)) && self.start <= FADER_FLOOR_DB
    }
}

/// `hit`'s control on the track `t`.
pub(crate) fn same_on(hit: MixerHit, t: TrackId) -> MixerHit {
    match hit {
        MixerHit::Fader(_) => MixerHit::Fader(t),
        MixerHit::Pan(_) => MixerHit::Pan(t),
        _ => hit,
    }
}

/// Where a track at `now` goes when moved by `by`: where `--by` takes it.
pub(crate) fn moved(hit: MixerHit, now: f64, by: f64) -> f64 {
    match hit {
        MixerHit::Pan(_) => pan_moved(now, by),
        _ => volume_moved(now, by),
    }
}

/// The value of `hit`'s fader or pan knob in `p`.
pub(crate) fn level_of(p: &Project, hit: MixerHit) -> Option<f64> {
    let t = p.tracks.get(&hit.track())?;
    match hit {
        MixerHit::Fader(_) => Some(t.volume_db),
        MixerHit::Pan(_) => Some(t.pan),
        _ => None,
    }
}

impl Studio {
    /// The tracks a press or a drag of `hit` changes: for a fader or a pan knob of a selected
    /// track, every selected one (the pressed first); with Alt, from -∞, or for any other
    /// control, the pressed one alone.
    pub(crate) fn drag_tracks(&self, hit: MixerHit) -> Vec<TrackId> {
        let alone = self
            .mixer
            .drag
            .is_some_and(|d| d.hit == hit && (d.alone || d.starts_silent()));
        if alone || !matches!(hit, MixerHit::Fader(_) | MixerHit::Pan(_)) {
            vec![hit.track()]
        } else {
            self.track_targets(hit.track())
        }
    }

    /// The drag in progress moves several tracks, each from its own value.
    pub(crate) fn drag_relative(&self) -> bool {
        self.mixer
            .drag
            .is_some_and(|d| self.drag_tracks(d.hit).len() > 1)
    }

    /// The value the drag in progress gives `hit`, a control like the dragged one on one of its
    /// tracks, when it moves from `now`: `value` itself unless the drag moves several tracks.
    pub(crate) fn dragged_to(&self, hit: MixerHit, now: Option<f64>, value: f64) -> f64 {
        match (self.mixer.drag, now) {
            (Some(d), Some(now)) if self.drag_relative() => moved(hit, now, d.by()),
            _ => value,
        }
    }
}

#[cfg(test)]
#[path = "group_tests.rs"]
mod tests;
