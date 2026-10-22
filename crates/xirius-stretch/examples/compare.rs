//! Compares Signalsmith Stretch with a plain WSOLA written here, on test signals.
//!
//! `cargo run --release -p xirius-stretch --example compare`
//!
//! Numbers: the pitch error (cents), how much of the output is not the expected partials
//! (residual, dB; lower is cleaner), how wide a click gets (ms), and the time per second of
//! input.

use std::f64::consts::TAU;
use std::time::Instant;

const RATE: f64 = 48_000.0;

fn tone(freqs: &[f64], secs: f64) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let g = 0.6 / freqs.len() as f64;
    (0..n)
        .map(|i| {
            let t = i as f64 / RATE;
            freqs.iter().map(|f| g * (TAU * f * t).sin()).sum::<f64>() as f32
        })
        .collect()
}

/// Short decaying bursts every `every` seconds.
fn clicks(every: f64, secs: f64) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let step = (every * RATE) as usize;
    let mut x = vec![0.0f32; n];
    for start in (step / 2..n).step_by(step) {
        for i in 0..(0.005 * RATE) as usize {
            if start + i < n {
                let t = i as f64 / RATE;
                x[start + i] = (0.8 * (-t / 0.001).exp() * (TAU * 3000.0 * t).sin()) as f32;
            }
        }
    }
    x
}

/// WSOLA: overlap-add of Hann frames, each taken where it best continues the last one.
fn wsola(x: &[f32], out_len: usize) -> Vec<f32> {
    const N: usize = 1024;
    const HS: usize = N / 2;
    const TOL: isize = 256;
    let ratio = out_len as f64 / x.len() as f64;
    let win: Vec<f32> = (0..N)
        .map(|i| (0.5 - 0.5 * (TAU * i as f64 / N as f64).cos()) as f32)
        .collect();
    let at = |i: isize| -> f32 {
        if i >= 0 && (i as usize) < x.len() {
            x[i as usize]
        } else {
            0.0
        }
    };
    let mut y = vec![0.0f32; out_len + N];
    let mut prev: isize = 0;
    let mut k = 0usize;
    while k * HS < out_len {
        let nominal = ((k * HS) as f64 / ratio) as isize;
        let pos = if k == 0 {
            0
        } else {
            // The natural continuation of the previous frame.
            let want: Vec<f32> = (0..N as isize)
                .map(|i| at(prev + HS as isize + i))
                .collect();
            let mut best = (f64::MIN, nominal);
            for d in -TOL..=TOL {
                let c = nominal + d;
                let mut s = 0.0f64;
                for (i, w) in want.iter().enumerate().step_by(2) {
                    s += (*w * at(c + i as isize)) as f64;
                }
                if s > best.0 {
                    best = (s, c);
                }
            }
            best.1
        };
        for i in 0..N {
            y[k * HS + i] += at(pos + i as isize) * win[i];
        }
        prev = pos;
        k += 1;
    }
    y.truncate(out_len);
    y
}

fn resample(x: &[f32], out_len: usize) -> Vec<f32> {
    let step = x.len() as f64 / out_len as f64;
    (0..out_len)
        .map(|i| {
            let p = i as f64 * step;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = x.get(j).copied().unwrap_or(0.0);
            let b = x.get(j + 1).copied().unwrap_or(0.0);
            a + (b - a) * f
        })
        .collect()
}

fn signalsmith(x: &[f32], out_len: usize, transpose: f64) -> Vec<f32> {
    xirius_stretch::render(&[x], RATE as f32, out_len, transpose, &mut |_| true)
        .unwrap()
        .remove(0)
}

fn native(x: &[f32], out_len: usize, transpose: f64) -> Vec<f32> {
    xirius_stretch::render_native(&[x], RATE as f32, out_len, transpose, &mut |_| true)
        .unwrap()
        .remove(0)
}

/// WSOLA pitch shift: stretch by the factor, then resample back to the length.
fn wsola_pitch(x: &[f32], factor: f64) -> Vec<f32> {
    let long = wsola(x, (x.len() as f64 * factor) as usize);
    resample(&long, x.len())
}

