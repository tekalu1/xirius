//! Tempo, speed and pitch of audio clips (EDT-007, EDT-016), and the transpose of a MIDI clip
//! (UI-311). The source is never rewritten: these set `AudioProcess` or the clip's transpose,
//! and the sound is worked out when the song plays.
//!
//! `clip.reverse`, `clip.normalize` and `clip.tempo.detect` need the source (its length, its
//! peak, its beat), so the session answers them and leaves one `clip.reverse.apply` /
//! `clip.normalize.apply` / `clip.tempo.detect.apply` for the history.

use super::*;
use crate::id::{ClipId, IdKind};
use crate::model::{AudioProcess, ClipBody, MAX_DB, MIN_DB, Note, clamp_semitones};
use crate::tempo::TempoMap;

pub(super) fn defs() -> Vec<CommandDef> {
    let [on, off] = on_off();
    vec![
        edit(
            "clip.tempo.set",
            "オーディオクリップの素材のテンポ（bpm）を設定します。曲のテンポに合わせているクリップは、素材の同じ範囲を保って長さが変わります",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::float("bpm", "素材のテンポ（bpm）")
                    .req()
                    .range(1.0, 999.0),
            ],
            |p, _, a| {
                let bpm = a.f64("bpm")?;
                let n = update(p, a, |x, _| x.tempo = Some(bpm))?;
                Ok(Output::msg(format!(
                    "クリップ {n} 件のテンポを {bpm} にしました"
                )))
            },
        ),
        session(
            "clip.tempo.detect",
            "オーディオクリップの鳴らす範囲から素材のテンポを検出し、クリップのテンポにします",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            Risk::Write,
            false,
        ),
        edit(
            "clip.tempo.detect.apply",
            "オーディオクリップごとに素材のテンポを設定します（clip.tempo.detect が使います）",
            vec![ArgDef::objects("clips", "クリップ [{id, bpm}]").req()],
            |p, _, a| {
                let xs = clip_objects(p, a)?;
                let map = TempoMap::from_project(p);
                for (id, x) in &xs {
                    let bpm = x["bpm"]
                        .as_f64()
                        .filter(|v| (1.0..=999.0).contains(v))
                        .ok_or_else(|| CmdError::invalid("bpm がありません"))?;
                    change(p, &map, *id, |x, _| x.tempo = Some(bpm))?;
                }
                // clip.tempo.detect has said what was found.
                Ok(Output::msg(""))
            },
        ),
        edit(
            "clip.stretch",
            "オーディオクリップを曲のテンポに合わせるか（伸縮するか）を設定します。素材のテンポが無いときは、クリップの位置の曲のテンポを素材のテンポにします。--follow を省くと切り替えます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::choice("follow", &["on", "off"], "曲のテンポに合わせる"),
            ],
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                let want = match a.opt_str("follow") {
                    Some(v) => v == "on",
                    None => !ids
                        .iter()
                        .all(|id| process(p, *id).is_ok_and(|x| x.follow && x.tempo.is_some())),
                };
                let n = update(p, a, |x, song| {
                    x.follow = want;
                    if want && x.tempo.is_none() {
                        // Plays as it did: the song's tempo where the clip starts.
                        x.tempo = Some(song);
                    }
                })?;
                Ok(Output::msg(if want {
                    format!("クリップ {n} 件を曲のテンポに合わせました")
                } else {
                    format!("クリップ {n} 件を曲のテンポに合わせないようにしました")
                }))
            },
        ),
        edit(
            "clip.speed.set",
            "オーディオクリップの速さ（%）を設定します。音の高さは変わりません。素材の同じ範囲を保って長さが変わります",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::float("percent", "速さ（%）")
                    .req()
                    .range(AudioProcess::MIN_SPEED, AudioProcess::MAX_SPEED),
            ],
            |p, _, a| {
                let percent = a.f64("percent")?;
                let n = update(p, a, |x, _| x.speed = percent)?;
                Ok(Output::msg(format!(
                    "クリップ {n} 件の速さを {percent}% にしました"
                )))
            },
        ),
        edit(
            "clip.transpose",
            "クリップの音の高さを半音で設定します（長さは変わりません）。オーディオのクリップは鳴らす高さが、MIDI のクリップはノートに足す値が変わります。--by は今の値からの変化量。範囲（−24〜24）の外は丸めます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("semis", "トランスポーズ（半音。範囲の外は丸めます）").clamp_range(
                    -f64::from(AudioProcess::SEMIS),
                    f64::from(AudioProcess::SEMIS),
                ),
                ArgDef::int("by", "変化量（半音）"),
            ],
            |p, _, a| {
                let (to, by) = absolute_or_by(a, "semis")?;
                let ids = a.require_ids::<ClipId>("クリップ")?;
                for id in &ids {
                    let c = p.clip_mut(*id)?.1;
                    match &mut c.body {
                        // The pitch only: the length stays, so the source's part does.
                        ClipBody::Audio { process, .. } => {
                            let v = to.unwrap_or(process.semis).saturating_add(by);
                            process.semis = v.clamp(-AudioProcess::SEMIS, AudioProcess::SEMIS);
                        }
                        ClipBody::Midi { transpose, .. } => {
                            let v = to.unwrap_or(i32::from(*transpose)).saturating_add(by);
                            *transpose = clamp_semitones(i64::from(v));
                        }
                    }
                }
                Ok(Output::msg(format!(
                    "クリップ {} 件をトランスポーズしました",
                    ids.len()
                )))
            },
        ),
        edit(
            "clip.transpose.write",
            "MIDI クリップのトランスポーズをノートの高さに反映し、値を 0 に戻します（鳴る高さは変わりません）。値が 0 のクリップはそのままです。反映した後の高さが 0〜127 の外に出るノートがあると、何も変えずに失敗します",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            |p, _, a| {
                let ids = a.require_ids::<ClipId>("クリップ")?;
                let (mut clips, mut moved) = (0, 0);
                for id in &ids {
                    let c = p.clip_mut(*id)?.1;
                    let ClipBody::Midi { notes, transpose } = &mut c.body else {
                        return Err(CmdError::invalid(format!(
                            "クリップ {id} は MIDI ではありません。書き込めるのは MIDI クリップだけです"
                        )));
                    };
                    let by = i32::from(*transpose);
                    if by == 0 {
                        continue;
                    }
                    let mut next = Vec::with_capacity(notes.len());
                    for n in notes.iter() {
                        let pitch = i32::from(n.pitch) + by;
                        if !(0..=127).contains(&pitch) {
                            return Err(CmdError::range(format!(
                                "クリップ {id} のノート {} を {by} 半音移動すると範囲外になります（{} → {pitch}）",
                                n.id, n.pitch
                            )));
                        }
                        next.push(Note {
                            pitch: pitch as u8,
                            ..*n
                        });
                    }
                    *notes = Arc::new(next);
                    *transpose = 0;
                    clips += 1;
                    moved += notes.len();
                }
                if clips == 0 {
                    return Ok(Output::msg(
                        "トランスポーズが 0 なので反映するものがありません",
                    ));
                }
                Ok(Output::msg(format!(
                    "クリップ {clips} 件のノート {moved} 個にトランスポーズを書き込みました"
                )))
            },
        ),
        edit(
            "clip.tune",
            "オーディオクリップの音の高さをセントで微調整します（長さは変わりません）。--by は今の値からの変化量",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::int("cents", "微調整（セント）").range(
                    -f64::from(AudioProcess::CENTS),
                    f64::from(AudioProcess::CENTS),
                ),
                ArgDef::int("by", "変化量（セント）"),
            ],
            |p, _, a| {
                let (to, by) = absolute_or_by(a, "cents")?;
                let n = update(p, a, |x, _| {
                    let v = to.unwrap_or(x.cents).saturating_add(by);
                    x.cents = v.clamp(-AudioProcess::CENTS, AudioProcess::CENTS);
                })?;
                Ok(Output::msg(format!(
                    "クリップ {n} 件の音の高さを微調整しました"
                )))
            },
        ),
        session(
            "clip.reverse",
            "オーディオクリップを逆再生にします（素材は書き換えません）。クリップは素材の同じ範囲を後ろから鳴らします。--on / --off を省くと切り替えます",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                on.clone(),
                off.clone(),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "clip.reverse.apply",
            "オーディオクリップの逆再生と素材の開始位置を設定します（clip.reverse が使います）",
            vec![ArgDef::objects("clips", "クリップ [{id, reverse, offset}]").req()],
            reverse_apply,
        ),
        session(
            "clip.normalize",
            "オーディオクリップのゲインを、鳴らす範囲のピークが --peak（既定 0 dB）になるように設定します（素材は書き換えません）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::float("peak", "ピークの目標（dBFS）")
                    .range(-60.0, 0.0)
                    .default(json!(0.0)),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "clip.normalize.apply",
            "オーディオクリップのゲインを設定します（clip.normalize が使います）",
            vec![ArgDef::objects("clips", "クリップ [{id, gain_db}]").req()],
            normalize_apply,
        ),
    ]
}

