//! Notes inside MIDI clips. Every command here takes note ids, or a clip id
//! meaning "all notes of that clip"; with no `--ids` it uses the selection.

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use super::*;
use crate::command::EditCtx;
use crate::id::{ClipId, IdKind};
use crate::time::quantize_tick;

pub(super) fn defs() -> Vec<CommandDef> {
    let scope = || {
        ArgDef::ids(
            IdKind::NoteScope,
            "ノート（クリップの ID を指定すると、そのクリップの全ノート）",
        )
    };
    vec![
        edit(
            "note.add",
            "ノートを 1 つ追加します",
            vec![
                ArgDef::id("clip", IdKind::Clip, "追加先の MIDI クリップ").req(),
                ArgDef::pos(
                    "at",
                    "開始位置（曲の中の位置。小節.拍.tick または tick 数）",
                )
                .req(),
                ArgDef::int("pitch", "ピッチ（MIDI ノート番号 0〜127。60 が中央の C）")
                    .req()
                    .range(0.0, 127.0),
                ArgDef::grid("length", "長さ（1/4・1/8t・tick 数）").default(json!("1/16")),
                ArgDef::int("velocity", "ベロシティ 1〜127")
                    .default(json!(100))
                    .range(1.0, 127.0),
                super::overlap::note_arg(),
            ],
            add_note,
        ),
        destroy(
            "note.delete",
            "ノートを削除します",
            vec![scope()],
            |p, _, a| {
                let n = remove_notes(p, &Scope::from_args(a)?)?;
                Ok(Output::msg(format!("ノート {n} 個を削除しました")))
            },
        ),
        edit(
            "note.move",
            "ノートを時間方向に移動します",
            vec![
                scope(),
                ArgDef::int("by", "移動量（tick。負の値で前へ）").req(),
                super::overlap::note_arg(),
            ],
            |p, cx, a| {
                let by = a.i64("by")?;
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
                    n.start += by;
                    if n.start < 0 {
                        return Err(CmdError::range(format!(
                            "ノート {} がクリップの先頭より前に出てしまいます",
                            n.id
                        )));
                    }
                    Ok(())
                })?;
                Ok(Output::msg(format!("ノート {n} 個を {by} tick 移動しました")))
            },
        ),
        edit(
            "note.resize",
            "ノートの長さを変更します",
            vec![
                scope(),
                ArgDef::grid("length", "新しい長さ（1/8・tick 数など）"),
                ArgDef::int("by", "伸ばす量（tick。負の値で縮める）"),
                ArgDef::choice("mode", &["next", "grid", "double", "half"], "長さの型"),
                ArgDef::grid("grid", "グリッドの長さ").default(json!("1/16")),
                super::overlap::note_arg(),
            ],
            |p, cx, a| {
                let length = if a.has("length") {
                    Some(grid(p, a, "length")?)
                } else {
                    None
                };
                let by = a.opt_i64("by");
                let mode = a.opt_str("mode");
                if usize::from(length.is_some()) + usize::from(by.is_some()) + usize::from(mode.is_some()) != 1 {
                    return Err(CmdError::invalid("--length・--by・--mode のいずれか 1 つを指定してください"));
                }
                let step = grid(p, a, "grid")?;
                let starts: HashMap<ClipId, Vec<Tick>> = p.tracks.values().flat_map(|t| t.clips.iter()
                    .map(|c| (c.id, c.notes().iter().map(|n| n.start).collect()))).collect();
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, c| {
                    let new = match mode {
                        Some("next") => starts[&c].iter().copied().filter(|t| *t > n.start).min().map_or(n.length, |t| t - n.start),
                        Some("grid") => step,
                        Some("double") => n.length.checked_mul(2).ok_or_else(|| CmdError::range("ノートの長さが大きすぎます"))?,
                        Some("half") => (n.length / 2).max(1),
                        _ => length.unwrap_or_else(|| n.length + by.unwrap_or(0)),
                    };
                    if new < 1 {
                        return Err(CmdError::range(format!(
                            "ノート {} の長さが 1 tick 未満になります",
                            n.id
                        )));
                    }
                    n.length = new;
                    Ok(())
                })?;
                Ok(Output::msg(format!("ノート {n} 個の長さを変更しました")))
            },
        ),
        edit(
            "note.quantize",
            "ノートの開始位置をクオンタイズします（--strength で強さを指定）",
            vec![
                scope(),
                ArgDef::grid("grid", "グリッド（1/16・1/8t・1/4d・tick 数）").default(json!("1/16")),
                ArgDef::int("strength", "クオンタイズの強さ %（100 で完全にグリッドに揃える）")
                    .default(json!(100))
                    .range(0.0, 100.0),
                ArgDef::flag("lengths", "長さもグリッドの倍数にクオンタイズする"),
                ArgDef::flag("restore", "元のタイミングに戻す"),
                ArgDef::flag("freeze", "現在のタイミングを元のタイミングとして固定する"),
                ArgDef::int("tuplet", "連符（3・5・7。グリッドの 2 つ・4 つ分に n 個）").range(1.0, 7.0),
                ArgDef::int("swing", "スウィング %（1 つおきの線を、半ステップのこの割合だけ後ろへ）")
                    .range(0.0, 100.0),
                ArgDef::int("end", "終わりをクオンタイズする強さ %（省くと長さを保って動かします）")
                    .range(0.0, 100.0),
                ArgDef::int("range", "線からこの割合（線の間の半分に対する %）以内のノートだけ動かす")
                    .range(0.0, 100.0),
                ArgDef::id("groove", IdKind::Clip, "グルーヴにする MIDI クリップ（グリッドの代わりに、そのノートの開始とベロシティに揃えます）"),
                ArgDef::int("velocity", "グルーヴのベロシティに揃える強さ %").range(0.0, 100.0),
                ArgDef::objects("groove-hits","外部グルーヴの [{at,velocity}]"),
                ArgDef::int("groove-period","外部グルーヴの周期（tick）").range(1.0,i64::MAX as f64),
                super::overlap::note_arg(),
            ],
            |p, cx, a| {
                let scope = Scope::from_args(a)?;
                if a.flag("restore") && a.flag("freeze") {
                    return Err(CmdError::invalid("復元と固定は同時に指定できません"));
                }
                let originals = p.original_timing.clone();
                if a.flag("restore") {
                    let n = edit_notes(p, &scope, cx.author.is_ai(), |n, _| {
                        if let Some(t) = originals.get(&n.id) { n.start = t.start; n.length = t.length; }
                        Ok(())
                    })?;
                    return Ok(Output::msg(format!("ノート {n} 個のタイミングを復元しました")));
                }
                let mut saved = Vec::new();
                edit_notes(p, &scope, false, |n, _| {
                    saved.push((n.id, crate::editing::Timing { start: n.start, length: n.length }));
                    Ok(())
                })?;
                for (id, timing) in saved {
                    if a.flag("freeze") { p.original_timing.remove(&id); }
                    else { p.original_timing.entry(id).or_insert(timing); }
                }
                if a.flag("freeze") { return Ok(Output::msg("クオンタイズを固定しました")); }
                let g = grid(p, a, "grid")?;
                let strength = a.opt_i64("strength").unwrap_or(100) as u32;
                // The grid counts from each bar of the song (UI-339), so the
                // notes are quantized in song time.
                let tm = p.time_map();
                let starts: HashMap<ClipId, Tick> = p
                    .tracks
                    .values()
                    .flat_map(|t| t.clips.iter().map(|c| (c.id, c.start)))
                    .collect();
                if let Some(q) = panel_quantize(p, a, g, strength)? {
                    let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, cid| {
                        let offset = starts.get(&cid).copied().unwrap_or(0);
                        (n.start, n.length, n.velocity) = q.note(&tm, offset, n.start, n.length, n.velocity);
                        Ok(())
                    })?;
                    return Ok(Output::msg(format!("ノート {n} 個をクオンタイズしました")));
                }
                let lengths = a.flag("lengths");
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, cid| {
                    let offset = starts.get(&cid).copied().unwrap_or(0);
                    n.start = (tm.quantize(offset + n.start, g, strength) - offset).max(0);
                    if lengths {
                        let q = quantize_tick(n.length, g, strength);
                        n.length = q.max(if strength == 100 { g } else { 1 });
                    }
                    Ok(())
                })?;
                Ok(Output::msg(format!(
                    "ノート {n} 個を {g} tick のグリッドにクオンタイズしました"
                )))
            },
        ),
        edit(
            "note.transpose",
            "ノートを半音単位でトランスポーズします（例: --by 12 で 1 オクターブ上）",
            vec![
                scope(),
                ArgDef::int("by", "半音数（負の値で下へ）")
                    .req()
                    .range(-127.0, 127.0),
                super::overlap::note_arg(),
            ],
            |p, cx, a| {
                let by = a.i64("by")?;
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
                    let to = n.pitch as i64 + by;
                    if !(0..=127).contains(&to) {
                        return Err(CmdError::range(format!(
                            "ノート {} を {by} 半音移動すると範囲外になります（{} → {to}）",
                            n.id, n.pitch
                        )));
                    }
                    n.pitch = to as u8;
                    Ok(())
                })?;
                Ok(Output::msg(format!("ノート {n} 個を {by} 半音トランスポーズしました")))
            },
        ),
        edit(
            "note.velocity.set",
            "ノートのベロシティを設定します（--value で同じ値に、--by で増減。1〜127 の範囲に収めます）",
            vec![
                scope(),
                ArgDef::int("value", "ベロシティ 1〜127").range(1.0, 127.0),
                ArgDef::int("by", "増減する量"),
            ],
            |p, cx, a| {
                let (value, by) = (a.opt_i64("value"), a.opt_i64("by"));
                if value.is_some() == by.is_some() {
                    return Err(CmdError::invalid("--value か --by のどちらか 1 つを指定してください"));
                }
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
                    let v = value.unwrap_or_else(|| n.velocity as i64 + by.unwrap_or(0));
                    n.velocity = v.clamp(1, 127) as u8;
                    Ok(())
                })?;
                Ok(Output::msg(format!("ノート {n} 個のベロシティを変更しました")))
            },
        ),
        edit(
            "note.mute",
            "ノートのミュートを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [vec![scope()], on_off().to_vec()].concat(),
            |p, cx, a| {
                let mut want = a.toggle()?;
                let n = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
                    let on = *want.get_or_insert(!n.muted);
                    n.muted = on;
                    Ok(())
                })?;
                Ok(Output::msg(format!(
                    "ノート {n} 個のミュートを{}",
                    onoff_word(want.unwrap_or(false))
                )))
            },
        ),
        edit(
            "note.set",
            "複数のノートを 1 回の操作でまとめて変更します（ドラッグ操作の結果の確定に使用）",
            vec![
                ArgDef::objects(
                    "notes",
                    "[{id, start?, length?, pitch?, velocity?}] の配列。start はクリップの先頭からの tick",
                )
                .req(),
                super::overlap::note_arg(),
            ],
            set_notes,
        ),
        edit(
            "note.put",
            "複数のノートを 1 回の操作でまとめて追加します（鉛筆ツールでの連続入力に使用）",
            vec![
                ArgDef::id("clip", IdKind::Clip, "追加先の MIDI クリップ").req(),
                ArgDef::objects(
                    "notes",
                    "[{at, pitch, length, velocity?, muted?}] の配列。at・length は「小節.拍.tick」または tick 数で、at はクリップの先頭から",
                )
                .req(),
                ArgDef::flag(
                    "grow",
                    "クリップの終わりより後ろのノートも置けるようにし、クリップをそのノートを含む小節の終わりまで伸ばします",
                ),
                super::overlap::note_arg(),
            ],
            put_notes,
        ),
        edit(
            "note.duplicate",
            "ノートを複製します（--by tick 後ろ、--pitch_by 半音上にずらして配置）",
            vec![
                scope(),
                ArgDef::int("by", "移動量（tick。負の値で前へ）").req(),
                ArgDef::int("pitch_by", "半音数（負の値で下へ）")
                    .default(json!(0))
                    .range(-127.0, 127.0),
                super::overlap::note_arg(),
            ],
            duplicate_notes,
        ),
        edit(
            "note.split",
            "ノートを指定位置で分割します（位置を含まないノートはそのまま）",
            vec![
                scope(),
                ArgDef::pos("at", "分割する位置（曲の中の位置。小節.拍.tick または tick 数）").req(),
            ],
            split_notes,
        ),
    ]
}

