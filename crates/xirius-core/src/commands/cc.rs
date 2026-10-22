//! Controller lanes inside MIDI clips (modulation, expression, sustain, pitch
//! bend, aftertouch). One command rewrites a stretch of a lane, so a pencil
//! stroke, a line, a move up or down and a delete are each one history step.

use serde_json::{Value, json};

use super::*;
use crate::command::EditCtx;
use crate::id::IdKind;
use crate::model::{CtrlKind, CtrlLane, CtrlPoint, CtrlShape};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "cc.replace",
            "コントローラーレーンの範囲を、指定したポイントで置き換えます（ポイントが空なら、その範囲を削除します）",
            vec![
                ArgDef::id("clip", IdKind::Clip, "MIDI クリップ").req(),
                ArgDef::text("lane", "レーン: cc1・cc11・cc64（CC の番号）・bend・at").req(),
                ArgDef::int("from", "範囲の開始位置（クリップの先頭からの tick）").req(),
                ArgDef::int(
                    "to",
                    "範囲の終了位置（tick。この位置のポイントも置き換えます）",
                )
                .req(),
                ArgDef::objects("points", "[{tick, value}] の配列（tick は範囲内）"),
            ],
            replace,
        ),
        edit(
            "cc.point.set",
            "コントローラーレーンのポイントの値を、まとめて同じ値にします",
            vec![
                ArgDef::id("clip", IdKind::Clip, "MIDI クリップ").req(),
                ArgDef::text("lane", "レーン: cc1・cc11・cc64（CC の番号）・bend・at").req(),
                ArgDef::text(
                    "ticks",
                    "対象のポイントの位置（クリップの先頭からの tick。カンマ区切り）",
                )
                .req(),
                ArgDef::int("value", "新しい値（レーンの範囲に収めます）").req(),
            ],
            set_points,
        ),
        edit(
            "cc.point.shape",
            "選んだ位置のポイントから次のポイントまでの形を設定します（step は階段、linear は直線。最後のポイントは対象外。CC64 は階段だけ）",
            vec![
                ArgDef::id("clip", IdKind::Clip, "MIDI クリップ").req(),
                ArgDef::text("lane", "レーン: cc1・cc11・cc64（CC の番号）・bend・at").req(),
                ArgDef::text(
                    "ticks",
                    "対象のポイントの位置（クリップの先頭からの tick。カンマ区切り）",
                )
                .req(),
                ArgDef::choice("shape", &["step", "linear"], "次のポイントまでの形").req(),
            ],
            set_shape,
        ),
    ]
}

fn replace(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let key = a.str("lane")?;
    let kind = CtrlKind::parse(key).ok_or_else(|| {
        CmdError::invalid(format!(
            "レーン `{key}` を読み取れません（cc1・bend・at など）"
        ))
    })?;
    let (from, to) = (a.i64("from")?, a.i64("to")?);
    if from < 0 || to < from {
        return Err(CmdError::range(format!(
            "範囲 {from}〜{to} が正しくありません"
        )));
    }
    let (lo, hi) = kind.range();
    let mut points = Vec::new();
    match a.0.get("points") {
        None | Some(Value::Null) => {}
        Some(Value::Array(list)) => {
            for (i, item) in list.iter().enumerate() {
                let t = item.get("tick").and_then(Value::as_i64);
                let v = item.get("value").and_then(Value::as_i64);
                let (Some(t), Some(v)) = (t, v) else {
                    return Err(CmdError::invalid(format!(
                        "{} 番目のポイントが {{tick, value}} の形式ではありません",
                        i + 1
                    )));
                };
                if t < from || t > to {
                    return Err(CmdError::range(format!(
                        "{} 番目のポイントの位置 {t} が範囲 {from}〜{to} の外です",
                        i + 1
                    )));
                }
                let shape = match item.get("shape").and_then(Value::as_str) {
                    None => CtrlShape::Step,
                    Some(name) => CtrlShape::parse(name).ok_or_else(|| {
                        CmdError::invalid(format!(
                            "{} 番目のポイントの形 `{name}` を読み取れません（step・linear）",
                            i + 1
                        ))
                    })?,
                };
                if shape == CtrlShape::Linear && kind.steps_only() {
                    return Err(CmdError::conflict(format!(
                        "{key} は階段だけで、直線にはできません"
                    )));
                }
                points.push(CtrlPoint {
                    tick: t,
                    value: v.clamp(i64::from(lo), i64::from(hi)) as i32,
                    shape,
                });
            }
        }
        Some(_) => {
            return Err(CmdError::invalid(
                "points には [{tick, value}] の配列を指定してください",
            ));
        }
    }
    put(p, cid, kind, from, to, points)
}