/// The objects of `--clips`, each with an audio clip's `id`.
fn clip_objects(p: &Project, a: &Args) -> CmdResult<Vec<(ClipId, serde_json::Value)>> {
    let xs = a.0["clips"]
        .as_array()
        .ok_or_else(|| CmdError::invalid("--clips を指定してください"))?;
    xs.iter()
        .map(|x| {
            let id: ClipId = x["id"]
                .as_str()
                .ok_or_else(|| CmdError::invalid("クリップの id がありません"))?
                .parse()
                .map_err(CmdError::invalid)?;
            process(p, id)?;
            Ok((id, x.clone()))
        })
        .collect()
}

fn reverse_apply(p: &mut Project, _: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let xs = clip_objects(p, a)?;
    for (id, x) in &xs {
        let on = x["reverse"]
            .as_bool()
            .ok_or_else(|| CmdError::invalid("reverse がありません"))?;
        let at = x["offset"]
            .as_i64()
            .ok_or_else(|| CmdError::invalid("offset がありません"))?;
        let ClipBody::Audio {
            process, offset, ..
        } = &mut p.clip_mut(*id)?.1.body
        else {
            unreachable!("checked above");
        };
        process.reverse = on;
        *offset = at.max(0);
    }
    Ok(Output::msg(format!(
        "クリップ {} 件の逆再生を切り替えました",
        xs.len()
    )))
}

