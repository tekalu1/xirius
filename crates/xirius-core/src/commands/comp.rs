//! Comping (REC-006, REC-015, REC-016): picks, over a range of a track, the take that plays. The
//! takes are the clips that lie over one another (see [`crate::takes`]); the range is cut out of
//! every one of them and only the chosen take is left unmuted there. One step of the history.

use super::*;
use crate::command::EditCtx;
use crate::id::IdKind;
use crate::takes::same_take;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "take.comp",
        "指定した範囲で、選んだテイクだけが再生されるようにします（範囲の両端でテイクを分割し、重なっている他のテイクをミュートします。1 回で取り消せます）",
        vec![
            ArgDef::id(
                "take",
                IdKind::Clip,
                "再生するテイク（そのテイクのクリップのいずれか。以前の範囲で分割されていてもよい）",
            )
            .req(),
            ArgDef::pos("from", "範囲の開始位置").req(),
            ArgDef::pos("to", "範囲の終了位置").req(),
        ],
        comp,
    )]
}

fn comp(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let take: ClipId = a.id("take")?;
    let (from, to) = (tick(p, a, "from")?, tick(p, a, "to")?);
    if from >= to {
        return Err(CmdError::range(format!(
            "範囲の開始 {from} は終了 {to} より前にしてください"
        )));
    }
    let (track, i) = p
        .find_clip(take)
        .ok_or_else(|| CmdError::not_found(format!("クリップ {take} が見つかりません")))?;
    // The take as it is now: the pieces of it (it may have been cut by an earlier comp).
    let chosen: Vec<ClipId> = {
        let t = &p.tracks[&track];
        let c = &t.clips[i];
        let mut pieces: Vec<&Clip> = t
            .clips
            .iter()
            .filter(|x| same_take(c, x) && x.start < to && from < x.start + x.length)
            .collect();
        pieces.sort_by_key(|x| x.start);
        let mut at = from;
        for x in &pieces {
            if x.start > at {
                break;
            }
            at = at.max(x.start + x.length);
        }
        if at < to || pieces.first().is_none_or(|x| x.start > from) {
            return Err(CmdError::range(format!(
                "テイク {take} は範囲 {from}〜{to} を含んでいません"
            )));
        }
        pieces.iter().map(|x| x.id).collect()
    };
    if crate::takes::already_plays(&p.tracks[&track], take, from, to) {
        return Err(CmdError::conflict(format!(
            "範囲 {from}〜{to} では、すでにテイク {take} だけが再生されています"
        )));
    }
    // Every clip that has something in the range, the chosen one among them.
    let over: Vec<ClipId> = p.tracks[&track]
        .clips
        .iter()
        .filter(|c| c.start < to && from < c.start + c.length)
        .map(|c| c.id)
        .collect();
    let mut inner = Vec::new();
    for id in over {
        // The part inside the range keeps the left side's id once the end is cut off.
        let mut cur = id;
        if let Some(right) = clip::split_one(p, cur, from)? {
            cur = right;
        }
        clip::split_one(p, cur, to)?;
        inner.push((chosen.contains(&id), cur));
    }
    for (is_take, id) in &inner {
        p.clip_mut(*id)?.1.muted = !is_take;
    }
    Ok(Output::with(
        format!(
            "範囲 {from}〜{to} で {} 本のテイクのうち 1 本を再生します",
            inner.len()
        ),
        json!({ "ids": inner.iter().map(|(_, id)| id.to_string()).collect::<Vec<_>>() }),
    ))
}