fn scope_of(ids: impl IntoIterator<Item = NoteId>) -> Scope {
    Scope {
        notes: ids.into_iter().collect(),
        clips: Default::default(),
    }
}

/// Puts new notes into their clips, giving each a fresh id. Returns the ids in order.
pub(super) fn add_copies(
    p: &mut Project,
    copies: Vec<(ClipId, Note)>,
    ai: bool,
) -> CmdResult<Vec<NoteId>> {
    let mut ids = Vec::new();
    for (cid, mut note) in copies {
        note.id = NoteId(p.alloc());
        note.ai = ai;
        ids.push(note.id);
        let (_, clip) = p.clip_mut(cid)?;
        if let ClipBody::Midi { notes, .. } = &mut clip.body {
            Arc::make_mut(notes).push(note);
        }
    }
    Ok(ids)
}

pub(super) fn set_notes(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let list = match a.0.get("notes") {
        Some(Value::Array(v)) if !v.is_empty() => v.clone(),
        _ => {
            return Err(CmdError::invalid(
                "notes に 1 つ以上のノートを指定してください",
            ));
        }
    };
    let mut changes: HashMap<NoteId, Map<String, Value>> = HashMap::new();
    for (i, item) in list.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            return Err(CmdError::invalid(format!(
                "{} 番目が {{id, ...}} の形式ではありません",
                i + 1
            )));
        };
        let id: NoteId = obj
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| CmdError::invalid(format!("{} 番目に id がありません", i + 1)))?
            .parse()
            .map_err(CmdError::invalid)?;
        changes.insert(id, obj.clone());
    }
    let scope = scope_of(changes.keys().copied());
    let n = edit_notes(p, &scope, cx.author.is_ai(), |n, _| {
        let c = &changes[&n.id];
        let int = |key: &str| c.get(key).map(|v| v.as_i64().ok_or(key.to_string()));
        let num = |key: &str, lo: i64, hi: i64| -> CmdResult<Option<i64>> {
            match int(key) {
                None => Ok(None),
                Some(Err(k)) => Err(CmdError::invalid(format!("{k} には整数を指定してください"))),
                Some(Ok(v)) if (lo..=hi).contains(&v) => Ok(Some(v)),
                Some(Ok(v)) => Err(CmdError::range(format!(
                    "ノート {} の {key} {v} は {lo}〜{hi} の範囲外です",
                    n.id
                ))),
            }
        };
        if let Some(v) = num("start", 0, i64::MAX)? {
            n.start = v;
        }
        if let Some(v) = num("length", 1, i64::MAX)? {
            n.length = v;
        }
        if let Some(v) = num("pitch", 0, 127)? {
            n.pitch = v as u8;
        }
        if let Some(v) = num("velocity", 1, 127)? {
            n.velocity = v as u8;
        }
        Ok(())
    })?;
    Ok(Output::msg(format!("ノート {n} 個を変更しました")))
}

