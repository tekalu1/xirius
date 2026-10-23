//! The Sampler (PLG-004): plays the multisamples of [`super::samples`] with 32 voices.
//!
//! A note plays the zone nearest its pitch, moved by reading the sample faster or slower
//! (cubic between frames), through an ADSR (a straight attack, decay and release that fall
//! exponentially, 60 dB over their time) and a resonant lowpass. The lowpass is left out
//! while it is wide open. When every voice sounds, the oldest is taken; a note played again
//! fades its last voice in 5 ms instead of cutting it. No latency.
//!
//! Controllers (EDT-004): pitch bend moves every voice up to [`BEND_RANGE`] semitones, the
//! sustain pedal (CC 64) keeps released notes sounding until it comes up, and the modulation
//! wheel (CC 1) adds vibrato, up to [`VIBRATO_DEPTH`] semitones at [`VIBRATO_HZ`].

use std::f64::consts::TAU;
use std::sync::Arc;

use super::bank::Params;
use super::dsp::{Glide, Ramp, STEP, Svf, SvfCoefs, db_to_gain};
use super::files::{FileInbox, FileSet, root_of};
use super::samples::{Library, library};
use crate::control::ControlState;
use crate::processor::{AudioIo, CTRL_PITCH_BEND, NoteEvent, NoteKind, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::SAMPLER`.
const SOUND: usize = 0;
const ATTACK: usize = 1;
const DECAY: usize = 2;
const SUSTAIN: usize = 3;
const RELEASE: usize = 4;
const CUTOFF: usize = 5;
const RESONANCE: usize = 6;
const VELOCITY: usize = 7;
const TUNE: usize = 8;
const FINE: usize = 9;
const VOLUME: usize = 10;

const VOICES: usize = 32;
/// Where an envelope counts as silent (-80 dB).
const SILENT: f64 = 1e-4;
/// How long a voice that gives way takes to fade.
const FADE_MS: f64 = 5.0;
/// How far a full pitch bend moves the pitch, in semitones either way.
pub const BEND_RANGE: f64 = 2.0;
/// The vibrato of the modulation wheel at its top: how far either way, in semitones, and how fast.
pub const VIBRATO_DEPTH: f64 = 0.5;
pub const VIBRATO_HZ: f64 = 5.5;
const MOD_WHEEL: u8 = 1;
const SUSTAIN_PEDAL: u8 = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
    /// Giving way to a new note.
    Fade,
}

/// What a voice reads (ENG-077): a zone of the synthesized library, or a file the song gave
/// the device. A file takes over once the set has one: the nearest of its roots answers, as
/// a synthesized zone would (zones and their own ranges arrive with PLG-067).
#[derive(Clone, Copy)]
enum Src {
    Synth {
        sound: usize,
        zone: usize,
    },
    /// The slot in the [`FileSet`].
    File {
        slot: usize,
    },
}

#[derive(Clone, Copy)]
struct Voice {
    stage: Stage,
    pitch: u8,
    src: Src,
    /// Where it reads in the sample, and how far it moves a frame before tuning.
    pos: f64,
    step: f64,
    gain: f64,
    env: f64,
    /// When it started, to find the oldest.
    born: u64,
    /// Let go while the sustain pedal was down: it releases when the pedal comes up.
    held: bool,
    filter: Svf,
    /// The right channel's filter of a file voice (the left is `filter`).
    filter_r: Svf,
}

impl Voice {
    fn idle() -> Voice {
        Voice {
            stage: Stage::Idle,
            pitch: 0,
            src: Src::Synth { sound: 0, zone: 0 },
            pos: 0.0,
            step: 1.0,
            gain: 0.0,
            env: 0.0,
            born: 0,
            held: false,
            filter: Svf::default(),
            filter_r: Svf::default(),
        }
    }
}

/// What the envelope does in a frame, worked out once every [`STEP`] frames.
#[derive(Clone, Copy, Default)]
struct Rates {
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    fade: f64,
}

pub struct Sampler {
    params: Params,
    glide: Glide,
    rate: f64,
    lib: Option<Arc<Library>>,
    voices: [Voice; VOICES],
    born: u64,
    rates: Rates,
    tune: f64,
    filter: Option<SvfCoefs>,
    volume: Ramp,
    /// Frames until the values are stepped again.
    until_step: usize,
    controls: ControlState,
    /// What the pitch bend multiplies the speed of every voice by.
    bend: f64,
    /// Where the vibrato is in its cycle (0..1).
    vibrato: f64,
    /// The channel the files the song gave the device arrive on, and the newest set.
    files_in: Option<FileInbox>,
    files: Option<Arc<FileSet>>,
}

impl Sampler {
    pub fn new(params: Params, files_in: Option<FileInbox>) -> Sampler {
        Sampler {
            glide: Glide::new(&xirius_params::SAMPLER),
            params,
            rate: 48_000.0,
            lib: None,
            files_in,
            files: None,
            voices: [Voice::idle(); VOICES],
            born: 0,
            rates: Rates::default(),
            tune: 1.0,
            filter: None,
            volume: Ramp::new(1.0),
            until_step: 0,
            controls: ControlState::new(),
            bend: 1.0,
            vibrato: 0.0,
        }
    }

    fn pedal_down(&self) -> bool {
        self.controls.cc(SUSTAIN_PEDAL) >= 64
    }

    fn control(&mut self, kind: NoteKind) {
        let was_down = self.pedal_down();
        self.controls.apply(kind);
        let NoteKind::Control { number, .. } = kind else {
            return;
        };
        if number == CTRL_PITCH_BEND {
            self.bend = 2f64.powf(BEND_RANGE * f64::from(self.controls.bend_unit()) / 12.0);
        } else if number == SUSTAIN_PEDAL && was_down && !self.pedal_down() {
            for v in self.voices.iter_mut().filter(|v| v.held) {
                v.held = false;
                if matches!(v.stage, Stage::Attack | Stage::Decay) {
                    v.stage = Stage::Release;
                }
            }
        }
    }

    /// What the vibrato multiplies the speed by over the next `frames`, moving it on.
    fn vibrato_step(&mut self, frames: usize) -> f64 {
        let depth = VIBRATO_DEPTH * f64::from(self.controls.unit(MOD_WHEEL));
        let k = if depth > 0.0 {
            2f64.powf(depth * (TAU * self.vibrato).sin() / 12.0)
        } else {
            1.0
        };
        self.vibrato = (self.vibrato + VIBRATO_HZ * frames as f64 / self.rate).fract();
        k
    }

    /// Works out what depends on the values.
    fn update(&mut self) {
        let g = &self.glide;
        let frames = |ms: f64| (ms * 0.001 * self.rate).max(1.0);
        // 60 dB over the time.
        let fall = |ms: f64| 10f64.powf(-3.0 / frames(ms));
        self.rates = Rates {
            attack: 1.0 / frames(g.get(ATTACK)),
            decay: fall(g.get(DECAY)),
            sustain: g.get(SUSTAIN) * 0.01,
            release: fall(g.get(RELEASE)),
            fade: 1.0 / frames(FADE_MS),
        };
        self.tune = 2f64.powf((g.get(TUNE) + g.get(FINE) * 0.01) / 12.0);
        let (cutoff, resonance) = (g.get(CUTOFF), g.get(RESONANCE) * 0.01);
        self.filter = (cutoff < 19_999.0 || resonance > 0.0)
            .then(|| SvfCoefs::lowpass(cutoff, resonance, self.rate));
    }

    /// The slot of the file whose root is nearest `pitch` (the higher one on a tie), when the
    /// device was given files.
    fn file_zone(&self, pitch: u8) -> Option<usize> {
        let set = self.files.as_ref()?;
        let mut best: Option<(usize, i32)> = None;
        for (i, s) in set.slots.iter().enumerate() {
            let Some(s) = s else { continue };
            let d = (i32::from(root_of(&s.name)) - i32::from(pitch)).abs();
            if best.is_none_or(|(_, b)| d <= b) {
                best = Some((i, d));
            }
        }
        best.map(|(i, _)| i)
    }

    fn note_on(&mut self, pitch: u8, velocity: f32) {
        // Once the song lists files, they are the sound: a pitch none of them takes, or a
        // slot whose file was not found, stays silent (ENG-077).
        if self.files.as_ref().is_some_and(|s| !s.slots.is_empty()) {
            if let Some(slot) = self.file_zone(pitch) {
                let set = self.files.as_ref().unwrap();
                let root = root_of(&set.slots[slot].as_ref().unwrap().name);
                self.start(
                    pitch,
                    velocity,
                    Src::File { slot },
                    2f64.powf((f64::from(pitch) - f64::from(root)) / 12.0),
                );
            }
            return;
        }
        let Some(lib) = &self.lib else {
            return;
        };
        let sound = self.glide.index(SOUND).min(lib.sounds.len() - 1);
        let s = &lib.sounds[sound];
        let zone = s.zone_for(pitch);
        let root = s.zones[zone].root;
        let src = Src::Synth { sound, zone };
        let step = 2f64.powf((f64::from(pitch) - f64::from(root)) / 12.0) * lib.rate / self.rate;
        self.start(pitch, velocity, src, step);
    }

    /// Gives `pitch` a voice for `src` at `step`: an old voice of the same pitch fades, the
    /// free or oldest slot takes the note.
    fn start(&mut self, pitch: u8, velocity: f32, src: Src, step: f64) {
        for v in &mut self.voices {
            if v.pitch == pitch && !matches!(v.stage, Stage::Idle | Stage::Fade) {
                v.stage = Stage::Fade;
            }
        }
        // A free voice, else the one that is fading, else the oldest.
        let k = (0..VOICES)
            .min_by_key(|&k| {
                let v = &self.voices[k];
                let rank = match v.stage {
                    Stage::Idle => 0,
                    Stage::Fade => 1,
                    Stage::Release => 2,
                    _ => 3,
                };
                (rank, v.born)
            })
            .unwrap_or(0);
        let spread = self.glide.get(VELOCITY) * 0.01;
        self.born += 1;
        self.voices[k] = Voice {
            stage: Stage::Attack,
            pitch,
            src,
            pos: 0.0,
            step,
            gain: f64::from(velocity.clamp(0.0, 1.0)).powf(2.0 * spread),
            env: 0.0,
            born: self.born,
            held: false,
            filter: Svf::default(),
            filter_r: Svf::default(),
        };
    }

    fn note_off(&mut self, pitch: u8) {
        let pedal = self.pedal_down();
        for v in &mut self.voices {
            if v.pitch == pitch && matches!(v.stage, Stage::Attack | Stage::Decay) {
                if pedal {
                    v.held = true;
                } else {
                    v.stage = Stage::Release;
                }
            }
        }
    }

    /// Adds the voices into `[from, to)` of both channels.
    fn render(&mut self, io: &mut AudioIo<'_>, from: usize, to: usize) {
        let speed = self.tune * self.bend * self.vibrato_step(to - from);
        let Some(lib) = &self.lib else {
            return;
        };
        let r = self.rates;
        let target = db_to_gain(self.glide.get(VOLUME));
        let (vol, dvol) = self.volume.toward(target, to - from);
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            let step = v.step * speed;
            let mut vol = vol;
            for i in from..to {
                match v.stage {
                    Stage::Attack => {
                        v.env += r.attack;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => v.env = r.sustain + (v.env - r.sustain) * r.decay,
                    Stage::Release => v.env *= r.release,
                    Stage::Fade => v.env -= r.fade,
                    Stage::Idle => break,
                }
                if v.env < SILENT && v.stage != Stage::Attack {
                    *v = Voice::idle();
                    break;
                }
                // The frame's pair: a synthesized zone is mono, a file keeps its channels.
                let (mut xl, mut xr);
                match v.src {
                    Src::Synth { sound, zone } => {
                        let z = &lib.sounds[sound].zones[zone];
                        if z.done(v.pos) {
                            *v = Voice::idle();
                            break;
                        }
                        let x = z.read(v.pos);
                        xl = x;
                        xr = x;
                        v.pos = z.wrap(v.pos + step);
                    }
                    Src::File { slot } => {
                        let s = self
                            .files
                            .as_ref()
                            .and_then(|s| s.slots.get(slot))
                            .and_then(|s| s.as_ref());
                        let Some(s) = s else {
                            *v = Voice::idle();
                            break;
                        };
                        if s.done(v.pos) {
                            *v = Voice::idle();
                            break;
                        }
                        (xl, xr) = s.read(v.pos);
                        v.pos += step;
                    }
                }
                xl *= v.env * v.gain;
                xr *= v.env * v.gain;
                if let Some(c) = &self.filter {
                    xl = v.filter.lowpass(c, xl);
                    xr = v.filter_r.lowpass(c, xr);
                }
                vol += dvol;
                io.left[i] += (xl * vol) as f32;
                io.right[i] += (xr * vol) as f32;
            }
        }
    }
}

impl Processor for Sampler {
    fn name(&self) -> &str {
        "Sampler"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        // Made here (off the audio thread) the first time, then shared.
        self.lib = Some(library(self.rate));
        self.voices = [Voice::idle(); VOICES];
        self.controls.reset();
        self.bend = 1.0;
        self.vibrato = 0.0;
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.update();
        self.volume.set(db_to_gain(self.glide.get(VOLUME)));
        self.until_step = STEP;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let n = io.frames();
        io.left.fill(0.0);
        io.right.fill(0.0);
        // A newer set fades the voices playing a file, so a held note does not jump into a
        // different file's middle.
        if let Some(inbox) = &mut self.files_in
            && inbox.poll(&mut self.files)
        {
            for v in self
                .voices
                .iter_mut()
                .filter(|v| matches!(v.src, Src::File { .. }))
            {
                v.stage = Stage::Fade;
            }
        }
        let mut at = 0;
        let mut next = 0;
        while at < n {
            while next < events.len() && events[next].offset as usize <= at {
                match events[next].kind {
                    kind @ NoteKind::Control { .. } => self.control(kind),
                    NoteKind::On { pitch, velocity } => self.note_on(pitch, velocity),
                    NoteKind::Off { pitch } => self.note_off(pitch),
                }
                next += 1;
            }
            if self.until_step == 0 {
                if self.glide.step(&mut self.params) {
                    self.update();
                }
                self.until_step = STEP;
            }
            let mut end = (at + self.until_step).min(n);
            if let Some(e) = events.get(next) {
                end = end.min(e.offset as usize);
            }
            self.render(io, at, end);
            self.until_step -= end - at;
            at = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::processor::Processor;

    /// The frequency of what plays in `x[from..from + len]` (by counting upward zero
    /// crossings, between frames).
    fn pitch_of(x: &[f32], from: usize, len: usize) -> f64 {
        let x = &x[from..from + len];
        let mut first = None;
        let mut last = 0.0;
        let mut count = 0;
        for i in 1..x.len() {
            if x[i - 1] < 0.0 && x[i] >= 0.0 {
                let t = (i - 1) as f64 + f64::from(-x[i - 1] / (x[i] - x[i - 1]));
                if first.is_none() {
                    first = Some(t);
                } else {
                    count += 1;
                }
                last = t;
            }
        }
        f64::from(count) * RATE / (last - first.unwrap())
    }

    fn hz(pitch: f64) -> f64 {
        440.0 * 2f64.powf((pitch - 69.0) / 12.0)
    }

    /// A note held for `hold` frames out of `frames`.
    fn play(
        p: &mut dyn Processor,
        pitch: u8,
        velocity: f32,
        hold: usize,
        frames: usize,
    ) -> Vec<f32> {
        let silence = vec![0.0; frames];
        let notes = [(10, on(pitch, velocity)), (10 + hold, off(pitch))];
        let (l, r) = render(p, &silence, &silence, &notes);
        assert_eq!(l, r);
        l
    }

    /// A sound with little but its fundamental (the electric piano, its overtones cut), so
    /// zero crossings count the pitch.
    fn plain(h: &super::super::StockHandle) {
        set_plain(h, "sound", 1.0);
        set_plain(h, "cutoff", 200.0);
    }

    #[test]
    fn a_note_plays_at_its_pitch_from_every_zone() {
        let (mut p, h) = make("sampler");
        plain(&h);
        // On a root, and moved the most up and down from one.
        for pitch in [45u8, 60, 66, 54, 30, 90] {
            set_plain(&h, "cutoff", (hz(f64::from(pitch)) * 1.5).min(20_000.0));
            p.prepare(&setup());
            let x = play(p.as_mut(), pitch, 1.0, 40_000, 40_000);
            let f = pitch_of(&x, 4800, 24_000);
            let cents = 1200.0 * (f / hz(f64::from(pitch))).log2();
            assert!(cents.abs() < 2.0, "{pitch}: {f} Hz, {cents} cents");
        }
    }

    #[test]
    fn tune_and_fine_move_the_pitch() {
        let (mut p, h) = make("sampler");
        plain(&h);
        set_plain(&h, "tune", -12.0);
        set_plain(&h, "fine", 50.0);
        p.prepare(&setup());
        let x = play(p.as_mut(), 69, 1.0, 40_000, 40_000);
        let f = pitch_of(&x, 4800, 24_000);
        let want = 440.0 * 2f64.powf(-11.5 / 12.0);
        assert!((1200.0 * (f / want).log2()).abs() < 2.0, "{f} for {want}");
    }

    #[test]
    fn every_sound_plays() {
        let (mut p, h) = make("sampler");
        for sound in 0..xirius_params::SAMPLER_SOUNDS.len() {
            set_plain(&h, "sound", sound as f64);
            p.prepare(&setup());
            let x = play(p.as_mut(), 60, 1.0, 24_000, 48_000);
            assert!(
                peak(&x) > 0.2 && peak(&x) <= 0.5 + 1e-3,
                "{sound}: {}",
                peak(&x)
            );
            assert!(x.iter().all(|v| v.is_finite()));
        }
    }

    /// A file the song gave the device plays from its next block, pitched from its name's
    /// root note (ENG-077).
    #[test]
    fn a_read_file_plays_pitched_from_its_root() {
        use crate::stock::files::{FileSet, FileSlot};
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        let data = crate::timeline::AudioData {
            left: sine(261.63, 0.4, 48_000),
            right: sine(261.63, 0.4, 48_000),
        };
        assert!(h.set_files(FileSet {
            slots: vec![Some(FileSlot {
                name: "tone_C3.wav".into(),
                data: std::sync::Arc::new(data),
            })],
        }));
        let x = play(p.as_mut(), 72, 1.0, 40_000, 40_000);
        // The file holds 19_200 frames; past it the voice is done.
        let f = pitch_of(&x, 4_800, 18_000);
        let cents = 1200.0 * (f / 523.26).log2();
        assert!(cents.abs() < 2.0, "{f} Hz, {cents} cents");
    }

    /// An empty slot (a file that was not found) stays silent (ENG-077).
    #[test]
    fn a_missing_file_opens_silent() {
        use crate::stock::files::FileSet;
        let (mut p, h) = make("sampler");
        assert!(h.set_files(FileSet { slots: vec![None] }));
        let x = play(p.as_mut(), 60, 1.0, 9_600, 9_600);
        assert_eq!(peak(&x), 0.0);
    }

    /// A newer set replaces the one that plays, and the old one is handed back to be freed
    /// off the audio thread (ENG-077).
    #[test]
    fn a_newer_set_replaces_the_sound_while_playing() {
        use crate::stock::files::{FileSet, FileSlot};
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        let mk = |hz: f64| FileSet {
            slots: vec![Some(FileSlot {
                name: "tone.wav".into(),
                data: std::sync::Arc::new(crate::timeline::AudioData {
                    left: sine(hz, 0.4, 48_000),
                    right: sine(hz, 0.4, 48_000),
                }),
            })],
        };
        assert!(h.set_files(mk(220.0)));
        assert!(h.set_files(mk(440.0)));
        let x = play(p.as_mut(), 60, 1.0, 40_000, 40_000);
        let f = pitch_of(&x, 4_800, 18_000);
        assert!((f - 440.0).abs() < 2.0, "{f}");

        // A set arriving while a note sounds fades the voice rather than jumping into the
        // other file's middle (ENG-077).
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        assert!(h.set_files(mk(440.0)));
        let mut l = vec![0.0f32; 4_800];
        let mut r = vec![0.0f32; 4_800];
        p.as_mut().process(
            &mut crate::processor::AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[crate::processor::NoteEvent {
                offset: 0,
                kind: crate::processor::NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            }],
        );
        assert!(h.set_files(mk(220.0)));
        let mut l = vec![0.0f32; 24_000];
        let mut r = vec![0.0f32; 24_000];
        p.as_mut().process(
            &mut crate::processor::AudioIo {
                left: &mut l,
                right: &mut r,
                key: None,
            },
            &[],
        );
        assert_eq!(
            peak(&l[2_000..]),
            0.0,
            "the voice kept playing past the swap"
        );
    }

    #[test]
    fn the_envelope_follows_the_adsr() {
        let (mut p, h) = make("sampler");
        // The organ holds its level, so what changes is the envelope.
        set_plain(&h, "sound", 5.0);
        set_plain(&h, "attack", 100.0);
        set_plain(&h, "decay", 200.0);
        set_plain(&h, "sustain", 50.0);
        set_plain(&h, "release", 100.0);
        p.prepare(&setup());
        let x = play(p.as_mut(), 60, 1.0, 48_000, 72_000);
        let level = |ms: f64| rms(&x[10 + (ms * 48.0) as usize..][..960]);
        // What the organ gives at full level.
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        p.prepare(&setup());
        let flat = play(p.as_mut(), 60, 1.0, 48_000, 48_000);
        let full = rms(&flat[10 + 38_400..][..960]);
        assert!(level(30.0) < full * 0.5, "the attack is still rising");
        // 60 dB in the decay time toward half: past three decay times, at half.
        assert!(
            (level(800.0) / full - 0.5).abs() < 0.03,
            "{}",
            level(800.0) / full
        );
        // Released at 1 s: 60 dB down 100 ms later, then nothing.
        let held = level(980.0);
        assert!(
            db(level(1100.0) / held) < -50.0,
            "{}",
            db(level(1100.0) / held)
        );
        assert!(x[10 + 48_000 + 9_600..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn velocity_sets_the_level_as_far_as_asked() {
        let (mut p, h) = make("sampler");
        let level = |p: &mut dyn Processor, v: f32| {
            p.prepare(&setup());
            rms(&play(p, 60, v, 9_600, 9_600))
        };
        set_plain(&h, "velocity", 100.0);
        p.prepare(&setup());
        // Full spread: the square of the velocity.
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 0.25).abs() < 0.005, "{r}");
        set_plain(&h, "velocity", 0.0);
        p.prepare(&setup());
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 1.0).abs() < 1e-6, "{r}");
    }

    #[test]
    fn the_cutoff_darkens_the_sound() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "sound", 2.0);
        p.prepare(&setup());
        let open = play(p.as_mut(), 72, 1.0, 24_000, 24_000);
        set_plain(&h, "cutoff", 400.0);
        p.prepare(&setup());
        let dark = play(p.as_mut(), 72, 1.0, 24_000, 24_000);
        // The highs: what changes from one frame to the next.
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        assert!(
            highs(&dark) < highs(&open) * 0.3,
            "{} vs {}",
            highs(&dark),
            highs(&open)
        );
    }

    #[test]
    fn a_chord_takes_a_voice_a_note_and_more_notes_take_the_oldest() {
        let bank = std::sync::Arc::new(super::super::bank::ParamBank::new(&xirius_params::SAMPLER));
        let mut p = super::Sampler::new(super::super::bank::Params::new(bank), None);
        p.prepare(&setup());
        let silence = vec![0.0; 9_600];
        let notes: Vec<_> = (0..40).map(|k| (k * 10, on(30 + k as u8, 0.8))).collect();
        let (l, _) = render(&mut p, &silence, &silence, &notes);
        assert!(l.iter().all(|v| v.is_finite()));
        // The 32 newest notes sound; the first 8 gave way.
        let mut playing: Vec<u8> = p.voices.iter().map(|v| v.pitch).collect();
        playing.sort();
        assert_eq!(playing, (38..70).collect::<Vec<u8>>());
    }

    #[test]
    fn a_note_played_again_does_not_click() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "sound", 5.0);
        p.prepare(&setup());
        let silence = vec![0.0; 9_600];
        let notes = [(10, on(60, 1.0)), (4_000, on(60, 1.0))];
        let (l, _) = render(p.as_mut(), &silence, &silence, &notes);
        let jump = l
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        let usual = l[1000..3990]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // The fading voice and the new one overlap for a moment; a cut would jump by the
        // whole level.
        assert!(jump < usual * 2.0, "{jump} vs {usual}");
    }

    fn bend(value: i32) -> crate::processor::NoteKind {
        crate::processor::NoteKind::control(crate::processor::CTRL_PITCH_BEND, value)
    }

    fn cc(number: u8, value: i32) -> crate::processor::NoteKind {
        crate::processor::NoteKind::control(number, value)
    }

    fn cents(f: f64, want: f64) -> f64 {
        1200.0 * (f / want).log2()
    }

    #[test]
    fn pitch_bend_moves_a_held_note_two_semitones_either_way_and_back() {
        let (mut p, h) = make("sampler");
        plain(&h);
        set_plain(&h, "cutoff", 1_000.0);
        p.prepare(&setup());
        let silence = vec![0.0; 96_000];
        let notes = [
            (10, on(69, 1.0)),
            (24_000, bend(8191)),
            (48_000, bend(-8192)),
            (72_000, bend(0)),
        ];
        let (x, _) = render(p.as_mut(), &silence, &silence, &notes);
        let at = |from: usize| pitch_of(&x, from + 2_400, 19_200);
        assert!(cents(at(0), 440.0).abs() < 2.0, "{}", at(0));
        assert!(cents(at(24_000), 440.0 * 2f64.powf(2.0 / 12.0)).abs() < 2.0);
        assert!(cents(at(48_000), 440.0 * 2f64.powf(-2.0 / 12.0)).abs() < 2.0);
        assert!(cents(at(72_000), 440.0).abs() < 2.0, "{}", at(72_000));
    }

    #[test]
    fn the_sustain_pedal_keeps_a_released_note_sounding_until_it_comes_up() {
        let (mut p, h) = make("sampler");
        // The organ holds its level, so only the pedal keeps it.
        set_plain(&h, "sound", 5.0);
        set_plain(&h, "release", 100.0);
        p.prepare(&setup());
        let silence = vec![0.0; 72_000];
        let free = [(10, on(60, 1.0)), (9_600, off(60))];
        let (open, _) = render(p.as_mut(), &silence, &silence, &free);
        p.prepare(&setup());
        let pedal = [
            (5, cc(64, 127)),
            (10, on(60, 1.0)),
            (9_600, off(60)),
            (48_000, cc(64, 0)),
        ];
        let (held, _) = render(p.as_mut(), &silence, &silence, &pedal);
        let before = rms(&held[4_800..9_600]);
        // Without the pedal it is gone 200 ms after the key comes up.
        assert_eq!(peak(&open[9_600 + 9_600..]), 0.0);
        // With it, still at its level a half second later.
        let later = rms(&held[36_000..40_800]);
        assert!((db(later / before)).abs() < 1.0, "{}", db(later / before));
        // The pedal up releases it.
        assert!(rms(&held[48_000 - 960..48_000]) > before * 0.5);
        assert_eq!(peak(&held[48_000 + 9_600..]), 0.0);
    }

    #[test]
    fn the_modulation_wheel_adds_vibrato() {
        let (mut p, h) = make("sampler");
        plain(&h);
        set_plain(&h, "cutoff", 1_000.0);
        // The pitch over windows a tenth of the vibrato's cycle.
        let spread = |p: &mut dyn Processor, wheel: i32| {
            p.prepare(&setup());
            let silence = vec![0.0; 48_000];
            let notes = [(5, cc(1, wheel)), (10, on(69, 1.0))];
            let (x, _) = render(p, &silence, &silence, &notes);
            let len = (RATE / super::VIBRATO_HZ / 10.0) as usize;
            let fs: Vec<f64> = (0..40)
                .map(|k| pitch_of(&x, 4_800 + k * len, len))
                .collect();
            let lo = fs.iter().copied().fold(f64::MAX, f64::min);
            let hi = fs.iter().copied().fold(0.0, f64::max);
            cents(hi, lo)
        };
        let still = spread(p.as_mut(), 0);
        let full = spread(p.as_mut(), 127);
        assert!(still < 5.0, "{still}");
        // Up to half a semitone either way: close to 100 cents from top to bottom.
        assert!(full > 60.0 && full < 110.0, "{full}");
    }

    #[test]
    fn silence_without_notes() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "resonance", 100.0);
        set_plain(&h, "cutoff", 1000.0);
        assert_silent(p.as_mut());
    }
}
