//! What is done to a stretch of time in one automation lane or in the tempo row (UI-241, AUT-010):
//! straighten it into a line, a curve or a flat run, move all of its values up or down (trim), or
//! scale and tilt them (transform). The line outside the stretch does not change.

use super::*;
use crate::auto::{AutoLane, AutoPoint, AutoTarget};
use crate::command::EditCtx;
use crate::error::ErrorCode;
use crate::id::{IdKind, TempoId, TrackId};
use crate::model::TempoPoint;

/// The values of a tempo are in bpm and stay inside these.
const BPM: (f64, f64) = (1.0, 999.0);

/// The curve a "curve" shape bends by when none is given.
const CURVE: f64 = 0.5;

pub(super) fn defs() -> Vec<CommandDef> {
    let on = |extra: Vec<ArgDef>| [lane_args(), extra].concat();
    vec![
        edit(
            "range.shape",
            "オートメーションレーン（またはテンポの行）の範囲を、直線・曲線・平らにします（範囲の中のポイントは両端の 2 つになり、外の線は変わりません）",
            on(vec![
                ArgDef::choice("shape", &["line", "curve", "flat"], "形").req(),
                ArgDef::float(
                    "curve",
                    "曲線の曲がり（-1〜1。省略時は 0.5。テンポには使えません）",
                )
                .range(-1.0, 1.0),
                ArgDef::float("value", "平らにする値（省略時は範囲の頭の値）"),
            ]),
            shape,
        ),
        edit(
            "range.trim",
            "オートメーションレーン（またはテンポの行）の範囲の値を、まとめて上げ下げします（外の線は変わりません）",
            on(vec![ArgDef::float(
                "delta",
                "上げ下げする量（オートメーションはレーンの高さに対する割合 -1〜1、テンポは bpm）",
            )
            .req()]),
            trim,
        ),
        edit(
            "range.transform",
            "オートメーションレーン（またはテンポの行）の範囲の値を、縮尺と傾きで変形します（外の線は変わりません）。画面で scale と tilt を両方省くと、範囲に変形の枠を出します",
            on(vec![
                ArgDef::float(
                    "scale",
                    "範囲の中の最小と最大の中央を軸にした縮尺（省略時は 1）",
                ),
                ArgDef::float(
                    "tilt",
                    "範囲の終わりでの上げ下げ量（頭は 0。オートメーションはレーンの高さに対する割合、テンポは bpm。省略時は 0）",
                ),
            ]),
            transform,
        ),
    ]
}

fn lane_args() -> Vec<ArgDef> {
    vec![
        ArgDef::id(
            "track",
            IdKind::Track,
            "オートメーションのトラック（テンポのときは不要）",
        ),
        ArgDef::text(
            "target",
            "対象（volume・pan・mute・send:<ID>・param:<ID>:<名前>）",
        ),
        ArgDef::flag("tempo", "テンポの行を対象にする"),
        ArgDef::pos("from", "範囲の開始位置").req(),
        ArgDef::pos("to", "範囲の終了位置").req(),
    ]
}

enum Place {
    Lane(TrackId, AutoTarget),
    Tempo,
}

fn place(a: &Args) -> CmdResult<Place> {
    if a.flag("tempo") {
        return Ok(Place::Tempo);
    }
    Ok(Place::Lane(a.id("track")?, a.id("target")?))
}

fn span(p: &Project, a: &Args) -> CmdResult<(Tick, Tick)> {
    let (from, to) = (tick(p, a, "from")?, tick(p, a, "to")?);
    if from < 0 || to <= from {
        return Err(CmdError::range(format!(
            "範囲 {from}〜{to} が正しくありません（終了は開始より後にしてください）"
        )));
    }
    Ok((from, to))
}

fn nothing(what: &str) -> CmdError {
    CmdError::new(ErrorCode::NothingToDo, what.to_string())
}

/// How the values of the stretch are rewritten.
#[derive(Clone, Copy)]
enum Op {
    Line,
    Curve(f64),
    Flat(Option<f64>),
    Trim(f64),
    Transform { scale: f64, tilt: f64 },
}

fn shape(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let place = place(a)?;
    let op = match a.str("shape")? {
        "line" => Op::Line,
        "curve" => {
            if matches!(place, Place::Tempo) {
                return Err(CmdError::invalid(
                    "テンポには曲線がありません（直線か平らにしてください）",
                ));
            }
            Op::Curve(a.opt_f64("curve").unwrap_or(CURVE))
        }
        "flat" => Op::Flat(a.opt_f64("value")),
        other => {
            return Err(CmdError::invalid(format!(
                "形 `{other}` を読み取れません（line・curve・flat）"
            )));
        }
    };
    run(p, a, place, op)
}

fn trim(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let place = place(a)?;
    run(p, a, place, Op::Trim(a.f64("delta")?))
}

fn transform(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let place = place(a)?;
    let op = Op::Transform {
        scale: a.opt_f64("scale").unwrap_or(1.0),
        tilt: a.opt_f64("tilt").unwrap_or(0.0),
    };
    run(p, a, place, op)
}

