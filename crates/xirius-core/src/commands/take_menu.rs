//! Whole-take operations alongside range comping.
use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "take.activate",
            "重なっているテイクのうち、このテイクだけを再生します",
            vec![ArgDef::id("id", IdKind::Clip, "テイク").req()],
            |p, _, a| {
                let (track, stack, row) = stack(p, a)?;
                for slot in stack.slots {
                    p.clip_mut(slot.clip)?.1.muted = slot.row != row;
                }
                Ok(Output::with(
                    "テイクを選択しました",
                    json!({"track": track.to_string()}),
                ))
            },
        ),
        destroy(
            "take.clean",
            "この範囲で使っていないテイクを削除します",
            vec![ArgDef::id("id", IdKind::Clip, "テイク").req()],
            |p, _, a| {
                let (_, stack, _) = stack(p, a)?;
                // A row used anywhere in the comp stays, including its muted pieces.
                let playing: HashSet<usize> = stack
                    .slots
                    .iter()
                    .filter(|s| s.plays)
                    .map(|s| s.row)
                    .collect();
                let ids: Vec<_> = stack
                    .slots
                    .iter()
                    .filter(|s| !playing.contains(&s.row))
                    .map(|s| s.clip)
                    .collect();
                if ids.is_empty() {
                    return Err(CmdError::invalid("未使用のテイクがありません"));
                }
                for id in &ids {
                    super::clip::take_clip(p, *id)?;
                }
                Ok(Output::msg("未使用のテイクを削除しました"))
            },
        ),
        destroy(
            "take.delete",
            "テイクを削除します",
            vec![ArgDef::id("id", IdKind::Clip, "テイク").req()],
            |p, _, a| {
                let (_, stack, row) = stack(p, a)?;
                for slot in stack.slots.into_iter().filter(|s| s.row == row) {
                    super::clip::take_clip(p, slot.clip)?;
                }
                Ok(Output::msg("テイクを削除しました"))
            },
        ),
        destroy(
            "take.undo-lap",
            "重なっているテイクのうち、最後の周（いちばん下のテイク）を取り消します（取り消したテイクだけが再生されていたときは、ひとつ前のテイクを再生します）",
            vec![ArgDef::id("id", IdKind::Clip, "テイク（重なっているもののいずれか）").req()],
            |p, _, a| {
                let (_, stack, _) = stack(p, a)?;
                let last = stack.rows - 1;
                let others_play = stack.slots.iter().any(|s| s.row != last && s.plays);
                for slot in stack.slots.iter().filter(|s| s.row == last) {
                    super::clip::take_clip(p, slot.clip)?;
                }
                if !others_play {
                    for slot in stack.slots.iter().filter(|s| s.row == last - 1) {
                        p.clip_mut(slot.clip)?.1.muted = false;
                    }
                }
                Ok(Output::msg("最後の周を取り消しました"))
            },
        ),
        edit(
            "take.unpack",
            "テイクを新しいトラックに展開します（元のテイクはそのまま残ります。1 回で取り消せます）",
            vec![ArgDef::id("id", IdKind::Clip, "テイク").req()],
            |p, _, a| {
                let (track, stack, row) = stack(p, a)?;
                let src = p.track(track)?.clone();
                let mut copy = super::track::clone_track(p, &src, true);
                copy.name = format!("{} テイク {}", src.name, row + 1);
                copy.armed = false;
                copy.monitor = false;
                let new = copy.id;
                let mut made = 0;
                for slot in stack.slots.iter().filter(|s| s.row == row) {
                    if let Some(c) = src.clips.iter().find(|c| c.id == slot.clip) {
                        let mut n = clone_clip(p, c);
                        n.muted = false;
                        copy.clips.push(n);
                        made += 1;
                    }
                }
                p.tracks.insert(new, Arc::new(copy));
                let (parent, i) = p.find_node(Node::Track(track)).expect("in the column");
                p.children_mut(parent).insert(i + 1, Node::Track(new));
                Ok(Output::with(
                    format!("テイクを新しいトラックに展開しました（クリップ {made} 本）"),
                    json!({ "track": new.to_string() }),
                ))
            },
        ),
    ]
}

fn stack(p: &Project, a: &Args) -> CmdResult<(crate::id::TrackId, crate::takes::TakeStack, usize)> {
    let id: ClipId = a.id("id")?;
    let (t, _) = p.clip(id)?;
    let stack = crate::takes::stack_of(t, id)
        .ok_or_else(|| CmdError::invalid("テイクが重なっていません"))?;
    let row = stack
        .slots
        .iter()
        .find(|s| s.clip == id)
        .expect("stack contains take")
        .row;
    Ok((t.id, stack, row))
}
