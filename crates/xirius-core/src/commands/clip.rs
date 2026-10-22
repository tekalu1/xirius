//! Clips on tracks.

use serde_json::json;

use super::*;
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::command::EditCtx;
use crate::id::{ClipId, IdKind, TrackId};
use crate::model::{Clip, CtrlLane, CtrlPoint, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    let add_args = || {
        vec![
            ArgDef::id("track", IdKind::Track, "配置先のトラック").req(),
            ArgDef::pos("at", "開始位置（小節.拍.tick または tick 数）").req(),
            ArgDef::int("length", "長さ（tick）"),
            ArgDef::float("bars", "長さ（小節。--length を省略したとき。省略時は 1）"),
            ArgDef::text("name", "名前"),
        ]
    };
    vec![
        edit(
            "clip.add.midi",
            "MIDI クリップを作成します（インストゥルメントトラックに配置）",
            add_args(),
            add_midi,
        ),
        edit(
            "clip.add.audio",
            "オーディオクリップを作成します（オーディオトラックに配置）",
            [
                add_args(),
                vec![
                    ArgDef::text(
                        "file",
                        "オーディオファイル（曲のフォルダーからの相対パスまたは絶対パス）",
                    )
                    .req(),
                ],
            ]
            .concat(),
            add_audio,
        ),
        destroy(
            "clip.delete",
            "クリップを削除します（--ripple で同じトラックの後ろのクリップを詰める）",
            [
                vec![ArgDef::ids(IdKind::Clip, "クリップ")],
                super::ripple::args().to_vec(),
            ]
            .concat(),
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                let mut gone = Vec::new();
                for id in &ids {
                    let (tid, c) = take_clip(p, *id)?;
                    gone.push((tid, c.start, c.start + c.length));
                }
                let moved = match super::ripple::wanted(a) {
                    Some(follow) => super::ripple::close_up(p, &gone, follow)?,
                    None => 0,
                };
                Ok(Output::msg(format!(
                    "クリップ {} 件を削除しました{}",
                    ids.len(),
                    super::ripple::told(moved)
                )))
            },
        ),
        edit(
            "clip.move",
            "クリップを移動します。--at で開始位置を指定、--by で相対移動、--track で別のトラックへ移動",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::pos(
                    "at",
                    "移動先の位置（先頭のクリップをこの位置へ。ほかのクリップも同じ量だけ移動）",
                )
                .alias(&["to"]),
                ArgDef::int("by", "移動量（tick。負の値で前へ）"),
                ArgDef::id("track", IdKind::Track, "移動先のトラック"),
                ArgDef::int(
                    "rows",
                    "下へ移動するトラック数（負の値で上へ。各クリップが同じトラック数だけ移動）",
                ),
                ArgDef::choice(
                    "automation",
                    &["follow", "stay"],
                    "クリップ範囲のオートメーション: follow で一緒に移動（既定）、stay でその場に残す。別のトラックへはボリューム・パン・ミュートのみ移動",
                ),
                super::overlap::clip_arg(),
            ],
            move_clips,
        ),
        edit(
            "clip.resize",
            "クリップの長さを変更します（中のノートは削除しません）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("length", "新しい長さ（tick）"),
                ArgDef::pos("end", "新しい終了位置"),
                ArgDef::int("by", "伸ばす量（tick。負の値で縮める）"),
                ArgDef::flag(
                    "adjacent",
                    "終了位置に接している次のクリップの開始位置も同じ量だけ動かす（境目だけが動き、隙間も重なりもできない）",
                ),
                super::ripple::args()[0].clone(),
                super::ripple::args()[1].clone(),
                super::overlap::clip_arg(),
            ],
            resize_clips,
        ),
        edit(
            "clip.slip",
            "クリップの枠を動かさず、中身（オーディオの読み出し位置）だけをずらします。--by が正なら中身が後ろへ動く",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("by", "中身を動かす量（tick。正で後ろへ、負で前へ）").req(),
            ],
            slip_clips,
        ),
        edit(
            "clip.trim",
            "クリップの開始位置をトリムします（終了位置は固定。中身は曲の同じ位置に残ります）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int(
                    "by",
                    "開始位置の移動量（tick。正の値で縮め、負の値で前へ伸ばす）",
                )
                .req(),
                super::overlap::clip_arg(),
            ],
            trim_clips,
        ),
        edit(
            "clip.split",
            "クリップを指定位置で分割します（左は元の ID のまま、右は新しい ID。位置をまたぐノートは両方に分かれます）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::pos("at", "分割する位置").req(),
            ],
            split_clips,
        ),
        edit(
            "clip.duplicate",
            "クリップを直後に複製します（ノートの ID は新しくなります）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("count", "複製する数")
                    .default(json!(1))
                    .range(1.0, 256.0),
                ArgDef::int(
                    "by",
                    "元の位置からのオフセット（tick。指定すると、直後ではなくこの位置に 1 つ複製）",
                ),
                ArgDef::int(
                    "rows",
                    "複製先のトラックのずれ（下へのトラック数。負の値で上へ）",
                ),
                super::overlap::clip_arg(),
            ],
            duplicate_clips,
        ),
        edit(
            "clip.quantize",
            "クリップの開始位置をグリッドにクオンタイズします（--strength で強さを指定。クリップの下のオートメーションも一緒に動きます）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::grid("grid", "グリッド（1/16・1/8t・1/4d・tick 数）")
                    .default(json!("1/16")),
                ArgDef::int(
                    "strength",
                    "クオンタイズの強さ %（100 で完全にグリッドに揃える）",
                )
                .default(json!(100))
                .range(0.0, 100.0),
            ],
            quantize_clips,
        ),
        edit(
            "clip.rename",
            "クリップの名前を変更します",
            vec![
                ArgDef::id("id", IdKind::Clip, "クリップ").req(),
                ArgDef::text("name", "新しい名前").req(),
            ],
            |p, _, a| {
                let id: ClipId = a.id("id")?;
                let name = a.str("name")?.trim().to_string();
                if name.is_empty() {
                    return Err(CmdError::invalid("名前を入力してください"));
                }
                p.clip_mut(id)?.1.name = name.clone();
                Ok(Output::msg(format!(
                    "{id} の名前を「{name}」に変更しました"
                )))
            },
        ),
        edit(
            "clip.mute",
            "クリップのミュートを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                vec![ArgDef::ids(IdKind::Clip, "クリップ")],
                on_off().to_vec(),
            ]
            .concat(),
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                let mut want = a.toggle()?;
                for id in &ids {
                    let c = p.clip_mut(*id)?.1;
                    let on = *want.get_or_insert(!c.muted);
                    c.muted = on;
                }
                Ok(Output::msg(format!(
                    "クリップ {} 件のミュートを{}",
                    ids.len(),
                    onoff_word(want.unwrap_or(false))
                )))
            },
        ),
    ]
}

