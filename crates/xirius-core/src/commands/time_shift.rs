//! Time put into the song and taken out of it, the whole song at once (ARR-005), and the
//! section commands built on that: a section copied in right after itself, cut in two,
//! given a colour, selected with what is in it.

use super::arrange::{MeterAt, bpm_at, lay_meters, meter_at, meter_ticks, rebuild_meters};
use super::clip::split_one;
use super::edit_range::parse_range;
use super::*;
use crate::auto::{AutoLane, AutoPoint};
use crate::clipboard::Clipboard;
use crate::clipfx::ClipFx;
use crate::command::EditCtx;
use crate::error::ErrorCode;
use crate::id::{IdKind, SectionId, TempoId, TimeSigId, TrackId};
use crate::model::{LoopRange, Section, TempoPoint};
use crate::time::Meter;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "range.time.insert",
            "範囲の位置に空の時間を挿入します。曲全体（全トラックのクリップ・オートメーション・テンポ・拍子・セクション・マーカー・ループ）が後ろへずれ、範囲の頭にかかるクリップは分かれます",
            span_args(),
            insert_time_cmd,
        ),
        destroy(
            "range.time.delete",
            "範囲の時間を取り除きます。範囲の中のクリップは消え、曲全体（全トラックのクリップ・オートメーション・テンポ・拍子・セクション・マーカー・ループ）が前へつまります",
            span_args(),
            delete_time_cmd,
        ),
        edit(
            "section.duplicate",
            "セクションを複製します。既定では元のすぐ後ろに差し込み、後ろのセクションは複製の長さだけ後ろへずれます（--at end で最後のセクションの後ろ）",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::text("at", "after（既定）か end").default(json!("after")),
                ArgDef::int(
                    "index",
                    "複製を置く順番（0 が先頭。--at の代わり。複製の後の順番）",
                )
                .range(0.0, 1e6),
            ],
            duplicate_section,
        ),
        read(
            "section.copy",
            "セクションを、中のクリップ・オートメーション・テンポごとクリップボードにコピーします",
            vec![ArgDef::id("id", IdKind::Section, "セクション").req()],
            |p, s, _, a| {
                let board = section_board(p, a.id("id")?)?;
                let out = Output::msg(format!("{}をコピーしました", board.describe()));
                s.clipboard = Some(board);
                Ok(out)
            },
        ),
        destroy(
            "section.cut",
            "セクションをクリップボードに置き、その時間を曲から取り除きます（後ろが前へつまります）",
            vec![ArgDef::id("id", IdKind::Section, "セクション").req()],
            |p, _, a| {
                let board = section_board(p, a.id("id")?)?;
                let (start, end) = (board.first_tick(), board.last_tick());
                delete_time(p, start, end)?;
                Ok(Output::msg(format!("{}を切り取りました", board.describe())).keeping(board))
            },
        ),
        edit(
            "section.paste",
            "クリップボードのセクションを貼り付けます。その位置に時間を差し込み、後ろは貼り付けた長さだけ後ろへずれます",
            vec![ArgDef::pos(
                "at",
                "貼り付ける位置（省略時は最後のセクションの後ろ）",
            )],
            |p, cx, a| {
                let Some(Clipboard::Section { source, id }) = cx.clipboard else {
                    return Err(CmdError::new(
                        ErrorCode::NothingToDo,
                        "クリップボードにセクションがありません",
                    ));
                };
                let at = match opt_tick(p, a, "at")? {
                    Some(t) => t,
                    None => p
                        .sections
                        .iter()
                        .map(|s| s.start + s.length)
                        .max()
                        .unwrap_or(0),
                };
                paste_section(p, source, *id, at)
            },
        ),
        edit(
            "section.split",
            "セクションを指定の位置で 2 つに分けます（名前と色は引き継ぎます）",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::pos("at", "分ける位置").req(),
            ],
            split_section,
        ),
        edit(
            "section.color.set",
            "セクションの色を設定します",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::text("color", "#rrggbb または none").req(),
            ],
            |p, _, a| {
                let id: SectionId = a.id("id")?;
                let color = color_arg(a)?;
                let s = p.sections.iter_mut().find(|s| s.id == id).ok_or_else(|| {
                    CmdError::not_found(format!("セクション {id} が見つかりません"))
                })?;
                s.color = color;
                Ok(Output::msg(format!("{id} の色を設定しました")))
            },
        ),
        edit(
            "loop.duplicate",
            "ループ範囲の中身を、ループのすぐ後ろへ複製します（後ろにあったものは置き換わります）。ループ範囲は複製した側に移ります",
            vec![],
            duplicate_loop,
        ),
        read(
            "select.in-section",
            "セクションにかかっているクリップを選択します",
            vec![ArgDef::id("id", IdKind::Section, "セクション").req()],
            |p, s, _, a| {
                let id: SectionId = a.id("id")?;
                let sec = p.sections.iter().find(|s| s.id == id).ok_or_else(|| {
                    CmdError::not_found(format!("セクション {id} が見つかりません"))
                })?;
                let (from, to) = (sec.start, sec.start + sec.length);
                s.selection = p
                    .tracks
                    .values()
                    .flat_map(|t| t.clips.iter())
                    .filter(|c| c.start < to && c.end() > from)
                    .map(|c| AnyId::Clip(c.id))
                    .collect();
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
    ]
}

