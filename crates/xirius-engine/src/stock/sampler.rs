//! The Sampler (PLG-067): plays the files the song gave it as zones, with 32 voices.
//!
//! Each file is a zone: its root note comes from its name, and the keys between two roots are
//! split at the middle ([`FileSet::layout`]). A note plays its zone moved by reading the file
//! faster or slower, with the zone's own start, end, loop, reverse and normalize, through the
//! device's one voice ([`super::voice`]): the envelope, the filter and the amp. Without files
//! it is silent. When every voice sounds, the oldest is taken; a note played again fades its
//! last voice in 5 ms instead of cutting it. No latency.
//!
//! Controllers (EDT-004): pitch bend moves every voice up to [`BEND_RANGE`] semitones, the
//! sustain pedal (CC 64) keeps released notes sounding until it comes up, and the modulation
//! wheel (CC 1) adds vibrato, up to [`VIBRATO_DEPTH`] semitones at [`VIBRATO_HZ`].

use std::f64::consts::TAU;
use std::sync::Arc;

use super::bank::Params;
use super::dsp::{Glide, Ramp, STEP, db_to_gain};
use super::files::{FileInbox, FileSet, Trigger, Voicing};
use super::voice::{Shape, Stage, Voice, data_of};
use crate::control::ControlState;
use crate::processor::{AudioIo, CTRL_PITCH_BEND, NoteEvent, NoteKind, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::SAMPLER`.
const ATTACK: usize = 0;
const DECAY: usize = 1;
const SUSTAIN: usize = 2;
const RELEASE: usize = 3;
const FTYPE: usize = 4;
const CUTOFF: usize = 5;
const RESONANCE: usize = 6;
const DRIVE: usize = 7;
const TRANSPOSE: usize = 8;
const FINE: usize = 9;
const VOLUME: usize = 10;
const PAN: usize = 11;
const VELOCITY: usize = 12;

const VOICES: usize = 32;
/// How far a full pitch bend moves the pitch, in semitones either way.
pub const BEND_RANGE: f64 = 2.0;
/// The vibrato of the modulation wheel at its top: how far either way, in semitones, and how fast.
pub const VIBRATO_DEPTH: f64 = 0.5;
pub const VIBRATO_HZ: f64 = 5.5;
const MOD_WHEEL: u8 = 1;
const SUSTAIN_PEDAL: u8 = 64;

pub struct Sampler {
    params: Params,
    glide: Glide,
    rate: f64,
    voices: [Voice; VOICES],
    /// The velocity each voice was played at, to work out its level again in a newer set.
    velocities: [f32; VOICES],
    born: u64,
    shape: Shape,
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
            files_in,
            files: None,
            voices: [Voice::IDLE; VOICES],
            velocities: [0.0; VOICES],
            born: 0,
            shape: Shape::default(),
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
                v.release();
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
        let voicing = Voicing {
            attack: g.get(ATTACK),
            decay: g.get(DECAY),
            sustain: g.get(SUSTAIN) * 0.01,
            release: g.get(RELEASE),
            ftype: g.index(FTYPE),
            cutoff: g.get(CUTOFF),
            resonance: g.get(RESONANCE) * 0.01,
            drive: g.get(DRIVE) * 0.01,
            transpose: g.get(TRANSPOSE),
            fine: g.get(FINE),
            volume: g.get(VOLUME),
            pan: g.get(PAN) * 0.01,
            velocity: g.get(VELOCITY) * 0.01,
        };
        self.shape = Shape::new(&voicing, self.rate, false);
    }

    fn note_on(&mut self, pitch: u8, velocity: f32) {
        let Some(set) = &self.files else {
            return;
        };
        // The zone whose keys hold the note; a slot whose file was not found stays silent.
        let Some(zone) = set.zones.iter().find(|z| (z.lo..=z.hi).contains(&pitch)) else {
            return;
        };
        let Some(slot) = set.slots.get(zone.slot).and_then(|s| s.as_ref()) else {
            return;
        };
        let step = 2f64.powf((f64::from(pitch) - f64::from(zone.root)) / 12.0);
        let level = self.shape.level(velocity);
        let Some(voice) = Voice::start(
            zone,
            slot,
            pitch,
            step,
            level,
            Trigger::Normal,
            self.born + 1,
        ) else {
            return;
        };
        for v in &mut self.voices {
            if v.pitch == pitch && !matches!(v.stage, Stage::Idle | Stage::Fade) {
                v.fade();
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
        self.born += 1;
        self.voices[k] = voice;
        self.velocities[k] = velocity;
    }

    fn note_off(&mut self, pitch: u8) {
        let pedal = self.pedal_down();
        for v in &mut self.voices {
            if v.pitch == pitch && matches!(v.stage, Stage::Attack | Stage::Decay) {
                if pedal {
                    v.held = true;
                } else {
                    v.release();
                }
            }
        }
    }

    /// A newer set keeps the voices whose file is still in their slot (with the zone's new
    /// cut) and fades the rest, so a held note does not jump into another file's middle.
    fn take_set(&mut self) {
        let set = self.files.as_deref();
        for (k, v) in self.voices.iter_mut().enumerate() {
            if !v.active() {
                continue;
            }
            let same = set.and_then(|s| {
                let slot = s.slots.get(v.slot)?.as_ref()?;
                let zone = s.zones.iter().find(|z| z.slot == v.slot)?;
                (data_of(slot) == v.data).then_some((zone, slot))
            });
            match same {
                Some((zone, slot)) => v.recut(zone, slot, self.shape.level(self.velocities[k])),
                None => v.fade(),
            }
        }
    }

    /// Hands the window the block's output peaks and the newest voice (ENG-072).
    fn show(&self, io: &AudioIo<'_>) {
        let probe = self.params.probe();
        probe.note_output(io.left, io.right);
        let newest = self
            .voices
            .iter()
            .filter(|v| v.active())
            .max_by_key(|v| v.born);
        probe.set_voice(
            newest.map(|v| {
                let (stage, position) = v.view();
                (v.pitch, stage, true, v.slot, position, v.env)
            }),
            newest.map_or(0, |v| v.born),
        );
    }

    /// Adds the voices into `[from, to)` of both channels.
    fn render(&mut self, io: &mut AudioIo<'_>, from: usize, to: usize) {
        let speed = self.shape.speed * self.bend * self.vibrato_step(to - from);
        let target = db_to_gain(self.glide.get(VOLUME));
        let (vol, dvol) = self.volume.toward(target, to - from);
        let Some(set) = &self.files else {
            return;
        };
        let shape = self.shape;
        for v in self.voices.iter_mut().filter(|v| v.active()) {
            let Some(slot) = set.slots.get(v.slot).and_then(|s| s.as_ref()) else {
                *v = Voice::IDLE;
                continue;
            };
            let mut vol = vol;
            for i in from..to {
                let Some((l, r)) = v.tick(slot, &shape, speed) else {
                    break;
                };
                vol += dvol;
                io.left[i] += (l * vol) as f32;
                io.right[i] += (r * vol) as f32;
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
        self.voices = [Voice::IDLE; VOICES];
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
        if let Some(inbox) = &mut self.files_in
            && inbox.poll(&mut self.files)
        {
            self.take_set();
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
        let probe = self.params.probe();
        probe.set_sounding(self.voices.iter().filter(|v| v.active()).count());
        if probe.watching() {
            self.show(io);
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

    /// A Sampler with one long sine at C3 (6 s), its envelope flat.
    fn tone() -> (Box<dyn Processor>, super::super::StockHandle) {
        let (p, h) = make("sampler");
        give(&h, vec![sine_file("tone_C3.wav", hz(60.0), 0.4, 288_000)]);
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        (p, h)
    }

    /// A Sampler with one file of `x` on both channels.
    fn of(name: &str, x: Vec<f32>) -> (Box<dyn Processor>, super::super::StockHandle) {
        let (p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        let data = crate::timeline::AudioData {
            left: x.clone(),
            right: x,
        };
        give(
            &h,
            vec![super::super::files::FileSlot::new(
                name,
                std::sync::Arc::new(data),
            )],
        );
        (p, h)
    }

    #[test]
    fn a_note_plays_at_its_pitch_from_every_zone() {
        let (mut p, h) = make("sampler");
        give(
            &h,
            vec![
                sine_file("low_C2.wav", hz(48.0), 0.4, 288_000),
                sine_file("mid_C3.wav", hz(60.0), 0.4, 288_000),
                sine_file("high_C5.wav", hz(84.0), 0.4, 288_000),
            ],
        );
        // On a root, and moved up and down from one.
        for pitch in [48u8, 60, 66, 54, 30, 90] {
            p.prepare(&setup());
            let x = play(p.as_mut(), pitch, 1.0, 40_000, 40_000);
            let f = pitch_of(&x, 4800, 24_000);
            let cents = 1200.0 * (f / hz(f64::from(pitch))).log2();
            assert!(cents.abs() < 2.0, "{pitch}: {f} Hz, {cents} cents");
        }
    }

    /// N4-smp-1: two files without a note in their names (both C3) split C3's keys, and each
    /// sounds: the root plays the first, the key above it the second.
    #[test]
    fn two_files_of_one_root_both_sound() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        set_plain(&h, "sustain", 100.0);
        give(
            &h,
            vec![
                sine_file("kick.wav", hz(60.0), 0.4, 288_000),
                sine_file("snare.wav", hz(60.0), 0.1, 288_000),
            ],
        );
        let mut level = |pitch| {
            p.prepare(&setup());
            peak(&play(p.as_mut(), pitch, 1.0, 20_000, 20_000)[4_800..])
        };
        let (kick, snare) = (level(60), level(61));
        assert!(kick > 0.0 && snare > 0.0, "{kick} {snare}");
        assert!(
            kick > snare * 2.0,
            "C3 plays kick.wav, C#3 snare.wav: {kick} {snare}"
        );
        assert!(level(30) > snare * 2.0 && level(100) < kick / 2.0);
    }

    #[test]
    fn transpose_and_fine_move_the_pitch() {
        let (mut p, h) = tone();
        set_plain(&h, "transpose", -12.0);
        set_plain(&h, "fine", 50.0);
        p.prepare(&setup());
        let x = play(p.as_mut(), 69, 1.0, 40_000, 40_000);
        let f = pitch_of(&x, 4800, 24_000);
        let want = 440.0 * 2f64.powf(-11.5 / 12.0);
        assert!((1200.0 * (f / want).log2()).abs() < 2.0, "{f} for {want}");
    }

    /// A file the song gave the device plays from its next block, pitched from its name's
    /// root note (ENG-077).
    #[test]
    fn a_read_file_plays_pitched_from_its_root() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        give(&h, vec![sine_file("tone_C3.wav", 261.63, 0.4, 48_000)]);
        let x = play(p.as_mut(), 72, 1.0, 40_000, 40_000);
        // The file holds 24_000 frames at an octave up; past it the voice is done.
        let f = pitch_of(&x, 4_800, 18_000);
        let cents = 1200.0 * (f / 523.26).log2();
        assert!(cents.abs() < 2.0, "{f} Hz, {cents} cents");
        assert_eq!(peak(&x[24_100..]), 0.0, "it played past the file's end");
    }

    /// An empty slot (a file that was not found) stays silent (ENG-077), as does a Sampler
    /// without files.
    #[test]
    fn a_missing_file_opens_silent() {
        let (mut p, _) = make("sampler");
        assert_eq!(peak(&play(p.as_mut(), 60, 1.0, 9_600, 9_600)), 0.0);
        let (mut p, h) = make("sampler");
        assert!(h.set_files(vec![None]));
        assert_eq!(peak(&play(p.as_mut(), 60, 1.0, 9_600, 9_600)), 0.0);
    }

    /// A set a full ring turned away (the device not polling) is not lost: once the device
    /// has polled, `resend_files` gets it through without another change (L2-smp-4).
    #[test]
    fn a_set_turned_away_by_a_full_ring_is_sent_again() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        let low = || vec![Some(sine_file("tone.wav", 220.0, 0.4, 48_000))];
        while h.set_files(low()) {}
        assert!(!h.set_files(vec![Some(sine_file("tone.wav", 440.0, 0.4, 48_000))]));
        assert!(!h.resend_files(), "the ring is still full");
        let x = play(p.as_mut(), 60, 1.0, 40_000, 40_000);
        assert!((pitch_of(&x, 4_800, 18_000) - 220.0).abs() < 2.0);
        assert!(h.resend_files());
        let x = play(p.as_mut(), 60, 1.0, 40_000, 40_000);
        let f = pitch_of(&x, 4_800, 18_000);
        assert!((f - 440.0).abs() < 2.0, "{f}");
        assert!(h.resend_files(), "nothing is left waiting");
    }

    /// A newer set replaces the one that plays, and the old one is handed back to be freed
    /// off the audio thread (ENG-077).
    #[test]
    fn a_newer_set_replaces_the_sound_while_playing() {
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        give(&h, vec![sine_file("tone.wav", 220.0, 0.4, 48_000)]);
        give(&h, vec![sine_file("tone.wav", 440.0, 0.4, 48_000)]);
        let x = play(p.as_mut(), 60, 1.0, 40_000, 40_000);
        let f = pitch_of(&x, 4_800, 18_000);
        assert!((f - 440.0).abs() < 2.0, "{f}");

        // Another file arriving while a note sounds fades the voice rather than jumping into
        // its middle; the same file with a new cut keeps playing (PLG-067).
        let (mut p, h) = make("sampler");
        set_plain(&h, "attack", 1.0);
        give(&h, vec![sine_file("tone.wav", 440.0, 0.4, 96_000)]);
        let block = |p: &mut dyn Processor, notes: &[(usize, crate::processor::NoteKind)]| {
            let silence = vec![0.0; 4_800];
            render(p, &silence, &silence, notes).0
        };
        block(p.as_mut(), &[(0, on(60, 1.0))]);
        assert!(h.set("file1.end", 0.9));
        assert!(
            peak(&block(p.as_mut(), &[])) > 0.3,
            "a new cut cut the note"
        );
        give(&h, vec![sine_file("tone.wav", 220.0, 0.4, 96_000)]);
        let l = block(p.as_mut(), &[]);
        assert_eq!(
            peak(&l[2_000..]),
            0.0,
            "the voice kept playing past the swap"
        );
    }

    #[test]
    fn the_envelope_follows_the_adsr() {
        let (mut p, h) = tone();
        set_plain(&h, "attack", 100.0);
        set_plain(&h, "decay", 200.0);
        set_plain(&h, "sustain", 50.0);
        set_plain(&h, "release", 100.0);
        p.prepare(&setup());
        let x = play(p.as_mut(), 60, 1.0, 48_000, 72_000);
        let level = |ms: f64| rms(&x[10 + (ms * 48.0) as usize..][..960]);
        // What the file gives at full level.
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
        let (mut p, h) = tone();
        let level = |p: &mut dyn Processor, v: f32| {
            p.prepare(&setup());
            rms(&play(p, 60, v, 9_600, 9_600))
        };
        set_plain(&h, "velocity", 100.0);
        // Full spread: the square of the velocity.
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 0.25).abs() < 0.005, "{r}");
        set_plain(&h, "velocity", 0.0);
        let r = level(p.as_mut(), 0.5) / level(p.as_mut(), 1.0);
        assert!((r - 1.0).abs() < 1e-6, "{r}");
    }

    /// The filter: a lowpass darkens, a highpass thins, a bandpass keeps its middle
    /// (PLG-067).
    #[test]
    fn the_filter_takes_what_its_type_asks() {
        // A low and a high tone in one file.
        let lo = sine(200.0, 0.3, 96_000);
        let hi = sine(5_000.0, 0.3, 96_000);
        let both: Vec<f32> = lo.iter().zip(&hi).map(|(a, b)| a + b).collect();
        let (mut p, h) = of("two_C3.wav", both);
        // The highs: what changes from one frame to the next.
        let highs = |x: &[f32]| rms(&x.windows(2).map(|w| w[1] - w[0]).collect::<Vec<_>>());
        let take = |p: &mut dyn Processor| {
            p.prepare(&setup());
            play(p, 60, 1.0, 24_000, 24_000)
        };
        let open = take(p.as_mut());
        set_plain(&h, "cutoff", 400.0);
        let dark = take(p.as_mut());
        assert!(highs(&dark) < highs(&open) * 0.1, "lowpass");
        set_plain(&h, "ftype", 2.0);
        set_plain(&h, "cutoff", 2_000.0);
        let thin = take(p.as_mut());
        assert!(
            rms(&thin) < rms(&open) * 0.8 && highs(&thin) > highs(&open) * 0.8,
            "highpass"
        );
        set_plain(&h, "ftype", 1.0);
        set_plain(&h, "cutoff", 1_000.0);
        let mid = take(p.as_mut());
        assert!(rms(&mid) < rms(&open) * 0.5, "bandpass");
        assert!(mid.iter().chain(&thin).all(|v| v.is_finite()));
    }

    /// A zone's Start and End cut its file, Reverse plays it backwards, and Normalize lifts
    /// its loudest sample to full scale (PLG-067).
    #[test]
    fn a_zones_cut_shapes_what_it_plays() {
        // A second of a quiet tone, then a second of silence.
        let mut x = sine(440.0, 0.1, 48_000);
        x.extend(std::iter::repeat_n(0.0, 48_000));
        let (mut p, h) = of("half_A3.wav", x);
        let take = |p: &mut dyn Processor| {
            p.prepare(&setup());
            play(p, 69, 1.0, 96_000, 96_000)
        };
        let whole = take(p.as_mut());
        assert!(rms(&whole[1_000..47_000]) > 0.05);
        assert_eq!(peak(&whole[48_100..]), 0.0);
        // From the middle: only the silence plays.
        assert!(h.set("file1.start", 0.5));
        assert_eq!(peak(&take(p.as_mut())), 0.0);
        // To a quarter: it ends there.
        assert!(h.set("file1.start", 0.0));
        assert!(h.set("file1.end", 0.25));
        let cut = take(p.as_mut());
        assert!(rms(&cut[1_000..23_000]) > 0.05);
        assert_eq!(peak(&cut[24_100..]), 0.0);
        // Backwards: the silence first, then the tone.
        assert!(h.set("file1.end", 1.0));
        assert!(h.set("file1.reverse", 1.0));
        let back = take(p.as_mut());
        assert_eq!(peak(&back[..47_900]), 0.0);
        assert!(rms(&back[49_000..95_000]) > 0.05);
        // Normalized: the tone at full scale.
        assert!(h.set("file1.reverse", 0.0));
        assert!(h.set("file1.normalize", 1.0));
        let loud = take(p.as_mut());
        let r = peak(&loud[1_000..47_000]) / peak(&whole[1_000..47_000]);
        assert!((r - 10.0).abs() < 0.2, "{r}");
    }

    /// A looped zone plays round its loop while the key is down, the seam crossfaded
    /// (PLG-067).
    #[test]
    fn a_looped_zone_sounds_while_the_key_is_down() {
        let (mut p, h) = tone();
        give(&h, vec![sine_file("tone_C3.wav", hz(60.0), 0.4, 24_000)]);
        assert!(h.set("file1.loop", 1.0));
        p.prepare(&setup());
        let x = play(p.as_mut(), 60, 1.0, 96_000, 120_000);
        // Half a second of file, held two seconds: still at its level near the end.
        let level = rms(&x[86_000..95_000]);
        assert!(level > 0.2, "{level}");
        assert!(x.iter().all(|v| v.is_finite()));
        let jump = x[2_000..95_000]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // A sine at C3 moves at most 0.4 · 2π · 262 / 48000 a frame; the seam a little more.
        assert!(jump < 0.03, "{jump}");
        // Let go, it releases and ends.
        assert_eq!(peak(&x[96_010 + 20_000..]), 0.0);
    }

    #[test]
    fn a_chord_takes_a_voice_a_note_and_more_notes_take_the_oldest() {
        use super::super::files::{FileSet, channel};
        let bank = std::sync::Arc::new(super::super::bank::ParamBank::new(&xirius_params::SAMPLER));
        let (mut out, inbox) = channel();
        let mut p = super::Sampler::new(super::super::bank::Params::new(bank), Some(inbox));
        let slots = vec![Some(sine_file("tone_C3.wav", 261.63, 0.2, 48_000))];
        assert!(out.set(FileSet::layout(
            &xirius_params::SAMPLER,
            &std::collections::BTreeMap::new(),
            slots
        )));
        p.prepare(&setup());
        let silence = vec![0.0; 9_600];
        let notes: Vec<_> = (0..40).map(|k| (k * 10, on(30 + k as u8, 0.8))).collect();
        let (l, _) = render(&mut p, &silence, &silence, &notes);
        assert!(l.iter().all(|v| v.is_finite()));
        // The 32 newest notes sound; the first 8 gave way.
        let mut playing: Vec<u8> = p
            .voices
            .iter()
            .filter(|v| v.active())
            .map(|v| v.pitch)
            .collect();
        playing.sort();
        assert_eq!(playing, (38..70).collect::<Vec<u8>>());
    }

    #[test]
    fn a_note_played_again_does_not_click() {
        let (mut p, _) = tone();
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
        let (mut p, _) = tone();
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
        let (mut p, h) = tone();
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
        let (mut p, _) = tone();
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
        let (mut p, h) = tone();
        set_plain(&h, "resonance", 100.0);
        set_plain(&h, "cutoff", 1000.0);
        set_plain(&h, "drive", 100.0);
        assert_silent(p.as_mut());
    }

    /// PLG-066: a note played and let go leaves no voice of the Sampler sounding.
    #[test]
    fn a_note_let_go_leaves_no_voice_sounding() {
        let (mut p, h) = tone();
        let held = vec![0.0; 4_800];
        render(p.as_mut(), &held, &held, &[(0, on(60, 0.8))]);
        assert!(h.probe().sounding() > 0);
        let after = vec![0.0; 48_000 * 4];
        render(p.as_mut(), &after, &after, &[(0, off(60))]);
        assert_eq!(h.probe().sounding(), 0);
    }
}
