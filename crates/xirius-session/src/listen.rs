//! `mix.listen` (AI-004, AI-013): what a stretch of the song sounds like, as numbers. The
//! stretch is rendered offline the way an export is, and nothing is written: the answer is the
//! peak, the RMS, the loudness, the stereo width, the same for each bar, and the spectrum.
//!
//! - The same song and the same range give the same numbers: the render is offline (ENG-030),
//!   and the analysis keeps no state and adds up in a fixed order.
//! - Levels are dBFS of the plain RMS over both channels, as the export reports them (a sine at
//!   full scale is -3 dBFS). A band of the spectrum is the part of that power in the band, so
//!   the bands add up to the whole.
//! - Silence has no level: those values are `null`.

use std::f64::consts::PI;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::{Project, TrackKind};
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};
use xirius_plugins::Catalog;

use crate::build::Builder;
use crate::export::{self, render, resolve_range};
use crate::graph::Select;
use crate::tempo::TempoMap;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["mix.listen"];

/// The nominal centres of the third-octave bands (IEC 61260); every third one is an octave's.
const THIRDS: [f64; 30] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
    10000.0, 12500.0, 16000.0, 20000.0,
];

/// The index of 1 kHz in [`THIRDS`]: the exact centres are 1 kHz times powers of 2^(1/3).
const KHZ: i32 = 16;

pub fn run(cx: &ExtContext<'_>, catalog: Option<Arc<Catalog>>, args: &Args) -> CmdResult<Output> {
    let p = cx.project;
    let range = crate::commands::options(p, cx.dir, args)?.range;
    let (a, b) = resolve_range(p, &range).map_err(|e| CmdError::invalid(e.0))?;
    let (select, target) = match args.opt_id::<TrackId>("track")? {
        None => (Select::All, json!({ "kind": "master" })),
        Some(id) => {
            let t = p
                .tracks
                .get(&id)
                .ok_or_else(|| CmdError::not_found(format!("トラック {id} が見つかりません")))?;
            let (select, kind) = match t.kind {
                TrackKind::Master => (Select::All, "master"),
                TrackKind::Bus => (Select::Bus(id), "bus"),
                _ => (Select::Track(id), "track"),
            };
            (
                select,
                json!({ "kind": kind, "id": id.to_string(), "name": t.name }),
            )
        }
    };
    let thirds = args.opt_str("bands") == Some("third");
    let heard = listen(p, cx.dir, catalog, select, a, b, thirds)?;
    let tm = p.time_map();
    let mut data = heard.to_json();
    data["target"] = target;
    data["range"] = json!({
        "from": tm.format(a),
        "to": tm.format(b),
        "from_tick": a,
        "to_tick": b,
    });
    let s = &heard.whole;
    let db = |x: Option<f64>, unit: &str| x.map_or("—".to_string(), |v| format!("{v:.1} {unit}"));
    let message = format!(
        "{}〜{}（{:.2} 秒）: peak {} / rms {} / {} / {} / 相関 {}",
        tm.format(a),
        tm.format(b),
        heard.frames as f64 / f64::from(heard.rate),
        db(s.peak_db, "dBFS"),
        db(s.rms_db, "dBFS"),
        db(s.lufs, "LUFS"),
        db(heard.true_peak_db, "dBTP"),
        s.correlation.map_or("—".to_string(), |c| format!("{c:.2}")),
    );
    Ok(Output::with(message, data))
}

/// The levels of a stretch of sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Levels {
    pub peak_db: Option<f64>,
    pub rms_db: Option<f64>,
    /// Integrated loudness; `None` when too short (under 400 ms) or too quiet to gate.
    pub lufs: Option<f64>,
    /// Of left and right: 1 the same, 0 unrelated, -1 opposite.
    pub correlation: Option<f64>,
    /// The side's share of the power: 0 mono, 0.5 unrelated (or one side only), 1 opposite.
    pub width: Option<f64>,
    pub mid_db: Option<f64>,
    pub side_db: Option<f64>,
    /// Left over right; positive when the left is louder.
    pub balance_db: Option<f64>,
}

