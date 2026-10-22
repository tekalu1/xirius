//! Commands the Studio Pro key table asks for that the dictionary did not have yet
//! (KEY-001, OPS-014): moving clips by a grid or a bar, and cutting them on every grid line.
use super::*;
use crate::command::EditCtx;
use crate::id::IdKind;

/// The most pieces one grid split may make, so a tiny grid cannot flood the song.
const MAX_PIECES: usize = 4096;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "clip.nudge",
            "クリップをグリッド 1 つ分（--unit bar なら 1 小節）、前か後ろへずらします",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ").req(),
                ArgDef::choice("dir", &["left", "right"], "向き").req(),
                ArgDef::choice("unit", &["grid", "bar"], "ずらす量の単位（既定は grid）"),
                ArgDef::grid("grid", "グリッド（既定は 1/16）"),
            ],
            nudge,
        ),
        edit(
            "clip.split.grid",
            "クリップをグリッドの線ごとに分割します",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ").req(),
                ArgDef::grid("grid", "グリッド（既定は 1/4）"),
            ],
            split_grid,
        ),
        // Which things are selected is the screen's; it moves the selection one step.
        session(
            "select.step",
            "選択を矢印の向きへ 1 つ動かします（--dir start / end は先頭と最後。--extend で選択を広げる）",
            vec![
                ArgDef::choice("dir", &crate::nav::Step::NAMES, "向き").req(),
                ArgDef::flag("extend", "選択を保ったまま広げる"),
            ],
            Risk::Read,
            false,
        ),
        // The screen fills `at` with the play position when a key runs it.
        edit(
            "clip.move.cursor",
            "クリップを再生位置へ移動します（先頭のクリップを --at へ。ほかのクリップも同じ量だけ移動）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ").req(),
                ArgDef::pos("at", "再生位置").req(),
            ],
            move_to_cursor,
        ),
    ]
}

fn grid_or(p: &Project, a: &Args, default: &str) -> CmdResult<Tick> {
    if a.has("grid") {
        grid(p, a, "grid")
    } else {
        parse_grid(default, p.ppq)
    }
}

fn nudge(p: &mut Project, ctx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let sign = match a.str("dir")? {
        "left" => -1,
        _ => 1,
    };
    let first = ids
        .iter()
        .map(|id| p.clip(*id).map(|(_, c)| c.start))
        .collect::<CmdResult<Vec<_>>>()?
        .into_iter()
        .min()
        .unwrap_or(0);
    let step = if a.opt_str("unit") == Some("bar") {
        let map = p.time_map();
        let (bar, _, _) = map.to_bbt(first);
        map.bar_start(bar + 1) - map.bar_start(bar)
    } else {
        grid_or(p, a, "1/16")?
    };
    let mut moved = a.0.clone();
    moved.insert("by".into(), json!(sign * step));
    for key in ["dir", "unit", "grid"] {
        moved.remove(key);
    }
    // A clip at the start of the song cannot go earlier: it stays, and the others stay with it.
    if first + sign * step < 0 {
        return Err(CmdError::range("曲の先頭より前には移動できません"));
    }
    super::clip::move_clips(p, ctx, &Args(moved))
}

fn move_to_cursor(p: &mut Project, ctx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    super::clip::move_clips(p, ctx, a)
}

fn split_grid(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let step = grid_or(p, a, "1/4")?;
    if step < 1 {
        return Err(CmdError::range("グリッドは 1 tick 以上にしてください"));
    }
    let ids = a.require_ids::<ClipId>("クリップ")?;
    let mut made = Vec::new();
    for id in ids {
        let (start, end) = {
            let c = p.clip(id)?.1;
            (c.start, c.end())
        };
        let mut piece = id;
        let mut at = (start.div_euclid(step) + 1) * step;
        while at < end {
            if made.len() >= MAX_PIECES {
                return Err(CmdError::range(
                    "分割が多すぎます。グリッドを大きくしてください",
                ));
            }
            match super::clip::split_one(p, piece, at)? {
                Some(right) => {
                    made.push(right.to_string());
                    piece = right;
                }
                None => break,
            }
            at += step;
        }
    }
    if made.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "グリッドの線がどのクリップの中にもありません",
        ));
    }
    Ok(Output::with(
        format!("グリッドの線で {} 件に分割しました", made.len()),
        json!({ "ids": made }),
    ))
}
