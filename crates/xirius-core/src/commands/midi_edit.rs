//! The piano roll's edits that work on a group of notes (EDT-022, EDT-054, EDT-060, EDT-076):
//! scatter the velocities, scale and tilt them, spread a chord out, split on the grid.

use std::collections::HashMap;

use super::*;
use crate::id::{ClipId, IdKind, NoteId};
use crate::model::Note;

pub(super) fn defs() -> Vec<CommandDef> {
    let scope = || ArgDef::ids(IdKind::NoteScope, "ノート");
    vec![
        edit(
            "note.velocity.humanize",
            "ノートのベロシティだけを少し散らします（タイミングは動かしません）",
            vec![
                scope(),
                ArgDef::int("amount", "ベロシティの最大変化量")
                    .default(json!(8))
                    .range(0.0, 126.0),
                ArgDef::int("seed", "乱数の種").default(json!(1)),
                ArgDef::flag("less", "変化量を半分にする"),
            ],
            velocity_humanize,
        ),
        edit(
            "note.velocity.transform",
            "ベロシティを、平均を中心に縮尺し（--scale）、時間に沿って傾けます（--tilt）",
            vec![
                scope(),
                ArgDef::int("scale", "縮尺 %（100 で変えない。0 で平均に揃える）")
                    .default(json!(100))
                    .range(0.0, 400.0),
                ArgDef::int(
                    "tilt",
                    "最後のノートで足す量（最初のノートは 0。負で下げる）",
                )
                .default(json!(0))
                .range(-126.0, 126.0),
            ],
            velocity_transform,
        ),
        edit(
            "note.strum",
            "同じ位置で始まる和音のノートを、低い音から（--spread が負なら高い音から）順にずらします",
            vec![
                scope(),
                ArgDef::int(
                    "spread",
                    "最初のノートと最後のノートの開始の差（tick。負で高い音から）",
                )
                .req()
                .range(-9600.0, 9600.0),
            ],
            strum,
        ),
        edit(
            "note.split.grid",
            "ノートをグリッドの線ごとに分割します",
            vec![
                scope(),
                ArgDef::grid("grid", "グリッド（1/16・1/8t・1/4d・tick 数）")
                    .default(json!("1/16")),
            ],
            split_on_grid,
        ),
        read(
            "quantize.program",
            "クオンタイズのプログラム A〜G を選びます（--save で今の値をそのプログラムに保存します）",
            vec![
                ArgDef::choice("program", &PROGRAMS, "プログラム").req(),
                ArgDef::flag(
                    "save",
                    "選ぶ代わりに、今のグリッドとクオンタイズの設定を保存する",
                ),
            ],
            quantize_program,
        ),
    ]
}

/// The letters of the quantize programs, in the order of Mod+Alt+1 to 7.
pub const PROGRAMS: [&str; 7] = ["A", "B", "C", "D", "E", "F", "G"];

/// What a program is before anyone saves over it: the grid, and the panel at rest (G swings).
fn built_in(letter: &str) -> Value {
    let grid = match letter {
        "A" => "1/4",
        "B" => "1/8",
        "C" | "G" => "1/16",
        "D" => "1/32",
        "E" => "1/8t",
        _ => "1/16t",
    };
    let swing = if letter == "G" { 50 } else { 0 };
    json!({
        "grid": grid,
        "panel": { "groove_on": false, "tuplet": 1, "swing": swing,
                   "start": 100, "end": 0, "velocity": 100, "range": 100 },
    })
}

fn quantize_program(
    _: &Project,
    s: &mut crate::session::SessionState,
    _: &crate::command::Registry,
    a: &Args,
) -> CmdResult<Output> {
    let letter = a.str("program")?.to_uppercase();
    if !PROGRAMS.contains(&letter.as_str()) {
        return Err(CmdError::invalid(format!(
            "プログラム `{letter}` はありません（A〜G）"
        )));
    }
    let in_roll = s.view.editor == "roll";
    if a.flag("save") {
        let (grid, panel) = if in_roll {
            (s.view.roll.grid.clone(), s.view.roll.quantize.clone())
        } else {
            (s.view.grid.clone(), s.view.quantize.clone())
        };
        s.view
            .quantize_programs
            .insert(letter.clone(), json!({ "grid": grid, "panel": panel }));
        return Ok(Output::msg(format!("プログラム {letter} に保存しました")));
    }
    let program = s
        .view
        .quantize_programs
        .get(&letter)
        .cloned()
        .unwrap_or_else(|| built_in(&letter));
    let grid = program["grid"].as_str().unwrap_or("1/16").to_string();
    let panel = program["panel"].clone();
    if in_roll {
        s.view.roll.grid = grid.clone();
        s.view.roll.quantize = panel;
    } else {
        s.view.grid = grid.clone();
        s.view.quantize = panel;
    }
    s.view.quantize_program = Some(letter.clone());
    Ok(Output::with(
        format!("クオンタイズのプログラム {letter}（{grid}）にしました"),
        json!({ "program": letter, "grid": grid }),
    ))
}