fn normalize_apply(
    p: &mut Project,
    _: &crate::command::EditCtx<'_>,
    a: &Args,
) -> CmdResult<Output> {
    let xs = clip_objects(p, a)?;
    for (id, x) in &xs {
        let db = x["gain_db"]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| CmdError::invalid("gain_db がありません"))?;
        let ClipBody::Audio { gain_db, .. } = &mut p.clip_mut(*id)?.1.body else {
            unreachable!("checked above");
        };
        *gain_db = db.clamp(MIN_DB, MAX_DB);
    }
    Ok(Output::msg(format!(
        "クリップ {} 件をノーマライズしました",
        xs.len()
    )))
}

/// The process of an audio clip.
pub(crate) fn process(p: &Project, id: ClipId) -> CmdResult<&AudioProcess> {
    match &p.clip(id)?.1.body {
        ClipBody::Audio { process, .. } => Ok(process),
        ClipBody::Midi { .. } => Err(CmdError::invalid(format!(
            "クリップ {id} は MIDI です。オーディオのクリップだけに使えます"
        ))),
    }
}

/// `--<name>` sets the value, `--by` moves it: exactly one of them.
fn absolute_or_by(a: &Args, name: &str) -> CmdResult<(Option<i32>, i32)> {
    let small = |v: i64| v.clamp(-1000, 1000) as i32;
    match (a.opt_i64(name), a.opt_i64("by")) {
        (Some(v), None) => Ok((Some(small(v)), 0)),
        (None, Some(d)) => Ok((None, small(d))),
        _ => Err(CmdError::invalid(format!(
            "--{name} と --by のどちらか 1 つだけを指定してください"
        ))),
    }
}

/// Changes the process of each clip in `--ids` (`f` also gets the song's tempo at the clip). Where the change speeds the sound up or down,
/// the clip keeps the same part of its source: its length and offset scale with the speed.
fn update(p: &mut Project, a: &Args, f: impl Fn(&mut AudioProcess, f64)) -> CmdResult<usize> {
    let ids = a.require_ids::<ClipId>("クリップ")?;
    for id in &ids {
        process(p, *id)?;
    }
    let map = TempoMap::from_project(p);
    for id in &ids {
        change(p, &map, *id, &f)?;
    }
    Ok(ids.len())
}

/// Changes the process of one audio clip, as [`update`] does.
fn change(
    p: &mut Project,
    map: &TempoMap,
    id: ClipId,
    f: impl Fn(&mut AudioProcess, f64),
) -> CmdResult<()> {
    let c = p.clip_mut(id)?.1;
    let song = map.bpm_at(c.start);
    let ClipBody::Audio {
        process, offset, ..
    } = &mut c.body
    else {
        return Err(CmdError::invalid(format!(
            "クリップ {id} は MIDI です。オーディオのクリップだけに使えます"
        )));
    };
    let was = process.ratio(song);
    f(process, song);
    let now = process.ratio(song);
    if (now - was).abs() > 1e-12 {
        let scale = now / was;
        let base = map.seconds_at(c.start);
        let off = (map.seconds_at(c.start + *offset) - base) * scale;
        *offset = (map.tick_at(base + off) - c.start).max(0);
        let len = (map.seconds_at(c.start + c.length) - base) * scale;
        c.length = (map.tick_at(base + len) - c.start).max(1);
    }
    Ok(())
}
