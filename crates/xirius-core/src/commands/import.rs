//! Importing an audio file (UI-075, UI-017, UI-046, ENG-031). `clip.import` is answered by the
//! session, which reads the file (its length and the tempo it carries); what it leaves for the
//! song comes here as one `clip.import.place`, so one import is one step of the history. The
//! file is referred to where it is (relative to the song folder when it is inside it) and is
//! never stretched: its length in the song is its length in seconds.

use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::command::EditCtx;
use crate::id::{ClipId, IdKind, Node, TempoId, TimeSigId, TrackId};
use crate::model::{AudioProcess, TempoPoint, TimeSigPoint, Track, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        session(
            "clip.import",
            "音声ファイル（WAV・AIFF・FLAC・MP3・Ogg Vorbis）をインポートし、オーディオクリップとして置きます。--track を省くと新しいオーディオトラックを作ります。ファイルの長さは変えません（1 回で取り消せます）",
            vec![
                ArgDef::text(
                    "file",
                    "音声ファイル（絶対パスまたは曲のフォルダーからの相対パス）",
                )
                .req(),
                ArgDef::id(
                    "track",
                    IdKind::Track,
                    "置くオーディオトラック（省略時は新しいトラック）",
                ),
                ArgDef::flag(
                    "new-track",
                    "新しいオーディオトラックに置く（--track を省いたときと同じ）",
                ),
                ArgDef::pos("at", "置く位置（省略時は曲の頭）"),
                ArgDef::text("name", "新しいトラックの名前（省略時はファイル名）"),
                ArgDef::text(
                    "into",
                    "新しいトラックの追加先のフォルダーの ID（root または省略時は最上位）",
                )
                .default(json!("root")),
                ArgDef::int("index", "フォルダー内の位置（0 が先頭。省略時は末尾）"),
                ArgDef::choice(
                    "tempo",
                    &["auto", "file", "keep"],
                    "ファイルのテンポの扱い（auto: 曲にクリップが無いときだけ曲のテンポにする、file: 曲のテンポにする、keep: 変えない）",
                )
                .default(json!("auto")),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "clip.import.batch",
            "インポートした音声ファイルをまとめて配置します",
            vec![ArgDef::objects("files", "配置する音声ファイル").req()],
            place_batch,
        ),
        edit(
            "clip.import.place",
            "インポートした音声ファイルをオーディオクリップとして置きます（clip.import が使います。--track を省くと新しいオーディオトラックを作ります）",
            vec![
                ArgDef::text(
                    "file",
                    "音声ファイル（曲のフォルダーからの相対パスまたは絶対パス）",
                )
                .req(),
                ArgDef::int("length", "クリップの長さ（tick）").req(),
                ArgDef::id(
                    "track",
                    IdKind::Track,
                    "置くオーディオトラック（省略時は新しいトラック）",
                ),
                ArgDef::pos("at", "置く位置（省略時は曲の頭）"),
                ArgDef::text("name", "新しいトラックの名前（省略時はファイル名）"),
                ArgDef::text(
                    "into",
                    "新しいトラックの追加先のフォルダーの ID（root または省略時は最上位）",
                )
                .default(json!("root")),
                ArgDef::int("index", "フォルダー内の位置（0 が先頭。省略時は末尾）"),
                ArgDef::float("bpm", "曲の頭のテンポをこの BPM にする（1〜999）").range(1.0, 999.0),
                ArgDef::float(
                    "tempo",
                    "クリップのテンポ（BPM。ファイルから分かったとき。省略時は bpm）",
                )
                .range(1.0, 999.0),
            ],
            place_import,
        ),
        edit(
            "clip.relink",
            "オーディオクリップの音声ファイルの参照を付け替えます（保存が、曲のフォルダーに集めたファイルへ向けるのに使います）",
            vec![
                ArgDef::objects("files", "付け替える参照 [{from, to}]").req(),
                ArgDef::objects(
                    "disk",
                    "一緒に済ませたディスクの名前の変更 [{from, to}]（file.rename が使います。取り消すと名前を戻し、やり直すとまた変えます）",
                ),
            ],
            relink,
        ),
        session(
            "midi.import",
            "MIDI ファイル（SMF の形式 0・1）をインポートし、トラックごと（形式 0 はチャンネルごと）に新しいインストゥルメントトラックへ置きます（1 回で取り消せます）",
            vec![
                ArgDef::text(
                    "file",
                    "MIDI ファイル（絶対パスまたは曲のフォルダーからの相対パス）",
                )
                .req(),
                ArgDef::pos("at", "置く位置（省略時は曲の頭）"),
                ArgDef::choice(
                    "tempo",
                    &["auto", "file", "keep"],
                    "ファイルのテンポと拍子の扱い（auto: 曲にクリップが無いときだけ曲のテンポと拍子にする、file: 曲のテンポと拍子にする、keep: 変えない）",
                )
                .default(json!("auto")),
                ArgDef::text("instrument", "新しいトラックのインストゥルメント").default(json!("Sampler")),
            ],
            Risk::Write,
            false,
        ),
        edit(
            "midi.import.place",
            "インポートした MIDI ファイルのパートを、新しいインストゥルメントトラックとして置きます（midi.import が使います）",
            vec![
                ArgDef::objects("parts", "パート [{name, clip, notes}]").req(),
                ArgDef::pos("at", "置く位置（省略時は曲の頭）"),
                ArgDef::text("instrument", "新しいトラックのインストゥルメント").req(),
                ArgDef::objects(
                    "tempo",
                    "置く位置からのテンポ [{tick, bpm}]（省略時は曲のテンポのまま）",
                ),
                ArgDef::objects(
                    "meter",
                    "置く位置の小節からの拍子 [{bar, num, den}]（bar は 1 から。省略時は曲の拍子のまま）",
                ),
            ],
            place_midi,
        ),
    ]
}

