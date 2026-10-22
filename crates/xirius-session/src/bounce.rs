//! Bouncing (EDT-015, EXP-003): `clip.bounce`, `range.bounce` and `track.bounce` render with
//! the export's renderer, write 32-bit float WAVs into `Bounces/` under the song's folder (a
//! song not saved yet: a folder of its own under the temporary folder, which a save gathers
//! into the song's folder) and leave one `bounce.apply`, so a bounce is one step of the history.
//!
//! - A clip bounced in place is rendered alone, without the inserts, the fader, the pan, its
//!   gain and its fades: only its tempo, speed, pitch and reverse are baked in, and the clip
//!   keeps its gain and fades.
//! - A bounce to a new track goes through the inserts and stops before the fader (as a
//!   freeze, EXP-004): the new track takes the fader, the pan, the sends and the output of the
//!   track it comes from.
//! - A range is mixed into one track through each track's inserts, fader and pan, but not its
//!   sends nor the master. The clips that lie wholly inside the range are muted; those that
//!   only overlap it are left as they are.
//! - After the last note up to 10 seconds of tail are kept, cut where the sound has died away.
//! - [`prepare`] counts into a [`Progress`] and stops when it is cancelled, from the window or
//!   with `job.cancel` (EXP-047, [`xirius_core::running`]); when anything fails the files
//!   written so far are removed and the song is left as it was.
//!
//! `track.freeze` (EXP-004) renders a track the way `track.bounce` does, into `Freeze/`, and
//! leaves one `track.freeze.apply`: the track then plays that file instead of its clips,
//! instrument and inserts, while its fader, pan and sends still work.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Map, Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{ClipBody, Media, Project, Track, TrackKind};
use xirius_core::running;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};
use xirius_plugins::Catalog;

use crate::build::Builder;
use crate::export::{Progress, render_with, sanitize, write_wav};
use crate::graph::Select;
use crate::import::{Plan, song_path};
use crate::record::{column, unused};
use crate::tempo::TempoMap;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 4] = [
    "clip.bounce",
    "range.bounce",
    "track.bounce",
    "track.freeze",
];

/// The folder under the song's media folder the bounces are written to (BRW-004).
pub const FOLDER: &str = Media::BOUNCES;

/// The folder under the song's media folder the frozen tracks are written to (BRW-004).
pub const FREEZE_FOLDER: &str = Media::FREEZE;

/// The most tail kept after the last note, in seconds.
pub const TAIL: f64 = 10.0;

/// The message of a bounce stopped by `job.cancel` or the window's square (EXP-047).
pub const CANCELLED: &str = "バウンスをキャンセルしました";

/// The message of a freeze stopped the same way.
pub const CANCELLED_FREEZE: &str = "フリーズをキャンセルしました";

/// Below this the tail is taken to have died away (-80 dB).
const QUIET: f32 = 1e-4;

pub fn run(
    cx: &ExtContext<'_>,
    catalog: Option<Arc<Catalog>>,
    name: &str,
    args: &Args,
    dry: bool,
) -> CmdResult<Plan> {
    prepare(cx.project, cx.dir, catalog, name, args, dry, None)
}

