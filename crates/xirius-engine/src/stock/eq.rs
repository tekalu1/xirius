//! The EQ (PLG-008): six bands and an output gain, in that order. Each band is on or off and
//! one of the types it takes (PLG-062, [`xirius_params::EQ_BANDS`]): a cut is a Butterworth
//! filter of 12, 24 or 48 dB per octave, its Q scaling every section's; a bell, a notch and
//! a shelf are those of the Audio EQ Cookbook. At their defaults the bands are a low cut, a
//! low shelf, two bells, a high shelf and a high cut, as they were before they had a type.
//! Moved values glide, and the filters are worked out again once every [`STEP`] frames while
//! they do. A band's switch, type and slope do not glide: the band crossfades from the old
//! filter to the new one over [`FADE_SECONDS`]. No latency.
//!
//! [`band_db`], [`response_db`] and [`Response`] give the curve a panel draws, from the same
//! coefficients the sound goes through.

use rtrb::{Producer, RingBuffer};
use xirius_params::{BandType, EQ, EQ_BANDS, MAX_PARAMS};

use super::bank::Params;
use super::dsp::{Biquad, Coefs, Glide, Ramp, STEP, butterworth_qs, db_to_gain, q_ratio};
use super::probe::{Probe, Tap};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

/// Where a band's parameters are in `xirius_params::EQ`.
#[derive(Clone, Copy)]
struct Band {
    on: usize,
    kind: usize,
    freq: usize,
    gain: usize,
    q: usize,
    slope: usize,
}

/// The six bands, in the order of [`EQ_BANDS`]; a test holds them to the ids there.
const BANDS: [Band; 6] = [
    Band {
        on: 0,
        kind: 17,
        freq: 1,
        gain: 18,
        q: 19,
        slope: 2,
    },
    Band {
        on: 20,
        kind: 21,
        freq: 3,
        gain: 4,
        q: 22,
        slope: 23,
    },
    Band {
        on: 24,
        kind: 25,
        freq: 5,
        gain: 6,
        q: 7,
        slope: 26,
    },
    Band {
        on: 27,
        kind: 28,
        freq: 8,
        gain: 9,
        q: 10,
        slope: 29,
    },
    Band {
        on: 30,
        kind: 31,
        freq: 11,
        gain: 12,
        q: 32,
        slope: 33,
    },
    Band {
        on: 13,
        kind: 34,
        freq: 14,
        gain: 35,
        q: 36,
        slope: 15,
    },
];
const OUTPUT: usize = 16;

/// Sections a band can take: four for a cut of 48 dB per octave, one for anything else.
const SLOTS: usize = 4;
/// Sections in a row, band by band.
const SECTIONS: usize = BANDS.len() * SLOTS;
/// How long a band takes to go from its old filter to the new one when its switch, type or
/// slope changes (M-dsp-1).
pub const FADE_SECONDS: f64 = 0.01;

/// What of a band does not glide: off, or its type and (a cut's) order.
type Shape = Option<(BandType, usize)>;

/// The shape of band `band` for the plain values `p`.
fn shape(p: &[f64], band: usize) -> Shape {
    let b = &BANDS[band];
    if p[b.on] < 0.5 {
        return None;
    }
    let kind = EQ_BANDS[band].type_of(p[b.kind]);
    let cut = matches!(kind, BandType::LowCut | BandType::HighCut);
    let ord = if cut {
        order(p[b.slope].max(0.0).round() as usize)
    } else {
        0
    };
    Some((kind, ord))
}

/// Runs `x` through the active sections of a band.
#[inline]
fn chain(state: &mut [Biquad], coefs: &[Coefs], active: &[bool], mut x: f64) -> f64 {
    for ((b, c), on) in state.iter_mut().zip(coefs).zip(active) {
        if *on {
            x = b.tick(c, x);
        }
    }
    x
}

