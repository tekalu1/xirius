use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "note.velocity.shape",
        "ノートのベロシティに強弱を付けます",
        vec![
            ArgDef::ids(IdKind::NoteScope, "ノート"),
            ArgDef::choice(
                "shape",
                &["stronger", "softer", "equal", "crescendo", "decrescendo"],
                "強弱の型",
            )
            .req(),
            ArgDef::int("amount", "変化量")
                .default(json!(10))
                .range(0.0, 126.0),
            ArgDef::int("value", "揃えるベロシティ")
                .default(json!(100))
                .range(1.0, 127.0),
        ],
        velocity_shape,
    )]
}

fn velocity_shape(
    p: &mut Project,
    cx: &crate::command::EditCtx<'_>,
    a: &Args,
) -> CmdResult<Output> {
    let scope = Scope::from_args(a)?;
    let shape = a.str("shape")?;
    let amount = a.opt_i64("amount").unwrap_or(10);
    let value = a.opt_i64("value").unwrap_or(100);
    let offsets: std::collections::HashMap<_, _> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.id, c.start)))
        .collect();
    let mut starts = Vec::new();
    edit_notes(p, &scope, false, |n, c| {
        starts.push(n.start + offsets[&c]);
        Ok(())
    })?;
    let lo = starts.iter().copied().min().unwrap_or(0);
    let hi = starts.iter().copied().max().unwrap_or(lo);
    let count = edit_notes(p, &scope, cx.author.is_ai(), |n, c| {
        let ramp = if hi == lo {
            0
        } else {
            amount * (n.start + offsets[&c] - lo) / (hi - lo)
        };
        let v = match shape {
            "stronger" => i64::from(n.velocity) + amount,
            "softer" => i64::from(n.velocity) - amount,
            "equal" => value,
            "crescendo" => i64::from(n.velocity) + ramp,
            "decrescendo" => i64::from(n.velocity) - ramp,
            _ => return Err(CmdError::invalid("ベロシティの型が正しくありません")),
        };
        n.velocity = v.clamp(1, 127) as u8;
        Ok(())
    })?;
    Ok(Output::msg(format!(
        "ノート {count} 個のベロシティを変更しました"
    )))
}
