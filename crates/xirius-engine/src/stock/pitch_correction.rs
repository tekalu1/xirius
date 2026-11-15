//! Real-time monophonic pitch correction (PLG-076).
//!
//! Algorithms implemented from the papers, with no implementation code copied:
//! A. de Cheveigné and H. Kawahara, "YIN, a fundamental frequency estimator for
//! speech and music", Journal of the Acoustical Society of America 111(4),
//! 1917–1930 (2002), DOI 10.1121/1.1458024.
//! E. Moulines and F. Charpentier, "Pitch-synchronous waveform processing
//! techniques for text-to-speech synthesis using diphones", Speech Communication
//! 9(5–6), 453–467 (1990), DOI 10.1016/0167-6393(90)90021-Z.

use std::f64::consts::PI;

use super::bank::Params;
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};

const KEY: usize = 0;
const SCALE: usize = 1;
const SPEED: usize = 2;
const TRANSITION: usize = 3;
const AMOUNT: usize = 4;
const FORMANT: usize = 5;
const RANGE: usize = 6;
const HOP: u64 = 256;
const DECIMATE: usize = 4;
const OBS_COUNT: usize = 64;

#[derive(Clone, Copy)]
struct Observation {
    center: i64,
    hz: f64,
    out_hz: f64,
    target: Option<i32>,
    cents: f64,
}

impl Observation {
    const EMPTY: Self = Self {
        center: -1,
        hz: 0.0,
        out_hz: 0.0,
        target: None,
        cents: 0.0,
    };

    fn voiced(self) -> bool {
        self.hz > 0.0 && self.target.is_some()
    }
}

pub struct PitchCorrection {
    params: Params,
    rate: f64,
    prepared_range: usize,
    max_tau: usize,
    latency: usize,
    input: [Vec<f32>; 2],
    mono: Vec<f32>,
    active_history: Vec<u128>,
    window: Vec<f32>,
    diff: Vec<f64>,
    wet: [Vec<f32>; 2],
    weight: Vec<f32>,
    frame: u64,
    voiced_hz: Option<f64>,
    active: u128,
    target: Option<i32>,
    smooth_offset: f64,
    transition_left: usize,
    next_grain: f64,
    out_hz: f64,
    cents: f64,
    observations: [Observation; OBS_COUNT],
    wet_mix: f32,
}

impl PitchCorrection {
    pub fn new(params: Params) -> Self {
        Self {
            params,
            rate: 48_000.0,
            prepared_range: 1,
            max_tau: 480,
            latency: 1920,
            input: [Vec::new(), Vec::new()],
            mono: Vec::new(),
            active_history: Vec::new(),
            window: Vec::new(),
            diff: Vec::new(),
            wet: [Vec::new(), Vec::new()],
            weight: Vec::new(),
            frame: 0,
            voiced_hz: None,
            active: 0,
            target: None,
            smooth_offset: 0.0,
            transition_left: 0,
            next_grain: 0.0,
            out_hz: 0.0,
            cents: 0.0,
            observations: [Observation::EMPTY; OBS_COUNT],
            wet_mix: 0.0,
        }
    }

    fn bounds(range: usize) -> (f64, f64) {
        match range {
            0 => (60.0, 300.0),
            2 => (200.0, 1200.0),
            _ => (100.0, 600.0),
        }
    }

    fn mono_at(&self, frame: i64) -> f32 {
        if frame < 0
            || frame as u64 > self.frame
            || self.frame - frame as u64 >= self.mono.len() as u64
        {
            0.0
        } else {
            self.mono[frame as usize % self.mono.len()]
        }
    }

    fn sample(&self, channel: usize, pos: f64) -> f32 {
        let a = pos.floor() as i64;
        let frac = (pos - a as f64) as f32;
        let get = |at: i64| {
            if at < 0
                || at as u64 > self.frame
                || self.frame - at as u64 >= self.input[channel].len() as u64
            {
                0.0
            } else {
                self.input[channel][at as usize % self.input[channel].len()]
            }
        };
        get(a) * (1.0 - frac) + get(a + 1) * frac
    }