/// The sections of band `band` for the plain values `p` (in `EQ` order); none where it
/// leaves the sound be (off, or a bell or a shelf at 0 dB).
fn sections(p: &[f64], band: usize, rate: f64) -> [Option<Coefs>; SLOTS] {
    let b = &BANDS[band];
    let mut out = [None; SLOTS];
    if p[b.on] < 0.5 {
        return out;
    }
    let (freq, gain, q) = (p[b.freq], p[b.gain], p[b.q]);
    match EQ_BANDS[band].type_of(p[b.kind]) {
        kind @ (BandType::LowCut | BandType::HighCut) => {
            // The Q goes on the last section (the highest Q) only: at the corner the gain of
            // a section is about its Q, so the whole is `q` (0.707 at 1, flat) at every
            // slope, rather than the bump raised to the power of the sections.
            let r = q_ratio(q);
            let qs = butterworth_qs(order(p[b.slope].max(0.0).round() as usize));
            for (k, bq) in qs.iter().enumerate() {
                let bq = if k + 1 == qs.len() { *bq * r } else { *bq };
                out[k] = Some(if kind == BandType::LowCut {
                    Coefs::highpass(freq, bq, rate)
                } else {
                    Coefs::lowpass(freq, bq, rate)
                });
            }
        }
        BandType::Notch => out[0] = Some(Coefs::notch(freq, q, rate)),
        _ if gain == 0.0 => {}
        BandType::LowShelf => out[0] = Some(Coefs::low_shelf_q(freq, gain, q, rate)),
        BandType::Peaking => out[0] = Some(Coefs::peak(freq, q, gain, rate)),
        BandType::HighShelf => out[0] = Some(Coefs::high_shelf_q(freq, gain, q, rate)),
    }
    out
}

/// The order of a cut for a slope's index (12, 24, 48 dB per octave).
fn order(slope: usize) -> usize {
    match slope {
        0 => 2,
        1 => 4,
        _ => 8,
    }
}

/// The EQ's curve for a set of values: what each band and the whole do to a sine.
#[derive(Clone, Debug)]
pub struct Response {
    rate: f64,
    bands: [[Option<Coefs>; SLOTS]; BANDS.len()],
    output: f64,
}

impl Response {
    /// The curve of `values`: 0..1 values in the order of `xirius_params::EQ` (a song's), the
    /// missing ones at their defaults.
    pub fn new(values: &[f64], rate: f64) -> Response {
        let mut p = [0.0; MAX_PARAMS];
        for (i, spec) in EQ.params.iter().enumerate() {
            let norm = values.get(i).copied().unwrap_or(spec.default_norm());
            p[i] = spec.to_plain(norm);
        }
        Response {
            rate,
            bands: std::array::from_fn(|b| sections(&p, b, rate)),
            output: p[OUTPUT],
        }
    }

    /// The gain in dB at `freq` of band `band` (0 to 5, low to high) alone: 0 where it is off
    /// or out of range, minus infinity at a notch's own frequency.
    pub fn band_db(&self, band: usize, freq: f64) -> f64 {
        self.bands.get(band).map_or(0.0, |s| {
            s.iter()
                .flatten()
                .map(|c| c.response_db(freq, self.rate))
                .sum()
        })
    }

    /// The gain in dB at `freq` of every band and the output gain.
    pub fn db(&self, freq: f64) -> f64 {
        (0..self.bands.len())
            .map(|b| self.band_db(b, freq))
            .sum::<f64>()
            + self.output
    }
}

/// The gain in dB at `freq` of band `band` alone ([`Response::band_db`]); `values` are 0..1
/// in the order of `xirius_params::EQ`.
pub fn band_db(values: &[f64], band: usize, freq: f64, rate: f64) -> f64 {
    Response::new(values, rate).band_db(band, freq)
}

/// The gain in dB at `freq` of the whole EQ, the output gain with it ([`Response::db`]).
pub fn response_db(values: &[f64], freq: f64, rate: f64) -> f64 {
    Response::new(values, rate).db(freq)
}

/// Pushes the mono (L+R)/2 of a block into ring `which`, dropping what does not fit and
/// marking the seam that leaves.
fn feed(ring: &mut Producer<f32>, probe: &Probe, which: usize, left: &[f32], right: &[f32]) {
    for (l, r) in left.iter().zip(right) {
        if ring.push(0.5 * (l + r)).is_err() {
            probe.mark_seam(which);
            break;
        }
    }
}

pub struct Eq {
    params: Params,
    glide: Glide,
    rate: f64,
    coefs: [Coefs; SECTIONS],
    /// Sections that change the sound; the others are skipped and their state kept at rest.
    active: [bool; SECTIONS],
    state: [[Biquad; SECTIONS]; 2],
    /// Each band's shape as last worked out.
    shapes: [Shape; BANDS.len()],
    /// The filters a band is fading out of (its sections as they were when its shape
    /// changed), and the frames left of each band's fade.
    old_coefs: [Coefs; SECTIONS],
    old_active: [bool; SECTIONS],
    old_state: [[Biquad; SECTIONS]; 2],
    fade: [usize; BANDS.len()],
    fade_len: usize,
    gain: f64,
    out: Ramp,
    /// The audio thread's ends of the analyser's rings (ENG-073): the mono input and output.
    rings: (Producer<f32>, Producer<f32>),
}

