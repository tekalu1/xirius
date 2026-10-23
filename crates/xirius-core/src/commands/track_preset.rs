//! Track presets (MIX-030) on the song's side. The commands that read and write the preset files
//! (`track.preset.save`, `apply`, `add`, `update`, `list`) are answered by the session, which
//! hands the preset it read to the three edits here; one apply, one add and one origin note are
//! each one step of the history. A track keeps its name, input, output and clips; it takes the
//! preset's effects, sends, fader, pan and colour, and the instrument when it is an instrument
//! track.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::*;
use crate::auto::AutoTarget;
use crate::clipboard::ChannelCopy;
use crate::id::{IdKind, PluginId, SendId, TrackId};
use crate::model::{AuxSend, Plugin, PluginKind, Track, TrackKind};
use crate::track_preset::{Adjust, AdjustKind, TrackPreset};

pub(super) fn defs() -> Vec<CommandDef> {
    let preset = || ArgDef::text("preset", "トラックのプリセットの名前").req();
    vec![
        session(
            "track.preset.save",
            "トラックの設定（エフェクト・センド・ボリューム・パン・色・インストゥルメント）をプリセットとして保存します",
            vec![
                ArgDef::id("track", IdKind::Track, "保存するトラック").req(),
                ArgDef::text("name", "プリセットの名前").req(),
            ],
            Risk::Write,
            false,
        ),
        session(
            "track.preset.apply",
            "プリセットをトラックに適用します（名前・入力・出力・クリップは変わりません。1 回の取り消しで戻せます）",
            vec![ArgDef::ids(IdKind::Track, "適用するトラック"), preset()],
            Risk::Write,
            false,
        ),
        session(
            "track.preset.add",
            "プリセットから新しいトラックを作ります（名前はプリセットの名前になります）",
            vec![preset(), into_arg(), index_arg()],
            Risk::Write,
            false,
        ),
        session(
            "track.preset.update",
            "トラックの今の設定で、そのトラックの元のプリセットを上書きします",
            vec![ArgDef::id("track", IdKind::Track, "更新するトラック").req()],
            Risk::Guarded,
            false,
        ),
        session(
            "track.preset.list",
            "保存したトラックのプリセットを一覧します",
            vec![],
            Risk::Read,
            false,
        ),
        edit(
            "track.preset.put",
            "読み込んだトラックのプリセットの内容を、トラックに反映します（track.preset.apply から使います）",
            vec![
                ArgDef::ids(IdKind::Track, "反映するトラック"),
                ArgDef::text("preset", "プリセットの内容（JSON）").req(),
                missing_arg(),
            ],
            |p, _, a| {
                let ids = a.require_ids::<TrackId>("トラック")?;
                for id in &ids {
                    if p.track(*id)?.kind == TrackKind::Master {
                        return Err(CmdError::conflict("マスターには適用できません"));
                    }
                }
                let (preset, mut rows) = parsed(a)?;
                let missing = missing(a);
                for id in &ids {
                    let from = rows.len();
                    apply_to(p, *id, &preset, &missing, &mut rows)?;
                    if ids.len() > 1 {
                        let name = p.track(*id)?.name.clone();
                        for r in &mut rows[from..] {
                            r.what = format!("{name}: {}", r.what);
                        }
                    }
                }
                Ok(Output::with(
                    format!(
                        "{} トラックにプリセット「{}」を適用しました",
                        ids.len(),
                        preset.name
                    ),
                    json!({ "ids": ids.iter().map(ToString::to_string).collect::<Vec<_>>(), "adjusted": rows }),
                ))
            },
        ),
        edit(
            "track.preset.make",
            "読み込んだトラックのプリセットから、新しいトラックを作ります（track.preset.add から使います）",
            vec![
                ArgDef::text("preset", "プリセットの内容（JSON）").req(),
                missing_arg(),
                into_arg(),
                index_arg(),
            ],
            |p, _, a| {
                let (preset, mut rows) = parsed(a)?;
                let parent = parent_arg(p, a, "into")?;
                let name = if p.tracks.values().any(|t| t.name == preset.name)
                    || p.folders.values().any(|f| f.name == preset.name)
                {
                    super::track::unique_name(p, &preset.name)
                } else {
                    preset.name.clone()
                };
                let id = TrackId(p.alloc());
                p.tracks.insert(
                    id,
                    std::sync::Arc::new(Track::new(id, preset.kind, name.clone())),
                );
                insert_at(p, parent, a.opt_i64("index"), Node::Track(id))?;
                apply_to(p, id, &preset, &missing(a), &mut rows)?;
                Ok(Output::with(
                    format!("トラック「{name}」をプリセットから作成しました"),
                    json!({ "id": id.to_string(), "adjusted": rows }),
                ))
            },
        ),
        edit(
            "track.preset.origin",
            "トラックが、どのプリセットから作られたか（保存したか）を記録します",
            vec![
                ArgDef::ids(IdKind::Track, "トラック"),
                ArgDef::text("name", "プリセットの名前").req(),
            ],
            |p, _, a| {
                let ids = a.require_ids::<TrackId>("トラック")?;
                let name = a.str("name")?.trim().to_string();
                for id in &ids {
                    p.track_mut(*id)?.preset = Some(name.clone());
                }
                Ok(Output::msg(format!(
                    "{} トラックのプリセットを「{name}」にしました",
                    ids.len()
                )))
            },
        ),
    ]
}

