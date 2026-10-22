//! Tempo, meter, sections, the song's own settings, the read-only commands,
//! selection and view, and the declarations of the commands the session runs
//! itself (history, files, versions, AI permission).

use serde_json::{Value, json};

use super::*;
use crate::command::{EditCtx, Registry};
use crate::id::{IdKind, SectionId, TempoId, TimeSigId, TrackId};
use crate::model::{Section, TempoPoint, TimeSigPoint, TrackKind};
use crate::session::{SessionState, exists};

pub(super) fn defs() -> Vec<CommandDef> {
    let mut v = editing();
    v.extend(reading());
    v.extend(selection_and_view());
    v.extend(session_commands());
    v
}

fn editing() -> Vec<CommandDef> {
    vec![
        edit(
            "project.settings",
            "曲の設定（サンプルレート・ビット深度・ファイルの置き場所）を変更します。サンプルレートとビット深度は再生と書き出しの初期値になります。置き場所を変えても、今あるファイルは動かしません",
            vec![
                ArgDef::int("sample_rate", "サンプルレート（Hz。8000〜384000）")
                    .range(8000.0, 384_000.0),
                ArgDef::int("bit_depth", "ビット深度（16・24・32。32 は浮動小数点）"),
                ArgDef::text(
                    "media_folder",
                    "録音・バウンス・フリーズ・コピーしたファイルの置き場所（曲のフォルダーからの相対パスか絶対パス。既定は Media）",
                ),
                ArgDef::flag(
                    "copy_imports",
                    "曲のフォルダーの外からインポートしたファイルを、置き場所の Imports にコピーします（false でコピーしません。既定は true）",
                ),
            ],
            |p, _, a| {
                let mut s = p.settings;
                if let Some(r) = a.opt_i64("sample_rate") {
                    s.sample_rate = r as u32;
                }
                if let Some(b) = a.opt_i64("bit_depth") {
                    if !matches!(b, 16 | 24 | 32) {
                        return Err(CmdError::range(
                            "--bit_depth には 16・24・32 のいずれかを指定してください",
                        ));
                    }
                    s.bit_depth = b as u16;
                }
                let mut m = p.media.clone();
                if let Some(f) = a.opt_str("media_folder") {
                    m.folder = media_folder(f)?;
                }
                if let Some(c) = a.opt_flag("copy_imports") {
                    m.copy_imports = c;
                }
                p.settings = s;
                let mut message = format!("{} Hz・{} bit", s.sample_rate, s.bit_depth);
                if a.has("media_folder") || a.has("copy_imports") {
                    message.push_str(&format!(
                        "・置き場所 {}・インポートしたファイルを{}",
                        m.folder,
                        if m.copy_imports {
                            "コピーします"
                        } else {
                            "コピーしません"
                        }
                    ));
                }
                let data = json!({
                    "sample_rate": s.sample_rate,
                    "bit_depth": s.bit_depth,
                    "media_folder": m.folder,
                    "copy_imports": m.copy_imports,
                });
                p.media = m;
                Ok(Output::with(message, data))
            },
        ),
        edit(
            "loop.set",
            "ループ範囲を設定します（範囲内を繰り返し再生でき、書き出しの範囲にも使えます）",
            vec![
                ArgDef::pos("start", "開始位置").req(),
                ArgDef::pos("end", "終了位置（開始位置より後）").req(),
            ],
            |p, _, a| {
                let (start, end) = (tick(p, a, "start")?, tick(p, a, "end")?);
                if start < 0 || end <= start {
                    return Err(CmdError::range(
                        "ループ範囲は 0 以上で、終了位置を開始位置より後にしてください",
                    ));
                }
                p.loop_range = Some(crate::model::LoopRange { start, end });
                let tm = p.time_map();
                Ok(Output::msg(format!(
                    "ループ範囲を {}〜{} に設定しました",
                    tm.format(start),
                    tm.format(end)
                )))
            },
        ),
        edit(
            "loop.clear",
            "ループ範囲を解除します",
            vec![],
            |p, _, _| {
                if p.loop_range.take().is_none() {
                    return Ok(Output::msg("ループ範囲は設定されていません"));
                }
                Ok(Output::msg("ループ範囲を解除しました"))
            },
        ),
        edit(
            "tempo.set",
            "テンポを設定します。指定位置にテンポポイントがあれば変更し、無ければ追加します",
            vec![
                ArgDef::float("bpm", "BPM（1〜999）")
                    .req()
                    .range(1.0, 999.0),
                ArgDef::pos("at", "位置（省略時は曲の先頭）"),
                ArgDef::flag("ramp", "次のテンポポイントまでなめらかに変化させる"),
            ],
            |p, _, a| {
                let at = opt_tick(p, a, "at")?.unwrap_or(0);
                if at < 0 {
                    return Err(CmdError::range("位置は 0 以上で指定してください"));
                }
                let bpm = a.f64("bpm")?;
                if let Some(t) = p.tempo.iter_mut().find(|t| t.tick == at) {
                    t.bpm = bpm;
                    if let Some(r) = a.opt_flag("ramp") {
                        t.ramp = r;
                    }
                    return Ok(Output::msg(format!("テンポを {bpm} に設定しました")));
                }
                let id = TempoId(p.alloc());
                p.tempo.push(TempoPoint {
                    id,
                    tick: at,
                    bpm,
                    ramp: a.flag("ramp"),
                });
                p.tempo.sort_by_key(|t| t.tick);
                Ok(created("テンポポイント", id))
            },
        ),
        destroy(
            "tempo.delete",
            "テンポポイントを削除します（曲の先頭のポイントは削除できません）",
            vec![ArgDef::id("id", IdKind::Tempo, "テンポポイント").req()],
            |p, _, a| {
                let id: TempoId = a.id("id")?;
                let i = p.tempo.iter().position(|t| t.id == id).ok_or_else(|| {
                    CmdError::not_found(format!("テンポポイント {id} が見つかりません"))
                })?;
                if p.tempo[i].tick == 0 {
                    return Err(CmdError::conflict(
                        "曲の先頭のテンポは削除できません（tempo.set で変更してください）",
                    ));
                }
                p.tempo.remove(i);
                Ok(Output::msg(format!("{id} を削除しました")))
            },
        ),
        destroy(
            "tempo.clear",
            "テンポの変化をすべて消します（曲の先頭のテンポだけが残ります）",
            vec![],
            |p, _, _| {
                let before = p.tempo.len();
                p.tempo.retain(|t| t.tick == 0);
                if p.tempo.len() == before {
                    return Err(CmdError::new(
                        crate::error::ErrorCode::NothingToDo,
                        "消すテンポの変化がありません",
                    ));
                }
                Ok(Output::msg(format!(
                    "テンポの変化を {} 個消しました",
                    before - p.tempo.len()
                )))
            },
        ),
        edit(
            "timesig.set",
            "拍子を設定します。指定した小節に拍子の変更点があれば変更し、無ければ追加します",
            vec![
                ArgDef::int("num", "拍数").req().range(1.0, 64.0),
                ArgDef::int("den", "1 拍の音符の長さ（1・2・4・8・16・32 分音符）").req(),
                ArgDef::int("bar", "小節（省略時は 1）")
                    .default(json!(1))
                    .range(1.0, 100_000.0),
            ],
            |p, _, a| {
                let (num, den, bar) = (
                    a.i64("num")? as u32,
                    a.i64("den")? as u32,
                    a.i64("bar")? as u32,
                );
                if !matches!(den, 1 | 2 | 4 | 8 | 16 | 32) {
                    return Err(CmdError::range(
                        "--den には 1・2・4・8・16・32 のいずれかを指定してください",
                    ));
                }
                if let Some(t) = p.timesig.iter_mut().find(|t| t.bar == bar) {
                    t.num = num;
                    t.den = den;
                    return Ok(Output::msg(format!(
                        "{bar} 小節目の拍子を {num}/{den} に設定しました"
                    )));
                }
                let id = TimeSigId(p.alloc());
                p.timesig.push(TimeSigPoint { id, bar, num, den });
                p.timesig.sort_by_key(|t| t.bar);
                Ok(created("拍子の変更点", id))
            },
        ),
        destroy(
            "timesig.delete",
            "拍子の変更点を削除します（1 小節目の拍子は削除できません）",
            vec![ArgDef::id("id", IdKind::TimeSig, "拍子の変更点").req()],
            |p, _, a| {
                let id: TimeSigId = a.id("id")?;
                let i =
                    p.timesig.iter().position(|t| t.id == id).ok_or_else(|| {
                        CmdError::not_found(format!("拍子 {id} が見つかりません"))
                    })?;
                if p.timesig[i].bar == 1 {
                    return Err(CmdError::conflict(
                        "1 小節目の拍子は削除できません（timesig.set で変更してください）",
                    ));
                }
                p.timesig.remove(i);
                Ok(Output::msg(format!("{id} を削除しました")))
            },
        ),
        edit(
            "section.add",
            "セクション（A メロ・サビなど）を追加します",
            vec![
                ArgDef::text("name", "名前").req(),
                ArgDef::pos("at", "開始位置（--range のときは不要）"),
                ArgDef::int("length", "長さ（tick）"),
                ArgDef::float(
                    "bars",
                    "長さ（小節。省略時は 1 小節で、次のセクションの手前までに収まります）",
                ),
                ArgDef::text("range", "loop または 開始..終了（at・length の代わり）"),
                ArgDef::text("color", "#rrggbb").default(json!("none")),
            ],
            |p, _, a| {
                let name = a.str("name")?.trim().to_string();
                if name.is_empty() {
                    return Err(CmdError::invalid("名前を入力してください"));
                }
                let (start, length) = if let Some(text) = a.opt_str("range") {
                    let (from, to) = super::edit_range::parse_range(p, text)?;
                    (from, to - from)
                } else {
                    let start = tick(p, a, "at")?;
                    if start < 0 {
                        return Err(CmdError::range("位置は 0 以上で指定してください"));
                    }
                    let mut length = length_arg(p, a, start)?;
                    // a length that was not asked for gives way to the next section
                    if !a.has("length")
                        && !a.has("bars")
                        && let Some(next) = p
                            .sections
                            .iter()
                            .filter(|s| s.start > start)
                            .map(|s| s.start)
                            .min()
                    {
                        length = length.min(next - start);
                    }
                    (start, length)
                };
                if start < 0 || length <= 0 {
                    return Err(CmdError::range("セクションの範囲が正しくありません"));
                }
                // ARR-007: sections do not overlap
                if let Some(o) = p
                    .sections
                    .iter()
                    .find(|s| s.start < start + length && start < s.start + s.length)
                {
                    return Err(CmdError::conflict(format!(
                        "セクション「{}」（{}）と重なるため、追加できません",
                        o.name, o.id
                    )));
                }
                let id = SectionId(p.alloc());
                p.sections.push(Section {
                    id,
                    name: name.clone(),
                    start,
                    length,
                    color: color_arg(a)?,
                });
                p.sections.sort_by_key(|s| (s.start, s.id));
                Ok(created(&format!("セクション「{name}」"), id))
            },
        ),
        destroy(
            "section.delete",
            "セクションを削除します",
            vec![ArgDef::id("id", IdKind::Section, "セクション").req()],
            |p, _, a| {
                let id: SectionId = a.id("id")?;
                let i = section_index(p, id)?;
                p.sections.remove(i);
                Ok(Output::msg(format!("{id} を削除しました")))
            },
        ),
        edit(
            "section.rename",
            "セクションの名前を変更します",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::text("name", "新しい名前").req(),
            ],
            |p, _, a| {
                let id: SectionId = a.id("id")?;
                let i = section_index(p, id)?;
                let name = a.str("name")?.trim().to_string();
                if name.is_empty() {
                    return Err(CmdError::invalid("名前を入力してください"));
                }
                p.sections[i].name = name.clone();
                Ok(Output::msg(format!(
                    "{id} の名前を「{name}」に変更しました"
                )))
            },
        ),
        edit(
            "section.move",
            "セクションを移動します（--at で位置を指定、--by で相対移動）",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::pos("at", "新しい開始位置").alias(&["to"]),
                ArgDef::int("by", "移動量（tick）"),
            ],
            |p, _, a| {
                let id: SectionId = a.id("id")?;
                let i = section_index(p, id)?;
                let (to, by) = (opt_tick(p, a, "at")?, a.opt_i64("by"));
                if to.is_some() == by.is_some() {
                    return Err(CmdError::invalid(
                        "--at か --by のどちらか 1 つを指定してください",
                    ));
                }
                let start = to.unwrap_or_else(|| p.sections[i].start + by.unwrap_or(0));
                if start < 0 {
                    return Err(CmdError::range("曲の先頭より前には移動できません"));
                }
                // ARR-007: sections do not overlap
                let length = p.sections[i].length;
                if let Some(o) = p
                    .sections
                    .iter()
                    .find(|s| s.id != id && s.start < start + length && start < s.start + s.length)
                {
                    return Err(CmdError::conflict(format!(
                        "セクション「{}」（{}）と重なるため、移動できません",
                        o.name, o.id
                    )));
                }
                p.sections[i].start = start;
                p.sections.sort_by_key(|s| (s.start, s.id));
                Ok(Output::msg(format!("{id} を移動しました")))
            },
        ),
        edit(
            "section.resize",
            "セクションの長さを変更します（次のセクションの手前までで止まり、重なりません）",
            vec![
                ArgDef::id("id", IdKind::Section, "セクション").req(),
                ArgDef::int("length", "長さ（tick）"),
                ArgDef::float("bars", "長さ（小節）"),
            ],
            |p, _, a| {
                let id: SectionId = a.id("id")?;
                let i = section_index(p, id)?;
                if !a.has("length") && !a.has("bars") {
                    return Err(CmdError::invalid("--length か --bars が必要です"));
                }
                let start = p.sections[i].start;
                let mut length = length_arg(p, a, start)?;
                // ARR-007: the next section is a wall
                let room = p
                    .sections
                    .iter()
                    .filter(|s| s.id != id && s.start > start)
                    .map(|s| s.start - start)
                    .min();
                let stopped = room.is_some_and(|r| length > r);
                if let Some(r) = room {
                    length = length.min(r);
                }
                if stopped && length == p.sections[i].length {
                    return Err(CmdError::new(
                        crate::error::ErrorCode::NothingToDo,
                        "次のセクションに接しているため、これ以上は伸ばせません",
                    ));
                }
                p.sections[i].length = length;
                if stopped {
                    return Ok(Output::msg(format!(
                        "{id} の長さを変更しました（次のセクションの手前までにしました）"
                    )));
                }
                Ok(Output::msg(format!("{id} の長さを変更しました")))
            },
        ),
        edit(
            "project.rename",
            "曲の名前を変更します",
            vec![ArgDef::text("name", "新しい名前").req()],
            |p, _: &EditCtx<'_>, a| {
                let name = a.str("name")?.trim().to_string();
                if name.is_empty() {
                    return Err(CmdError::invalid("名前を入力してください"));
                }
                p.name = name.clone();
                Ok(Output::msg(format!("曲の名前を「{name}」に変更しました")))
            },
        ),
    ]
}