/// What each analyser ring holds.
pub const RING: usize = 2048;

impl Eq {
    pub fn new(params: Params) -> Eq {
        let (input, input_out) = RingBuffer::new(RING);
        let (output, output_out) = RingBuffer::new(RING);
        params.probe().give_rings(input_out, output_out);
        Eq {
            rings: (input, output),
            glide: Glide::new(&EQ),
            params,
            rate: 48_000.0,
            coefs: [Coefs::THROUGH; SECTIONS],
            active: [false; SECTIONS],
            state: [[Biquad::default(); SECTIONS]; 2],
            shapes: [None; BANDS.len()],
            old_coefs: [Coefs::THROUGH; SECTIONS],
            old_active: [false; SECTIONS],
            old_state: [[Biquad::default(); SECTIONS]; 2],
            fade: [0; BANDS.len()],
            fade_len: 1,
            gain: 1.0,
            out: Ramp::new(1.0),
        }
    }

    /// Works out every section from the gliding values. A band whose shape changed starts
    /// a fade from what it was, its new sections from rest; `fade` false (preparing) jumps.
    fn update(&mut self, fade: bool) {
        let g = &self.glide;
        let mut next = [None; SECTIONS];
        for (b, slots) in next.as_chunks_mut::<SLOTS>().0.iter_mut().enumerate() {
            *slots = sections(&g.cur, b, self.rate);
            let now = shape(&g.cur, b);
            if now == self.shapes[b] {
                continue;
            }
            self.shapes[b] = now;
            if fade {
                // A change again before the fade ends fades out of the newer filter.
                let r = b * SLOTS..(b + 1) * SLOTS;
                self.old_coefs[r.clone()].copy_from_slice(&self.coefs[r.clone()]);
                self.old_active[r.clone()].copy_from_slice(&self.active[r.clone()]);
                for (old, ch) in self.old_state.iter_mut().zip(&mut self.state) {
                    old[r.clone()].copy_from_slice(&ch[r.clone()]);
                }
                // Its new sections come up from rest below.
                self.active[r].fill(false);
                self.fade[b] = self.fade_len;
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
        self.fade = [0; BANDS.len()];
        self.fade_len = ((FADE_SECONDS * self.rate).round() as usize).max(1);
        self.update(false);
        self.out.set(self.gain);
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        let probe = self.params.probe();
        let watch = probe.watching();
        let tap = if watch { probe.tap() } else { Tap::Off };
        if watch {
            probe.note_input(io.left, io.right);
        }
        if tap.input() {
            feed(&mut self.rings.0, self.params.probe(), 0, io.left, io.right);
        }
        let mut at = 0;
        while at < n {
            let len = STEP.min(n - at);
            if self.glide.step(&mut self.params) {
                self.update(true);
            }
            let (g0, dg) = self.out.toward(self.gain, len);
            let fading = self.fade.iter().any(|f| *f > 0);
            let whole = self.fade_len as f64;
            for (ch, buf) in [&mut *io.left, &mut *io.right].into_iter().enumerate() {
                let state = &mut self.state[ch];
                let old = &mut self.old_state[ch];
                let mut g = g0;
                for (j, s) in buf[at..at + len].iter_mut().enumerate() {
                    let mut x = f64::from(*s);
                    if fading {
                        let bands = state.as_chunks_mut::<SLOTS>().0.iter_mut();
                        let olds = old.as_chunks_mut::<SLOTS>().0.iter_mut();
                        for (b, (now, was)) in bands.zip(olds).enumerate() {
                            let r = b * SLOTS..(b + 1) * SLOTS;
                            let y = chain(now, &self.coefs[r.clone()], &self.active[r.clone()], x);
                            let left = self.fade[b].saturating_sub(j);
                            x = if left == 0 {
                                y
                            } else {
                                let z =
                                    chain(was, &self.old_coefs[r.clone()], &self.old_active[r], x);
                                let a = 1.0 - left as f64 / whole;
                                z + (y - z) * a
                            };
                        }
                    } else {
                        x = chain(state, &self.coefs, &self.active, x);
                    }
                    g += dg;
                    *s = (x * g) as f32;
                }
            }
            for f in &mut self.fade {
                *f = f.saturating_sub(len);
            }
            at += len;
        }
        if watch {
            self.params.probe().note_output(io.left, io.right);
        }
        if tap.output() {
            feed(&mut self.rings.1, self.params.probe(), 1, io.left, io.right);
        }
        for ch in self.state.iter_mut().chain(&mut self.old_state) {
            for b in ch.iter_mut() {
                b.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::f64::consts::TAU;
    use std::sync::Arc;

    use super::super::StockHandle;
    use super::super::bank::{ParamBank, Params};
    use super::super::testing::*;
    use super::*;

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

    #[test]
    fn the_band_table_is_the_catalogues() {
        for (b, ids) in BANDS.iter().zip(EQ_BANDS) {
            let at = |id| EQ.param(id).unwrap().0;
            assert_eq!(b.on, at(ids.on), "{}", ids.on);
            assert_eq!(b.kind, at(ids.kind), "{}", ids.kind);
            assert_eq!(b.freq, at(ids.freq), "{}", ids.freq);
            assert_eq!(b.gain, at(ids.gain), "{}", ids.gain);
            assert_eq!(b.q, at(ids.q), "{}", ids.q);
            assert_eq!(b.slope, at(ids.slope), "{}", ids.slope);
        }
        assert_eq!(EQ.param("output").unwrap().0, OUTPUT);
        assert_eq!(EQ.params.len(), 37);
    }

    /// The EQ word for word as it was before its bands had a type (PLG-008), the shelves'
    /// coefficients with it.
    mod before {
        use super::super::super::bank::Params;
        use super::super::super::dsp::{
            Biquad, Coefs, Glide, Ramp, STEP, butterworth_qs, db_to_gain,
        };
        use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

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
        const SECTIONS: usize = 12;

        fn w(freq: f64, rate: f64) -> (f64, f64) {
            let w0 = std::f64::consts::TAU * freq.clamp(1.0, rate * 0.49) / rate;
            (w0.cos(), w0.sin())
        }

        fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Coefs {
            Coefs {
                b0: b0 / a0,
                b1: b1 / a0,
                b2: b2 / a0,
                a1: a1 / a0,
                a2: a2 / a0,
            }
        }

        fn low_shelf(freq: f64, gain_db: f64, rate: f64) -> Coefs {
            let (c, s) = w(freq, rate);
            let a = 10f64.powf(gain_db / 40.0);
            let alpha = s / 2.0 * 2f64.sqrt();
            let k = 2.0 * a.sqrt() * alpha;
            norm(
                a * ((a + 1.0) - (a - 1.0) * c + k),
                2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                a * ((a + 1.0) - (a - 1.0) * c - k),
                (a + 1.0) + (a - 1.0) * c + k,
                -2.0 * ((a - 1.0) + (a + 1.0) * c),
                (a + 1.0) + (a - 1.0) * c - k,
            )
        }

        fn high_shelf(freq: f64, gain_db: f64, rate: f64) -> Coefs {
            let (c, s) = w(freq, rate);
            let a = 10f64.powf(gain_db / 40.0);
            let alpha = s / 2.0 * 2f64.sqrt();
            let k = 2.0 * a.sqrt() * alpha;
            norm(
                a * ((a + 1.0) + (a - 1.0) * c + k),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                a * ((a + 1.0) + (a - 1.0) * c - k),
                (a + 1.0) - (a - 1.0) * c + k,
                2.0 * ((a - 1.0) - (a + 1.0) * c),
                (a + 1.0) - (a - 1.0) * c - k,
            )
        }

        pub struct Eq {
            params: Params,
            glide: Glide,
            rate: f64,
            coefs: [Coefs; SECTIONS],
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
                    (LOW_GAIN, low_shelf(g.get(LOW_FREQ), g.get(LOW_GAIN), rate)),
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
                        high_shelf(g.get(HIGH_FREQ), g.get(HIGH_GAIN), rate),
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
    }

    fn noise(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Different on the two channels: two tones and noise.
    fn program(frames: usize, seed: u64) -> (Vec<f32>, Vec<f32>) {
        let mut s = seed;
        let mut side = |ch: f64| -> Vec<f32> {
            (0..frames)
                .map(|i| {
                    let t = i as f64 / RATE;
                    let tone = (TAU * (90.0 + 40.0 * ch) * t).sin() + (TAU * 3100.0 * t).sin();
                    (0.3 * tone + 0.3 * (2.0 * noise(&mut s) - 1.0)) as f32
                })
                .collect()
        };
        (side(0.0), side(1.0))
    }

    /// Runs today's EQ and the one before on one bank, `knobs` moving the old 17 parameters
    /// between chunks, and compares every sample's bits.
    fn against_before(knobs: impl Fn(&ParamBank, &mut u64)) {
        let bank = Arc::new(ParamBank::new(&EQ));
        let mut now = Eq::new(Params::new(bank.clone()));
        let mut was = before::Eq::new(Params::new(bank.clone()));
        now.prepare(&setup());
        was.prepare(&setup());
        let (l, r) = program(RATE as usize * 2, 3);
        let mut seed = 5;
        let mut at = 0;
        while at < l.len() {
            knobs(&bank, &mut seed);
            let end = (at + 4_801).min(l.len());
            let (a, b) = render(&mut now, &l[at..end], &r[at..end], &[]);
            let (c, d) = render(&mut was, &l[at..end], &r[at..end], &[]);
            for (j, (x, y)) in a.iter().zip(&c).chain(b.iter().zip(&d)).enumerate() {
                assert_eq!(x.to_bits(), y.to_bits(), "frame {}", at + j % (end - at));
            }
            at = end;
        }
    }

    #[test]
    fn at_the_defaults_it_sounds_as_it_did_to_the_bit() {
        against_before(|_, _| {});
    }

    #[test]
    fn the_old_knobs_anywhere_sound_as_they_did_to_the_bit() {
        // The cuts' switches and slopes aside: they jumped before and crossfade now (M-dsp-1;
        // `a_bands_switch_type_and_slope_crossfade` holds them).
        let shapes = [BANDS[0].on, BANDS[0].slope, BANDS[5].on, BANDS[5].slope];
        against_before(|bank, seed| {
            for i in (0..=OUTPUT).filter(|i| !shapes.contains(i)) {
                let mut v = noise(seed);
                // A gain at 0 dB now and then, so a band leaves and comes back.
                if EQ.params[i].unit == xirius_params::Unit::Db && noise(seed) < 0.3 {
                    v = 0.5;
                }
                bank.set_norm(i, v);
            }
        });
    }

    #[test]
    fn a_song_without_the_new_ids_sounds_as_it_did() {
        // PLG-062: a song from before the band types holds the old 17 ids alone; the rest
        // come up at their defaults, and the sound is the old one to the bit.
        let bank = Arc::new(ParamBank::new(&EQ));
        let handle = StockHandle::new(bank.clone(), None);
        for (i, spec) in EQ.params.iter().enumerate().skip(OUTPUT + 1) {
            let away = if spec.default_norm() < 0.5 { 1.0 } else { 0.0 };
            bank.set_norm(i, away);
        }
        let mut song = BTreeMap::new();
        let mut seed = 9;
        for spec in &EQ.params[..=OUTPUT] {
            song.insert(spec.id.to_string(), noise(&mut seed));
        }
        song.insert("low_cut_on".to_string(), 1.0);
        song.insert("high_cut_on".to_string(), 1.0);
        assert!(handle.set_all(&song).is_empty());
        let mut now = Eq::new(Params::new(bank.clone()));
        let mut was = before::Eq::new(Params::new(bank));
        now.prepare(&setup());
        was.prepare(&setup());
        let (l, r) = program(RATE as usize, 4);
        let (a, b) = render(&mut now, &l, &r, &[]);
        let (c, d) = render(&mut was, &l, &r, &[]);
        for (x, y) in a.iter().zip(&c).chain(b.iter().zip(&d)) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
    }

    /// The defaults with `set` (plain values) over them, as 0..1 values.
    fn values(set: &[(&str, f64)]) -> Vec<f64> {
        let mut v: Vec<f64> = EQ.params.iter().map(|p| p.default_norm()).collect();
        for (id, plain) in set {
            let (i, spec) = EQ.param(id).unwrap();
            v[i] = spec.to_norm(*plain);
        }
        v
    }

    #[test]
    fn the_curve_is_what_the_sound_goes_through() {
        let set = [
            ("b2_type", 0.0),
            ("low_freq", 120.0),
            ("b2_q", 1.4),
            ("b2_slope", 0.0),
            ("mid1_freq", 700.0),
            ("mid1_gain", -7.0),
            ("mid1_q", 3.0),
            ("b4_type", 4.0),
            ("mid2_freq", 3000.0),
            ("mid2_gain", 5.0),
            ("mid2_q", 1.2),
            ("b5_type", 1.0),
            ("high_freq", 9000.0),
            ("b5_q", 4.0),
            ("high_cut_on", 1.0),
            ("high_cut_freq", 15_000.0),
            ("output", -2.0),
        ];
        let curve = Response::new(&values(&set), RATE);
        for f in [60.0, 120.0, 400.0, 700.0, 2000.0, 3000.0, 6000.0, 12_000.0] {
            let want = curve.db(f);
            let got = gain_at(f, &set);
            assert!((got - want).abs() < 0.05, "{f} Hz: {got} against {want}");
        }
        assert_eq!(response_db(&values(&set), 700.0, RATE), curve.db(700.0));
        let alone = band_db(&values(&set), 2, 700.0, RATE);
        assert!((alone + 7.0).abs() < 1e-6, "{alone}");
    }

    #[test]
    fn a_curve_of_the_defaults_is_flat_and_missing_values_are_defaults() {
        for f in [20.0, 1000.0, 20_000.0] {
            assert_eq!(response_db(&[], f, RATE), 0.0);
            assert_eq!(response_db(&values(&[]), f, RATE), 0.0);
        }
        let low_cut = values(&[("low_cut_on", 1.0), ("low_cut_freq", 400.0)]);
        assert!((band_db(&low_cut, 0, 400.0, RATE) + 3.01).abs() < 0.01);
        assert_eq!(band_db(&low_cut, 1, 400.0, RATE), 0.0);
        assert_eq!(band_db(&low_cut, 9, 400.0, RATE), 0.0);
    }

    #[test]
    fn a_band_off_leaves_the_sound_and_a_cut_ignores_its_gain() {
        let off = [("b3_on", 0.0), ("mid1_gain", 12.0)];
        assert!(gain_at(400.0, &off).abs() < 0.01);
        let flat = std::f64::consts::FRAC_1_SQRT_2;
        let cut = [
            ("b3_type", 0.0),
            ("mid1_freq", 400.0),
            ("mid1_gain", 12.0),
            ("mid1_q", flat),
        ];
        assert!((gain_at(400.0, &cut) + 3.0).abs() < 0.2);
        let one = gain_at(200.0, &cut);
        assert!((-26.0..-22.0).contains(&one), "{one}");
        // Band 3 as a high cut of 12 dB per octave.
        let high = [
            ("b3_type", 5.0),
            ("mid1_freq", 2000.0),
            ("mid1_q", flat),
            ("b3_slope", 0.0),
        ];
        let one = gain_at(4000.0, &high);
        assert!((-14.0..-10.0).contains(&one), "{one}");
    }

    /// The largest second difference of `x`: a sine's is its amplitude times
    /// (2π·f/rate)², a jump's is about the jump.
    fn roughness(x: &[f32]) -> f64 {
        x.windows(3)
            .map(|w| (f64::from(w[2]) - 2.0 * f64::from(w[1]) + f64::from(w[0])).abs())
            .fold(0.0, f64::max)
    }

    /// A 1 kHz sine through an EQ whose `id` is set to each of `values` in turn, every
    /// 1999 frames (so the changes fall at many phases): the output's roughness.
    fn switched(p: &mut dyn Processor, h: &StockHandle, id: &str, values: &[f64]) -> f64 {
        let input = sine(1000.0, 0.25, 1999 * 8 * values.len());
        let mut out = Vec::new();
        let chunks = input.chunks(1999);
        for (chunk, v) in chunks.zip(values.iter().cycle()) {
            set_plain(h, id, *v);
            out.extend(render(p, chunk, chunk, &[]).0);
        }
        roughness(&out)
    }

    /// M-dsp-1: a band's switch, type and slope go over in a crossfade, with no click; the
    /// EQ before (the cuts' switch and slope jumped) clicks there.
    #[test]
    fn a_bands_switch_type_and_slope_crossfade() {
        // A sine of 0.25 lifted 12 dB at most: 1.0 × (2π/48)² ≈ 0.017 at rest.
        let calm = 0.03;
        let bell = |set: &[(&str, f64)]| {
            let (p, h) = make("eq");
            for (id, v) in [("mid1_freq", 1000.0), ("mid1_gain", 12.0), ("mid1_q", 1.0)] {
                set_plain(&h, id, v);
            }
            for (id, v) in set {
                set_plain(&h, id, *v);
            }
            (p, h)
        };
        let (mut p, h) = bell(&[]);
        let on = switched(p.as_mut(), &h, "b3_on", &[0.0, 1.0]);
        let (mut p, h) = bell(&[]);
        let kind = switched(p.as_mut(), &h, "b3_type", &[2.0, 0.0, 5.0, 3.0]);
        let (mut p, h) = bell(&[("b3_type", 0.0)]);
        let slope = switched(p.as_mut(), &h, "b3_slope", &[0.0, 2.0, 1.0]);
        let (mut p, h) = bell(&[("low_cut_freq", 1000.0)]);
        let cut = switched(p.as_mut(), &h, "low_cut_on", &[1.0, 0.0]);
        for (what, r) in [("on", on), ("type", kind), ("slope", slope), ("cut", cut)] {
            assert!(r < calm, "{what}: {r}");
        }
        // The EQ before, its low cut switched the same way.
        let bank = Arc::new(ParamBank::new(&EQ));
        let mut was = before::Eq::new(Params::new(bank.clone()));
        was.prepare(&setup());
        let id = |k: &str| EQ.params.iter().position(|p| p.id == k).unwrap();
        bank.set_norm(
            id("low_cut_freq"),
            EQ.params[id("low_cut_freq")].to_norm(1000.0),
        );
        let input = sine(1000.0, 0.25, 1999 * 16);
        let mut out = Vec::new();
        for (chunk, v) in input.chunks(1999).zip([1.0, 0.0].iter().cycle()) {
            bank.set_norm(id("low_cut_on"), *v);
            out.extend(render(&mut was, chunk, chunk, &[]).0);
        }
        let before = roughness(&out);
        assert!(before > 2.0 * calm, "{before}");
    }

    #[test]
    fn a_notch_takes_its_frequency_out_whatever_the_gain() {
        let set = [
            ("b2_type", 3.0),
            ("low_freq", 300.0),
            ("b2_q", 4.0),
            ("low_gain", 9.0),
        ];
        assert!(gain_at(300.0, &set) < -40.0);
        assert!(gain_at(3000.0, &set).abs() < 0.1);
        assert!(gain_at(30.0, &set).abs() < 0.1);
        assert_eq!(band_db(&values(&set), 1, 300.0, RATE), f64::NEG_INFINITY);
    }

    #[test]
    fn a_shelf_and_a_cut_bump_at_the_corner_when_their_q_rises() {
        let flat = values(&[("low_gain", 6.0)]);
        let peaky = values(&[("low_gain", 6.0), ("b2_q", 3.0)]);
        let at = |v: &[f64], f: u32| band_db(v, 1, f64::from(f), RATE);
        assert!(at(&flat, 30) > 5.5 && at(&peaky, 30) > 5.5);
        // Above the corner a high Q dips under 0 dB; a slope of 1 never does.
        let dip = (100..2000).map(|f| at(&peaky, f)).fold(0.0, f64::min);
        let none = (100..2000).map(|f| at(&flat, f)).fold(0.0, f64::min);
        assert!(dip < -1.0, "{dip}");
        assert!(none > -1e-6, "{none}");
        let cut = values(&[("low_cut_on", 1.0), ("low_cut_freq", 400.0), ("b1_q", 4.0)]);
        assert!(band_db(&cut, 0, 400.0, RATE) > 6.0);
    }

    #[test]
    fn a_cut_bumps_by_its_q_alone_at_every_slope() {
        // The peak of one section of Q `q`: none up to 0.707.
        let one = |q: f64| {
            if q <= std::f64::consts::FRAC_1_SQRT_2 {
                0.0
            } else {
                20.0 * (q * q / (q * q - 0.25).sqrt()).log10()
            }
        };
        let freqs: Vec<f64> = (0..=3000)
            .map(|i| 20.0 * 1000f64.powf(f64::from(i) / 3000.0))
            .collect();
        for (band, on, freq, q, slope, corner) in [
            (
                0,
                "low_cut_on",
                "low_cut_freq",
                "b1_q",
                "low_cut_slope",
                400.0,
            ),
            (
                5,
                "high_cut_on",
                "high_cut_freq",
                "b6_q",
                "high_cut_slope",
                2000.0,
            ),
        ] {
            for qv in [0.3, 0.707, 1.0, 2.0, 4.0, 10.0, 18.0] {
                let mut peaks = Vec::new();
                for s in 0..3 {
                    let v = values(&[(on, 1.0), (freq, corner), (q, qv), (slope, f64::from(s))]);
                    let r = Response::new(&v, RATE);
                    let peak = freqs
                        .iter()
                        .map(|&f| r.band_db(band, f))
                        .fold(f64::MIN, f64::max);
                    assert!(peak <= one(qv) + 0.5, "{q}={qv} slope {s}: {peak} dB");
                    // At the corner the whole is the Q, whatever the slope.
                    let at = r.band_db(band, corner);
                    assert!(
                        (at - 20.0 * qv.log10()).abs() < 0.5,
                        "{q}={qv} slope {s}: {at} dB"
                    );
                    peaks.push(peak);
                }
                let spread = peaks.iter().fold(f64::MIN, |a, &b| a.max(b))
                    - peaks.iter().fold(f64::MAX, |a, &b| a.min(b));
                assert!(spread < 1.0, "{q}={qv}: {peaks:?}");
            }
        }
        // And the sound: 48 dB per octave at Q 18 is as loud at the corner as one section.
        let set = [
            ("low_cut_on", 1.0),
            ("low_cut_freq", 400.0),
            ("b1_q", 18.0),
            ("low_cut_slope", 2.0),
        ];
        let heard = gain_at(400.0, &set);
        assert!(heard < 20.0 * 18f64.log10() + 0.5, "{heard}");
    }

    #[test]
    fn the_rings_are_fed_only_while_watched() {
        let (mut p, h) = make("eq");
        let probe = h.probe();
        let (mut input, mut output) = (Vec::new(), Vec::new());
        assert_eq!(probe.tap(), Tap::Output);
        let left = vec![1.0f32; 1000];
        let right = vec![0.0f32; 1000];
        render(p.as_mut(), &left, &right, &[]);
        assert!(probe.take_samples(&mut input, &mut output));
        assert!(input.is_empty() && output.is_empty(), "not watched");
        probe.watch(true);
        render(p.as_mut(), &left, &right, &[]);
        probe.take_samples(&mut input, &mut output);
        // Flat, the output is the input: the mono of 1 and 0.
        assert!(input.is_empty());
        assert_eq!(output, vec![0.5; 1000]);
        probe.set_tap(Tap::Both);
        let (mut input, mut output) = (Vec::new(), Vec::new());
        let long = vec![0.25f32; 5000];
        render(p.as_mut(), &long, &long, &[]);
        probe.take_samples(&mut input, &mut output);
        // The input's ring is new, and a full ring drops the rest until it is read (M-dsp-3):
        // what was gathered and what it held are before a seam, so the take starts over.
        assert_eq!((input.len(), output.len()), (0, 0));
        let fresh: Vec<f32> = (0..700).map(|i| i as f32 / 700.0).collect();
        render(p.as_mut(), &fresh, &fresh, &[]);
        probe.take_samples(&mut input, &mut output);
        assert_eq!(input, fresh);
        assert_eq!(output, fresh);
        probe.set_tap(Tap::Off);
        render(p.as_mut(), &long, &long, &[]);
        probe.set_tap(Tap::Input);
        render(p.as_mut(), &long[..300], &long[..300], &[]);
        let (mut input, mut output) = (Vec::new(), Vec::new());
        probe.take_samples(&mut input, &mut output);
        assert_eq!((input.len(), output.len()), (300, 0));
        probe.watch(false);
        render(p.as_mut(), &long, &long, &[]);
        probe.take_samples(&mut input, &mut output);
        assert_eq!((input.len(), output.len()), (300, 0));
        // Watched again, what the window gathered before the gap is gone.
        probe.watch(true);
        render(p.as_mut(), &long[..200], &long[..200], &[]);
        probe.take_samples(&mut input, &mut output);
        assert_eq!((input.len(), output.len()), (200, 0));
    }

    #[test]
    fn only_the_eq_has_rings() {
        let (_, h) = make("compressor");
        assert!(!h.probe().take_samples(&mut Vec::new(), &mut Vec::new()));
    }

    #[test]
    fn the_analyser_finds_a_tone_the_eq_passes() {
        let (mut p, h) = make("eq");
        let probe = h.probe();
        probe.watch(true);
        let x = sine(1000.0, 0.5, RING);
        render(p.as_mut(), &x, &x, &[]);
        let (mut input, mut output) = (Vec::new(), Vec::new());
        probe.take_samples(&mut input, &mut output);
        let points = 200;
        let s = crate::analyzer::spectrum(&output, RATE, points);
        let top = (0..points).max_by(|a, b| s[*a].total_cmp(&s[*b])).unwrap();
        let f = 20.0 * 1000f64.powf(top as f64 / (points - 1) as f64);
        assert!((f / 1000.0).log2().abs() <= 1.0 / 6.0, "{f} Hz");
    }
}
