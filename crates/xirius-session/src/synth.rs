//! Placeholder instruments: stand-ins for the Sampler and the Drum Kit until real sample playback
//! exists. They make a clearly audible, deterministic sound from note events so the session layer
//! can be heard and tested end to end. Fixed voice count, no allocation while processing.

use std::f64::consts::TAU;

use xirius_engine::{AudioIo, ControlState, NoteEvent, NoteKind, ProcessSetup, Processor};

const VOICES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// A sine at the note's pitch with a short attack and release (the Sampler).
    Tone,
    /// One-shot hits chosen by pitch: kick, snare, hat, tom (the Drum Kit).
    Drums,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    on: bool,
    held: bool,
    pitch: u8,
    vel: f32,
    phase: f64,
    /// Frames since the note started.
    age: u64,
    /// Frames since the note was released.
    released: u64,
}

pub struct PlaceholderSynth {
    flavor: Flavor,
    rate: f64,
    voices: [Voice; VOICES],
    noise: u32,
    /// The controller values the song has set (EDT-004). The sound does not use them yet.
    controls: ControlState,
}

impl PlaceholderSynth {
    pub fn new(flavor: Flavor) -> Self {
        Self {
            flavor,
            rate: 48_000.0,
            voices: [Voice::default(); VOICES],
            noise: 0x1234_5678,
            controls: ControlState::new(),
        }
    }

    /// The controller values received so far.
    pub fn controls(&self) -> &ControlState {
        &self.controls
    }

