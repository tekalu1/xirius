//! Cut, copy, paste, duplicate and delete over notes, clips and the points of a controller lane
//! or an automation lane (CTX-028, KEY-003, OPS-015). The CLI, MCP and the screen all go through
//! these; what a copy takes lives in the session (`SessionState::clipboard`), outside the history.
//!
//! What is picked:
//! - `--clip` with `--lane`: the points of that controller lane (`--ticks`, or `--from`/`--to`,
//!   narrow it; positions are ticks from the start of the clip, as `cc.replace` counts them);
//! - `--track` with `--target`: the points of that automation lane (song ticks);
//! - otherwise `--ids` (notes and clips; the selection when omitted).
//!
//! The core has no playhead: `edit.paste --at` is where the first thing goes. Left out, or
//! `original`, things go back where they were taken from.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::*;
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::clipboard::{ClipCopy, Clipboard, NoteCopy, PluginCopy};
use crate::command::EditCtx;
use crate::error::ErrorCode;
use crate::id::{IdKind, PluginId, TrackId};
use crate::model::{CtrlKind, CtrlLane, CtrlPoint, PluginKind, TrackKind};

const TARGET_HELP: &str = "オートメーションの対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>）";

/// Which things a command is pointed at.
fn scope_args() -> Vec<ArgDef> {
    vec![
        ArgDef::ids(
            IdKind::Copyable,
            "ノート・クリップ・トラック（省略時は選択中のもの。トラックだけのときはトラックごと）",
        )
        .alias(&["tracks"]),
        ArgDef::id(
            "clip",
            IdKind::Clip,
            "コントローラーのポイントを持つ MIDI クリップ",
        ),
        ArgDef::text(
            "lane",
            "コントローラーのレーン: cc1・cc64・bend・at（--clip と組み合わせます）",
        ),
        ArgDef::id("track", IdKind::Track, "オートメーションのトラック"),
        ArgDef::text("target", TARGET_HELP),
        ArgDef::id(
            "plugin",
            IdKind::Plugin,
            "チャンネルのインサートのエフェクト（複製は、すぐ後ろに入ります）",
        ),
        ArgDef::text(
            "ticks",
            "レーンのポイントの位置（カンマ区切りの tick。コントローラーはクリップの先頭から、オートメーションは曲の先頭から）",
        ),
        ArgDef::int(
            "from",
            "レーンのポイントの範囲の開始（tick。この位置のポイントも含みます）",
        ),
        ArgDef::int(
            "to",
            "レーンのポイントの範囲の終了（tick。この位置のポイントも含みます）",
        ),
    ]
}

