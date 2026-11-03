//! `control.*`: a track's macro controls (MIX-039). Each edit is one undo step;
//! setting a control writes every target it moves into the song as well.
use serde_json::{Value, json};

use super::*;
use crate::auto::AutoTarget;
use crate::controls::{ControlId, Mapping, can_map};
use crate::id::{IdKind, TrackId};
use crate::model::{Project, Track};

pub(super) fn defs() -> Vec<CommandDef> {
    let track = || ArgDef::id("track", IdKind::Track, "トラック").req();
    let control = || {
        ArgDef::text(
            "control",
            "マクロコントロール: k1〜k8（つまみ）・b1〜b8（ボタン）・x1・y1・x2・y2（パッド）",
        )
        .req()
    };
    vec![
        edit(
            "control.set",
            "マクロコントロールの値を設定し、割り当て先を動かします（0〜1。ボタンは 0 か 1）",
            vec![
                track(),
                control(),
                ArgDef::float("value", "値（0〜1）").range(0.0, 1.0).req(),
            ],
            |p, _, a| set(p, a),
        ),
        edit(
            "control.xy",
            "X/Y パッドを動かし、X と Y に当てた 2 つのマクロ（つまみ）を一緒に設定します",
            vec![
                track(),
                ArgDef::float("x", "X の値（0〜1）").range(0.0, 1.0).req(),
                ArgDef::float("y", "Y の値（0〜1）").range(0.0, 1.0).req(),
            ],
            |p, _, a| xy(p, a),
        ),
        edit(
            "control.xy.assign",
            "X/Y パッドの X か Y に当てるマクロ（k1〜k8）を選びます（もう片方に当たっていれば入れ替えます）",
            vec![
                track(),
                ArgDef::text("axis", "x か y").req(),
                ArgDef::text("control", "マクロ: k1〜k8").req(),
            ],
            |p, _, a| xy_assign(p, a),
        ),
        edit(
            "control.assign",
            "マクロコントロールに割り当て先を足します（同じ先の範囲は書き換えます。ほかのマクロの先は割り当てられません）",
            vec![
                track(),
                control(),
                ArgDef::text(
                    "target",
                    "割り当て先: param:<プラグイン>:<名前>・branch:<分岐>:gain|pan|mute・volume・pan・send:<センド>",
                )
                .req(),
                ArgDef::float("min", "値 0 のときの先の位置（0〜1。省略時は 0）").range(0.0, 1.0),
                ArgDef::float("max", "値 1 のときの先の位置（0〜1。min より小さくすると逆向き。省略時は 1）")
                    .range(0.0, 1.0),
                ArgDef::float("curve", "曲がり（-1〜1。0 で直線）").range(-1.0, 1.0),
            ],
            |p, _, a| assign(p, a, a.str("target")?),
        ),
        edit(
            "control.assign.touched",
            "最後に触ったパラメーターをマクロコントロールに割り当てます（学習。プラグインの窓のつまみも拾います）",
            vec![
                track(),
                control(),
                ArgDef::float("min", "値 0 のときの先の位置（0〜1。省略時は 0）").range(0.0, 1.0),
                ArgDef::float("max", "値 1 のときの先の位置（0〜1。min より小さくすると逆向き。省略時は 1）")
                    .range(0.0, 1.0),
                ArgDef::float("curve", "曲がり（-1〜1。0 で直線）").range(-1.0, 1.0),
            ],
            |p, cx, a| {
                let Some(target) = cx.touched else {
                    return Err(CmdError::not_found(
                        "まだパラメーターに触れていません（機器のつまみを動かしてから学習します）",
                    ));
                };
                assign(p, a, target)
            },
        ),
        edit(
            "control.unassign",
            "マクロコントロールの割り当てを解除します（--target が無ければ全部）",
            vec![
                track(),
                control(),
                ArgDef::text("target", "外す割り当て先（省略時は全部）"),
            ],
            |p, _, a| unassign(p, a),
        ),
        edit(
            "control.rename",
            "マクロコントロール名を変更します（空にすると割り当て先の名前に戻ります）",
            vec![track(), control(), ArgDef::text("name", "名前").req()],
            |p, _, a| rename(p, a),
        ),
        read(
            "control.list",
            "トラックのマクロコントロールと割り当てを JSON で返します",
            vec![track()],
            |p, _, _, a| {
                let t = p.track(a.id::<TrackId>("track")?)?;
                Ok(Output::with(
                    format!("{} のマクロコントロールを表示しました", t.name),
                    list(t),
                ))
            },
        ),
    ]
}

fn control_of(a: &Args) -> CmdResult<ControlId> {
    a.str("control")?.parse().map_err(CmdError::invalid)
}

fn target_of(s: &str) -> CmdResult<AutoTarget> {
    if s.starts_with("control:") {
        return Err(CmdError::invalid(
            "マクロコントロールをマクロコントロールに割り当てることはできません",
        ));
    }
    let t: AutoTarget = s.parse().map_err(CmdError::invalid)?;
    if !can_map(&t) {
        return Err(CmdError::invalid(format!(
            "{s} はマクロコントロールに割り当てられません（param・branch・volume・pan・send が割り当てられます）"
        )));
    }
    Ok(t)
}