    /// YIN difference, cumulative mean normalized difference, first threshold
    /// valley and parabolic interpolation. A failed threshold is unvoiced.
    fn detect(&mut self) -> Option<f64> {
        let max = self.max_tau;
        if self.frame + 1 < (2 * max) as u64 {
            return None;
        }
        let (lo, hi) = Self::bounds(self.prepared_range);
        let min_lag = (self.rate / hi / DECIMATE as f64).floor().max(2.0) as usize;
        let max_lag = (self.rate / lo / DECIMATE as f64).ceil() as usize;
        let start = self.frame as usize + 1 - 2 * max;
        let ring = self.mono.len();
        let first = start % ring;
        let copied = (ring - first).min(2 * max);
        self.window[..copied].copy_from_slice(&self.mono[first..first + copied]);
        if copied < 2 * max {
            self.window[copied..2 * max].copy_from_slice(&self.mono[..2 * max - copied]);
        }
        let mut energy = 0.0;
        for j in max..2 * max {
            let v = f64::from(self.window[j]);
            energy += v * v;
        }
        if energy / (max as f64) < 0.000_025 {
            return None;
        }
        self.diff[0] = 1.0;
        let mut sum = 0.0;
        let width = max / DECIMATE;
        for lag in 1..=max_lag {
            let mut d = 0.0;
            for j in 0..width {
                let a = f64::from(self.window[j * DECIMATE]);
                let b = f64::from(self.window[(j + lag) * DECIMATE]);
                let delta = a - b;
                d += delta * delta;
            }
            sum += d;
            self.diff[lag] = if sum > 0.0 { d * lag as f64 / sum } else { 1.0 };
        }
        let mut chosen = None;
        for lag in min_lag..=max_lag {
            if self.diff[lag] < 0.14 {
                let mut bottom = lag;
                while bottom < max_lag && self.diff[bottom + 1] < self.diff[bottom] {
                    bottom += 1;
                }
                chosen = Some(bottom);
                break;
            }
        }
        let mut lag = chosen?;
        // An octave candidate with substantially better periodicity is a detection
        // correction. It never changes the target-note boundaries.
        let twice = lag * 2;
        if twice <= max_lag && self.diff[twice] < self.diff[lag] * 0.65 {
            lag = twice;
        }
        let coarse = lag * DECIMATE;
        let lo_tau = coarse
            .saturating_sub(DECIMATE)
            .max((self.rate / hi).floor() as usize);
        let hi_tau = (coarse + DECIMATE).min(max);
        let mut best = (coarse, f64::INFINITY);
        for tau in lo_tau..=hi_tau {
            let d = self.full_difference(tau);
            if d < best.1 {
                best = (tau, d);
            }
        }
        let tau = best.0;
        let left = self.full_difference(tau.saturating_sub(1));
        let mid = best.1;
        let right = self.full_difference((tau + 1).min(max));
        let denom = left - 2.0 * mid + right;
        let offset = if denom.abs() > 1e-12 {
            0.5 * (left - right) / denom
        } else {
            0.0
        };
        Some(self.rate / (tau as f64 + offset.clamp(-0.5, 0.5)))
    }

    fn full_difference(&self, tau: usize) -> f64 {
        let mut sum = 0.0;
        for j in 0..self.max_tau {
            let difference = f64::from(self.window[j] - self.window[j + tau]);
            sum += difference * difference;
        }
        sum
    }

    fn nearest_note(&self, midi: f64, notes: u128) -> Option<i32> {
        let mask = xirius_params::pitch_correction::scale_mask(self.params.norm(SCALE));
        let key = self.params.index(KEY) as i32;
        let mut chosen = midi.round() as i32;
        let mut distance = f64::INFINITY;
        if notes == 0 && mask == 0 {
            return None;
        }
        if notes != 0 {
            for note in 0..128 {
                if notes & (1u128 << note) == 0 {
                    continue;
                }
                let candidate = note + 12 * ((midi - note as f64) / 12.0).round() as i32;
                let d = (midi - candidate as f64).abs();
                if d < distance || (d == distance && candidate < chosen) {
                    chosen = candidate;
                    distance = d;
                }
            }
        } else {
            for candidate in 0..128i32 {
                let relative = (candidate - key).rem_euclid(12) as u32;
                if mask & (1 << relative) == 0 {
                    continue;
                }
                let d = (midi - candidate as f64).abs();
                if d < distance || (d == distance && candidate < chosen) {
                    chosen = candidate;
                    distance = d;
                }
            }
        }
        Some(chosen)
    }