/// Replace the points of a lane between `from` and `to` (a lane left empty is removed).
pub(super) fn put(
    p: &mut Project,
    cid: ClipId,
    kind: CtrlKind,
    from: Tick,
    to: Tick,
    points: Vec<CtrlPoint>,
) -> CmdResult<Output> {
    let key = kind.key();
    let (_, clip) = p.clip_mut(cid)?;
    if !clip.is_midi() {
        return Err(CmdError::conflict(format!(
            "{cid} は MIDI クリップではありません"
        )));
    }
    let i = clip.controllers.iter().position(|l| l.kind == kind);
    let mut merged: Vec<CtrlPoint> = i
        .map(|i| clip.controllers[i].points.iter().copied().collect())
        .unwrap_or_default();
    merged.retain(|q| q.tick < from || q.tick > to);
    let added = points.len();
    merged.extend(points);
    // Stable: a point at the same tick as an older one replaces it.
    merged.sort_by_key(|q| q.tick);
    merged.dedup_by(|later, earlier| {
        if later.tick == earlier.tick {
            *earlier = *later;
            true
        } else {
            false
        }
    });
    match (i, merged.is_empty()) {
        (Some(i), true) => {
            clip.controllers.remove(i);
        }
        (Some(i), false) => clip.controllers[i].points = Arc::new(merged),
        (None, true) => {}
        (None, false) => {
            clip.controllers.push(CtrlLane {
                kind,
                points: Arc::new(merged),
            });
            clip.controllers.sort_by_key(|l| l.kind);
        }
    }
    Ok(Output::with(
        format!("{key} の {from}〜{to} tick を {added} 個のポイントで置き換えました"),
        json!({ "lane": key, "points": added }),
    ))
}

fn set_points(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let key = a.str("lane")?;
    let kind = CtrlKind::parse(key).ok_or_else(|| {
        CmdError::invalid(format!(
            "レーン `{key}` を読み取れません（cc1・bend・at など）"
        ))
    })?;
    let mut ticks = Vec::new();
    for part in a
        .str("ticks")?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ticks.push(part.parse::<i64>().map_err(|_| {
            CmdError::invalid(format!("ticks の `{part}` を数として読み取れません"))
        })?);
    }
    let (lo, hi) = kind.range();
    let value = a.i64("value")?.clamp(i64::from(lo), i64::from(hi)) as i32;
    let (_, clip) = p.clip_mut(cid)?;
    if !clip.is_midi() {
        return Err(CmdError::conflict(format!(
            "{cid} は MIDI クリップではありません"
        )));
    }
    let lane = clip
        .controllers
        .iter_mut()
        .find(|l| l.kind == kind)
        .ok_or_else(|| CmdError::not_found(format!("{cid} に {key} のレーンがありません")))?;
    let mut changed = 0;
    let points = Arc::make_mut(&mut lane.points);
    for q in points.iter_mut().filter(|q| ticks.contains(&q.tick)) {
        q.value = value;
        changed += 1;
    }
    if changed == 0 {
        return Err(CmdError::not_found(format!(
            "{key} に指定の位置のポイントがありません"
        )));
    }
    Ok(Output::with(
        format!("{key} の {changed} 個のポイントを {value} にしました"),
        json!({ "lane": key, "points": changed, "value": value }),
    ))
}

fn set_shape(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let key = a.str("lane")?;
    let kind = CtrlKind::parse(key).ok_or_else(|| {
        CmdError::invalid(format!(
            "レーン `{key}` を読み取れません（cc1・bend・at など）"
        ))
    })?;
    let name = a.str("shape")?;
    let shape = CtrlShape::parse(name).ok_or_else(|| {
        CmdError::invalid(format!("形 `{name}` を読み取れません（step・linear）"))
    })?;
    if shape == CtrlShape::Linear && kind.steps_only() {
        return Err(CmdError::conflict(format!(
            "{key} は階段だけで、直線にはできません"
        )));
    }
    let mut ticks = Vec::new();
    for part in a
        .str("ticks")?
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        ticks.push(part.parse::<i64>().map_err(|_| {
            CmdError::invalid(format!("ticks の `{part}` を数として読み取れません"))
        })?);
    }
    let (_, clip) = p.clip_mut(cid)?;
    if !clip.is_midi() {
        return Err(CmdError::conflict(format!(
            "{cid} は MIDI クリップではありません"
        )));
    }
    let lane = clip
        .controllers
        .iter_mut()
        .find(|l| l.kind == kind)
        .ok_or_else(|| CmdError::not_found(format!("{cid} に {key} のレーンがありません")))?;
    let last = lane.points.len().saturating_sub(1);
    let points = Arc::make_mut(&mut lane.points);
    let mut changed = 0;
    for (i, q) in points.iter_mut().enumerate() {
        if i < last && ticks.contains(&q.tick) {
            q.shape = shape;
            changed += 1;
        }
    }
    if changed == 0 {
        return Err(CmdError::not_found(format!(
            "{key} に、次のポイントがあるポイントが指定の位置にありません"
        )));
    }
    Ok(Output::with(
        format!("{key} の {changed} 個のポイントの次までの形を {name} にしました"),
        json!({ "lane": key, "points": changed, "shape": name }),
    ))
}
