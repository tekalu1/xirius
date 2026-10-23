//! Built-in processors that need no plugin: enough to make and shape sound in tests and demos.

use std::f64::consts::TAU;

use crate::control::ControlState;
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};

/// A free-running sine wave on both channels.
pub struct Sine {
    freq: f64,
    amp: f32,
    phase: f64,
    step: f64,
}

impl Sine {
    pub fn new(freq: f64, amp: f32) -> Self {
        Self {
            freq,
            amp,
            phase: 0.0,
            step: 0.0,
        }
    }
}

impl Processor for Sine {
    fn name(&self) -> &str {
        "sine"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.step = TAU * self.freq / setup.sample_rate;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        for (l, r) in io.left.iter_mut().zip(io.right.iter_mut()) {
            let v = self.amp * self.phase.sin() as f32;
            *l = v;
            *r = v;
            self.phase += self.step;
            if self.phase >= TAU {
                self.phase -= TAU;
            }
        }
    }
}

/// A fixed amplitude multiplier, for inserts.
pub struct Gain {
    gain: f32,
}

impl Gain {
    pub fn new(gain: f32) -> Self {
        Self { gain }
    }
}

impl Processor for Gain {
    fn name(&self) -> &str {
        "gain"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        for s in io.left.iter_mut().chain(io.right.iter_mut()) {
            *s *= self.gain;
        }
    }
}

/// Delays the signal by a fixed number of frames and reports it as latency.
pub struct Delay {
    frames: usize,
    line: [Vec<f32>; 2],
    pos: usize,
}

impl Delay {
    pub fn new(frames: usize) -> Self {
        Self {
            frames,
            line: [vec![0.0; frames.max(1)], vec![0.0; frames.max(1)]],
            pos: 0,
        }
    }
}

impl Processor for Delay {
    fn name(&self) -> &str {
        "delay"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        if self.frames == 0 {
            return;
        }
        for i in 0..io.frames() {
            let l = std::mem::replace(&mut self.line[0][self.pos], io.left[i]);
            let r = std::mem::replace(&mut self.line[1][self.pos], io.right[i]);
            io.left[i] = l;
            io.right[i] = r;
            self.pos = (self.pos + 1) % self.frames;
        }
    }

    fn latency(&self) -> u32 {
        self.frames as u32
    }
}

const VOICES: usize = 8;

#[derive(Clone, Copy)]
struct Voice {
    pitch: u8,
    phase: f64,
    step: f64,
    level: f32,
    target: f32,
}

/// A small polyphonic sine synth that answers note events (stands in for an instrument plugin).
pub struct SineSynth {
    voices: [Option<Voice>; VOICES],
    ramp: f32,
    inv_rate: f64,
    /// The controller values the song has set (EDT-004). The sound does not use them yet.
    controls: ControlState,
}

impl Default for SineSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl SineSynth {
    pub fn new() -> Self {
        Self {
            voices: [None; VOICES],
            ramp: 0.0,
            inv_rate: 1.0 / 48_000.0,
            controls: ControlState::new(),
        }
    }

    /// The controller values received so far.
    pub fn controls(&self) -> &ControlState {
        &self.controls
    }

    fn handle(&mut self, kind: NoteKind) {
        let sample_rate_step = self.inv_rate;
        match kind {
            NoteKind::On { pitch, velocity } => {
                let freq = 440.0 * 2f64.powf((pitch as f64 - 69.0) / 12.0);
                let voice = Voice {
                    pitch,
                    phase: 0.0,
                    step: TAU * freq * sample_rate_step,
                    level: 0.0,
                    target: velocity * 0.25,
                };
                let slot = self
                    .voices
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(pitch as usize % VOICES);
                self.voices[slot] = Some(voice);
            }
            NoteKind::Off { pitch } => {
                for v in self.voices.iter_mut().flatten() {
                    if v.pitch == pitch {
                        v.target = 0.0;
                    }
                }
            }
            NoteKind::Control { .. } => {
                self.controls.apply(kind);
            }
        }
    }
}

impl Processor for SineSynth {
    fn name(&self) -> &str {
        "sine-synth"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        // 5 ms attack and release.
        self.ramp = (1.0 / (0.005 * setup.sample_rate)) as f32;
        self.inv_rate = 1.0 / setup.sample_rate;
        self.voices = [None; VOICES];
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let mut next = 0;
        for i in 0..io.frames() {
            while next < events.len() && events[next].offset as usize <= i {
                self.handle(events[next].kind);
                next += 1;
            }
            let mut sum = 0.0f32;
            for slot in self.voices.iter_mut() {
                let Some(v) = slot else { continue };
                if v.level < v.target {
                    v.level = (v.level + self.ramp).min(v.target);
                } else if v.level > v.target {
                    v.level = (v.level - self.ramp).max(v.target);
                }
                sum += v.level * v.phase.sin() as f32;
                v.phase = (v.phase + v.step) % TAU;
                if v.target == 0.0 && v.level == 0.0 {
                    *slot = None;
                }
            }
            io.left[i] = sum;
            io.right[i] = sum;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CTRL_PITCH_BEND, RenderMode};

    #[test]
    fn the_synth_keeps_the_controller_values_it_is_sent() {
        let mut s = SineSynth::default();
        s.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        });
        let ev = |offset, number, value| NoteEvent {
            offset,
            kind: NoteKind::control(number, value),
        };
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        s.process(
            &mut AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[
                ev(3, 64, 127),
                ev(10, CTRL_PITCH_BEND, -2000),
                ev(20, 64, 0),
            ],
        );
        assert_eq!(s.controls().cc(64), 0);
        assert_eq!(s.controls().pitch_bend(), -2000);
    }
}
