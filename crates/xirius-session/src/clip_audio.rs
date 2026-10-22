//! `clip.reverse`, `clip.normalize` and `clip.tempo.detect` (EDT-016): they read each clip's
//! source (its length, its peak, its beat) and leave one `clip.reverse.apply` /
//! `clip.normalize.apply` / `clip.tempo.detect.apply` for the session to run, so one use is one
//! step of the history. The source is never rewritten.

use std::path::Path;

use serde_json::{Map, Value, json};
use xirius_core::id::ClipId;
use xirius_core::model::{AudioProcess, ClipBody, MAX_DB, MIN_DB, Project};
use xirius_core::tempo::TempoMap;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};

use crate::import::{Plan, disk_path};
use crate::{beat, decode};

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 3] = ["clip.reverse", "clip.normalize", "clip.tempo.detect"];

/// Below this the clip is taken as silent and its gain is left alone.
const SILENT: f32 = 1e-6;

pub fn run(cx: &ExtContext<'_>, name: &str, args: &Args, dry: bool) -> CmdResult<Plan> {
    let p = cx.project;
    let ids = args.require_ids::<ClipId>("クリップ")?;
    let clips = ids
        .iter()
        .map(|id| Audio::of(p, *id))
        .collect::<CmdResult<Vec<_>>>()?;
    let map = TempoMap::from_project(p);
    let (output, edit, objects) = match name {
        "clip.reverse" => reverse(cx, &map, args, &clips)?,
        "clip.tempo.detect" => detect(cx, &map, &clips)?,
        _ => normalize(cx, &map, args, &clips)?,
    };
    let mut a = Map::new();
    a.insert("clips".into(), Value::Array(objects));
    Ok(Plan {
        output,
        edits: if dry {
            Vec::new()
        } else {
            vec![(edit.into(), a)]
        },
    })
}

/// An audio clip as these commands see it.
struct Audio<'a> {
    id: ClipId,
    start: i64,
    length: i64,
    file: &'a str,
    offset: i64,
    gain_db: f64,
    process: AudioProcess,
}

