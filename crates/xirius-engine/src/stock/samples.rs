//! The Sampler's sounds (PLG-004): multisamples made by additive synthesis when a Sampler is
//! first prepared at a sample rate, then kept for every Sampler at that rate.
//!
//! No sample files are bundled: every sound here is computed from the partial lists below
//! (written for Xirius, under the project's licence), so nothing comes from a sample library.
//! Each sound has a zone every octave from C1 to C8; a note plays the nearest zone, moved at
//! most six semitones, and the partials of a zone stop below what six semitones up would fold
//! over the Nyquist frequency. Held sounds (strings, organ) loop with the seam crossfaded into
//! the sample; the others decay to silence in the sample itself.

use crate::rt_lock::Mutex;
use std::f64::consts::TAU;
use std::sync::Arc;

/// The zones' root notes: C1 to C8.
pub const ROOTS: [u8; 8] = [24, 36, 48, 60, 72, 84, 96, 108];
/// What a zone is normalised to at its loudest.
const PEAK: f64 = 0.5;

pub struct Zone {
    pub root: u8,
    pub data: Vec<f32>,
    /// `[start, end)` in frames: past `end` the sample goes on from `start`.
    pub looped: Option<(usize, usize)>,
}

impl Zone {
    /// The frame `i` of the sample as it plays (looped, then silent past its end).
    #[inline]
    pub fn at(&self, i: usize) -> f32 {
        match self.looped {
            Some((start, end)) if i >= end => self.data[start + (i - end) % (end - start)],
            _ => self.data.get(i).copied().unwrap_or(0.0),
        }
    }

    /// Between frames by a cubic (Hermite) curve.
    #[inline]
    pub fn read(&self, pos: f64) -> f64 {
        let i = pos as usize;
        let t = pos - i as f64;
        let xm1 = if i == 0 {
            0.0
        } else {
            f64::from(self.at(i - 1))
        };
        let (x0, x1, x2) = (
            f64::from(self.at(i)),
            f64::from(self.at(i + 1)),
            f64::from(self.at(i + 2)),
        );
        let c1 = 0.5 * (x1 - xm1);
        let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
        let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
        ((c3 * t + c2) * t + c1) * t + x0
    }

    /// Whether a voice at `pos` has played the whole sample.
    pub fn done(&self, pos: f64) -> bool {
        self.looped.is_none() && pos >= self.data.len() as f64
    }

    /// The frame to go on from once `pos` passed the loop's end.
    #[inline]
    pub fn wrap(&self, pos: f64) -> f64 {
        match self.looped {
            Some((start, end)) if pos >= end as f64 => pos - (end - start) as f64,
            _ => pos,
        }
    }
}

pub struct Sound {
    pub zones: Vec<Zone>,
}

impl Sound {
    /// The zone that plays `pitch`: the nearest root, the higher one on a tie (moving a sound
    /// down never folds over).
    pub fn zone_for(&self, pitch: u8) -> usize {
        let mut best = 0;
        for (k, z) in self.zones.iter().enumerate() {
            let d = (i32::from(z.root) - i32::from(pitch)).abs();
            let b = (i32::from(self.zones[best].root) - i32::from(pitch)).abs();
            if d <= b {
                best = k;
            }
        }
        best
    }
}

pub struct Library {
    pub rate: f64,
    /// In the order of `xirius_params::SAMPLER_SOUNDS`.
    pub sounds: Vec<Sound>,
}

/// The sounds at `rate`, made the first time they are asked for.
pub fn library(rate: f64) -> Arc<Library> {
    static CACHE: Mutex<Vec<Arc<Library>>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(lib) = cache.iter().find(|l| l.rate == rate) {
        return lib.clone();
    }
    let lib = Arc::new(make(rate));
    cache.push(lib.clone());
    lib
}

/// One partial: a sine at `ratio` times the root, falling 60 dB in `decay` seconds.
#[derive(Clone, Copy)]
struct Partial {
    ratio: f64,
    amp: f64,
    decay: f64,
}

/// How a sound is made at a root frequency.
struct Recipe {
    partials: fn(f0: f64) -> Vec<Partial>,
    /// The sample's length in seconds at a root frequency.
    seconds: fn(f0: f64) -> f64,
    /// For held sounds: the loop's start and length in seconds, and its crossfade.
    looped: Option<(f64, f64, f64)>,
}

const RECIPES: [Recipe; 6] = [
    Recipe {
        partials: piano,
        seconds: |f0| piano_decay(f0).clamp(1.0, 6.0),
        looped: None,
    },
    Recipe {
        partials: electric_piano,
        seconds: |f0| (tine_decay(f0) * 1.1).clamp(0.8, 3.0),
        looped: None,
    },
    Recipe {
        partials: strings,
        seconds: |_| 1.2,
        looped: Some((0.4, 0.8, 0.2)),
    },
    Recipe {
        partials: bass,
        seconds: |_| 2.0,
        looped: None,
    },
    Recipe {
        partials: pluck,
        seconds: |f0| (1.2 * (220.0 / f0).powf(0.3)).clamp(0.5, 2.0),
        looped: None,
    },
    Recipe {
        partials: organ,
        seconds: |_| 0.6,
        looped: Some((0.2, 0.4, 0.1)),
    },
];