/// Works out the bounce `name` asks for, renders it and writes the files (not when `dry`).
/// The plan's edit puts the result into the song; nothing else is changed.
///
/// `job` is the progress a window shows and the job's place in [`running`] (whose id the window
/// keeps); without it the bounce takes its own, so `job.cancel` stops it all the same.
pub fn prepare(
    p: &Project,
    dir: Option<&Path>,
    catalog: Option<Arc<Catalog>>,
    name: &str,
    args: &Args,
    dry: bool,
    job: Option<(&Progress, running::Ticket)>,
) -> CmdResult<Plan> {
    let work = match name {
        "clip.bounce" => clips(p, args)?,
        "range.bounce" => range(p, args)?,
        "track.bounce" => tracks(p, args)?,
        "track.freeze" => freeze(p, args)?,
        n => {
            return Err(CmdError::invalid(format!(
                "{n} はバウンスのコマンドではありません"
            )));
        }
    };
    let output = Output::with(work.message.clone(), json!({"dry_run": dry}));
    if dry {
        return Ok(Plan {
            output,
            edits: Vec::new(),
        });
    }
    let rate = p.settings.sample_rate;
    let mut builder = Builder::new(rate, dir.map(Path::to_path_buf));
    builder.set_catalog(catalog);
    let mut written = Vec::new();
    // `job.cancel` stops it from anywhere (EXP-047): a watcher passes its word to the progress.
    let own = Progress::default();
    let (progress, ticket) = match job {
        Some((progress, ticket)) => (progress, ticket),
        None => (&own, running::start(name, dir)),
    };
    let done = AtomicBool::new(false);
    let rendered = std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if ticket.cancelled() {
                    progress.cancel();
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        let r = render_all(p, dir, &mut builder, &work, Some(progress), &mut written);
        done.store(true, Ordering::Relaxed);
        r
    });
    // A word that came as the last block ended still stops it.
    if ticket.cancelled() {
        progress.cancel();
    }
    drop(ticket);
    let rendered = match rendered {
        _ if progress.cancelled() => Err(CmdError::io(if work.freeze {
            CANCELLED_FREEZE
        } else {
            CANCELLED
        })),
        r => r,
    };
    match rendered {
        Ok(edit) => {
            let data = json!({
                "files": written.iter().map(|f| song_path(f, dir)).collect::<Vec<_>>(),
            });
            let apply = if work.freeze {
                "track.freeze.apply"
            } else {
                "bounce.apply"
            };
            Ok(Plan {
                output: Output::with(work.message, data),
                edits: vec![(apply.into(), edit)],
            })
        }
        Err(e) => {
            for f in &written {
                let _ = std::fs::remove_file(f);
            }
            Err(e)
        }
    }
}

/// A track the bounce `name` adds, as a window shows it while the bounce runs (EXP-047).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    /// The track it goes below.
    pub after: TrackId,
    /// The track whose colour it takes.
    pub like: Option<TrackId>,
    pub name: String,
    /// Where its clips go, start and end in ticks (without the tail).
    pub spans: Vec<(i64, i64)>,
}

/// The tracks the bounce `name` adds, in order; none for a bounce in place or a freeze.
pub fn planned(p: &Project, name: &str, args: &Args) -> CmdResult<Vec<Planned>> {
    let work = match name {
        "clip.bounce" => clips(p, args)?,
        "range.bounce" => range(p, args)?,
        "track.bounce" => tracks(p, args)?,
        _ => return Ok(Vec::new()),
    };
    let mut out: Vec<Planned> = work
        .tracks
        .iter()
        .map(|t| Planned {
            after: t.after,
            like: t.like,
            name: t.name.clone(),
            spans: Vec::new(),
        })
        .collect();
    for (piece, dest) in &work.pieces {
        if let Dest::Track(k) = dest {
            out[*k].spans.push((piece.from, piece.to));
        }
    }
    Ok(out)
}

/// One sound to render.
struct Piece {
    /// The song as this piece is heard (tracks, faders and plug-ins set for it).
    song: Project,
    /// The tracks summed into the piece.
    select: Vec<Select>,
    from: i64,
    to: i64,
    /// The most tail after `to`, in seconds.
    tail: f64,
    /// The name the file and the clip take after.
    name: String,
}

/// Where a rendered piece goes.
enum Dest {
    /// Replaces the sound of this clip.
    Clip(ClipId),
    /// A clip on the new track at this index of [`Work::tracks`].
    Track(usize),
    /// What this track plays while it is frozen.
    Frozen(TrackId),
}

struct NewTrack {
    after: TrackId,
    like: Option<TrackId>,
    name: String,
}

struct Work {
    pieces: Vec<(Piece, Dest)>,
    tracks: Vec<NewTrack>,
    mute_clips: Vec<ClipId>,
    mute_tracks: Vec<TrackId>,
    message: String,
    /// A freeze rather than a bounce.
    freeze: bool,
}