fn into_arg() -> ArgDef {
    ArgDef::text(
        "into",
        "追加先のフォルダーの ID（root または省略時は最上位）",
    )
    .default(json!("root"))
}

fn index_arg() -> ArgDef {
    ArgDef::int("index", "フォルダー内の位置（0 が先頭。省略時は末尾）")
}

fn missing_arg() -> ArgDef {
    ArgDef::text(
        "missing",
        "この PC に無いプラグインの uid（コンマ区切り。空のスロットとして残します）",
    )
}

fn missing(a: &Args) -> HashSet<String> {
    a.opt_str("missing")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parsed(a: &Args) -> CmdResult<(TrackPreset, Vec<Adjust>)> {
    let v: Value = serde_json::from_str(a.str("preset")?)
        .map_err(|e| CmdError::invalid(format!("プリセットを読み込めません: {e}")))?;
    TrackPreset::parse(&v)
}

/// Puts `preset` on the track `id`.
fn apply_to(
    p: &mut Project,
    id: TrackId,
    preset: &TrackPreset,
    missing: &HashSet<String>,
    rows: &mut Vec<Adjust>,
) -> CmdResult<()> {
    let kind = p.track(id)?.kind;
    let is_instrument = |x: &Plugin| x.kind == PluginKind::Instrument;
    for x in preset.inserts.iter().filter(|x| missing.contains(&x.uid)) {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            x.name.as_str(),
            "この PC に無いため、空のスロットにしました",
        ));
    }
    let effects: Vec<Plugin> = preset
        .inserts
        .iter()
        .filter(|x| !is_instrument(x))
        .cloned()
        .collect();
    let mut sends = Vec::new();
    for (i, s) in preset.sends.iter().enumerate() {
        match bus_for(p, s, id) {
            Some(to) => sends.push(AuxSend {
                id: SendId(0),
                to,
                level_db: s.level_db,
                pre: s.pre,
                muted_level: None,
            }),
            None => rows.push(Adjust::new(
                AdjustKind::Rejected,
                if s.to_name.is_empty() {
                    format!("センド {}", i + 1)
                } else {
                    format!("センド（{}）", s.to_name)
                },
                "送り先のバスが無いため、外しました",
            )),
        }
    }
    let copy = ChannelCopy {
        source: id,
        master: false,
        inserts: effects,
        sends,
        volume_db: preset.volume_db,
        pan: preset.pan,
    };
    let keys = super::channel_settings::replace_inserts(p, id, &copy)?;
    if keys > 0 {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            "サイドチェーンの送り元",
            format!("{keys} 件は、送り元が無いかルーティングがループするため外しました"),
        ));
    }
    let dropped = super::channel_settings::replace_sends(p, id, &copy)?;
    if dropped > 0 {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            "センド",
            format!("{dropped} 件は、ルーティングがループするため外しました"),
        ));
    }
    match (
        kind,
        preset.inserts.iter().find(|x| is_instrument(x)).cloned(),
    ) {
        (TrackKind::Instrument, Some(mut inst)) => {
            inst.id = PluginId(p.alloc());
            let t = p.track_mut(id)?;
            let old: Vec<PluginId> = t
                .inserts
                .iter()
                .filter(|x| is_instrument(x))
                .map(|x| x.id)
                .collect();
            t.inserts.retain(|x| !is_instrument(x));
            let new_inst = inst.id;
            t.inserts.insert(0, inst);
            // The rack's seat passes to the instrument taking its place
            // (PRJ-026); other layers lose theirs and the fix drops them.
            if let Some(r) = t.rack.as_mut()
                && let Some(first) = old.first()
            {
                r.replace_plugin(*first, new_inst);
            }
            t.automation
                .retain(|l| !matches!(&l.target, AutoTarget::Param(pid, _) if old.contains(pid)));
        }
        (_, Some(inst)) if kind != TrackKind::Instrument => rows.push(Adjust::new(
            AdjustKind::Dropped,
            inst.name,
            "インストゥルメントトラックではないため、外しました",
        )),
        _ => {}
    }
    p.fix_track_rack(id);
    let t = p.track_mut(id)?;
    t.volume_db = preset.volume_db;
    t.pan = preset.pan;
    if preset.color.is_some() {
        t.color = preset.color.clone();
    }
    t.preset = Some(preset.name.clone());
    Ok(())
}

/// The bus a send of a preset goes to on this song: the one with its id and name, else the only
/// one with its name.
fn bus_for(p: &Project, s: &crate::track_preset::PresetSend, me: TrackId) -> Option<TrackId> {
    let bus =
        |id: &TrackId| *id != me && p.tracks.get(id).is_some_and(|b| b.kind == TrackKind::Bus);
    if let Some(id) = s.to.filter(bus)
        && (s.to_name.is_empty() || p.tracks[&id].name == s.to_name)
    {
        return Some(id);
    }
    let same: Vec<TrackId> = p
        .tracks
        .values()
        .filter(|b| b.kind == TrackKind::Bus && b.id != me && b.name == s.to_name)
        .map(|b| b.id)
        .collect();
    match same.as_slice() {
        [one] if !s.to_name.is_empty() => Some(*one),
        _ => None,
    }
}