/// Where a paste goes.
fn dest_args() -> Vec<ArgDef> {
    vec![
        ArgDef::pos(
            "at",
            "貼り付ける位置（最初のものがここに来ます。省略か original で、元の位置）",
        ),
        ArgDef::id(
            "clip",
            IdKind::Clip,
            "貼り付け先の MIDI クリップ（ノートとコントローラーのポイント。省略時は取った元のクリップ）",
        ),
        ArgDef::text(
            "lane",
            "貼り付け先のコントローラーのレーン（省略時は取った元のレーン）",
        ),
        ArgDef::id(
            "track",
            IdKind::Track,
            "貼り付け先のトラック（クリップは最初のクリップの行をここに合わせ、他は相対的にずらします。オートメーションのポイントもここへ。エフェクトはインサートの末尾へ）",
        ),
        ArgDef::text("target", TARGET_HELP),
        super::overlap::note_arg(),
        super::overlap::clip_arg(),
    ]
}

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "edit.copy",
            "ノート・クリップ・レーンのポイントをクリップボードにコピーします",
            scope_args(),
            |p, s, _, a| {
                let board = gather(p, a, &s.selection)?;
                let msg = format!("{}をコピーしました", board.describe());
                let out = summary(&board, msg);
                s.clipboard = Some(board);
                Ok(out)
            },
        ),
        destroy(
            "edit.cut",
            "ノート・クリップ・レーンのポイントを切り取り、クリップボードに置きます（クリップは --ripple で同じトラックの後ろを詰める）",
            [scope_args(), super::ripple::args().to_vec()].concat(),
            |p, _, a| {
                let board = gather(p, a, &[])?;
                let moved = remove_rippled(p, &board, a)?;
                let msg = format!(
                    "{}を切り取りました{}",
                    board.describe(),
                    super::ripple::told(moved)
                );
                Ok(summary(&board, msg).keeping(board))
            },
        ),
        destroy(
            "edit.delete",
            "ノート・クリップ・レーンのポイントを削除します（クリップボードは変えません。クリップは --ripple で同じトラックの後ろを詰める）",
            [scope_args(), super::ripple::args().to_vec()].concat(),
            |p, _, a| {
                let board = gather(p, a, &[])?;
                let moved = remove_rippled(p, &board, a)?;
                let msg = format!(
                    "{}を削除しました{}",
                    board.describe(),
                    super::ripple::told(moved)
                );
                Ok(summary(&board, msg))
            },
        ),
        edit(
            "edit.paste",
            "クリップボードのものを貼り付けます（貼り付けたものの ID は新しくなります）",
            dest_args(),
            |p, cx, a| {
                let board = cx.clipboard.ok_or_else(|| {
                    CmdError::new(ErrorCode::NothingToDo, "クリップボードが空です")
                })?;
                let shift = match at(p, a)? {
                    Some(t) => t - board.first_tick(),
                    None => 0,
                };
                let at_text = match at(p, a)? {
                    Some(t) => p.time_map().format(t),
                    None => "元の位置".to_string(),
                };
                let (done, mut out) = put(p, cx, board, shift, a)?;
                out.message = format!("{}を{at_text}に貼り付けました{done}", board.describe());
                Ok(out)
            },
        ),
        edit(
            "edit.duplicate",
            "ノート・クリップ・レーンのポイントを複製します（クリップボードは変えません。既定は、選んだものの幅だけ右）",
            [
                scope_args(),
                vec![
                    ArgDef::int(
                        "by",
                        "元の位置からのずれ（tick。省略時は、選んだものの幅だけ右）",
                    ),
                    super::overlap::note_arg(),
                    super::overlap::clip_arg(),
                ],
            ]
            .concat(),
            |p, cx, a| {
                let board = gather(p, a, &[])?;
                let span = board.last_tick() - board.first_tick();
                let shift =
                    a.opt_i64("by")
                        .unwrap_or(if span > 0 { span } else { Tick::from(p.ppq) });
                let (done, mut out) = put(p, cx, &board, shift, a)?;
                out.message = format!("{}を複製しました{done}", board.describe());
                Ok(out)
            },
        ),
        read(
            "edit.clipboard",
            "クリップボードにあるものを表示します",
            vec![],
            |p, s, _, _| match &s.clipboard {
                None => Ok(Output::with(
                    "クリップボードは空です",
                    json!({ "kind": Value::Null, "count": 0 }),
                )),
                Some(b) => {
                    let tm = p.time_map();
                    Ok(Output::with(
                        format!(
                            "{}（{}〜{}）",
                            b.describe(),
                            tm.format(b.first_tick()),
                            tm.format(b.last_tick())
                        ),
                        json!({
                            "kind": b.kind(),
                            "count": b.len(),
                            "first": b.first_tick(),
                            "last": b.last_tick(),
                        }),
                    ))
                }
            },
        ),
    ]
}

fn summary(board: &Clipboard, message: String) -> Output {
    Output::with(
        message,
        json!({ "kind": board.kind(), "count": board.len() }),
    )
}

fn nothing(what: &str) -> CmdError {
    CmdError::new(ErrorCode::NothingToDo, what.to_string())
}

fn parse_kind(s: &str) -> CmdResult<CtrlKind> {
    CtrlKind::parse(s).ok_or_else(|| {
        CmdError::invalid(format!(
            "レーン `{s}` を読み取れません（cc1・bend・at など）"
        ))
    })
}