fn put_notes(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let list = match a.0.get("notes") {
        Some(Value::Array(v)) if !v.is_empty() => v.clone(),
        _ => {
            return Err(CmdError::invalid(
                "notes に 1 つ以上のノートを指定してください",
            ));
        }
    };
    let grow = a.flag("grow");
    let (clip_start, clip_length) = {
        let c = p.clip(cid)?.1;
        if !c.is_midi() {
            return Err(CmdError::conflict(format!(
                "{cid} は MIDI クリップではありません"
            )));
        }
        (c.start, c.length)
    };
    let mut notes = Vec::new();
    let mut last_end = clip_length;
    for (i, item) in list.iter().enumerate() {
        let field = |key: &str| item.get(key).and_then(Value::as_i64);
        let need = |key: &str, lo: i64, hi: i64| -> CmdResult<i64> {
            let v = match item.get(key) {
                Some(Value::Number(n)) => n.as_i64(),
                // `at` is where the note goes from the clip's start; `length` is how long it
                // sounds. A `bar.beat.tick` position is read where the clip begins.
                Some(Value::String(s)) if key == "at" || key == "length" => p
                    .time_map()
                    .parse(s)
                    .ok()
                    .map(|tick| if key == "at" { tick - clip_start } else { tick }),
                _ => None,
            };
            let Some(v) = v else {
                return Err(if key == "at" || key == "length" {
                    CmdError::invalid(format!(
                        "{} 番目の {key} は tick の整数か \"小節.拍.tick\" で指定してください",
                        i + 1
                    ))
                } else {
                    CmdError::invalid(format!("{} 番目の {key} は整数で指定してください", i + 1))
                });
            };
            if (lo..=hi).contains(&v) {
                Ok(v)
            } else {
                Err(CmdError::range(format!(
                    "{} 番目の {key} {v} は {lo}〜{hi} の範囲外です",
                    i + 1
                )))
            }
        };
        let at = need("at", 0, if grow { i64::MAX / 4 } else { clip_length - 1 })?;
        let length = need("length", 1, i64::MAX / 4)?;
        last_end = last_end.max(at + length);
        let pitch = need("pitch", 0, 127)?;
        let velocity = match field("velocity") {
            None => 100,
            Some(_) => need("velocity", 1, 127)?,
        };
        let muted = match item.get("muted") {
            None => false,
            Some(Value::Bool(v)) => *v,
            Some(_) => return Err(CmdError::invalid("muted には真偽値を指定してください")),
        };
        notes.push((
            cid,
            Note {
                id: NoteId(0),
                start: at,
                length,
                pitch: pitch as u8,
                velocity: velocity as u8,
                muted,
                ai: false,
            },
        ));
    }
    let ids = add_copies(p, notes, cx.author.is_ai())?;
    // The clip grows to the end of the bar that holds the last note's end.
    if grow && last_end > clip_length {
        let tm = p.time_map();
        let bar = tm.to_bbt(clip_start + last_end - 1).0;
        let new_end = tm.bar_start(bar + 1);
        p.clip_mut(cid)?.1.length = new_end - clip_start;
    }
    Ok(Output::with(
        format!("ノート {} 個を追加しました", ids.len()),
        json!({ "ids": ids.iter().map(ToString::to_string).collect::<Vec<_>>() }),
    ))
}

