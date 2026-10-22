//! `export.midi` (EXP-026): the notes of instrument tracks as standard MIDI files, the way out
//! for a song's notes now that it has no score.
//!
//! One file (format 1) holds a conductor track with the song's name, tempo and time signatures,
//! then a track of each instrument track, named after it. Each track can also go to a file of
//! its own. The notes and controllers are the ones the song plays: muted clips and notes are
//! left out, and a note ends at the end of its clip. Notes that start in the range are written,
//! from the range's start, and end at its end at the latest.
//!
//! The record keeps what each file was made from ([`base`]), so the export panel tells which
//! files the song has moved away from, as it does for the mix and the stems
//! ([`crate::export_stale`]). A file is stale when the notes, controllers or name of a track in
//! it change, or the tempo, time signatures or the song's name.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, CtrlKind, CtrlShape, Project, Track, TrackKind};
use xirius_core::tempo::TempoMap;
use xirius_core::{Args, CmdError, CmdResult, Output};

use crate::export::{self, name_from, names_track, numbered, resolve_range};
use crate::smf::build::vlq;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["export.midi"];

/// The folder under the song's folder the files go to.
pub const FOLDER: &str = "MIDI";

/// The file name patterns used when `--name` is left out and none is remembered.
pub const ONE_NAME: &str = "{曲}";
pub const EACH_NAME: &str = "{曲}_{トラック}";

/// What `{トラック}` stands for in the name of the one file.
const ALL_TRACKS: &str = "MIDI";

/// Where the remembered pattern of each kind is kept.
pub fn name_key(each: bool) -> &'static str {
    if each { "midi_each_name" } else { "midi_name" }
}

/// The pattern used last of the kind, else the first one.
pub fn pattern_of(remembered: &Map<String, Value>, each: bool) -> String {
    remembered
        .get(name_key(each))
        .and_then(Value::as_str)
        .unwrap_or(if each { EACH_NAME } else { ONE_NAME })
        .to_string()
}