/// `--at`: a position, or `None` for the original place.
fn at(p: &Project, a: &Args) -> CmdResult<Option<Tick>> {
    if matches!(a.opt_str("at"), Some("original")) {
        return Ok(None);
    }
    opt_tick(p, a, "at")
}

// ---- picking ----------------------------------------------------------------------------------------

/// Which points of a lane: all, or those at `ticks`, or those within `from`..=`to`.
struct Pick {
    ticks: Option<HashSet<Tick>>,
    from: Option<Tick>,
    to: Option<Tick>,
}

impl Pick {
    fn from_args(a: &Args) -> CmdResult<Pick> {
        let ticks = match a.opt_str("ticks") {
            None => None,
            Some(s) => {
                let mut set = HashSet::new();
                for part in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    set.insert(part.parse::<Tick>().map_err(|_| {
                        CmdError::invalid(format!("--ticks の `{part}` を数として読み取れません"))
                    })?);
                }
                Some(set)
            }
        };
        Ok(Pick {
            ticks,
            from: a.opt_i64("from"),
            to: a.opt_i64("to"),
        })
    }

    fn keeps(&self, t: Tick) -> bool {
        self.ticks.as_ref().is_none_or(|s| s.contains(&t))
            && self.from.is_none_or(|f| t >= f)
            && self.to.is_none_or(|x| t <= x)
    }
}

/// What the arguments point at (the `selection` when no `--ids` is given).
fn gather(p: &Project, a: &Args, selection: &[AnyId]) -> CmdResult<Clipboard> {
    if let Some(lane) = a.opt_str("lane") {
        return gather_ctrl(p, a, lane);
    }
    if a.has("target") {
        return gather_auto(p, a);
    }
    if a.has("plugin") {
        return gather_plugin(p, a);
    }
    // Only tracks in `--ids` (or in the selection): the tracks with their clips and routing.
    let picked = a.ids::<AnyId>("ids")?;
    if !picked.is_empty() && picked.iter().all(|i| matches!(i, AnyId::Track(_))) {
        return super::track_clipboard::gather(p, a);
    }
    gather_items(p, a, selection)
}

fn gather_ctrl(p: &Project, a: &Args, lane: &str) -> CmdResult<Clipboard> {
    let cid: ClipId = a.id("clip")?;
    let kind = parse_kind(lane)?;
    let pick = Pick::from_args(a)?;
    let (_, clip) = p.clip(cid)?;
    if !clip.is_midi() {
        return Err(CmdError::conflict(format!(
            "{cid} は MIDI クリップではありません"
        )));
    }
    let l = clip
        .controllers
        .iter()
        .find(|l| l.kind == kind)
        .ok_or_else(|| CmdError::not_found(format!("{cid} に {lane} のレーンがありません")))?;
    let points: Vec<CtrlPoint> = l
        .points
        .iter()
        .filter(|q| pick.keeps(q.tick))
        .map(|q| CtrlPoint {
            tick: q.tick + clip.start,
            ..*q
        })
        .collect();
    if points.is_empty() {
        return Err(nothing("指定の範囲にポイントがありません"));
    }
    Ok(Clipboard::Controller {
        clip: cid,
        kind,
        points,
    })
}

fn gather_plugin(p: &Project, a: &Args) -> CmdResult<Clipboard> {
    let id: PluginId = a.id("plugin")?;
    let (track, plugin) = p
        .tracks
        .iter()
        .find_map(|(t, x)| {
            x.inserts
                .iter()
                .find(|y| y.id == id)
                .map(|y| (*t, y.clone()))
        })
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))?;
    if plugin.kind == PluginKind::Instrument {
        return Err(CmdError::conflict(
            "インストゥルメントはコピーできません（差し替えるか、トラックごと複製してください）",
        ));
    }
    Ok(Clipboard::Plugins(vec![PluginCopy { track, plugin }]))
}