/// `--media_folder` as the song keeps it: trimmed, with `/` in a relative path.
fn media_folder(given: &str) -> CmdResult<String> {
    let f = given.trim();
    if f.is_empty() {
        return Err(CmdError::invalid("--media_folder が空です"));
    }
    if std::path::Path::new(f).is_absolute() {
        return Ok(f.to_string());
    }
    Ok(f.replace('\\', "/").trim_end_matches('/').to_string())
}

fn section_index(p: &Project, id: SectionId) -> CmdResult<usize> {
    p.sections
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| CmdError::not_found(format!("セクション {id} が見つかりません")))
}

// ---- reading ---------------------------------------------------------------------------

fn reading() -> Vec<CommandDef> {
    vec![
        read(
            "project.get",
            "曲の概要（名前・テンポ・拍子・セクション・各要素の数）を表示します",
            vec![],
            |p, _, _, _| {
                let tm = p.time_map();
                let tempo: Vec<Value> = p
                .tempo
                .iter()
                .map(|t| json!({"id": t.id, "at": tm.format(t.tick), "tick": t.tick, "bpm": t.bpm, "ramp": t.ramp}))
                .collect();
                let sig: Vec<Value> =
                p.timesig.iter().map(|t| json!({"id": t.id, "bar": t.bar, "meter": format!("{}/{}", t.num, t.den)})).collect();
                let sections: Vec<Value> = p
                .sections
                .iter()
                .map(|s| json!({"id": s.id, "name": s.name, "at": tm.format(s.start), "start": s.start, "length": s.length}))
                .collect();
                let clips: usize = p.tracks.values().map(|t| t.clips.len()).sum();
                let notes: usize = p
                    .tracks
                    .values()
                    .flat_map(|t| t.clips.iter())
                    .map(|c| c.notes().len())
                    .sum();
                let data = json!({
                    "name": p.name,
                    "ppq": p.ppq,
                    "sample_rate": p.settings.sample_rate,
                    "bit_depth": p.settings.bit_depth,
                    "media_folder": p.media.folder,
                    "copy_imports": p.media.copy_imports,
                    "loop": p.loop_range.map(|l| json!({"start": l.start, "end": l.end})),
                    "tracks": p.tracks.len() - 1,
                    "folders": p.folders.len(),
                    "clips": clips,
                    "notes": notes,
                    "tempo": tempo,
                    "timesig": sig,
                    "sections": sections,
                });
                Ok(Output::with(
                    format!(
                        "「{}」トラック {}・クリップ {clips}・ノート {notes}",
                        p.name,
                        p.tracks.len() - 1
                    ),
                    data,
                ))
            },
        ),
        read(
            "track.list",
            "トラックを上から順に一覧表示します（フォルダーの入れ子は depth で示します）",
            vec![],
            track_list,
        ),
        read(
            "clip.list",
            "クリップを一覧表示します",
            vec![ArgDef::id(
                "track",
                IdKind::Track,
                "このトラックのクリップのみ",
            )],
            |p, _, _, a| {
                let only: Option<TrackId> = a.opt_id("track")?;
                let tm = p.time_map();
                let mut rows = Vec::new();
                for t in p.tracks.values() {
                    if only.is_some_and(|o| o != t.id) {
                        continue;
                    }
                    for c in &t.clips {
                        rows.push(json!({
                            "id": c.id, "track": t.id, "name": c.name,
                            "kind": if c.is_midi() { "midi" } else { "audio" },
                            "at": tm.format(c.start), "start": c.start, "length": c.length,
                            "muted": c.muted, "notes": c.notes().len(),
                        }));
                    }
                }
                rows.sort_by_key(|r| r["start"].as_i64().unwrap_or(0));
                Ok(Output::with(
                    format!("クリップ {} 件", rows.len()),
                    Value::Array(rows),
                ))
            },
        ),
        read(
            "note.list",
            "ノートを一覧表示します",
            vec![
                ArgDef::id("clip", IdKind::Clip, "このクリップのノートのみ"),
                ArgDef::int("limit", "最大件数")
                    .default(json!(500))
                    .range(1.0, 100_000.0),
            ],
            |p, _, _, a| {
                let only: Option<ClipId> = a.opt_id("clip")?;
                let limit = a.opt_i64("limit").unwrap_or(500) as usize;
                let tm = p.time_map();
                let (mut rows, mut total) = (Vec::new(), 0usize);
                for t in p.tracks.values() {
                    for c in &t.clips {
                        if only.is_some_and(|o| o != c.id) {
                            continue;
                        }
                        for n in c.notes() {
                            total += 1;
                            if rows.len() < limit {
                                rows.push(json!({
                                    "id": n.id, "clip": c.id, "at": tm.format(c.start + n.start),
                                    "start": n.start, "length": n.length, "pitch": n.pitch,
                                    "velocity": n.velocity, "muted": n.muted, "ai": n.ai,
                                }));
                            }
                        }
                    }
                }
                Ok(Output::with(
                    format!("ノート {total} 個（{} 個を表示）", rows.len()),
                    Value::Array(rows),
                ))
            },
        ),
        read(
            "ops.list",
            "使用できるコマンドを一覧表示します（--grep で名前と説明を絞り込み）",
            vec![ArgDef::text("grep", "この語句を含むものだけ")],
            |_, _, reg, a| {
                let rows: Vec<Value> = reg
                    .find(a.opt_str("grep"))
                    .into_iter()
                    .map(|d| {
                        json!({
                            "name": d.name,
                            "summary": d.summary,
                            "args": d.args.iter().map(|x| x.name).collect::<Vec<_>>(),
                            "history": d.history,
                            "risk": d.risk,
                            "screen_only": d.screen_only,
                        })
                    })
                    .collect();
                Ok(Output::with(
                    format!("コマンド {} 件", rows.len()),
                    Value::Array(rows),
                ))
            },
        ),
        read(
            "ops.schema",
            "コマンドの引数の JSON Schema を表示します（MCP の tools/list と同じ形式）",
            vec![ArgDef::text("name", "コマンド名（省略時はすべて）")],
            |_, _, reg, a| schema(reg, a),
        ),
    ]
}