impl Levels {
    fn of(left: &[f32], right: &[f32], rate: u32) -> Levels {
        let (mut peak, mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        for (l, r) in left.iter().zip(right) {
            let (l, r) = (f64::from(*l), f64::from(*r));
            peak = peak.max(l.abs()).max(r.abs());
            ll += l * l;
            rr += r * r;
            lr += l * r;
        }
        let n = left.len().max(1) as f64;
        let (mid, side) = ((ll + 2.0 * lr + rr) / 4.0, (ll - 2.0 * lr + rr) / 4.0);
        let side = side.max(0.0);
        Levels {
            peak_db: amp_db(peak),
            rms_db: power_db((ll + rr) / (2.0 * n)),
            lufs: integrated(left, right, rate).0,
            correlation: (ll > 0.0 && rr > 0.0).then(|| round(lr / (ll * rr).sqrt(), 1000.0)),
            width: (mid + side > 0.0).then(|| round(side / (mid + side), 1000.0)),
            mid_db: power_db(mid / n),
            side_db: power_db(side / n),
            balance_db: (ll > 0.0 && rr > 0.0).then(|| round(10.0 * (ll / rr).log10(), 100.0)),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "peak_db": self.peak_db,
            "rms_db": self.rms_db,
            "lufs": self.lufs,
            "correlation": self.correlation,
            "width": self.width,
            "mid_db": self.mid_db,
            "side_db": self.side_db,
            "balance_db": self.balance_db,
        })
    }
}

/// One bar of the stretch (the first and the last may be cut by the range).
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub bar: u32,
    pub from_tick: i64,
    pub to_tick: i64,
    pub levels: Levels,
}

/// One band of the spectrum.
#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    /// The nominal centre.
    pub hz: f64,
    pub from_hz: f64,
    pub to_hz: f64,
    /// The power in the band, dBFS; `None` when there is none.
    pub db: Option<f64>,
}

/// What [`listen`] heard.
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub rate: u32,
    pub frames: u64,
    pub whole: Levels,
    pub true_peak_db: Option<f64>,
    /// Loudness range, LU (EBU Tech 3342); `None` for a stretch too short to have one.
    pub lra: Option<f64>,
    pub bars: Vec<Bar>,
    pub bands: Vec<Band>,
    /// Third-octave bands, or octave bands.
    pub thirds: bool,
    /// The spectrum's centre of gravity, Hz, from 20 Hz up.
    pub centroid_hz: Option<f64>,
    pub warnings: Vec<String>,
}

impl Heard {
    pub fn to_json(&self) -> Value {
        let mut whole = self.whole.to_json();
        whole["true_peak_db"] = json!(self.true_peak_db);
        whole["lra"] = json!(self.lra);
        json!({
            "sample_rate": self.rate,
            "frames": self.frames,
            "seconds": round(self.frames as f64 / f64::from(self.rate), 1000.0),
            "levels": whole,
            "bars": self.bars.iter().map(|b| {
                let mut v = b.levels.to_json();
                v["bar"] = json!(b.bar);
                v["from_tick"] = json!(b.from_tick);
                v["to_tick"] = json!(b.to_tick);
                v
            }).collect::<Vec<_>>(),
            "spectrum": {
                "bands": if self.thirds { "third" } else { "octave" },
                "centroid_hz": self.centroid_hz,
                "items": self.bands.iter().map(|b| json!({
                    "hz": b.hz,
                    "from_hz": b.from_hz,
                    "to_hz": b.to_hz,
                    "db": b.db,
                })).collect::<Vec<_>>(),
            },
            "warnings": self.warnings,
        })
    }
}

/// Renders ticks `[a, b)` of `select` at the song's rate and measures them.
pub fn listen(
    p: &Project,
    dir: Option<&Path>,
    catalog: Option<Arc<Catalog>>,
    select: Select,
    a: i64,
    b: i64,
    thirds: bool,
) -> CmdResult<Heard> {
    let rate = p.settings.sample_rate;
    let r = f64::from(rate);
    let tmap = TempoMap::from_project(p);
    let (from, to) = (tmap.frame_at(a, r), tmap.frame_at(b, r));
    if to <= from {
        return Err(CmdError::invalid("解析する範囲の長さが 0 です"));
    }
    let mut builder = Builder::new(rate, dir.map(Path::to_path_buf));
    builder.set_catalog(catalog);
    let out = render(&mut builder, p, select, from, to, 0).map_err(|e| CmdError::conflict(e.0))?;
    Ok(analyse(p, &out, rate, a, b, thirds))
}

/// Measures a render of ticks `[a, b)` of `p`.
pub fn analyse(
    p: &Project,
    out: &export::Rendered,
    rate: u32,
    a: i64,
    b: i64,
    thirds: bool,
) -> Heard {
    let r = f64::from(rate);
    let tmap = TempoMap::from_project(p);
    let tm = p.time_map();
    let base = tmap.frame_at(a, r);
    let len = out.left.len();
    let frame = |tick: i64| ((tmap.frame_at(tick, r) - base) as usize).min(len);
    let mut bars = Vec::new();
    let mut bar = tm.to_bbt(a).0;
    loop {
        let (start, end) = (tm.bar_start(bar).max(a), tm.bar_start(bar + 1).min(b));
        if start >= b {
            break;
        }
        let (f0, f1) = (frame(start), frame(end));
        bars.push(Bar {
            bar,
            from_tick: start,
            to_tick: end,
            levels: Levels::of(&out.left[f0..f1], &out.right[f0..f1], rate),
        });
        bar += 1;
    }
    let (_, true_peak_db, lra) = integrated(&out.left, &out.right, rate);
    let (bands, centroid_hz) = spectrum(&out.left, &out.right, rate, thirds);
    Heard {
        rate,
        frames: len as u64,
        whole: Levels::of(&out.left, &out.right, rate),
        true_peak_db,
        lra,
        bars,
        bands,
        thirds,
        centroid_hz,
        warnings: out.warnings.clone(),
    }
}

