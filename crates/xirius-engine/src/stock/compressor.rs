//! The Compressor (PLG-009): feed-forward, the two channels linked on the louder one's peak.
//!
//! The level in dB goes through a curve with a soft knee (Giannoulis, Massberg and Reiss,
//! "Digital Dynamic Range Compressor Design", 2012); the gain reduction it asks for is smoothed
//! with the attack while it grows and the release while it shrinks, so the release does not
//! wait for the attack to let go. Then the makeup gain, and a dry/wet mix for parallel
//! compression. No latency.
//!
//! The level is read from the key (PLG-071): the sidechain's sources when the key reaches
//! the insert (the plug-in's sidechain is on, Ext); the compressor's own signal otherwise
//! (Int), exactly as before the sidechain. The one Gain knob is on the key at Ext (the Side
//! Gain) and on the input at Int (the Input Gain); at 0 dB it is not there to the bit. The Filter cuts the key (its own signal too)
//! at 48 dB/oct below the Low Cut and above the High Cut; Swap takes out the band between
//! them instead. Listen puts the key, filtered, out in place of the compressed signal.

use super::bank::Params;
use super::dsp::{Biquad, Coefs, Glide, STEP, butterworth_qs, db_to_gain, gain_to_db};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

const THRESHOLD: usize = 0;
const RATIO: usize = 1;
const ATTACK: usize = 2;
const RELEASE: usize = 3;
const KNEE: usize = 4;
const MAKEUP: usize = 5;
const MIX: usize = 6;
const GAIN: usize = 7;
const FILTER: usize = 8;
const LOW_CUT: usize = 9;
const HIGH_CUT: usize = 10;
const LISTEN: usize = 11;

pub struct Compressor {
    params: Params,
    glide: Glide,
    rate: f64,
    attack: f64,
    release: f64,
    /// The smoothed gain reduction in dB (0 or more).
    reduction: f64,
    filter: KeyFilter,
    /// Whether the filter ran in the last step, so it starts clean when it is turned on.
    filtering: bool,
}

/// The Gain as a factor, 1 exactly at 0 dB: the bank keeps a value in an f32, so 0 dB (a
/// third of the way up its range) comes back a hair off it.
fn gain_of(db: f64) -> f64 {
    if db.abs() < 1e-3 { 1.0 } else { db_to_gain(db) }
}

/// The gain reduction (dB, 0 or more) the curve asks for at `level` dB.
pub fn reduction(level: f64, threshold: f64, ratio: f64, knee: f64) -> f64 {
    let over = level - threshold;
    let slope = 1.0 / ratio - 1.0;
    let out = if 2.0 * over < -knee {
        level
    } else if knee > 0.0 && 2.0 * over.abs() <= knee {
        level + slope * (over + knee / 2.0).powi(2) / (2.0 * knee)
    } else {
        threshold + over / ratio
    };
    (level - out).max(0.0)
}

/// The sidechain filter on both channels of the key: four sections of a Butterworth for each
/// cut, 48 dB/oct. In turn, a highpass at the Low Cut and a lowpass at the High Cut keep the
/// band between them (Swap trades the two frequencies in the song; the filter has no swap).
struct KeyFilter {
    /// The Low Cut's sections (a highpass).
    low: [Coefs; 4],
    /// The High Cut's sections (a lowpass).
    high: [Coefs; 4],
    low_on: bool,
    high_on: bool,
    /// Per channel, the Low Cut's sections then the High Cut's.
    state: [[Biquad; 8]; 2],
}

impl KeyFilter {
    fn new() -> KeyFilter {
        KeyFilter {
            low: [Coefs::THROUGH; 4],
            high: [Coefs::THROUGH; 4],
            low_on: false,
            high_on: false,
            state: [[Biquad::default(); 8]; 2],
        }
    }