fn clip_name(a: &Args, default: &str) -> String {
    a.opt_str("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(default)
        .to_string()
}

fn place(p: &mut Project, tid: TrackId, clip: Clip) -> CmdResult<()> {
    p.track_mut(tid)?.clips.push(clip);
    Ok(())
}

fn add_midi(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    if p.track(tid)?.kind != TrackKind::Instrument {
        return Err(CmdError::conflict(format!(
            "MIDI クリップはインストゥルメントトラックにのみ配置できます（{tid}）"
        )));
    }
    let start = tick(p, a, "at")?;
    let length = length_arg(p, a, start)?;
    let id = ClipId(p.alloc());
    let name = clip_name(a, "MIDI");
    let body = ClipBody::Midi {
        notes: Arc::new(Vec::new()),
        transpose: 0,
    };
    place(
        p,
        tid,
        Clip {
            id,
            name: name.clone(),
            color: None,
            start,
            length,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body,
        },
    )?;
    Ok(created(&format!("クリップ「{name}」"), id))
}

fn add_audio(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    if p.track(tid)?.kind != TrackKind::Audio {
        return Err(CmdError::conflict(format!(
            "オーディオクリップはオーディオトラックにのみ配置できます（{tid}）"
        )));
    }
    let start = tick(p, a, "at")?;
    let length = length_arg(p, a, start)?;
    let id = ClipId(p.alloc());
    let file = a.str("file")?.to_string();
    let name = clip_name(a, file.rsplit(['/', '\\']).next().unwrap_or("Audio"));
    let body = ClipBody::Audio {
        file,
        offset: 0,
        gain_db: 0.0,
        process: Default::default(),
    };
    place(
        p,
        tid,
        Clip {
            id,
            name: name.clone(),
            color: None,
            start,
            length,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body,
        },
    )?;
    Ok(created(&format!("クリップ「{name}」"), id))
}

pub(super) fn take_clip(p: &mut Project, id: ClipId) -> CmdResult<(TrackId, Clip)> {
    let (tid, i) = p
        .find_clip(id)
        .ok_or_else(|| CmdError::not_found(format!("クリップ {id} が見つかりません")))?;
    let c = p.track_mut(tid)?.clips.remove(i);
    Ok((tid, c))
}

pub(super) fn move_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let dest: Option<TrackId> = a.opt_id("track")?;
    let to = opt_tick(p, a, "at")?;
    let by = a.opt_i64("by");
    let rows = a.opt_i64("rows");
    if to.is_some() && by.is_some() {
        return Err(CmdError::invalid("--at と --by は同時に指定できません"));
    }
    if dest.is_some() && rows.is_some() {
        return Err(CmdError::invalid(
            "--track と --rows は同時に指定できません",
        ));
    }
    if to.is_none() && by.is_none() && dest.is_none() && rows.is_none() {
        return Err(CmdError::invalid(
            "--at・--by・--track・--rows のいずれかが必要です",
        ));
    }
    let mut first = Tick::MAX;
    for id in &ids {
        first = first.min(p.clip(*id)?.1.start);
    }
    let delta = match (to, by) {
        (Some(t), _) => t - first,
        (_, Some(b)) => b,
        _ => 0,
    };
    if first + delta < 0 {
        return Err(CmdError::range("曲の先頭より前には移動できません"));
    }
    if let Some(d) = dest {
        let kind = p.track(d)?.kind;
        for id in &ids {
            let ok = matches!(
                (&p.clip(*id)?.1.body, kind),
                (ClipBody::Midi { .. }, TrackKind::Instrument)
                    | (ClipBody::Audio { .. }, TrackKind::Audio)
            );
            if !ok {
                return Err(CmdError::conflict(format!(
                    "クリップ {id} は {d} に配置できない種類です"
                )));
            }
        }
    }
    let mut dests = Vec::new();
    for id in &ids {
        dests.push(match rows.filter(|r| *r != 0) {
            Some(r) => Some(row_below(p, *id, r)?),
            None => dest,
        });
    }
    if a.opt_str("automation") != Some("stay") {
        let mut moves = Vec::new();
        for (id, dest) in ids.iter().zip(&dests) {
            let (t, c) = p.clip(*id)?;
            moves.push((t.id, c.start, c.start + c.length, dest.unwrap_or(t.id)));
        }
        carry_automation(p, &moves, delta)?;
    }
    for (id, dest) in ids.iter().zip(dests) {
        match dest {
            Some(d) => {
                let (_, mut c) = take_clip(p, *id)?;
                c.start += delta;
                place(p, d, c)?;
            }
            None => p.clip_mut(*id)?.1.start += delta,
        }
    }
    Ok(Output::msg(format!(
        "クリップ {} 件を移動しました",
        ids.len()
    )))
}