fn span_args() -> Vec<ArgDef> {
    vec![
        ArgDef::text(
            "range",
            "loop・開始..終了・section:名前（start・end・length の代わり）",
        ),
        ArgDef::pos("start", "範囲の開始（--range のときは不要）"),
        ArgDef::pos("end", "範囲の終了（--length の代わり）"),
        ArgDef::int("length", "範囲の長さ（tick）"),
        ArgDef::float("bars", "範囲の長さ（小節）"),
    ]
}

/// The span the arguments name: `--range`, or `--start` with `--end`, `--length` or `--bars`.
fn span(p: &Project, a: &Args) -> CmdResult<(Tick, Tick)> {
    let (start, end) = if let Some(text) = a.opt_str("range") {
        parse_range(p, text)?
    } else {
        let start = tick(p, a, "start")?;
        let end = match opt_tick(p, a, "end")? {
            Some(end) => end,
            None => start + length_arg(p, a, start)?,
        };
        (start, end)
    };
    if start < 0 || end <= start {
        return Err(CmdError::range("範囲の終了は開始より後にしてください"));
    }
    Ok((start, end))
}

fn insert_time_cmd(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (start, end) = span(p, a)?;
    insert_time(p, start, end - start)?;
    Ok(Output::msg(format!(
        "{} tick の時間を挿入しました",
        end - start
    )))
}

fn delete_time_cmd(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (start, end) = span(p, a)?;
    delete_time(p, start, end)?;
    Ok(Output::msg(format!(
        "{} tick の時間を取り除きました",
        end - start
    )))
}

// ---- clips and lanes ---------------------------------------------------------------------

fn clip_ids(p: &Project) -> Vec<ClipId> {
    p.tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| c.id))
        .collect()
}

/// Cuts every clip of every track at `at`.
fn split_all(p: &mut Project, at: Tick) -> CmdResult<()> {
    for id in clip_ids(p) {
        split_one(p, id, at)?;
    }
    Ok(())
}

/// Puts a point on the line of every lane at each place, and another a tick before it,
/// so that the line on one side of the place does not lean on the other.
pub(super) fn cut_at(p: &mut Project, at: &[Tick]) -> CmdResult<()> {
    for &t in at {
        split_all(p, t)?;
    }
    let tids: Vec<TrackId> = p.tracks.keys().copied().collect();
    for tid in tids {
        for lane in &mut p.track_mut(tid)?.automation {
            for &t in at {
                pin_edge(lane, t);
            }
        }
    }
    Ok(())
}

fn pin_edge(lane: &mut AutoLane, at: Tick) {
    if lane.points.is_empty() {
        return;
    }
    let before = (at > 1)
        .then(|| lane.point_at(at - 1))
        .flatten()
        .filter(|_| !lane.points.iter().any(|x| x.tick == at - 1));
    let here = lane
        .point_at(at)
        .filter(|_| !lane.points.iter().any(|x| x.tick == at));
    let pts = Arc::make_mut(&mut lane.points);
    pts.extend(before.map(|b| AutoPoint {
        tick: at - 1,
        step: false,
        curve: 0.0,
        ..b
    }));
    pts.extend(here.map(|h| AutoPoint { curve: 0.0, ..h }));
    pts.sort_by_key(|x| x.tick);
}

