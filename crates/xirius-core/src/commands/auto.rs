//! Automation lanes and their points (AUT-001, AUT-004, AUT-005, AUT-009,
//! AUT-011, AUT-016).

use serde_json::{Value, json};

use super::*;
use crate::auto::{AutoLane, AutoMode, AutoPoint, AutoTarget};
use crate::command::EditCtx;
use crate::id::{IdKind, TrackId};
use crate::model::Track;

const TARGET_HELP: &str =
    "対象（volume・pan・mute・send:<センドの ID>・param:<プラグインの ID>:<パラメーター名>）";

pub(super) fn defs() -> Vec<CommandDef> {
    let lane = || {
        vec![
            ArgDef::id("track", IdKind::Track, "トラック").req(),
            ArgDef::text("target", TARGET_HELP).req(),
        ]
    };
    let point = || {
        [
            lane(),
            vec![
                ArgDef::int("index", "ポイントの番号（0 から、時間順）")
                    .req()
                    .range(0.0, 1e9),
            ],
        ]
        .concat()
    };
    vec![
        edit(
            "auto.lane.add",
            "オートメーションレーンを追加します（非表示のレーンは再表示します）",
            lane(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let t = p.track_mut(tid)?;
                if let Some(l) = t.automation.iter_mut().find(|l| l.target == target) {
                    if !l.hidden {
                        return Err(CmdError::conflict(format!(
                            "{tid} には {target} のレーンがすでにあります"
                        )));
                    }
                    l.hidden = false;
                    return Ok(Output::msg(format!("{tid} の {target} のレーンを表示しました")));
                }
                if !target.exists_on(t) {
                    return Err(CmdError::not_found(format!("{tid} に {target} が見つかりません")));
                }
                t.automation.push(AutoLane::on_track(target.clone(),t));
                Ok(Output::msg(format!("{tid} に {target} のレーンを追加しました")))
            },
        ),
        edit(
            "auto.lane.hide",
            "レーンを非表示にします（ポイントは残り、再生にも反映されます）",
            lane(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                if l.hidden {
                    return Err(CmdError::new(
                        crate::error::ErrorCode::NothingToDo,
                        "すでに非表示です",
                    ));
                }
                l.hidden = true;
                Ok(Output::msg(format!("{tid} の {target} のレーンを非表示にしました")))
            },
        ),
        destroy(
            "auto.lane.remove",
            "レーンをポイントごと削除します",
            lane(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let t = p.track_mut(tid)?;
                let before = t.automation.len();
                t.automation.retain(|l| l.target != target);
                if t.automation.len() == before {
                    return Err(no_lane(tid, &target));
                }
                Ok(Output::msg(format!("{tid} の {target} のレーンを削除しました")))
            },
        ),
        edit(
            "auto.lane.target",
            "レーンの対象を変更します（ポイントの値はそのまま）",
            [lane(), vec![ArgDef::text("to", "新しい対象").req()]].concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let to: AutoTarget = a.id("to")?;
                let t = p.track_mut(tid)?;
                if !to.exists_on(t) {
                    return Err(CmdError::not_found(format!("{tid} に {to} が見つかりません")));
                }
                if t.automation.iter().any(|l| l.target == to) {
                    return Err(CmdError::conflict(format!(
                        "{tid} には {to} のレーンがすでにあります"
                    )));
                }
                let suspended = matches!(to, AutoTarget::Send(id) if t.sends.iter().any(|s|s.id==id && s.muted_level.is_some()));
                let l = lane_mut(t, &target)?;
                let pts: Vec<AutoPoint> = l
                    .points
                    .iter()
                    .map(|x| AutoPoint {
                        value: to.from_norm(target.to_norm(x.value)),
                        ..*x
                    })
                    .collect();
                l.points = Arc::new(pts);
                l.target = to.clone();
                l.suspended = suspended;
                Ok(Output::msg(format!(
                    "{tid} の {target} のレーンを {to} に変更しました"
                )))
            },
        ),
        edit(
            "auto.mode",
            "オートメーションモードを設定します（off・read・touch・latch・write）",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::choice(
                    "mode",
                    &["off", "read", "touch", "latch", "write"],
                    "モード",
                )
                .req(),
            ],
            |p, _, a| {
                let ids = a.require_ids::<TrackId>("トラック")?;
                let mode = AutoMode::parse(a.str("mode")?)
                    .ok_or_else(|| CmdError::invalid("--mode には off・read・touch・latch・write のいずれかを指定してください"))?;
                for id in &ids {
                    p.track_mut(*id)?.auto_mode = mode;
                }
                Ok(Output::msg(format!(
                    "{} トラックを {} に設定しました",
                    ids.len(),
                    mode.name()
                )))
            },
        ),
        edit(
            "auto.point.add",
            "ポイントを追加します（レーンが無ければ追加し、同じ位置のポイントは値を変更します）",
            [
                lane(),
                vec![
                    ArgDef::pos("at", "位置").req(),
                    ArgDef::float("value", "値（dB・-1〜1・0/1・パラメーターの値）").req(),
                    ArgDef::float("curve", "次のポイントまでのカーブ（-1〜1）").range(-1.0, 1.0),
                ],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let at = tick(p, a, "at")?;
                let value = in_range(&target, a.f64("value")?)?;
                let curve = a.opt_f64("curve").unwrap_or(0.0);
                let t = p.track_mut(tid)?;
                if !t.automation.iter().any(|l| l.target == target) {
                    if !target.exists_on(t) {
                        return Err(CmdError::not_found(format!("{tid} に {target} が見つかりません")));
                    }
                    t.automation.push(AutoLane::on_track(target.clone(),t));
                }
                let l = lane_mut(t, &target)?;
                l.hidden = false;
                let pts = Arc::make_mut(&mut l.points);
                let i = pts.partition_point(|x| x.tick < at);
                match pts.get_mut(i) {
                    Some(x) if x.tick == at => {
                        x.value = value;
                        x.curve = curve;
                    }
                    _ => pts.insert(
                        i,
                        AutoPoint {
                            curve,
                            ..AutoPoint::new(at, value)
                        },
                    ),
                }
                Ok(Output::with(
                    format!("{tid} の {target} にポイントを追加しました"),
                    json!({ "index": i }),
                ))
            },
        ),
        edit(
            "auto.point.move",
            "ポイントを移動します（隣のポイントを押しのけます）",
            [
                point(),
                vec![
                    ArgDef::pos("at", "新しい位置"),
                    ArgDef::float("value", "新しい値"),
                ],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let index = a.i64("index")? as usize;
                let at = opt_tick(p, a, "at")?;
                let value = a.opt_f64("value");
                if at.is_none() && value.is_none() {
                    return Err(CmdError::invalid("--at か --value が必要です"));
                }
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                let original = *l.points.get(index).ok_or_else(|| CmdError::not_found(format!("ポイント {index} が見つかりません")))?;
                l.push_point(index, at.unwrap_or(original.tick), value.unwrap_or(original.value));
                Ok(Output::msg(format!(
                    "{tid} の {target} のポイント {index} を移動しました"
                )))
            },
        ),
        destroy("auto.point.delete", "ポイントを削除します", point(), |p, _, a| {
            let (tid, target) = lane_args(a)?;
            let index = a.i64("index")? as usize;
            let l = lane_mut(p.track_mut(tid)?, &target)?;
            if index >= l.points.len() {
                return Err(CmdError::not_found(format!("ポイント {index} が見つかりません")));
            }
            Arc::make_mut(&mut l.points).remove(index);
            Ok(Output::msg(format!(
                "{tid} の {target} のポイント {index} を削除しました"
            )))
        }),
        edit(
            "auto.point.set",
            "選んだ位置のポイントの値をそろえて設定します（位置は動きません）",
            [
                lane(),
                vec![
                    ArgDef::text("ticks", "ポイントの位置（`960,1920` のように、カンマで区切る）").req(),
                    ArgDef::float("value", "値").req(),
                ],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let ticks = point_ticks(a)?;
                let value = in_range(&target, a.f64("value")?)?;
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                let pts = Arc::make_mut(&mut l.points);
                let mut n = 0;
                for x in pts.iter_mut().filter(|x| ticks.contains(&x.tick)) {
                    x.value = value;
                    n += 1;
                }
                if n == 0 {
                    return Err(CmdError::not_found("その位置にポイントがありません"));
                }
                Ok(Output::msg(format!(
                    "{tid} の {target} のポイント {n} 個の値を設定しました"
                )))
            },
        ),
        edit(
            "auto.point.shape",
            "選んだ位置のポイントから次のポイントまでのカーブを設定します（0 で直線。--step で階段。最後のポイントは対象外）",
            [
                lane(),
                vec![
                    ArgDef::text("ticks", "ポイントの位置（`960,1920` のように、カンマで区切る）").req(),
                    ArgDef::float("curve", "カーブ（-1〜1）")
                        .req()
                        .range(-1.0, 1.0),
                    ArgDef::flag("step", "階段にします"),
                ],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let ticks = point_ticks(a)?;
                let curve = a.f64("curve")?;
                let step = a.flag("step");
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                let last = l.points.len().saturating_sub(1);
                let pts = Arc::make_mut(&mut l.points);
                let mut n = 0;
                for (i, x) in pts.iter_mut().enumerate() {
                    if i < last && ticks.contains(&x.tick) {
                        x.curve = curve;
                        x.step = step;
                        n += 1;
                    }
                }
                if n == 0 {
                    return Err(CmdError::not_found(
                        "次のポイントがあるポイントが、その位置にありません",
                    ));
                }
                Ok(Output::msg(format!(
                    "{tid} の {target} のカーブ {n} 個を変更しました"
                )))
            },
        ),
        destroy(
            "auto.clear",
            "レーンのポイントをまとめて消します（--range loop でループの中だけ）",
            [
                lane(),
                vec![ArgDef::choice(
                    "range",
                    &["loop"],
                    "範囲（省略時はすべて）",
                )],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let in_loop = match a.opt_str("range") {
                    None => None,
                    Some("loop") => Some(p.loop_range.ok_or_else(|| {
                        CmdError::not_found("ループが設定されていません")
                    })?),
                    Some(_) => return Err(CmdError::invalid("--range には loop を指定してください")),
                };
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                let before = l.points.len();
                let pts = Arc::make_mut(&mut l.points);
                match in_loop {
                    Some(r) => pts.retain(|x| x.tick < r.start || x.tick >= r.end),
                    None => pts.clear(),
                }
                let gone = before - pts.len();
                if gone == 0 {
                    return Err(CmdError::new(
                        crate::error::ErrorCode::NothingToDo,
                        "消すポイントがありません",
                    ));
                }
                Ok(Output::msg(format!(
                    "{tid} の {target} のポイントを {gone} 個消しました"
                )))
            },
        ),
        edit(
            "auto.segment.shape",
            "ポイントから次のポイントまでのカーブを設定します（-1 で下に、1 で上に膨らみます。0 で直線。--step で階段）",
            [
                point(),
                vec![
                    ArgDef::float("curve", "カーブ（-1〜1）")
                        .req()
                        .range(-1.0, 1.0),
                    ArgDef::flag(
                        "step",
                        "階段にします（次のポイントまで値を保ち、そこで切り替わります）",
                    ),
                ],
            ]
            .concat(),
            |p, _, a| {
                let (tid, target) = lane_args(a)?;
                let index = a.i64("index")? as usize;
                let curve = a.f64("curve")?;
                let step = a.flag("step");
                let l = lane_mut(p.track_mut(tid)?, &target)?;
                if index + 1 >= l.points.len() {
                    return Err(CmdError::not_found(format!("ポイント {index} の後ろにカーブがありません")));
                }
                let x = &mut Arc::make_mut(&mut l.points)[index];
                x.curve = curve;
                x.step = step;
                Ok(Output::msg(format!(
                    "{tid} の {target} のカーブ {index} を変更しました"
                )))
            },
        ),
        edit(
            "auto.write",
            "再生中に書き込んだポイントで、レーンの範囲を置き換えます（1 回の再生を 1 回で取り消せます。レーンが無ければ追加します）",
            vec![
                ArgDef::objects(
                    "writes",
                    "[{track, target, from, to, points: [{tick, value}], fallback}] の配列（from〜to のポイントを points で置き換えます。fallback は、新しいレーンが範囲の外で保つ値で、省くとトラックの今の値です）",
                )
                .req(),
            ],
            write,
        ),
    ]
}