/// SET-001: snaps the start of each clip to the grid; the automation under the clip goes with it.
fn quantize_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let g = grid(p, a, "grid")?;
    let strength = a.opt_i64("strength").unwrap_or(100) as u32;
    let mut moves = Vec::new();
    for id in &ids {
        let (t, c) = p.clip(*id)?;
        let to = crate::time::quantize_tick(c.start, g, strength).max(0);
        moves.push((*id, t.id, c.start, c.start + c.length, to - c.start));
    }
    let mut moved = 0;
    for (_, tid, start, end, delta) in &moves {
        if *delta != 0 {
            carry_automation(p, &[(*tid, *start, *end, *tid)], *delta)?;
        }
    }
    for (id, _, _, _, delta) in &moves {
        if *delta != 0 {
            p.clip_mut(*id)?.1.start += delta;
            moved += 1;
        }
    }
    Ok(Output::msg(format!(
        "クリップ {} 件中 {moved} 件を {g} tick のグリッドにクオンタイズしました",
        ids.len()
    )))
}

/// AUT-012: the automation under each moved clip goes with it. Every stretch
/// is lifted before any is laid down, so clips moved past each other do not
/// pick up what another just brought. Only lanes with a point under the clip
/// move; to another track only volume, pan and mute go (sends and plug-ins
/// are the track's own).
pub(super) fn carry_automation(
    p: &mut Project,
    moves: &[(TrackId, Tick, Tick, TrackId)],
    delta: Tick,
) -> CmdResult<()> {
    if delta == 0 && moves.iter().all(|m| m.0 == m.3) {
        return Ok(());
    }
    let mut lifted = Vec::new();
    for &(src, start, end, dst) in moves {
        for l in &mut p.track_mut(src)?.automation {
            let shared = matches!(
                l.target,
                AutoTarget::Volume | AutoTarget::Pan | AutoTarget::Mute
            );
            if (src != dst && !shared) || !l.points.iter().any(|x| (start..end).contains(&x.tick)) {
                continue;
            }
            let pts = l.take(start, end);
            lifted.push((dst, l.target.clone(), start + delta, end + delta, pts));
        }
    }
    for (dst, target, from, to, pts) in lifted {
        let t = p.track_mut(dst)?;
        if !t.automation.iter().any(|l| l.target == target) {
            t.automation.push(AutoLane::on_track(target.clone(), t));
        }
        if let Some(l) = t.automation.iter_mut().find(|l| l.target == target) {
            l.hidden = false;
            l.splice(
                from,
                to,
                pts.into_iter().map(|x| AutoPoint {
                    tick: x.tick + delta,
                    ..x
                }),
            );
        }
    }
    Ok(())
}