/// Opens a gap of `len` ticks at `a` in a lane: the line holds its value across it.
fn open_gap(lane: &mut AutoLane, a: Tick, len: Tick) {
    if lane.points.is_empty() {
        return;
    }
    let inside = lane.points.iter().any(|x| x.tick < a) && lane.points.iter().any(|x| x.tick >= a);
    let held = lane.point_at(a);
    let pts = Arc::make_mut(&mut lane.points);
    for x in pts.iter_mut() {
        if x.tick >= a {
            x.tick += len;
        }
    }
    if let (true, Some(h)) = (inside, held) {
        let pin = AutoPoint { curve: 0.0, ..h };
        pts.push(AutoPoint { tick: a, ..pin });
        if !pts.iter().any(|x| x.tick == a + len) {
            pts.push(AutoPoint {
                tick: a + len,
                ..pin
            });
        }
    }
    pts.sort_by_key(|x| x.tick);
}

/// Closes the stretch `[a, z)` in a lane: the line jumps at `a` to what it was at `z`.
fn close_gap(lane: &mut AutoLane, a: Tick, z: Tick) {
    if lane.points.is_empty() {
        return;
    }
    let len = z - a;
    let (va, vb, vz) = (
        lane.curve_at(a as f64),
        lane.curve_at((a - 1) as f64),
        lane.point_at(z),
    );
    let pts = Arc::make_mut(&mut lane.points);
    pts.retain(|x| x.tick < a || x.tick >= z);
    for x in pts.iter_mut() {
        if x.tick >= z {
            x.tick -= len;
        }
    }
    if !pts.iter().any(|x| x.tick == a)
        && let Some(v) = vz
    {
        pts.push(AutoPoint {
            tick: a,
            curve: 0.0,
            ..v
        });
    }
    if let (Some(va), Some(vb), Some(v)) = (va, vb, vz)
        && (va - v.value).abs() > 1e-9
        && a > 1
        && !pts.iter().any(|x| x.tick == a - 1)
    {
        pts.push(AutoPoint {
            tick: a - 1,
            value: vb,
            curve: 0.0,
            step: true,
        });
    }
    pts.sort_by_key(|x| x.tick);
}

/// Takes away the clips lying inside `[a, z)` (cut at both ends first).
fn clear_clips(p: &mut Project, a: Tick, z: Tick) -> CmdResult<()> {
    split_all(p, a)?;
    split_all(p, z)?;
    let tids: Vec<TrackId> = p.tracks.keys().copied().collect();
    let mut gone = Vec::new();
    for tid in tids {
        let t = p.track_mut(tid)?;
        t.clips.retain(|c| {
            let inside = c.start >= a && c.end() <= z;
            if inside {
                gone.push(c.id);
            }
            !inside
        });
    }
    for id in gone {
        p.fx_forget(id);
    }
    Ok(())
}

// ---- tempo -------------------------------------------------------------------------------

/// Drops the added points that say what is in force already.
fn tidy_tempo(out: &mut Vec<TempoPoint>, added: &[TempoId]) {
    out.sort_by_key(|x| x.tick);
    let mut kept: Vec<TempoPoint> = Vec::new();
    for x in out.drain(..) {
        if let Some(prev) = kept.last()
            && added.contains(&x.id)
            && !prev.ramp
            && !x.ramp
            && (prev.bpm - x.bpm).abs() < 1e-9
        {
            continue;
        }
        kept.push(x);
    }
    *out = kept;
}

fn new_tempo(
    p: &mut Project,
    tick: Tick,
    bpm: f64,
    ramp: bool,
    added: &mut Vec<TempoId>,
) -> TempoPoint {
    let id = TempoId(p.alloc());
    added.push(id);
    TempoPoint {
        id,
        tick,
        bpm,
        ramp,
    }
}

/// Whether the line of the tempo at `t` is part of a ramp that began earlier.
fn in_ramp(tempo: &[TempoPoint], t: Tick) -> bool {
    tempo
        .iter()
        .rev()
        .find(|x| x.tick <= t)
        .is_some_and(|x| x.ramp && x.tick < t)
}

