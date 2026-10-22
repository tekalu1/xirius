//! Small building blocks the built-ins share: filters, smoothing, delay lines.

use std::f64::consts::{PI, TAU};

use xirius_params::{BuiltinDef, Curve, MAX_PARAMS};

use super::bank::Params;

/// Frames between two updates of what a moving parameter sets (filter coefficients).
pub const STEP: usize = 32;

pub fn db_to_gain(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

pub fn gain_to_db(g: f64) -> f64 {
    20.0 * g.max(1e-30).log10()
}

/// Zero for values too small to hear, so feedback paths do not run on denormal numbers.
#[inline]
pub fn flush(x: f64) -> f64 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// The coefficients of a second-order filter (RBJ's Audio EQ Cookbook), normalised by a0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefs {
    pub const THROUGH: Coefs = Coefs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefs {
        Coefs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn w(freq: f64, rate: f64) -> (f64, f64) {
        let w0 = TAU * freq.clamp(1.0, rate * 0.49) / rate;
        (w0.cos(), w0.sin())
    }

    pub fn lowpass(freq: f64, q: f64, rate: f64) -> Coefs {
        let (c, s) = Self::w(freq, rate);
        let alpha = s / (2.0 * q);
        Self::norm(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn highpass(freq: f64, q: f64, rate: f64) -> Coefs {
        let (c, s) = Self::w(freq, rate);
        let alpha = s / (2.0 * q);
        Self::norm(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn peak(freq: f64, q: f64, gain_db: f64, rate: f64) -> Coefs {
        let (c, s) = Self::w(freq, rate);
        let a = 10f64.powf(gain_db / 40.0);
        let alpha = s / (2.0 * q);
        Self::norm(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    /// A shelf with a slope of 1 (the steepest without a bump).
    pub fn low_shelf(freq: f64, gain_db: f64, rate: f64) -> Coefs {
        let (c, s) = Self::w(freq, rate);
        let a = 10f64.powf(gain_db / 40.0);
        let alpha = s / 2.0 * 2f64.sqrt();
        let k = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - k),
            (a + 1.0) + (a - 1.0) * c + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - k,
        )
    }

    pub fn high_shelf(freq: f64, gain_db: f64, rate: f64) -> Coefs {
        let (c, s) = Self::w(freq, rate);
        let a = 10f64.powf(gain_db / 40.0);
        let alpha = s / 2.0 * 2f64.sqrt();
        let k = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - k),
            (a + 1.0) - (a - 1.0) * c + k,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - k,
        )
    }

    /// The gain in dB at `freq`, for tests and drawing a curve.
    pub fn response_db(&self, freq: f64, rate: f64) -> f64 {
        let w = TAU * freq / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        let num = num_re * num_re + num_im * num_im;
        let den = den_re * den_re + den_im * den_im;
        10.0 * (num / den).log10()
    }
}

/// One second-order section with its own state (transposed direct form II, in `f64`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    s1: f64,
    s2: f64,
}

impl Biquad {
    #[inline]
    pub fn tick(&mut self, c: &Coefs, x: f64) -> f64 {
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        y
    }

    pub fn flush(&mut self) {
        self.s1 = flush(self.s1);
        self.s2 = flush(self.s2);
    }

    pub fn reset(&mut self) {
        *self = Biquad::default();
    }
}

/// The Q of each section of a Butterworth filter of `order` (2, 4 or 8).
pub fn butterworth_qs(order: usize) -> &'static [f64] {
    match order {
        2 => &[std::f64::consts::FRAC_1_SQRT_2],
        4 => &[0.541_196_100_146_197, 1.306_562_964_876_376_7],
        _ => &[
            0.509_795_579_104_159_2,
            0.601_344_886_935_045_1,
            0.899_976_223_136_415_6,
            2.562_915_447_741_505_5,
        ],
    }
}

/// The coefficient of a one-pole smoother with time constant `ms`, run once every `every`
/// frames.
pub fn smoothing(ms: f64, rate: f64, every: usize) -> f64 {
    if ms <= 0.0 {
        return 0.0;
    }
    (-(every as f64) / (ms * 0.001 * rate)).exp()
}

/// A one-pole lowpass: `y += (x - y) * (1 - a)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    pub y: f64,
}

