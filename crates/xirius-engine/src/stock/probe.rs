//! What a built-in shows of its sound while its window watches (ENG-072): the gain
//! reduction, the peaks in and out, the onsets of what goes into a delay or a reverb, the
//! hits of the Drum Kit and the Sampler's newest voice; and the EQ's samples for the
//! analyser (ENG-073).
//!
//! The audio thread writes only while [`Probe::watching`], into atomics and rings: nothing
//! locks or allocates. The window reads them on its own clock; only its side locks.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use rtrb::Consumer;

use crate::rt_lock::Mutex;

/// The groups of the Drum Kit, in the order of its group levels.
pub const HIT_GROUPS: usize = 6;

/// `voice[0]` when no voice plays.
const NO_VOICE: u32 = u32::MAX;
/// The bit of `voice[0]` that says the source is a file slot, not a synthesized sound.
const FILE_BIT: u32 = 0x8000;

pub struct Probe {
    watching: AtomicBool,
    /// The most gain reduction (dB, 0 or more) of the last block, as `f32` bits.
    gr: AtomicU32,
    /// The peaks (0 or more, `f32` bits) since the window last took them: left, right.
    peak_in: [AtomicU32; 2],
    peak_out: [AtomicU32; 2],
    /// Onsets counted in the input since the device was made.
    onsets: AtomicU32,
    /// Hits of each group since the device was made.
    hits: [AtomicU32; HIT_GROUPS],
    /// The Sampler's newest voice:
    /// - `[0]`: [`NO_VOICE`], or `pitch << 24 | stage << 16 | source`, where `stage` is a
    ///   [`VoiceStage`] and `source` is a file slot with [`FILE_BIT`] set, or the index of a
    ///   synthesized sound;
    /// - `[1]`: where it reads, 0..1 of the source's length, as `f32` bits;
    /// - `[2]`: its envelope, 0..1, as `f32` bits.
    ///
    /// The three are written one after the other, so a read can mix two blocks.
    voice: [AtomicU32; 3],
    /// How many voices of the Sampler or the Drum Kit sound, after the last block (PLG-066).
    /// Written whether or not a window watches.
    sounding: AtomicU32,
    /// Which of the EQ's samples go into the rings, as a [`Tap`].
    tap: AtomicU8,
    /// The window's ends of the EQ's rings: the mono input and output.
    rings: Mutex<Option<(Consumer<f32>, Consumer<f32>)>>,
}

/// What the EQ hands its analyser: its input, its output, both or neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Tap {
    Off = 0,
    Input = 1,
    #[default]
    Output = 2,
    Both = 3,
}

impl Tap {
    pub const ALL: [Tap; 4] = [Tap::Off, Tap::Input, Tap::Output, Tap::Both];

    pub fn input(self) -> bool {
        matches!(self, Tap::Input | Tap::Both)
    }

    pub fn output(self) -> bool {
        matches!(self, Tap::Output | Tap::Both)
    }

    fn from_bits(b: u8) -> Tap {
        match b {
            0 => Tap::Off,
            1 => Tap::Input,
            3 => Tap::Both,
            _ => Tap::Output,
        }
    }
}

impl Default for Probe {
    fn default() -> Probe {
        Probe::new()
    }
}

impl Probe {
    pub fn new() -> Probe {
        Probe {
            watching: AtomicBool::new(false),
            gr: AtomicU32::new(0),
            peak_in: [AtomicU32::new(0), AtomicU32::new(0)],
            peak_out: [AtomicU32::new(0), AtomicU32::new(0)],
            onsets: AtomicU32::new(0),
            hits: std::array::from_fn(|_| AtomicU32::new(0)),
            voice: [
                AtomicU32::new(NO_VOICE),
                AtomicU32::new(0),
                AtomicU32::new(0),
            ],
            sounding: AtomicU32::new(0),
            tap: AtomicU8::new(Tap::default() as u8),
            rings: Mutex::new(None),
        }
    }

    /// Starts or stops the writes. Starting clears the reduction, the peaks and the voice, so
    /// nothing from an earlier watch shows.
    pub fn watch(&self, on: bool) {
        if on && !self.watching() {
            self.gr.store(0, Ordering::Relaxed);
            for p in self.peak_in.iter().chain(&self.peak_out) {
                p.store(0, Ordering::Relaxed);
            }
            self.voice[0].store(NO_VOICE, Ordering::Relaxed);
        }
        self.watching.store(on, Ordering::Release);
    }