fn tempo_open_gap(p: &mut Project, a: Tick, len: Tick) {
    let old = p.tempo.clone();
    let mut out: Vec<TempoPoint> = old
        .iter()
        .cloned()
        .map(|mut x| {
            if x.tick >= a && x.tick > 0 {
                x.tick += len;
            }
            x
        })
        .collect();
    let mut added = Vec::new();
    if a > 0 {
        let bpm = bpm_at(&old, a);
        let ramp = in_ramp(&old, a);
        if !out.iter().any(|x| x.tick == a) {
            let x = new_tempo(p, a, bpm, false, &mut added);
            out.push(x);
        }
        if ramp && !out.iter().any(|x| x.tick == a + len) {
            let x = new_tempo(p, a + len, bpm, true, &mut added);
            out.push(x);
        }
    }
    tidy_tempo(&mut out, &added);
    p.tempo = out;
}

fn tempo_close_gap(p: &mut Project, a: Tick, z: Tick) {
    let old = p.tempo.clone();
    let len = z - a;
    let mut out: Vec<TempoPoint> = old
        .iter()
        .filter(|x| x.tick < a || x.tick >= z)
        .cloned()
        .map(|mut x| {
            if x.tick >= z {
                x.tick -= len;
            }
            x
        })
        .collect();
    let mut added = Vec::new();
    if !out.iter().any(|x| x.tick == a) {
        let x = new_tempo(p, a, bpm_at(&old, z), in_ramp(&old, z), &mut added);
        out.push(x);
    }
    tidy_tempo(&mut out, &added);
    p.tempo = out;
}

// ---- meters ------------------------------------------------------------------------------

fn meters_open_gap(p: &mut Project, a: Tick, len: Tick) {
    let map = p.time_map();
    let here = meter_at(&p.timesig, map.to_bbt(a).0);
    let mut at = meter_ticks(p);
    for x in &mut at {
        if x.0 >= a && x.0 > 0 {
            x.0 += len;
        }
    }
    if a > 0 && !at.iter().any(|x| x.0 == a) {
        at.push((a, None, here));
    }
    rebuild_meters(p, at);
}

fn meters_close_gap(p: &mut Project, a: Tick, z: Tick) {
    let map = p.time_map();
    let len = z - a;
    let at_z = meter_at(&p.timesig, map.to_bbt(z).0);
    let mut at: Vec<(Tick, Option<TimeSigId>, Meter)> = meter_ticks(p)
        .into_iter()
        .filter(|x| x.0 < a || x.0 >= z)
        .map(|mut x| {
            if x.0 >= z {
                x.0 -= len;
            }
            x
        })
        .collect();
    if !at.iter().any(|x| x.0 == a) {
        at.push((a, None, at_z));
    }
    rebuild_meters(p, at);
}

// ---- the whole song ----------------------------------------------------------------------

/// Puts `len` ticks of nothing into the song at `a`.
pub(super) fn insert_time(p: &mut Project, a: Tick, len: Tick) -> CmdResult<()> {
    if a < 0 || len <= 0 {
        return Err(CmdError::range(
            "挿入する範囲は 0 以上の位置と 1 tick 以上の長さが必要です",
        ));
    }
    split_all(p, a)?;
    let tids: Vec<TrackId> = p.tracks.keys().copied().collect();
    for tid in tids {
        let t = p.track_mut(tid)?;
        for c in &mut t.clips {
            if c.start >= a {
                c.start += len;
            }
        }
        for lane in &mut t.automation {
            open_gap(lane, a, len);
        }
    }
    tempo_open_gap(p, a, len);
    meters_open_gap(p, a, len);
    for s in &mut p.sections {
        if s.start >= a {
            s.start += len;
        } else if s.start + s.length > a {
            s.length += len;
        }
    }
    if let Some(l) = &mut p.loop_range {
        if l.start >= a {
            l.start += len;
            l.end += len;
        } else if l.end > a {
            l.end += len;
        }
    }
    for m in &mut p.markers {
        if m.tick >= a {
            m.tick += len;
        }
    }
    Ok(())
}

