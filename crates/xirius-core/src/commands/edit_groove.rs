use super::*;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![read(
        "view.groove.import",
        "外部グルーヴを読み込みます",
        vec![
            ArgDef::text("name", "グルーヴ名").req(),
            ArgDef::int("period", "周期（tick）")
                .req()
                .range(1.0, i64::MAX as f64),
            ArgDef::objects("hits", "[{at,velocity}]").req(),
        ],
        |_, s, _, a| {
            let period = a.i64("period")?;
            s.view.roll.external_groove = Some(crate::editing::Groove {
                name: a.str("name")?.to_string(),
                period,
                hits: groove_hits(&a.0["hits"], period)?,
            });
            Ok(Output::msg("グルーヴを読み込みました"))
        },
    )]
}

pub(super) fn groove_hits(value: &Value, period: Tick) -> CmdResult<Vec<(Tick, u8)>> {
    let list = value
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| CmdError::invalid("グルーヴにノートがありません"))?;
    let mut hits = Vec::new();
    for item in list {
        let at = item["at"]
            .as_i64()
            .ok_or_else(|| CmdError::invalid("グルーヴの位置が正しくありません"))?;
        let vel = item["velocity"]
            .as_u64()
            .ok_or_else(|| CmdError::invalid("グルーヴのベロシティが正しくありません"))?;
        if !(0..period).contains(&at) || !(1..=127).contains(&vel) {
            return Err(CmdError::range("グルーヴの値が範囲外です"));
        }
        hits.push((at, vel as u8));
    }
    hits.sort_by_key(|(at, _)| *at);
    hits.dedup_by_key(|(at, _)| *at);
    Ok(hits)
}