fn gather_auto(p: &Project, a: &Args) -> CmdResult<Clipboard> {
    let tid: TrackId = a.id("track")?;
    let target: AutoTarget = a.id("target")?;
    let pick = Pick::from_args(a)?;
    let l = p
        .track(tid)?
        .automation
        .iter()
        .find(|l| l.target == target)
        .ok_or_else(|| CmdError::not_found(format!("{tid} に {target} のレーンがありません")))?;
    let points: Vec<AutoPoint> = l
        .points
        .iter()
        .filter(|q| pick.keeps(q.tick))
        .copied()
        .collect();
    if points.is_empty() {
        return Err(nothing("指定の範囲にポイントがありません"));
    }
    Ok(Clipboard::Automation {
        track: tid,
        target,
        points,
    })
}

fn gather_items(p: &Project, a: &Args, selection: &[AnyId]) -> CmdResult<Clipboard> {
    let mut ids = a.ids::<AnyId>("ids")?;
    if ids.is_empty() {
        ids = selection.to_vec();
    }
    let clips: HashSet<ClipId> = ids
        .iter()
        .filter_map(|i| match i {
            AnyId::Clip(c) => Some(*c),
            _ => None,
        })
        .collect();
    let notes: HashSet<NoteId> = ids
        .iter()
        .filter_map(|i| match i {
            AnyId::Note(n) => Some(*n),
            _ => None,
        })
        .collect();
    if clips.is_empty() && notes.is_empty() {
        return Err(CmdError::invalid(
            "ノートかクリップを --ids で指定してください（選択中のものもありません）",
        ));
    }
    // A clip in the pick takes its notes with it: the clips are what is kept.
    if !clips.is_empty() {
        let mut out = Vec::new();
        for (tid, t) in &p.tracks {
            for c in t.clips.iter().filter(|c| clips.contains(&c.id)) {
                out.push(ClipCopy {
                    track: *tid,
                    clip: c.clone(),
                });
            }
        }
        if let Some(missing) = clips.iter().find(|c| !out.iter().any(|x| x.clip.id == **c)) {
            return Err(CmdError::not_found(format!(
                "クリップ {missing} が見つかりません"
            )));
        }
        out.sort_by_key(|c| (c.clip.start, c.clip.id));
        return Ok(Clipboard::Clips(out));
    }
    let mut out = Vec::new();
    for t in p.tracks.values() {
        for c in &t.clips {
            for n in c.notes().iter().filter(|n| notes.contains(&n.id)) {
                out.push(NoteCopy {
                    clip: c.id,
                    note: Note {
                        start: c.start + n.start,
                        ..*n
                    },
                });
            }
        }
    }
    if let Some(missing) = notes.iter().find(|n| !out.iter().any(|x| x.note.id == **n)) {
        return Err(CmdError::not_found(format!(
            "ノート {missing} が見つかりません"
        )));
    }
    out.sort_by_key(|c| (c.note.start, c.note.pitch, c.note.id));
    Ok(Clipboard::Notes(out))
}

// ---- removing ---------------------------------------------------------------------------------------

/// Takes out what `board` holds; clips taken out with `--ripple` close up their tracks
/// (EDT-135). Gives how many clips moved to close up.
fn remove_rippled(p: &mut Project, board: &Clipboard, a: &Args) -> CmdResult<usize> {
    remove(p, board)?;
    match (board, super::ripple::wanted(a)) {
        (Clipboard::Clips(v), Some(follow)) => {
            let gone: Vec<_> = v
                .iter()
                .map(|c| (c.track, c.clip.start, c.clip.start + c.clip.length))
                .collect();
            super::ripple::close_up(p, &gone, follow)
        }
        _ => Ok(0),
    }
}