fn resize_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let (length, end, by) = (a.opt_i64("length"), opt_tick(p, a, "end")?, a.opt_i64("by"));
    if usize::from(length.is_some()) + usize::from(end.is_some()) + usize::from(by.is_some()) != 1 {
        return Err(CmdError::invalid(
            "--length・--end・--by のいずれか 1 つだけを指定してください",
        ));
    }
    let ripple = super::ripple::wanted(a);
    // Rippling moves the next clip as a whole, so its start is not trimmed as well.
    let adjacent = a.flag("adjacent") && ripple.is_none();
    let mut resized = Vec::new();
    for id in &ids {
        let (tid, c) = p.clip_mut(*id)?;
        let old_end = c.start + c.length;
        let new = match (length, end, by) {
            (Some(l), _, _) => l,
            (_, Some(e), _) => e - c.start,
            (_, _, Some(b)) => c.length + b,
            _ => unreachable!("checked above"),
        };
        if new < 1 {
            return Err(CmdError::range(format!(
                "クリップ {id} の長さが 1 tick 未満になります"
            )));
        }
        let delta = new - c.length;
        c.length = new;
        resized.push((*id, old_end, delta));
        if adjacent && delta != 0 {
            // The clip that starts where this one ended moves its start with it.
            let next = Arc::make_mut(p.tracks.get_mut(&tid).expect("found above"))
                .clips
                .iter_mut()
                .find(|x| x.id != *id && !ids.contains(&x.id) && x.start == old_end)
                .ok_or_else(|| {
                    CmdError::conflict(format!(
                        "クリップ {id} の終了位置に接しているクリップがありません"
                    ))
                })?;
            let nid = next.id;
            trim_clip(next, nid, delta)?;
            p.fx_trimmed(nid, delta);
        }
    }
    let moved = match ripple {
        Some(follow) => super::ripple::follow_ends(p, &resized, follow)?,
        None => 0,
    };
    Ok(Output::msg(format!(
        "クリップ {} 件の長さを変更しました{}",
        ids.len(),
        super::ripple::told(moved)
    )))
}

