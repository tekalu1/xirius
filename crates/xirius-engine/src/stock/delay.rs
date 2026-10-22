//! The Delay (PLG-011): a stereo echo whose time is a note length at the song's tempo or a
//! time in ms. What goes into the lines passes a low cut and a high cut, so every repeat gets
//! darker and thinner, as on a tape echo. With ping-pong the repeats go left, right, left.
//! A moved time slides (the pitch bends a little, as on tape) instead of clicking. No latency.

use super::bank::Params;
use super::dsp::{Biquad, Coefs, Glide, Line, Ramp, STEP, flush, smoothing};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::DELAY`.
const SYNC: usize = 0;
const TIME: usize = 1;
const DIVISION: usize = 2;
const FEEL: usize = 3;
const FEEDBACK: usize = 4;
const MIX: usize = 5;
const PING_PONG: usize = 6;
const LOW_CUT: usize = 7;
const HIGH_CUT: usize = 8;

/// The longest echo in seconds (a dotted whole note at 45 bpm); slower ones are held here.
const LONGEST: f64 = 8.0;

/// The note lengths of `xirius_params::DIVISIONS` in whole notes.
const DIVISION_WHOLES: [f64; 6] = [1.0 / 32.0, 1.0 / 16.0, 1.0 / 8.0, 0.25, 0.5, 1.0];
/// Straight, dotted, triplet.
const FEEL_FACTORS: [f64; 3] = [1.0, 1.5, 2.0 / 3.0];

pub struct Delay {
    params: Params,
    glide: Glide,
    rate: f64,
    lines: [Line; 2],
    /// The echo time in frames as it slides, and where it slides to.
    time: f64,
    target: f64,
    slide: f64,
    low_cut: Coefs,
    high_cut: Coefs,
    filters: [[Biquad; 2]; 2],
    dry: Ramp,
    wet: Ramp,
}

impl Delay {
    pub fn new(params: Params) -> Delay {
        Delay {
            glide: Glide::new(&xirius_params::DELAY),
            params,
            rate: 48_000.0,
            lines: [Line::new(0), Line::new(0)],
            time: 2.0,
            target: 2.0,
            slide: 0.0,
            low_cut: Coefs::THROUGH,
            high_cut: Coefs::THROUGH,
            filters: [[Biquad::default(); 2]; 2],
            dry: Ramp::new(1.0),
            wet: Ramp::new(0.0),
        }
    }

    /// The echo time in seconds the values and the tempo ask for.
    fn seconds(&self) -> f64 {
        let g = &self.glide;
        if g.on(SYNC) {
            let wholes = DIVISION_WHOLES[g.index(DIVISION).min(DIVISION_WHOLES.len() - 1)]
                * FEEL_FACTORS[g.index(FEEL).min(FEEL_FACTORS.len() - 1)];
            60.0 / self.params.tempo().max(1.0) * 4.0 * wholes
        } else {
            g.get(TIME) * 0.001
        }
    }

    fn update(&mut self) {
        let longest = (self.lines[0].len() - 4) as f64;
        self.target = (self.seconds() * self.rate).clamp(2.0, longest);
        let g = &self.glide;
        self.low_cut = Coefs::highpass(g.get(LOW_CUT), std::f64::consts::FRAC_1_SQRT_2, self.rate);
        self.high_cut = Coefs::lowpass(g.get(HIGH_CUT), std::f64::consts::FRAC_1_SQRT_2, self.rate);
    }

    /// The dry and wet gains for a mix of 0..1: both full in the middle.
    fn gains(&self) -> (f64, f64) {
        let mix = self.glide.get(MIX) * 0.01;
        ((2.0 * (1.0 - mix)).min(1.0), (2.0 * mix).min(1.0))
    }
}

impl Processor for Delay {
    fn name(&self) -> &str {
        "Delay"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        let len = (LONGEST * self.rate).ceil() as usize + 8;
        self.lines = [Line::new(len), Line::new(len)];
        self.filters = [[Biquad::default(); 2]; 2];
        // About 60 ms to slide to a new time.
        self.slide = smoothing(60.0, self.rate, 1);
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.update();
        self.time = self.target;
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
            let feedback = self.glide.get(FEEDBACK) * 0.01;
            let ping_pong = self.glide.on(PING_PONG);
            let (dry, wet) = self.gains();
            let (mut d, dd) = self.dry.toward(dry, len);
            let (mut w, dw) = self.wet.toward(wet, len);
            for i in at..at + len {
                self.time = self.target + (self.time - self.target) * self.slide;
                let l = f64::from(io.left[i]);
                let r = f64::from(io.right[i]);
                let el = self.lines[0].read(self.time);
                let er = self.lines[1].read(self.time);
                // Ping-pong: both channels go in on the left, and each line feeds the other.
                let (into_l, into_r) = if ping_pong {
                    (0.5 * (l + r) + er * feedback, el * feedback)
                } else {
                    (l + el * feedback, r + er * feedback)
                };
                for (ch, x) in [into_l, into_r].into_iter().enumerate() {
                    let [lo, hi] = &mut self.filters[ch];
                    let y = hi.tick(&self.high_cut, lo.tick(&self.low_cut, x));
                    self.lines[ch].push(flush(y));
                }
                d += dd;
                w += dw;
                io.left[i] = (l * d + el * w) as f32;
                io.right[i] = (r * d + er * w) as f32;
            }
            at += len;
        }
        for ch in &mut self.filters {
            for b in ch.iter_mut() {
                b.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use crate::processor::Processor;

    /// The echo alone, with filters that leave it as it is.
    fn wet_only(h: &super::super::StockHandle) {
        set_plain(h, "mix", 100.0);
        set_plain(h, "feedback", 0.0);
        set_plain(h, "low_cut", 20.0);
        set_plain(h, "high_cut", 20_000.0);
    }

    /// The frame of the loudest sample after `from`.
    fn loudest(x: &[f32], from: usize) -> usize {
        (from..x.len())
            .max_by(|a, b| x[*a].abs().total_cmp(&x[*b].abs()))
            .unwrap()
    }

    /// The echo lands at `frame` (the high cut at 20 kHz smears it by a frame or two).
    fn lands_at(x: &[f32], frame: usize) {
        let at = loudest(x, 1000);
        assert!((frame..frame + 3).contains(&at), "{at} for {frame}");
        // What is left above 20 kHz is a sixth of the band: sqrt(5/6) = 0.91 of the energy.
        assert!(energy(x, frame) > 0.88, "{}", energy(x, frame));
    }

    /// The energy around `at` (a repeat smeared a little by the filters).
    fn energy(x: &[f32], at: usize) -> f64 {
        x[at - 64..at + 64]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    fn impulse_response(p: &mut dyn Processor, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let x = impulse(frames);
        render(p, &x, &x, &[])
    }

    #[test]
    fn the_echo_follows_the_tempo() {
        // A dotted eighth at 120 bpm: 375 ms.
        let (mut p, h) = make("delay");
        wet_only(&h);
        p.prepare(&setup());
        let (l, r) = impulse_response(p.as_mut(), 30_000);
        lands_at(&l, 18_000);
        assert!(l[..17_990].iter().all(|v| v.abs() < 1e-3));
        assert_eq!(l, r);
        // At 100 bpm: 450 ms; a quarter triplet at 100 bpm: 400 ms.
        h.set_tempo(100.0);
        p.prepare(&setup());
        let (l, _) = impulse_response(p.as_mut(), 30_000);
        lands_at(&l, 21_600);
        set_plain(&h, "division", 3.0);
        set_plain(&h, "feel", 2.0);
        p.prepare(&setup());
        let (l, _) = impulse_response(p.as_mut(), 30_000);
        lands_at(&l, 19_200);
    }

    #[test]
    fn the_echo_takes_a_free_time_when_not_synced() {
        let (mut p, h) = make("delay");
        wet_only(&h);
        set_plain(&h, "sync", 0.0);
        set_plain(&h, "time", 250.0);
        p.prepare(&setup());
        let (l, _) = impulse_response(p.as_mut(), 20_000);
        lands_at(&l, 12_000);
    }

    #[test]
    fn repeats_fall_by_the_feedback() {
        let (mut p, h) = make("delay");
        wet_only(&h);
        set_plain(&h, "sync", 0.0);
        set_plain(&h, "time", 100.0);
        set_plain(&h, "feedback", 50.0);
        p.prepare(&setup());
        let (l, _) = impulse_response(p.as_mut(), 30_000);
        let e: Vec<f64> = (1..=4).map(|k| energy(&l, k * 4800)).collect();
        for k in 1..4 {
            let ratio = e[k] / e[k - 1];
            assert!((ratio - 0.5).abs() < 0.02, "repeat {k}: {ratio}");
        }
        // The filters take a little more off every repeat.
        set_plain(&h, "high_cut", 2000.0);
        p.prepare(&setup());
        let (dark, _) = impulse_response(p.as_mut(), 30_000);
        assert!(energy(&dark, 3 * 4800) / energy(&dark, 4800) < 0.2);
    }

    #[test]
    fn ping_pong_goes_left_then_right() {
        let (mut p, h) = make("delay");
        wet_only(&h);
        set_plain(&h, "sync", 0.0);
        set_plain(&h, "time", 100.0);
        set_plain(&h, "feedback", 80.0);
        set_plain(&h, "ping_pong", 1.0);
        p.prepare(&setup());
        let (l, r) = impulse_response(p.as_mut(), 30_000);
        for k in 1..=4 {
            let (on, off) = if k % 2 == 1 { (&l, &r) } else { (&r, &l) };
            assert!(energy(on, k * 4800) > 0.2, "repeat {k}");
            let leak = energy(off, k * 4800) / energy(on, k * 4800);
            assert!(leak < 1e-3, "repeat {k}: {leak}");
        }
    }

    #[test]
    fn the_dry_sound_passes_at_the_default_mix() {
        let (mut p, _) = make("delay");
        let x = sine(440.0, 0.5, 10_000);
        let (l, _) = render(p.as_mut(), &x, &x, &[]);
        assert_eq!(l, x, "nothing but the dry sound before the first echo");
    }

    #[test]
    fn a_new_tempo_moves_the_echo_while_it_plays() {
        let (mut p, h) = make("delay");
        wet_only(&h);
        p.prepare(&setup());
        h.set_tempo(60.0);
        let x = impulse(70_000);
        let (l, _) = render(p.as_mut(), &x, &x, &[]);
        lands_at(&l, 36_000);
    }

    #[test]
    fn silence_in_is_silence_out() {
        let (mut p, h) = make("delay");
        set_plain(&h, "feedback", 95.0);
        set_plain(&h, "mix", 100.0);
        set_plain(&h, "ping_pong", 1.0);
        assert_silent(p.as_mut());
    }
}
