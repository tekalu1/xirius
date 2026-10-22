use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "note.join",
        "同じ高さのノートを結合します",
        vec![ArgDef::ids(IdKind::NoteScope, "ノート")],
        join_notes,
    )]
}

fn join_notes(p: &mut Project, cx: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let scope = Scope::from_args(a)?;
    let mut picked = Vec::new();
    edit_notes(p, &scope, false, |n, c| {
        picked.push((c, *n));
        Ok(())
    })?;
    if picked.len() < 2 {
        return Err(CmdError::invalid("結合には 2 つ以上のノートが必要です"));
    }
    picked.sort_by_key(|(_, n)| (n.start, n.id));
    let (cid, first) = picked[0];
    if picked
        .iter()
        .any(|(c, n)| *c != cid || n.pitch != first.pitch)
    {
        return Err(CmdError::conflict(
            "同じパートの同じ高さのノートを選択してください",
        ));
    }
    let end = picked
        .iter()
        .map(|(_, n)| n.start + n.length)
        .max()
        .unwrap();
    let removed = Scope {
        notes: picked.iter().skip(1).map(|(_, n)| n.id).collect(),
        clips: HashSet::new(),
    };
    if picked
        .iter()
        .any(|(_, n)| p.original_timing.contains_key(&n.id))
    {
        let originals: Vec<_> = picked
            .iter()
            .map(|(_, n)| {
                p.original_timing
                    .get(&n.id)
                    .copied()
                    .unwrap_or(crate::editing::Timing {
                        start: n.start,
                        length: n.length,
                    })
            })
            .collect();
        let start = originals.iter().map(|o| o.start).min().unwrap();
        let end = originals.iter().map(|o| o.start + o.length).max().unwrap();
        p.original_timing.insert(
            first.id,
            crate::editing::Timing {
                start,
                length: end - start,
            },
        );
        for (_, n) in picked.iter().skip(1) {
            p.original_timing.remove(&n.id);
        }
    }
    remove_notes(p, &removed)?;
    let keep = Scope {
        notes: HashSet::from([first.id]),
        clips: HashSet::new(),
    };
    edit_notes(p, &keep, cx.author.is_ai(), |n, _| {
        n.length = end - first.start;
        Ok(())
    })?;
    Ok(Output::with(
        "ノートを結合しました",
        json!({"ids":[first.id.to_string()]}),
    ))
}