    fn mark(&self, center: f64, period: f64) -> f64 {
        let radius = (period * 0.5).round() as i64;
        let mid = center.round() as i64;
        let mut best = mid;
        let mut peak = f32::NEG_INFINITY;
        for i in mid - radius..=mid + radius {
            let value = self.mono_at(i);
            if value > peak {
                peak = value;
                best = i;
            }
        }
        best as f64
    }

    fn grain(&mut self, center: f64, input_period: f64, output_period: f64, formant: bool) {
        let source = self.mark(center, input_period);
        let radius = input_period.ceil() as i64;
        let wet_len = self.weight.len();
        let center_frame = center.round() as i64;
        for offset in -radius..=radius {
            let out_frame = center_frame + offset;
            if out_frame < 0 || out_frame as u64 + self.latency as u64 <= self.frame {
                continue;
            }
            let phase = offset as f64 / input_period;
            if phase.abs() >= 1.0 {
                continue;
            }
            let window = (0.5 + 0.5 * (PI * phase).cos()) as f32;
            let index = out_frame as usize % wet_len;
            let source_offset = if formant {
                offset as f64
            } else {
                offset as f64 * input_period / output_period
            };
            self.wet[0][index] += self.sample(0, source + source_offset) * window;
            self.wet[1][index] += self.sample(1, source + source_offset) * window;
            self.weight[index] += window;
        }
    }

    fn event(&mut self, kind: NoteKind) {
        match kind {
            NoteKind::On { pitch, velocity } if velocity > 0.0 => self.active |= 1u128 << pitch,
            NoteKind::On { pitch, .. } | NoteKind::Off { pitch } => {
                self.active &= !(1u128 << pitch)
            }
            NoteKind::Control { .. } => {}
        }
    }

    fn observation(&self, frame: i64) -> Observation {
        if frame < 0 {
            return Observation::EMPTY;
        }
        let nominal = (frame + self.max_tau as i64) / HOP as i64;
        let a = self.observations[nominal as usize % OBS_COUNT];
        let b = self.observations[(nominal as usize + 1) % OBS_COUNT];
        if a.center < 0 || a.center > frame {
            return Observation::EMPTY;
        }
        if b.center <= a.center || b.center < frame {
            return a;
        }
        let fraction = ((frame - a.center) as f64 / (b.center - a.center) as f64).clamp(0.0, 1.0);
        if !a.voiced() || !b.voiced() {
            return if fraction < 0.5 { a } else { b };
        }
        Observation {
            center: frame,
            hz: a.hz + (b.hz - a.hz) * fraction,
            out_hz: a.out_hz + (b.out_hz - a.out_hz) * fraction,
            target: if fraction < 0.5 { a.target } else { b.target },
            cents: a.cents + (b.cents - a.cents) * fraction,
        }
    }
}