/// AUT-002: what one pass of playing wrote, laid into the lanes in one step.
fn write(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let Some(Value::Array(list)) = a.0.get("writes") else {
        return Err(CmdError::invalid(
            "writes には [{track, target, from, to, points}] の配列を指定してください",
        ));
    };
    let mut planned = Vec::new();
    for (i, w) in list.iter().enumerate() {
        let n = i + 1;
        let field = |k: &str| {
            w.get(k)
                .ok_or_else(|| CmdError::invalid(format!("{n} 番目に {k} がありません")))
        };
        let tid: TrackId = field("track")?
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| CmdError::invalid(format!("{n} 番目の track を読み取れません")))?;
        let target: AutoTarget = field("target")?
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| CmdError::invalid(format!("{n} 番目の target を読み取れません")))?;
        let (from, to) = (field("from")?.as_i64(), field("to")?.as_i64());
        let (Some(from), Some(to)) = (from, to) else {
            return Err(CmdError::invalid(format!(
                "{n} 番目の from・to が数値ではありません"
            )));
        };
        if from < 0 || to < from {
            return Err(CmdError::range(format!(
                "{n} 番目の範囲 {from}〜{to} が正しくありません"
            )));
        }
        let (lo, hi) = target.range();
        let mut pts = Vec::new();
        for x in field("points")?.as_array().into_iter().flatten() {
            let (Some(tick), Some(value)) = (
                x.get("tick").and_then(Value::as_i64),
                x.get("value").and_then(Value::as_f64),
            ) else {
                return Err(CmdError::invalid(format!(
                    "{n} 番目のポイントが {{tick, value}} の形式ではありません"
                )));
            };
            if !(from..=to).contains(&tick) {
                return Err(CmdError::range(format!(
                    "{n} 番目のポイントの位置 {tick} が範囲 {from}〜{to} の外です"
                )));
            }
            pts.push(AutoPoint::new(tick, value.clamp(lo, hi)));
        }
        if !target.exists_on(p.track(tid)?) {
            return Err(CmdError::not_found(format!(
                "{tid} に {target} が見つかりません"
            )));
        }
        // What the channel had when play started: the song's own value may
        // have followed the control during the pass (a plug-in's editor).
        let fallback = match w.get("fallback") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_f64().ok_or_else(|| {
                CmdError::invalid(format!("{n} 番目の fallback が数値ではありません"))
            })?),
        };
        planned.push((
            tid,
            target,
            from,
            to,
            pts,
            fallback.map(|v| v.clamp(lo, hi)),
        ));
    }
    if planned.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "書き込まれたポイントがありません",
        ));
    }
    let lanes = planned.len();
    for (tid, target, from, to, pts, given) in planned {
        let t = p.track_mut(tid)?;
        if !t.automation.iter().any(|l| l.target == target) {
            t.automation.push(AutoLane::on_track(target.clone(), t));
        }
        let fallback = given.or_else(|| target.stored_value(t));
        let l = lane_mut(t, &target)?;
        l.hidden = false;
        l.overwrite(from, to, pts, fallback);
    }
    Ok(Output::msg(format!("{lanes} レーンに書き込みました")))
}