fn remove(p: &mut Project, board: &Clipboard) -> CmdResult<()> {
    match board {
        Clipboard::Section { .. } => {
            return Err(CmdError::invalid(
                "セクションは section.cut で切り取ります（時間ごと取り除くため）",
            ));
        }
        Clipboard::Tracks { ids, .. } => {
            for id in ids {
                p.delete_track(*id)?;
            }
        }
        Clipboard::Notes(v) => {
            let scope = Scope {
                notes: v.iter().map(|c| c.note.id).collect(),
                clips: HashSet::new(),
            };
            remove_notes(p, &scope)?;
        }
        Clipboard::Clips(v) => {
            for c in v {
                let (tid, i) = p.find_clip(c.clip.id).ok_or_else(|| {
                    CmdError::not_found(format!("クリップ {} が見つかりません", c.clip.id))
                })?;
                p.track_mut(tid)?.clips.remove(i);
            }
        }
        Clipboard::Plugins(v) => {
            for c in v {
                let tid = track::plugin_owner(p, c.plugin.id)?;
                p.track_mut(tid)?.inserts.retain(|x| x.id != c.plugin.id);
                p.fix_track_rack(tid);
            }
        }
        // A channel's settings are copied, never cut.
        Clipboard::Channel(_) => {}
        Clipboard::Controller { clip, kind, points } => {
            let (_, cl) = p.clip_mut(*clip)?;
            let ticks: HashSet<Tick> = points.iter().map(|q| q.tick - cl.start).collect();
            if let Some(i) = cl.controllers.iter().position(|l| l.kind == *kind) {
                let rest: Vec<CtrlPoint> = cl.controllers[i]
                    .points
                    .iter()
                    .filter(|q| !ticks.contains(&q.tick))
                    .copied()
                    .collect();
                if rest.is_empty() {
                    cl.controllers.remove(i);
                } else {
                    cl.controllers[i].points = Arc::new(rest);
                }
            }
        }
        Clipboard::Automation {
            track,
            target,
            points,
        } => {
            let ticks: HashSet<Tick> = points.iter().map(|q| q.tick).collect();
            let t = p.track_mut(*track)?;
            let l = t
                .automation
                .iter_mut()
                .find(|l| l.target == *target)
                .ok_or_else(|| {
                    CmdError::not_found(format!("{track} に {target} のレーンがありません"))
                })?;
            Arc::make_mut(&mut l.points).retain(|q| !ticks.contains(&q.tick));
        }
    }
    Ok(())
}

// ---- putting back -----------------------------------------------------------------------------------