/// The instrument tracks, top to bottom in the track column.
pub fn instrument_tracks(p: &Project) -> Vec<TrackId> {
    fn walk(p: &Project, kids: &[Node], out: &mut Vec<TrackId>) {
        for n in kids {
            match n {
                Node::Track(t) => {
                    if p.tracks
                        .get(t)
                        .is_some_and(|t| t.kind == TrackKind::Instrument)
                    {
                        out.push(*t);
                    }
                }
                Node::Folder(f) => {
                    if let Some(f) = p.folders.get(f) {
                        walk(p, &f.children, out);
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(p, &p.root, &mut out);
    out
}

/// An export worked out from its arguments, before anything is written.
#[derive(Debug, Clone)]
pub struct Plan {
    pub each: bool,
    /// The file, or the folder of the files of each track.
    pub target: PathBuf,
    pub span: (i64, i64),
    pub pattern: String,
    /// Each file and the tracks it holds, in the column's order.
    pub files: Vec<(PathBuf, Vec<TrackId>)>,
    /// The arguments that make the same export again.
    pub stored: Map<String, Value>,
}

impl Plan {
    /// The files, with the key [`base`] keeps each under: the one file is the mix's, a file of
    /// each track its track's.
    pub fn keyed(&self) -> Vec<(String, Vec<TrackId>)> {
        self.files
            .iter()
            .map(|(_, ids)| match (self.each, ids.first()) {
                (true, Some(id)) => (id.to_string(), ids.clone()),
                _ => ("mix".to_string(), ids.clone()),
            })
            .collect()
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|(f, _)| f.clone()).collect()
    }
}

fn track_of(p: &Project, id: TrackId) -> CmdResult<&Track> {
    let t = p.track(id)?;
    if t.kind != TrackKind::Instrument {
        return Err(CmdError::invalid(format!(
            "「{}」はインストゥルメントトラックではありません（MIDI ファイルに書き出せるのはインストゥルメントトラックだけです）",
            t.shown_name()
        )));
    }
    Ok(t)
}

/// Works out an export. `remembered` holds the patterns used last; `busy` the files other
/// exports of this process are writing.
pub fn plan(
    p: &Project,
    dir: Option<&Path>,
    args: &Args,
    remembered: &Map<String, Value>,
    busy: &[PathBuf],
) -> CmdResult<Plan> {
    let column = instrument_tracks(p);
    let mut clips: Vec<(TrackId, &Clip)> = Vec::new();
    for id in args.ids::<ClipId>("clips")? {
        let (t, c) = p.clip(id)?;
        if !c.is_midi() || t.kind != TrackKind::Instrument {
            return Err(CmdError::invalid(format!(
                "「{}」は MIDI のクリップではありません",
                c.name
            )));
        }
        clips.push((t.id, c));
    }
    let mut tracks = args.ids::<TrackId>("tracks")?;
    for id in &tracks {
        track_of(p, *id)?;
    }
    if tracks.is_empty() {
        tracks = if clips.is_empty() {
            column.clone()
        } else {
            clips.iter().map(|(t, _)| *t).collect()
        };
    }
    tracks.sort_by_key(|t| column.iter().position(|c| c == t));
    tracks.dedup();
    if tracks.is_empty() {
        return Err(CmdError::invalid(
            "書き出せるインストゥルメントトラックがありません",
        ));
    }

    // The range: the one asked for, else the clips', else the song.
    let mut ranged = args.clone();
    let has = |k: &str| args.0.get(k).is_some_and(|v| !v.is_null());
    if !has("range") {
        if has("from") && has("to") {
            ranged.0.insert("range".into(), json!("custom"));
        } else if !clips.is_empty() {
            let from = clips.iter().map(|(_, c)| c.start).min().unwrap_or(0);
            let to = clips.iter().map(|(_, c)| c.end()).max().unwrap_or(0);
            ranged.0.insert("range".into(), json!("custom"));
            ranged.0.insert("from".into(), json!(from));
            ranged.0.insert("to".into(), json!(to));
        }
    }
    let o = crate::commands::options(p, dir, &ranged)?;
    let span = resolve_range(p, &o.range).map_err(|e| CmdError::conflict(e.0))?;
    let fixed = matches!(o.range, export::RangeSpec::Ticks(..));

    let each = args.flag("each");
    let pattern = args
        .opt_str("name")
        .map(str::to_string)
        .unwrap_or_else(|| pattern_of(remembered, each));
    if pattern.trim().is_empty() {
        return Err(CmdError::invalid("--name を入力してください"));
    }
    if each && !names_track(&pattern) {
        return Err(CmdError::invalid(
            "トラックごとに書き出すときは、ファイル名に {トラック} を入れてください",
        ));
    }
    let keep = args.flag("numbered");
    let out = args.opt_str("out");
    let (target, files) = if each {
        let folder = match out {
            Some(s) => PathBuf::from(s),
            None => default_folder(dir),
        };
        let folder = crate::export_paths::absolute(&folder)?;
        let mut taken = busy.to_vec();
        let mut files = Vec::new();
        for id in &tracks {
            let name = name_from(&pattern, &p.name, p.tracks[id].shown_name());
            let path = numbered(&folder.join(format!("{name}.mid")), &taken, keep);
            taken.push(path.clone());
            files.push((path, vec![*id]));
        }
        (folder, files)
    } else {
        let file = match out {
            Some(s) => match Path::new(s).extension().map(|e| e.to_string_lossy()) {
                Some(e) if e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi") => {
                    PathBuf::from(s)
                }
                Some(e) => {
                    return Err(CmdError::invalid(format!(
                        ".{e} のファイルには書き出せません（.mid を指定してください）"
                    )));
                }
                None => PathBuf::from(format!("{s}.mid")),
            },
            None => default_folder(dir)
                .join(format!("{}.mid", name_from(&pattern, &p.name, ALL_TRACKS))),
        };
        let file = numbered(&crate::export_paths::absolute(&file)?, busy, keep);
        (file.clone(), vec![(file, tracks.clone())])
    };

    let mut stored = args.0.clone();
    for k in ["background", "dry_run", "clips"] {
        stored.remove(k);
    }
    if fixed || !clips.is_empty() {
        stored.insert("range".into(), json!("custom"));
        stored.insert("from".into(), json!(span.0));
        stored.insert("to".into(), json!(span.1));
    }
    stored.insert("name".into(), json!(pattern));
    stored.insert("each".into(), json!(each));
    let ids: Vec<String> = tracks.iter().map(ToString::to_string).collect();
    stored.insert("tracks".into(), json!(ids));
    stored.insert("out".into(), json!(target.display().to_string()));
    Ok(Plan {
        each,
        target,
        span,
        pattern,
        files,
        stored,
    })
}

fn default_folder(dir: Option<&Path>) -> PathBuf {
    dir.map_or_else(|| PathBuf::from(FOLDER), |d| d.join(FOLDER))
}

/// What a dry run says.
pub fn preview(plan: &Plan) -> Output {
    let (a, b) = plan.span;
    let files: Vec<String> = plan
        .files
        .iter()
        .map(|(f, _)| f.display().to_string())
        .collect();
    Output::with(
        format!(
            "{} に MIDI ファイルを書き出します（{a}〜{b} tick）",
            plan.target.display()
        ),
        json!({
            "path": plan.target.display().to_string(),
            "files": files,
            "from": a,
            "to": b,
            "each": plan.each,
            "name": plan.pattern,
        }),
    )
}

/// Writes the files of `plan`.
pub fn write(p: &Project, plan: &Plan) -> CmdResult<(String, Value)> {
    let mut files = Vec::new();
    let (mut all_notes, mut all_tracks) = (0usize, 0usize);
    for (path, ids) in &plan.files {
        let (bytes, notes) = file_bytes(p, plan.span, ids)?;
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| {
                CmdError::conflict(format!("{} を作成できません: {e}", d.display()))
            })?;
        }
        std::fs::write(path, bytes)
            .map_err(|e| CmdError::conflict(format!("{} に書き出せません: {e}", path.display())))?;
        let name = match (plan.each, ids.first()) {
            (true, Some(id)) => p.tracks[id].shown_name().to_string(),
            _ => path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        all_notes += notes;
        all_tracks += ids.len();
        files.push(json!({
            "name": name,
            "path": path.display().to_string(),
            "strip": if plan.each { ids.first().map(ToString::to_string) } else { None },
            "tracks": ids.len(),
            "notes": notes,
        }));
    }
    let message = if plan.each {
        format!(
            "{} に MIDI ファイルを {} 個書き出しました（{all_notes} ノート）",
            plan.target.display(),
            files.len()
        )
    } else {
        format!(
            "{} に書き出しました（{all_tracks} トラック・{all_notes} ノート）",
            plan.target.display()
        )
    };
    Ok((
        message,
        json!({
            "path": plan.target.display().to_string(),
            "from": plan.span.0,
            "to": plan.span.1,
            "each": plan.each,
            "files": files,
            "tracks": all_tracks,
            "notes": all_notes,
        }),
    ))
}

/// The order of events at the same tick: a note ends, then a controller moves, then a note
/// starts.
const OFF: u8 = 0;
const CTRL: u8 = 1;
const ON: u8 = 2;

/// A track chunk of events at ticks from its start, closed at `end`.
fn chunk(mut events: Vec<(i64, u8, Vec<u8>)>, end: i64) -> Vec<u8> {
    events.sort_by_key(|e| (e.0, e.1));
    let mut body = Vec::new();
    let mut at = 0i64;
    for (tick, _, bytes) in &events {
        body.extend(vlq(u32::try_from(tick - at).unwrap_or(0)));
        body.extend(bytes);
        at = *tick;
    }
    body.extend(vlq(u32::try_from(end - at).unwrap_or(0)));
    body.extend([0xff, 0x2f, 0x00]);
    let mut out = b"MTrk".to_vec();
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    out
}

fn meta(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut v = vec![0xff, kind];
    v.extend(vlq(data.len() as u32));
    v.extend_from_slice(data);
    v
}

/// How often a tempo ramp or a straight-line controller is written: a sixteenth note.
fn step(p: &Project) -> i64 {
    (i64::from(p.ppq) / 4).max(1)
}

/// The conductor track: the song's name, then its tempo and time signatures from the range's
/// start.
fn conductor(p: &Project, (a, b): (i64, i64)) -> Vec<u8> {
    let mut ev = vec![(0, CTRL, meta(0x03, p.name.as_bytes()))];
    let tm = p.time_map();
    let mut meters: Vec<(i64, u32, u32)> = p
        .timesig
        .iter()
        .map(|t| (tm.bar_start(t.bar), t.num, t.den))
        .collect();
    meters.sort_by_key(|m| m.0);
    let first = meters
        .iter()
        .rev()
        .find(|m| m.0 <= a)
        .copied()
        .unwrap_or((a, 4, 4));
    for (tick, num, den) in std::iter::once((a, first.1, first.2))
        .chain(meters.iter().copied().filter(|m| m.0 > a && m.0 < b))
    {
        let power = den.max(1).trailing_zeros() as u8;
        ev.push((
            tick - a,
            CTRL,
            meta(0x58, &[num.clamp(1, 255) as u8, power, 24, 8]),
        ));
    }
    let map = TempoMap::from_project(p);
    let mut at: Vec<i64> = vec![a];
    let mut points = p.tempo.clone();
    points.sort_by_key(|t| t.tick);
    for (i, t) in points.iter().enumerate() {
        at.push(t.tick);
        if let Some(next) = points.get(i + 1).filter(|_| t.ramp) {
            at.extend((t.tick..next.tick).step_by(step(p) as usize));
        }
    }
    at.retain(|t| *t >= a && *t < b);
    at.sort();
    at.dedup();
    let mut last = 0u32;
    for tick in at {
        let micros = (60_000_000.0 / map.bpm_at(tick))
            .round()
            .clamp(1.0, 16_777_215.0) as u32;
        if micros != last {
            ev.push((tick - a, CTRL, meta(0x51, &micros.to_be_bytes()[1..])));
            last = micros;
        }
    }
    chunk(ev, b - a)
}

/// The bytes of a controller's value.
fn ctrl_bytes(kind: CtrlKind, value: i32) -> Vec<u8> {
    match kind {
        CtrlKind::Cc(n) => vec![0xb0, n & 0x7f, value.clamp(0, 127) as u8],
        CtrlKind::PitchBend => {
            let v = (value.clamp(-8192, 8191) + 8192) as u16;
            vec![0xe0, (v & 0x7f) as u8, (v >> 7) as u8]
        }
        CtrlKind::Aftertouch => vec![0xd0, value.clamp(0, 127) as u8],
    }
}

/// The values a clip's controller takes, at song ticks, ending at rest at the clip's end.
fn ctrl_values(p: &Project, c: &Clip, lane: &xirius_core::model::CtrlLane) -> Vec<(i64, i32)> {
    let pts: Vec<_> = lane.points.iter().filter(|q| q.tick < c.length).collect();
    let mut out: Vec<(i64, i32)> = Vec::new();
    for (i, q) in pts.iter().enumerate() {
        out.push((c.start + q.tick, q.value));
        if let (CtrlShape::Linear, Some(n)) = (q.shape, pts.get(i + 1)) {
            let span = (n.tick - q.tick).max(1);
            let mut t = q.tick + step(p);
            while t < n.tick {
                let f = (t - q.tick) as f64 / span as f64;
                let v = (f64::from(q.value) + f64::from(n.value - q.value) * f).round() as i32;
                out.push((c.start + t, v));
                t += step(p);
            }
        }
    }
    if !out.is_empty() {
        out.push((c.end(), lane.kind.rest()));
    }
    out
}

/// A track's chunk: its name, then the notes and controllers of its clips in the range.
/// Returns the chunk and the number of notes.
fn track_chunk(p: &Project, t: &Track, (a, b): (i64, i64)) -> (Vec<u8>, usize) {
    let mut ev = vec![(0, CTRL, meta(0x03, t.shown_name().as_bytes()))];
    let mut notes = 0;
    for c in t.clips.iter().filter(|c| c.is_midi() && !c.muted) {
        for n in c.notes() {
            if n.muted || n.start >= c.length || n.start < 0 {
                continue;
            }
            let on = c.start + n.start;
            if on < a || on >= b {
                continue;
            }
            let off = (on + n.length).min(c.end()).min(b).max(on);
            // What the clip plays: its transpose and the track's are added here, since a MIDI
            // file has nowhere to keep them (UI-311/UI-312). The track's velocity is applied to
            // the note's (UI-312).
            let by = c.transpose().saturating_add(t.transpose);
            let Some(pitch) = crate::build::playable(by, n.pitch) else {
                continue;
            };
            let velocity = xirius_core::model::track_velocity(n.velocity, t.velocity);
            ev.push((on - a, ON, vec![0x90, pitch, velocity]));
            ev.push((off - a, OFF, vec![0x80, pitch, 0x40]));
            notes += 1;
        }
        for lane in &c.controllers {
            let values = ctrl_values(p, c, lane);
            // The value at the range's start, when the clip is playing then.
            if c.start < a
                && c.end() > a
                && let Some(v) = values.iter().rev().find(|v| v.0 < a)
            {
                ev.push((0, CTRL, ctrl_bytes(lane.kind, v.1)));
            }
            let mut last = None;
            for (tick, v) in values.into_iter().filter(|v| v.0 >= a && v.0 < b) {
                if last != Some((tick, v)) {
                    ev.push((tick - a, CTRL, ctrl_bytes(lane.kind, v)));
                }
                last = Some((tick, v));
            }
        }
    }
    (chunk(ev, b - a), notes)
}

/// A format 1 file of the conductor track and a track of each of `ids`, with the number of
/// notes.
pub fn file_bytes(p: &Project, span: (i64, i64), ids: &[TrackId]) -> CmdResult<(Vec<u8>, usize)> {
    let division = u16::try_from(p.ppq)
        .ok()
        .filter(|d| *d > 0 && *d <= 0x7fff)
        .ok_or_else(|| {
            CmdError::conflict(format!(
                "1 拍の分解能 {} は MIDI ファイルに書けません（32767 まで）",
                p.ppq
            ))
        })?;
    let mut tracks = vec![conductor(p, span)];
    let mut notes = 0;
    for id in ids {
        let (bytes, n) = track_chunk(p, &p.tracks[id], span);
        tracks.push(bytes);
        notes += n;
    }
    Ok((crate::smf::build::file(1, division, &tracks), notes))
}

/// What a track's MIDI is, as a fingerprint: its name and the notes and controllers of its
/// MIDI clips where they are.
pub fn track_print(t: &Track) -> String {
    let clips: Vec<Value> = t
        .clips
        .iter()
        .filter(|c| c.is_midi())
        .map(|c| {
            let notes: Vec<Value> = c
                .notes()
                .iter()
                .map(|n| json!([n.start, n.length, n.pitch, n.velocity, n.muted]))
                .collect();
            json!({
                "start": c.start,
                "length": c.length,
                "muted": c.muted,
                "transpose": c.transpose(),
                "notes": notes,
                "controllers": serde_json::to_value(&c.controllers).unwrap_or(Value::Null),
            })
        })
        .collect();
    crate::export_stale::hash(&json!({
        "name": t.shown_name(),
        "transpose": t.transpose,
        "velocity": t.velocity,
        "clips": clips
    }))
}

/// What every track of a MIDI file shares: the song's name, tempo and time signatures.
pub fn song_print(p: &Project) -> String {
    crate::export_stale::hash(&json!({
        "name": p.name,
        "ppq": p.ppq,
        "tempo": p.tempo,
        "timesig": p.timesig,
    }))
}

/// What an export keeps to tell later which of its files are stale.
pub fn base(p: &Project, span: (i64, i64), keyed: &[(String, Vec<TrackId>)]) -> Value {
    let mut prints = Map::new();
    let mut names = Map::new();
    let mut files = Map::new();
    for (key, ids) in keyed {
        for id in ids {
            if let Some(t) = p.tracks.get(id) {
                prints.insert(id.to_string(), json!(track_print(t)));
                names.insert(id.to_string(), json!(t.shown_name()));
            }
        }
        let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
        files.insert(key.clone(), json!(ids));
    }
    json!({
        "kind": "midi",
        "span": [span.0, span.1],
        "song": song_print(p),
        "strips": prints,
        "names": names,
        "files": files,
    })
}

/// Whether a record's fingerprints are of MIDI files.
pub fn is_midi(base: &Map<String, Value>) -> bool {
    base.get("kind").and_then(Value::as_str) == Some("midi")
}