impl OnePole {
    /// The `a` for a cutoff at `freq`.
    pub fn coef(freq: f64, rate: f64) -> f64 {
        (-TAU * freq.clamp(1.0, rate * 0.49) / rate).exp()
    }

    #[inline]
    pub fn lowpass(&mut self, x: f64, a: f64) -> f64 {
        self.y = x + (self.y - x) * a;
        self.y
    }
}

/// The values of a built-in's parameters as they glide toward what was set, so a moved
/// frequency or gain does not jump (once every [`STEP`] frames, about 20 ms to settle).
pub struct Glide {
    def: &'static BuiltinDef,
    pub cur: [f64; MAX_PARAMS],
    moving: bool,
    a: f64,
}

impl Glide {
    pub fn new(def: &'static BuiltinDef) -> Glide {
        Glide {
            def,
            cur: [0.0; MAX_PARAMS],
            moving: true,
            a: 0.0,
        }
    }

    /// Starts at the values with nothing to glide.
    pub fn prepare(&mut self, params: &Params, rate: f64) {
        self.a = smoothing(20.0, rate, STEP);
        for i in 0..self.def.params.len() {
            self.cur[i] = params.get(i);
        }
        self.moving = true;
    }

    /// Moves one step toward the parameters. True when a value changed, so what depends on
    /// it is worked out again.
    pub fn step(&mut self, params: &mut Params) -> bool {
        if params.refresh() {
            self.moving = true;
        }
        if !self.moving {
            return false;
        }
        let mut still = false;
        for (i, p) in self.def.params.iter().enumerate() {
            let target = params.get(i);
            let cur = self.cur[i];
            if cur == target {
                continue;
            }
            let next = match p.curve {
                Curve::Log if cur > 0.0 && target > 0.0 => {
                    let r = (cur / target).ln() * self.a;
                    if r.abs() < 1e-4 {
                        target
                    } else {
                        target * r.exp()
                    }
                }
                Curve::Linear | Curve::Log => {
                    let d = (cur - target) * self.a;
                    if d.abs() < 1e-4 * (p.max - p.min).abs() {
                        target
                    } else {
                        target + d
                    }
                }
                Curve::Int | Curve::Choice(_) | Curve::Toggle => target,
            };
            self.cur[i] = next;
            still |= next != target;
        }
        self.moving = still;
        true
    }

    pub fn get(&self, i: usize) -> f64 {
        self.cur[i]
    }

    pub fn on(&self, i: usize) -> bool {
        self.cur[i] >= 0.5
    }

    pub fn index(&self, i: usize) -> usize {
        self.cur[i].max(0.0).round() as usize
    }
}

/// A gain that ramps to its target over one block, so a change does not click.
#[derive(Clone, Copy, Debug)]
pub struct Ramp {
    cur: f64,
}

impl Ramp {
    pub fn new(v: f64) -> Ramp {
        Ramp { cur: v }
    }

    pub fn set(&mut self, v: f64) {
        self.cur = v;
    }

    /// The per-frame step to reach `target` in `frames`, and the start.
    pub fn toward(&mut self, target: f64, frames: usize) -> (f64, f64) {
        let start = self.cur;
        let step = if frames == 0 {
            0.0
        } else {
            (target - start) / frames as f64
        };
        self.cur = target;
        (start, step)
    }
}

/// A delay line read at a fractional distance. The buffer is allocated in [`Line::new`].
pub struct Line {
    buf: Vec<f32>,
    pos: usize,
}

impl Line {
    pub fn new(len: usize) -> Line {
        Line {
            buf: vec![0.0; len.max(4)],
            pos: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }

    #[inline]
    pub fn push(&mut self, x: f64) {
        self.buf[self.pos] = x as f32;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
    }

    /// The value written `d` frames ago (1 is the last write), `1 <= d <= len - 1`.
    #[inline]
    pub fn at(&self, d: usize) -> f64 {
        let n = self.buf.len();
        let i = (self.pos + n - d.clamp(1, n - 1)) % n;
        f64::from(self.buf[i])
    }

    /// The value `d` frames ago, between frames by a cubic (Hermite) curve. `2 <= d <= len - 3`.
    #[inline]
    pub fn read(&self, d: f64) -> f64 {
        let n = self.buf.len();
        let d = d.clamp(2.0, (n - 3) as f64);
        let k = d.floor() as usize;
        let t = d - k as f64;
        // Going further back in time as t grows: x0 -> x1.
        let (xm1, x0, x1, x2) = (self.at(k - 1), self.at(k), self.at(k + 1), self.at(k + 2));
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    }
}

/// A Schroeder allpass for diffusion.
pub struct Allpass {
    line: Line,
    delay: usize,
}

impl Allpass {
    pub fn new(delay: usize) -> Allpass {
        Allpass {
            line: Line::new(delay + 2),
            delay: delay.max(1),
        }
    }