    pub fn watching(&self) -> bool {
        self.watching.load(Ordering::Acquire)
    }

    /// The most gain reduction of the last block, in dB (0 or more).
    pub fn gr_db(&self) -> f32 {
        f32::from_bits(self.gr.load(Ordering::Relaxed))
    }

    /// The peaks (linear, 0 or more) since the last call, `(input, output)`, each
    /// `[left, right]`; they start again from 0.
    pub fn take_peaks(&self) -> ([f32; 2], [f32; 2]) {
        let take = |p: &[AtomicU32; 2]| {
            p.each_ref()
                .map(|a| f32::from_bits(a.swap(0, Ordering::Relaxed)))
        };
        (take(&self.peak_in), take(&self.peak_out))
    }

    /// Onsets counted in the input so far (the Delay and the Reverb); the window shows one for
    /// every rise. It wraps.
    pub fn onsets(&self) -> u32 {
        self.onsets.load(Ordering::Relaxed)
    }

    /// Hits of each group so far (the Drum Kit's made hits; a pad playing a file has no
    /// group). They wrap.
    pub fn hits(&self) -> [u32; HIT_GROUPS] {
        self.hits.each_ref().map(|h| h.load(Ordering::Relaxed))
    }

    /// The Sampler's newest voice, if one plays.
    pub fn voice(&self) -> Option<VoiceView> {
        let head = self.voice[0].load(Ordering::Relaxed);
        if head == NO_VOICE {
            return None;
        }
        Some(VoiceView {
            pitch: (head >> 24) as u8,
            stage: VoiceStage::from_bits((head >> 16) as u8),
            file: head & FILE_BIT != 0,
            source: (head & (FILE_BIT - 1)) as u16,
            position: f32::from_bits(self.voice[1].load(Ordering::Relaxed)),
            env: f32::from_bits(self.voice[2].load(Ordering::Relaxed)),
        })
    }

    /// How many voices of the Sampler or the Drum Kit sounded after the last block (0 for
    /// the other built-ins).
    pub fn sounding(&self) -> u32 {
        self.sounding.load(Ordering::Relaxed)
    }

    /// Which samples the EQ hands its analyser while watched.
    pub fn tap(&self) -> Tap {
        Tap::from_bits(self.tap.load(Ordering::Relaxed))
    }

    pub fn set_tap(&self, tap: Tap) {
        self.tap.store(tap as u8, Ordering::Relaxed);
    }

    /// Appends the samples the EQ handed over since the last call: the mono (L+R)/2 of its
    /// input and of its output, as [`Probe::tap`] picks. A ring holds 2048; what comes while
    /// it is full is dropped, so the window takes them often. False for a built-in with no
    /// rings (all but the EQ). Locks: the window's side only.
    pub fn take_samples(&self, input: &mut Vec<f32>, output: &mut Vec<f32>) -> bool {
        let mut rings = self.rings.lock().unwrap_or_else(|e| e.into_inner());
        let Some((i, o)) = rings.as_mut() else {
            return false;
        };
        for (ring, to) in [(i, input), (o, output)] {
            let n = ring.slots();
            if let Ok(chunk) = ring.read_chunk(n) {
                to.extend(chunk);
            }
        }
        true
    }

    /// Keeps the window's ends of the rings. Called where the processor is made, off the
    /// audio thread.
    pub(crate) fn give_rings(&self, input: Consumer<f32>, output: Consumer<f32>) {
        *self.rings.lock().unwrap_or_else(|e| e.into_inner()) = Some((input, output));
    }

    // What the audio thread writes. Each is for a block, after a check of `watching`.