    fn set(&mut self, low: f64, high: f64, rate: f64) {
        let def = &xirius_params::COMPRESSOR.params;
        let (low_on, high_on) = (!def[LOW_CUT].is_off(low), !def[HIGH_CUT].is_off(high));
        // A half that was off held what it had when it went off; it starts again from nothing.
        for s in &mut self.state {
            let (lows, highs) = s.split_at_mut(4);
            if low_on && !self.low_on {
                lows.iter_mut().for_each(Biquad::reset);
            }
            if high_on && !self.high_on {
                highs.iter_mut().for_each(Biquad::reset);
            }
        }
        (self.low_on, self.high_on) = (low_on, high_on);
        for (k, q) in butterworth_qs(8).iter().enumerate() {
            self.low[k] = Coefs::highpass(low, *q, rate);
            self.high[k] = Coefs::lowpass(high, *q, rate);
        }
    }

    #[inline]
    fn tick(&mut self, ch: usize, x: f64) -> f64 {
        let s = &mut self.state[ch];
        let (lows, highs) = s.split_at_mut(4);
        let run = |cs: &[Coefs; 4], bq: &mut [Biquad], x: f64| {
            bq.iter_mut().zip(cs).fold(x, |y, (b, c)| b.tick(c, y))
        };
        let mut y = x;
        if self.low_on {
            y = run(&self.low, lows, y);
        }
        if self.high_on {
            y = run(&self.high, highs, y);
        }
        y
    }

    fn flush(&mut self) {
        for b in self.state.iter_mut().flatten() {
            b.flush();
        }
    }

    fn reset(&mut self) {
        for b in self.state.iter_mut().flatten() {
            b.reset();
        }
    }
}

impl Compressor {
    pub fn new(params: Params) -> Compressor {
        Compressor {
            glide: Glide::new(&xirius_params::COMPRESSOR),
            params,
            rate: 48_000.0,
            attack: 0.0,
            release: 0.0,
            reduction: 0.0,
            filter: KeyFilter::new(),
            filtering: false,
        }
    }

    fn update(&mut self) {
        let coef = |ms: f64| (-1.0 / (ms * 0.001 * self.rate)).exp();
        self.attack = coef(self.glide.get(ATTACK));
        self.release = coef(self.glide.get(RELEASE));
        let g = &self.glide;
        self.filter.set(g.get(LOW_CUT), g.get(HIGH_CUT), self.rate);
    }
}

