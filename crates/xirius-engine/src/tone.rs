//! The test tone (UI-260): a sine on the left, then on the right, at about −18 dBFS, so a person
//! hears which side is which before recording. Like the metronome it goes to the device only
//! (not into the meters, the tap or a recording), and a silent output stays silent.
//!
//! The control side sends a [`Tone`] with [`crate::Command::SetTone`]; the audio thread plays it
//! once and hands it back.

/// The level of the tone's peak, in dBFS.
pub const LEVEL_DB: f32 = -18.0;
/// The pitch of the tone, in Hz.
pub const PITCH: f64 = 440.0;
/// How long each side sounds, and the pause between them, in seconds.
pub const SIDE_SECONDS: f64 = 0.6;
pub const GAP_SECONDS: f64 = 0.15;
/// The fade at each end of a side, so it neither clicks on nor off.
const FADE_SECONDS: f64 = 0.01;

/// One play of the test tone.
pub struct Tone {
    step: f32,
    phase: f32,
    gain: f32,
    /// Frames played so far.
    t: u32,
    side: u32,
    gap: u32,
    fade: u32,
}

impl Tone {
    pub fn new(sample_rate: f64) -> Tone {
        let frames = |s: f64| (s * sample_rate).round().max(1.0) as u32;
        Tone {
            step: (std::f64::consts::TAU * PITCH / sample_rate) as f32,
            phase: 0.0,
            gain: 10f32.powf(LEVEL_DB / 20.0),
            t: 0,
            side: frames(SIDE_SECONDS),
            gap: frames(GAP_SECONDS),
            fade: frames(FADE_SECONDS),
        }
    }

    /// How many frames the whole tone lasts.
    pub fn frames(&self) -> u32 {
        2 * self.side + self.gap
    }

    /// Frames still to play.
    pub fn left(&self) -> u32 {
        self.frames().saturating_sub(self.t)
    }

    pub fn is_over(&self) -> bool {
        self.left() == 0
    }

    /// The next frame, left and right.
    pub fn next_frame(&mut self) -> [f32; 2] {
        if self.is_over() {
            return [0.0; 2];
        }
        let t = self.t;
        self.t += 1;
        let (at, right) = if t < self.side {
            (t, false)
        } else if t < self.side + self.gap {
            return [0.0; 2];
        } else {
            (t - self.side - self.gap, true)
        };
        let edge = at.min(self.side - 1 - at);
        let env = (edge as f32 / self.fade as f32).min(1.0);
        let s = self.gain * env * self.phase.sin();
        self.phase += self.step;
        if self.phase > std::f32::consts::TAU {
            self.phase -= std::f32::consts::TAU;
        }
        if right { [0.0, s] } else { [s, 0.0] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_sounds_on_the_left_then_on_the_right_at_minus_18_dbfs() {
        let mut tone = Tone::new(48_000.0);
        let frames: Vec<[f32; 2]> = (0..tone.frames()).map(|_| tone.next_frame()).collect();
        assert!(tone.is_over());
        assert_eq!(tone.next_frame(), [0.0; 2]);
        let side = (SIDE_SECONDS * 48_000.0) as usize;
        let gap = (GAP_SECONDS * 48_000.0) as usize;
        let peak = |part: &[[f32; 2]], c: usize| part.iter().fold(0f32, |m, f| m.max(f[c].abs()));
        let (left, rest) = frames.split_at(side);
        let (pause, right) = rest.split_at(gap);
        let db = |x: f32| 20.0 * x.log10();
        assert!(
            (db(peak(left, 0)) - LEVEL_DB).abs() < 0.1,
            "{}",
            peak(left, 0)
        );
        assert_eq!(peak(left, 1), 0.0);
        assert_eq!(peak(pause, 0).max(peak(pause, 1)), 0.0);
        assert_eq!(peak(right, 0), 0.0);
        assert!((db(peak(right, 1)) - LEVEL_DB).abs() < 0.1);
        // It fades in and out rather than clicking.
        assert!(left[0][0].abs() < 1e-3 && left[side - 1][0].abs() < 0.02);
    }
}