    fn start(&mut self, pitch: u8, vel: f32) {
        // Retrigger the same pitch, else a free voice, else steal the oldest.
        let idx = self
            .voices
            .iter()
            .position(|v| v.on && v.pitch == pitch && v.held)
            .or_else(|| self.voices.iter().position(|v| !v.on))
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| v.age)
                    .map_or(0, |(i, _)| i)
            });
        self.voices[idx] = Voice {
            on: true,
            held: true,
            pitch,
            vel,
            ..Voice::default()
        };
    }

    fn release(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.on && v.held && v.pitch == pitch {
                v.held = false;
            }
        }
    }

    fn next_noise(&mut self) -> f32 {
        self.noise = self
            .noise
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

fn hz(pitch: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

impl Processor for PlaceholderSynth {
    fn name(&self) -> &str {
        match self.flavor {
            Flavor::Tone => "sampler (placeholder)",
            Flavor::Drums => "drum kit (placeholder)",
        }
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let n = io.frames();
        let mut next = 0;
        for i in 0..n {
            while next < events.len() && events[next].offset as usize <= i {
                match events[next].kind {
                    NoteKind::On { pitch, velocity } => self.start(pitch, velocity),
                    NoteKind::Off { pitch } => self.release(pitch),
                    kind @ NoteKind::Control { .. } => {
                        self.controls.apply(kind);
                    }
                }
                next += 1;
            }
            let mut sum = 0.0f32;
            for k in 0..VOICES {
                let v = self.voices[k];
                if !v.on {
                    continue;
                }
                let t = v.age as f64 / self.rate;
                let (sample, done) = match self.flavor {
                    Flavor::Tone => {
                        let attack = (t / 0.005).min(1.0) as f32;
                        let release = if v.held {
                            1.0
                        } else {
                            1.0 - (v.released as f64 / (0.04 * self.rate)).min(1.0) as f32
                        };
                        let s = (v.phase.sin() as f32) * attack * release * v.vel * 0.25;
                        (s, !v.held && release <= 0.0)
                    }
                    Flavor::Drums => {
                        let noise = self.next_noise();
                        drum_sample(v.pitch, t, v.phase, noise, v.vel)
                    }
                };
                sum += sample;
                let freq = match self.flavor {
                    Flavor::Tone => hz(v.pitch),
                    Flavor::Drums => drum_freq(v.pitch, t),
                };
                let voice = &mut self.voices[k];
                voice.phase = (voice.phase + TAU * freq / self.rate) % TAU;
                voice.age += 1;
                if !voice.held {
                    voice.released += 1;
                }
                if done {
                    voice.on = false;
                }
            }
            io.left[i] = sum;
            io.right[i] = sum;
        }
    }
}

/// The source of an audio track: silence, so the clips on the timeline are all it plays.
pub struct Silence;

impl Processor for Silence {
    fn timeline_only(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "audio"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        io.clear();
    }
}

enum Hit {
    Kick,
    Snare,
    Hat,
    Tom,
}

fn hit(pitch: u8) -> Hit {
    match pitch {
        35 | 36 => Hit::Kick,
        37..=40 => Hit::Snare,
        42 | 44 | 46 | 49 | 51 | 53 | 55 | 57 | 59 => Hit::Hat,
        _ => Hit::Tom,
    }
}

fn drum_freq(pitch: u8, t: f64) -> f64 {
    match hit(pitch) {
        Hit::Kick => 50.0 + 100.0 * (-t / 0.03).exp(),
        Hit::Snare => 190.0,
        Hit::Hat => 0.0,
        Hit::Tom => hz(pitch.saturating_sub(24)).clamp(60.0, 400.0),
    }
}

/// One sample of a drum hit and whether it has died away.
fn drum_sample(pitch: u8, t: f64, phase: f64, noise: f32, vel: f32) -> (f32, bool) {
    let (decay, len) = match hit(pitch) {
        Hit::Kick => (0.12, 0.4),
        Hit::Snare => (0.07, 0.3),
        Hit::Hat => (0.02, 0.1),
        Hit::Tom => (0.1, 0.4),
    };
    let env = (-t / decay).exp() as f32;
    let tone = phase.sin() as f32;
    let s = match hit(pitch) {
        Hit::Kick | Hit::Tom => tone,
        Hit::Snare => 0.5 * tone + 0.7 * noise,
        Hit::Hat => noise,
    };
    (s * env * vel * 0.4, t >= len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_engine::RenderMode;

    fn run(flavor: Flavor, events: &[NoteEvent], frames: usize) -> Vec<f32> {
        let mut s = PlaceholderSynth::new(flavor);
        s.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: frames,
            mode: RenderMode::Offline,
        });
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        s.process(
            &mut AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            events,
        );
        l
    }

    fn on(offset: u32, pitch: u8) -> NoteEvent {
        NoteEvent {
            offset,
            kind: NoteKind::On {
                pitch,
                velocity: 1.0,
            },
        }
    }

    #[test]
    fn silent_until_a_note_and_deterministic() {
        let a = run(Flavor::Tone, &[on(100, 69)], 2000);
        let b = run(Flavor::Tone, &[on(100, 69)], 2000);
        assert_eq!(a, b);
        assert!(a[..100].iter().all(|&x| x == 0.0));
        assert!(a[100..].iter().any(|&x| x.abs() > 0.1));
    }

    #[test]
    fn a_released_tone_dies_away() {
        let off = NoteEvent {
            offset: 500,
            kind: NoteKind::Off { pitch: 69 },
        };
        let a = run(Flavor::Tone, &[on(0, 69), off], 6000);
        assert!(a[300..500].iter().any(|&x| x.abs() > 0.1));
        assert!(a[500 + 2100..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn drum_hits_fade_without_a_note_off() {
        let a = run(Flavor::Drums, &[on(0, 36)], 48_000);
        assert!(a[..200].iter().any(|&x| x.abs() > 0.1));
        assert!(a[40_000..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn the_placeholder_keeps_the_controller_values_it_is_sent() {
        let ev = |offset, number, value| NoteEvent {
            offset,
            kind: NoteKind::control(number, value),
        };
        let mut s = PlaceholderSynth::new(Flavor::Tone);
        s.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        });
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        s.process(
            &mut AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[ev(0, 1, 100), ev(5, 1, 80)],
        );
        assert_eq!(s.controls().cc(1), 80);
    }
}