impl Work {
    fn new(message: String) -> Work {
        Work {
            pieces: Vec::new(),
            tracks: Vec::new(),
            mute_clips: Vec::new(),
            mute_tracks: Vec::new(),
            message,
            freeze: false,
        }
    }
}

/// The song with `id` heard before its fader: unmuted, at 0 dB, in the middle, with nothing
/// moving its fader, pan or mute. `keep` says which of its clips stay.
fn pre_fader(
    p: &Project,
    id: TrackId,
    keep: impl Fn(&xirius_core::model::Clip) -> bool,
) -> Project {
    let mut song = p.clone();
    let t = Arc::make_mut(song.tracks.get_mut(&id).expect("checked"));
    t.volume_db = 0.0;
    t.pan = 0.0;
    t.mute = false;
    t.solo = false;
    t.automation.retain(|l| {
        !matches!(
            l.target,
            AutoTarget::Volume | AutoTarget::Pan | AutoTarget::Mute
        )
    });
    t.clips.retain(keep);
    song
}

fn bounce_name(t: &Track) -> String {
    format!("{} バウンス", t.name)
}

/// What a bounced clip is called (EDT-111): the clip's name and バウンス.
fn bounced(name: &str) -> String {
    match name.trim() {
        "" => "バウンス".to_string(),
        s => format!("{s} バウンス"),
    }
}

fn clips(p: &Project, args: &Args) -> CmdResult<Work> {
    let ids = args.require_ids::<ClipId>("クリップ")?;
    let new_track = args.flag("new-track");
    let mut work = Work::new(String::new());
    // A new track for each track the clips are on, below it.
    let mut made: BTreeMap<TrackId, usize> = BTreeMap::new();
    let (mut replaced, mut moved) = (0, 0);
    for id in &ids {
        let (t, c) = p.clip(*id)?;
        let tid = t.id;
        let audio = matches!(c.body, ClipBody::Audio { .. });
        if audio && !new_track {
            // Alone, without anything the clip keeps after the bounce.
            let mut song = pre_fader(p, tid, |x| x.id == *id);
            song.clip_fx.remove(id);
            let t = Arc::make_mut(song.tracks.get_mut(&tid).expect("checked"));
            t.inserts.clear();
            t.automation.clear();
            t.input_gain_db = 0.0;
            if let Some(x) = t.clips.first_mut() {
                x.muted = false;
                if let ClipBody::Audio { gain_db, .. } = &mut x.body {
                    *gain_db = 0.0;
                }
            }
            work.pieces.push((
                Piece {
                    song,
                    select: vec![Select::Track(tid)],
                    from: c.start,
                    to: c.start + c.length,
                    tail: 0.0,
                    name: bounced(&c.name),
                },
                Dest::Clip(*id),
            ));
            replaced += 1;
            continue;
        }
        let k = *made.entry(tid).or_insert_with(|| {
            work.tracks.push(NewTrack {
                after: tid,
                like: Some(tid),
                name: bounce_name(t),
            });
            work.tracks.len() - 1
        });
        let mut song = pre_fader(p, tid, |x| x.id == *id);
        let t = Arc::make_mut(song.tracks.get_mut(&tid).expect("checked"));
        if let Some(x) = t.clips.first_mut() {
            x.muted = false;
        }
        work.pieces.push((
            Piece {
                song,
                select: vec![Select::Track(tid)],
                from: c.start,
                to: c.start + c.length,
                tail: TAIL,
                name: bounced(&c.name),
            },
            Dest::Track(k),
        ));
        work.mute_clips.push(*id);
        moved += 1;
    }
    work.message = match (replaced, moved) {
        (n, 0) => format!("クリップ {n} 件をバウンスしました"),
        (0, n) => format!("クリップ {n} 件を新しいトラックにバウンスしました"),
        (a, b) => format!("クリップ {a} 件をバウンスし、{b} 件を新しいトラックにバウンスしました"),
    };
    Ok(work)
}