/// The residual after taking out the best fit of `freqs` in 4096-sample windows of the
/// middle half, against the whole (dB).
fn residual_db(y: &[f32], freqs: &[f64]) -> f64 {
    const W: usize = 4096;
    let (a, b) = (y.len() / 4, y.len() * 3 / 4);
    let (mut total, mut rest) = (0.0, 0.0);
    let mut s = a;
    while s + W <= b {
        let seg: Vec<f64> = y[s..s + W].iter().map(|v| *v as f64).collect();
        let mut r = seg.clone();
        for f in freqs {
            let (mut c, mut d) = (0.0, 0.0);
            for (i, v) in seg.iter().enumerate() {
                let t = (s + i) as f64 / RATE;
                c += v * (TAU * f * t).cos();
                d += v * (TAU * f * t).sin();
            }
            c *= 2.0 / W as f64;
            d *= 2.0 / W as f64;
            for (i, v) in r.iter_mut().enumerate() {
                let t = (s + i) as f64 / RATE;
                *v -= c * (TAU * f * t).cos() + d * (TAU * f * t).sin();
            }
        }
        total += seg.iter().map(|v| v * v).sum::<f64>();
        rest += r.iter().map(|v| v * v).sum::<f64>();
        s += W;
    }
    10.0 * (rest / total).log10()
}

fn cents(y: &[f32], want: f64) -> f64 {
    let (a, b) = (y.len() / 4, y.len() * 3 / 4);
    let (mut first, mut last, mut n) = (None, 0.0, 0usize);
    for i in a.max(1)..b {
        let (p, q) = (y[i - 1] as f64, y[i] as f64);
        if p < 0.0 && q >= 0.0 {
            let t = (i - 1) as f64 + p / (p - q);
            if first.is_none() {
                first = Some(t);
            } else {
                n += 1;
            }
            last = t;
        }
    }
    let f = n as f64 * RATE / (last - first.unwrap_or(0.0));
    1200.0 * (f / want).log2()
}

/// For each expected click, the span (ms) where the envelope stays above a tenth of its peak,
/// and how many separate bursts show up (1 is right; 2 means a doubled click).
fn click_shape(y: &[f32], every_out: f64) -> (f64, f64) {
    let step = (every_out * RATE) as usize;
    let half = step / 2;
    let (mut width, mut bursts, mut count) = (0.0, 0.0, 0.0);
    let mut centre = half;
    while centre + half < y.len() {
        let seg = &y[centre - half / 2..centre + half / 2];
        let peak = seg.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if peak > 0.01 {
            // A smoothed envelope (1 ms), so the 3 kHz cycles do not count as separate bursts.
            let k = (0.001 * RATE) as usize;
            let env: Vec<f32> = (0..seg.len())
                .map(|i| {
                    seg[i.saturating_sub(k)..(i + 1).min(seg.len())]
                        .iter()
                        .fold(0.0f32, |m, v| m.max(v.abs()))
                })
                .collect();
            let above: Vec<usize> = (0..env.len()).filter(|&i| env[i] > peak * 0.1).collect();
            width += (above.last().unwrap() - above.first().unwrap()) as f64 / RATE * 1000.0;
            let mut b = 0;
            let mut on = false;
            for v in &env {
                let now = *v > peak * 0.3;
                if now && !on {
                    b += 1;
                }
                on = now;
            }
            bursts += b as f64;
            count += 1.0;
        }
        centre += step;
    }
    (width / count, bursts / count)
}