/// A MIDI file's parts as new instrument tracks, in the file's order: the song takes the file's
/// tempo and time signatures first when they are given, so each clip is whole bars of them.
fn place_midi(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let parts = a.0["parts"]
        .as_array()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| CmdError::invalid("ノートがありません"))?;
    let at = opt_tick(p, a, "at")?.unwrap_or(0).max(0);
    if let Some(meter) = a.0.get("meter").and_then(Value::as_array) {
        let (bar0, beat, tick) = p.time_map().to_bbt(at);
        if (beat, tick) != (1, 0) {
            return Err(CmdError::conflict(
                "拍子は小節の頭に置くときだけ合わせられます",
            ));
        }
        let mut points = Vec::new();
        for m in meter {
            let field = |k: &str| m[k].as_u64().map(|v| v as u32);
            let (Some(bar), Some(num), Some(den)) = (field("bar"), field("num"), field("den"))
            else {
                return Err(CmdError::invalid("拍子の値が不正です"));
            };
            if bar < 1 || !(1..=64).contains(&num) || !matches!(den, 1 | 2 | 4 | 8 | 16 | 32) {
                return Err(CmdError::range(format!("拍子 {num}/{den} は使えません")));
            }
            points.push((bar0 + bar - 1, num, den));
        }
        p.timesig.retain(|t| t.bar < bar0);
        for (bar, num, den) in points {
            let id = TimeSigId(p.alloc());
            p.timesig.push(TimeSigPoint { id, bar, num, den });
        }
        p.timesig.sort_by_key(|t| t.bar);
    }
    if let Some(tempo) = a.0.get("tempo").and_then(Value::as_array) {
        let mut points = Vec::new();
        for t in tempo {
            let (Some(tick), Some(bpm)) = (t["tick"].as_i64(), t["bpm"].as_f64()) else {
                return Err(CmdError::invalid("テンポの値が不正です"));
            };
            if tick < 0 || !(1.0..=999.0).contains(&bpm) {
                return Err(CmdError::range(format!("テンポ {bpm} BPM は使えません")));
            }
            points.push((at + tick, bpm));
        }
        p.tempo.retain(|t| t.tick < at);
        for (tick, bpm) in points {
            let id = TempoId(p.alloc());
            p.tempo.push(TempoPoint {
                id,
                tick,
                bpm,
                ramp: false,
            });
        }
        p.tempo.sort_by_key(|t| t.tick);
    }
    let registry = crate::command::Registry::standard();
    let def = registry
        .get("track.add.with-instrument")
        .expect("registered");
    let instrument = a.str("instrument")?;
    let mut placed = Vec::new();
    for part in parts {
        let clip = part["clip"].as_str().unwrap_or_default();
        let base = part["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(clip);
        let mut raw = serde_json::Map::new();
        raw.insert("instrument".into(), json!(instrument));
        if !base.is_empty() {
            raw.insert("name".into(), json!(free_name(p, base)));
        }
        if !clip.is_empty() {
            raw.insert("clip".into(), json!(clip));
        }
        raw.insert("at".into(), json!(at));
        raw.insert("notes".into(), part["notes"].clone());
        let args = registry.normalize(def, &raw)?;
        placed.push(super::instrument::add_with_instrument(p, cx, &args)?.data);
    }
    Ok(Output::with(
        format!("{} 本のトラックを置きました", placed.len()),
        json!({ "placed": placed }),
    ))
}