impl Processor for PitchCorrection {
    fn name(&self) -> &str {
        "Pitch Correction"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.params.refresh();
        self.rate = setup.sample_rate;
        // Range sets the declared latency. The host rebuilds its delay graph
        // around prepare(), so keep this choice fixed until the next prepare.
        self.prepared_range = self.params.index(RANGE);
        let (lo, _) = Self::bounds(self.prepared_range);
        self.max_tau = (self.rate / lo).ceil() as usize;
        // A grain scheduled one period ahead can read one period beyond its
        // centre after a half-period mark search. This is the limiting lookahead.
        self.latency = (5 * self.max_tau).div_ceil(2) + 1;
        let capacity = self.latency + 4 * self.max_tau + setup.max_block + 4;
        self.input = [vec![0.0; capacity], vec![0.0; capacity]];
        self.mono = vec![0.0; capacity];
        self.active_history = vec![0; capacity];
        self.window = vec![0.0; 2 * self.max_tau];
        self.diff = vec![0.0; self.max_tau + 2];
        self.wet = [vec![0.0; capacity], vec![0.0; capacity]];
        self.weight = vec![0.0; capacity];
        self.frame = 0;
        self.voiced_hz = None;
        self.active = 0;
        self.target = None;
        self.smooth_offset = 0.0;
        self.transition_left = 0;
        self.next_grain = 0.0;
        self.out_hz = 0.0;
        self.cents = 0.0;
        self.observations = [Observation::EMPTY; OBS_COUNT];
        self.wet_mix = 0.0;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        self.params.refresh();
        let amount = self.params.get(AMOUNT) / 100.0;
        let formant = self.params.on(FORMANT);
        let speed_frames = self.rate * self.params.get(SPEED) * 0.001;
        let transition_frames = self.rate * self.params.get(TRANSITION) * 0.001;
        let mut event_index = 0;
        for i in 0..io.frames() {
            while event_index < events.len() && events[event_index].offset as usize == i {
                self.event(events[event_index].kind);
                event_index += 1;
            }
            let t = self.frame;
            let index = t as usize % self.mono.len();
            self.input[0][index] = io.left[i];
            self.input[1][index] = io.right[i];
            self.mono[index] = 0.5 * (io.left[i] + io.right[i]);
            self.active_history[index] = self.active;
            if t.is_multiple_of(HOP) {
                self.voiced_hz = self.detect();
                let center = t as i64 - self.max_tau as i64;
                let mut observation = Observation {
                    center,
                    ..Observation::EMPTY
                };
                if let Some(hz) = self.voiced_hz {
                    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
                    let active = if center >= 0 {
                        self.active_history[center as usize % self.active_history.len()]
                    } else {
                        0
                    };
                    let target = self.nearest_note(midi, active);
                    if self.target != target {
                        let changed_note = self.target.is_some();
                        if !changed_note {
                            self.smooth_offset = 0.0;
                        }
                        self.target = target;
                        self.transition_left = if changed_note {
                            transition_frames.round() as usize
                        } else {
                            0
                        };
                    }
                    let ms_frames = if self.transition_left > 0 {
                        transition_frames
                    } else {
                        speed_frames
                    };
                    let alpha = 1.0 - (-(HOP as f64) / ms_frames.max(1.0)).exp();
                    self.smooth_offset +=
                        (target.map_or(0.0, |n| n as f64 - midi) - self.smooth_offset) * alpha;
                    self.transition_left = self.transition_left.saturating_sub(HOP as usize);
                    let shifted = midi + self.smooth_offset * amount;
                    self.out_hz = 440.0 * 2f64.powf((shifted - 69.0) / 12.0);
                    self.cents = target.map_or(0.0, |note| (midi - note as f64) * 100.0);
                    observation.hz = hz;
                    observation.out_hz = self.out_hz;
                    observation.target = target;
                    observation.cents = self.cents;
                } else {
                    self.target = None;
                    self.smooth_offset = 0.0;
                    self.out_hz = 0.0;
                    self.cents = 0.0;
                }
                self.observations[(t / HOP) as usize % OBS_COUNT] = observation;
            }
            if t >= self.latency as u64 {
                let out_frame = t - self.latency as u64;
                let obs = self.observation(out_frame as i64);
                if obs.voiced() && amount > 0.0 {
                    let input_period = self.rate / obs.hz;
                    let output_period = self.rate / obs.out_hz.max(1.0);
                    while self.next_grain <= out_frame as f64 + input_period {
                        self.grain(self.next_grain, input_period, output_period, formant);
                        self.next_grain += output_period;
                    }
                } else {
                    self.next_grain = out_frame as f64;
                }
                let wet_index = out_frame as usize % self.weight.len();
                let dry_index = out_frame as usize % self.input[0].len();
                let wet_weight = self.weight[wet_index];
                let use_wet = obs.voiced() && amount > 0.0 && wet_weight > 0.1;
                let step = (1.0 / (self.rate * 0.003)) as f32;
                self.wet_mix = (self.wet_mix + if use_wet { step } else { -step }).clamp(0.0, 1.0);
                for (channel, output) in [&mut io.left[i], &mut io.right[i]].into_iter().enumerate()
                {
                    let dry = self.input[channel][dry_index];
                    let wet = if wet_weight > 0.1 {
                        self.wet[channel][wet_index] / wet_weight
                    } else {
                        dry
                    };
                    *output = dry + self.wet_mix * (wet - dry);
                    self.wet[channel][wet_index] = 0.0;
                }
                self.weight[wet_index] = 0.0;
            } else {
                io.left[i] = 0.0;
                io.right[i] = 0.0;
            }
            self.frame += 1;
        }
        if self.params.probe().watching() {
            self.params.probe().pitch(
                self.voiced_hz.map_or(0.0, |v| v as f32),
                self.out_hz as f32,
                self.target.unwrap_or(-1),
                self.cents as f32,
            );
        }
    }

    fn latency(&self) -> u32 {
        self.latency as u32
    }
}
