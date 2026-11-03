//! The Limiter (PLG-012): a brickwall with 5 ms of lookahead, the two channels linked.
//!
//! For each frame the gain that would keep it under the ceiling is held for the lookahead
//! (the smallest of the last [`Limiter::look`] + 1 frames, kept in a monotonic queue), let go
//! with the release, and averaged over the lookahead. Since every value averaged for a frame
//! is at most what that frame needs, the delayed audio never goes over the ceiling, and the
//! gain moves with no corner. The lookahead is the latency the mix is told of (PDC).
//!
//! With a sidechain key (PLG-072, Ext) the key's peaks are detected too: the louder of the key
//! and the main signal sets the gain, so the key turns the audio down and the audio still
//! never goes over the ceiling. No key is Int, the brickwall as before. The key has no filter
//! and no listen, and Input stays on the audio.

use super::bank::Params;
use super::dsp::{Glide, Line, STEP, db_to_gain, gain_to_db};
use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

const INPUT: usize = 0;
const CEILING: usize = 1;
const RELEASE: usize = 2;

/// The lookahead in seconds.
const LOOKAHEAD: f64 = 0.005;

pub struct Limiter {
    params: Params,
    glide: Glide,
    rate: f64,
    /// The lookahead in frames.
    look: usize,
    release: f64,
    delay: [Line; 2],
    /// The monotonic queue of (frame, needed gain) in a ring: the front is the smallest.
    queue: Vec<(u64, f64)>,
    head: usize,
    len: usize,
    frame: u64,
    /// The held gain after the release.
    env: f64,
    /// The last `look` values of `env`, and their sum.
    boxcar: Vec<f64>,
    box_at: usize,
    sum: f64,
}

impl Limiter {
    pub fn new(params: Params) -> Limiter {
        Limiter {
            glide: Glide::new(&xirius_params::LIMITER),
            params,
            rate: 48_000.0,
            look: 0,
            release: 0.0,
            delay: [Line::new(0), Line::new(0)],
            queue: Vec::new(),
            head: 0,
            len: 0,
            frame: 0,
            env: 1.0,
            boxcar: Vec::new(),
            box_at: 0,
            sum: 0.0,
        }
    }

    fn reset(&mut self) {
        for d in &mut self.delay {
            d.clear();
        }
        self.head = 0;
        self.len = 0;
        self.frame = 0;
        self.env = 1.0;
        self.boxcar.fill(1.0);
        self.box_at = 0;
        self.sum = self.look as f64;
    }

