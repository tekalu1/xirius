//! The pencil's shapes (UI-128): the shape the pencil draws, and the commands that lay one
//! down in an automation lane, a controller lane or the tempo row. A stroke is a time range,
//! the values at its two ends and, for a wave, an amplitude, a period and a phase.

use super::*;
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::command::EditCtx;
use crate::id::{IdKind, TempoId, TrackId};
use crate::model::{CtrlKind, CtrlPoint, CtrlShape, TempoPoint};
use crate::shape::{Dot, Shape, Stroke, generate, resample, thin};

const SHAPES: &[&str] = &[
    "free", "line", "parabola", "square", "triangle", "saw", "sine",
];

/// Freehand strokes are thinned until no point is further than this share of the range from the
/// line the others make.
const THIN_SHARE: f64 = 0.005;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "tool.draw.shape",
            "鉛筆が描く形を選びます（--shape で指定、--next・--prev で順に切り替えます）",
            vec![
                ArgDef::choice(
                    "shape",
                    &[
                        "free",
                        "line",
                        "parabola",
                        "square",
                        "triangle",
                        "saw",
                        "sine",
                        "transform",
                    ],
                    "形",
                ),
                ArgDef::flag("next", "次の形にする"),
                ArgDef::flag("prev", "前の形にする"),
            ],
            |_, s, _, a| {
                let now = s.view.draw_shape;
                let next = if let Some(name) = a.opt_str("shape") {
                    Shape::parse(name)
                        .ok_or_else(|| CmdError::invalid(format!("形 `{name}` を読み取れません")))?
                } else if a.flag("next") {
                    now.step(1)
                } else if a.flag("prev") {
                    now.step(-1)
                } else {
                    return Err(CmdError::invalid(
                        "--shape・--next・--prev のいずれかが必要です",
                    ));
                };
                s.view.draw_shape = next;
                Ok(Output::with(
                    format!("鉛筆の形を{}にしました", next.label()),
                    json!({ "shape": next.name() }),
                ))
            },
        ),
        edit(
            "auto.draw",
            "オートメーションレーンの範囲に、形（直線・放物線・矩形波・三角波・ノコギリ波・サイン波・フリーハンド）を描きます",
            [
                vec![
                    ArgDef::id("track", IdKind::Track, "トラック").req(),
                    ArgDef::text(
                        "target",
                        "対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>）",
                    )
                    .req(),
                ],
                stroke_args(),
            ]
            .concat(),
            auto_draw,
        ),
        edit(
            "cc.draw",
            "コントローラーレーンの範囲に、形をグリッドごとの階段で描きます",
            [
                vec![
                    ArgDef::id("clip", IdKind::Clip, "MIDI クリップ").req(),
                    ArgDef::text("lane", "レーン: cc1・cc11・cc64（CC の番号）・bend・at").req(),
                ],
                stroke_args(),
            ]
            .concat(),
            cc_draw,
        ),
        edit(
            "tempo.draw",
            "テンポの範囲に、形を描きます（曲の先頭のポイントは動かしません）",
            stroke_args(),
            tempo_draw,
        ),
    ]
}

fn stroke_args() -> Vec<ArgDef> {
    vec![
        ArgDef::choice("shape", SHAPES, "形").req(),
        ArgDef::pos("from", "範囲の開始位置").req(),
        ArgDef::pos("to", "範囲の終了位置").req(),
        ArgDef::float("value", "開始位置の値（フリーハンドでは不要）"),
        ArgDef::float("end-value", "終了位置の値（省略時は開始位置と同じ）").alias(&["end_value"]),
        ArgDef::float(
            "amp",
            "波の振幅（矩形波・三角波・ノコギリ波・サイン波。省略時は範囲の 1/4）",
        ),
        ArgDef::int("period", "波の 1 周期の長さ（tick。省略時は 1 拍）").range(1.0, 1e9),
        ArgDef::int("phase", "波の位相のずれ（tick。省略時は 0）"),
        ArgDef::int(
            "step",
            "ポイントの間隔（tick）。フリーハンドはこの間隔で読み直し、省略すると間引く",
        )
        .range(1.0, 1e9),
        ArgDef::objects("points", "フリーハンドの軌跡 [{tick, value}] の配列"),
    ]
}

struct Plan {
    shape: Shape,
    stroke: Stroke,
    points: Vec<(Tick, f64)>,
    step: Option<Tick>,
}

/// What a stroke command was given. `lo`..`hi` is the range of the lane's values.
fn plan(p: &Project, a: &Args, lo: f64, hi: f64, relative: bool) -> CmdResult<Plan> {
    let name = a.str("shape")?;
    let shape = Shape::parse(name)
        .filter(|s| *s != Shape::Transform)
        .ok_or_else(|| CmdError::invalid(format!("形 `{name}` を読み取れません")))?;
    let (from, to) = if relative {
        (a.i64("from")?, a.i64("to")?)
    } else {
        (tick(p, a, "from")?, tick(p, a, "to")?)
    };
    if from < 0 || to <= from {
        return Err(CmdError::range(format!(
            "範囲 {from}〜{to} が正しくありません（終了は開始より後にしてください）"
        )));
    }
    let mut points = Vec::new();
    if shape == Shape::Free {
        let list = match a.0.get("points") {
            Some(Value::Array(list)) if !list.is_empty() => list,
            _ => {
                return Err(CmdError::invalid(
                    "フリーハンドには points（[{tick, value}] の配列）が必要です",
                ));
            }
        };
        for (i, item) in list.iter().enumerate() {
            let (t, v) = (
                item.get("tick").and_then(Value::as_i64),
                item.get("value").and_then(Value::as_f64),
            );
            let (Some(t), Some(v)) = (t, v) else {
                return Err(CmdError::invalid(format!(
                    "{} 番目のポイントが {{tick, value}} の形式ではありません",
                    i + 1
                )));
            };
            points.push((t.clamp(from, to), v.clamp(lo, hi)));
        }
    }
    let v0 = match a.opt_f64("value") {
        Some(v) => v,
        None if shape == Shape::Free => 0.0,
        None => return Err(CmdError::invalid("--value が必要です")),
    };
    let period = a.opt_f64("period").unwrap_or(f64::from(p.ppq));
    if shape.is_wave() && period < 1.0 {
        return Err(CmdError::range("周期は 1 tick 以上にしてください"));
    }
    Ok(Plan {
        shape,
        stroke: Stroke {
            from,
            to,
            v0: v0.clamp(lo, hi),
            v1: a.opt_f64("end-value").unwrap_or(v0).clamp(lo, hi),
            amp: a.opt_f64("amp").unwrap_or((hi - lo) / 4.0),
            period,
            phase: a.opt_f64("phase").unwrap_or(0.0),
        },
        points,
        step: a.opt_i64("step"),
    })
}