fn schema(reg: &Registry, a: &Args) -> CmdResult<Output> {
    match a.opt_str("name") {
        Some(n) => {
            let d = reg
                .get(n)
                .ok_or_else(|| CmdError::not_found(format!("コマンド {n} が見つかりません")))?;
            Ok(Output::with(format!("{n} の Schema"), d.json_schema()))
        }
        None => {
            let all = reg.tool_schemas();
            Ok(Output::with(
                format!("Schema {} 件", all.len()),
                Value::Array(all),
            ))
        }
    }
}

fn track_list(p: &Project, _: &mut SessionState, _: &Registry, _: &Args) -> CmdResult<Output> {
    fn row(p: &Project, node: Node, depth: usize, parent: Option<String>) -> Value {
        match node {
            Node::Track(id) => {
                let t = &p.tracks[&id];
                json!({
                    "id": id, "kind": t.kind, "name": t.name, "depth": depth, "parent": parent,
                    "mute": t.mute, "solo": t.solo, "armed": t.armed,
                    "volume_db": t.volume_db, "pan": t.pan,
                    "output": p.effective_output(id).map(|o| o.to_string()),
                    "inserts": t.inserts.len(), "sends": t.sends.len(), "clips": t.clips.len(),
                })
            }
            Node::Folder(id) => {
                let f = &p.folders[&id];
                json!({"id": id, "kind": "folder", "name": f.name, "depth": depth, "parent": parent, "bus": f.bus})
            }
        }
    }
    fn walk(
        p: &Project,
        kids: &[Node],
        depth: usize,
        parent: Option<String>,
        out: &mut Vec<Value>,
    ) {
        for n in kids {
            out.push(row(p, *n, depth, parent.clone()));
            if let Node::Folder(f) = n {
                walk(
                    p,
                    &p.folders[f].children,
                    depth + 1,
                    Some(f.to_string()),
                    out,
                );
            }
        }
    }
    let mut rows = Vec::new();
    walk(p, &p.root, 0, None, &mut rows);
    rows.push(row(p, Node::Track(TrackId::MASTER), 0, None));
    let n = rows.len();
    Ok(Output::with(format!("{n} 行"), Value::Array(rows)))
}