impl Processor for Compressor {
    fn name(&self) -> &str {
        "Compressor"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.reduction = 0.0;
        self.filter.reset();
        self.filtering = false;
        self.update();
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        // A key shorter than the block is not one the engine hands over; read none of it.
        let key = io.key.filter(|(l, r)| l.len() >= n && r.len() >= n);
        let mut at = 0;
        while at < n {
            let len = STEP.min(n - at);
            if self.glide.step(&mut self.params) {
                self.update();
            }
            let g = &self.glide;
            let (threshold, ratio, knee) = (g.get(THRESHOLD), g.get(RATIO), g.get(KNEE));
            let makeup = g.get(MAKEUP);
            let wet = g.get(MIX) / 100.0;
            let dry = 1.0 - wet;
            let gain = gain_of(g.get(GAIN));
            let (input, side) = match key {
                Some(_) => (1.0, gain),
                None => (gain, 1.0),
            };
            let (filter, listen) = (g.on(FILTER), g.on(LISTEN));
            if filter && !self.filtering {
                self.filter.reset();
            }
            self.filtering = filter;
            for i in at..at + len {
                let (l, r) = (
                    f64::from(io.left[i]) * input,
                    f64::from(io.right[i]) * input,
                );
                let (mut kl, mut kr) = match key {
                    Some((kl, kr)) => (f64::from(kl[i]) * side, f64::from(kr[i]) * side),
                    None => (l, r),
                };
                if filter {
                    kl = self.filter.tick(0, kl);
                    kr = self.filter.tick(1, kr);
                }
                let level = gain_to_db(kl.abs().max(kr.abs()));
                let want = reduction(level, threshold, ratio, knee);
                let a = if want > self.reduction {
                    self.attack
                } else {
                    self.release
                };
                self.reduction = want + (self.reduction - want) * a;
                if listen {
                    io.left[i] = kl as f32;
                    io.right[i] = kr as f32;
                } else {
                    let gain = dry + wet * db_to_gain(makeup - self.reduction);
                    io.left[i] = (l * gain) as f32;
                    io.right[i] = (r * gain) as f32;
                }
            }
            at += len;
        }
        if self.filtering {
            self.filter.flush();
        }
        if self.reduction < 1e-9 {
            self.reduction = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;
    use std::sync::Arc;

    use super::super::bank::{ParamBank, Params};
    use super::super::dsp::gain_to_db;
    use super::super::testing::*;
    use super::{Compressor, FILTER, GAIN, LISTEN, reduction};
    use crate::processor::Processor;

    /// A square wave: its level is the same at every frame, so the reduction settles flat.
    fn square(amp: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| if (i / 24) % 2 == 0 { amp } else { -amp })
            .collect()
    }

    fn settled_gain(set: &[(&str, f64)], amp: f32) -> f64 {
        let (mut p, h) = make("compressor");
        for (id, v) in set {
            set_plain(&h, id, *v);
        }
        // Prepared again, so it starts at the values instead of gliding to them.
        p.prepare(&setup());
        let x = square(amp, RATE as usize);
        let (l, r) = render(p.as_mut(), &x, &x, &[]);
        assert_eq!(l, r, "linked channels");
        let tail = x.len() / 2;
        db(rms(&l[tail..]) / rms(&x[tail..]))
    }

    #[test]
    fn the_curve_follows_threshold_ratio_and_knee() {
        assert_eq!(reduction(-30.0, -18.0, 4.0, 0.0), 0.0);
        assert!((reduction(-6.0, -18.0, 4.0, 0.0) - 9.0).abs() < 1e-9);
        assert!((reduction(-6.0, -18.0, 20.0, 0.0) - 11.4).abs() < 1e-9);
        // Inside the knee the reduction starts gently; at its edges it meets the hard curve.
        let at_threshold = reduction(-18.0, -18.0, 4.0, 6.0);
        assert!((at_threshold - 0.5625).abs() < 1e-9);
        assert_eq!(reduction(-21.0, -18.0, 4.0, 6.0), 0.0);
        assert!(
            (reduction(-15.0, -18.0, 4.0, 6.0) - reduction(-15.0, -18.0, 4.0, 0.0)).abs() < 1e-9
        );
    }

    #[test]
    fn a_loud_signal_is_reduced_by_the_ratio() {
        // -6 dB over a -18 dB threshold at 4:1 comes out 9 dB down.
        let set = [("threshold", -18.0), ("ratio", 4.0), ("knee", 0.0)];
        let g = settled_gain(&set, 0.5);
        assert!((g + 9.0).abs() < 0.05, "{g}");
        // Makeup gives it back, a quiet signal is left alone, 1:1 does nothing.
        let up = [
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("makeup", 9.0),
        ];
        assert!(settled_gain(&up, 0.5).abs() < 0.05);
        assert!(settled_gain(&set, 0.05).abs() < 0.01);
        let one = settled_gain(&[("ratio", 1.0)], 0.5);
        assert!(one.abs() < 0.01, "{one}");
        // Half wet: half the dry signal and half the reduced one.
        let half = [
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("mix", 50.0),
        ];
        let want = db(0.5 + 0.5 * 10f64.powf(-9.0 / 20.0));
        assert!((settled_gain(&half, 0.5) - want).abs() < 0.05);
    }

    #[test]
    fn attack_and_release_take_their_time() {
        let (mut p, h) = make("compressor");
        for (id, v) in [
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("attack", 10.0),
            ("release", 100.0),
        ] {
            set_plain(&h, id, v);
        }
        p.prepare(&setup());
        // Quiet, loud for half a second, quiet again (a steady level, not a wave).
        let quiet = 0.01f32;
        let loud = 0.5f32;
        let half = RATE as usize / 2;
        let mut x = vec![quiet; half];
        x.extend(vec![loud; half]);
        x.extend(vec![quiet; half]);
        let (l, _) = render(p.as_mut(), &x, &x, &[]);
        let reduction_at = |i: usize| -db(f64::from(l[i]) / f64::from(x[i]));
        let ms = |t: f64| (t * RATE / 1000.0) as usize;
        // One attack time in, about 63 % of the 9 dB; after five, all of it.
        let a = reduction_at(half + ms(10.0));
        assert!((5.0..6.5).contains(&a), "{a}");
        assert!((reduction_at(half + ms(50.0)) - 9.0).abs() < 0.1);
        // One release time after it goes quiet, about 37 % is left.
        let r = reduction_at(2 * half + ms(100.0));
        assert!((2.8..3.9).contains(&r), "{r}");
    }

    #[test]
    fn silence_in_is_silence_out() {
        let (mut p, h) = make("compressor");
        set_plain(&h, "makeup", 24.0);
        set_plain(&h, "mix", 50.0);
        assert_silent(p.as_mut());
    }

    /// The Compressor as it was before it took a key (PLG-009), word for word: what a song
    /// without one must still sound like, to the bit.
    mod before {
        use super::super::super::bank::Params;
        use super::super::super::dsp::{Glide, STEP, db_to_gain, gain_to_db};
        use super::super::reduction;
        use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

        const THRESHOLD: usize = 0;
        const RATIO: usize = 1;
        const ATTACK: usize = 2;
        const RELEASE: usize = 3;
        const KNEE: usize = 4;
        const MAKEUP: usize = 5;
        const MIX: usize = 6;

        pub struct Compressor {
            params: Params,
            glide: Glide,
            rate: f64,
            attack: f64,
            release: f64,
            pub reduction: f64,
        }

        impl Compressor {
            pub fn new(params: Params) -> Compressor {
                Compressor {
                    glide: Glide::new(&xirius_params::COMPRESSOR),
                    params,
                    rate: 48_000.0,
                    attack: 0.0,
                    release: 0.0,
                    reduction: 0.0,
                }
            }

            fn update(&mut self) {
                let coef = |ms: f64| (-1.0 / (ms * 0.001 * self.rate)).exp();
                self.attack = coef(self.glide.get(ATTACK));
                self.release = coef(self.glide.get(RELEASE));
            }
        }

        impl Processor for Compressor {
            fn name(&self) -> &str {
                "Compressor"
            }

            fn prepare(&mut self, setup: &ProcessSetup) {
                self.rate = setup.sample_rate;
                self.params.refresh();
                self.glide.prepare(&self.params, self.rate);
                self.reduction = 0.0;
                self.update();
            }

            fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
                let n = io.frames();
                let mut at = 0;
                while at < n {
                    let len = STEP.min(n - at);
                    if self.glide.step(&mut self.params) {
                        self.update();
                    }
                    let g = &self.glide;
                    let (threshold, ratio, knee) = (g.get(THRESHOLD), g.get(RATIO), g.get(KNEE));
                    let makeup = g.get(MAKEUP);
                    let wet = g.get(MIX) / 100.0;
                    let dry = 1.0 - wet;
                    for i in at..at + len {
                        let (l, r) = (f64::from(io.left[i]), f64::from(io.right[i]));
                        let level = gain_to_db(l.abs().max(r.abs()));
                        let want = reduction(level, threshold, ratio, knee);
                        let a = if want > self.reduction {
                            self.attack
                        } else {
                            self.release
                        };
                        self.reduction = want + (self.reduction - want) * a;
                        let gain = dry + wet * db_to_gain(makeup - self.reduction);
                        io.left[i] = (l * gain) as f32;
                        io.right[i] = (r * gain) as f32;
                    }
                    at += len;
                }
                if self.reduction < 1e-9 {
                    self.reduction = 0.0;
                }
            }
        }
    }