    /// Takes in the gain frame `self.frame` needs; gives the smallest of the last `look + 1`.
    #[inline]
    fn hold(&mut self, need: f64) -> f64 {
        let cap = self.queue.len();
        // Values behind that are larger can never be the smallest again.
        while self.len > 0 {
            let back = (self.head + self.len - 1) % cap;
            if self.queue[back].1 < need {
                break;
            }
            self.len -= 1;
        }
        self.queue[(self.head + self.len) % cap] = (self.frame, need);
        self.len += 1;
        let oldest = self.frame.saturating_sub(self.look as u64);
        while self.queue[self.head].0 < oldest {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        self.frame += 1;
        self.queue[self.head].1
    }
}

impl Processor for Limiter {
    fn name(&self) -> &str {
        "Limiter"
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.rate = setup.sample_rate;
        self.look = ((LOOKAHEAD * self.rate).round() as usize).max(1);
        self.delay = [Line::new(self.look + 2), Line::new(self.look + 2)];
        self.queue = vec![(0, 1.0); self.look + 2];
        self.boxcar = vec![1.0; self.look];
        self.params.refresh();
        self.glide.prepare(&self.params, self.rate);
        self.release = (-1.0 / (self.glide.get(RELEASE) * 0.001 * self.rate)).exp();
        self.reset();
    }

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        let n = io.frames();
        let look = self.look;
        let key = io.key.filter(|(l, r)| l.len() >= n && r.len() >= n);
        let watch = self.params.probe().watching();
        if watch {
            self.params.probe().note_input(io.left, io.right);
        }
        let mut least: f64 = 1.0;
        let mut at = 0;
        while at < n {
            let len = STEP.min(n - at);
            if self.glide.step(&mut self.params) {
                self.release = (-1.0 / (self.glide.get(RELEASE) * 0.001 * self.rate)).exp();
            }
            let input = db_to_gain(self.glide.get(INPUT));
            let ceiling = db_to_gain(self.glide.get(CEILING));
            for i in at..at + len {
                let l = f64::from(io.left[i]) * input;
                let r = f64::from(io.right[i]) * input;
                let peak = match key {
                    Some((kl, kr)) => l
                        .abs()
                        .max(r.abs())
                        .max(f64::from(kl[i].abs()).max(f64::from(kr[i].abs()))),
                    None => l.abs().max(r.abs()),
                };
                let need = if peak > ceiling { ceiling / peak } else { 1.0 };
                let held = self.hold(need);
                self.env = if held < self.env {
                    held
                } else {
                    held + (self.env - held) * self.release
                };
                self.sum += self.env - self.boxcar[self.box_at];
                self.boxcar[self.box_at] = self.env;
                self.box_at += 1;
                if self.box_at == look {
                    self.box_at = 0;
                    // Summed afresh once a lap, so rounding never builds up.
                    self.sum = self.boxcar.iter().sum();
                }
                let gain = (self.sum / look as f64).min(1.0);
                least = least.min(gain);
                self.delay[0].push(l);
                self.delay[1].push(r);
                // What rounding is left is clipped; it is far below what can be heard.
                let out = |x: f64| (x * gain).clamp(-ceiling, ceiling) as f32;
                io.left[i] = out(self.delay[0].at(look + 1));
                io.right[i] = out(self.delay[1].at(look + 1));
            }
            at += len;
        }
        if watch {
            let probe = self.params.probe();
            probe.set_gr(-gain_to_db(least.max(1e-9)));
            probe.note_output(io.left, io.right);
        }
    }

