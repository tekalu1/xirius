//! Offline time-stretch and pitch-shift of a whole sound (EDT-007, LIC-013).
//!
//! The work is done by Signalsmith Stretch (MIT), whose headers are vendored in `vendor/`.
//! This is meant for a background thread, never the audio thread: it allocates, takes about
//! 15 ms per second of stereo audio (release build), and reports progress so the caller can
//! show it and stop it. `examples/compare.rs` measures it against a plain WSOLA.

use std::ffi::c_void;
use std::os::raw::c_int;

unsafe extern "C" {
    fn xirius_stretch_min_output(channels: c_int, rate: f32) -> c_int;
    fn xirius_stretch_render(
        channels: c_int,
        rate: f32,
        input: *const *const f32,
        input_length: usize,
        output: *const *mut f32,
        output_length: usize,
        transpose: f32,
        progress: Option<extern "C" fn(*mut c_void, f64) -> c_int>,
        user: *mut c_void,
    ) -> c_int;
}

/// The caller stopped the work through the progress callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

/// The frequency multiplier for a shift in semitones and cents.
pub fn transpose_factor(semis: f64, cents: f64) -> f64 {
    2f64.powf((semis + cents / 100.0) / 12.0)
}

/// Stretches `input` (one slice per channel, all the same length) to `out_len` frames and
/// multiplies its pitch by `transpose` (1 keeps it). `progress` gets the done fraction
/// (0..=1) now and then; returning `false` stops the work.
pub fn render(
    input: &[&[f32]],
    rate: f32,
    out_len: usize,
    transpose: f64,
    progress: &mut dyn FnMut(f64) -> bool,
) -> Result<Vec<Vec<f32>>, Cancelled> {
    if (transpose - 1.0).abs() < 1e-9 {
        return stretch(input, rate, out_len, 1.0, progress);
    }
    // The vocoder's own transpose is off by a few cents on a plain tone (it estimates each
    // peak from the bin energies), so shift the pitch by stretching to `transpose` times the
    // length and resampling back, which keeps the pitch exact.
    let long = (out_len as f64 * transpose).round().max(1.0) as usize;
    let mut part = |d: f64| progress(d * 0.9);
    let stretched = stretch(input, rate, long, 1.0, &mut part)?;
    let out = stretched.iter().map(|c| resample(c, out_len)).collect();
    if !progress(1.0) {
        return Err(Cancelled);
    }
    Ok(out)
}

/// Like [`render`], but with the vocoder's own transpose (kept for the comparison example).
pub fn render_native(
    input: &[&[f32]],
    rate: f32,
    out_len: usize,
    transpose: f64,
    progress: &mut dyn FnMut(f64) -> bool,
) -> Result<Vec<Vec<f32>>, Cancelled> {
    stretch(input, rate, out_len, transpose, progress)
}

fn stretch(
    input: &[&[f32]],
    rate: f32,
    out_len: usize,
    transpose: f64,
    progress: &mut dyn FnMut(f64) -> bool,
) -> Result<Vec<Vec<f32>>, Cancelled> {
    let channels = input.len();
    let in_len = input.iter().map(|c| c.len()).min().unwrap_or(0);
    if channels == 0 || out_len == 0 || in_len == 0 {
        return Ok(vec![vec![0.0; out_len]; channels]);
    }
    // SAFETY: plain arithmetic on the C++ side.
    let min = unsafe { xirius_stretch_min_output(channels as c_int, rate) }.max(0) as usize;
    if out_len >= min {
        return run(input, in_len, rate, out_len, transpose, progress);
    }
    // Too short for the exact method: pad with silence, keeping the ratio, and cut back.
    let pad_out = min;
    let pad_in = (in_len as f64 * pad_out as f64 / out_len as f64).ceil() as usize;
    let padded: Vec<Vec<f32>> = input
        .iter()
        .map(|c| {
            let mut v = c[..in_len].to_vec();
            v.resize(pad_in, 0.0);
            v
        })
        .collect();
    let views: Vec<&[f32]> = padded.iter().map(|c| c.as_slice()).collect();
    let mut out = run(&views, pad_in, rate, pad_out, transpose, progress)?;
    for c in &mut out {
        c.truncate(out_len);
    }
    Ok(out)
}