fn velocity_humanize(
    p: &mut Project,
    cx: &crate::command::EditCtx<'_>,
    a: &Args,
) -> CmdResult<Output> {
    let amount = a.opt_i64("amount").unwrap_or(8) / if a.flag("less") { 2 } else { 1 };
    let mut state = a.opt_i64("seed").unwrap_or(1) as u64;
    let count = edit_notes(p, &Scope::from_args(a)?, cx.author.is_ai(), |n, _| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let d = ((state >> 32) % (2 * amount + 1) as u64) as i64 - amount;
        n.velocity = (i64::from(n.velocity) + d).clamp(1, 127) as u8;
        Ok(())
    })?;
    Ok(Output::msg(format!(
        "ノート {count} 個のベロシティを散らしました"
    )))
}

/// The song-wide start of every note in `scope`, with the clip it is in.
fn starts_of(p: &mut Project, scope: &Scope) -> CmdResult<Vec<(NoteId, ClipId, Tick, u8)>> {
    let offsets: HashMap<ClipId, Tick> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.id, c.start)))
        .collect();
    let mut all = Vec::new();
    edit_notes(p, scope, false, |n, c| {
        all.push((n.id, c, n.start + offsets[&c], n.pitch));
        Ok(())
    })?;
    Ok(all)
}

fn velocity_transform(
    p: &mut Project,
    cx: &crate::command::EditCtx<'_>,
    a: &Args,
) -> CmdResult<Output> {
    let scope = Scope::from_args(a)?;
    let scale = a.opt_i64("scale").unwrap_or(100) as f64 / 100.0;
    let tilt = a.opt_i64("tilt").unwrap_or(0) as f64;
    let mut seen = Vec::new();
    edit_notes(p, &scope, false, |n, _| {
        seen.push(f64::from(n.velocity));
        Ok(())
    })?;
    let at: HashMap<NoteId, Tick> = starts_of(p, &scope)?
        .into_iter()
        .map(|(id, _, t, _)| (id, t))
        .collect();
    let lo = at.values().copied().min().unwrap_or(0);
    let hi = at.values().copied().max().unwrap_or(lo);
    let mean = seen.iter().sum::<f64>() / seen.len().max(1) as f64;
    let count = edit_notes(p, &scope, cx.author.is_ai(), |n, _| {
        let along = if hi == lo {
            0.0
        } else {
            (at[&n.id] - lo) as f64 / (hi - lo) as f64
        };
        let v = mean + (f64::from(n.velocity) - mean) * scale + tilt * along;
        n.velocity = v.round().clamp(1.0, 127.0) as u8;
        Ok(())
    })?;
    Ok(Output::msg(format!(
        "ノート {count} 個のベロシティを変形しました"
    )))
}

fn strum(p: &mut Project, cx: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let scope = Scope::from_args(a)?;
    let spread = a.i64("spread")?;
    let mut chords: HashMap<(ClipId, Tick), Vec<(NoteId, u8)>> = HashMap::new();
    edit_notes(p, &scope, false, |n, c| {
        chords
            .entry((c, n.start))
            .or_default()
            .push((n.id, n.pitch));
        Ok(())
    })?;
    let mut shift: HashMap<NoteId, Tick> = HashMap::new();
    for chord in chords.values_mut() {
        if chord.len() < 2 {
            continue;
        }
        chord.sort_by_key(|&(id, pitch)| (pitch, id));
        if spread < 0 {
            chord.reverse();
        }
        let last = (chord.len() - 1) as i64;
        for (i, (id, _)) in chord.iter().enumerate() {
            shift.insert(*id, spread.abs() * i as i64 / last);
        }
    }
    if shift.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "同じ位置で始まるノートが 2 つ以上ありません",
        ));
    }
    let count = edit_notes(p, &scope, cx.author.is_ai(), |n, _| {
        let by = shift.get(&n.id).copied().unwrap_or(0).min(n.length - 1);
        n.start += by;
        n.length -= by;
        Ok(())
    })?;
    Ok(Output::msg(format!("ノート {count} 個をストラムしました")))
}

fn split_on_grid(p: &mut Project, cx: &crate::command::EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let step = grid(p, a, "grid")?.max(1);
    let scope = Scope::from_args(a)?;
    let offsets: HashMap<ClipId, Tick> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.id, c.start)))
        .collect();
    let tm = p.time_map();
    let mut pieces: Vec<(ClipId, Note)> = Vec::new();
    edit_notes(p, &scope, cx.author.is_ai(), |n, c| {
        let offset = offsets[&c];
        let end = n.start + n.length;
        // The first line after `t` (clip time), counted from its bar in song
        // time (UI-339).
        let next = |t: Tick| tm.on_grid(offset + t + 1, step, crate::time::Side::After) - offset;
        let mut line = next(n.start);
        let mut from = n.start;
        let mut first = true;
        while line < end {
            if first {
                n.length = line - from;
                first = false;
            } else {
                pieces.push((
                    c,
                    Note {
                        start: from,
                        length: line - from,
                        ..*n
                    },
                ));
            }
            from = line;
            line = next(line);
        }
        if !first {
            pieces.push((
                c,
                Note {
                    start: from,
                    length: end - from,
                    ..*n
                },
            ));
        }
        Ok(())
    })?;
    if pieces.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "グリッドの線をまたぐノートがありません",
        ));
    }
    let n = pieces.len();
    super::note::add_copies(p, pieces, cx.author.is_ai())?;
    Ok(Output::msg(format!("ノートを {n} 個に分けて足しました")))
}
