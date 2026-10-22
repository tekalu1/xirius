//! What the audio thread tells and is told about the input, through atomics only (REC-002,
//! REC-011, REC-012): the level of every input channel, the trim applied to each, and which
//! channels are heard on the output (the input monitor).
//!
//! The monitor is direct: the picked channels are added to what goes to the device, after the
//! master was metered and tapped, like the click. It is in neither the master nor an export,
//! and the recording does not hear it twice. A silent output stays silent; its level
//! ([`InputState::take_monitor_peak`]) is measured all the same, so a test can tell that the
//! input would be heard.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::record::MAX_INPUT_CHANNELS;

/// Shared between the audio thread and the control side. One per engine; see
/// [`crate::AudioEngine::input_state`] and [`crate::Controller::input_state`].
pub struct InputState {
    /// Trim per input channel, a linear gain as `f32` bits (1.0: none).
    gain: [AtomicU32; MAX_INPUT_CHANNELS],
    /// The highest absolute sample per channel since it was last taken, after the trim.
    peak: [AtomicU32; MAX_INPUT_CHANNELS],
    /// Channels heard on the left and on the right output, one bit per channel.
    monitor: [AtomicU32; 2],
    /// The highest absolute sample added to the left and to the right output.
    monitor_peak: [AtomicU32; 2],
}

impl Default for InputState {
    fn default() -> Self {
        InputState {
            gain: std::array::from_fn(|_| AtomicU32::new(1.0f32.to_bits())),
            peak: std::array::from_fn(|_| AtomicU32::new(0)),
            monitor: [AtomicU32::new(0), AtomicU32::new(0)],
            monitor_peak: [AtomicU32::new(0), AtomicU32::new(0)],
        }
    }
}

impl InputState {
    /// The trim of input channel `ch` (from 0), a linear gain.
    pub fn set_gain(&self, ch: usize, gain: f32) {
        if let Some(g) = self.gain.get(ch) {
            g.store(gain.max(0.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub fn gain(&self, ch: usize) -> f32 {
        self.gain
            .get(ch)
            .map_or(1.0, |g| f32::from_bits(g.load(Ordering::Relaxed)))
    }

    /// The channels (bit `c` for channel `c`) heard on the left and on the right output; `0, 0`
    /// is no monitor. A mono input heard in the middle is `1, 1`.
    pub fn set_monitor(&self, left: u32, right: u32) {
        self.monitor[0].store(left, Ordering::Relaxed);
        self.monitor[1].store(right, Ordering::Relaxed);
    }

    pub fn monitor(&self) -> (u32, u32) {
        (
            self.monitor[0].load(Ordering::Relaxed),
            self.monitor[1].load(Ordering::Relaxed),
        )
    }

    /// The level of channel `ch` since the last call (linear, 0 when nothing came in).
    pub fn take_peak(&self, ch: usize) -> f32 {
        self.peak
            .get(ch)
            .map_or(0.0, |p| f32::from_bits(p.swap(0, Ordering::Relaxed)))
    }

    /// What was added to the left and to the right output since the last call.
    pub fn take_monitor_peak(&self) -> [f32; 2] {
        [0, 1].map(|i| f32::from_bits(self.monitor_peak[i].swap(0, Ordering::Relaxed)))
    }

    // ---- the audio thread's side ----

    /// Applies the trims to `buf` (channel `c` at `c * frames..`) and keeps each channel's peak.
    pub(crate) fn process(&self, buf: &mut [f32], frames: usize) {
        if frames == 0 {
            return;
        }
        for (c, ch) in buf.chunks_exact_mut(frames).enumerate() {
            let g = self.gain(c);
            let mut peak = 0.0f32;
            if g == 1.0 {
                for s in ch.iter() {
                    peak = peak.max(s.abs());
                }
            } else {
                for s in ch.iter_mut() {
                    *s *= g;
                    peak = peak.max(s.abs());
                }
            }
            raise(&self.peak[c], peak);
        }
    }

    /// Adds the monitored channels of `heard` to `left` and `right`.
    pub(crate) fn mix(&self, heard: &[f32], frames: usize, left: &mut [f32], right: &mut [f32]) {
        let (ml, mr) = self.monitor();
        if frames == 0 || (ml | mr) == 0 {
            return;
        }
        let channels = heard.len() / frames;
        let mut peaks = [0.0f32; 2];
        for (side, (mask, out)) in [(ml, left), (mr, right)].into_iter().enumerate() {
            for c in 0..channels.min(MAX_INPUT_CHANNELS) {
                if mask & (1 << c) == 0 {
                    continue;
                }
                for (o, s) in out.iter_mut().zip(&heard[c * frames..(c + 1) * frames]) {
                    *o += *s;
                }
            }
            for o in out.iter() {
                peaks[side] = peaks[side].max(o.abs());
            }
        }
        for (p, v) in self.monitor_peak.iter().zip(peaks) {
            raise(p, v);
        }
    }
}

/// `cell = max(cell, v)` for non-negative floats stored as bits (their order is the bits').
fn raise(cell: &AtomicU32, v: f32) {
    cell.fetch_max(v.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trim_scales_and_the_peak_is_taken_once() {
        let s = InputState::default();
        s.set_gain(1, 0.5);
        let mut buf = vec![0.2, -0.4, 0.8, -1.0];
        s.process(&mut buf, 2);
        assert_eq!(buf, vec![0.2, -0.4, 0.4, -0.5]);
        assert_eq!(s.take_peak(0), 0.4);
        assert_eq!(s.take_peak(1), 0.5);
        assert_eq!(s.take_peak(1), 0.0);
    }

    #[test]
    fn only_the_channels_on_the_mask_reach_each_side() {
        let s = InputState::default();
        s.set_monitor(0b01, 0b10);
        let heard = [0.25, 0.25, 0.5, 0.5];
        let (mut l, mut r) = ([0.0; 2], [0.0; 2]);
        s.mix(&heard, 2, &mut l, &mut r);
        assert_eq!((l, r), ([0.25, 0.25], [0.5, 0.5]));
        assert_eq!(s.take_monitor_peak(), [0.25, 0.5]);
        s.set_monitor(0, 0);
        s.mix(&heard, 2, &mut l, &mut r);
        assert_eq!((l, r), ([0.25, 0.25], [0.5, 0.5]));
    }
}
