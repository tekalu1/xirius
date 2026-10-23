//! Swapping one plug-in for another in its place (PLG-019): the instrument
//! of an instrument track, or any insert. The old plug-in's values go with
//! it; the new one starts from its own defaults.
//!
//! Also a preset put in as one step (UI-098), and an instrument track made
//! with its instrument, armed or holding the notes of a file (UI-074, UI-076).

use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::command::EditCtx;
use crate::id::{ClipId, IdKind, Node, NoteId, PluginId, TrackId};
use crate::model::{Clip, ClipBody, Note, Plugin, PluginKind, Track, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "plugin.replace",
            "プラグインを別のものに差し替えます（位置はそのまま）。--slot instrument では、インストゥルメントトラックのインストゥルメントを差し替えます（インストゥルメントが無ければ先頭に入れます）",
            vec![
                ArgDef::id("id", IdKind::Plugin, "差し替えるプラグイン"),
                ArgDef::id(
                    "track",
                    IdKind::Track,
                    "トラック（--slot instrument のとき）",
                ),
                ArgDef::choice(
                    "slot",
                    &["insert", "instrument"],
                    "差し替える場所（insert は --id のプラグイン、instrument はトラックのインストゥルメント）",
                )
                .default(json!("insert")),
                ArgDef::text("name", "新しいプラグインの名前").req(),
                ArgDef::text("format", "vst3・clap・builtin など").default(json!("builtin")),
                ArgDef::text("uid", "形式ごとの固有 ID（省略時は名前）"),
            ],
            replace_plugin,
        ),
        edit(
            "plugin.preset.load",
            "プリセットの値をまとめてプラグインに設定します（1 回の操作として元に戻せます）",
            vec![
                ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
                ArgDef::text("preset", "プリセットの名前").req(),
                ArgDef::objects("values", "値の一覧（{param, value} の配列）").req(),
            ],
            load_preset,
        ),
        edit(
            "track.add.with-instrument",
            "インストゥルメントを入れたインストゥルメントトラックを追加します。--arm で録音準備にし、--notes を渡すとそのノートの MIDI クリップを置きます",
            vec![
                ArgDef::text("name", "トラック名（省略時はインストゥルメントの名前）"),
                ArgDef::text(
                    "into",
                    "追加先のフォルダーの ID（root または省略時は最上位）",
                )
                .default(json!("root")),
                ArgDef::int("index", "フォルダー内の位置（0 が先頭。省略時は末尾）"),
                ArgDef::text("instrument", "インストゥルメントの名前").req(),
                ArgDef::text("format", "vst3・clap・builtin など").default(json!("builtin")),
                ArgDef::text("uid", "形式ごとの固有 ID（省略時は名前）"),
                ArgDef::flag("arm", "録音準備にする"),
                ArgDef::pos("at", "クリップの位置（省略時は曲の頭）"),
                ArgDef::text("clip", "クリップの名前（省略時はトラック名）"),
                ArgDef::objects(
                    "notes",
                    "ノートの一覧（{at, pitch, length, velocity} の配列。at・length は「小節.拍.tick」または tick 数で、at はクリップの頭から）",
                ),
            ],
            add_with_instrument,
        ),
    ]
}