fn run(p: &mut Project, a: &Args, place: Place, op: Op) -> CmdResult<Output> {
    let (from, to) = span(p, a)?;
    let n = match place {
        Place::Lane(tid, target) => on_lane(p, tid, &target, from, to, op)?,
        Place::Tempo => on_tempo(p, from, to, op)?,
    };
    Ok(Output::with(
        format!("範囲の中のポイントを {n} 個にしました"),
        json!({ "points": n }),
    ))
}

/// The values of the stretch, one per point, rewritten; `norm` are the values in the unit of the
/// op (a lane's height, or bpm) and stay within `lo..=hi`.
fn rework(op: Op, at: &[Tick], norm: &[f64], span: (Tick, Tick), lo: f64, hi: f64) -> Vec<f64> {
    let (from, to) = span;
    let (min, max) = norm
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let center = (min + max) / 2.0;
    at.iter()
        .zip(norm)
        .map(|(t, v)| {
            let frac = (*t - from) as f64 / (to - from) as f64;
            match op {
                Op::Trim(delta) => v + delta,
                Op::Transform { scale, tilt } => center + (v - center) * scale + tilt * frac,
                _ => *v,
            }
            .clamp(lo, hi)
        })
        .collect()
}

fn on_lane(
    p: &mut Project,
    tid: TrackId,
    target: &AutoTarget,
    from: Tick,
    to: Tick,
    op: Op,
) -> CmdResult<usize> {
    let stepped = target.stepped();
    let t = p.track_mut(tid)?;
    let fallback = target.stored_value(t);
    let l = t
        .automation
        .iter_mut()
        .find(|l| l.target == *target)
        .ok_or_else(|| CmdError::not_found(format!("{tid} に {target} のレーンがありません")))?;
    let (Some(a), Some(b)) = (l.point_at(from), l.point_at(to)) else {
        return Err(nothing("レーンにポイントがありません"));
    };
    let to_n = |v: f64| target.to_norm(v);
    let from_n = |n: f64| target.from_norm(n.clamp(0.0, 1.0));
    let points: Vec<AutoPoint> = match op {
        Op::Line | Op::Curve(_) => vec![
            AutoPoint {
                curve: if stepped {
                    0.0
                } else if let Op::Curve(c) = op {
                    c
                } else {
                    0.0
                },
                step: stepped,
                ..a
            },
            b,
        ],
        Op::Flat(value) => {
            let v = value.map_or(a.value, |v| from_n(to_n(v)));
            vec![
                AutoPoint {
                    value: v,
                    curve: 0.0,
                    step: stepped,
                    ..a
                },
                AutoPoint { value: v, ..b },
            ]
        }
        Op::Trim(_) | Op::Transform { .. } => {
            let all = stretch(l, from, to).unwrap_or_default();
            let ticks: Vec<Tick> = all.iter().map(|x| x.tick).collect();
            let norm: Vec<f64> = all.iter().map(|x| to_n(x.value)).collect();
            let moved = rework(op, &ticks, &norm, (from, to), 0.0, 1.0);
            all.iter()
                .zip(moved)
                .map(|(x, n)| AutoPoint {
                    value: from_n(n),
                    ..*x
                })
                .collect()
        }
    };
    let n = points.len();
    l.overwrite(from, to, points, fallback);
    Ok(n)
}

/// The points of the stretch `[from, to]` of a lane: the values in force at both ends and the
/// points between. None where the lane has no line.
pub fn stretch(l: &AutoLane, from: Tick, to: Tick) -> Option<Vec<AutoPoint>> {
    let (a, b) = (l.point_at(from)?, l.point_at(to)?);
    let mut all = vec![a];
    all.extend(
        l.points
            .iter()
            .filter(|x| x.tick > from && x.tick < to)
            .copied(),
    );
    all.push(b);
    Some(all)
}

/// What a drag on a lane's range does to its values (AUT-010), as `range.trim` and
/// `range.transform` do it: `delta` and `tilt` are parts of the lane's height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeMove {
    Trim(f64),
    Transform { scale: f64, tilt: f64 },
}

/// `l` with the stretch `[from, to]` moved as the command would move it; the line outside does
/// not change. `fallback` is the value the track holds where the lane has none. None where the
/// lane has no line.
pub fn moved_lane(
    l: &AutoLane,
    from: Tick,
    to: Tick,
    how: RangeMove,
    fallback: Option<f64>,
) -> Option<AutoLane> {
    let all = stretch(l, from, to)?;
    let op = op_of(how);
    let target = &l.target;
    let ticks: Vec<Tick> = all.iter().map(|x| x.tick).collect();
    let norm: Vec<f64> = all.iter().map(|x| target.to_norm(x.value)).collect();
    let moved = rework(op, &ticks, &norm, (from, to), 0.0, 1.0);
    let points: Vec<AutoPoint> = all
        .iter()
        .zip(moved)
        .map(|(x, n)| AutoPoint {
            value: target.from_norm(n.clamp(0.0, 1.0)),
            ..*x
        })
        .collect();
    let mut out = l.clone();
    out.overwrite(from, to, points, fallback);
    Some(out)
}