/// A track that has sound of its own to bounce.
fn bounceable(p: &Project, id: TrackId) -> CmdResult<&Track> {
    let t = p.track(id)?;
    match t.kind {
        TrackKind::Audio | TrackKind::Instrument => Ok(t),
        _ => Err(CmdError::invalid(format!(
            "「{}」はバウンスできません。オーディオかインストゥルメントのトラックを指定してください",
            t.name
        ))),
    }
}

fn tracks(p: &Project, args: &Args) -> CmdResult<Work> {
    let ids = args.require_ids::<TrackId>("トラック")?;
    let mut work = Work::new(format!(
        "{} トラックを新しいトラックにバウンスしました",
        ids.len()
    ));
    for id in &ids {
        let t = bounceable(p, *id)?;
        let (from, to) = span(t)?;
        work.tracks.push(NewTrack {
            after: *id,
            like: Some(*id),
            name: bounce_name(t),
        });
        work.pieces.push((
            Piece {
                song: pre_fader(p, *id, |_| true),
                select: vec![Select::Track(*id)],
                from,
                to,
                tail: TAIL,
                name: bounce_name(t),
            },
            Dest::Track(work.tracks.len() - 1),
        ));
        work.mute_tracks.push(*id);
    }
    Ok(work)
}

/// From the start of the first clip heard on `t` to the end of the last.
fn span(t: &Track) -> CmdResult<(i64, i64)> {
    let heard = t.clips.iter().filter(|c| !c.muted);
    let from = heard.clone().map(|c| c.start).min();
    let to = heard.map(|c| c.start + c.length).max();
    match (from, to) {
        (Some(from), Some(to)) => Ok((from, to)),
        _ => Err(CmdError::invalid(format!(
            "「{}」には鳴らすクリップがありません",
            t.name
        ))),
    }
}

fn freeze(p: &Project, args: &Args) -> CmdResult<Work> {
    let ids = args.require_ids::<TrackId>("トラック")?;
    let mut work = Work::new(String::new());
    work.freeze = true;
    for id in &ids {
        let t = p.track(*id)?;
        if !t.has_clips() {
            return Err(CmdError::invalid(format!(
                "「{}」はフリーズできません。オーディオかインストゥルメントのトラックを指定してください",
                t.name
            )));
        }
        if t.frozen.is_some() {
            continue;
        }
        if t.midi_out.is_some() && t.kind == TrackKind::Instrument {
            return Err(CmdError::invalid(format!(
                "「{}」は MIDI の出力に送っているため、フリーズできません",
                t.name
            )));
        }
        let (from, to) = span(t)?;
        work.pieces.push((
            Piece {
                song: pre_fader(p, *id, |_| true),
                select: vec![Select::Track(*id)],
                from,
                to,
                tail: TAIL,
                name: t.name.clone(),
            },
            Dest::Frozen(*id),
        ));
    }
    work.message = match work.pieces.len() {
        0 => {
            return Err(CmdError::new(
                xirius_core::ErrorCode::NothingToDo,
                "フリーズしていないトラックがありません",
            ));
        }
        1 => "トラックをフリーズしました".to_string(),
        n => format!("{n} トラックをフリーズしました"),
    };
    Ok(work)
}

/// A position argument in ticks: a number of ticks or `bar.beat.tick`.
fn tick_arg(p: &Project, args: &Args, name: &str) -> CmdResult<i64> {
    match args.0.get(name) {
        Some(Value::Number(n)) => n
            .as_i64()
            .ok_or_else(|| CmdError::invalid(format!("--{name} は tick の整数で指定してください"))),
        Some(Value::String(s)) => p.time_map().parse(s),
        _ => Err(CmdError::invalid(format!("--{name} が必要です"))),
    }
}