fn round(x: f64, by: f64) -> f64 {
    (x * by).round() / by
}

fn power_db(power: f64) -> Option<f64> {
    (power > 0.0).then(|| round(10.0 * power.log10(), 100.0))
}

fn amp_db(amp: f64) -> Option<f64> {
    (amp > 0.0).then(|| round(20.0 * amp.log10(), 100.0))
}

/// Integrated loudness, true peak and loudness range (EBU R 128).
fn integrated(left: &[f32], right: &[f32], rate: u32) -> (Option<f64>, Option<f64>, Option<f64>) {
    use ebur128::{EbuR128, Mode};
    let Ok(mut m) = EbuR128::new(2, rate, Mode::I | Mode::LRA | Mode::TRUE_PEAK) else {
        return (None, None, None);
    };
    if m.add_frames_planar_f32(&[left, right]).is_err() {
        return (None, None, None);
    }
    let lufs = m.loudness_global().ok().filter(|l| l.is_finite());
    let tp = (0..2)
        .filter_map(|c| m.true_peak(c).ok())
        .fold(0.0f64, f64::max);
    let lra = m.loudness_range().ok().filter(|l| l.is_finite());
    (
        lufs.map(|l| round(l, 100.0)),
        amp_db(tp),
        lra.filter(|_| lufs.is_some()).map(|l| round(l, 100.0)),
    )
}

/// The power of both channels in each band (Welch: Hann windows overlapping by half, the
/// powers averaged), and the centroid.
fn spectrum(left: &[f32], right: &[f32], rate: u32, thirds: bool) -> (Vec<Band>, Option<f64>) {
    let r = f64::from(rate);
    // About 3 Hz a bin, so the lowest third-octave band has bins of its own.
    let n = ((rate / 3) as usize).next_power_of_two();
    let len = left.len();
    let mut starts: Vec<usize> = (0..)
        .map(|i| i * n / 2)
        .take_while(|s| s + n <= len)
        .collect();
    if starts.last().map_or(len > 0, |s| s + n < len) {
        starts.push(len.saturating_sub(n));
    }
    let twiddles: Vec<(f64, f64)> = (0..n / 2)
        .map(|k| {
            let a = -2.0 * PI * k as f64 / n as f64;
            (a.cos(), a.sin())
        })
        .collect();
    // The mean power of each bin, one-sided, over both channels.
    let mut power = vec![0.0f64; n / 2];
    let (mut re, mut im) = (vec![0.0f64; n], vec![0.0f64; n]);
    for &s in &starts {
        let w_len = n.min(len - s);
        let mut w2 = 0.0;
        re.iter_mut().for_each(|x| *x = 0.0);
        im.iter_mut().for_each(|x| *x = 0.0);
        for i in 0..w_len {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / w_len as f64).cos();
            w2 += w * w;
            // Left and right in one transform: z = l + i r.
            re[i] = w * f64::from(left[s + i]);
            im[i] = w * f64::from(right[s + i]);
        }
        if w2 == 0.0 {
            continue;
        }
        fft(&mut re, &mut im, &twiddles);
        for (k, p) in power.iter_mut().enumerate().skip(1) {
            let j = n - k;
            let both = re[k] * re[k] + im[k] * im[k] + re[j] * re[j] + im[j] * im[j];
            *p += both / (2.0 * n as f64 * w2);
        }
    }
    let frames = starts.len().max(1) as f64;
    let bin_hz = r / n as f64;
    let (mut sum, mut moment) = (0.0, 0.0);
    for (k, p) in power.iter().enumerate() {
        let hz = k as f64 * bin_hz;
        if hz >= 20.0 {
            sum += p;
            moment += p * hz;
        }
    }
    let centroid = (sum > 0.0).then(|| round(moment / sum, 10.0));
    let half = if thirds { 1.0 / 6.0 } else { 0.5 };
    let bands = THIRDS
        .iter()
        .enumerate()
        .filter(|(i, _)| thirds || i % 3 == 1)
        .filter_map(|(i, &hz)| {
            let exact = 1000.0 * 2f64.powf((i as i32 - KHZ) as f64 / 3.0);
            let (lo, hi) = (
                exact * 2f64.powf(-half),
                (exact * 2f64.powf(half)).min(r / 2.0),
            );
            (lo < r / 2.0).then(|| {
                let in_band: f64 = power
                    .iter()
                    .enumerate()
                    .filter(|(k, _)| (lo..hi).contains(&(*k as f64 * bin_hz)))
                    .map(|(_, p)| p)
                    .sum();
                Band {
                    hz,
                    from_hz: round(lo, 10.0),
                    to_hz: round(hi, 10.0),
                    db: power_db(in_band / frames),
                }
            })
        })
        .collect();
    (bands, centroid)
}