fn duplicate_notes(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let (by, pitch_by) = (a.i64("by")?, a.opt_i64("pitch_by").unwrap_or(0));
    let mut copies = Vec::new();
    // Read through `edit_notes` so that a clip in the scope counts as all its notes.
    let scope = Scope::from_args(a)?;
    edit_notes(p, &scope, false, |n, cid| {
        let mut c = *n;
        c.start += by;
        let pitch = i64::from(n.pitch) + pitch_by;
        if c.start < 0 || !(0..=127).contains(&pitch) {
            return Err(CmdError::range(format!(
                "ノート {} の複製が範囲外になります",
                n.id
            )));
        }
        c.pitch = pitch as u8;
        copies.push((cid, c));
        Ok(())
    })?;
    let ids = add_copies(p, copies, cx.author.is_ai())?;
    Ok(Output::with(
        format!("ノート {} 個を複製しました", ids.len()),
        json!({ "ids": ids.iter().map(ToString::to_string).collect::<Vec<_>>() }),
    ))
}

fn split_notes(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let at = tick(p, a, "at")?;
    let starts: HashMap<ClipId, Tick> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.id, c.start)))
        .collect();
    let mut rights = Vec::new();
    let originals = p.original_timing.clone();
    let mut left_timing = Vec::new();
    let mut right_timing = Vec::new();
    let scope = Scope::from_args(a)?;
    edit_notes(p, &scope, cx.author.is_ai(), |n, cid| {
        let rel = at - starts[&cid];
        if rel > n.start && rel < n.start + n.length {
            right_timing.push(originals.get(&n.id).map(|o| crate::editing::Timing {
                start: o.start.max(rel),
                length: (o.start + o.length - rel).max(1),
            }));
            if let Some(o) = originals.get(&n.id) {
                left_timing.push((
                    n.id,
                    crate::editing::Timing {
                        start: o.start.min(rel - 1),
                        length: (rel - o.start).clamp(1, o.length),
                    },
                ));
            }
            rights.push((
                cid,
                Note {
                    start: rel,
                    length: n.start + n.length - rel,
                    ..*n
                },
            ));
            n.length = rel - n.start;
        }
        Ok(())
    })?;
    if rights.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "分割する位置がどのノートにも含まれていません",
        ));
    }
    let ids = add_copies(p, rights, cx.author.is_ai())?;
    p.original_timing.extend(left_timing);
    p.original_timing.extend(
        ids.iter()
            .zip(right_timing)
            .filter_map(|(id, o)| o.map(|o| (*id, o))),
    );
    Ok(Output::with(
        format!("ノート {} 個を分割しました", ids.len()),
        json!({ "ids": ids.iter().map(ToString::to_string).collect::<Vec<_>>() }),
    ))
}