fn replace_plugin(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let name = a.str("name")?.trim().to_string();
    if name.is_empty() {
        return Err(CmdError::invalid("名前を入力してください"));
    }
    let old: Option<PluginId> = a.opt_id("id")?;
    let (tid, kind) = if a.str("slot")? == "instrument" {
        let tid = match (a.opt_id::<TrackId>("track")?, old) {
            (Some(t), _) => t,
            (None, Some(id)) => owner(p, id)?,
            (None, None) => {
                return Err(CmdError::invalid(
                    "--track か --id で、差し替えるトラックを指定してください",
                ));
            }
        };
        if p.track(tid)?.kind != TrackKind::Instrument {
            return Err(CmdError::conflict(
                "インストゥルメントを差し替えられるのはインストゥルメントトラックだけです",
            ));
        }
        (tid, PluginKind::Instrument)
    } else {
        let id = old
            .ok_or_else(|| CmdError::invalid("--id で、差し替えるプラグインを指定してください"))?;
        let tid = owner(p, id)?;
        let kind = p
            .track(tid)?
            .inserts
            .iter()
            .find(|x| x.id == id)
            .expect("found")
            .kind;
        (tid, kind)
    };
    let id = PluginId(p.alloc());
    let x = Plugin {
        samples: Vec::new(),
        id,
        name: name.clone(),
        kind,
        format: a.str("format")?.to_string(),
        uid: a
            .opt_str("uid")
            .map_or_else(|| name.clone(), str::to_string),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        isolation: Default::default(),
        sidechain: None,
        preset: None,
    };
    let t = p.track_mut(tid)?;
    let at = match kind {
        PluginKind::Instrument => t
            .inserts
            .first()
            .filter(|f| f.kind == PluginKind::Instrument)
            .map(|_| 0),
        PluginKind::Effect => t.inserts.iter().position(|x| Some(x.id) == old),
    };
    // The send rows a replaced plug-in's sidechain made go with it.
    let mut gone_keys: Vec<crate::id::SendId> = Vec::new();
    let gone = match at {
        Some(i) => {
            let old_id = t.inserts[i].id;
            let before = std::mem::replace(&mut t.inserts[i], x);
            if let Some(sc) = before.sidechain {
                gone_keys = sc.sources.iter().map(|s| s.id).collect();
            }
            let name = before.name;
            // The rack's seat follows the plug-in put in it (PRJ-026).
            if let Some(r) = t.rack.as_mut() {
                r.replace_plugin(old_id, id);
            }
            Some(name)
        }
        None => {
            t.inserts.insert(0, x);
            None
        }
    };
    let tid = t.id;
    if !gone_keys.is_empty() {
        for t in p.tracks.values_mut() {
            Arc::make_mut(t)
                .automation
                .retain(|l| !matches!(&l.target, crate::auto::AutoTarget::Send(id) if gone_keys.contains(id)));
        }
    }
    p.fix_track_rack(tid);
    let msg = match gone {
        Some(before) => format!("「{before}」を「{name}」に差し替えました"),
        None => format!("インストゥルメントに「{name}」を入れました"),
    };
    Ok(Output::with(msg, json!({ "id": id.to_string() })))
}

fn load_preset(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let preset = a.str("preset")?.trim().to_string();
    let Some(Value::Array(list)) = a.0.get("values") else {
        return Err(CmdError::invalid("values に値の一覧を指定してください"));
    };
    let mut values = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let param = item.get("param").and_then(Value::as_str);
        let value = item.get("value").and_then(Value::as_f64);
        match (param, value) {
            (Some(k), Some(v)) => values.push((k.to_string(), v)),
            _ => {
                return Err(CmdError::invalid(format!(
                    "{} 番目に param と value を指定してください",
                    i + 1
                )));
            }
        }
    }
    let tid = owner(p, id)?;
    let x = p
        .track_mut(tid)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found");
    x.params.extend(values);
    Ok(Output::msg(format!(
        "{} のプリセットを「{preset}」にしました",
        x.name
    )))
}