// ---- selection and view (never in the history) -----------------------------------------------

fn selection_and_view() -> Vec<CommandDef> {
    let ids = || vec![ArgDef::ids(IdKind::Any, "選択する対象").req()];
    vec![
        read(
            "select.set",
            "選択を指定したものだけにします",
            ids(),
            |p, s, _, a| {
                let new = pick(p, a)?;
                s.selection = new;
                Ok(Output::msg(format!(
                    "{} 件を選択しました",
                    s.selection.len()
                )))
            },
        ),
        read(
            "select.add",
            "選択に追加します",
            ids(),
            |p, s, _, a| {
                for id in pick(p, a)? {
                    if !s.selection.contains(&id) {
                        s.selection.push(id);
                    }
                }
                Ok(Output::msg(format!("選択は {} 件", s.selection.len())))
            },
        ),
        read(
            "select.remove",
            "選択から除外します",
            ids(),
            |p, s, _, a| {
                let gone = pick(p, a)?;
                s.selection.retain(|i| !gone.contains(i));
                Ok(Output::msg(format!("選択は {} 件", s.selection.len())))
            },
        ),
        read(
            "select.clear",
            "選択を解除します",
            vec![],
            |_, s, _, _| {
                s.selection.clear();
                Ok(Output::msg("選択を解除しました"))
            },
        ),
        read(
            "select.get",
            "現在の選択を表示します",
            vec![],
            |_, s, _, _| {
                let ids: Vec<String> = s.selection.iter().map(ToString::to_string).collect();
                Ok(Output::with(format!("{} 件", ids.len()), json!(ids)))
            },
        ),
        read(
            "view.zoom",
            "表示のズーム倍率を設定します（横・縦）",
            vec![
                ArgDef::float("h", "横の倍率").range(0.01, 1000.0),
                ArgDef::float("v", "縦の倍率").range(0.01, 1000.0),
            ],
            |_, s, _, a| {
                if let Some(h) = a.opt_f64("h") {
                    s.view.zoom_h = h;
                }
                if let Some(v) = a.opt_f64("v") {
                    s.view.zoom_v = v;
                }
                Ok(Output::msg(format!(
                    "ズーム 横 {} / 縦 {}",
                    s.view.zoom_h, s.view.zoom_v
                )))
            },
        ),
        read(
            "view.scroll",
            "表示の左端の位置を設定します",
            vec![ArgDef::pos("at", "位置").alias(&["to"]).req()],
            |p, s, _, a| {
                let t = tick(p, a, "at")?.max(0);
                s.view.scroll_tick = t;
                Ok(Output::msg(format!(
                    "{} に移動しました",
                    p.time_map().format(t)
                )))
            },
        ),
        read(
            "view.grid",
            "編集グリッドを設定します（1/16・1/8t・1/4d など）",
            vec![
                ArgDef::grid("grid", "グリッド").req(),
                ArgDef::choice("editor", &["arrange", "roll"], "エディター"),
            ],
            |p, s, _, a| {
                let text = match a.0.get("grid") {
                    Some(Value::String(t)) => t.clone(),
                    Some(other) => other.to_string(),
                    None => return Err(CmdError::invalid("--grid が必要です")),
                };
                crate::time::parse_grid(&text, p.ppq)?;
                if a.opt_str("editor") == Some("roll") {
                    s.view.roll.grid = text.clone();
                } else {
                    s.view.grid = text.clone();
                }
                Ok(Output::msg(format!("グリッドを {text} に設定しました")))
            },
        ),
        read(
            "view.link",
            "アレンジの Link を切り替えます",
            on_off().to_vec(),
            |_, s, _, a| {
                s.view.link = toggled(a, s.view.link)?;
                Ok(Output::msg(format!("Link を{}", onoff_word(s.view.link))))
            },
        ),
        read(
            "view.snap",
            "グリッドへのスナップを切り替えます（--on / --off。省略時は現在の状態を反転）",
            [
                on_off().to_vec(),
                vec![ArgDef::choice("editor", &["arrange", "roll"], "エディター")],
            ]
            .concat(),
            |_, s, _, a| {
                let on = if a.opt_str("editor") == Some("roll") {
                    s.view.roll.snap = toggled(a, s.view.roll.snap)?;
                    s.view.roll.snap
                } else {
                    s.view.snap = toggled(a, s.view.snap)?;
                    s.view.snap
                };
                Ok(Output::msg(format!("スナップを{}", onoff_word(on))))
            },
        ),
    ]
}