fn run(
    input: &[&[f32]],
    in_len: usize,
    rate: f32,
    out_len: usize,
    transpose: f64,
    progress: &mut dyn FnMut(f64) -> bool,
) -> Result<Vec<Vec<f32>>, Cancelled> {
    let mut out: Vec<Vec<f32>> = vec![vec![0.0; out_len]; input.len()];
    let ins: Vec<*const f32> = input.iter().map(|c| c.as_ptr()).collect();
    let outs: Vec<*mut f32> = out.iter_mut().map(|c| c.as_mut_ptr()).collect();

    extern "C" fn tick(user: *mut c_void, done: f64) -> c_int {
        // SAFETY: `user` is the `&mut &mut dyn FnMut` below, alive for the whole call.
        let f = unsafe { &mut *(user as *mut &mut dyn FnMut(f64) -> bool) };
        c_int::from(!f(done))
    }
    let mut f: &mut dyn FnMut(f64) -> bool = progress;
    // SAFETY: every channel pointer covers `in_len` / `out_len` floats, and the buffers
    // outlive the call.
    let r = unsafe {
        xirius_stretch_render(
            ins.len() as c_int,
            rate,
            ins.as_ptr(),
            in_len,
            outs.as_ptr(),
            out_len,
            transpose as f32,
            Some(tick),
            &mut f as *mut &mut dyn FnMut(f64) -> bool as *mut c_void,
        )
    };
    match r {
        0 => Ok(out),
        1 => Err(Cancelled),
        // Not reached: `stretch` pads short outputs first.
        _ => Ok(out),
    }
}

/// Band-limited resampling to `out_len` frames (Blackman-windowed sinc, 16 zero crossings a
/// side, from a table of 512 phases).
pub fn resample(x: &[f32], out_len: usize) -> Vec<f32> {
    use std::f64::consts::{PI, TAU};
    const PHASES: usize = 512;
    if x.is_empty() || out_len == 0 {
        return vec![0.0; out_len];
    }
    let step = x.len() as f64 / out_len as f64;
    // Below the output's Nyquist when shrinking.
    let cut = (1.0 / step).min(1.0);
    let half = (16.0 / cut).ceil() as usize;
    let taps = half * 2;
    // Row p holds the taps for a fraction p / PHASES past the centre sample, summing to 1.
    let mut table = vec![0.0f32; (PHASES + 1) * taps];
    for p in 0..=PHASES {
        let frac = p as f64 / PHASES as f64;
        let row = &mut table[p * taps..(p + 1) * taps];
        let mut sum = 0.0;
        for (k, t) in row.iter_mut().enumerate() {
            let d = frac + half as f64 - 1.0 - k as f64;
            let w = d / half as f64;
            if w.abs() >= 1.0 {
                continue;
            }
            let win = 0.42 + 0.5 * (PI * w).cos() + 0.08 * (TAU * w).cos();
            let a = cut * d;
            let sinc = if a.abs() < 1e-12 {
                1.0
            } else {
                (PI * a).sin() / (PI * a)
            };
            *t = (sinc * win) as f32;
            sum += sinc * win;
        }
        for t in row.iter_mut() {
            *t = (*t as f64 / sum) as f32;
        }
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = (i as f64 + 0.5) * step - 0.5;
        let centre = pos.floor();
        let at = (pos - centre) * PHASES as f64;
        let p = (at as usize).min(PHASES - 1);
        let mix = (at - p as f64) as f32;
        let (a, b) = (
            &table[p * taps..(p + 1) * taps],
            &table[(p + 1) * taps..(p + 2) * taps],
        );
        let first = centre as isize - half as isize + 1;
        let mut acc = 0.0f32;
        if first >= 0 && first as usize + taps <= x.len() {
            let xs = &x[first as usize..first as usize + taps];
            for k in 0..taps {
                acc += xs[k] * (a[k] + (b[k] - a[k]) * mix);
            }
        } else {
            for k in 0..taps {
                let j = first + k as isize;
                if j >= 0 && (j as usize) < x.len() {
                    acc += x[j as usize] * (a[k] + (b[k] - a[k]) * mix);
                }
            }
        }
        out.push(acc);
    }
    out
}

#[cfg(test)]
mod tests;
