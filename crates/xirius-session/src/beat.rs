//! Finding the tempo of a sound (EDT-016's `clip.tempo.detect`). The sound becomes an onset
//! envelope (how much louder each 3 ms gets), whose autocorrelation peaks at the beat period;
//! between 60 and 200 BPM the strongest peak wins, leaning towards 120 so a half or a double tempo
//! loses to the usual one. The period is then fitted to the peaks at its multiples, and a loop
//! whose length is within 0.1% of a whole number of bars is taken as exactly that.

/// Envelope steps per second.
const STEPS: f64 = 344.0;
const SLOWEST: f64 = 60.0;
const FASTEST: f64 = 200.0;
/// The tempo the choice leans towards, and how far (in octaves) the lean reaches.
const USUAL: f64 = 120.0;
const OCTAVES: f64 = 1.0;
/// Below this share of the envelope's power the beat is taken as not there.
const CLEAR: f64 = 0.15;
/// At most this much of the sound is looked at.
const LONGEST: f64 = 120.0;
/// Multiples of the period the fit uses.
const MULTIPLES: usize = 8;
/// How near a whole number of bars a loop's length is to be taken as exact. The detection is
/// finer than this, so a clip that only happens to be near whole bars keeps its tempo.
const LOOP: f64 = 0.001;

/// The tempo of `x` (mono, at `rate`) in BPM, or `None` when it has no clear beat.
pub fn detect(x: &[f32], rate: u32) -> Option<f64> {
    let rate = f64::from(rate);
    let x = &x[..x.len().min((LONGEST * rate) as usize)];
    let hop = (rate / STEPS).round().max(1.0) as usize;
    let steps = rate / hop as f64;
    let env = envelope(x, hop);
    let n = env.len();
    let r = |l: usize| -> f64 {
        if l >= n {
            return 0.0;
        }
        env[..n - l]
            .iter()
            .zip(&env[l..])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            / (n - l) as f64
    };
    let power = r(0);
    if power <= 1e-12 {
        return None;
    }
    let first = (60.0 * steps / FASTEST).floor().max(1.0) as usize;
    let last = ((60.0 * steps / SLOWEST).ceil() as usize + 1).min(n / 2);
    if last <= first + 1 {
        return None;
    }
    let ac: Vec<f64> = (first - 1..=last + 1).map(|l| r(l) / power).collect();
    let at = |l: usize| ac[l + 1 - first];
    let lean = |lag: f64| {
        let octaves = (60.0 * steps / lag / USUAL).log2() / OCTAVES;
        (-0.5 * octaves * octaves).exp()
    };
    let lag = (first..=last)
        .filter(|&l| at(l) >= at(l - 1) && at(l) >= at(l + 1) && at(l) >= CLEAR)
        .max_by(|&a, &b| (at(a) * lean(a as f64)).total_cmp(&(at(b) * lean(b as f64))))?;

    // The period, fitted to where the peaks at its multiples (that fit twice in the envelope) are.
    let mut cache = std::collections::HashMap::new();
    let mut r_at = |l: usize| *cache.entry(l).or_insert_with(|| r(l));
    let (mut moment, mut weight) = (0.0, 0.0);
    let mut period = lag as f64;
    for k in 1..=MULTIPLES {
        let k = k as f64;
        let guess = (period * k).round() as usize;
        if guess + 3 > n / 2 {
            break;
        }
        let top = (guess - 2..=guess + 2)
            .max_by(|&a, &b| r_at(a).total_cmp(&r_at(b)))
            .unwrap_or(guess);
        let (y0, y1, y2) = (r_at(top - 1), r_at(top), r_at(top + 1));
        let bend = y0 - 2.0 * y1 + y2;
        let shift = if bend < 0.0 {
            (0.5 * (y0 - y2) / bend).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        moment += k * (top as f64 + shift);
        weight += k * k;
        period = moment / weight;
    }
    let bpm = 60.0 * steps / period;
    Some(snap(bpm, x.len() as f64 / rate))
}

/// How much louder each step gets than the one before, smoothed, around its mean.
fn envelope(x: &[f32], hop: usize) -> Vec<f64> {
    let level: Vec<f64> = x
        .chunks_exact(hop)
        .map(|c| {
            let e = c.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / hop as f64;
            (1.0 + 1e5 * e).ln()
        })
        .collect();
    let rise: Vec<f64> = (0..level.len())
        .map(|i| match i {
            0 => 0.0,
            _ => (level[i] - level[i - 1]).max(0.0),
        })
        .collect();
    let mut env: Vec<f64> = (0..rise.len())
        .map(|i| {
            let before = rise[i.saturating_sub(1)];
            let after = rise.get(i + 1).copied().unwrap_or(0.0);
            0.25 * before + 0.5 * rise[i] + 0.25 * after
        })
        .collect();
    let mean = env.iter().sum::<f64>() / env.len().max(1) as f64;
    for v in &mut env {
        *v -= mean;
    }
    env
}

/// A loop of `seconds` that is a whole number of bars (of four beats) within [`LOOP`] is taken as
/// exactly that; otherwise the tempo is kept to a hundredth.
fn snap(bpm: f64, seconds: f64) -> f64 {
    let beats = seconds * bpm / 60.0;
    let whole = (beats / 4.0).round() * 4.0;
    if whole >= 4.0 && (beats - whole).abs() / whole <= LOOP {
        let exact = 60.0 * whole / seconds;
        if (exact - exact.round()).abs() < 0.005 {
            return exact.round();
        }
        return (exact * 100.0).round() / 100.0;
    }
    (bpm * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// Short 1 kHz ticks at `bpm` for `seconds`, starting `lead` seconds in.
    fn clicks(bpm: f64, seconds: f64, lead: f64) -> Vec<f32> {
        let rate = f64::from(RATE);
        let beat = 60.0 / bpm;
        (0..(seconds * rate) as usize)
            .map(|i| {
                let t = i as f64 / rate - lead;
                if t < 0.0 {
                    return 0.0;
                }
                let since = t % beat;
                (0.8 * (-since * 60.0).exp() * (since * 1000.0 * std::f64::consts::TAU).sin())
                    as f32
            })
            .collect()
    }

    #[test]
    fn finds_the_beat_of_ticks() {
        // 13.3 s at 72 BPM is 15.96 beats, near four bars but not a loop of them.
        for bpm in [72.0, 100.0, 128.0, 174.0] {
            let got = detect(&clicks(bpm, 13.3, 0.11), RATE).unwrap();
            assert!((got - bpm).abs() < 0.03, "{bpm}: {got}");
        }
    }

    #[test]
    fn a_loop_of_whole_bars_is_exact() {
        // Four bars at 128 BPM.
        assert_eq!(detect(&clicks(128.0, 7.5, 0.0), RATE), Some(128.0));
        // Two bars at 93 BPM.
        let got = detect(&clicks(93.0, 8.0 * 60.0 / 93.0, 0.0), RATE).unwrap();
        assert_eq!(got, 93.0);
    }

    #[test]
    fn silence_and_noise_have_no_beat() {
        assert_eq!(detect(&vec![0.0; RATE as usize * 4], RATE), None);
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..RATE as usize * 6)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
            })
            .collect();
        assert_eq!(detect(&noise, RATE), None);
        assert_eq!(detect(&[0.5; 100], RATE), None);
    }
}