fn range(p: &Project, args: &Args) -> CmdResult<Work> {
    let (from, to) = (tick_arg(p, args, "from")?, tick_arg(p, args, "to")?);
    if from >= to {
        return Err(CmdError::range(format!(
            "範囲の開始 {from} は終了 {to} より前にしてください"
        )));
    }
    let tail = args.opt_f64("tail").unwrap_or(TAIL).clamp(0.0, TAIL);
    let ids = args.ids::<TrackId>("tracks")?;
    if ids.is_empty() {
        return Err(CmdError::invalid("--tracks を指定してください"));
    }
    for id in &ids {
        bounceable(p, *id)?;
    }
    // In the order of the column, the new track below the lowest of them.
    let order = column(p);
    let mut ids = ids;
    ids.sort_by_key(|id| order.iter().position(|x| x == id));
    ids.dedup();
    let mut work = Work::new(String::new());
    // Only the clips that lie wholly in the range are bounced, as only they are muted after.
    let within = |c: &xirius_core::model::Clip| c.start >= from && c.start + c.length <= to;
    let inside = |t: &Track| -> Vec<ClipId> {
        t.clips
            .iter()
            .filter(|c| !c.muted && within(c))
            .map(|c| c.id)
            .collect()
    };
    if args.flag("per-track") {
        for id in &ids {
            let t = p.track(*id)?;
            work.tracks.push(NewTrack {
                after: *id,
                like: Some(*id),
                name: bounce_name(t),
            });
            work.pieces.push((
                Piece {
                    song: pre_fader(p, *id, within),
                    select: vec![Select::Track(*id)],
                    from,
                    to,
                    tail,
                    name: bounce_name(t),
                },
                Dest::Track(work.tracks.len() - 1),
            ));
            work.mute_clips.extend(inside(t));
        }
        work.message = format!("選択範囲を {} トラックにバウンスしました", ids.len());
    } else {
        // Each track as a stem (its inserts, fader and pan; no sends, no master), summed.
        let name = "ミックスダウン".to_string();
        work.tracks.push(NewTrack {
            after: *ids.last().expect("not empty"),
            like: None,
            name: name.clone(),
        });
        let mut song = p.clone();
        for id in &ids {
            let t = Arc::make_mut(song.tracks.get_mut(id).expect("checked"));
            t.clips.retain(within);
        }
        work.pieces.push((
            Piece {
                song,
                select: ids.iter().map(|id| Select::Track(*id)).collect(),
                from,
                to,
                tail,
                name,
            },
            Dest::Track(0),
        ));
        for id in &ids {
            work.mute_clips.extend(inside(p.track(*id)?));
        }
        work.message = "選択範囲を新しいトラックにバウンスしました".to_string();
    }
    Ok(work)
}

/// Where the files go: `name/` in the song's media folder, or one under the temporary folder.
fn folder(p: &Project, dir: Option<&Path>, name: &str) -> PathBuf {
    match dir {
        Some(d) => p.media.place(d, name),
        None => std::env::temp_dir().join("Xirius").join(name),
    }
}

