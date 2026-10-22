//! The level of the input of an armed track (REC-002, REC-012): measured by the engine with the
//! transport stopped, and shown as a meter with the zone a good recording level lies in.
//!
//! The session hands over the loudest level of each input channel since the last frame. This
//! gives them the ballistics of the mixer's meters and picks the channels a track records.

use xirius_core::model::Track;
use xirius_engine::mixer::MeterReading;
use xirius_engine::record::MAX_INPUT_CHANNELS;
use xirius_session::record::input_channels;

use crate::mixer::meters::{Meter, db_to_t};

/// The quiet end of the zone a recording level should be in, in dB (peak).
pub const ZONE_LOW_DB: f32 = -18.0;
/// The loud end of the zone, in dB. Above it there is too little room left for a louder take.
pub const ZONE_HIGH_DB: f32 = -6.0;

/// Where the zone sits on the meter scale, 0..1.
pub fn zone() -> (f32, f32) {
    (db_to_t(ZONE_LOW_DB), db_to_t(ZONE_HIGH_DB))
}

/// Where a level on the meter scale sits against the zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Nothing is coming in.
    Silent,
    /// Quieter than the zone.
    Low,
    Good,
    /// Louder than the zone.
    High,
}

/// The meter of every input channel of the device.
#[derive(Clone, Debug, Default)]
pub struct InputMeters {
    chan: [Meter; MAX_INPUT_CHANNELS],
}

impl InputMeters {
    /// One frame: the loudest levels since the last frame go in. Gives true while any channel
    /// still shows something.
    pub fn feed(&mut self, levels: &[f32; MAX_INPUT_CHANNELS]) -> bool {
        let mut moving = false;
        for (m, level) in self.chan.iter_mut().zip(levels) {
            m.step(MeterReading {
                peak: [*level; 2],
                rms: [*level; 2],
            });
            moving |= m.level[0] > 0.0 || m.hold[0] > 0.0 || m.clipped(0);
        }
        moving
    }

    /// The meter of a track: its channels as left and right (one channel shows on both).
    /// `device` is how many channels the watched input has. A name that does not fit the input
    /// shows nothing.
    pub fn of(&self, track: &Track, device: usize) -> Meter {
        let Ok(ch) = input_channels(track.input.as_deref(), device) else {
            return Meter::default();
        };
        let at = |i: usize| self.chan.get(i).copied().unwrap_or_default();
        let (l, r) = match ch.as_slice() {
            [a, b] => (at(*a), at(*b)),
            [a] => (at(*a), at(*a)),
            _ => return Meter::default(),
        };
        Meter::showing(
            [l.level[0], r.level[0]],
            [l.rms[0], r.rms[0]],
            [l.hold[0], r.hold[0]],
        )
    }

    pub fn clear(&mut self) {
        self.chan = [Meter::default(); MAX_INPUT_CHANNELS];
    }
}

/// How the held peak of `m` sits against the zone.
pub fn fit(m: &Meter) -> Fit {
    fit_of(m.hold[0].max(m.hold[1]))
}

/// How a peak on the meter scale sits against the zone.
pub fn fit_of(top: f32) -> Fit {
    let (low, high) = zone();
    if top <= 0.0 {
        Fit::Silent
    } else if top < low {
        Fit::Low
    } else if top > high {
        Fit::High
    } else {
        Fit::Good
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::meters::level_t;
    use xirius_core::id::TrackId;
    use xirius_core::model::TrackKind;

    fn track(input: Option<&str>) -> Track {
        let mut t = Track::new(TrackId(1), TrackKind::Audio, "Vox");
        t.input = input.map(str::to_string);
        t
    }

    #[test]
    fn a_track_shows_the_channels_of_its_input_and_a_mono_input_on_both_sides() {
        let mut m = InputMeters::default();
        let mut levels = [0.0; MAX_INPUT_CHANNELS];
        levels[0] = 0.5;
        levels[1] = 0.1;
        m.feed(&levels);
        let pair = m.of(&track(None), 2);
        assert_eq!(pair.level, [level_t(0.5), level_t(0.1)]);
        let one = m.of(&track(Some("In 2")), 2);
        assert_eq!(one.level, [level_t(0.1); 2]);
        // An input the device does not have shows nothing.
        assert_eq!(m.of(&track(Some("In 9")), 2), Meter::default());
    }

    #[test]
    fn the_held_peak_is_measured_against_the_zone() {
        let mut m = InputMeters::default();
        let mut at = |linear: f32| {
            let mut levels = [0.0; MAX_INPUT_CHANNELS];
            levels[0] = linear;
            levels[1] = linear;
            m.clear();
            m.feed(&levels);
            fit(&m.of(&track(None), 2))
        };
        assert_eq!(at(0.0), Fit::Silent);
        assert_eq!(at(0.02), Fit::Low); // -34 dB
        assert_eq!(at(0.3), Fit::Good); // -10 dB
        assert_eq!(at(0.9), Fit::High); // -0.9 dB
    }

    #[test]
    fn a_level_falls_away_when_the_input_goes_quiet() {
        let mut m = InputMeters::default();
        let mut levels = [0.0; MAX_INPUT_CHANNELS];
        levels[0] = 0.5;
        assert!(m.feed(&levels));
        let mut frames = 0;
        while m.feed(&[0.0; MAX_INPUT_CHANNELS]) {
            frames += 1;
            assert!(frames < 1000);
        }
        assert!(frames > 10);
    }
}