    /// A number in 0..1 from a seed, the same on every run.
    fn noise(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Different on the two channels, swelling and dying away, now under the threshold, now
    /// far over it.
    fn program(frames: usize, seed: u64) -> (Vec<f32>, Vec<f32>) {
        let mut s = seed;
        let mut side = |ch: f64| -> Vec<f32> {
            (0..frames)
                .map(|i| {
                    let t = i as f64 / RATE;
                    let swell = 0.5 + 0.5 * (TAU * 0.7 * t + ch).sin();
                    let tone = (TAU * (110.0 + 55.0 * ch) * t).sin();
                    (swell * (0.6 * tone + 0.4 * (2.0 * noise(&mut s) - 1.0))) as f32
                })
                .collect()
        };
        (side(0.0), side(1.0))
    }

    /// The new and the old Compressor on one bank, run on the same signal (the key, if any,
    /// to the new one only) while every parameter but Filter and Listen keeps moving, the Gain
    /// held at its 0 dB; `hold` fixes some others.
    fn against_before(key: Option<(&[f32], &[f32])>, hold: &[(usize, f64)]) {
        let def = &xirius_params::COMPRESSOR;
        let bank = Arc::new(ParamBank::new(def));
        let mut now = Compressor::new(Params::new(bank.clone()));
        let mut was = before::Compressor::new(Params::new(bank.clone()));
        now.prepare(&setup());
        was.prepare(&setup());
        let (l, r) = program(RATE as usize * 3, 7);
        let chunk = 4_801;
        let mut seed = 11;
        let mut squeezed = false;
        let mut at = 0;
        while at < l.len() {
            for i in 0..def.params.len() {
                if i != FILTER && i != LISTEN {
                    bank.set_norm(i, noise(&mut seed));
                }
            }
            bank.set_norm(GAIN, def.params[GAIN].default_norm());
            for (i, v) in hold {
                bank.set_norm(*i, *v);
            }
            let end = (at + chunk).min(l.len());
            let k = key.map(|(kl, kr)| (&kl[at..end], &kr[at..end]));
            let (a, b) = render_keyed(&mut now, &l[at..end], &r[at..end], k, &[]);
            let (c, d) = render(&mut was, &l[at..end], &r[at..end], &[]);
            for (j, (x, y)) in a.iter().zip(&c).chain(b.iter().zip(&d)).enumerate() {
                assert_eq!(x.to_bits(), y.to_bits(), "frame {}", at + j % (end - at));
            }
            squeezed |= a.iter().zip(&l[at..end]).any(|(o, i)| o != i);
            at = end;
        }
        assert_eq!(now.reduction.to_bits(), was.reduction.to_bits());
        assert!(squeezed, "the signal was compressed somewhere");
    }

    #[test]
    fn without_a_key_it_sounds_as_it_did_to_the_bit() {
        against_before(None, &[]);
    }

    /// The title bar's switch (the engine handing the key over, or not) is the only one: no
    /// stepped knob, at any of its steps, keeps a key that reaches the insert from the
    /// detector.
    #[test]
    fn no_knob_keeps_a_key_handed_over_from_the_detector() {
        let def = &xirius_params::COMPRESSOR;
        let frames = RATE as usize / 5;
        let main = vec![0.05f32; frames];
        let key = square(0.9, frames);
        let full = (gain_to_db(0.9) + 18.0) * 0.75;
        for (i, spec) in def.params.iter().enumerate() {
            let Some(steps) = spec.steps() else {
                continue;
            };
            for k in 0..steps {
                let bank = Arc::new(ParamBank::new(def));
                for (id, v) in PUMP {
                    let (j, s) = def.param(id).unwrap();
                    bank.set_norm(j, s.to_norm(v));
                }
                bank.set_norm(i, f64::from(k) / f64::from(steps - 1));
                let mut p = Compressor::new(Params::new(bank));
                p.prepare(&setup());
                render_keyed(&mut p, &main, &main, Some((&key, &key)), &[]);
                let got = p.reduction;
                assert!((got - full).abs() < 0.05, "{} at {k}: {got}", spec.id);
            }
        }
    }
    /// A kick on the key: beats `every` frames apart from `first`, each loud for `long`
    /// frames and silent between.
    fn beats(frames: usize, first: usize, every: usize, long: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .map(|i| {
                if i < first || (i - first) % every >= long {
                    0.0
                } else if (i - first).is_multiple_of(2) {
                    amp
                } else {
                    -amp
                }
            })
            .collect()
    }

    /// The gain reduction in dB at every frame.
    fn reduction_of(out: &[f32], main: &[f32]) -> Vec<f64> {
        out.iter()
            .zip(main)
            .map(|(o, m)| -db(f64::from(*o) / f64::from(*m)))
            .collect()
    }

    fn keyed(set: &[(&str, f64)], main: &[f32], key: Option<&[f32]>) -> Vec<f32> {
        let (mut p, h) = make("compressor");
        for (id, v) in set {
            set_plain(&h, id, *v);
        }
        p.prepare(&setup());
        let (l, r) = render_keyed(p.as_mut(), main, main, key.map(|k| (k, k)), &[]);
        assert_eq!(l, r, "linked channels");
        l
    }

    const PUMP: [(&str, f64); 5] = [
        ("threshold", -18.0),
        ("ratio", 4.0),
        ("knee", 0.0),
        ("attack", 1.0),
        ("release", 50.0),
    ];

    #[test]
    fn the_gain_reduction_moves_on_the_beats_of_the_key() {
        // A pad under the threshold, so on its own it is never touched.
        let frames = RATE as usize * 2;
        let main = vec![0.05f32; frames];
        let (first, every, long) = (1_000, RATE as usize / 2, 2_400);
        let kick = beats(frames, first, every, long, 0.9);
        let out = keyed(&PUMP, &main, Some(&kick));
        let gr = reduction_of(&out, &main);
        // 17.1 dB over at 4:1: 12.8 dB down once the attack is through.
        let full = (gain_to_db(0.9) + 18.0) * 0.75;
        for b in (first..frames).step_by(every) {
            // Nothing the frame before the beat, something on the beat itself.
            assert!(gr[b - 1] < 0.01, "before beat {b}: {}", gr[b - 1]);
            assert!(gr[b] > 0.2, "on beat {b}: {}", gr[b]);
            // All of it by the end of the hit, and let go before the next.
            let held = gr[b + long - 1];
            assert!((held - full).abs() < 0.05, "{held}");
            assert!(gr[b + long + 2_400] < held / 2.0);
        }
        assert!(gr[..first].iter().all(|g| *g == 0.0));
        // With no key coming (Int), the pad is left alone.
        assert_eq!(keyed(&PUMP, &main, None), main);
    }

    #[test]
    fn with_no_key_the_gain_raises_the_input() {
        // Int: the one Gain knob is the Input Gain. 6 dB up, -6 dB in is 0 dB, 18 dB over
        // the threshold at 4:1, so 13.5 dB down: 7.5 dB under what went in.
        let set = [
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("gain", 6.0),
        ];
        let g = settled_gain(&set, 0.5);
        assert!((g + 7.5).abs() < 0.05, "{g}");
        // Under the threshold the signal comes out raised by it.
        let x = square(0.05, 4_000);
        let up = keyed(&[("threshold", 0.0), ("gain", 6.0)], &x, None);
        let k = 10f64.powf(6.0 / 20.0);
        assert!(
            up.iter()
                .zip(&x)
                .all(|(u, x)| (f64::from(*u) - f64::from(*x) * k).abs() < 1e-6)
        );
    }

    #[test]
    fn the_gain_raises_the_key_alone_before_it_is_measured() {
        let frames = RATE as usize;
        let main = vec![0.05f32; frames];
        // -24 dB: under the threshold until it is 12 dB up.
        let key = square(0.063, frames);
        let settled = |side: f64| {
            let mut set = PUMP.to_vec();
            set.push(("gain", side));
            let out = keyed(&set, &main, Some(&key));
            reduction_of(&out, &main)[frames - 1]
        };
        assert_eq!(settled(0.0), 0.0);
        let want = (gain_to_db(0.063) + 12.0 + 18.0) * 0.75;
        let got = settled(12.0);
        assert!((got - want).abs() < 0.05, "{got}");
        // The signal itself is not raised: with a silent key it comes out as it went in.
        let mut set = PUMP.to_vec();
        set.push(("gain", 24.0));
        let quiet = vec![0.0f32; frames];
        assert_eq!(keyed(&set, &main, Some(&quiet)), main);
    }

    /// The level (dB) of a sine at `freq` after the sidechain filter, heard with Listen.
    fn filtered(set: &[(&str, f64)], freq: f64) -> f64 {
        let x = sine(freq, 0.5, RATE as usize / 2);
        let mut all = vec![("filter", 1.0), ("listen", 1.0)];
        all.extend_from_slice(set);
        let out = keyed(&all, &x, None);
        let tail = x.len() / 2;
        db(rms(&out[tail..]) / rms(&x[tail..]))
    }

    #[test]
    fn the_filter_cuts_48_db_an_octave_past_its_frequencies() {
        let low = [("low_cut", 1000.0)];
        let at = filtered(&low, 1000.0);
        assert!((at + 3.01).abs() < 0.1, "{at}");
        let past = filtered(&low, 500.0);
        assert!((-49.5..-47.0).contains(&past), "{past}");
        assert!(filtered(&low, 8000.0).abs() < 0.05);
        let high = [("high_cut", 1000.0)];
        let at = filtered(&high, 1000.0);
        assert!((at + 3.01).abs() < 0.1, "{at}");
        let past = filtered(&high, 2000.0);
        assert!((-49.5..-47.0).contains(&past), "{past}");
        assert!(filtered(&high, 100.0).abs() < 0.05);
        // At the ends of their ranges both are off: the key goes through untouched.
        let x = sine(30.0, 0.5, 4_000);
        let out = keyed(&[("filter", 1.0), ("listen", 1.0)], &x, None);
        assert_eq!(out, x);
    }

    #[test]
    fn the_filter_decides_what_the_detector_hears() {
        // A loud low key pumps the pad; with the low cut above it, it does not.
        let frames = RATE as usize / 2;
        let main = vec![0.05f32; frames];
        let key = sine(60.0, 0.9, frames);
        let pumped = reduction_of(&keyed(&PUMP, &main, Some(&key)), &main);
        assert!(pumped[frames - 1] > 6.0);
        let mut cut = PUMP.to_vec();
        cut.extend([("filter", 1.0), ("low_cut", 1000.0)]);
        let left = reduction_of(&keyed(&cut, &main, Some(&key)), &main);
        assert!(left[frames / 2..].iter().all(|g| *g < 0.01));
    }

    /// A cut that comes back on starts from nothing: what it held when it went off is not
    /// heard again (the detector would hear it as a click).
    #[test]
    fn a_cut_turned_on_again_starts_empty() {
        let frames = RATE as usize / 5;
        let loud = sine(1414.0, 0.9, frames);
        let quiet = vec![0.0f32; frames];
        let (mut p, h) = make("compressor");
        for (id, v) in [("filter", 1.0), ("listen", 1.0), ("low_cut", 1000.0)] {
            set_plain(&h, id, v);
        }
        p.prepare(&setup());
        let mut run = |x: &[f32]| render_keyed(p.as_mut(), x, x, None, &[]).0;
        run(&loud);
        // Off while the signal still plays, then silence with it off.
        set_plain(&h, "low_cut", 20.0);
        run(&loud);
        assert!(run(&quiet).iter().all(|y| *y == 0.0));
        // On again over silence: nothing comes out.
        set_plain(&h, "low_cut", 1000.0);
        let out = run(&quiet);
        let most = out.iter().fold(0.0f32, |m, y| m.max(y.abs()));
        assert_eq!(most, 0.0);
        // The same for the High Cut, off at the top of its range.
        set_plain(&h, "low_cut", 20.0);
        set_plain(&h, "high_cut", 500.0);
        run(&loud);
        set_plain(&h, "high_cut", 20_000.0);
        run(&loud);
        run(&quiet);
        set_plain(&h, "high_cut", 500.0);
        let out = run(&quiet);
        let most = out.iter().fold(0.0f32, |m, y| m.max(y.abs()));
        assert_eq!(most, 0.0);
    }

    #[test]
    fn listen_plays_the_key_in_place_of_the_signal() {
        let frames = 10_000;
        let main = sine(440.0, 0.5, frames);
        let key = beats(frames, 100, 3_000, 500, 0.7);
        assert_eq!(keyed(&[("listen", 1.0)], &main, Some(&key)), key);
        // Raised by the Side Gain.
        let up = keyed(&[("listen", 1.0), ("gain", 6.0)], &main, Some(&key));
        let g = 10f32.powf(6.0 / 20.0);
        assert!(up.iter().zip(&key).all(|(u, k)| (u - k * g).abs() < 1e-6));
    }
}
