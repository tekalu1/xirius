//! The Drum Kit (PLG-068): 128 pads (banks A to H of 16) that play the files the song gave
//! it, each through its own voice of the Sampler's ([`super::voice`]).
//!
//! A pad's files are its layers ([`FileSet::layout`]): the hit's velocity picks one, or they
//! take turns, or one is picked at random, or all play at once, as the pad's layer mode asks.
//! The pad's trigger says what its key does after the hit: a one shot plays to its end, a
//! loop or a normal pad sounds while the key is down, a toggle starts on one press and lets
//! go on the next. A pad that chokes itself cuts its last hit when hit again; pads in the
//! same choke group cut each other. A muted pad is silent; while any pad is soloed, only the
//! soloed ones sound. Without files it is silent. No latency.
//!
//! Notes: C1 (36) is pad A1 and each key up is the next pad, round the 128 keys.
//!
//! Controllers (EDT-004) leave it alone: pads do not bend or take vibrato.

use std::sync::Arc;

use xirius_params::PADS;

use super::bank::Params;
use super::dsp::{Glide, Ramp, STEP, db_to_gain};
use super::files::{FileInbox, FileSet, LayerMode, Trigger};
use super::voice::{Shape, Stage, Voice, data_of};
use crate::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::DRUM_KIT`.
const VOLUME: usize = 0;

/// The key of pad A1.
const PAD_BASE: u8 = 36;
const VOICES: usize = 32;

/// The pad a key plays.
fn pad_of(pitch: u8) -> usize {
    (usize::from(pitch) + PADS - usize::from(PAD_BASE)) % PADS
}

pub struct DrumKit {
    params: Params,
    glide: Glide,
    rate: f64,
    voices: [Voice; VOICES],
    /// The velocity each voice was hit at, to work out its level again in a newer set.
    velocities: [f32; VOICES],
    born: u64,
    /// Each pad's voicing at the rate, worked out when a set arrives.
    shapes: [Shape; PADS],
    /// The next layer of each pad that plays its layers in turn.
    turns: [u32; PADS],
    /// Picks the layer of a pad that plays one at random.
    seed: u32,
    volume: Ramp,
    until_step: usize,
    files_in: Option<FileInbox>,
    files: Option<Arc<FileSet>>,
}

impl DrumKit {
    pub fn new(params: Params, files_in: Option<FileInbox>) -> DrumKit {
        DrumKit {
            glide: Glide::new(&xirius_params::DRUM_KIT),
            params,
            rate: 48_000.0,
            voices: [Voice::IDLE; VOICES],
            velocities: [0.0; VOICES],
            born: 0,
            shapes: [Shape::default(); PADS],
            turns: [0; PADS],
            seed: 0x9E37_79B9,
            volume: Ramp::new(1.0),
            until_step: 0,
            files_in,
            files: None,
        }
    }

    /// Works out each pad's voicing at the rate.
    fn shape_pads(&mut self) {
        let Some(set) = &self.files else {
            return;
        };
        for (shape, pad) in self.shapes.iter_mut().zip(&set.pads) {
            *shape = Shape::new(&pad.voicing, self.rate, true);
        }
    }

    fn random(&mut self) -> u32 {
        // xorshift32
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }

    /// Takes a voice for a new hit: a free one, else the one that is fading, else the oldest.
    fn take_voice(&self) -> usize {
        (0..VOICES)
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
            .unwrap_or(0)
    }

    fn note_on(&mut self, pitch: u8, velocity: f32) {
        let Some(set) = self.files.clone() else {
            return;
        };
        let p = pad_of(pitch);
        let Some(pad) = set.pads.get(p) else {
            return;
        };
        if pad.count == 0 || pad.mute || (set.soloed && !pad.solo) {
            return;
        }
        if pad.trigger == Trigger::Toggle {
            // The second press lets the first go.
            let mut sounding = false;
            for v in self.voices.iter_mut().filter(|v| v.pad == p) {
                if matches!(v.stage, Stage::Attack | Stage::Decay) {
                    v.release();
                    sounding = true;
                }
            }
            if sounding {
                return;
            }
        }
        // Choke: the pad's own last hit, or every pad of its group.
        if pad.choke > 0 {
            for v in self.voices.iter_mut().filter(|v| v.active()) {
                let same = match pad.choke {
                    1 => v.pad == p,
                    group => set.pads[v.pad].choke == group,
                };
                if same {
                    v.fade();
                }
            }
        }
        let layers = &set.zones[pad.first..pad.first + pad.count];
        let at = (velocity.clamp(0.0, 1.0) * 127.0).round() as u8;
        let (from, to) = match pad.layer_mode {
            LayerMode::Velocity => {
                let k = layers
                    .iter()
                    .position(|z| (z.lo..=z.hi).contains(&at))
                    .unwrap_or(layers.len() - 1);
                (k, k + 1)
            }
            LayerMode::RoundRobin => {
                let k = self.turns[p] as usize % layers.len();
                self.turns[p] = ((k + 1) % layers.len()) as u32;
                (k, k + 1)
            }
            LayerMode::Random => {
                let k = self.random() as usize % layers.len();
                (k, k + 1)
            }
            LayerMode::Stack => (0, layers.len()),
        };
        let level = self.shapes[p].level(velocity);
        if self.params.probe().watching() {
            self.params.probe().add_hit(p);
        }
        for zone in &layers[from..to] {
            let Some(slot) = set.slots.get(zone.slot).and_then(|s| s.as_ref()) else {
                continue;
            };
            let Some(voice) =
                Voice::start(zone, slot, pitch, 1.0, level, pad.trigger, self.born + 1)
            else {
                continue;
            };
            let k = self.take_voice();
            self.born += 1;
            self.voices[k] = voice;
            self.velocities[k] = velocity;
        }
    }

    fn note_off(&mut self, pitch: u8) {
        let p = pad_of(pitch);
        for v in self.voices.iter_mut().filter(|v| v.pad == p && v.active()) {
            if matches!(v.trigger, Trigger::Loop | Trigger::Normal) {
                v.release();
            }
        }
    }

    /// A newer set keeps the voices whose file is still in their slot (with the pad's new
    /// cut and voicing) and fades the rest.
    fn take_set(&mut self) {
        self.shape_pads();
        let set = self.files.as_deref();
        for (k, v) in self.voices.iter_mut().enumerate() {
            if !v.active() {
                continue;
            }
            let same = set.and_then(|s| {
                let slot = s.slots.get(v.slot)?.as_ref()?;
                let zone = s.zones.iter().find(|z| z.slot == v.slot)?;
                (data_of(slot) == v.data && zone.pad == v.pad).then_some((zone, slot))
            });
            match same {
                Some((zone, slot)) => {
                    v.recut(zone, slot, self.shapes[v.pad].level(self.velocities[k]));
                }
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
        probe.set_voice(newest.map(|v| {
            let (stage, position) = v.view();
            (v.pitch, stage, true, v.slot, position, v.env)
        }));
    }

    /// Adds the voices into `[from, to)` of both channels.
    fn render(&mut self, io: &mut AudioIo<'_>, from: usize, to: usize) {
        let target = db_to_gain(self.glide.get(VOLUME));
        let (vol, dvol) = self.volume.toward(target, to - from);
        let Some(set) = &self.files else {
            return;
        };
        for v in self.voices.iter_mut().filter(|v| v.active()) {
            let Some(slot) = set.slots.get(v.slot).and_then(|s| s.as_ref()) else {
                *v = Voice::IDLE;
                continue;
            };
            let shape = self.shapes[v.pad];
            let mut vol = vol;
            for i in from..to {
                let Some((l, r)) = v.tick(slot, &shape, shape.speed) else {
                    break;
                };
                vol += dvol;
                io.left[i] += (l * vol) as f32;
                io.right[i] += (r * vol) as f32;
            }
        }
    }
}

impl Processor for DrumKit {
    fn name(&self) -> &str {
        "Drum Kit"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        self.voices = [Voice::IDLE; VOICES];
        self.turns = [0; PADS];
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.shape_pads();
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
                    NoteKind::On { pitch, velocity } => self.note_on(pitch, velocity),
                    NoteKind::Off { pitch } => self.note_off(pitch),
                    NoteKind::Control { .. } => {}
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

    /// A Drum Kit with `files` (a name and a level each, a 200 ms tone at 440 Hz), with the
    /// pads' values `set` (`A1.trigger`, 0..1).
    fn kit(
        files: &[(&str, f64)],
        set: &[(&str, f64)],
    ) -> (Box<dyn Processor>, super::super::StockHandle) {
        let (p, h) = make("drum-kit");
        for (k, v) in set {
            assert!(h.set(k, *v), "{k}");
        }
        give(
            &h,
            files
                .iter()
                .map(|(name, amp)| sine_file(name, 440.0, *amp, 9_600))
                .collect(),
        );
        (p, h)
    }

    /// The 0..1 value of a pad's choice `index` (`A1.trigger` 3 is Toggle).
    fn choice(h: &super::super::StockHandle, key: &str, index: f64) -> f64 {
        let spec = h.def().resolve(key).unwrap().spec;
        spec.to_norm(index)
    }

    fn hits(
        p: &mut dyn Processor,
        notes: &[(usize, crate::processor::NoteKind)],
        frames: usize,
    ) -> Vec<f32> {
        // Long enough for a 200 ms hit to end, so the next call starts clear.
        let silence = vec![0.0; frames.max(12_000)];
        let (l, r) = render(p, &silence, &silence, notes);
        assert_eq!(l, r);
        l
    }

    #[test]
    fn each_file_is_a_pad_from_c1_up() {
        let (mut p, _) = kit(&[("kick.wav", 0.4), ("snare.wav", 0.2)], &[]);
        let a1 = hits(p.as_mut(), &[(0, on(36, 1.0))], 24_000);
        let a2 = hits(p.as_mut(), &[(0, on(37, 1.0))], 24_000);
        let a3 = hits(p.as_mut(), &[(0, on(38, 1.0))], 24_000);
        assert!(
            (peak(&a1) / peak(&a2) - 2.0).abs() < 0.05,
            "{} {}",
            peak(&a1),
            peak(&a2)
        );
        assert_eq!(peak(&a3), 0.0, "a pad without a file");
        // One shot: the key let go at once still plays the whole file.
        let short = hits(p.as_mut(), &[(0, on(36, 1.0)), (100, off(36))], 24_000);
        assert!(rms(&short[8_000..9_000]) > 0.1);
        assert_eq!(peak(&short[9_700..]), 0.0);
    }

    #[test]
    fn a_files_pad_value_moves_it() {
        let (mut p, h) = kit(&[("kick.wav", 0.4)], &[]);
        assert!(h.set(
            "file1.pad",
            h.def().resolve("file1.pad").unwrap().spec.to_norm(17.0)
        ));
        assert_eq!(peak(&hits(p.as_mut(), &[(0, on(36, 1.0))], 4_800)), 0.0);
        // B2 is pad 17, key 53.
        assert!(peak(&hits(p.as_mut(), &[(0, on(53, 1.0))], 4_800)) > 0.3);
    }

    #[test]
    fn velocity_layers_pick_by_how_hard_and_round_robin_takes_turns() {
        let soft_loud = [("hit_v1.wav", 0.1), ("hit_v2.wav", 0.4)];
        let top = |h: &super::super::StockHandle| {
            let spec = h.def().resolve("file1.top").unwrap().spec;
            spec.to_norm(64.0)
        };
        let (mut p, h) = kit(&soft_loud, &[]);
        let t = top(&h);
        assert!(h.set("file1.top", t));
        assert!(h.set("file2.pad", 0.0));
        set_plain(&h, "volume", 0.0);
        let at = |p: &mut dyn Processor, v: f32| peak(&hits(p, &[(0, on(36, v))], 4_800));
        // A1's velocity spread is 80 %: what plays is the layer, scaled by the velocity.
        let soft = at(p.as_mut(), 0.3);
        let loud = at(p.as_mut(), 0.9);
        assert!(loud / soft > 4.0 * 0.8, "{soft} {loud}");
        // Round robin: one then the other, whatever the velocity.
        let rr = choice(&h, "A1.layer_mode", 1.0);
        assert!(h.set("A1.layer_mode", rr));
        assert!(h.set("A1.velocity", 0.0));
        let first = at(p.as_mut(), 1.0);
        let second = at(p.as_mut(), 1.0);
        let third = at(p.as_mut(), 1.0);
        assert!(
            (first - 0.1).abs() < 0.01 && (second - 0.4).abs() < 0.02,
            "{first} {second}"
        );
        assert!((third - first).abs() < 1e-3);
    }

    #[test]
    fn triggers_answer_the_key_as_they_say() {
        // A looping pad sounds while the key is down, then releases.
        let (mut p, h) = kit(&[("pad.wav", 0.4)], &[]);
        let lp = choice(&h, "A1.trigger", 1.0);
        assert!(h.set("A1.trigger", lp));
        let x = hits(p.as_mut(), &[(0, on(36, 1.0)), (48_000, off(36))], 72_000);
        assert!(rms(&x[40_000..47_000]) > 0.1, "it stopped looping");
        assert_eq!(peak(&x[48_000 + 20_000..]), 0.0);
        // A toggle starts on one press and lets go on the next; its key up does nothing.
        let tg = choice(&h, "A1.trigger", 3.0);
        assert!(h.set("A1.trigger", tg));
        assert!(h.set("A1.loop", 1.0));
        let x = hits(
            p.as_mut(),
            &[(0, on(36, 1.0)), (10, off(36)), (24_000, on(36, 1.0))],
            48_000,
        );
        assert!(rms(&x[20_000..23_000]) > 0.1, "the key up stopped it");
        assert_eq!(
            peak(&x[24_000 + 20_000..]),
            0.0,
            "the second press did not stop it"
        );
    }

    #[test]
    fn a_choke_cuts_the_pad_or_its_group() {
        let (mut p, h) = kit(&[("open.wav", 0.4), ("closed.wav", 0.4)], &[]);
        let lp = choice(&h, "A1.trigger", 1.0);
        assert!(h.set("A1.trigger", lp));
        let group = choice(&h, "A1.choke", 2.0);
        assert!(h.set("A1.choke", group));
        assert!(h.set("A2.choke", group));
        // The open hat loops until the closed one in its group cuts it.
        let x = hits(
            p.as_mut(),
            &[(0, on(36, 1.0)), (24_000, on(37, 1.0))],
            48_000,
        );
        assert!(rms(&x[20_000..23_000]) > 0.1);
        assert_eq!(peak(&x[24_000 + 9_700..]), 0.0, "the open hat kept ringing");
        // Without the group it keeps going.
        assert!(h.set("A2.choke", 0.0));
        let x = hits(
            p.as_mut(),
            &[(0, on(36, 1.0)), (24_000, on(37, 1.0))],
            48_000,
        );
        assert!(rms(&x[40_000..47_000]) > 0.1);
    }

    #[test]
    fn mute_and_solo_pick_the_pads_that_sound() {
        let (mut p, h) = kit(&[("a.wav", 0.4), ("b.wav", 0.4)], &[]);
        let both = [(0, on(36, 1.0)), (0, on(37, 1.0))];
        let full = peak(&hits(p.as_mut(), &both, 4_800));
        assert!(h.set("A1.mute", 1.0));
        let one = peak(&hits(p.as_mut(), &both, 4_800));
        assert!((one / full - 0.5).abs() < 0.02, "{one} {full}");
        assert!(h.set("A1.mute", 0.0));
        assert!(h.set("A2.solo", 1.0));
        assert_eq!(peak(&hits(p.as_mut(), &[(0, on(36, 1.0))], 4_800)), 0.0);
        assert!(peak(&hits(p.as_mut(), &[(0, on(37, 1.0))], 4_800)) > 0.3);
    }

    #[test]
    fn a_pads_voice_sets_its_level_and_pitch() {
        let (mut p, h) = kit(&[("a.wav", 0.4)], &[]);
        let plain = |k: &str, v: f64| h.def().resolve(k).unwrap().spec.to_norm(v);
        let open = peak(&hits(p.as_mut(), &[(0, on(36, 1.0))], 4_800));
        assert!(h.set("A1.volume", plain("A1.volume", -6.0)));
        let quiet = peak(&hits(p.as_mut(), &[(0, on(36, 1.0))], 4_800));
        assert!((db(quiet / open) + 6.0).abs() < 0.2, "{}", db(quiet / open));
        // An octave up plays the file in half the time.
        assert!(h.set("A1.volume", plain("A1.volume", 0.0)));
        assert!(h.set("A1.transpose", plain("A1.transpose", 12.0)));
        let up = hits(p.as_mut(), &[(0, on(36, 1.0))], 9_600);
        assert!(rms(&up[3_000..4_500]) > 0.1);
        assert_eq!(peak(&up[4_900..]), 0.0);
    }

    #[test]
    fn a_newer_set_fades_the_playing_pad() {
        let (mut p, h) = kit(&[("a.wav", 0.4)], &[]);
        let short = vec![0.0; 1_000];
        render(p.as_mut(), &short, &short, &[(0, on(36, 1.0))]);
        give(&h, vec![sine_file("b.wav", 220.0, 0.4, 9_600)]);
        let x = hits(p.as_mut(), &[], 4_800);
        assert_eq!(peak(&x[1_000..]), 0.0);
    }

    #[test]
    fn a_note_without_files_is_silent() {
        let (mut p, _) = make("drum-kit");
        assert_eq!(peak(&hits(p.as_mut(), &[(0, on(36, 1.0))], 4_800)), 0.0);
    }

    #[test]
    fn silence_without_notes() {
        let (mut p, _) = kit(&[("a.wav", 0.4)], &[]);
        assert_silent(p.as_mut());
    }

    /// PLG-066: a hit leaves no voice of the Drum Kit sounding once it has played out.
    #[test]
    fn a_note_let_go_leaves_no_voice_sounding() {
        let (mut p, h) = kit(&[("a.wav", 0.4)], &[]);
        let short = vec![0.0; 1_000];
        render(p.as_mut(), &short, &short, &[(0, on(36, 0.8))]);
        assert!(h.probe().sounding() > 0);
        hits(p.as_mut(), &[(0, off(36))], 48_000);
        assert_eq!(h.probe().sounding(), 0);
    }
}