/// Takes the time `[a, z)` out of the song: what is in it goes, what is after it comes up.
pub(super) fn delete_time(p: &mut Project, a: Tick, z: Tick) -> CmdResult<()> {
    if a < 0 || z <= a {
        return Err(CmdError::range("範囲の終了は開始より後にしてください"));
    }
    let len = z - a;
    clear_clips(p, a, z)?;
    let tids: Vec<TrackId> = p.tracks.keys().copied().collect();
    for tid in tids {
        let t = p.track_mut(tid)?;
        for c in &mut t.clips {
            if c.start >= z {
                c.start -= len;
            }
        }
        for lane in &mut t.automation {
            close_gap(lane, a, z);
        }
    }
    tempo_close_gap(p, a, z);
    meters_close_gap(p, a, z);
    let map = |t: Tick| {
        if t <= a {
            t
        } else if t >= z {
            t - len
        } else {
            a
        }
    };
    p.sections.retain_mut(|s| {
        let (start, end) = (map(s.start), map(s.start + s.length));
        s.start = start;
        s.length = end - start;
        s.length > 0
    });
    p.loop_range = p
        .loop_range
        .map(|l| LoopRange {
            start: map(l.start),
            end: map(l.end),
        })
        .filter(|l| l.end > l.start);
    p.markers.retain(|m| m.tick < a || m.tick >= z);
    for m in &mut p.markers {
        m.tick = map(m.tick);
    }
    Ok(())
}

// ---- a stretch of the song lifted out and laid down again --------------------------------

/// What lies in a stretch of the song, kept relative to its start.
struct Material {
    len: Tick,
    clips: Vec<(TrackId, Clip, Option<ClipFx>)>,
    lanes: Vec<(TrackId, usize, Vec<AutoPoint>)>,
    tempo: Vec<(Tick, f64, bool)>,
    /// The meter at the start and the changes after it, each with whether it stood on a bar
    /// line (the changes always do).
    meters: Vec<(Tick, Meter, bool)>,
}

fn take_material(p: &Project, a: Tick, z: Tick) -> CmdResult<Material> {
    // the cuts at the two ends are made on a copy: the song itself is left as it is
    let mut scratch = p.clone();
    split_all(&mut scratch, a)?;
    split_all(&mut scratch, z)?;
    let mut clips = Vec::new();
    let mut lanes = Vec::new();
    for (tid, t) in &scratch.tracks {
        for c in t.clips.iter().filter(|c| c.start >= a && c.end() <= z) {
            let mut c = c.clone();
            c.start -= a;
            clips.push((*tid, c.clone(), scratch.clip_fx.get(&c.id).cloned()));
        }
    }
    for (tid, t) in &p.tracks {
        for (i, lane) in t.automation.iter().enumerate() {
            if lane.points.is_empty() {
                continue;
            }
            let head = lane.point_at(a).map(|h| AutoPoint {
                tick: 0,
                curve: 0.0,
                ..h
            });
            let tail = lane.curve_at((z - 1) as f64).map(|value| AutoPoint {
                tick: z - a - 1,
                value,
                curve: 0.0,
                step: false,
            });
            let mut pts: Vec<AutoPoint> = head.into_iter().collect();
            pts.extend(
                lane.points
                    .iter()
                    .filter(|x| x.tick > a && x.tick < z - 1)
                    .map(|x| AutoPoint {
                        tick: x.tick - a,
                        ..*x
                    }),
            );
            pts.extend(tail);
            lanes.push((*tid, i, pts));
        }
    }
    let mut tempo = vec![(0, bpm_at(&p.tempo, a), false)];
    tempo.extend(
        p.tempo
            .iter()
            .filter(|x| x.tick > a && x.tick < z)
            .map(|x| (x.tick - a, x.bpm, x.ramp)),
    );
    let map = p.time_map();
    let head = map.to_bbt(a).0;
    let mut meters = vec![(0, meter_at(&p.timesig, head), map.bar_start(head) == a)];
    meters.extend(
        meter_ticks(p)
            .into_iter()
            .filter(|x| x.0 > a && x.0 < z)
            .map(|x| (x.0 - a, x.2, true)),
    );
    Ok(Material {
        len: z - a,
        clips,
        lanes,
        tempo,
        meters,
    })
}