/// Lays `board` into the song, every position moved by `shift`. Returns a note for the message
/// (things left out) and the answer (with the ids made).
fn put(
    p: &mut Project,
    cx: &EditCtx<'_>,
    board: &Clipboard,
    shift: Tick,
    a: &Args,
) -> CmdResult<(String, Output)> {
    let ai = cx.author.is_ai();
    match board {
        Clipboard::Section { source, id } => Ok((
            String::new(),
            super::time_shift::paste_section(p, source, *id, board.first_tick() + shift)?,
        )),
        Clipboard::Tracks { source, ids } => {
            Ok((String::new(), super::track_clipboard::put(p, source, ids)?))
        }
        Clipboard::Notes(v) => {
            let dest: Option<ClipId> = a.opt_id("clip")?;
            let mut made = Vec::new();
            let mut left_out = 0;
            for c in v {
                let cid = dest.unwrap_or(c.clip);
                let (start, length) = {
                    let (_, clip) = p.clip(cid)?;
                    if !clip.is_midi() {
                        return Err(CmdError::conflict(format!(
                            "{cid} は MIDI クリップではありません"
                        )));
                    }
                    (clip.start, clip.length)
                };
                let rel = c.note.start + shift - start;
                if rel < 0 || rel >= length {
                    left_out += 1;
                    continue;
                }
                let mut n = c.note;
                n.id = NoteId(p.alloc());
                n.start = rel;
                n.length = n.length.min(length - rel).max(1);
                n.ai = ai;
                made.push(n.id.to_string());
                if let ClipBody::Midi { notes, .. } = &mut p.clip_mut(cid)?.1.body {
                    Arc::make_mut(notes).push(n);
                }
            }
            if made.is_empty() {
                return Err(CmdError::range(
                    "貼り付け先がクリップの範囲の外です（クリップの中の位置を指定してください）",
                ));
            }
            Ok((
                left_out_note(left_out),
                Output::with(String::new(), json!({ "ids": made })),
            ))
        }
        Clipboard::Clips(v) => {
            let order: Vec<TrackId> = column_nodes(p)
                .into_iter()
                .filter_map(|n| match n {
                    Node::Track(t) => Some(t),
                    Node::Folder(_) => None,
                })
                .collect();
            let row = |t: TrackId| order.iter().position(|x| *x == t);
            let dest: Option<TrackId> = a.opt_id("track")?;
            // The first clip lands on `dest`; the others keep their rows relative to it.
            let first = v
                .iter()
                .min_by_key(|c| (c.clip.start, row(c.track).unwrap_or(usize::MAX)));
            let delta = match (dest, first) {
                (Some(d), Some(f)) => match (row(d), row(f.track)) {
                    (Some(d), Some(f)) => Some(d as i64 - f as i64),
                    _ => None,
                },
                _ => None,
            };
            let mut plan = Vec::new();
            for c in v {
                let to = match (dest, delta) {
                    (None, _) => c.track,
                    (Some(d), None) => d,
                    (Some(d), Some(dl)) => match row(c.track) {
                        None => d,
                        Some(r) => usize::try_from(r as i64 + dl)
                            .ok()
                            .and_then(|i| order.get(i))
                            .copied()
                            .ok_or_else(|| {
                                CmdError::range(
                                    "貼り付け先に、そのトラック数のトラックがありません",
                                )
                            })?,
                    },
                };
                let fits = matches!(
                    (&c.clip.body, p.track(to)?.kind),
                    (ClipBody::Midi { .. }, TrackKind::Instrument)
                        | (ClipBody::Audio { .. }, TrackKind::Audio)
                );
                if !fits {
                    return Err(CmdError::conflict(format!(
                        "クリップ「{}」は {to} に配置できない種類です",
                        c.clip.name
                    )));
                }
                if c.clip.start + shift < 0 {
                    return Err(CmdError::range("曲の先頭より前には貼り付けられません"));
                }
                plan.push((to, c));
            }
            let mut made = Vec::new();
            for (to, c) in plan {
                let mut copy = clone_clip(p, &c.clip);
                copy.start = c.clip.start + shift;
                made.push(copy.id.to_string());
                p.track_mut(to)?.clips.push(copy);
            }
            Ok((
                String::new(),
                Output::with(String::new(), json!({ "ids": made })),
            ))
        }
        Clipboard::Channel(c) => Ok((
            String::new(),
            super::channel_settings::paste_from_board(p, cx, c, a)?,
        )),
        Clipboard::Plugins(v) => {
            let mut made = Vec::new();
            for c in v {
                // Left alone: right after the one it was copied from (a copy of a copy
                // too); `--track`: the end of that channel.
                let (tid, index) = match a.opt_id::<TrackId>("track")? {
                    Some(t) => (t, None),
                    None => match track::plugin_owner(p, c.plugin.id) {
                        Ok(t) => {
                            let i = p.track(t)?.inserts.iter().position(|x| x.id == c.plugin.id);
                            (t, i.map(|i| i + 1))
                        }
                        Err(_) => (c.track, None),
                    },
                };
                let mut copy = c.plugin.clone();
                copy.id = PluginId(p.alloc());
                made.push(copy.id.to_string());
                track::place_plugin(p, tid, copy, index)?;
            }
            Ok((
                String::new(),
                Output::with(String::new(), json!({ "ids": made })),
            ))
        }
        Clipboard::Controller { clip, kind, points } => {
            let cid: ClipId = a.opt_id("clip")?.unwrap_or(*clip);
            let to_kind = match a.opt_str("lane") {
                Some(s) => parse_kind(s)?,
                None => *kind,
            };
            let (_, cl) = p.clip_mut(cid)?;
            if !cl.is_midi() {
                return Err(CmdError::conflict(format!(
                    "{cid} は MIDI クリップではありません"
                )));
            }
            let (slo, shi) = kind.range();
            let (lo, hi) = to_kind.range();
            let convert = |v: i32| -> i32 {
                if to_kind == *kind {
                    return v;
                }
                let r = f64::from(v - slo) / f64::from(shi - slo);
                (f64::from(lo) + r * f64::from(hi - lo)).round() as i32
            };
            let mut fresh: Vec<CtrlPoint> = Vec::new();
            let mut left_out = 0;
            for q in points {
                let rel = q.tick + shift - cl.start;
                if rel < 0 || rel >= cl.length {
                    left_out += 1;
                    continue;
                }
                fresh.push(CtrlPoint {
                    tick: rel,
                    value: convert(q.value).clamp(lo, hi),
                    ..*q
                });
            }
            if fresh.is_empty() {
                return Err(CmdError::range(
                    "貼り付け先がクリップの範囲の外です（クリップの中の位置を指定してください）",
                ));
            }
            let n = fresh.len();
            let ticks: HashSet<Tick> = fresh.iter().map(|q| q.tick).collect();
            let i = cl.controllers.iter().position(|l| l.kind == to_kind);
            let mut merged: Vec<CtrlPoint> = i
                .map(|i| cl.controllers[i].points.iter().copied().collect())
                .unwrap_or_default();
            merged.retain(|q| !ticks.contains(&q.tick));
            merged.extend(fresh);
            merged.sort_by_key(|q| q.tick);
            match i {
                Some(i) => cl.controllers[i].points = Arc::new(merged),
                None => {
                    cl.controllers.push(CtrlLane {
                        kind: to_kind,
                        points: Arc::new(merged),
                    });
                    cl.controllers.sort_by_key(|l| l.kind);
                }
            }
            Ok((
                left_out_note(left_out),
                Output::with(String::new(), json!({ "points": n })),
            ))
        }
        Clipboard::Automation {
            track,
            target,
            points,
        } => {
            let tid: TrackId = a.opt_id("track")?.unwrap_or(*track);
            let to: AutoTarget = if a.has("target") {
                a.id("target")?
            } else {
                target.clone()
            };
            let (lo, hi) = to.range();
            let mut fresh = Vec::new();
            let mut left_out = 0;
            for q in points {
                if q.tick + shift < 0 {
                    left_out += 1;
                    continue;
                }
                let v = if to == *target {
                    q.value
                } else {
                    to.from_norm(target.to_norm(q.value))
                };
                fresh.push(AutoPoint {
                    tick: q.tick + shift,
                    value: v.clamp(lo, hi),
                    ..*q
                });
            }
            if fresh.is_empty() {
                return Err(CmdError::range("曲の先頭より前には貼り付けられません"));
            }
            let t = p.track_mut(tid)?;
            if !t.automation.iter().any(|l| l.target == to) {
                if !to.exists_on(t) {
                    return Err(CmdError::not_found(format!(
                        "{tid} に {to} が見つかりません"
                    )));
                }
                t.automation.push(AutoLane::on_track(to.clone(), t));
            }
            let l = t
                .automation
                .iter_mut()
                .find(|l| l.target == to)
                .expect("added above");
            l.hidden = false;
            let n = fresh.len();
            let ticks: HashSet<Tick> = fresh.iter().map(|q| q.tick).collect();
            let pts = Arc::make_mut(&mut l.points);
            pts.retain(|q| !ticks.contains(&q.tick));
            pts.extend(fresh);
            pts.sort_by_key(|q| q.tick);
            Ok((
                left_out_note(left_out),
                Output::with(String::new(), json!({ "points": n })),
            ))
        }
    }
}

fn left_out_note(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("（範囲の外の {n} 個は除きました）")
    }
}
