//! The strips' level meters (UI-009, UI-024, UI-038): what the engine measured,
//! handed over without a lock by the session ([`SessionHost::take_levels`]),
//! turned into what the screen draws each frame.
//!
//! A meter rises at once and falls slowly; the peak line holds for a moment,
//! then falls; the clip light stays on for a second after the signal went
//! over full scale.
//!
//! [`SessionHost::take_levels`]: xirius_session::host::SessionHost::take_levels

use std::collections::BTreeMap;

use xirius_engine::mixer::MeterReading;
use xirius_session::host::Strip;

/// The bottom of the fader and the meter scale, in dB.
pub const FLOOR_DB: f32 = xirius_core::model::FADER_FLOOR_DB as f32;
/// The top of the fader scale, in dB.
pub const CEIL_DB: f32 = 6.0;

/// Where a level in dB sits on the fader and meter scale, 0..1. The scale is
/// squared so the top 20 dB, where mixing happens, take most of the height.
pub fn db_to_t(db: f32) -> f32 {
    if db <= FLOOR_DB {
        return 0.0;
    }
    ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB))
        .clamp(0.0, 1.0)
        .powi(2)
}

/// The level in dB at a point of the scale, to a tenth of a dB.
pub fn t_to_db(t: f32) -> f32 {
    if t <= 0.0005 {
        return FLOOR_DB;
    }
    let db = t.clamp(0.0, 1.0).sqrt() * (CEIL_DB - FLOOR_DB) + FLOOR_DB;
    (db * 10.0).round() / 10.0
}

/// Where a linear level (1.0 is full scale) sits on the scale.
pub fn level_t(linear: f32) -> f32 {
    if linear <= 0.0 {
        return 0.0;
    }
    db_to_t(20.0 * linear.log10())
}

/// How far a meter falls in a frame.
const FALL: f32 = 0.025;
/// How far the held peak falls in a frame once its hold is over.
const PEAK_FALL: f32 = 0.006;
/// Frames the peak line holds still.
const HOLD_FRAMES: u16 = 38;
/// Frames the clip light stays on.
const CLIP_FRAMES: u16 = 60;

/// One strip's meter, both channels, on the 0..1 scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meter {
    pub level: [f32; 2],
    pub rms: [f32; 2],
    /// The held peak line.
    pub hold: [f32; 2],
    hold_left: [u16; 2],
    clip_left: [u16; 2],
}

impl Meter {
    /// The clip light of a channel is on.
    pub fn clipped(&self, ch: usize) -> bool {
        self.clip_left[ch] > 0
    }

    /// A meter showing these levels (the clip light off).
    pub(crate) fn showing(level: [f32; 2], rms: [f32; 2], hold: [f32; 2]) -> Meter {
        Meter {
            level,
            rms,
            hold,
            ..Meter::default()
        }
    }

    fn quiet(&self) -> bool {
        self.level
            .iter()
            .chain(&self.rms)
            .chain(&self.hold)
            .all(|v| *v <= 0.0)
            && self.clip_left == [0, 0]
    }

    /// One frame with `r` as the newest reading (silence when there is none).
    pub(crate) fn step(&mut self, r: MeterReading) {
        for ch in 0..2 {
            let v = level_t(r.peak[ch]);
            let prev = self.level[ch];
            let d = if v > prev { v } else { v.max(prev - FALL) };
            self.level[ch] = d;
            let rms = level_t(r.rms[ch]);
            self.rms[ch] = if rms > self.rms[ch] {
                rms
            } else {
                rms.max(self.rms[ch] - FALL)
            };
            if d >= self.hold[ch] {
                self.hold[ch] = d;
                self.hold_left[ch] = HOLD_FRAMES;
            } else if self.hold_left[ch] > 0 {
                self.hold_left[ch] -= 1;
            } else {
                self.hold[ch] = d.max(self.hold[ch] - PEAK_FALL);
            }
            if r.peak[ch] > 1.0 {
                self.clip_left[ch] = CLIP_FRAMES;
            } else {
                self.clip_left[ch] = self.clip_left[ch].saturating_sub(1);
            }
        }
    }
}

/// Every strip's meter.
#[derive(Clone, Debug, Default)]
pub struct Meters {
    strips: BTreeMap<Strip, Meter>,
}

impl Meters {
    /// The meter of a strip (silent when it never had a level).
    pub fn get(&self, strip: Strip) -> Meter {
        self.strips.get(&strip).copied().unwrap_or_default()
    }

    /// One frame: the loudest levels since the last frame go in, every
    /// meter moves. Returns true while any meter still shows something, so
    /// the screen asks for another frame.
    pub fn feed(&mut self, levels: &BTreeMap<Strip, MeterReading>) -> bool {
        for strip in levels.keys() {
            self.strips.entry(*strip).or_default();
        }
        let mut moving = false;
        for (strip, m) in &mut self.strips {
            m.step(levels.get(strip).copied().unwrap_or_default());
            moving |= !m.quiet();
        }
        self.strips.retain(|_, m| !m.quiet());
        moving
    }

    /// Strips that show a level.
    pub fn count(&self) -> usize {
        self.strips.len()
    }

    pub fn clear(&mut self) {
        self.strips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_runs_from_the_floor_to_six_db_and_back() {
        assert_eq!(db_to_t(-120.0), 0.0);
        assert_eq!(db_to_t(6.0), 1.0);
        let unity = db_to_t(0.0);
        assert!(unity > 0.8 && unity < 0.85, "{unity}");
        for db in [-48.0, -12.0, -3.5, 0.0, 4.2] {
            assert_eq!(t_to_db(db_to_t(db)), db);
        }
        assert_eq!(t_to_db(0.0), FLOOR_DB);
        assert_eq!(level_t(1.0), db_to_t(0.0));
        assert_eq!(level_t(0.0), 0.0);
    }

    #[test]
    fn a_meter_rises_at_once_falls_slowly_and_holds_its_peak() {
        let strip = Strip::Master;
        let mut m = Meters::default();
        let loud = MeterReading {
            peak: [1.0, 0.5],
            rms: [0.5, 0.25],
        };
        assert!(m.feed(&BTreeMap::from([(strip, loud)])));
        let top = m.get(strip);
        assert_eq!(top.level[0], level_t(1.0));
        assert!(top.level[1] < top.level[0]);
        assert!(!top.clipped(0));

        // Silence: one frame falls by one step, the peak line stays.
        m.feed(&BTreeMap::new());
        let after = m.get(strip);
        assert!((after.level[0] - (top.level[0] - FALL)).abs() < 1e-6);
        assert_eq!(after.hold[0], top.level[0]);

        // Long after, everything has fallen and the strip is let go.
        let mut frames = 0;
        while m.feed(&BTreeMap::new()) {
            frames += 1;
            assert!(frames < 1000);
        }
        assert!(frames > usize::from(HOLD_FRAMES));
        assert_eq!(m.get(strip), Meter::default());
    }

    #[test]
    fn going_over_full_scale_lights_the_clip_light_for_a_second() {
        let strip = Strip::Master;
        let mut m = Meters::default();
        let over = MeterReading {
            peak: [1.2, 0.2],
            rms: [0.7, 0.1],
        };
        m.feed(&BTreeMap::from([(strip, over)]));
        assert!(m.get(strip).clipped(0));
        assert!(!m.get(strip).clipped(1));
        for _ in 0..CLIP_FRAMES {
            m.feed(&BTreeMap::new());
        }
        assert!(!m.get(strip).clipped(0));
    }
}