fn make(rate: f64) -> Library {
    // Every sound in a thread of its own: the first Sampler waits for the slowest one only.
    let sounds = std::thread::scope(|s| {
        let jobs: Vec<_> = RECIPES
            .iter()
            .map(|r| s.spawn(move || make_sound(r, rate)))
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("a sound could not be made"))
            .collect()
    });
    Library { rate, sounds }
}

fn hz(pitch: f64) -> f64 {
    440.0 * 2f64.powf((pitch - 69.0) / 12.0)
}

fn make_sound(recipe: &Recipe, rate: f64) -> Sound {
    let zones = ROOTS
        .iter()
        .map(|&root| {
            let f0 = hz(f64::from(root));
            // What six semitones up keeps under the Nyquist frequency.
            let top = 0.45 * rate / 2f64.powf(0.5);
            let partials: Vec<Partial> = (recipe.partials)(f0)
                .into_iter()
                .filter(|p| p.ratio * f0 < top)
                .collect();
            // A held sound is made a little past its loop, which may end up to half a period
            // later than asked.
            let seconds = match recipe.looped {
                Some((start, len, _)) => (start + len + 0.1).max((recipe.seconds)(f0)),
                None => (recipe.seconds)(f0),
            };
            let mut data = additive(f0, &partials, (seconds * rate) as usize, rate);
            let looped = recipe.looped.map(|(start, len, fade)| {
                // A whole number of periods, so the seam meets itself as nearly as it can.
                let period = rate / f0;
                let len = ((len * rate / period).round().max(1.0) * period).round() as usize;
                let start = (start * rate) as usize;
                let fade = ((fade * rate) as usize).min(start).min(len);
                crossfade(&mut data, start, start + len, fade);
                data.truncate(start + len);
                (start, start + len)
            });
            if looped.is_none() {
                // A tenth of the sample: a long low note that is cut short dies away.
                let frames = data.len() / 10;
                fade_out(&mut data, frames.max((0.03 * rate) as usize));
            }
            let data = normalise(data);
            Zone { root, data, looped }
        })
        .collect();
    Sound { zones }
}

/// The sum of the partials with a 3 ms attack. Each sine turns by a rotation and each decay
/// shrinks by a factor, frame by frame, so no sine is computed in the loop.
fn additive(f0: f64, partials: &[Partial], frames: usize, rate: f64) -> Vec<f64> {
    let mut out = vec![0.0; frames];
    for (n, p) in partials.iter().enumerate() {
        let w = TAU * p.ratio * f0 / rate;
        let (rc, rs) = (w.cos(), w.sin());
        // Spread starting phases, so the partials do not all peak together.
        let phase = (n * n) as f64 * 0.7;
        let (mut c, mut s) = (phase.cos(), phase.sin());
        let shrink = if p.decay.is_finite() {
            10f64.powf(-3.0 / (p.decay * rate))
        } else {
            1.0
        };
        let mut amp = p.amp;
        for v in out.iter_mut() {
            *v += amp * s;
            (c, s) = (c * rc - s * rs, s * rc + c * rs);
            amp *= shrink;
        }
    }
    let attack = (0.003 * rate) as usize;
    for (i, v) in out.iter_mut().take(attack).enumerate() {
        *v *= i as f64 / attack as f64;
    }
    out
}

/// Blends the `fade` frames before `end` into those before `start` (equal power), so playing
/// on from `start` after `end` has no seam.
fn crossfade(data: &mut [f64], start: usize, end: usize, fade: usize) {
    for i in 0..fade {
        let t = (i as f64 + 0.5) / fade as f64;
        let (out, into) = ((t * TAU / 4.0).cos(), (t * TAU / 4.0).sin());
        let k = end - fade + i;
        data[k] = data[k] * out + data[start - fade + i] * into;
    }
}

fn fade_out(data: &mut [f64], frames: usize) {
    let n = data.len();
    let frames = frames.min(n);
    for i in 0..frames {
        data[n - frames + i] *= 1.0 - (i + 1) as f64 / frames as f64;
    }
}

fn normalise(data: Vec<f64>) -> Vec<f32> {
    let peak = data.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let k = if peak > 0.0 { PEAK / peak } else { 0.0 };
    data.into_iter().map(|v| (v * k) as f32).collect()
}

// The recipes. Ratios above 1 that are not whole numbers are the stretched partials of a
// stiff string or the bell of a tine.

