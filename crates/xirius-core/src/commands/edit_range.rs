use super::*;
use crate::clipboard::{ClipCopy, Clipboard};
use crate::id::IdKind;

/// The arguments that name a range: `range`, `loop`, or `start` and `end`.
fn range_args() -> [ArgDef; 4] {
    [
        ArgDef::text(
            "range",
            "loop・開始..終了・section:名前（start・end・loop の代わり）",
        ),
        ArgDef::pos("start", "範囲の開始（--range のときは不要）"),
        ArgDef::pos("end", "範囲の終了（--range のときは不要）"),
        ArgDef::flag("loop", "ループ範囲を使う（--range loop と同じ）"),
    ]
}

pub(super) fn defs() -> Vec<CommandDef> {
    let with_range = |make: fn(&'static str, &'static str, Vec<ArgDef>, EditFn) -> CommandDef,
                      name: &'static str,
                      doc: &'static str,
                      run: EditFn| {
        let mut args = vec![ArgDef::ids(IdKind::Clip, "クリップ")];
        args.extend(range_args());
        make(name, doc, args, run)
    };
    // EDT-135: taking the range out can close up the tracks behind it.
    let rippling = |name: &'static str, doc: &'static str, run: EditFn| {
        let mut def = with_range(destroy, name, doc, run);
        def.args.extend(super::ripple::args());
        def
    };
    vec![
        with_range(
            edit,
            "clip.split.range",
            "範囲の両端でクリップを分割します",
            split_range,
        ),
        rippling(
            "range.delete",
            "範囲の中身を削除します（両端でクリップを分割し、範囲の中のクリップを消す。--ripple で範囲のあったトラックの後ろを詰める）",
            delete_range,
        ),
        with_range(
            edit,
            "range.duplicate",
            "範囲の中身を、範囲の直後に複製します（両端でクリップを分割し、範囲の長さだけ後ろへ複製する）",
            duplicate_range,
        ),
        {
            let mut args = vec![ArgDef::ids(IdKind::Clip, "クリップ")];
            args.extend(range_args());
            read(
                "range.copy",
                "範囲の中身をクリップボードへコピーします（両端で切った形で入れ、曲は変えません）",
                args,
                |p, s, _, a| {
                    // The cut at the ends is made on a scratch song: a copy leaves the song alone.
                    let mut scratch = p.clone();
                    let (_, _, inside) = pieces_inside(&mut scratch, a)?;
                    let board = board_of(&scratch, &inside)?;
                    let msg = format!("範囲の中の{}をコピーしました", board.describe());
                    s.clipboard = Some(board.clone());
                    Ok(Output::with(msg, json!({ "count": board.len() })))
                },
            )
        },
        rippling(
            "range.cut",
            "範囲の中身を切り取ってクリップボードへ置きます（両端でクリップを分割し、範囲の中のクリップを消す。--ripple で範囲のあったトラックの後ろを詰める）",
            cut_range,
        ),
    ]
}

/// The range the arguments name.
fn range_of(p: &Project, a: &Args) -> CmdResult<(Tick, Tick)> {
    let (start, end) = if let Some(text) = a.opt_str("range") {
        parse_range(p, text)?
    } else if a.flag("loop") {
        parse_range(p, "loop")?
    } else {
        (tick(p, a, "start")?, tick(p, a, "end")?)
    };
    if start < 0 || end <= start {
        return Err(CmdError::range("範囲の終了は開始より後にしてください"));
    }
    Ok((start, end))
}

/// Cuts `ids` at both ends of the range. The ids grow by the pieces that were cut off.
fn cut_at_ends(p: &mut Project, ids: &mut Vec<ClipId>, start: Tick, end: Tick) -> CmdResult<()> {
    for at in [start, end] {
        for id in ids.clone() {
            if let Some(right) = super::clip::split_one(p, id, at)? {
                ids.push(right);
            }
        }
    }
    Ok(())
}