/// `--ticks`: the positions of the points a command acts on.
fn point_ticks(a: &Args) -> CmdResult<std::collections::HashSet<Tick>> {
    let list = a.str("ticks")?;
    let mut ticks = std::collections::HashSet::new();
    for part in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        ticks.insert(part.parse::<Tick>().map_err(|_| {
            CmdError::invalid(format!("--ticks の `{part}` を数として読み取れません"))
        })?);
    }
    if ticks.is_empty() {
        return Err(CmdError::invalid(
            "--ticks にポイントの位置を指定してください",
        ));
    }
    Ok(ticks)
}

fn lane_args(a: &Args) -> CmdResult<(TrackId, AutoTarget)> {
    Ok((a.id("track")?, a.id("target")?))
}

fn no_lane(tid: TrackId, target: &AutoTarget) -> CmdError {
    CmdError::not_found(format!("{tid} に {target} のレーンがありません"))
}

fn lane_mut<'a>(t: &'a mut Track, target: &AutoTarget) -> CmdResult<&'a mut AutoLane> {
    let tid = t.id;
    t.automation
        .iter_mut()
        .find(|l| l.target == *target)
        .ok_or_else(|| no_lane(tid, target))
}

fn in_range(target: &AutoTarget, v: f64) -> CmdResult<f64> {
    let (lo, hi) = target.range();
    if !(lo..=hi).contains(&v) {
        return Err(CmdError::range(format!(
            "{target} の値は {lo}〜{hi} の範囲で指定してください（{v} は範囲外です）"
        )));
    }
    Ok(v)
}