    pub(crate) fn set_gr(&self, db: f64) {
        self.gr
            .store((db.max(0.0) as f32).to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn note_input(&self, left: &[f32], right: &[f32]) {
        note_peaks(&self.peak_in, left, right);
    }

    pub(crate) fn note_output(&self, left: &[f32], right: &[f32]) {
        note_peaks(&self.peak_out, left, right);
    }

    pub(crate) fn add_onsets(&self, n: u32) {
        if n > 0 {
            self.onsets.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Every block, watched or not.
    pub(crate) fn set_sounding(&self, n: usize) {
        self.sounding.store(n as u32, Ordering::Relaxed);
    }

    pub(crate) fn add_hit(&self, group: usize) {
        if let Some(h) = self.hits.get(group) {
            h.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// `position` is 0..1 of the source; `source` is kept to 15 bits.
    pub(crate) fn set_voice(&self, playing: Option<(u8, VoiceStage, bool, usize, f64, f64)>) {
        let Some((pitch, stage, file, source, position, env)) = playing else {
            self.voice[0].store(NO_VOICE, Ordering::Relaxed);
            return;
        };
        let source = (source as u32).min(FILE_BIT - 1) | if file { FILE_BIT } else { 0 };
        self.voice[1].store(
            (position.clamp(0.0, 1.0) as f32).to_bits(),
            Ordering::Relaxed,
        );
        self.voice[2].store((env.clamp(0.0, 1.0) as f32).to_bits(), Ordering::Relaxed);
        let head = u32::from(pitch) << 24 | u32::from(stage as u8) << 16 | source;
        self.voice[0].store(head, Ordering::Relaxed);
    }
}

/// Raises each channel's peak to the block's, if higher. Non-negative `f32`s order as their
/// bits do, so the atomic's own `fetch_max` does it.
fn note_peaks(peaks: &[AtomicU32; 2], left: &[f32], right: &[f32]) {
    for (p, x) in peaks.iter().zip([left, right]) {
        let m = x.iter().fold(0f32, |m, v| m.max(v.abs()));
        if m > 0.0 {
            p.fetch_max(m.to_bits(), Ordering::Relaxed);
        }
    }
}

/// Where a Sampler voice is in its envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VoiceStage {
    Attack = 0,
    Decay = 1,
    Release = 2,
    /// Giving way to a new note.
    Fade = 3,
}

impl VoiceStage {
    fn from_bits(b: u8) -> VoiceStage {
        match b {
            0 => VoiceStage::Attack,
            1 => VoiceStage::Decay,
            2 => VoiceStage::Release,
            _ => VoiceStage::Fade,
        }
    }
}

/// The Sampler's newest voice as [`Probe::voice`] reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceView {
    pub pitch: u8,
    pub stage: VoiceStage,
    /// Whether it plays a file of the song (`source` is its slot) or a synthesized sound
    /// (`source` is the sound).
    pub file: bool,
    pub source: u16,
    /// Where it reads, 0..1 of the source's length (a looped sound wraps).
    pub position: f32,
    /// Its envelope, 0..1.
    pub env: f32,
}

/// Counts onsets: a fast follower (instant rise, 20 ms fall) that rises 6 dB over a slow one
/// (30 ms rise, 300 ms fall) and above -50 dBFS. It counts again once the fast one is back
/// within 3 dB of the slow one, so a held sound counts once.
pub(crate) struct Onsets {
    fast: f32,
    slow: f32,
    fast_fall: f32,
    slow_rise: f32,
    slow_fall: f32,
    armed: bool,
    /// Whether the last block was watched: a watch starts from rest.
    live: bool,
}

/// -50 dBFS.
const ONSET_FLOOR: f32 = 0.003_162;

impl Onsets {
    pub fn new() -> Onsets {
        let mut o = Onsets {
            fast: 0.0,
            slow: 0.0,
            fast_fall: 0.0,
            slow_rise: 0.0,
            slow_fall: 0.0,
            armed: true,
            live: false,
        };
        o.prepare(48_000.0);
        o
    }

    pub fn prepare(&mut self, rate: f64) {
        let coef = |ms: f64| (-1.0 / (ms * 0.001 * rate)).exp() as f32;
        self.fast_fall = coef(20.0);
        self.slow_rise = coef(30.0);
        self.slow_fall = coef(300.0);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
        self.armed = true;
    }

    /// Adds the onsets of a block of the input to `probe` while it is watched.
    pub fn note(&mut self, probe: &Probe, left: &[f32], right: &[f32]) {
        if !probe.watching() {
            self.live = false;
            return;
        }
        if !self.live {
            self.reset();
            self.live = true;
        }
        probe.add_onsets(self.count(left, right));
    }

    /// Onsets in a block of the input.
    pub fn count(&mut self, left: &[f32], right: &[f32]) -> u32 {
        let mut n = 0;
        for (l, r) in left.iter().zip(right) {
            let x = l.abs().max(r.abs());
            self.fast = if x > self.fast {
                x
            } else {
                x + (self.fast - x) * self.fast_fall
            };
            let k = if self.fast > self.slow {
                self.slow_rise
            } else {
                self.slow_fall
            };
            self.slow = self.fast + (self.slow - self.fast) * k;
            if self.armed && self.fast > ONSET_FLOOR && self.fast > 2.0 * self.slow {
                n += 1;
                self.armed = false;
            } else if !self.armed && self.fast < 1.41 * self.slow {
                self.armed = true;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::super::compressor::reduction;
    use super::super::dsp::{db_to_gain, gain_to_db};
    use super::super::testing::*;
    use super::*;
    use crate::processor::NoteKind;

    #[test]
    fn peaks_keep_the_highest_until_taken() {
        let p = Probe::new();
        p.note_input(&[0.2, -0.5], &[0.1, 0.0]);
        p.note_input(&[0.3], &[-0.4]);
        p.note_output(&[0.9], &[0.0]);
        assert_eq!(p.take_peaks(), ([0.5, 0.4], [0.9, 0.0]));
        assert_eq!(p.take_peaks(), ([0.0, 0.0], [0.0, 0.0]));
    }

    #[test]
    fn a_voice_comes_back_as_it_was_packed() {
        let p = Probe::new();
        assert_eq!(p.voice(), None);
        p.set_voice(Some((64, VoiceStage::Release, true, 3, 0.25, 0.5)));
        let v = p.voice().unwrap();
        assert_eq!(
            (v.pitch, v.stage, v.file, v.source),
            (64, VoiceStage::Release, true, 3)
        );
        assert_eq!((v.position, v.env), (0.25, 0.5));
        p.set_voice(Some((127, VoiceStage::Attack, false, 7, 2.0, -1.0)));
        let v = p.voice().unwrap();
        assert_eq!(
            (v.pitch, v.stage, v.file, v.source),
            (127, VoiceStage::Attack, false, 7)
        );
        assert_eq!((v.position, v.env), (1.0, 0.0));
        p.set_voice(None);
        assert_eq!(p.voice(), None);
    }

    #[test]
    fn watching_again_clears_what_an_earlier_watch_left() {
        let p = Probe::new();
        p.watch(true);
        p.set_gr(6.0);
        p.note_output(&[0.5], &[0.5]);
        p.set_voice(Some((60, VoiceStage::Decay, false, 0, 0.1, 0.8)));
        p.watch(false);
        p.watch(true);
        assert_eq!(p.gr_db(), 0.0);
        assert_eq!(p.take_peaks(), ([0.0; 2], [0.0; 2]));
        assert_eq!(p.voice(), None);
    }

    /// `bursts` of noise 300 ms apart, each 50 ms, then a held tone of a second.
    fn bursts(bursts: usize, rate: f64) -> Vec<f32> {
        let gap = (0.3 * rate) as usize;
        let on = (0.05 * rate) as usize;
        let mut x = vec![0.0; gap * bursts];
        let mut seed = 1u32;
        for b in 0..bursts {
            for s in &mut x[b * gap..b * gap + on] {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *s = (seed >> 8) as f32 / (1 << 24) as f32 - 0.5;
            }
        }
        x.extend(
            (0..rate as usize)
                .map(|i| 0.5 * (std::f64::consts::TAU * 220.0 * i as f64 / rate).sin() as f32),
        );
        x
    }

    #[test]
    fn onsets_count_each_rise_and_a_held_tone_once() {
        let rate = 48_000.0;
        let x = bursts(5, rate);
        let mut o = Onsets::new();
        o.prepare(rate);
        let n: u32 = x.chunks(256).map(|c| o.count(c, c)).sum();
        assert_eq!(n, 6);
        let quiet: Vec<f32> = x.iter().map(|s| s * 1e-3).collect();
        o.reset();
        assert_eq!(o.count(&quiet, &quiet), 0, "below the floor");
    }

    #[test]
    fn the_reduction_shown_is_the_curves_on_a_steady_sine() {
        let (mut p, h) = make("compressor");
        for (id, v) in [
            ("threshold", -20.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("attack", 0.1),
            ("release", 2000.0),
        ] {
            set_plain(&h, id, v);
        }
        p.prepare(&setup());
        let probe = h.probe();
        probe.watch(true);
        let x = sine(1000.0, 0.5, RATE as usize);
        render(p.as_mut(), &x, &x, &[]);
        let want = reduction(gain_to_db(0.5), -20.0, 4.0, 0.0);
        let got = f64::from(probe.gr_db());
        assert!(want > 10.0);
        assert!((got - want).abs() < 0.1, "{got} against {want}");
    }

    #[test]
    fn the_limiter_never_shows_or_goes_past_its_ceiling() {
        let (mut p, h) = make("limiter");
        set_plain(&h, "input", 12.0);
        set_plain(&h, "ceiling", -3.0);
        p.prepare(&setup());
        let probe = h.probe();
        probe.watch(true);
        let ceiling = db_to_gain(-3.0);
        let x = bursts(4, RATE);
        let (mut most_in, mut most_out, mut most_gr) = (0f32, 0f32, 0f32);
        for chunk in x.chunks(4800) {
            let (l, r) = render(p.as_mut(), chunk, chunk, &[]);
            assert!(peak(&l).max(peak(&r)) <= ceiling + 1e-6);
            let (input, output) = probe.take_peaks();
            assert!(f64::from(output[0].max(output[1])) <= ceiling + 1e-6);
            most_in = most_in.max(input[0]);
            most_out = most_out.max(output[0]);
            most_gr = most_gr.max(probe.gr_db());
        }
        assert!(most_in > 0.45, "{most_in}");
        assert!(f64::from(most_out) > ceiling * 0.9, "{most_out}");
        // The loudest input, 0.5 with 12 dB on, is 9 dB over the ceiling.
        assert!((8.5..9.1).contains(&most_gr), "{most_gr}");
    }

    /// A built-in, what to play into it and the notes to play.
    type Play = (&'static str, Vec<f32>, Vec<(usize, NoteKind)>);

    fn every_built_in() -> Vec<Play> {
        let input = bursts(3, RATE);
        let silence = vec![0.0; RATE as usize];
        let hits = vec![
            (0, on(36, 1.0)),
            (9_600, on(38, 1.0)),
            (19_200, on(42, 1.0)),
        ];
        let notes = vec![(0, on(60, 1.0)), (24_000, off(60))];
        vec![
            ("eq", input.clone(), vec![]),
            ("compressor", input.clone(), vec![]),
            ("limiter", input.clone(), vec![]),
            ("delay", input.clone(), vec![]),
            ("reverb", input, vec![]),
            ("sampler", silence.clone(), notes),
            ("drum-kit", silence, hits),
        ]
    }

    #[test]
    fn nothing_is_written_while_not_watched() {
        for (key, x, notes) in every_built_in() {
            let (mut p, h) = make(key);
            if key == "limiter" {
                set_plain(&h, "input", 12.0);
                p.prepare(&setup());
            }
            let probe = h.probe();
            render(p.as_mut(), &x, &x, &notes);
            assert_eq!(probe.gr_db(), 0.0, "{key}");
            assert_eq!(probe.take_peaks(), ([0.0; 2], [0.0; 2]), "{key}");
            assert_eq!(probe.onsets(), 0, "{key}");
            assert_eq!(probe.hits(), [0; HIT_GROUPS], "{key}");
            assert_eq!(probe.voice(), None, "{key}");
            // Watched, the same play writes.
            probe.watch(true);
            render(p.as_mut(), &x, &x, &notes);
            let (_, output) = probe.take_peaks();
            assert!(output[0] > 0.0, "{key}");
            match key {
                "compressor" | "limiter" => assert!(probe.gr_db() > 0.0, "{key}"),
                "delay" | "reverb" => assert_eq!(probe.onsets(), 4, "{key}"),
                "drum-kit" => assert_eq!(probe.hits(), [1, 1, 1, 0, 0, 0]),
                _ => {}
            }
            probe.watch(false);
            let (before, voice) = (probe.onsets(), probe.voice());
            render(p.as_mut(), &x, &x, &notes);
            assert_eq!(probe.take_peaks(), ([0.0; 2], [0.0; 2]), "{key}");
            assert_eq!((probe.onsets(), probe.voice()), (before, voice), "{key}");
        }
    }

    #[test]
    fn the_sampler_shows_its_newest_voice() {
        let (mut p, h) = make("sampler");
        let probe = h.probe();
        probe.watch(true);
        let silence = vec![0.0; RATE as usize / 2];
        render(
            p.as_mut(),
            &silence,
            &silence,
            &[(0, on(60, 1.0)), (4_800, on(67, 1.0))],
        );
        let v = probe.voice().expect("a voice plays");
        assert_eq!((v.pitch, v.file), (67, false));
        assert!(v.position > 0.0 && v.position <= 1.0, "{}", v.position);
        assert!(v.env > 0.0, "{}", v.env);
    }

    #[test]
    fn watching_leaves_the_sound_alone() {
        for (key, x, notes) in every_built_in() {
            let (mut a, _) = make(key);
            let (mut b, hb) = make(key);
            hb.probe().watch(true);
            let quiet = render(a.as_mut(), &x, &x, &notes);
            let watched = render(b.as_mut(), &x, &x, &notes);
            assert!(quiet == watched, "{key}");
        }
    }
}