    fn latency(&self) -> u32 {
        self.look as u32
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;

    const LOOK: usize = 240;

    #[test]
    fn the_lookahead_is_the_latency() {
        let (p, _) = make("limiter");
        assert_eq!(p.latency() as usize, LOOK);
        let mut p = super::super::create("limiter").unwrap().0;
        p.prepare(&crate::processor::ProcessSetup {
            sample_rate: 96_000.0,
            ..setup()
        });
        assert_eq!(p.latency(), 480);
    }

    #[test]
    fn a_quiet_signal_comes_out_as_it_went_in_later() {
        let (mut p, _) = make("limiter");
        let x = sine(440.0, 0.5, RATE as usize / 2);
        let (l, r) = render(p.as_mut(), &x, &x, &[]);
        assert!(l[..LOOK].iter().all(|s| *s == 0.0));
        for i in LOOK..x.len() {
            assert!((l[i] - x[i - LOOK]).abs() < 1e-6, "{i}");
        }
        assert_eq!(l, r);
    }

    #[test]
    fn nothing_goes_over_the_ceiling() {
        let (mut p, h) = make("limiter");
        set_plain(&h, "ceiling", -6.0);
        set_plain(&h, "input", 12.0);
        p.prepare(&setup());
        // A loud sine with sudden bursts on one channel only.
        let n = RATE as usize;
        let mut l = sine(110.0, 0.8, n);
        let r = sine(3000.0, 0.3, n);
        for k in (0..n).step_by(9_001) {
            l[k] = 1.5;
            l[(k + 1).min(n - 1)] = -2.0;
        }
        let (ol, or) = render(p.as_mut(), &l, &r, &[]);
        let ceiling = 10f64.powf(-6.0 / 20.0);
        assert!(peak(&ol) <= ceiling + 1e-6, "{}", peak(&ol));
        assert!(peak(&or) <= ceiling + 1e-6, "{}", peak(&or));
        // It reaches the ceiling (limits rather than just turning down).
        assert!(peak(&ol) > ceiling * 0.98);
    }

    #[test]
    fn the_gain_comes_back_with_the_release() {
        let (mut p, h) = make("limiter");
        set_plain(&h, "ceiling", -12.0);
        set_plain(&h, "release", 50.0);
        p.prepare(&setup());
        // A loud second half second, then a quiet one.
        let half = RATE as usize / 2;
        let mut x = sine(200.0, 1.0, half);
        x.extend(sine(200.0, 0.1, half));
        let (l, _) = render(p.as_mut(), &x, &x, &[]);
        let at = |ms: f64| half + LOOK + (ms * RATE / 1000.0) as usize;
        let level = |from: usize| db(rms(&l[from..from + 480]) / (0.1 / 2f64.sqrt()));
        // Just after, still turned down by most of the 12 dB; five release times on, back.
        assert!(level(at(2.0)) < -8.0, "{}", level(at(2.0)));
        assert!(level(at(300.0)).abs() < 0.1, "{}", level(at(300.0)));
    }

    /// Renders with the same key on both channels.
    fn keyed(p: &mut dyn crate::Processor, x: &[f32], key: &[f32]) -> Vec<f32> {
        render_keyed(p, x, x, Some((key, key)), &[]).0
    }

    #[test]
    fn a_key_turns_the_audio_down_and_it_still_stays_under_the_ceiling() {
        let (mut p, h) = make("limiter");
        set_plain(&h, "ceiling", -6.0);
        p.prepare(&setup());
        let ceiling = 10f64.powf(-6.0 / 20.0);
        let n = RATE as usize / 2;
        // A quiet main signal and a key twice the ceiling for the second half.
        let x = sine(300.0, 0.2, n);
        let key: Vec<f32> = (0..n)
            .map(|i| if i < n / 2 { 0.0 } else { 2.0 * ceiling as f32 })
            .collect();
        let l = keyed(p.as_mut(), &x, &key);
        let level = |from: usize| db(rms(&l[from..from + 4_800]) / (0.2 / 2f64.sqrt()));
        // Before the key, the audio as it was; with it, turned down by the 6 dB it is over.
        assert!(level(LOOK + 4_800).abs() < 0.01, "{}", level(LOOK + 4_800));
        assert!((level(n / 2 + LOOK + 4_800) + 6.0).abs() < 0.1);

        // A loud main signal under a quiet key: the ceiling holds.
        let (mut p, h) = make("limiter");
        set_plain(&h, "ceiling", -6.0);
        set_plain(&h, "input", 12.0);
        p.prepare(&setup());
        let mut x = sine(110.0, 0.8, n);
        x[n / 3] = 2.0;
        let l = keyed(p.as_mut(), &x, &vec![0.05; n]);
        assert!(peak(&l) <= ceiling + 1e-6, "{}", peak(&l));
        assert!(peak(&l) > ceiling * 0.98);
    }

    #[test]
    fn a_silent_key_leaves_the_brickwall_as_it_was() {
        let x = sine(110.0, 0.9, RATE as usize / 4);
        let (mut a, h) = make("limiter");
        set_plain(&h, "input", 6.0);
        a.prepare(&setup());
        let (mut b, h) = make("limiter");
        set_plain(&h, "input", 6.0);
        b.prepare(&setup());
        let (int, _) = render(a.as_mut(), &x, &x, &[]);
        assert_eq!(keyed(b.as_mut(), &x, &vec![0.0; x.len()]), int);
    }

    #[test]
    fn silence_in_is_silence_out() {
        let (mut p, h) = make("limiter");
        set_plain(&h, "input", 24.0);
        assert_silent(p.as_mut());
    }
}