/// Every audio clip that names `from` names `to` instead, in one step.
fn relink(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    check_disk(cx, a)?;
    let pairs = a.0["files"]
        .as_array()
        .ok_or_else(|| CmdError::invalid("付け替えるファイルを指定してください"))?
        .iter()
        .map(|f| {
            let get = |k: &str| {
                f.get(k)
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| CmdError::invalid(format!("付け替えの `{k}` がありません")))
            };
            Ok((get("from")?, get("to")?))
        })
        .collect::<CmdResult<Vec<(String, String)>>>()?;
    let ids: Vec<TrackId> = p.tracks.keys().copied().collect();
    let mut changed = 0;
    for id in ids {
        let t = &p.tracks[&id];
        let hits = t.clips.iter().any(|c| {
            matches!(&c.body, ClipBody::Audio { file, .. } if pairs.iter().any(|(f, _)| f == file))
        });
        let frozen = t
            .frozen
            .as_ref()
            .and_then(|z| pairs.iter().find(|(f, _)| *f == z.file))
            .map(|(_, to)| to.clone());
        if !hits && frozen.is_none() {
            continue;
        }
        let t = p.track_mut(id)?;
        if let (Some(z), Some(to)) = (&mut t.frozen, frozen) {
            z.file = to;
            changed += 1;
        }
        for clip in &mut t.clips {
            if let ClipBody::Audio { file, .. } = &mut clip.body
                && let Some((_, to)) = pairs.iter().find(|(f, _)| f == file)
            {
                *file = to.clone();
                changed += 1;
            }
        }
    }
    if changed == 0 {
        return Err(CmdError::new(
            crate::ErrorCode::NothingToDo,
            "付け替えるクリップがありません",
        ));
    }
    Ok(Output::with(
        format!("{changed} 個のクリップのファイルを付け替えました"),
        json!({ "relinked": changed }),
    ))
}

/// The renames `disk` names have been made: each one is in one folder, and the new name is
/// on the disk (an undo renames it back, BRW-013).
fn check_disk(cx: &EditCtx<'_>, a: &Args) -> CmdResult<()> {
    let Some(moves) = a.0.get("disk").and_then(Value::as_array) else {
        return Ok(());
    };
    for m in moves {
        let get = |k: &str| {
            m.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(std::path::Path::new)
                .ok_or_else(|| CmdError::invalid(format!("名前の変更の `{k}` がありません")))
        };
        let (from, to) = (get("from")?, get("to")?);
        if from.parent() != to.parent() || from.file_name().is_none() {
            return Err(CmdError::invalid(
                "名前の変更は同じフォルダーの中のものだけ指定できます",
            ));
        }
        let on_disk = match cx.store {
            Some(s) if to.is_relative() => s.dir.join(to),
            _ => to.to_path_buf(),
        };
        if std::fs::symlink_metadata(&on_disk).is_err() {
            return Err(CmdError::not_found(format!(
                "{} はありません",
                on_disk.display()
            )));
        }
    }
    Ok(())
}

