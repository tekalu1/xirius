//! The Compressor (PLG-009): feed-forward, the two channels linked on the louder one's peak.
//!
//! The level in dB goes through a curve with a soft knee (Giannoulis, Massberg and Reiss,
//! "Digital Dynamic Range Compressor Design", 2012); the gain reduction it asks for is smoothed
//! with the attack while it grows and the release while it shrinks, so the release does not
//! wait for the attack to let go. Then the makeup gain, and a dry/wet mix for parallel
//! compression. No latency.

use super::bank::Params;
use super::dsp::{Glide, STEP, db_to_gain, gain_to_db};
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
    /// The smoothed gain reduction in dB (0 or more).
    reduction: f64,
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

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::reduction;

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
}