/// Moves what a clip plays without moving the clip: the offset into the file
/// changes. A MIDI clip has no file to slip over, so it is refused.
fn slip_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let by = a.i64("by")?;
    for id in &ids {
        let c = p.clip_mut(*id)?.1;
        let ClipBody::Audio { offset, .. } = &mut c.body else {
            return Err(CmdError::invalid(format!(
                "クリップ {id} は MIDI です。スリップはオーディオのクリップだけに使えます"
            )));
        };
        if *offset - by < 0 {
            return Err(CmdError::range(format!(
                "クリップ {id} はオーディオの先頭より前へは動かせません"
            )));
        }
        *offset -= by;
    }
    Ok(Output::msg(format!(
        "クリップ {} 件の中身をずらしました",
        ids.len()
    )))
}

fn split_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let at = tick(p, a, "at")?;
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let mut made = Vec::new();
    for id in &ids {
        if let Some(right) = split_one(p, *id, at)? {
            made.push(right.to_string());
        }
    }
    if made.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "分割する位置がどのクリップにも含まれていません",
        ));
    }
    Ok(Output::with(
        format!("クリップ {} 件を分割しました", made.len()),
        json!({ "ids": made }),
    ))
}

/// Cuts one clip at `at`; gives the clip made to its right, or `None` when `at` is not inside it.
pub(super) fn split_one(p: &mut Project, id: ClipId, at: Tick) -> CmdResult<Option<ClipId>> {
    let (tid, i) = p
        .find_clip(id)
        .ok_or_else(|| CmdError::not_found(format!("クリップ {id} が見つかりません")))?;
    let (start, length) = {
        let c = &p.tracks[&tid].clips[i];
        (c.start, c.length)
    };
    if at <= start || at >= start + length {
        return Ok(None);
    }
    let rel = at - start;
    let mut next = p.next_id;
    let originals = p.original_timing.clone();
    let mut timing_changes = Vec::new();
    let right_id = ClipId(next);
    next += 1;
    let t = p.track_mut(tid)?;
    let c = &mut t.clips[i];
    let right_controllers = split_controllers(&mut c.controllers, rel);
    let right_body = match &mut c.body {
        ClipBody::Midi { notes, transpose } => {
            let (mut left, mut right) = (Vec::new(), Vec::new());
            for n in notes.iter() {
                if n.start >= rel {
                    if let Some(o) = originals.get(&n.id) {
                        timing_changes.push((
                            n.id,
                            crate::editing::Timing {
                                start: (o.start - rel).max(0),
                                length: o.length,
                            },
                        ));
                    }
                    right.push(Note {
                        start: n.start - rel,
                        ..*n
                    });
                } else if n.start + n.length > rel {
                    if let Some(o) = originals.get(&n.id) {
                        timing_changes.push((
                            n.id,
                            crate::editing::Timing {
                                start: o.start.min(rel - 1),
                                length: (rel - o.start).clamp(1, o.length),
                            },
                        ));
                        timing_changes.push((
                            NoteId(next),
                            crate::editing::Timing {
                                start: (o.start - rel).max(0),
                                length: (o.start + o.length - rel).max(1),
                            },
                        ));
                    }
                    left.push(Note {
                        length: rel - n.start,
                        ..*n
                    });
                    right.push(Note {
                        id: NoteId(next),
                        start: 0,
                        length: n.start + n.length - rel,
                        ..*n
                    });
                    next += 1;
                } else {
                    left.push(*n);
                }
            }
            *notes = Arc::new(left);
            ClipBody::Midi {
                notes: Arc::new(right),
                transpose: *transpose,
            }
        }
        ClipBody::Audio {
            file,
            offset,
            gain_db,
            process,
        } => ClipBody::Audio {
            file: file.clone(),
            offset: *offset + rel,
            gain_db: *gain_db,
            process: *process,
        },
    };
    let right = Clip {
        id: right_id,
        name: c.name.clone(),
        color: c.color.clone(),
        start: at,
        length: length - rel,
        muted: c.muted,
        take: c.take,
        controllers: right_controllers,
        body: right_body,
    };
    c.length = rel;
    t.clips.insert(i + 1, right);
    p.next_id = next;
    p.original_timing.extend(timing_changes);
    p.fx_split(id, right_id, rel, rel);
    Ok(Some(right_id))
}