/// The points of a plan, in time order and inside `lo`..`hi`. A stroke with a step is read at
/// that interval; without one a wave keeps its corners and the freehand one is thinned.
fn dots(plan: &Plan, lo: f64, hi: f64) -> Vec<Dot> {
    let s = &plan.stroke;
    let mut out = if plan.shape == Shape::Free {
        let kept = match plan.step {
            Some(step) => resample(&plan.points, s.from, s.to, step),
            None => thin(
                &{
                    let mut sorted = plan.points.clone();
                    sorted.sort_by_key(|p| p.0);
                    sorted
                },
                (hi - lo) * THIN_SHARE,
            ),
        };
        kept.into_iter()
            .map(|(tick, value)| Dot {
                tick,
                value,
                step: false,
                curve: 0.0,
            })
            .collect()
    } else if let Some(step) = plan.step {
        let mut at = s.from;
        let mut v = Vec::new();
        while at < s.to {
            v.push(Dot {
                tick: at,
                value: crate::shape::value_at(plan.shape, s, at as f64),
                step: false,
                curve: 0.0,
            });
            at += step.max(1);
        }
        v.push(Dot {
            tick: s.to,
            value: crate::shape::value_at(plan.shape, s, s.to as f64),
            step: false,
            curve: 0.0,
        });
        v
    } else {
        generate(plan.shape, s)
    };
    for d in &mut out {
        d.value = d.value.clamp(lo, hi);
    }
    out
}

fn auto_draw(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let target: AutoTarget = a.id("target")?;
    let (lo, hi) = target.range();
    let plan = plan(p, a, lo, hi, false)?;
    let stepped = target.stepped();
    let pts: Vec<AutoPoint> = dots(&plan, lo, hi)
        .into_iter()
        .map(|d| AutoPoint {
            tick: d.tick,
            value: d.value,
            curve: if stepped { 0.0 } else { d.curve },
            step: stepped || d.step,
        })
        .collect();
    let (from, to) = (plan.stroke.from, plan.stroke.to);
    let t = p.track_mut(tid)?;
    if !t.automation.iter().any(|l| l.target == target) {
        if !target.exists_on(t) {
            return Err(CmdError::not_found(format!(
                "{tid} に {target} が見つかりません"
            )));
        }
        t.automation.push(AutoLane::on_track(target.clone(), t));
    }
    let fallback = target.stored_value(t);
    let l = t
        .automation
        .iter_mut()
        .find(|l| l.target == target)
        .ok_or_else(|| CmdError::not_found(format!("{tid} に {target} のレーンがありません")))?;
    l.hidden = false;
    let n = pts.len();
    l.overwrite(from, to, pts, fallback);
    Ok(Output::with(
        format!("{tid} の {target} に {n} 個のポイントを描きました"),
        json!({ "points": n }),
    ))
}

fn cc_draw(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let key = a.str("lane")?;
    let kind = CtrlKind::parse(key).ok_or_else(|| {
        CmdError::invalid(format!(
            "レーン `{key}` を読み取れません（cc1・bend・at など）"
        ))
    })?;
    let (lo, hi) = kind.range();
    let mut plan = plan(p, a, f64::from(lo), f64::from(hi), true)?;
    // A controller lane keeps a value per grid step, so the stroke is always read at one.
    plan.step
        .get_or_insert_with(|| (i64::from(p.ppq) / 4).max(1));
    let points: Vec<CtrlPoint> = dots(&plan, f64::from(lo), f64::from(hi))
        .into_iter()
        .map(|d| CtrlPoint {
            tick: d.tick,
            value: d.value.round() as i32,
            shape: CtrlShape::Step,
        })
        .collect();
    cc::put(p, cid, kind, plan.stroke.from, plan.stroke.to, points)
}

fn tempo_draw(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let plan = plan(p, a, 1.0, 999.0, false)?;
    let (from, to) = (plan.stroke.from, plan.stroke.to);
    let drawn = dots(&plan, 1.0, 999.0);
    p.tempo
        .retain(|t| t.tick == 0 || t.tick < from || t.tick > to);
    let mut n = 0;
    for d in drawn {
        if d.tick == 0 {
            if let Some(first) = p.tempo.iter_mut().find(|t| t.tick == 0) {
                first.bpm = d.value;
                first.ramp = !d.step;
            }
            continue;
        }
        let id = TempoId(p.alloc());
        p.tempo.push(TempoPoint {
            id,
            tick: d.tick,
            bpm: d.value,
            ramp: !d.step,
        });
        n += 1;
    }
    p.tempo.sort_by_key(|t| t.tick);
    Ok(Output::with(
        format!("テンポに {n} 個のポイントを描きました"),
        json!({ "points": n }),
    ))
}