/// The file's name without its folder and extension: the clip's (and a new track's) name.
fn file_stem(file: &str) -> &str {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

fn place_batch(p: &mut Project, cx: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let files = a.0["files"]
        .as_array()
        .ok_or_else(|| CmdError::invalid("音声ファイルを指定してください"))?;
    if files.is_empty() {
        return Err(CmdError::invalid("音声ファイルを指定してください"));
    }
    let registry = crate::command::Registry::standard();
    let def = registry.get("clip.import.place").expect("registered");
    let mut placed = Vec::new();
    for file in files {
        let raw = file
            .as_object()
            .ok_or_else(|| CmdError::invalid("配置する音声ファイルが不正です"))?;
        let args = registry.normalize(def, raw)?;
        placed.push(place_import(p, cx, &args)?.data);
    }
    Ok(Output::with(
        "音声ファイルを配置しました",
        json!({"placed":placed}),
    ))
}

fn place_import(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let file = a.str("file")?.trim().to_string();
    if file.is_empty() {
        return Err(CmdError::invalid("音声ファイルを指定してください"));
    }
    let length = a.i64("length")?;
    if length < 1 {
        return Err(CmdError::range("長さは 1 tick 以上にしてください"));
    }
    let at = opt_tick(p, a, "at")?.unwrap_or(0);
    if at < 0 {
        return Err(CmdError::range("位置は 0 以上で指定してください"));
    }
    let stem = match file_stem(&file) {
        "" => "Audio",
        s => s,
    }
    .to_string();
    let existing: Option<TrackId> = a.opt_id("track")?;
    let (tid, new_name) = match existing {
        Some(tid) => {
            if p.track(tid)?.kind != TrackKind::Audio {
                return Err(CmdError::conflict(format!(
                    "音声ファイルはオーディオトラックにのみ置けます（{tid}）"
                )));
            }
            (tid, None)
        }
        None => {
            let parent = parent_arg(p, a, "into")?;
            let name = match a.opt_str("name").map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => n.to_string(),
                None => free_name(p, &stem),
            };
            let tid = TrackId(p.alloc());
            let mut track = Track::new(tid, TrackKind::Audio, name.clone());
            track.color = Some(p.next_track_color());
            p.tracks.insert(tid, Arc::new(track));
            insert_at(p, parent, a.opt_i64("index"), Node::Track(tid))?;
            (tid, Some(name))
        }
    };
    let id = ClipId(p.alloc());
    p.track_mut(tid)?.clips.push(Clip {
        id,
        name: stem.clone(),
        color: None,
        start: at,
        length,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file,
            offset: 0,
            gain_db: 0.0,
            process: AudioProcess {
                tempo: a.opt_f64("tempo").or(a.opt_f64("bpm")),
                ..Default::default()
            },
        },
    });
    let bpm = a.opt_f64("bpm");
    if let Some(bpm) = bpm {
        match p.tempo.iter_mut().find(|t| t.tick == 0) {
            Some(t) => t.bpm = bpm,
            None => {
                let id = TempoId(p.alloc());
                p.tempo.insert(
                    0,
                    TempoPoint {
                        id,
                        tick: 0,
                        bpm,
                        ramp: false,
                    },
                );
            }
        }
    }
    let track_name = p.track(tid)?.name.clone();
    let mut msg = match new_name {
        Some(n) => format!("トラック「{n}」を作成し、「{stem}」を置きました"),
        None => format!("トラック「{track_name}」に「{stem}」を置きました"),
    };
    if let Some(bpm) = bpm {
        msg.push_str(&format!("。テンポを {} BPM に設定しました", show_bpm(bpm)));
    }
    Ok(Output::with(
        msg,
        json!({ "id": id.to_string(), "clip": id.to_string(), "track": tid.to_string(), "bpm": bpm }),
    ))
}

/// `92` for 92.0, `87.5` for 87.5.
fn show_bpm(bpm: f64) -> String {
    let r = (bpm * 100.0).round() / 100.0;
    if r.fract() == 0.0 {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// `base`, or `base 2`, `base 3`, ... when a track or folder has it.
fn free_name(p: &Project, base: &str) -> String {
    let taken =
        |n: &str| p.tracks.values().any(|t| t.name == n) || p.folders.values().any(|f| f.name == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .expect("endless")
}
