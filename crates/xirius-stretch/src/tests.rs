use super::*;

const RATE: f32 = 48_000.0;

fn sine(freq: f64, secs: f64) -> Vec<f32> {
    let n = (secs * RATE as f64) as usize;
    (0..n)
        .map(|i| (0.5 * (std::f64::consts::TAU * freq * i as f64 / RATE as f64).sin()) as f32)
        .collect()
}

/// The frequency of the middle half, from rising zero crossings.
fn frequency(x: &[f32]) -> f64 {
    let (a, b) = (x.len() / 4, x.len() * 3 / 4);
    let mut first = None;
    let mut last = 0.0;
    let mut count = 0usize;
    for i in a..b {
        let (p, q) = (x[i - 1] as f64, x[i] as f64);
        if p < 0.0 && q >= 0.0 {
            let t = (i - 1) as f64 + p / (p - q);
            if first.is_none() {
                first = Some(t);
            } else {
                count += 1;
            }
            last = t;
        }
    }
    count as f64 * RATE as f64 / (last - first.unwrap())
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn stretch(x: &[f32], out_len: usize, transpose: f64) -> Vec<f32> {
    render(&[x, x], RATE, out_len, transpose, &mut |_| true)
        .unwrap()
        .remove(0)
}

#[test]
fn the_output_has_the_asked_length() {
    let x = sine(440.0, 1.0);
    for out_len in [24_000, 48_000, 72_000, 96_000, 100_001] {
        assert_eq!(stretch(&x, out_len, 1.0).len(), out_len);
    }
}

#[test]
fn stretching_keeps_the_pitch() {
    let x = sine(440.0, 2.0);
    for ratio in [0.5, 0.75, 1.5, 2.0] {
        let y = stretch(&x, (x.len() as f64 * ratio) as usize, 1.0);
        let f = frequency(&y);
        assert!((f - 440.0).abs() < 2.0, "ratio {ratio}: {f} Hz");
        let p = peak(&y[y.len() / 4..y.len() * 3 / 4]);
        assert!((0.35..0.7).contains(&p), "ratio {ratio}: peak {p}");
    }
}

#[test]
fn transposing_moves_the_pitch_and_keeps_the_length() {
    let x = sine(440.0, 2.0);
    for (semis, want) in [(12.0, 880.0), (-12.0, 220.0), (3.0, 523.25)] {
        let y = stretch(&x, x.len(), transpose_factor(semis, 0.0));
        assert_eq!(y.len(), x.len());
        let f = frequency(&y);
        assert!((f - want).abs() / want < 0.005, "{semis} semis: {f} Hz");
    }
    let y = stretch(&x, x.len(), transpose_factor(0.0, 50.0));
    let f = frequency(&y);
    assert!((f - 452.89).abs() < 1.5, "+50 ct: {f} Hz");
}

#[test]
fn a_short_sound_is_padded_and_cut_back() {
    let x = sine(440.0, 0.01);
    let y = stretch(&x, x.len() * 2, 1.0);
    assert_eq!(y.len(), x.len() * 2);
    assert!(y.iter().all(|s| s.is_finite()));
}

#[test]
fn empty_input_gives_silence() {
    let y = render(&[&[], &[]], RATE, 10, 1.0, &mut |_| true).unwrap();
    assert_eq!(y, vec![vec![0.0; 10]; 2]);
}

#[test]
fn progress_rises_and_can_stop() {
    let x = sine(440.0, 3.0);
    let mut seen = Vec::new();
    render(&[&x, &x], RATE, x.len() * 2, 1.0, &mut |d| {
        seen.push(d);
        true
    })
    .unwrap();
    assert!(seen.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(*seen.last().unwrap(), 1.0);

    let mut calls = 0;
    let r = render(&[&x, &x], RATE, x.len() * 2, 1.0, &mut |_| {
        calls += 1;
        calls < 3
    });
    assert_eq!(r, Err(Cancelled));
    assert_eq!(calls, 3);
}

#[test]
fn resampling_keeps_the_level_and_scales_the_frequency() {
    let x = sine(440.0, 1.0);
    let y = resample(&x, x.len() / 2);
    assert_eq!(y.len(), x.len() / 2);
    // Half as many samples at the same rate: twice the frequency.
    let f = frequency(&y);
    assert!((f - 880.0).abs() < 1.0, "{f} Hz");
    let p = peak(&y[y.len() / 4..y.len() * 3 / 4]);
    assert!((p - 0.5).abs() < 0.01, "peak {p}");
    let dc = resample(&[0.25; 1000], 1700);
    assert!(dc[100..1600].iter().all(|v| (v - 0.25).abs() < 1e-3));
}
