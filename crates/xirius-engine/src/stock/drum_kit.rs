//! The Drum Kit (PLG-005): drums made as they are hit, from the recipes below (no samples).
//!
//! A hit is up to two oscillators whose pitch falls from where it starts (the skin), noise
//! through a low cut and a high cut (snares, claps, shakers), and six square waves at the
//! uneven ratios of an analogue drum machine through a low cut (hats and cymbals). Each part
//! fades 60 dB over its time. Notes follow the General MIDI drum map; the two kits are a
//! standard kit and an electronic one in the manner of an analogue drum machine. A closed or
//! pedal hat stops an open one. Tune and decay apply to the hits that start after they move;
//! the volume and the group levels move what plays. No latency.
//!
//! Controllers (EDT-004) leave it alone: a hit plays out whatever the key or the sustain pedal
//! does after it, and a General MIDI kit does not bend or take vibrato.

use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use super::bank::Params;
use super::dsp::{Biquad, Coefs, Glide, Noise, Ramp, STEP, db_to_gain};
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::DRUM_KIT`.
const KIT: usize = 0;
const VOLUME: usize = 1;
const TUNE: usize = 2;
const DECAY: usize = 3;
const VELOCITY: usize = 4;
/// The group levels follow in the order of the groups.
const GROUP_LEVELS: usize = 5;

// The groups.
const KICK: usize = 0;
const SNARE: usize = 1;
const HATS: usize = 2;
const TOMS: usize = 3;
const CYMBALS: usize = 4;
const PERCUSSION: usize = 5;
const GROUPS: usize = 6;

const VOICES: usize = 24;
const SILENT: f64 = 1e-4;
const FADE_MS: f64 = 5.0;
/// The level of a hit at full velocity before the volume.
const LEVEL: f64 = 0.8;
/// The ratios of an analogue drum machine's metal (its six squares, in Hz at a scale of 1).
const METAL_HZ: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

/// The notes the kits play (General MIDI): the tests go through all of them.
#[cfg(test)]
const NOTES: [u8; 34] = [
    35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 59,
    60, 61, 62, 63, 64, 69, 70, 75, 76, 77,
];

/// An oscillator: its pitch falls from `from` to `to` with a time constant of `sweep` ms.
#[derive(Clone, Copy, Debug)]
struct Osc {
    from: f64,
    to: f64,
    sweep: f64,
    decay: f64,
    level: f64,
    square: bool,
}

/// Noise through a low cut and a high cut; a clap is a few quick bursts first.
#[derive(Clone, Copy, Debug)]
struct Hiss {
    low_cut: f64,
    high_cut: f64,
    decay: f64,
    level: f64,
    bursts: u32,
}

#[derive(Clone, Copy, Debug)]
struct Metal {
    scale: f64,
    low_cut: f64,
    decay: f64,
    level: f64,
}

#[derive(Clone, Copy, Debug)]
struct Hit {
    group: usize,
    /// -1 (left) to 1 (right), as the drummer sits seen from the front.
    pan: f64,
    oscs: [Option<Osc>; 2],
    hiss: Option<Hiss>,
    metal: Option<Metal>,
}

const fn sine(from: f64, to: f64, sweep: f64, decay: f64, level: f64) -> Option<Osc> {
    Some(Osc {
        from,
        to,
        sweep,
        decay,
        level,
        square: false,
    })
}

const fn square(hz: f64, decay: f64, level: f64) -> Option<Osc> {
    Some(Osc {
        from: hz,
        to: hz,
        sweep: 1.0,
        decay,
        level,
        square: true,
    })
}

const fn hiss(low_cut: f64, high_cut: f64, decay: f64, level: f64) -> Option<Hiss> {
    Some(Hiss {
        low_cut,
        high_cut,
        decay,
        level,
        bursts: 0,
    })
}

const fn metal(scale: f64, low_cut: f64, decay: f64, level: f64) -> Option<Metal> {
    Some(Metal {
        scale,
        low_cut,
        decay,
        level,
    })
}

const fn hit(
    group: usize,
    pan: f64,
    oscs: [Option<Osc>; 2],
    hiss: Option<Hiss>,
    metal: Option<Metal>,
) -> Option<Hit> {
    Some(Hit {
        group,
        pan,
        oscs,
        hiss,
        metal,
    })
}

const TOM_HZ: [(u8, f64, f64); 6] = [
    (41, 82.0, -0.5),
    (43, 98.0, -0.3),
    (45, 117.0, -0.1),
    (47, 139.0, 0.1),
    (48, 165.0, 0.3),
    (50, 196.0, 0.5),
];

/// What `note` is in `kit` (0 standard, 1 electronic), or nothing.
fn recipe(kit: usize, note: u8) -> Option<Hit> {
    let electro = kit == 1;
    if let Some(&(_, hz, pan)) = TOM_HZ.iter().find(|t| t.0 == note) {
        return if electro {
            hit(
                TOMS,
                pan,
                [sine(hz * 1.3, hz, 15.0, 450.0, 0.9), None],
                None,
                None,
            )
        } else {
            hit(
                TOMS,
                pan,
                [
                    sine(hz * 1.6, hz, 40.0, 380.0, 0.8),
                    sine(hz * 2.4, hz * 1.5, 30.0, 120.0, 0.2),
                ],
                hiss(60.0, 4000.0, 40.0, 0.15),
                None,
            )
        };
    }
    match (note, electro) {
        // Kicks.
        (35 | 36, false) => {
            let low = if note == 35 { 45.0 } else { 52.0 };
            hit(
                KICK,
                0.0,
                [sine(low * 3.0, low, 30.0, 380.0, 1.0), None],
                hiss(20.0, 3000.0, 6.0, 0.35),
                None,
            )
        }
        (35 | 36, true) => {
            let low = if note == 35 { 42.0 } else { 48.0 };
            hit(
                KICK,
                0.0,
                [sine(low * 2.6, low, 18.0, 900.0, 1.0), None],
                hiss(20.0, 2000.0, 3.0, 0.2),
                None,
            )
        }
        // Snares, the side stick and the clap.
        (37, _) => hit(
            SNARE,
            -0.1,
            [
                sine(1700.0, 1600.0, 5.0, 25.0, 0.5),
                sine(820.0, 800.0, 5.0, 40.0, 0.4),
            ],
            hiss(2500.0, 9000.0, 25.0, 0.3),
            None,
        ),
        (38 | 40, false) => {
            let lift = if note == 40 { 1.1 } else { 1.0 };
            hit(
                SNARE,
                -0.1,
                [
                    sine(260.0 * lift, 190.0 * lift, 12.0, 140.0, 0.55),
                    sine(340.0 * lift, 330.0 * lift, 8.0, 70.0, 0.25),
                ],
                hiss(1200.0 * lift, 10_000.0, 200.0, 0.6),
                None,
            )
        }
        (38 | 40, true) => {
            let lift = if note == 40 { 1.1 } else { 1.0 };
            hit(
                SNARE,
                -0.1,
                [
                    sine(240.0 * lift, 180.0 * lift, 8.0, 110.0, 0.6),
                    sine(330.0 * lift, 330.0 * lift, 1.0, 80.0, 0.35),
                ],
                hiss(1800.0 * lift, 8000.0, 160.0, 0.45),
                None,
            )
        }
        (39, _) => hit(
            SNARE,
            0.0,
            [None, None],
            Some(Hiss {
                low_cut: 900.0,
                high_cut: 3000.0,
                decay: if electro { 260.0 } else { 180.0 },
                level: 0.9,
                bursts: if electro { 4 } else { 3 },
            }),
            None,
        ),
        // Hats: closed, pedal, open.
        (42, false) => hit(
            HATS,
            0.4,
            [None, None],
            hiss(7000.0, 16_000.0, 45.0, 0.25),
            metal(1.0, 7000.0, 60.0, 1.5),
        ),
        (44, false) => hit(
            HATS,
            0.4,
            [None, None],
            hiss(6000.0, 14_000.0, 30.0, 0.2),
            metal(1.0, 6000.0, 40.0, 1.2),
        ),
        (46, false) => hit(
            HATS,
            0.4,
            [None, None],
            hiss(7000.0, 16_000.0, 700.0, 0.2),
            metal(1.0, 7000.0, 900.0, 1.35),
        ),
        (42, true) => hit(HATS, 0.3, [None, None], None, metal(1.0, 8000.0, 45.0, 2.4)),
        (44, true) => hit(HATS, 0.3, [None, None], None, metal(1.0, 7000.0, 30.0, 2.0)),
        (46, true) => hit(
            HATS,
            0.3,
            [None, None],
            None,
            metal(1.0, 8000.0, 600.0, 2.2),
        ),
        // Cymbals: crashes, the ride and its bell, the china and the splash.
        (49 | 57 | 52 | 55, _) => {
            let (scale, decay, pan) = match note {
                49 => (1.0, 1600.0, -0.6),
                57 => (1.12, 1800.0, 0.6),
                52 => (0.9, 1300.0, 0.7),
                _ => (1.3, 500.0, -0.4),
            };
            let decay = if electro { decay * 0.8 } else { decay };
            hit(
                CYMBALS,
                pan,
                [None, None],
                hiss(5000.0, 16_000.0, decay * 0.8, 0.3),
                metal(scale, 4000.0, decay, 0.45),
            )
        }
        (51 | 59, _) => hit(
            CYMBALS,
            0.7,
            [sine(4200.0, 4200.0, 1.0, 800.0, 0.06), None],
            hiss(6000.0, 14_000.0, 900.0, 0.08),
            metal(1.5, 5000.0, 1800.0, 0.7),
        ),
        (53, _) => hit(
            CYMBALS,
            0.7,
            [
                sine(1180.0, 1180.0, 1.0, 900.0, 0.35),
                sine(1770.0, 1770.0, 1.0, 600.0, 0.2),
            ],
            None,
            metal(1.5, 3000.0, 700.0, 0.3),
        ),
        // Percussion.
        (54, _) => hit(
            PERCUSSION,
            0.5,
            [None, None],
            Some(Hiss {
                low_cut: 6000.0,
                high_cut: 15_000.0,
                decay: 140.0,
                level: 0.35,
                bursts: 2,
            }),
            metal(2.2, 7000.0, 160.0, 0.35),
        ),
        (56, _) => hit(
            PERCUSSION,
            0.3,
            [square(540.0, 280.0, 0.25), square(800.0, 280.0, 0.25)],
            None,
            None,
        ),
        (60 | 61, _) => {
            let hz = if note == 60 { 420.0 } else { 300.0 };
            hit(
                PERCUSSION,
                -0.3,
                [sine(hz * 1.15, hz, 8.0, 120.0, 0.6), None],
                hiss(1500.0, 6000.0, 15.0, 0.1),
                None,
            )
        }
        (62..=64, _) => {
            let hz = match note {
                62 => 330.0,
                63 => 300.0,
                _ => 210.0,
            };
            let decay = if note == 62 { 60.0 } else { 220.0 };
            hit(
                PERCUSSION,
                0.3,
                [sine(hz * 1.2, hz, 10.0, decay, 0.6), None],
                hiss(1000.0, 5000.0, 12.0, 0.12),
                None,
            )
        }
        (69 | 70, _) => hit(
            PERCUSSION,
            0.4,
            [None, None],
            hiss(5000.0, 14_000.0, if note == 69 { 70.0 } else { 45.0 }, 0.45),
            None,
        ),
        (75, _) => hit(
            PERCUSSION,
            0.2,
            [sine(2500.0, 2500.0, 1.0, 45.0, 0.5), None],
            None,
            None,
        ),
        (76 | 77, _) => {
            let hz = if note == 76 { 1050.0 } else { 800.0 };
            hit(
                PERCUSSION,
                0.2,
                [sine(hz * 1.05, hz, 3.0, 50.0, 0.5), None],
                hiss(800.0, 4000.0, 8.0, 0.1),
                None,
            )
        }
        _ => None,
    }
}

/// What an oscillator does while a hit sounds.
#[derive(Clone, Copy, Debug, Default)]
struct OscState {
    on: bool,
    square: bool,
    phase: f64,
    /// The pitch above where it settles, in cycles a frame, and its fall.
    lift: f64,
    lift_k: f64,
    settle: f64,
    env: f64,
    env_k: f64,
}

#[derive(Clone, Copy, Debug)]
struct Voice {
    on: bool,
    note: u8,
    group: usize,
    born: u64,
    age: u32,
    left: f64,
    right: f64,
    fade: f64,
    fading: bool,
    oscs: [OscState; 2],
    noise: Noise,
    hiss_on: bool,
    hiss_env: f64,
    hiss_k: f64,
    burst_k: f64,
    burst_len: u32,
    bursts_end: u32,
    hiss_filters: [Biquad; 2],
    hiss_coefs: [Coefs; 2],
    metal_on: bool,
    metal_phase: [f64; 6],
    metal_step: [f64; 6],
    metal_env: f64,
    metal_k: f64,
    metal_filters: [Biquad; 2],
    metal_coefs: Coefs,
}

impl Voice {
    fn idle() -> Voice {
        Voice {
            on: false,
            note: 0,
            group: 0,
            born: 0,
            age: 0,
            left: 0.0,
            right: 0.0,
            fade: 1.0,
            fading: false,
            oscs: [OscState::default(); 2],
            noise: Noise::new(1),
            hiss_on: false,
            hiss_env: 0.0,
            hiss_k: 0.0,
            burst_k: 0.0,
            burst_len: 1,
            bursts_end: 0,
            hiss_filters: [Biquad::default(); 2],
            hiss_coefs: [Coefs::THROUGH; 2],
            metal_on: false,
            metal_phase: [0.0; 6],
            metal_step: [0.0; 6],
            metal_env: 0.0,
            metal_k: 0.0,
            metal_filters: [Biquad::default(); 2],
            metal_coefs: Coefs::THROUGH,
        }
    }

    /// One frame of the hit (before its level and pan); false once it has died away.
    #[inline]
    fn tick(&mut self, fade_step: f64) -> Option<f64> {
        let mut x = 0.0;
        let mut alive = false;
        for o in self.oscs.iter_mut().filter(|o| o.on) {
            let dt = o.settle + o.lift;
            let wave = if o.square {
                square_wave(o.phase, dt)
            } else {
                (TAU * o.phase).sin()
            };
            x += wave * o.env;
            o.phase = (o.phase + dt).fract();
            o.lift *= o.lift_k;
            o.env *= o.env_k;
            alive |= o.env >= SILENT;
        }
        if self.hiss_on {
            if self.age < self.bursts_end {
                self.hiss_env = if self.age.is_multiple_of(self.burst_len) {
                    1.0
                } else {
                    self.hiss_env * self.burst_k
                };
            } else if self.age == self.bursts_end {
                self.hiss_env = 1.0;
            } else {
                self.hiss_env *= self.hiss_k;
            }
            let [lo, hi] = &mut self.hiss_filters;
            let [lc, hc] = &self.hiss_coefs;
            let n = hi.tick(hc, lo.tick(lc, self.noise.sample()));
            x += n * self.hiss_env;
            alive |= self.hiss_env >= SILENT || self.age < self.bursts_end;
        }
        if self.metal_on {
            let mut m = 0.0;
            for (p, dt) in self.metal_phase.iter_mut().zip(self.metal_step) {
                m += square_wave(*p, dt);
                *p = (*p + dt).fract();
            }
            let [a, b] = &mut self.metal_filters;
            let m = b.tick(&self.metal_coefs, a.tick(&self.metal_coefs, m / 6.0));
            x += m * self.metal_env;
            self.metal_env *= self.metal_k;
            alive |= self.metal_env >= SILENT;
        }
        if self.fading {
            self.fade -= fade_step;
            alive &= self.fade > 0.0;
            x *= self.fade.max(0.0);
        }
        self.age = self.age.saturating_add(1);
        alive.then_some(x)
    }
}

/// A square wave with its steps smoothed over a frame (polyBLEP), so it does not alias much.
#[inline]
fn square_wave(phase: f64, dt: f64) -> f64 {
    let blep = |t: f64| {
        if t < dt {
            let t = t / dt;
            t + t - t * t - 1.0
        } else if t > 1.0 - dt {
            let t = (t - 1.0) / dt;
            t * t + t + t + 1.0
        } else {
            0.0
        }
    };
    let v = if phase < 0.5 { 1.0 } else { -1.0 };
    v + blep(phase) - blep((phase + 0.5).fract())
}

pub struct DrumKit {
    params: Params,
    glide: Glide,
    rate: f64,
    voices: [Voice; VOICES],
    born: u64,
    levels: [Ramp; GROUPS],
    until_step: usize,
}

impl DrumKit {
    pub fn new(params: Params) -> DrumKit {
        DrumKit {
            glide: Glide::new(&xirius_params::DRUM_KIT),
            params,
            rate: 48_000.0,
            voices: [Voice::idle(); VOICES],
            born: 0,
            levels: [Ramp::new(LEVEL); GROUPS],
            until_step: 0,
        }
    }

    fn level(&self, group: usize) -> f64 {
        LEVEL * db_to_gain(self.glide.get(VOLUME) + self.glide.get(GROUP_LEVELS + group))
    }

    fn note_on(&mut self, note: u8, velocity: f32) {
        let kit = self.glide.index(KIT);
        let Some(hit) = recipe(kit, note) else {
            return;
        };
        // A closed or pedal hat stops an open one.
        if matches!(note, 42 | 44) {
            for v in self.voices.iter_mut().filter(|v| v.on && v.note == 46) {
                v.fading = true;
            }
        }
        let k = (0..VOICES)
            .min_by_key(|&k| (self.voices[k].on, self.voices[k].born))
            .unwrap_or(0);
        let rate = self.rate;
        let pitch = 2f64.powf(self.glide.get(TUNE) / 12.0);
        let stretch = self.glide.get(DECAY) * 0.01;
        let frames = |ms: f64| (ms * 0.001 * rate).max(1.0);
        let fall = |ms: f64| 10f64.powf(-3.0 / frames(ms * stretch));
        let spread = self.glide.get(VELOCITY) * 0.01;
        let gain = f64::from(velocity.clamp(0.0, 1.0)).powf(2.0 * spread);
        let angle = (hit.pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
        self.born += 1;
        let mut v = Voice::idle();
        v.on = true;
        v.note = note;
        v.group = hit.group;
        v.born = self.born;
        v.left = gain * angle.cos() * std::f64::consts::SQRT_2;
        v.right = gain * angle.sin() * std::f64::consts::SQRT_2;
        v.noise = Noise::new(0x9E37_79B9 ^ (self.born as u32).wrapping_mul(2_654_435_761));
        for (state, osc) in v.oscs.iter_mut().zip(hit.oscs) {
            let Some(o) = osc else { continue };
            *state = OscState {
                on: true,
                square: o.square,
                phase: 0.0,
                lift: (o.from - o.to) * pitch / rate,
                lift_k: (-1.0 / frames(o.sweep)).exp(),
                settle: o.to * pitch / rate,
                env: o.level,
                env_k: fall(o.decay),
            };
        }
        if let Some(h) = hit.hiss {
            v.hiss_on = true;
            v.hiss_k = fall(h.decay);
            v.burst_len = frames(10.0) as u32;
            v.burst_k = 10f64.powf(-3.0 / frames(12.0));
            v.bursts_end = h.bursts * v.burst_len;
            // The level goes into the high cut, so the envelope (and each burst) starts at 1.
            v.hiss_coefs = [
                Coefs::highpass(h.low_cut * pitch, FRAC_1_SQRT_2, rate),
                scale_coefs(
                    Coefs::lowpass(h.high_cut * pitch, FRAC_1_SQRT_2, rate),
                    h.level,
                ),
            ];
        }
        if let Some(m) = hit.metal {
            v.metal_on = true;
            v.metal_env = m.level;
            v.metal_k = fall(m.decay);
            for (step, hz) in v.metal_step.iter_mut().zip(METAL_HZ) {
                *step = hz * m.scale * pitch / rate;
            }
            // Each square starts somewhere else, so the six do not step together.
            for (k, p) in v.metal_phase.iter_mut().enumerate() {
                *p = (k as f64 * 0.37).fract();
            }
            v.metal_coefs = Coefs::highpass(m.low_cut * pitch, FRAC_1_SQRT_2, rate);
        }
        self.voices[k] = v;
    }
}

/// The filter with its output scaled by `gain`.
fn scale_coefs(c: Coefs, gain: f64) -> Coefs {
    Coefs {
        b0: c.b0 * gain,
        b1: c.b1 * gain,
        b2: c.b2 * gain,
        ..c
    }
}

impl Processor for DrumKit {
    fn name(&self) -> &str {
        "Drum Kit"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        self.voices = [Voice::idle(); VOICES];
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        for g in 0..GROUPS {
            let level = self.level(g);
            self.levels[g].set(level);
        }
        self.until_step = STEP;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let n = io.frames();
        io.left.fill(0.0);
        io.right.fill(0.0);
        let fade_step = 1.0 / (FADE_MS * 0.001 * self.rate);
        let mut at = 0;
        let mut next = 0;
        while at < n {
            while next < events.len() && events[next].offset as usize <= at {
                if let NoteKind::On { pitch, velocity } = events[next].kind {
                    self.note_on(pitch, velocity);
                }
                next += 1;
            }
            if self.until_step == 0 {
                self.glide.step(&mut self.params);
                self.until_step = STEP;
            }
            let mut end = (at + self.until_step).min(n);
            if let Some(e) = events.get(next) {
                end = end.min(e.offset as usize);
            }
            let len = end - at;
            let mut ramps = [(0.0, 0.0); GROUPS];
            for (g, r) in ramps.iter_mut().enumerate() {
                let level = self.level(g);
                *r = self.levels[g].toward(level, len);
            }
            for v in self.voices.iter_mut().filter(|v| v.on) {
                let (mut level, step) = ramps[v.group];
                for i in at..end {
                    let Some(x) = v.tick(fade_step) else {
                        v.on = false;
                        break;
                    };
                    level += step;
                    io.left[i] += (x * level * v.left) as f32;
                    io.right[i] += (x * level * v.right) as f32;
                }
            }
            self.until_step -= len;
            at = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    fn hit_once(
        p: &mut dyn Processor,
        note: u8,
        velocity: f32,
        frames: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        p.prepare(&setup());
        let silence = vec![0.0; frames];
        render(
            p,
            &silence,
            &silence,
            &[(0, on(note, velocity)), (100, off(note))],
        )
    }

    fn mono(l: &[f32], r: &[f32]) -> Vec<f32> {
        l.iter().zip(r).map(|(a, b)| a + b).collect()
    }

    /// When the level (in 5 ms windows) first falls 40 dB below its loudest.
    fn ring_time(x: &[f32]) -> f64 {
        let w = 240;
        let levels: Vec<f64> = x.chunks(w).map(rms).collect();
        let top = levels.iter().cloned().fold(0.0, f64::max);
        let k = levels.iter().rposition(|l| *l > top * 0.01).unwrap();
        (k + 1) as f64 * w as f64 / RATE
    }

    /// The pitch in `x[from..from + len]` by upward zero crossings.
    fn pitch_of(x: &[f32], from: usize, len: usize) -> f64 {
        let x = &x[from..from + len];
        let ups: Vec<f64> = (1..x.len())
            .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
            .map(|i| (i - 1) as f64 + f64::from(-x[i - 1] / (x[i] - x[i - 1])))
            .collect();
        (ups.len() - 1) as f64 * RATE / (ups[ups.len() - 1] - ups[0])
    }

    #[test]
    fn every_note_of_both_kits_sounds_and_dies_away() {
        let (mut p, h) = make("drum-kit");
        for kit in 0..2 {
            set_plain(&h, "kit", kit as f64);
            for note in NOTES {
                let (l, r) = hit_once(p.as_mut(), note, 1.0, 4 * RATE as usize);
                let peak = peak(&l).max(peak(&r));
                assert!((0.05..1.5).contains(&peak), "kit {kit} note {note}: {peak}");
                assert!(l.iter().chain(&r).all(|v| v.is_finite()));
                let tail = &l[l.len() - 4800..];
                assert!(
                    tail.iter().all(|v| *v == 0.0),
                    "kit {kit} note {note} rings on"
                );
            }
        }
    }

    #[test]
    fn a_note_outside_the_map_is_silent() {
        let (mut p, _) = make("drum-kit");
        let (l, r) = hit_once(p.as_mut(), 20, 1.0, 9_600);
        assert_eq!(peak(&l) + peak(&r), 0.0);
    }

    #[test]
    fn the_kick_falls_in_pitch() {
        let (mut p, _) = make("drum-kit");
        let (l, r) = hit_once(p.as_mut(), 36, 1.0, RATE as usize);
        let x = mono(&l, &r);
        let early = pitch_of(&x, 0, 960);
        let late = pitch_of(&x, 9_600, 9_600);
        assert!(early > late * 1.5, "{early} then {late}");
        assert!((late - 52.0).abs() < 2.0, "{late}");
    }

    #[test]
    fn tune_moves_the_toms() {
        let (mut p, h) = make("drum-kit");
        set_plain(&h, "kit", 1.0);
        let settled = |p: &mut dyn Processor| {
            let (l, r) = hit_once(p, 45, 1.0, RATE as usize);
            pitch_of(&mono(&l, &r), 9_600, 9_600)
        };
        let base = settled(p.as_mut());
        assert!((base - 117.0).abs() < 2.0, "{base}");
        set_plain(&h, "tune", 7.0);
        let up = settled(p.as_mut());
        let cents = 1200.0 * (up / base).log2();
        assert!((cents - 700.0).abs() < 10.0, "{cents}");
    }

    #[test]
    fn decay_stretches_the_ring() {
        let (mut p, h) = make("drum-kit");
        let ring = |p: &mut dyn Processor| {
            let (l, r) = hit_once(p, 41, 1.0, 2 * RATE as usize);
            ring_time(&mono(&l, &r))
        };
        let base = ring(p.as_mut());
        set_plain(&h, "decay", 200.0);
        let long = ring(p.as_mut());
        assert!((long / base - 2.0).abs() < 0.15, "{base} then {long}");
    }

    #[test]
    fn the_electronic_kick_rings_longer() {
        let (mut p, h) = make("drum-kit");
        let (l, r) = hit_once(p.as_mut(), 36, 1.0, 2 * RATE as usize);
        let standard = ring_time(&mono(&l, &r));
        set_plain(&h, "kit", 1.0);
        let (l, r) = hit_once(p.as_mut(), 36, 1.0, 2 * RATE as usize);
        let electro = ring_time(&mono(&l, &r));
        assert!(electro > standard * 1.8, "{standard} vs {electro}");
    }

    #[test]
    fn a_closed_hat_stops_the_open_one() {
        let (mut p, _) = make("drum-kit");
        let silence = vec![0.0; RATE as usize];
        let (open, _) = render(p.as_mut(), &silence, &silence, &[(0, on(46, 1.0))]);
        p.prepare(&setup());
        let (choked, _) = render(
            p.as_mut(),
            &silence,
            &silence,
            &[(0, on(46, 1.0)), (4_800, on(42, 1.0))],
        );
        // 150 ms after: the closed hat is gone and the open one would still ring.
        let at = 4_800 + 7_200;
        assert!(rms(&open[at..at + 2_400]) > 1e-3);
        assert!(rms(&choked[at..at + 2_400]) < rms(&open[at..at + 2_400]) * 1e-2);
    }

    #[test]
    fn group_levels_and_the_volume_set_the_level() {
        let (mut p, h) = make("drum-kit");
        let level = |p: &mut dyn Processor, note: u8| {
            let (l, r) = hit_once(p, note, 1.0, 9_600);
            rms(&mono(&l, &r))
        };
        let (snare, kick) = (level(p.as_mut(), 38), level(p.as_mut(), 36));
        set_plain(&h, "snare", -12.0);
        // The noise differs from hit to hit, so the snare is a little off exact.
        assert!((level(p.as_mut(), 38) / snare - db_to_gain(-12.0)).abs() < 0.02);
        assert!((level(p.as_mut(), 36) / kick - 1.0).abs() < 0.02);
        set_plain(&h, "volume", -6.0);
        let k = level(p.as_mut(), 36) / kick;
        assert!((k - db_to_gain(-6.0)).abs() < 0.01, "{k}");
    }

    #[test]
    fn velocity_sets_the_level_as_far_as_asked() {
        let (mut p, h) = make("drum-kit");
        let level = |p: &mut dyn Processor, v: f32| {
            let (l, r) = hit_once(p, 75, v, 4_800);
            rms(&l) + rms(&r)
        };
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 0.25).abs() < 1e-3, "{r}");
        set_plain(&h, "velocity", 0.0);
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 1.0).abs() < 1e-6, "{r}");
    }

    #[test]
    fn the_hats_sit_right_and_the_kick_in_the_middle() {
        let (mut p, _) = make("drum-kit");
        let (l, r) = hit_once(p.as_mut(), 42, 1.0, 4_800);
        assert!(rms(&r) > rms(&l) * 1.3);
        let (l, r) = hit_once(p.as_mut(), 36, 1.0, 4_800);
        assert_eq!(l, r);
    }

    #[test]
    fn silence_without_notes() {
        let (mut p, _) = make("drum-kit");
        assert_silent(p.as_mut());
    }
}