/// Lays material down at `at`, over what is there.
fn place_material(p: &mut Project, m: &Material, at: Tick) -> CmdResult<()> {
    let z = at + m.len;
    // what the lanes and the tempo say at the far end stays said
    let ends: Vec<(TrackId, usize, Option<AutoPoint>)> = m
        .lanes
        .iter()
        .map(|(tid, i, _)| {
            let end = p
                .tracks
                .get(tid)
                .and_then(|t| t.automation.get(*i))
                .and_then(|l| l.point_at(z));
            (*tid, *i, end)
        })
        .collect();
    let bz = bpm_at(&p.tempo, z);
    let tm = p.time_map();
    let meter_z = meter_at(&p.timesig, tm.to_bbt(z).0);
    let old_meters = meter_ticks(p);
    clear_clips(p, at, z)?;
    for (tid, c, fx) in &m.clips {
        let mut copy = clone_clip_fresh(p, c);
        copy.start += at;
        if let Some(fx) = fx {
            p.clip_fx.insert(copy.id, fx.clone());
        }
        p.track_mut(*tid)?.clips.push(copy);
    }
    for (tid, i, pts) in &m.lanes {
        let end = ends
            .iter()
            .find(|e| e.0 == *tid && e.1 == *i)
            .and_then(|e| e.2);
        let Some(lane) = p.track_mut(*tid)?.automation.get_mut(*i) else {
            continue;
        };
        let laid: Vec<AutoPoint> = pts
            .iter()
            .map(|x| AutoPoint {
                tick: x.tick + at,
                ..*x
            })
            .collect();
        lane.splice(at, z - 1, laid);
        if let Some(e) = end
            && !lane.points.iter().any(|x| x.tick == z)
        {
            Arc::make_mut(&mut lane.points).push(AutoPoint {
                tick: z,
                curve: 0.0,
                ..e
            });
            Arc::make_mut(&mut lane.points).sort_by_key(|x| x.tick);
        }
    }
    let mut out: Vec<TempoPoint> = p
        .tempo
        .iter()
        .filter(|x| x.tick == 0 && at > 0 || x.tick < at || x.tick >= z)
        .cloned()
        .collect();
    let mut added = Vec::new();
    for &(t, bpm, ramp) in &m.tempo {
        out.retain(|x| x.tick != t + at);
        let x = new_tempo(p, t + at, bpm, ramp, &mut added);
        out.push(x);
    }
    if !out.iter().any(|x| x.tick == z) {
        let x = new_tempo(p, z, bz, false, &mut added);
        out.push(x);
    }
    tidy_tempo(&mut out, &added);
    p.tempo = out;
    // With changes of meter about, each stays on its tick among the music (ARR-004); a song
    // of one meter keeps its bars as they are.
    let keep = old_meters.len() > 1 || m.meters.len() > 1;
    let mut meters: Vec<MeterAt> = old_meters
        .into_iter()
        .filter(|x| x.0 == 0 && at > 0 || x.0 < at || x.0 >= z)
        .map(|x| (x, keep))
        .collect();
    meters.retain(|x| !m.meters.iter().any(|&(t, ..)| t + at == x.0.0));
    meters.extend(
        m.meters
            .iter()
            .map(|&(t, mt, line)| ((t + at, None, mt), keep && line)),
    );
    if !meters.iter().any(|x| x.0.0 == z) {
        let line = tm.bar_start(tm.to_bbt(z).0) == z;
        meters.push(((z, None, meter_z), keep && line));
    }
    lay_meters(p, meters);
    Ok(())
}

/// A copy of a clip of another song state, with ids of its own in this one.
fn clone_clip_fresh(p: &mut Project, c: &Clip) -> Clip {
    let mut copy = c.clone();
    copy.id = ClipId(p.alloc());
    if let ClipBody::Midi { notes, .. } = &c.body {
        let fresh: Vec<Note> = notes
            .iter()
            .map(|n| Note {
                id: crate::id::NoteId(p.alloc()),
                ..*n
            })
            .collect();
        copy.body = ClipBody::Midi {
            notes: Arc::new(fresh),
            transpose: c.transpose(),
        };
    }
    copy
}

// ---- sections and the loop ---------------------------------------------------------------