fn set(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let id = control_of(a)?;
    let mut v = a.opt_f64("value").unwrap_or(0.0).clamp(0.0, 1.0);
    if id.is_button() {
        v = v.round();
    }
    let t = p.track_mut(tid)?;
    t.controls.get_mut(id).value = v;
    t.apply_control_at(id, v);
    Ok(Output::msg(format!(
        "{} を {:.0} % にしました",
        t.control_label(id),
        v * 100.0
    )))
}

fn xy(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let t = p.track_mut(tid)?;
    let mut said = Vec::new();
    for (id, arg) in t.controls.xy_knobs().into_iter().zip(["x", "y"]) {
        let v = a.opt_f64(arg).unwrap_or(0.0).clamp(0.0, 1.0);
        t.controls.get_mut(id).value = v;
        t.apply_control_at(id, v);
        said.push(format!("{} を {:.0} %", t.control_label(id), v * 100.0));
    }
    Ok(Output::msg(format!("{}にしました", said.join("、"))))
}

fn xy_assign(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let axis = match a.str("axis")? {
        "x" | "X" => 0,
        "y" | "Y" => 1,
        other => {
            return Err(CmdError::invalid(format!(
                "`{other}` は x でも y でもありません"
            )));
        }
    };
    let ControlId::Knob(k) = control_of(a)? else {
        return Err(CmdError::invalid(
            "X/Y パッドに当てられるのはつまみ（k1〜k8）だけです",
        ));
    };
    let t = p.track_mut(tid)?;
    t.controls.set_xy(axis, k);
    let [x, y] = t.controls.xy_knobs();
    Ok(Output::msg(format!(
        "X/Y パッドを X {}、Y {} にしました",
        t.control_label(x),
        t.control_label(y)
    )))
}

fn assign(p: &mut Project, a: &Args, target: &str) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let id = control_of(a)?;
    let target = target_of(target)?;
    let t = p.track_mut(tid)?;
    let Some(now) = target.current(t) else {
        return Err(CmdError::not_found(format!(
            "{} に {target} はありません",
            t.name
        )));
    };
    if let Some(other) = t.controls.owner_of(&target)
        && other != id
    {
        return Err(CmdError::conflict(format!(
            "{target} は既に {other}（{}）に割り当てられています",
            t.control_label(other)
        )));
    }
    let c = t.controls.get_mut(id);
    let at = c.maps.iter().position(|m| m.target == target);
    let mut m = at.map_or_else(|| Mapping::new(target.clone()), |i| c.maps[i].clone());
    if let Some(x) = a.opt_f64("min") {
        m.from = x;
    }
    if let Some(x) = a.opt_f64("max") {
        m.to = x;
    }
    if let Some(x) = a.opt_f64("curve") {
        m.curve = x;
    }
    // the first target sets the control where the target already is, so nothing jumps
    if c.maps.is_empty() && !id.is_button() {
        c.value = m.inverse(target.to_norm(now));
    }
    match at {
        Some(i) => c.maps[i] = m,
        None => c.maps.push(m),
    }
    let v = c.value;
    t.apply_control_at(id, v);
    Ok(Output::msg(format!(
        "{id}（{}）に {target} を割り当てました",
        t.control_label(id)
    )))
}

fn unassign(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let id = control_of(a)?;
    let target = a.opt_str("target").map(target_of).transpose()?;
    let t = p.track_mut(tid)?;
    let label = t.control_label(id);
    let c = t.controls.get_mut(id);
    let before = c.maps.len();
    match &target {
        Some(x) => c.maps.retain(|m| m.target != *x),
        None => c.maps.clear(),
    }
    let n = before - c.maps.len();
    if n == 0 {
        return Err(CmdError::not_found(match target {
            Some(x) => format!("{id}（{label}）に {x} は割り当てられていません"),
            None => format!("{id}（{label}）には割り当てがありません"),
        }));
    }
    Ok(Output::msg(format!(
        "{id}（{label}）の割り当てを {n} 件外しました"
    )))
}

fn rename(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let id = control_of(a)?;
    let name = a.str("name")?.trim().to_string();
    let t = p.track_mut(tid)?;
    t.controls.get_mut(id).name = (!name.is_empty()).then_some(name);
    Ok(Output::msg(format!(
        "{id} の名前を {} にしました",
        t.control_label(id)
    )))
}

/// Every control of the track, with its name, value and maps.
pub fn list(t: &Track) -> Value {
    let rows: Vec<Value> = t
        .controls
        .iter()
        .map(|(id, c)| {
            json!({
                "control": id.to_string(),
                "name": t.control_label(id),
                "named": c.name.is_some(),
                "value": c.value,
                "maps": c.maps.iter().map(|m| json!({
                    "target": m.target.to_string(),
                    "name": crate::controls::target_name(t, &m.target),
                    "from": m.from,
                    "to": m.to,
                    "curve": m.curve,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "track": t.id.to_string(), "controls": rows })
}