/// The bpm in force at `tick`: a ramp glides to the next point, otherwise the value holds.
pub fn bpm_at(tempo: &[TempoPoint], tick: Tick) -> f64 {
    let Some(i) = tempo.iter().rposition(|t| t.tick <= tick) else {
        return tempo.first().map_or(120.0, |t| t.bpm);
    };
    let a = &tempo[i];
    match tempo.get(i + 1) {
        Some(b) if a.ramp && b.tick > a.tick => {
            a.bpm + (b.bpm - a.bpm) * (tick - a.tick) as f64 / (b.tick - a.tick) as f64
        }
        _ => a.bpm,
    }
}

/// Whether the line leaving `tick` glides.
fn ramp_at(tempo: &[TempoPoint], tick: Tick) -> bool {
    tempo
        .iter()
        .rev()
        .find(|t| t.tick <= tick)
        .is_some_and(|t| t.ramp)
}

fn on_tempo(p: &mut Project, from: Tick, to: Tick, op: Op) -> CmdResult<usize> {
    if p.tempo.is_empty() {
        return Err(nothing("テンポがありません"));
    }
    let mut tempo = std::mem::take(&mut p.tempo);
    let n = rewrite_tempo(&mut tempo, from, to, op, || TempoId(p.alloc()));
    p.tempo = tempo;
    Ok(n)
}

/// The tempo with the stretch `[from, to]` moved as `range.trim` and `range.transform` move it
/// (AUT-010): `Trim` and `tilt` are in bpm. The points it adds have no id the song knows.
pub fn moved_tempo(tempo: &[TempoPoint], from: Tick, to: Tick, how: RangeMove) -> Vec<TempoPoint> {
    let mut out = tempo.to_vec();
    if out.is_empty() || to <= from {
        return out;
    }
    let mut next = u64::MAX;
    rewrite_tempo(&mut out, from, to, op_of(how), || {
        next -= 1;
        TempoId(next)
    });
    out
}

fn op_of(how: RangeMove) -> Op {
    match how {
        RangeMove::Trim(d) => Op::Trim(d),
        RangeMove::Transform { scale, tilt } => Op::Transform { scale, tilt },
    }
}

/// `tempo` with the stretch rewritten by `op`; `alloc` names the points it adds. The number of
/// points the stretch has afterwards.
fn rewrite_tempo(
    tempo: &mut Vec<TempoPoint>,
    from: Tick,
    to: Tick,
    op: Op,
    mut alloc: impl FnMut() -> TempoId,
) -> usize {
    let before = tempo.clone();
    let (a, b) = (bpm_at(&before, from), bpm_at(&before, to));
    let inner: Vec<TempoPoint> = before
        .iter()
        .filter(|t| t.tick > from && t.tick < to)
        .cloned()
        .collect();
    let end_ramp = ramp_at(&before, to);
    // The points of the stretch, as (tick, bpm, glides).
    let mut made: Vec<(Tick, f64, bool)> = match op {
        Op::Line | Op::Curve(_) => vec![(from, a, true), (to, b, end_ramp)],
        Op::Flat(value) => {
            let v = value.unwrap_or(a).clamp(BPM.0, BPM.1);
            vec![(from, v, false), (to, v, end_ramp)]
        }
        Op::Trim(_) | Op::Transform { .. } => {
            let mut all = vec![(from, a, ramp_at(&before, from))];
            all.extend(inner.iter().map(|t| (t.tick, t.bpm, t.ramp)));
            all.push((to, b, end_ramp));
            let ticks: Vec<Tick> = all.iter().map(|x| x.0).collect();
            let norm: Vec<f64> = all.iter().map(|x| x.1).collect();
            let moved = rework(op, &ticks, &norm, (from, to), BPM.0, BPM.1);
            all.iter().zip(moved).map(|(x, v)| (x.0, v, x.2)).collect()
        }
    };
    // A trim or a transform hands the line outside back as it was, one tick beyond each end.
    let moves = matches!(op, Op::Trim(_) | Op::Transform { .. });
    let mut guards = Vec::new();
    if moves {
        if from > 1 && !before.iter().any(|t| t.tick == from - 1) {
            guards.push((from - 1, bpm_at(&before, from - 1), false));
        }
        guards.push((to + 1, bpm_at(&before, to + 1), ramp_at(&before, to + 1)));
    }
    // The first point of the song stays; it takes the new value when the stretch starts there.
    tempo.retain(|t| t.tick == 0 || t.tick < from || t.tick > to + i64::from(moves));
    for (tick, bpm, ramp) in made.drain(..).chain(guards) {
        if let Some(first) = tempo.iter_mut().find(|t| t.tick == tick) {
            first.bpm = bpm;
            first.ramp = ramp;
            continue;
        }
        tempo.push(TempoPoint {
            id: alloc(),
            tick,
            bpm,
            ramp,
        });
    }
    tempo.sort_by_key(|t| t.tick);
    tempo
        .iter()
        .filter(|t| t.tick >= from && t.tick <= to)
        .count()
}
