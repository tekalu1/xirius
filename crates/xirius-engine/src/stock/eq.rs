//! The EQ (PLG-008): a low cut, a low shelf, two bells, a high shelf, a high cut and an output
//! gain, in that order. The cuts are Butterworth filters of 12, 24 or 48 dB per octave; the
//! bells and shelves are those of the Audio EQ Cookbook. Moved values glide, and the filters
//! are worked out again once every [`STEP`] frames while they do. No latency.

use super::bank::Params;
use super::dsp::{Biquad, Coefs, Glide, Ramp, STEP, butterworth_qs, db_to_gain};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

// The parameters, in the order of `xirius_params::EQ`.
const LOW_CUT_ON: usize = 0;
const LOW_CUT_FREQ: usize = 1;
const LOW_CUT_SLOPE: usize = 2;
const LOW_FREQ: usize = 3;
const LOW_GAIN: usize = 4;
const MID1_FREQ: usize = 5;
const MID1_GAIN: usize = 6;
const MID1_Q: usize = 7;
const MID2_FREQ: usize = 8;
const MID2_GAIN: usize = 9;
const MID2_Q: usize = 10;
const HIGH_FREQ: usize = 11;
const HIGH_GAIN: usize = 12;
const HIGH_CUT_ON: usize = 13;
const HIGH_CUT_FREQ: usize = 14;
const HIGH_CUT_SLOPE: usize = 15;
const OUTPUT: usize = 16;

/// Sections in a row: four for the low cut, the four bands, four for the high cut.
const SECTIONS: usize = 12;

pub struct Eq {
    params: Params,
    glide: Glide,
    rate: f64,
    coefs: [Coefs; SECTIONS],
    /// Sections that change the sound; the others are skipped and their state kept at rest.
    active: [bool; SECTIONS],
    state: [[Biquad; SECTIONS]; 2],
    gain: f64,
    out: Ramp,
}

impl Eq {
    pub fn new(params: Params) -> Eq {
        Eq {
            glide: Glide::new(&xirius_params::EQ),
            params,
            rate: 48_000.0,
            coefs: [Coefs::THROUGH; SECTIONS],
            active: [false; SECTIONS],
            state: [[Biquad::default(); SECTIONS]; 2],
            gain: 1.0,
            out: Ramp::new(1.0),
        }
    }

    /// Works out every section from the gliding values.
    fn update(&mut self) {
        let g = &self.glide;
        let rate = self.rate;
        let mut next = [None; SECTIONS];
        if g.on(LOW_CUT_ON) {
            let qs = butterworth_qs(order(g.index(LOW_CUT_SLOPE)));
            for (k, q) in qs.iter().enumerate() {
                next[k] = Some(Coefs::highpass(g.get(LOW_CUT_FREQ), *q, rate));
            }
        }
        let bands = [
            (
                LOW_GAIN,
                Coefs::low_shelf(g.get(LOW_FREQ), g.get(LOW_GAIN), rate),
            ),
            (
                MID1_GAIN,
                Coefs::peak(g.get(MID1_FREQ), g.get(MID1_Q), g.get(MID1_GAIN), rate),
            ),
            (
                MID2_GAIN,
                Coefs::peak(g.get(MID2_FREQ), g.get(MID2_Q), g.get(MID2_GAIN), rate),
            ),
            (
                HIGH_GAIN,
                Coefs::high_shelf(g.get(HIGH_FREQ), g.get(HIGH_GAIN), rate),
            ),
        ];
        for (k, (gain, c)) in bands.into_iter().enumerate() {
            if g.get(gain) != 0.0 {
                next[4 + k] = Some(c);
            }
        }
        if g.on(HIGH_CUT_ON) {
            let qs = butterworth_qs(order(g.index(HIGH_CUT_SLOPE)));
            for (k, q) in qs.iter().enumerate() {
                next[8 + k] = Some(Coefs::lowpass(g.get(HIGH_CUT_FREQ), *q, rate));
            }
        }
        for (k, c) in next.into_iter().enumerate() {
            let on = c.is_some();
            if on && !self.active[k] {
                // A section that comes back starts from rest, not from where it stopped.
                for ch in &mut self.state {
                    ch[k].reset();
                }
            }
            self.active[k] = on;
            self.coefs[k] = c.unwrap_or(Coefs::THROUGH);
        }
        self.gain = db_to_gain(g.get(OUTPUT));
    }
}

/// The order of a cut for a slope's index (12, 24, 48 dB per octave).
fn order(slope: usize) -> usize {
    match slope {
        0 => 2,
        1 => 4,
        _ => 8,
    }
}