fn sorted_sections(p: &Project) -> Vec<Section> {
    let mut v = p.sections.clone();
    v.sort_by_key(|s| (s.start, s.id));
    v
}

fn duplicate_section(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: SectionId = a.id("id")?;
    let secs = sorted_sections(p);
    let i = secs
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| CmdError::not_found(format!("セクション {id} が見つかりません")))?;
    let s = secs[i].clone();
    let block_end = |k: usize| {
        secs.get(k + 1)
            .map_or(secs[k].start + secs[k].length, |n| n.start)
    };
    let from = s.start;
    let to = block_end(i);
    let at = match a.opt_str("at").unwrap_or("after") {
        "after" => to,
        "end" => block_end(secs.len() - 1),
        other => {
            return Err(CmdError::invalid(format!(
                "--at に `{other}` は使えません（after か end）"
            )));
        }
    };
    let m = take_material(p, from, to)?;
    insert_time(p, at, m.len)?;
    place_material(p, &m, at)?;
    let new = SectionId(p.alloc());
    p.sections.push(Section {
        id: new,
        start: at,
        ..s.clone()
    });
    p.sections.sort_by_key(|s| (s.start, s.id));
    // a copy that goes elsewhere is the copy moved there, in the same step
    if let Some(to) = a.opt_i64("index") {
        let at = p.sections.iter().position(|s| s.id == new);
        if at != Some(to as usize) {
            super::arrange::reorder_section(p, new, to as usize)?;
        }
    }
    Ok(created(&format!("セクション「{}」の複製", s.name), new))
}

fn section_board(p: &Project, id: SectionId) -> CmdResult<Clipboard> {
    if !p.sections.iter().any(|s| s.id == id) {
        return Err(CmdError::not_found(format!(
            "セクション {id} が見つかりません"
        )));
    }
    Ok(Clipboard::Section {
        source: Arc::new(p.clone()),
        id,
    })
}

/// Puts a section of `source` into the song at `at`: time made there, what lay in the
/// section laid down in it, and a section of the same name and colour over it.
pub(super) fn paste_section(
    p: &mut Project,
    source: &Project,
    id: SectionId,
    at: Tick,
) -> CmdResult<Output> {
    let s = source
        .sections
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| CmdError::not_found(format!("セクション {id} が見つかりません")))?
        .clone();
    if at < 0 {
        return Err(CmdError::range("貼り付ける位置は 0 以上にしてください"));
    }
    let m = take_material(source, s.start, s.start + s.length)?;
    // a position inside a section is moved to its end: the paste never lands in the middle of one
    let at = p
        .sections
        .iter()
        .find(|x| at > x.start && at < x.start + x.length)
        .map_or(at, |x| x.start + x.length);
    insert_time(p, at, m.len)?;
    place_material(p, &m, at)?;
    let new = SectionId(p.alloc());
    p.sections.push(Section {
        id: new,
        start: at,
        ..s.clone()
    });
    p.sections.sort_by_key(|s| (s.start, s.id));
    Ok(created(&format!("セクション「{}」の貼り付け", s.name), new))
}

fn split_section(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: SectionId = a.id("id")?;
    let at = tick(p, a, "at")?;
    let i = p
        .sections
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| CmdError::not_found(format!("セクション {id} が見つかりません")))?;
    let s = p.sections[i].clone();
    if at <= s.start || at >= s.start + s.length {
        return Err(CmdError::range("分ける位置がセクションの中にありません"));
    }
    p.sections[i].length = at - s.start;
    let new = SectionId(p.alloc());
    p.sections.push(Section {
        id: new,
        start: at,
        length: s.start + s.length - at,
        ..s
    });
    p.sections.sort_by_key(|s| (s.start, s.id));
    Ok(created("セクションの後半", new))
}

fn duplicate_loop(p: &mut Project, _: &EditCtx<'_>, _: &Args) -> CmdResult<Output> {
    let l = p
        .loop_range
        .ok_or_else(|| CmdError::not_found("ループ範囲がありません"))?;
    let m = take_material(p, l.start, l.end)?;
    place_material(p, &m, l.end)?;
    p.loop_range = Some(LoopRange {
        start: l.end,
        end: l.end + m.len,
    });
    Ok(Output::msg("ループの中身を後ろへ複製しました"))
}