fn main() {
    let secs = 4.0;
    let sine = tone(&[440.0], secs);
    let chord = tone(&[261.63, 329.63, 392.0], secs);
    let click = clicks(0.25, secs);
    println!("| test | signalsmith (+resample for pitch) | signalsmith (own transpose) | wsola |");
    println!("|---|---|---|---|");
    for ratio in [0.75, 1.5] {
        let n = (sine.len() as f64 * ratio) as usize;
        let (s, w) = (signalsmith(&sine, n, 1.0), wsola(&sine, n));
        println!(
            "| sine 440 Hz x{ratio}: pitch error (ct) | {:.2} | (same) | {:.2} |",
            cents(&s, 440.0),
            cents(&w, 440.0)
        );
        println!(
            "| sine 440 Hz x{ratio}: residual (dB) | {:.1} | (same) | {:.1} |",
            residual_db(&s, &[440.0]),
            residual_db(&w, &[440.0])
        );
        let (s, w) = (signalsmith(&chord, n, 1.0), wsola(&chord, n));
        println!(
            "| chord C-E-G x{ratio}: residual (dB) | {:.1} | (same) | {:.1} |",
            residual_db(&s, &[261.63, 329.63, 392.0]),
            residual_db(&w, &[261.63, 329.63, 392.0])
        );
        let n = (click.len() as f64 * ratio) as usize;
        let (s, w) = (signalsmith(&click, n, 1.0), wsola(&click, n));
        let (sw, sb) = click_shape(&s, 0.25 * ratio);
        let (ww, wb) = click_shape(&w, 0.25 * ratio);
        println!("| clicks x{ratio}: width (ms) | {sw:.1} | (same) | {ww:.1} |");
        println!("| clicks x{ratio}: bursts per click | {sb:.2} | (same) | {wb:.2} |");
    }
    for semis in [3.0, -5.0, 12.0] {
        let f = 2f64.powf(semis / 12.0);
        let want = 440.0 * f;
        let (s, n, w) = (
            signalsmith(&sine, sine.len(), f),
            native(&sine, sine.len(), f),
            wsola_pitch(&sine, f),
        );
        println!(
            "| sine 440 Hz {semis:+} st: pitch error (ct) | {:.2} | {:.2} | {:.2} |",
            cents(&s, want),
            cents(&n, want),
            cents(&w, want)
        );
        println!(
            "| sine 440 Hz {semis:+} st: residual (dB) | {:.1} | {:.1} | {:.1} |",
            residual_db(&s, &[want]),
            residual_db(&n, &[want]),
            residual_db(&w, &[want])
        );
        let chord_want: Vec<f64> = [261.63, 329.63, 392.0].iter().map(|c| c * f).collect();
        let (s, n, w) = (
            signalsmith(&chord, chord.len(), f),
            native(&chord, chord.len(), f),
            wsola_pitch(&chord, f),
        );
        println!(
            "| chord C-E-G {semis:+} st: residual (dB) | {:.1} | {:.1} | {:.1} |",
            residual_db(&s, &chord_want),
            residual_db(&n, &chord_want),
            residual_db(&w, &chord_want)
        );
    }
    let long = tone(&[220.0, 330.0], 30.0);
    let t = Instant::now();
    let _ = xirius_stretch::render(
        &[&long, &long],
        RATE as f32,
        long.len() * 3 / 2,
        1.0,
        &mut |_| true,
    );
    let ss = t.elapsed().as_secs_f64() / 30.0 * 1000.0;
    let t = Instant::now();
    let _ = (
        wsola(&long, long.len() * 3 / 2),
        wsola(&long, long.len() * 3 / 2),
    );
    let ws = t.elapsed().as_secs_f64() / 30.0 * 1000.0;
    let t = Instant::now();
    let f = 2f64.powf(3.0 / 12.0);
    let _ = xirius_stretch::render(&[&long, &long], RATE as f32, long.len(), f, &mut |_| true);
    let sp = t.elapsed().as_secs_f64() / 30.0 * 1000.0;
    let t = Instant::now();
    let _ =
        xirius_stretch::render_native(&[&long, &long], RATE as f32, long.len(), f, &mut |_| true);
    let np = t.elapsed().as_secs_f64() / 30.0 * 1000.0;
    let t = Instant::now();
    let _ = (wsola_pitch(&long, f), wsola_pitch(&long, f));
    let wp = t.elapsed().as_secs_f64() / 30.0 * 1000.0;
    println!("| stereo x1.5: ms per second of input | {ss:.1} | (same) | {ws:.1} |");
    println!("| stereo +3 st: ms per second of input | {sp:.1} | {np:.1} | {wp:.1} |");
}