fn render_all(
    p: &Project,
    dir: Option<&Path>,
    builder: &mut Builder,
    work: &Work,
    progress: Option<&Progress>,
    written: &mut Vec<PathBuf>,
) -> CmdResult<Map<String, Value>> {
    let failed = |why: String| {
        let what = if work.freeze {
            "フリーズ"
        } else {
            "バウンス"
        };
        CmdError::io(format!("{what}できません（{why}）"))
    };
    let rate = f64::from(builder.rate());
    let tmap = TempoMap::from_project(p);
    let frames_of = |x: &Piece| {
        let (a, b) = (tmap.frame_at(x.from, rate), tmap.frame_at(x.to, rate));
        let tail = (x.tail * rate).round() as u64;
        (a, b, tail)
    };
    if let Some(pr) = progress {
        let total: u64 = work
            .pieces
            .iter()
            .map(|(x, _)| {
                let (a, b, tail) = frames_of(x);
                (b - a + tail) * x.select.len() as u64
            })
            .sum();
        pr.set(0, total);
    }
    let out = folder(p, dir, if work.freeze { FREEZE_FOLDER } else { FOLDER });
    std::fs::create_dir_all(&out).map_err(|e| failed(e.to_string()))?;
    let mut frozen = Vec::new();
    let mut replace = Vec::new();
    let mut clips: Vec<Vec<Value>> = vec![Vec::new(); work.tracks.len()];
    // Where the next clip on each new track starts: a tail stops there.
    let mut next: Vec<Vec<i64>> = vec![Vec::new(); work.tracks.len()];
    for (x, into) in &work.pieces {
        if let Dest::Track(k) = into {
            next[*k].push(x.from);
        }
    }
    for (x, into) in &work.pieces {
        let (a, b, tail) = frames_of(x);
        let (mut left, mut right) = (Vec::new(), Vec::new());
        for select in &x.select {
            let r = render_with(builder, &x.song, *select, a, b, tail, progress)
                .map_err(|e| failed(e.0))?;
            if left.is_empty() {
                (left, right) = (r.left, r.right);
            } else {
                for (s, v) in left.iter_mut().zip(&r.left) {
                    *s += v;
                }
                for (s, v) in right.iter_mut().zip(&r.right) {
                    *s += v;
                }
            }
        }
        // The tail, as far as it sounds and not over the next clip.
        let body = (b - a) as usize;
        let mut keep = body;
        for i in (body..left.len()).rev() {
            if left[i].abs() > QUIET || right[i].abs() > QUIET {
                keep = i + 1;
                break;
            }
        }
        if let Dest::Track(k) = into
            && let Some(stop) = next[*k].iter().copied().filter(|t| *t > x.from).min()
        {
            let limit = tmap.frame_at(stop, rate).saturating_sub(a) as usize;
            keep = keep.min(limit.max(body));
        }
        left.truncate(keep);
        right.truncate(keep);
        let base = match sanitize(&x.name).trim() {
            "" => "Bounce".to_string(),
            s => s.to_string(),
        };
        let path = unused(&out, &base);
        write_wav(&path, &left, &right, builder.rate(), 32).map_err(|e| failed(e.0))?;
        written.push(path.clone());
        let file = song_path(&path, dir);
        match into {
            Dest::Clip(id) => {
                replace.push(json!({"id": id.to_string(), "file": file, "name": x.name}));
            }
            Dest::Frozen(id) => {
                frozen.push(json!({"id": id.to_string(), "file": file, "start": x.from}));
            }
            Dest::Track(k) => {
                let end = tmap.seconds_at(x.from) + keep as f64 / rate;
                let length = (tmap.tick_at(end) - x.from).max(1);
                clips[*k].push(json!({
                    "name": x.name,
                    "file": file,
                    "start": x.from,
                    "length": length,
                }));
            }
        }
    }
    let tracks: Vec<Value> = work
        .tracks
        .iter()
        .zip(clips)
        .map(|(t, clips)| {
            let mut v = json!({
                "after": t.after.to_string(),
                "name": t.name,
                "clips": clips,
            });
            if let Some(like) = t.like {
                v["like"] = json!(like.to_string());
            }
            v
        })
        .collect();
    let mut m = Map::new();
    if work.freeze {
        m.insert("tracks".into(), Value::Array(frozen));
        return Ok(m);
    }
    if !replace.is_empty() {
        m.insert("replace".into(), Value::Array(replace));
    }
    if !tracks.is_empty() {
        m.insert("tracks".into(), Value::Array(tracks));
    }
    let ids = |xs: Vec<String>| Value::Array(xs.into_iter().map(Value::String).collect());
    if !work.mute_clips.is_empty() {
        m.insert(
            "mute_clips".into(),
            ids(work.mute_clips.iter().map(ToString::to_string).collect()),
        );
    }
    if !work.mute_tracks.is_empty() {
        m.insert(
            "mute_tracks".into(),
            ids(work.mute_tracks.iter().map(ToString::to_string).collect()),
        );
    }
    Ok(m)
}