/// An in-place radix-2 FFT; `re.len()` is a power of two and `twiddles` holds e^(-2πik/n) for
/// k below n/2.
fn fft(re: &mut [f64], im: &mut [f64], twiddles: &[(f64, f64)]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut size = 2;
    while size <= n {
        let step = n / size;
        for start in (0..n).step_by(size) {
            for k in 0..size / 2 {
                let (wr, wi) = twiddles[k * step];
                let (a, b) = (start + k, start + k + size / 2);
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        size <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f64, amp: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (amp * (2.0 * PI * hz * i as f64 / f64::from(rate)).sin()) as f32)
            .collect()
    }

    #[test]
    fn the_fft_matches_a_plain_dft() {
        let n = 64;
        let twiddles: Vec<(f64, f64)> = (0..n / 2)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / n as f64;
                (a.cos(), a.sin())
            })
            .collect();
        let x: Vec<f64> = (0..n).map(|i| ((i * 7 % 13) as f64 - 6.0) / 6.0).collect();
        let y: Vec<f64> = (0..n).map(|i| ((i * 5 % 11) as f64 - 5.0) / 5.0).collect();
        let (mut re, mut im) = (x.clone(), y.clone());
        fft(&mut re, &mut im, &twiddles);
        for k in 0..n {
            let (mut sr, mut si) = (0.0, 0.0);
            for t in 0..n {
                let a = -2.0 * PI * (k * t) as f64 / n as f64;
                sr += x[t] * a.cos() - y[t] * a.sin();
                si += x[t] * a.sin() + y[t] * a.cos();
            }
            assert!(
                (re[k] - sr).abs() < 1e-9 && (im[k] - si).abs() < 1e-9,
                "bin {k}"
            );
        }
    }

    #[test]
    fn a_sine_lands_in_its_band_and_the_bands_add_up_to_the_rms() {
        let rate = 48_000;
        let x = sine(1000.0, 0.5, rate, rate as usize * 2);
        let (bands, centroid) = spectrum(&x, &x, rate, false);
        assert_eq!(bands.len(), 10);
        let k = bands.iter().find(|b| b.hz == 1000.0).unwrap();
        // 0.5 peak: -9.03 dBFS RMS.
        let want = 10.0 * (0.125f64).log10();
        assert!((k.db.unwrap() - want).abs() < 0.1, "{k:?}");
        for b in bands.iter().filter(|b| b.hz != 1000.0) {
            assert!(b.db.is_none_or(|d| d < want - 40.0), "{b:?}");
        }
        assert!((centroid.unwrap() - 1000.0).abs() < 5.0, "{centroid:?}");
        let thirds = spectrum(&x, &x, rate, true).0;
        assert_eq!(thirds.len(), 30);
        let top = thirds
            .iter()
            .max_by(|a, b| a.db.unwrap_or(-999.0).total_cmp(&b.db.unwrap_or(-999.0)))
            .unwrap();
        assert_eq!(top.hz, 1000.0);
    }

    #[test]
    fn width_and_correlation_tell_mono_from_wide() {
        let rate = 48_000;
        let x = sine(440.0, 0.5, rate, rate as usize);
        let neg: Vec<f32> = x.iter().map(|s| -s).collect();
        let quiet: Vec<f32> = x.iter().map(|s| s * 0.5).collect();
        let zero = vec![0.0f32; x.len()];
        let mono = Levels::of(&x, &x, rate);
        assert_eq!((mono.correlation, mono.width), (Some(1.0), Some(0.0)));
        assert_eq!(mono.side_db, None);
        let opposite = Levels::of(&x, &neg, rate);
        assert_eq!(
            (opposite.correlation, opposite.width),
            (Some(-1.0), Some(1.0))
        );
        let left = Levels::of(&x, &zero, rate);
        assert_eq!((left.correlation, left.width), (None, Some(0.5)));
        let lean = Levels::of(&x, &quiet, rate);
        assert!((lean.balance_db.unwrap() - 6.02).abs() < 0.01, "{lean:?}");
        let silent = Levels::of(&zero, &zero, rate);
        assert_eq!(
            (silent.peak_db, silent.rms_db, silent.lufs, silent.width),
            (None, None, None, None)
        );
    }
}