fn pick(p: &Project, a: &Args) -> CmdResult<Vec<AnyId>> {
    let ids = a.ids::<AnyId>("ids")?;
    for id in &ids {
        if !exists(p, id) {
            return Err(CmdError::not_found(format!("{id} が見つかりません")));
        }
    }
    Ok(ids)
}

// ---- commands the session runs itself ----------------------------------------------------------------

fn session_commands() -> Vec<CommandDef> {
    let path = |help: &'static str| vec![ArgDef::text("path", help).req()];
    vec![
        session(
            "history.list",
            "操作履歴を表示します（ユーザーと AI で共通）",
            vec![ArgDef::int("limit", "新しいものからの件数").range(1.0, 100_000.0)],
            Risk::Read,
            false,
        ),
        session(
            "history.undo",
            "操作を取り消します",
            vec![
                ArgDef::int("count", "取り消す操作の数")
                    .default(json!(1))
                    .range(1.0, 100_000.0),
            ],
            Risk::Write,
            false,
        ),
        session(
            "history.redo",
            "取り消した操作をやり直します",
            vec![
                ArgDef::int("count", "やり直す操作の数")
                    .default(json!(1))
                    .range(1.0, 100_000.0),
            ],
            Risk::Write,
            false,
        ),
        session(
            "history.jump",
            "履歴の任意の手順に移動します",
            vec![ArgDef::int("step", "手順の番号（history.list の step）").req()],
            Risk::Write,
            false,
        ),
        session(
            "project.save",
            "曲を保存します",
            vec![],
            Risk::Guarded,
            false,
        ),
        session(
            "project.save-as",
            "名前を付けて保存します（履歴とバージョンもコピーされます）",
            path("保存先のフォルダー"),
            Risk::Guarded,
            false,
        ),
        session(
            "project.cleanup",
            "どのクリップにも履歴にもバージョンにも使われていない録音のファイルを、ごみ箱に移動します（dry_run で一覧だけを返します）",
            vec![],
            Risk::Destructive,
            false,
        ),
        session(
            "project.open",
            "別の曲を開きます。DAWproject（.dawproject）は新しい曲として開き、読み込めなかったものを一覧にして返します",
            vec![
                ArgDef::text("path", "曲のフォルダー、または .dawproject のファイル").req(),
                ArgDef::text(
                    "media",
                    ".dawproject の中の音のファイルを取り出すフォルダー（省略時は一時フォルダー。保存すると曲のフォルダーにコピーします）",
                ),
            ],
            Risk::HumanOnly,
            false,
        ),
        session(
            "version.save",
            "名前を付けたバージョンを保存します",
            vec![ArgDef::text("name", "バージョン名").req()],
            Risk::Guarded,
            false,
        ),
        session(
            "version.list",
            "バージョンを一覧表示します（名前付きと自動保存）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "version.restore",
            "曲をバージョンの状態に戻します（これも 1 つの操作として履歴に残り、取り消せます）",
            vec![ArgDef::text("id", "バージョンの ID（v1 など）").req()],
            Risk::Write,
            true,
        ),
        session(
            "version.open",
            "バージョンを別の曲として開きます（現在の曲はそのまま）",
            vec![
                ArgDef::text("id", "バージョンの ID（v1 など）").req(),
                ArgDef::text("path", "新しい曲のフォルダー").req(),
            ],
            Risk::Guarded,
            false,
        ),
        session(
            "ai.permission.get",
            "AI の権限を表示します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "ai.permission.set",
            "AI の権限を設定します（off・read・edit・save）。AI からの変更はユーザーの承認を待ちます",
            vec![ArgDef::choice("level", &["off", "read", "edit", "save"], "権限").req()],
            Risk::Guarded,
            false,
        ),
        session(
            "ai.request.begin",
            "AI の依頼を開始します。終了までの操作を履歴の 1 つの操作にまとめます（draft では作業コピーで進め、ユーザーが取り込むまで曲は変わりません）",
            vec![
                ArgDef::text("title", "依頼のタイトル（履歴に表示）").req(),
                ArgDef::flag("draft", "作業コピーで実行する"),
            ],
            Risk::Write,
            false,
        ),
        session(
            "ai.request.end",
            "AI の依頼を終了します（draft の作業コピーはユーザーの確認待ちになります）",
            vec![],
            Risk::Write,
            false,
        ),
        session(
            "ai.request.status",
            "進行中の依頼と、作業コピーを取り込んだ場合の変更内容を表示します",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "ai.request.accept",
            "AI の作業コピーを曲に取り込みます（1 つの操作になり、取り消せます）",
            vec![ArgDef::choice(
                "on_conflict",
                &["reject", "replace"],
                "作業コピーの作成後に曲が変更されていた場合の扱い（reject で中止、replace で置き換え）",
            )],
            Risk::HumanOnly,
            true,
        ),
        session(
            "ai.request.revert",
            "AI の依頼を破棄します（作業コピーを破棄するか、直接の編集を依頼の開始時点まで戻します）",
            vec![],
            Risk::HumanOnly,
            false,
        ),
        session(
            "ai.batch",
            "複数のコマンドを 1 つの操作としてまとめて実行します（途中で失敗した場合はすべて取り消します）",
            vec![
                ArgDef::objects("commands", "[{command, args}] の配列").req(),
                ArgDef::text("title", "履歴に表示するタイトル"),
            ],
            Risk::Write,
            true,
        ),
        session(
            "ai.activity",
            "AI が編集中のものと、AI が編集してユーザーが未確認のものを表示します（UI のハイライトと枠の表示に使用）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "ai.pending",
            "ユーザーの承認待ちの確認を一覧表示します（削除・上書き保存）",
            vec![],
            Risk::Read,
            false,
        ),
        session(
            "ai.decide",
            "AI の操作の確認に回答します（トークンは 1 回限り・有効期限 120 秒。回答が無ければ実行しません）",
            vec![
                ArgDef::text("token", "確認トークン").req(),
                ArgDef::choice("answer", &["yes", "no"], "実行を許可するか").req(),
            ],
            Risk::HumanOnly,
            true,
        ),
    ]
}

#[allow(dead_code)]
fn _kinds(_: TrackKind) {}
