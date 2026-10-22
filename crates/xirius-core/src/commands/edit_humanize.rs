use super::*;
use crate::id::IdKind;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "note.humanize",
        "ノートのタイミングとベロシティに揺らぎを付けます",
        vec![
            ArgDef::ids(IdKind::NoteScope, "ノート"),
            ArgDef::int("timing", "タイミングの最大変化量（tick）")
                .default(json!(20))
                .range(0.0, 9600.0),
            ArgDef::int("velocity", "ベロシティの最大変化量")
                .default(json!(5))
                .range(0.0, 126.0),
            ArgDef::int("seed", "乱数の種").default(json!(1)),
            ArgDef::flag("less", "変化量を半分にする"),
        ],
        humanize,
    )]
}

fn humanize(p: &mut Project, cx: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let divisor = if a.flag("less") { 2 } else { 1 };
    let timing = a.opt_i64("timing").unwrap_or(20) / divisor;
    let velocity = a.opt_i64("velocity").unwrap_or(5) / divisor;
    let mut state = a.opt_i64("seed").unwrap_or(1) as u64;
    let mut random = |range: i64| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 32) % (2 * range + 1) as u64) as i64 - range
    };
    let count = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
        n.start = (n.start + random(timing)).max(0);
        n.velocity = (i64::from(n.velocity) + random(velocity)).clamp(1, 127) as u8;
        Ok(())
    })?;
    Ok(Output::msg(format!(
        "ノート {count} 個をヒューマナイズしました"
    )))
}