    #[inline]
    pub fn tick(&mut self, x: f64, g: f64) -> f64 {
        let d = self.line.at(self.delay);
        let w = x + g * d;
        self.line.push(flush(w));
        d - g * w
    }

    pub fn clear(&mut self) {
        self.line.clear();
    }
}

/// A sine at `phase` (0..1) for slow modulation.
pub fn lfo(phase: f64) -> f64 {
    (TAU * phase).sin()
}

/// A tiny deterministic noise source (xorshift), -1..1.
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Noise {
        Noise(seed.max(1))
    }

    #[inline]
    pub fn sample(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        f64::from(x) / f64::from(u32::MAX) * 2.0 - 1.0
    }
}

/// A topology-preserving state-variable lowpass (resonance 0..1).
#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f64,
    ic2: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SvfCoefs {
    a1: f64,
    a2: f64,
    a3: f64,
}

impl SvfCoefs {
    pub fn lowpass(freq: f64, resonance: f64, rate: f64) -> SvfCoefs {
        let g = (PI * freq.clamp(10.0, rate * 0.45) / rate).tan();
        let q = 0.5 + resonance.clamp(0.0, 1.0) * 19.5;
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        SvfCoefs { a1, a2, a3 }
    }
}

impl Svf {
    #[inline]
    pub fn lowpass(&mut self, c: &SvfCoefs, x: f64) -> f64 {
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        v2
    }

    pub fn reset(&mut self) {
        *self = Svf::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    #[test]
    fn peak_reaches_its_gain_at_its_frequency() {
        let c = Coefs::peak(1000.0, 1.0, 6.0, RATE);
        assert!((c.response_db(1000.0, RATE) - 6.0).abs() < 0.01);
        assert!(c.response_db(50.0, RATE).abs() < 0.2);
        assert!(c.response_db(15_000.0, RATE).abs() < 0.3);
    }

    #[test]
    fn shelves_reach_their_gain_far_from_the_corner() {
        let lo = Coefs::low_shelf(200.0, -9.0, RATE);
        assert!((lo.response_db(20.0, RATE) + 9.0).abs() < 0.2);
        assert!(lo.response_db(10_000.0, RATE).abs() < 0.1);
        let hi = Coefs::high_shelf(4000.0, 6.0, RATE);
        assert!((hi.response_db(20_000.0, RATE) - 6.0).abs() < 0.3);
        assert!(hi.response_db(100.0, RATE).abs() < 0.1);
    }

    #[test]
    fn butterworth_cascade_is_3_db_down_at_the_corner() {
        for order in [2, 4, 8] {
            let total: f64 = butterworth_qs(order)
                .iter()
                .map(|q| Coefs::lowpass(1000.0, *q, RATE).response_db(1000.0, RATE))
                .sum();
            assert!((total + 3.01).abs() < 0.05, "order {order}: {total}");
        }
    }

    #[test]
    fn line_reads_whole_and_fractional_distances() {
        let mut l = Line::new(16);
        for i in 0..10 {
            l.push(f64::from(i));
        }
        assert_eq!(l.at(1), 9.0);
        assert_eq!(l.at(3), 7.0);
        assert!((l.read(3.0) - 7.0).abs() < 1e-9);
        assert!(
            (l.read(3.5) - 6.5).abs() < 1e-9,
            "a straight ramp stays straight"
        );
    }

    #[test]
    fn noise_stays_in_range() {
        let mut n = Noise::new(7);
        for _ in 0..10_000 {
            let v = n.sample();
            assert!((-1.0..=1.0).contains(&v));
        }
    }
}