fn strings(ids: &[ClipId]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn split_range(p: &mut Project, _: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (start, end) = range_of(p, a)?;
    let mut ids = a.require_ids::<ClipId>("クリップ")?;
    cut_at_ends(p, &mut ids, start, end)?;
    Ok(Output::with(
        "範囲の両端で分割しました",
        json!({ "ids": strings(&ids) }),
    ))
}

/// Cuts at the ends and says which of the pieces lie inside the range.
fn pieces_inside(p: &mut Project, a: &Args) -> CmdResult<(Tick, Tick, Vec<ClipId>)> {
    let (start, end) = range_of(p, a)?;
    let mut ids = a.require_ids::<ClipId>("クリップ")?;
    cut_at_ends(p, &mut ids, start, end)?;
    let mut inside = Vec::new();
    for id in ids {
        let (_, c) = p.clip(id)?;
        if c.start >= start && c.start + c.length <= end {
            inside.push(id);
        }
    }
    Ok((start, end, inside))
}

/// Takes the clips `inside` out; with `--ripple`, each track a clip was named on closes up
/// the range (EDT-135). Gives how many clips moved to close up.
fn take_inside(
    p: &mut Project,
    a: &Args,
    (start, end): (Tick, Tick),
    inside: &[ClipId],
) -> CmdResult<usize> {
    let mut tracks = Vec::new();
    for id in a.require_ids::<ClipId>("クリップ")? {
        let t = p.clip(id)?.0.id;
        if !tracks.contains(&t) {
            tracks.push(t);
        }
    }
    for id in inside {
        super::clip::take_clip(p, *id)?;
    }
    match super::ripple::wanted(a) {
        Some(follow) => {
            let gone: Vec<_> = tracks.into_iter().map(|t| (t, start, end)).collect();
            super::ripple::close_up(p, &gone, follow)
        }
        None => Ok(0),
    }
}

fn delete_range(p: &mut Project, _: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (start, end, inside) = pieces_inside(p, a)?;
    let moved = take_inside(p, a, (start, end), &inside)?;
    Ok(Output::with(
        format!(
            "範囲の中のクリップ {} 件を削除しました{}",
            inside.len(),
            super::ripple::told(moved)
        ),
        json!({ "ids": strings(&inside) }),
    ))
}

/// The clips `inside` the range as a clipboard.
fn board_of(p: &Project, inside: &[ClipId]) -> CmdResult<Clipboard> {
    if inside.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "範囲の中にクリップがありません",
        ));
    }
    let mut out = Vec::new();
    for id in inside {
        let (t, c) = p.clip(*id)?;
        out.push(ClipCopy {
            track: t.id,
            clip: c.clone(),
        });
    }
    out.sort_by_key(|c| (c.clip.start, c.clip.id));
    Ok(Clipboard::Clips(out))
}

fn cut_range(p: &mut Project, _: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (start, end, inside) = pieces_inside(p, a)?;
    let board = board_of(p, &inside)?;
    let moved = take_inside(p, a, (start, end), &inside)?;
    Ok(Output::with(
        format!(
            "範囲の中のクリップ {} 件を切り取りました{}",
            inside.len(),
            super::ripple::told(moved)
        ),
        json!({ "ids": strings(&inside) }),
    )
    .keeping(board))
}

fn duplicate_range(
    p: &mut Project,
    _: &crate::command::EditCtx<'_>,
    a: &Args,
) -> CmdResult<Output> {
    let (start, end, inside) = pieces_inside(p, a)?;
    let mut made = Vec::new();
    for id in inside {
        let (tid, c) = {
            let (t, c) = p.clip(id)?;
            (t.id, c.clone())
        };
        let mut copy = super::clone_clip(p, &c);
        copy.start = c.start + (end - start);
        made.push(copy.id.to_string());
        p.track_mut(tid)?.clips.push(copy);
    }
    Ok(Output::with(
        format!("範囲の中のクリップ {} 件を複製しました", made.len()),
        json!({ "ids": made }),
    ))
}

/// `loop`, `section:NAME-or-ID` or `a..b` (bar.beat.tick or ticks) as a start and an end.
pub(crate) fn parse_range(p: &Project, text: &str) -> CmdResult<(Tick, Tick)> {
    let text = text.trim();
    if text == "loop" {
        let range = p
            .loop_range
            .ok_or_else(|| CmdError::not_found("ループ範囲がありません"))?;
        return Ok((range.start, range.end));
    }
    if let Some(key) = text.strip_prefix("section:") {
        let key = key.trim();
        let s = p
            .sections
            .iter()
            .find(|s| s.id.to_string() == key || s.name == key)
            .ok_or_else(|| CmdError::not_found(format!("セクション `{key}` がありません")))?;
        return Ok((s.start, s.start + s.length));
    }
    match text.split_once("..") {
        Some((a, b)) => {
            let map = p.time_map();
            Ok((map.parse(a.trim())?, map.parse(b.trim())?))
        }
        None => Err(CmdError::invalid(format!(
            "--range に `{text}` は使えません（loop・section:名前・開始..終了のいずれか）"
        ))),
    }
}
