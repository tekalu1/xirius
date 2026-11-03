//! The spectrum the EQ's window draws behind its curve (ENG-073): the newest samples of a
//! tap, through a Hann window and a 2048-point FFT, smoothed over a sixth of an octave.

use std::f64::consts::{PI, TAU};

/// The FFT's length: the newest this many samples are measured.
pub const FFT_LEN: usize = 2048;
/// What a point with no sound at all reads, in dB.
pub const FLOOR_DB: f32 = -120.0;
/// The range the points span, in Hz.
pub const LOWEST: f64 = 20.0;
pub const HIGHEST: f64 = 20_000.0;

/// The level in dBFS (a full-scale sine reads 0) at `points` frequencies spaced evenly in
/// pitch from [`LOWEST`] to [`HIGHEST`], of the newest [`FFT_LEN`] of `samples` (fewer are
/// padded with silence before them). Each point is the strongest bin within a sixth of an
/// octave around it, or between the two bins nearest it where the bins are wider than that,
/// so a sine reads its level at any pitch (less the window's scallop, at most 1.5 dB).
pub fn spectrum(samples: &[f32], rate: f64, points: usize) -> Vec<f32> {
    if points == 0 {
        return Vec::new();
    }
    let n = FFT_LEN;
    let newest = &samples[samples.len().saturating_sub(n)..];
    let pad = n - newest.len();
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let mut window_sum = 0.0;
    for (i, r) in re.iter_mut().enumerate() {
        let w = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
        window_sum += w;
        if i >= pad {
            *r = f64::from(newest[i - pad]) * w;
        }
    }
    fft(&mut re, &mut im);
    // Power of each bin up to Nyquist, scaled so a sine's peak bin reads its amplitude.
    let scale = 2.0 / window_sum;
    let power: Vec<f64> = (0..=n / 2)
        .map(|k| (re[k] * re[k] + im[k] * im[k]) * scale * scale)
        .collect();
    let valid = rate.is_finite() && rate > 0.0;
    let bin = if valid { rate / n as f64 } else { 1.0 };
    let half = 2f64.powf(1.0 / 12.0);
    (0..points)
        .map(|i| {
            if !valid {
                return FLOOR_DB;
            }
            let t = if points > 1 {
                i as f64 / (points - 1) as f64
            } else {
                0.0
            };
            let f = LOWEST * (HIGHEST / LOWEST).powf(t);
            let lo = (f / half / bin).ceil().max(0.0) as usize;
            let hi = ((f * half / bin).floor() as usize).min(n / 2);
            let p = if lo <= hi {
                power[lo..=hi].iter().copied().fold(0.0, f64::max)
            } else {
                // Narrower than a bin: between the two around it.
                let x = (f / bin).min((n / 2) as f64);
                let k = (x as usize).min(n / 2 - 1);
                let t = x - k as f64;
                power[k] * (1.0 - t) + power[k + 1] * t
            };
            to_db(p)
        })
        .collect()
}

fn to_db(power: f64) -> f32 {
    if power > 0.0 {
        ((10.0 * power.log10()) as f32).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

/// An in-place radix-2 FFT; the length is a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        let (ws, wc) = angle.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut c, mut s) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                (c, s) = (c * wc - s * ws, c * ws + s * wc);
            }
        }
        len *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn sine(freq: f64, amp: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (amp * (TAU * freq * i as f64 / RATE).sin()) as f32)
            .collect()
    }

    /// The frequency of point `i` of `points`.
    fn freq_of(i: usize, points: usize) -> f64 {
        LOWEST * (HIGHEST / LOWEST).powf(i as f64 / (points - 1) as f64)
    }

    #[test]
    fn the_fft_finds_a_bins_sine_and_nothing_else() {
        let n = 64;
        let mut re: Vec<f64> = (0..n)
            .map(|i| (TAU * 5.0 * i as f64 / n as f64).cos())
            .collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        for k in 0..n {
            let m = re[k].hypot(im[k]);
            let want = if k == 5 || k == n - 5 {
                n as f64 / 2.0
            } else {
                0.0
            };
            assert!((m - want).abs() < 1e-9, "bin {k}: {m}");
        }
    }

    #[test]
    fn a_1_khz_sine_peaks_within_a_sixth_of_an_octave_of_it() {
        let points = 240;
        let s = spectrum(&sine(1000.0, 0.5, 4096), RATE, points);
        assert_eq!(s.len(), points);
        let (top, level) =
            s.iter().enumerate().fold(
                (0, f32::MIN),
                |m, (i, v)| if *v > m.1 { (i, *v) } else { m },
            );
        let octaves = (freq_of(top, points) / 1000.0).log2().abs();
        assert!(octaves <= 1.0 / 6.0, "{} Hz", freq_of(top, points));
        // Half of full scale.
        assert!((level + 6.02).abs() < 1.5, "{level}");
        // Far from it, the window keeps it down.
        assert!(s[0] < level - 60.0, "{}", s[0]);
        assert!(s[points - 1] < level - 60.0, "{}", s[points - 1]);
    }

    /// A sine reads its level wherever it is, low or high (M-dsp-2): the band's strongest bin,
    /// not its mean, which read high tones low and tilted the picture.
    #[test]
    fn a_sine_reads_its_level_at_every_pitch() {
        let points = 240;
        for freq in [
            45.0, 110.0, 333.0, 1000.0, 2345.0, 7000.0, 12_500.0, 18_000.0,
        ] {
            for amp in [1.0, 0.5, 0.1] {
                let s = spectrum(&sine(freq, amp, 4096), RATE, points);
                let top = s.iter().copied().fold(f32::MIN, f32::max);
                let want = 20.0 * (amp as f32).log10();
                assert!((top - want).abs() < 1.5, "{freq} Hz at {amp}: {top} dB");
            }
        }
    }

    #[test]
    fn silence_and_nothing_read_the_floor() {
        assert!(
            spectrum(&[0.0; 3000], RATE, 10)
                .iter()
                .all(|v| *v == FLOOR_DB)
        );
        assert!(spectrum(&[], RATE, 10).iter().all(|v| *v == FLOOR_DB));
        assert!(spectrum(&[0.5; 10], 0.0, 4).iter().all(|v| *v == FLOOR_DB));
        assert!(spectrum(&[0.5; 10], RATE, 0).is_empty());
        assert_eq!(spectrum(&[0.5; 10], RATE, 1).len(), 1);
    }

    #[test]
    fn fewer_samples_than_the_fft_still_find_the_tone() {
        let points = 120;
        let s = spectrum(&sine(4000.0, 0.5, 1500), RATE, points);
        let top = (0..points).max_by(|a, b| s[*a].total_cmp(&s[*b])).unwrap();
        let octaves = (freq_of(top, points) / 4000.0).log2().abs();
        assert!(octaves <= 1.0 / 6.0, "{} Hz", freq_of(top, points));
    }
}
