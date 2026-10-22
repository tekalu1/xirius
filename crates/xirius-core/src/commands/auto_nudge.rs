use super::*;
use crate::{
    auto::AutoTarget,
    id::{IdKind, TrackId},
};
use std::collections::BTreeSet;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "auto.points.move",
        "選択したポイントを相対的に移動します",
        vec![
            ArgDef::id("track", IdKind::Track, "トラック").req(),
            ArgDef::text("target", "対象").req(),
            ArgDef::text("ticks", "ポイントの位置（カンマ区切り）").req(),
            ArgDef::int("by", "移動するtick数"),
            ArgDef::float("value-by", "値の増減"),
        ],
        |p, _, a| {
            let t: TrackId = a.str("track")?.parse().map_err(CmdError::invalid)?;
            let target: AutoTarget = a.str("target")?.parse().map_err(CmdError::invalid)?;
            let ticks: BTreeSet<Tick> = a
                .str("ticks")?
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<Tick>()
                        .map_err(|_| CmdError::invalid("ポイントの位置が不正です"))
                })
                .collect::<Result<_, _>>()?;
            let by = a.opt_i64("by").unwrap_or(0);
            let value_by = a.opt_f64("value-by").unwrap_or(0.0);
            let lane = p
                .track_mut(t)?
                .automation
                .iter_mut()
                .find(|l| l.target == target)
                .ok_or_else(|| CmdError::not_found("レーンがありません"))?;
            if ticks.is_empty()
                || ticks.len()
                    != lane
                        .points
                        .iter()
                        .filter(|p| ticks.contains(&p.tick))
                        .count()
            {
                return Err(CmdError::not_found("その位置にポイントがありません"));
            }
            let by = lane.push_points(&ticks, by, value_by);
            Ok(Output::with("ポイントを移動しました", json!({"by":by})))
        },
    )]
}
