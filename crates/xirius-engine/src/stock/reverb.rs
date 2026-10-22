//! The Reverb (PLG-010): an algorithmic room. The input passes a low cut and the predelay;
//! eight early reflections are tapped from there, and four allpasses smear it before it goes
//! into a feedback delay network of eight lines mixed by a Hadamard matrix. Each line loses
//! what makes the tail fall 60 dB in the decay time, and a one-pole lowpass in the loop takes
//! the highs away sooner (the damping). The lines' lengths move slowly a few frames apart so
//! the tail does not ring at fixed pitches. No latency.

use super::bank::Params;
use super::dsp::{Allpass, Biquad, Coefs, Glide, Line, OnePole, Ramp, STEP, flush, lfo};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::REVERB`.
const SIZE: usize = 0;
const DECAY: usize = 1;
const PREDELAY: usize = 2;
const DAMPING: usize = 3;
const LOW_CUT: usize = 4;
const EARLY: usize = 5;
const WIDTH: usize = 6;
const MIX: usize = 7;

const LINES: usize = 8;
/// The lines' lengths in ms at the middle size (mutually far from simple ratios).
const LENGTHS_MS: [f64; LINES] = [29.7, 37.1, 41.1, 43.7, 53.3, 59.9, 67.7, 73.3];
/// How fast each line's length wanders, in Hz.
const WANDER_HZ: [f64; LINES] = [0.13, 0.21, 0.29, 0.37, 0.47, 0.59, 0.71, 0.83];
/// How far it wanders, in ms.
const WANDER_MS: f64 = 0.2;
/// The early reflections at the middle size: (ms, gain) for the left and the right.
const TAPS: [[(f64, f64); 8]; 2] = [
    [
        (7.1, 0.84),
        (11.3, -0.67),
        (17.9, 0.55),
        (23.5, -0.45),
        (31.1, 0.36),
        (37.7, -0.30),
        (47.3, 0.24),
        (59.9, -0.18),
    ],
    [
        (8.3, 0.81),
        (12.7, -0.69),
        (16.1, 0.57),
        (25.9, -0.43),
        (29.3, 0.38),
        (41.9, -0.28),
        (44.3, 0.25),
        (55.7, -0.19),
    ],
];
/// The diffusers before the network, in ms, for the left and the right.
const DIFFUSERS_MS: [[f64; 4]; 2] = [[4.77, 3.59, 12.73, 9.31], [4.97, 3.73, 13.11, 8.93]];
const DIFFUSION: f64 = 0.6;
/// What the network's sum is scaled by, so a long tail sits near the level of the dry sound.
const TAIL_GAIN: f64 = 0.3;
const LONGEST_PREDELAY_MS: f64 = 200.0;

/// The lengths at a size of 0..1 are the middle ones times this.
fn scale(size: f64) -> f64 {
    0.4 + 1.2 * size
}

pub struct Reverb {
    params: Params,
    glide: Glide,
    rate: f64,
    low_cut: Coefs,
    cuts: [Biquad; 2],
    pre: [Line; 2],
    predelay: f64,
    taps: [[(usize, f64); 8]; 2],
    early: f64,
    diffusers: [[Allpass; 4]; 2],
    lines: [Line; LINES],
    /// Each line's length in frames, its gain per pass and its wander's phase.
    lengths: [f64; LINES],
    gains: [f64; LINES],
    phases: [f64; LINES],
    steps: [f64; LINES],
    wander: f64,
    damping: f64,
    lows: [OnePole; LINES],
    width: f64,
    dry: Ramp,
    wet: Ramp,
}

impl Reverb {
    pub fn new(params: Params) -> Reverb {
        Reverb {
            glide: Glide::new(&xirius_params::REVERB),
            params,
            rate: 48_000.0,
            low_cut: Coefs::THROUGH,
            cuts: [Biquad::default(); 2],
            pre: [Line::new(0), Line::new(0)],
            predelay: 2.0,
            taps: [[(1, 0.0); 8]; 2],
            early: 0.0,
            diffusers: std::array::from_fn(|_| std::array::from_fn(|_| Allpass::new(1))),
            lines: std::array::from_fn(|_| Line::new(0)),
            lengths: [0.0; LINES],
            gains: [0.0; LINES],
            phases: [0.0; LINES],
            steps: [0.0; LINES],
            wander: 0.0,
            damping: 0.0,
            lows: [OnePole::default(); LINES],
            width: 1.0,
            dry: Ramp::new(1.0),
            wet: Ramp::new(0.0),
        }
    }

    fn frames(&self, ms: f64) -> f64 {
        ms * 0.001 * self.rate
    }

    fn update(&mut self) {
        let g = &self.glide;
        let s = scale(g.get(SIZE) * 0.01);
        let decay = g.get(DECAY).max(0.05);
        let pre_room = (self.pre[0].len() - 4) as f64;
        let predelay = self.frames(g.get(PREDELAY)).clamp(2.0, pre_room);
        let damping = OnePole::coef(g.get(DAMPING), self.rate);
        let low_cut = Coefs::highpass(g.get(LOW_CUT), std::f64::consts::FRAC_1_SQRT_2, self.rate);
        let early = g.get(EARLY) * 0.01;
        let width = g.get(WIDTH) * 0.01;
        for (i, ms) in LENGTHS_MS.iter().enumerate() {
            let len = self.frames(ms * s);
            self.lengths[i] = len;
            self.gains[i] = 10f64.powf(-3.0 * len / (decay * self.rate));
        }
        for (ch, taps) in TAPS.iter().enumerate() {
            for (k, (ms, gain)) in taps.iter().enumerate() {
                let at = (predelay + self.frames(ms * s)).round() as usize;
                self.taps[ch][k] = (at.min(self.pre[ch].len() - 1), *gain);
            }
        }
        self.predelay = predelay;
        self.damping = damping;
        self.low_cut = low_cut;
        self.early = early;
        self.width = width;
    }

    fn gains(&self) -> (f64, f64) {
        let mix = self.glide.get(MIX) * 0.01;
        ((2.0 * (1.0 - mix)).min(1.0), (2.0 * mix).min(1.0))
    }

    fn reset(&mut self) {
        self.cuts = [Biquad::default(); 2];
        for l in &mut self.pre {
            l.clear();
        }
        for ch in &mut self.diffusers {
            for a in ch.iter_mut() {
                a.clear();
            }
        }
        for l in &mut self.lines {
            l.clear();
        }
        self.lows = [OnePole::default(); LINES];
        for (i, p) in self.phases.iter_mut().enumerate() {
            *p = i as f64 / LINES as f64;
        }
    }
}

/// The 8-point Hadamard transform, scaled to keep the energy.
#[inline]
fn hadamard(x: &mut [f64; LINES]) {
    let mut h = 1;
    while h < LINES {
        for i in (0..LINES).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
    let k = (LINES as f64).sqrt().recip();
    for v in x.iter_mut() {
        *v *= k;
    }
}

impl Processor for Reverb {
    fn name(&self) -> &str {
        "Reverb"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        let largest = scale(1.0);
        let pre = self.frames(LONGEST_PREDELAY_MS + 60.0 * largest).ceil() as usize + 8;
        self.pre = [Line::new(pre), Line::new(pre)];
        self.wander = self.frames(WANDER_MS);
        for i in 0..LINES {
            let len = self.frames(LENGTHS_MS[i] * largest) + self.wander;
            self.lines[i] = Line::new(len.ceil() as usize + 8);
            self.steps[i] = WANDER_HZ[i] / self.rate;
        }
        for (ch, ms) in DIFFUSERS_MS.iter().enumerate() {
            for (k, ms) in ms.iter().enumerate() {
                self.diffusers[ch][k] = Allpass::new(self.frames(*ms).round() as usize);
            }
        }
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.update();
        self.reset();
        let (dry, wet) = self.gains();
        self.dry.set(dry);
        self.wet.set(wet);
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        let mut at = 0;
        while at < n {
            let len = STEP.min(n - at);
            if self.glide.step(&mut self.params) {
                self.update();
            }
            let (dry, wet) = self.gains();
            let (mut d, dd) = self.dry.toward(dry, len);
            let (mut w, dw) = self.wet.toward(wet, len);
            for i in at..at + len {
                let x = [f64::from(io.left[i]), f64::from(io.right[i])];
                let mut early = [0.0; 2];
                let mut into = [0.0; 2];
                for ch in 0..2 {
                    let cut = self.cuts[ch].tick(&self.low_cut, x[ch]);
                    let pre = &mut self.pre[ch];
                    pre.push(cut);
                    for (at, gain) in &self.taps[ch] {
                        early[ch] += pre.at(*at) * gain;
                    }
                    let mut v = pre.read(self.predelay);
                    for a in &mut self.diffusers[ch] {
                        v = a.tick(v, DIFFUSION);
                    }
                    into[ch] = v;
                }
                let mut y = [0.0; LINES];
                for (k, y) in y.iter_mut().enumerate() {
                    self.phases[k] += self.steps[k];
                    if self.phases[k] >= 1.0 {
                        self.phases[k] -= 1.0;
                    }
                    let d = self.lengths[k] + self.wander * lfo(self.phases[k]);
                    *y = self.lines[k].read(d);
                }
                let mut fb = [0.0; LINES];
                for (k, fb) in fb.iter_mut().enumerate() {
                    *fb = self.lows[k].lowpass(y[k], self.damping) * self.gains[k];
                }
                hadamard(&mut fb);
                for k in 0..LINES {
                    // The left goes into the even lines, the right into the odd, signs mixed.
                    let sign = if k & 2 == 0 { 1.0 } else { -1.0 };
                    self.lines[k].push(flush(fb[k] + into[k & 1] * sign));
                }
                let tail_l = (y[0] - y[2] + y[4] - y[6]) * TAIL_GAIN;
                let tail_r = (y[1] - y[3] + y[5] - y[7]) * TAIL_GAIN;
                let wl = tail_l + early[0] * self.early;
                let wr = tail_r + early[1] * self.early;
                let mid = 0.5 * (wl + wr);
                let side = 0.5 * (wl - wr) * self.width;
                d += dd;
                w += dw;
                io.left[i] = (x[0] * d + (mid + side) * w) as f32;
                io.right[i] = (x[1] * d + (mid - side) * w) as f32;
            }
            at += len;
        }
        for b in &mut self.cuts {
            b.flush();
        }
        for p in &mut self.lows {
            p.y = flush(p.y);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::processor::Processor;

    /// The tail alone, as plain as it gets.
    fn tail_only(h: &super::super::StockHandle) {
        set_plain(h, "mix", 100.0);
        set_plain(h, "early", 0.0);
        set_plain(h, "predelay", 0.0);
        set_plain(h, "low_cut", 20.0);
        set_plain(h, "damping", 20_000.0);
    }

    /// The time the impulse response takes to fall 60 dB in the middle of the band (250 Hz to
    /// 2 kHz, as rooms are measured), from its fall from -5 to -35 dB (Schroeder's backward
    /// integration). The highs fall sooner, as in a room.
    fn rt60(l: &[f32], r: &[f32]) -> f64 {
        use super::super::dsp::{Biquad, Coefs};
        let q = std::f64::consts::FRAC_1_SQRT_2;
        let (hp, lp) = (
            Coefs::highpass(250.0, q, RATE),
            Coefs::lowpass(2000.0, q, RATE),
        );
        let band = |x: &[f32]| {
            let (mut a, mut b) = (Biquad::default(), Biquad::default());
            x.iter()
                .map(|v| b.tick(&lp, a.tick(&hp, f64::from(*v))))
                .collect::<Vec<f64>>()
        };
        let (l, r) = (band(l), band(r));
        let e: Vec<f64> = l.iter().zip(&r).map(|(a, b)| a * a + b * b).collect();
        let mut edc = vec![0.0; e.len()];
        let mut sum = 0.0;
        for i in (0..e.len()).rev() {
            sum += e[i];
            edc[i] = sum;
        }
        let total = edc[0];
        let when = |db: f64| {
            let level = total * 10f64.powf(db / 10.0);
            edc.iter().position(|v| *v < level).unwrap() as f64 / RATE
        };
        (when(-35.0) - when(-5.0)) * 2.0
    }

    fn impulse_response(p: &mut dyn Processor, seconds: f64) -> (Vec<f32>, Vec<f32>) {
        let x = impulse((seconds * RATE) as usize);
        render(p, &x, &x, &[])
    }

    #[test]
    fn the_tail_falls_60_db_in_the_decay_time() {
        for (decay, size) in [(0.5, 50.0), (2.0, 50.0), (2.0, 100.0), (6.0, 20.0)] {
            let (mut p, h) = make("reverb");
            tail_only(&h);
            set_plain(&h, "decay", decay);
            set_plain(&h, "size", size);
            p.prepare(&setup());
            let (l, r) = impulse_response(p.as_mut(), decay * 2.0 + 0.5);
            let t = rt60(&l, &r);
            assert!((t / decay - 1.0).abs() < 0.05, "{decay} s at {size}%: {t}");
        }
    }

    #[test]
    fn damping_takes_the_highs_away_sooner() {
        let hf = |damping: f64| {
            let (mut p, h) = make("reverb");
            tail_only(&h);
            set_plain(&h, "damping", damping);
            p.prepare(&setup());
            let (l, _) = impulse_response(p.as_mut(), 1.5);
            // The sample-to-sample difference stands for the highs, late in the tail.
            let late = &l[RATE as usize..];
            let diff: Vec<f32> = late.windows(2).map(|w| w[1] - w[0]).collect();
            rms(&diff) / rms(late)
        };
        let bright = hf(20_000.0);
        let dark = hf(2000.0);
        assert!(dark < bright * 0.5, "{dark} vs {bright}");
    }

    #[test]
    fn nothing_comes_before_the_predelay() {
        let (mut p, h) = make("reverb");
        set_plain(&h, "mix", 100.0);
        set_plain(&h, "predelay", 50.0);
        p.prepare(&setup());
        let (l, r) = impulse_response(p.as_mut(), 0.5);
        assert!(l[..2400].iter().chain(&r[..2400]).all(|v| *v == 0.0));
        assert!(peak(&l[2400..2400 + 960]) > 0.05, "the first reflection");
    }

    #[test]
    fn the_tail_spreads_unless_the_width_is_none() {
        let (mut p, h) = make("reverb");
        set_plain(&h, "mix", 100.0);
        p.prepare(&setup());
        let (l, r) = impulse_response(p.as_mut(), 1.0);
        // Left and right are different sounds of a similar level.
        let corr = l.iter().zip(&r).map(|(a, b)| f64::from(a * b)).sum::<f64>()
            / (rms(&l) * rms(&r) * l.len() as f64);
        assert!(corr.abs() < 0.3, "{corr}");
        assert!((db(rms(&l) / rms(&r))).abs() < 1.5);
        set_plain(&h, "width", 0.0);
        p.prepare(&setup());
        let (l, r) = impulse_response(p.as_mut(), 1.0);
        assert_eq!(l, r);
    }

    #[test]
    fn a_dry_mix_changes_nothing_and_the_level_stays_near() {
        let (mut p, h) = make("reverb");
        set_plain(&h, "mix", 0.0);
        p.prepare(&setup());
        let x = sine(440.0, 0.5, 20_000);
        let (l, _) = render(p.as_mut(), &x, &x, &[]);
        assert_eq!(l, x);
        // All wet, noise in: the tail comes out within a few dB of what went in.
        set_plain(&h, "mix", 100.0);
        p.prepare(&setup());
        let mut noise = super::super::dsp::Noise::new(7);
        let x: Vec<f32> = (0..RATE as usize * 2)
            .map(|_| (noise.sample() * 0.3) as f32)
            .collect();
        let (l, r) = render(p.as_mut(), &x, &x, &[]);
        let half = RATE as usize;
        let ratio = db((rms(&l[half..]) + rms(&r[half..])) / (2.0 * rms(&x[half..])));
        assert!((-6.0..3.0).contains(&ratio), "{ratio}");
    }

    #[test]
    fn silence_in_is_silence_out() {
        let (mut p, h) = make("reverb");
        set_plain(&h, "decay", 20.0);
        set_plain(&h, "mix", 100.0);
        set_plain(&h, "size", 100.0);
        assert_silent(p.as_mut());
    }
}