/// Cuts the controller lanes at `rel`: what is before stays, what is after is
/// returned, measured from the cut. The value in force at the cut is repeated
/// at the start of the right part, so the right clip begins where the left left off.
fn split_controllers(lanes: &mut Vec<CtrlLane>, rel: Tick) -> Vec<CtrlLane> {
    let mut right = Vec::new();
    let mut left = Vec::new();
    for lane in lanes.drain(..) {
        let before: Vec<CtrlPoint> = lane
            .points
            .iter()
            .filter(|p| p.tick < rel)
            .copied()
            .collect();
        let mut after: Vec<CtrlPoint> = lane
            .points
            .iter()
            .filter(|p| p.tick >= rel)
            .map(|p| CtrlPoint {
                tick: p.tick - rel,
                ..*p
            })
            .collect();
        if let Some(last) = before.last()
            && after.first().is_none_or(|p| p.tick > 0)
        {
            after.insert(0, CtrlPoint::new(0, last.value));
        }
        if !before.is_empty() {
            left.push(CtrlLane {
                kind: lane.kind,
                points: Arc::new(before),
            });
        }
        if !after.is_empty() {
            right.push(CtrlLane {
                kind: lane.kind,
                points: Arc::new(after),
            });
        }
    }
    *lanes = left;
    right
}

fn duplicate_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let count = a.opt_i64("count").unwrap_or(1).max(1);
    let mut made = Vec::new();
    if a.has("by") || a.has("rows") {
        let by = a.opt_i64("by").unwrap_or(0);
        let rows = a.opt_i64("rows").unwrap_or(0);
        let mut copies = Vec::new();
        for id in &ids {
            let (tid, c) = {
                let (t, c) = p.clip(*id)?;
                (t.id, c.clone())
            };
            if c.start + by < 0 {
                return Err(CmdError::range("曲の先頭より前には配置できません"));
            }
            let dest = if rows == 0 {
                tid
            } else {
                row_below(p, *id, rows)?
            };
            copies.push((dest, c));
        }
        for (dest, c) in copies {
            let mut copy = clone_clip(p, &c);
            copy.start = c.start + by;
            made.push(copy.id.to_string());
            place(p, dest, copy)?;
        }
        return Ok(Output::with(
            format!("クリップ {} 件を複製しました", made.len()),
            json!({ "ids": made }),
        ));
    }
    for id in &ids {
        let (tid, c) = {
            let (t, c) = p.clip(*id)?;
            (t.id, c.clone())
        };
        for k in 1..=count {
            let mut copy = clone_clip(p, &c);
            copy.start = c.start + c.length * k;
            made.push(copy.id.to_string());
            place(p, tid, copy)?;
        }
    }
    Ok(Output::with(
        format!("クリップ {} 件を複製しました", made.len()),
        json!({ "ids": made }),
    ))
}

