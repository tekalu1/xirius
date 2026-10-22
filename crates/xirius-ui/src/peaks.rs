//! Synthetic waveform data as a min/max peak pyramid (UI-023).
//!
//! Real audio would be reduced to peaks once at import. Here the base level is
//! synthesised directly from a hash-based envelope, so 200 tracks of four
//! minutes cost milliseconds instead of gigabytes of samples. The renderer only
//! ever sees the pyramid.

pub const SAMPLE_RATE: f64 = 48_000.0;
/// Samples summarised by one level-0 peak.
pub const BASE_SPP: u32 = 128;

/// Min/max of a run of samples, quantised to i8 (1/127 of full scale).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peak {
    pub min: i8,
    pub max: i8,
}

impl Peak {
    pub fn merge(self, other: Peak) -> Peak {
        Peak {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// `levels[k]` has one peak per `BASE_SPP << k` samples.
#[derive(Clone, Debug)]
pub struct PeakPyramid {
    levels: Vec<Vec<Peak>>,
}

impl PeakPyramid {
    pub fn from_base(base: Vec<Peak>) -> PeakPyramid {
        let mut levels = vec![base];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let prev = levels.last().expect("levels is never empty");
            let next: Vec<Peak> = prev
                .chunks(2)
                .map(|pair| {
                    pair.iter()
                        .copied()
                        .reduce(Peak::merge)
                        .expect("non-empty chunk")
                })
                .collect();
            levels.push(next);
        }
        PeakPyramid { levels }
    }

    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    pub fn len_samples(&self) -> u64 {
        self.levels[0].len() as u64 * u64::from(BASE_SPP)
    }

    /// Samples per peak at `level`.
    pub fn level_spp(level: usize) -> f64 {
        f64::from(BASE_SPP) * (1u64 << level) as f64
    }

    /// The coarsest level that still has at least one peak per pixel, i.e. the
    /// largest level whose `spp` is <= `samples_per_px` (level 0 at minimum).
    pub fn level_for(&self, samples_per_px: f64) -> usize {
        let mut level = 0;
        while level + 1 < self.levels.len() && Self::level_spp(level + 1) <= samples_per_px {
            level += 1;
        }
        level
    }

    pub fn level(&self, level: usize) -> &[Peak] {
        &self.levels[level]
    }

    /// Min/max over the peaks of `level` that overlap `[start, end)` samples.
    /// Returns `None` when the range is outside the data.
    pub fn reduce(&self, level: usize, start: f64, end: f64) -> Option<Peak> {
        let peaks = &self.levels[level];
        let spp = Self::level_spp(level);
        let first = (start / spp).floor().max(0.0) as usize;
        let last = ((end / spp).ceil() as usize)
            .max(first + 1)
            .min(peaks.len());
        if first >= last {
            return None;
        }
        peaks[first..last].iter().copied().reduce(Peak::merge)
    }
}

/// splitmix64: a cheap, deterministic hash.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn unit(x: u64) -> f32 {
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Builds a four-minute-style track: slow loudness phrases with transients on
/// top, different for every `seed`.
pub fn synth_base(seed: u64, seconds: f64) -> Vec<Peak> {
    let n = (seconds * SAMPLE_RATE / f64::from(BASE_SPP)).ceil() as usize;
    let bucket_s = f64::from(BASE_SPP) / SAMPLE_RATE;
    let phrase = 1.5 + (mix(seed) % 40) as f64 / 10.0; // seconds
    let beat = 0.25 + (mix(seed ^ 0xA5) % 30) as f64 / 100.0; // seconds
    let floor = 0.05 + unit(mix(seed ^ 0x77)) * 0.2;
    (0..n)
        .map(|i| {
            let t = i as f64 * bucket_s;
            let slow = 0.5 + 0.5 * (t / phrase * std::f64::consts::TAU).sin();
            let beat_pos = (t / beat).fract();
            let hit = (-beat_pos * 6.0).exp();
            let env = (floor as f64 + (0.65 * slow + 0.5 * hit) * (1.0 - floor as f64)).min(1.0);
            let h = mix(seed.wrapping_mul(0x1_0000_0001).wrapping_add(i as u64));
            let a = (0.55 + 0.45 * unit(h)) as f64;
            let b = (0.55 + 0.45 * unit(h.rotate_left(24))) as f64;
            Peak {
                min: (-(env * a) * 127.0) as i8,
                max: (env * b * 127.0) as i8,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brute(base: &[Peak], first: usize, last: usize) -> Peak {
        base[first..last]
            .iter()
            .copied()
            .reduce(Peak::merge)
            .unwrap()
    }

    #[test]
    fn pyramid_levels_are_exact_merges_of_the_base() {
        let base = synth_base(7, 5.0);
        let pyr = PeakPyramid::from_base(base.clone());
        for level in 0..pyr.level_count() {
            let width = 1usize << level;
            for (i, peak) in pyr.level(level).iter().enumerate() {
                let first = i * width;
                let last = ((i + 1) * width).min(base.len());
                assert_eq!(*peak, brute(&base, first, last), "level {level} peak {i}");
            }
        }
        assert_eq!(pyr.level(pyr.level_count() - 1).len(), 1);
    }

    #[test]
    fn level_choice_follows_zoom() {
        let pyr = PeakPyramid::from_base(synth_base(1, 30.0));
        assert_eq!(pyr.level_for(1.0), 0);
        assert_eq!(pyr.level_for(127.0), 0);
        assert_eq!(pyr.level_for(128.0), 0);
        assert_eq!(pyr.level_for(256.0), 1);
        assert_eq!(pyr.level_for(1000.0), 2);
        assert_eq!(pyr.level_for(1e12), pyr.level_count() - 1);
    }

    #[test]
    fn reduce_matches_brute_force_and_covers_the_range() {
        let base = synth_base(3, 10.0);
        let pyr = PeakPyramid::from_base(base.clone());
        // 1000 samples = peaks 7..=15 of level 0 (7*128=896 .. 16*128=2048).
        let got = pyr.reduce(0, 1000.0, 2000.0).unwrap();
        assert_eq!(got, brute(&base, 7, 16));
        assert!(pyr.reduce(0, 1e12, 1e12 + 10.0).is_none());
        // An empty range still returns the peak it falls in.
        assert_eq!(pyr.reduce(0, 0.0, 0.0).unwrap(), base[0]);
    }

    #[test]
    fn synthetic_waveform_is_deterministic_and_in_range() {
        let a = synth_base(42, 3.0);
        let b = synth_base(42, 3.0);
        let c = synth_base(43, 3.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.iter().all(|p| p.min <= 0 && p.max >= 0));
        assert!(a.iter().any(|p| p.max > 60), "should have loud parts");
    }

    #[test]
    fn length_matches_duration() {
        let pyr = PeakPyramid::from_base(synth_base(1, 2.0));
        let samples = pyr.len_samples() as f64;
        assert!((samples - 2.0 * SAMPLE_RATE).abs() < f64::from(BASE_SPP));
    }
}