impl<'a> Audio<'a> {
    fn of(p: &'a Project, id: ClipId) -> CmdResult<Audio<'a>> {
        let c = p.clip(id)?.1;
        match &c.body {
            ClipBody::Audio {
                file,
                offset,
                gain_db,
                process,
            } => Ok(Audio {
                id,
                start: c.start,
                length: c.length,
                file,
                offset: *offset,
                gain_db: *gain_db,
                process: *process,
            }),
            ClipBody::Midi { .. } => Err(CmdError::invalid(format!(
                "クリップ {id} は MIDI です。オーディオのクリップだけに使えます"
            ))),
        }
    }

    /// Output seconds per source second where the clip starts.
    fn ratio(&self, map: &TempoMap) -> f64 {
        self.process.ratio(map.bpm_at(self.start))
    }

    /// Where the clip's window starts and how long it is, in seconds of its played sound.
    fn window(&self, map: &TempoMap) -> (f64, f64) {
        let base = map.seconds_at(self.start);
        (
            map.seconds_at(self.start + self.offset) - base,
            map.seconds_at(self.start + self.length) - base,
        )
    }
}

fn reverse(
    cx: &ExtContext<'_>,
    map: &TempoMap,
    args: &Args,
    clips: &[Audio<'_>],
) -> CmdResult<(Output, &'static str, Vec<Value>)> {
    let on = match args.toggle()? {
        Some(v) => v,
        None => !clips.iter().all(|c| c.process.reverse),
    };
    let mut objects = Vec::new();
    for c in clips {
        let mut offset = c.offset;
        if c.process.reverse != on {
            // The same part of the source, read from the other end: the window's distance from
            // the end of the played sound becomes its distance from the start.
            let path = disk_path(c.file, cx.dir)?;
            let info = decode::probe(&path).map_err(CmdError::io)?;
            let played = info.seconds() * c.ratio(map);
            let (off, len) = c.window(map);
            let flipped = (played - off - len).max(0.0);
            let base = map.seconds_at(c.start);
            offset = (map.tick_at(base + flipped) - c.start).max(0);
        }
        objects.push(json!({"id": c.id.to_string(), "reverse": on, "offset": offset}));
    }
    let n = clips.len();
    let msg = if on {
        format!("クリップ {n} 件を逆再生にしました")
    } else {
        format!("クリップ {n} 件の逆再生を解除しました")
    };
    Ok((
        Output::with(msg, json!({"ids": ids(clips), "reverse": on})),
        "clip.reverse.apply",
        objects,
    ))
}

fn normalize(
    cx: &ExtContext<'_>,
    map: &TempoMap,
    args: &Args,
    clips: &[Audio<'_>],
) -> CmdResult<(Output, &'static str, Vec<Value>)> {
    let target = args.opt_f64("peak").unwrap_or(0.0);
    let mut objects = Vec::new();
    let mut changes = Vec::new();
    for c in clips {
        let path = disk_path(c.file, cx.dir)?;
        let peak = peak(&path, c, map)?;
        let gain = if peak < SILENT {
            c.gain_db
        } else {
            (target - 20.0 * f64::from(peak).log10()).clamp(MIN_DB, MAX_DB)
        };
        let gain = (gain * 100.0).round() / 100.0;
        changes.push(json!({"id": c.id.to_string(), "db": gain - c.gain_db}));
        objects.push(json!({"id": c.id.to_string(), "gain_db": gain}));
    }
    let msg = match changes.as_slice() {
        [one] => format!(
            "クリップをノーマライズしました（{:+.1} dB）",
            one["db"].as_f64().unwrap_or(0.0)
        ),
        _ => format!("クリップ {} 件をノーマライズしました", clips.len()),
    };
    Ok((
        Output::with(msg, json!({"ids": ids(clips), "changes": changes})),
        "clip.normalize.apply",
        objects,
    ))
}

fn detect(
    cx: &ExtContext<'_>,
    map: &TempoMap,
    clips: &[Audio<'_>],
) -> CmdResult<(Output, &'static str, Vec<Value>)> {
    let mut objects = Vec::new();
    for c in clips {
        let path = disk_path(c.file, cx.dir)?;
        let d = decode::decode(&path).map_err(CmdError::io)?;
        let (a, b) = source_window(&d, c, map);
        let mono: Vec<f32> = if d.mono {
            d.left[a..b].to_vec()
        } else {
            d.left[a..b]
                .iter()
                .zip(&d.right[a..b])
                .map(|(l, r)| 0.5 * (l + r))
                .collect()
        };
        let bpm = beat::detect(&mono, d.rate).ok_or_else(|| {
            CmdError::invalid(if clips.len() == 1 {
                "テンポを検出できません".to_string()
            } else {
                format!("クリップ {} のテンポを検出できません", c.id)
            })
        })?;
        objects.push(json!({"id": c.id.to_string(), "bpm": bpm}));
    }
    let msg = match objects.as_slice() {
        [one] => format!(
            "テンポを検出しました（{} BPM）",
            show_bpm(one["bpm"].as_f64().unwrap_or(0.0))
        ),
        _ => format!("クリップ {} 件のテンポを検出しました", clips.len()),
    };
    Ok((
        Output::with(msg, json!({"ids": ids(clips), "tempos": objects.clone()})),
        "clip.tempo.detect.apply",
        objects,
    ))
}

/// 128 as "128", 127.53 as "127.53".
fn show_bpm(bpm: f64) -> String {
    if (bpm - bpm.round()).abs() < 0.005 {
        format!("{}", bpm.round())
    } else {
        format!("{bpm:.2}")
    }
}

/// The frames of the source `c` plays.
fn source_window(d: &decode::Decoded, c: &Audio<'_>, map: &TempoMap) -> (usize, usize) {
    let frames = d.left.len();
    let rate = f64::from(d.rate);
    let ratio = c.ratio(map);
    let (off, len) = c.window(map);
    // Seconds of the played sound to seconds of the source.
    let (mut from, mut to) = (off / ratio, (off + len) / ratio);
    if c.process.reverse {
        let total = frames as f64 / rate;
        (from, to) = (total - to, total - from);
    }
    let at = |s: f64| ((s * rate).round().max(0.0) as usize).min(frames);
    let (a, b) = (at(from), at(to));
    (a, b.max(a))
}

/// The highest sample of the part of the source `c` plays (before its gain).
fn peak(path: &Path, c: &Audio<'_>, map: &TempoMap) -> CmdResult<f32> {
    let d = decode::decode(path).map_err(CmdError::io)?;
    let (a, b) = source_window(&d, c, map);
    let side = |x: &[f32]| {
        x.get(a..b)
            .unwrap_or(&[])
            .iter()
            .fold(0f32, |m, v| m.max(v.abs()))
    };
    let mut peak = side(&d.left);
    if !d.mono {
        peak = peak.max(side(&d.right));
    }
    Ok(peak)
}

fn ids(clips: &[Audio<'_>]) -> Vec<String> {
    clips.iter().map(|c| c.id.to_string()).collect()
}