/// The track `rows` places below the clip's track in the column (negative is
/// above, folders are skipped), which must take the clip's kind.
fn row_below(p: &Project, id: ClipId, rows: i64) -> CmdResult<TrackId> {
    let (t, c) = p.clip(id)?;
    let order: Vec<TrackId> = column_nodes(p)
        .into_iter()
        .filter_map(|n| match n {
            Node::Track(t) => Some(t),
            Node::Folder(_) => None,
        })
        .collect();
    let at = order.iter().position(|x| *x == t.id).unwrap_or(0) as i64 + rows;
    let dest = usize::try_from(at)
        .ok()
        .and_then(|i| order.get(i))
        .copied()
        .ok_or_else(|| {
            CmdError::range(format!(
                "クリップ {id} の {rows} トラック先にトラックがありません"
            ))
        })?;
    let fits = matches!(
        (&c.body, p.track(dest)?.kind),
        (ClipBody::Midi { .. }, TrackKind::Instrument) | (ClipBody::Audio { .. }, TrackKind::Audio)
    );
    if !fits {
        return Err(CmdError::conflict(format!(
            "クリップ {id} は {dest} に配置できない種類です"
        )));
    }
    Ok(dest)
}

pub(super) fn trim_clip(c: &mut Clip, id: ClipId, by: Tick) -> CmdResult<()> {
    if c.length - by < 1 {
        return Err(CmdError::range(format!(
            "クリップ {id} の長さが 1 tick 未満になります"
        )));
    }
    if c.start + by < 0 {
        return Err(CmdError::range("曲の先頭より前には伸ばせません"));
    }
    if let ClipBody::Audio { offset, .. } = &c.body
        && *offset + by < 0
    {
        return Err(CmdError::range(format!(
            "クリップ {id} はオーディオの先頭より前には伸ばせません"
        )));
    }
    c.start += by;
    c.length -= by;
    match &mut c.body {
        ClipBody::Audio { offset, .. } => *offset += by,
        ClipBody::Midi { notes, .. } => {
            let kept: Vec<Note> = notes
                .iter()
                .filter(|n| n.start + n.length - by > 0)
                .map(|n| {
                    let start = n.start - by;
                    Note {
                        start: start.max(0),
                        length: n.length + start.min(0),
                        ..*n
                    }
                })
                .collect();
            *notes = Arc::new(kept);
        }
    }
    for lane in &mut c.controllers {
        let held = lane.points.iter().rev().find(|x| x.tick - by < 0).copied();
        let mut pts: Vec<CtrlPoint> = lane
            .points
            .iter()
            .filter(|x| x.tick - by >= 0)
            .map(|x| CtrlPoint {
                tick: x.tick - by,
                ..*x
            })
            .collect();
        if let Some(h) = held
            && pts.first().is_none_or(|x| x.tick > 0)
        {
            pts.insert(0, CtrlPoint::new(0, h.value));
        }
        lane.points = Arc::new(pts);
    }
    c.controllers.retain(|l| !l.points.is_empty());
    Ok(())
}

/// Moves the left edge by `by` and keeps the content where it was in the song:
/// notes and controller points shift the other way, an audio clip starts later
/// (or earlier) in its file. Notes left wholly before the new start are dropped;
/// a note across it is cut there.
fn trim_clips(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let by = a.i64("by")?;
    for id in &ids {
        let c = p.clip_mut(*id)?.1;
        trim_clip(c, *id, by)?;
        p.fx_trimmed(*id, by);
    }
    Ok(Output::msg(format!(
        "クリップ {} 件の開始位置をトリムしました",
        ids.len()
    )))
}