impl Processor for Eq {
    fn name(&self) -> &str {
        "EQ"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.state = [[Biquad::default(); SECTIONS]; 2];
        self.active = [false; SECTIONS];
        self.update();
        self.out.set(self.gain);
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        let mut at = 0;
        while at < n {
            let len = STEP.min(n - at);
            if self.glide.step(&mut self.params) {
                self.update();
            }
            let (g0, dg) = self.out.toward(self.gain, len);
            for (ch, buf) in [&mut *io.left, &mut *io.right].into_iter().enumerate() {
                let state = &mut self.state[ch];
                let mut g = g0;
                for s in &mut buf[at..at + len] {
                    let mut x = f64::from(*s);
                    let sections = state.iter_mut().zip(&self.coefs).zip(&self.active);
                    for ((b, c), _) in sections.filter(|(_, on)| **on) {
                        x = b.tick(c, x);
                    }
                    g += dg;
                    *s = (x * g) as f32;
                }
            }
            at += len;
        }
        for ch in &mut self.state {
            for b in ch.iter_mut() {
                b.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;

    /// The gain in dB of a sine at `freq` through the EQ as `set` leaves it.
    fn gain_at(freq: f64, set: &[(&str, f64)]) -> f64 {
        let (mut p, h) = make("eq");
        for (id, v) in set {
            set_plain(&h, id, *v);
        }
        let frames = RATE as usize;
        let input = sine(freq, 0.5, frames);
        let (l, _) = render(p.as_mut(), &input, &input, &[]);
        let tail = frames / 2;
        db(rms(&l[tail..]) / rms(&input[tail..]))
    }

    #[test]
    fn flat_settings_change_nothing() {
        for f in [50.0, 1000.0, 12_000.0] {
            assert!(gain_at(f, &[]).abs() < 0.01, "{f} Hz");
        }
    }

    #[test]
    fn a_bell_lifts_its_frequency_and_leaves_the_rest() {
        let set = [("mid1_freq", 1000.0), ("mid1_gain", 9.0), ("mid1_q", 2.0)];
        assert!((gain_at(1000.0, &set) - 9.0).abs() < 0.1);
        assert!(gain_at(100.0, &set).abs() < 0.2);
        assert!(gain_at(10_000.0, &set).abs() < 0.2);
        let cut = [("mid2_freq", 3000.0), ("mid2_gain", -12.0)];
        assert!((gain_at(3000.0, &cut) + 12.0).abs() < 0.1);
    }

    #[test]
    fn shelves_reach_their_gain() {
        let set = [
            ("low_freq", 200.0),
            ("low_gain", -6.0),
            ("high_freq", 5000.0),
            ("high_gain", 4.0),
        ];
        assert!((gain_at(30.0, &set) + 6.0).abs() < 0.3);
        assert!((gain_at(18_000.0, &set) - 4.0).abs() < 0.3);
        assert!(gain_at(1200.0, &set).abs() < 0.6);
    }

    #[test]
    fn cuts_fall_by_their_slope() {
        // An octave and two below the corner: 24 dB/oct falls about 24 and 48 dB.
        let set = [("low_cut_on", 1.0), ("low_cut_freq", 400.0)];
        assert!((gain_at(400.0, &set) + 3.0).abs() < 0.2);
        let one = gain_at(200.0, &set);
        assert!((-26.0..-22.0).contains(&one), "{one}");
        let steep = [
            ("low_cut_on", 1.0),
            ("low_cut_freq", 400.0),
            ("low_cut_slope", 2.0),
        ];
        let one = gain_at(200.0, &steep);
        assert!((-50.0..-46.0).contains(&one), "{one}");
        let high = [
            ("high_cut_on", 1.0),
            ("high_cut_freq", 2000.0),
            ("high_cut_slope", 0.0),
        ];
        assert!((gain_at(2000.0, &high) + 3.0).abs() < 0.2);
        let one = gain_at(4000.0, &high);
        assert!((-14.0..-10.0).contains(&one), "{one}");
        assert!(gain_at(200.0, &high).abs() < 0.05);
    }

    #[test]
    fn output_gain_applies() {
        assert!((gain_at(1000.0, &[("output", -6.0)]) + 6.0).abs() < 0.01);
    }

    #[test]
    fn silence_in_is_silence_out() {
        let (mut p, h) = make("eq");
        set_plain(&h, "mid1_gain", 18.0);
        set_plain(&h, "low_cut_on", 1.0);
        set_plain(&h, "output", 24.0);
        assert_silent(p.as_mut());
    }
}