fn add_note(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let cid: ClipId = a.id("clip")?;
    let at = tick(p, a, "at")?;
    let length = grid(p, a, "length")?;
    let (pitch, velocity) = (a.i64("pitch")?, a.opt_i64("velocity").unwrap_or(100));
    let (start, clip_length) = {
        let c = p.clip(cid)?.1;
        if !c.is_midi() {
            return Err(CmdError::conflict(format!(
                "{cid} は MIDI クリップではありません"
            )));
        }
        (c.start, c.length)
    };
    let rel = at - start;
    if rel < 0 || rel >= clip_length {
        return Err(CmdError::range(format!(
            "位置がクリップ {cid} の範囲外です（{start}〜{} tick）",
            start + clip_length
        )));
    }
    let id = NoteId(p.alloc());
    let note = Note {
        id,
        start: rel,
        length,
        pitch: pitch as u8,
        velocity: velocity.clamp(1, 127) as u8,
        muted: false,
        ai: cx.author.is_ai(),
    };
    let (_, clip) = p.clip_mut(cid)?;
    if let ClipBody::Midi { notes, .. } = &mut clip.body {
        Arc::make_mut(notes).push(note);
    }
    Ok(created("ノート", id))
}

/// The panel's settings, when any is given (EDT-073); none keeps the plain quantize.
fn panel_quantize(
    p: &Project,
    a: &Args,
    step: Tick,
    strength: u32,
) -> CmdResult<Option<crate::quantize::Quantize>> {
    use crate::quantize::{Lines, Quantize};
    let given = [
        "tuplet",
        "swing",
        "end",
        "range",
        "groove",
        "velocity",
        "groove-hits",
    ]
    .iter()
    .any(|k| a.0.get(*k).is_some_and(|v| !v.is_null()));
    if !given {
        return Ok(None);
    }
    let pct = |k: &str, or: u32| a.opt_i64(k).map_or(or, |v| v.clamp(0, 100) as u32);
    let lines = if let Some(value) = a.0.get("groove-hits") {
        let period = a.i64("groove-period")?;
        let hits = super::edit_groove::groove_hits(value, period)?;
        Lines::Groove {
            origin: 0,
            period,
            hits,
        }
    } else {
        match a.opt_id::<ClipId>("groove")? {
            Some(id) => {
                let (_, c) = p.clip(id)?;
                let hits: Vec<(Tick, u8)> =
                    c.notes().iter().map(|n| (n.start, n.velocity)).collect();
                if hits.is_empty() {
                    return Err(CmdError::invalid(format!(
                        "クリップ {id} にノートが無いので、グルーヴにできません"
                    )));
                }
                Lines::Groove {
                    origin: c.start,
                    period: c.length.max(1),
                    hits,
                }
            }
            None => Lines::Grid {
                step,
                tuplet: a.opt_i64("tuplet").map_or(1, |v| v.clamp(1, 7) as u32),
                swing: pct("swing", 0),
            },
        }
    };
    Ok(Some(Quantize {
        lines,
        start: strength,
        end: a.opt_i64("end").map(|v| v.clamp(0, 100) as u32),
        velocity: pct("velocity", 0),
        range: pct("range", 100),
    }))
}