pub(super) fn add_with_instrument(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let instrument = a.str("instrument")?.trim().to_string();
    if instrument.is_empty() {
        return Err(CmdError::invalid(
            "インストゥルメントの名前を入力してください",
        ));
    }
    let at = opt_tick(p, a, "at")?.unwrap_or(0).max(0);
    let notes = match a.0.get("notes") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(v)) => note_list(p, at, v)?,
        Some(_) => {
            return Err(CmdError::invalid(
                "notes にはノートの配列を指定してください",
            ));
        }
    };
    let parent = parent_arg(p, a, "into")?;
    let name = match a.opt_str("name").map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => free_name(p, &instrument),
    };
    let armed = a.flag("arm");

    let tid = TrackId(p.alloc());
    let mut track = Track::new(tid, TrackKind::Instrument, name.clone());
    track.color = Some(p.next_track_color());
    let plugin = PluginId(p.alloc());
    track.inserts.push(Plugin {
        samples: Vec::new(),
        id: plugin,
        name: instrument.clone(),
        kind: PluginKind::Instrument,
        format: a.str("format")?.to_string(),
        uid: a
            .opt_str("uid")
            .map_or_else(|| instrument.clone(), str::to_string),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        isolation: Default::default(),
        sidechain: None,
        preset: None,
    });
    track.armed = armed;
    let count = notes.len();
    let clip = if notes.is_empty() {
        None
    } else {
        // whole bars, from the bar the clip starts in to the one the last note ends in
        let end = notes.iter().map(|n| n.start + n.length).max().unwrap_or(1);
        let tm = p.time_map();
        let (bar, _, _) = tm.to_bbt(at + end - 1);
        let length = (tm.bar_start(bar + 1) - at).max(1);
        let notes = notes
            .into_iter()
            .map(|n| Note {
                id: NoteId(p.alloc()),
                ..n
            })
            .collect();
        let id = ClipId(p.alloc());
        track.clips.push(Clip {
            id,
            name: a
                .opt_str("clip")
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or(&name)
                .to_string(),
            color: None,
            start: at,
            length,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Midi {
                notes: Arc::new(notes),
                transpose: 0,
            },
        });
        Some(id)
    };
    p.tracks.insert(tid, Arc::new(track));
    insert_at(p, parent, a.opt_i64("index"), Node::Track(tid))?;
    let msg = match (clip, armed) {
        (Some(_), _) => format!("トラック「{name}」にノート {count} 個を置きました"),
        (None, true) => format!("トラック「{name}」を作成し、録音準備にしました"),
        (None, false) => format!("トラック「{name}」を作成しました"),
    };
    Ok(Output::with(
        msg,
        json!({
            "id": tid.to_string(),
            "plugin": plugin.to_string(),
            "clip": clip.map(|c| c.to_string()),
        }),
    ))
}

/// The notes of `track.add.with-instrument`, checked; their ids come later. `clip_start` is
/// where the clip begins in the song, so a note given as `bar.beat.tick` is placed relative
/// to it (the same as a tick count).
fn note_list(p: &Project, clip_start: Tick, list: &[Value]) -> CmdResult<Vec<Note>> {
    let mut out = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let need = |key: &str, lo: i64, hi: i64| -> CmdResult<i64> {
            let bad = || {
                if key == "at" || key == "length" {
                    CmdError::invalid(format!(
                        "{} 番目の {key} は tick の整数か \"小節.拍.tick\" で指定してください",
                        i + 1
                    ))
                } else {
                    CmdError::invalid(format!("{} 番目の {key} は整数で指定してください", i + 1))
                }
            };
            let v = match item.get(key) {
                Some(Value::Number(n)) => n.as_i64().ok_or_else(bad)?,
                Some(Value::String(s)) if key == "at" || key == "length" => {
                    p.time_map().parse(s).map_err(|_| bad())?
                        - if key == "at" { clip_start } else { 0 }
                }
                _ => return Err(bad()),
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
        let velocity = match item.get("velocity") {
            None => 100,
            Some(_) => need("velocity", 1, 127)?,
        };
        out.push(Note {
            id: NoteId(0),
            start: need("at", 0, i64::MAX)?,
            length: need("length", 1, i64::MAX)?,
            pitch: need("pitch", 0, 127)? as u8,
            velocity: velocity as u8,
            muted: false,
            ai: false,
        });
    }
    Ok(out)
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

fn owner(p: &Project, id: PluginId) -> CmdResult<TrackId> {
    p.tracks
        .iter()
        .find(|(_, t)| t.inserts.iter().any(|x| x.id == id))
        .map(|(tid, _)| *tid)
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))
}