fn piano_decay(f0: f64) -> f64 {
    // Middle C rings for about four and a half seconds; the low strings longer.
    (7.0 * (110.0 / f0).sqrt()).clamp(1.0, 10.0)
}

fn piano(f0: f64) -> Vec<Partial> {
    let stiffness = 0.000_3;
    let t0 = piano_decay(f0);
    (1..=24)
        .map(|n| {
            let n = f64::from(n);
            Partial {
                ratio: n * (1.0 + stiffness * n * n).sqrt(),
                amp: n.powf(-1.1) * (-n * f0 / 4000.0).exp(),
                decay: t0 / (1.0 + 0.25 * (n - 1.0)),
            }
        })
        .collect()
}

fn tine_decay(f0: f64) -> f64 {
    (3.0 * (220.0 / f0).powf(0.4)).clamp(0.8, 4.0)
}

fn electric_piano(f0: f64) -> Vec<Partial> {
    let t0 = tine_decay(f0);
    let bell = 0.2 * (-f0 / 2000.0).exp();
    vec![
        Partial {
            ratio: 1.0,
            amp: 1.0,
            decay: t0,
        },
        Partial {
            ratio: 2.0,
            amp: 0.25,
            decay: t0 * 0.4,
        },
        Partial {
            ratio: 3.0,
            amp: 0.08,
            decay: t0 * 0.3,
        },
        Partial {
            ratio: 7.13,
            amp: bell,
            decay: 0.25,
        },
        Partial {
            ratio: 11.4,
            amp: bell * 0.4,
            decay: 0.15,
        },
    ]
}

fn strings(f0: f64) -> Vec<Partial> {
    // Two saws a few cents apart, darker toward the top.
    let mut out = Vec::new();
    for cents in [-7.0, 7.0] {
        let detune = 2f64.powf(cents / 1200.0);
        for n in 1..=24 {
            let n = f64::from(n);
            let f = n * f0;
            out.push(Partial {
                ratio: n * detune,
                amp: 0.5 / n / (1.0 + (f / 3000.0).powi(2)),
                decay: f64::INFINITY,
            });
        }
    }
    out
}

fn bass(f0: f64) -> Vec<Partial> {
    (1..=16)
        .map(|n| {
            let n = f64::from(n);
            Partial {
                ratio: n,
                amp: n.powf(-1.3) * (-n * f0 / 1500.0).exp(),
                decay: 2.5 / (1.0 + 0.6 * (n - 1.0)),
            }
        })
        .collect()
}

fn pluck(f0: f64) -> Vec<Partial> {
    let t0 = 0.9 * (220.0 / f0).powf(0.3);
    (1..=24)
        .map(|n| {
            let n = f64::from(n);
            Partial {
                ratio: n,
                amp: 1.0 / n,
                decay: t0 / (1.0 + 0.5 * (n - 1.0)),
            }
        })
        .collect()
}

fn organ(_f0: f64) -> Vec<Partial> {
    // Drawbars 8', 4', 2 2/3', 2', 1 1/3', 1'.
    [
        (1.0, 1.0),
        (2.0, 0.7),
        (3.0, 0.5),
        (4.0, 0.35),
        (6.0, 0.25),
        (8.0, 0.2),
    ]
    .into_iter()
    .map(|(ratio, amp)| Partial {
        ratio,
        amp,
        decay: f64::INFINITY,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_a_zone_every_octave_and_is_cached() {
        let a = library(48_000.0);
        let b = library(48_000.0);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.sounds.len(), xirius_params::SAMPLER_SOUNDS.len());
        for s in &a.sounds {
            assert_eq!(s.zones.len(), ROOTS.len());
            for z in &s.zones {
                let peak = z.data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                assert!((f64::from(peak) - PEAK).abs() < 1e-6);
                assert!(z.data.iter().all(|v| v.is_finite()));
            }
        }
    }

    #[test]
    fn a_note_plays_the_nearest_zone() {
        let lib = library(48_000.0);
        let s = &lib.sounds[0];
        assert_eq!(s.zones[s.zone_for(60)].root, 60);
        assert_eq!(s.zones[s.zone_for(65)].root, 60);
        assert_eq!(s.zones[s.zone_for(66)].root, 72);
        assert_eq!(s.zones[s.zone_for(0)].root, 24);
        assert_eq!(s.zones[s.zone_for(127)].root, 108);
    }

    #[test]
    fn a_loop_has_no_seam() {
        let lib = library(48_000.0);
        for sound in [2, 5] {
            for z in &lib.sounds[sound].zones {
                let (start, end) = z.looped.unwrap();
                // The step across the seam is no bigger than the steps inside the loop.
                let step = |i: usize| (z.at(i + 1) - z.at(i)).abs();
                let inside = (start..end - 1).map(step).fold(0.0f32, f32::max);
                assert!(step(end - 1) <= inside, "{} at {start}", z.root);
            }
        }
    }
}
