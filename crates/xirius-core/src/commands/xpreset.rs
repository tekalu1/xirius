//! Device presets (PLG-063) on the song's side. `preset.save`, `preset.load`,
//! `preset.list`, `preset.remove`, `preset.rename` and `preset.factory.restore` are
//! answered by the session, which owns the `presets/` folder; the one edit here,
//! `preset.put`, puts the device a preset file describes onto a track — apply it to the
//! device already there, add it as a new insert, or take the instrument's seat — as a
//! single step of the history.

use serde_json::{Value, json};

use super::*;
use crate::auto::AutoTarget;
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId, SendId, TrackId};
use crate::model::{Plugin, PluginKind, TrackKind};
use crate::track_preset::{Adjust, AdjustKind};
use crate::xpreset::{self, DevicePreset, PresetDevice};

pub(super) fn defs() -> Vec<CommandDef> {
    let track = || ArgDef::id("track", IdKind::Track, "トラック").req();
    let preset = || {
        ArgDef::text(
            "preset",
            "プリセットの名前か、.xpreset / plugin.preset のファイル",
        )
        .req()
    };
    let of = || ArgDef::id("of", IdKind::Plugin, "プリセットを読み込むプラグイン");
    let device = || {
        ArgDef::text(
            "device",
            "機器の uid か名前（その機器のプリセットだけに絞ります）",
        )
    };
    vec![
        session(
            "preset.save",
            "機器の今の設定を .xpreset のプリセットに保存します（同じ名前があるときは --overwrite で上書きします）",
            vec![
                track(),
                ArgDef::choice("kind", &["fx", "instrument"], "プリセットの種類").req(),
                ArgDef::id("of", IdKind::Plugin, "プリセットにするプラグイン"),
                ArgDef::text("name", "プリセットの名前").req(),
                ArgDef::flag(
                    "overwrite",
                    "この機器に同じ名前（大文字と小文字の違いは同じ）のプリセットがあれば上書きします",
                ),
            ],
            Risk::Guarded,
            false,
        ),
        session(
            "preset.load",
            "プリセットをトラックの機器に読み込みます（1 回の取り消しで戻せます）",
            vec![track(), preset(), of(), device()],
            Risk::Write,
            false,
        ),
        session(
            "preset.list",
            "機器のプリセットを一覧します（機器ごとに Default が先頭、ほかは名前の順）",
            vec![
                ArgDef::choice("kind", &["fx", "instrument"], "プリセットの種類"),
                device(),
                ArgDef::text("query", "名前で絞る文字列"),
            ],
            Risk::Read,
            false,
        ),
        session(
            "preset.remove",
            "プリセットを削除します（「Default」は消せません）",
            vec![preset(), device()],
            Risk::Guarded,
            false,
        ),
        session(
            "preset.rename",
            "プリセットの名前を変えます",
            vec![preset(), ArgDef::text("name", "新しい名前").req(), device()],
            Risk::Guarded,
            false,
        ),
        session(
            "preset.factory.restore",
            "一覧に無い内蔵のプリセットを足し直します（あるものは上書きしません）",
            vec![device()],
            Risk::Guarded,
            false,
        ),
        edit(
            "preset.put",
            "読み込んだプリセットの内容を、トラックの機器に反映します（preset.load から使います）",
            vec![
                track(),
                ArgDef::text("preset", "プリセットの内容（JSON）").req(),
                ArgDef::text("name", "プリセットの名前（プラグインに記録します）"),
                of(),
                ArgDef::text(
                    "missing",
                    "この PC に無いプラグインの uid（コンマ区切り。空のスロットとして残します）",
                ),
                ArgDef::text(
                    "lost",
                    "見つからないため外したサンプルの名前（JSON の配列。調整の行に出します）",
                ),
            ],
            put,
        ),
    ]
}

fn put(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let v: Value = serde_json::from_str(a.str("preset")?)
        .map_err(|e| CmdError::invalid(format!("プリセットを読み込めません: {e}")))?;
    let (preset, mut rows) = DevicePreset::parse(&v, None)?;
    let name = a
        .opt_str("name")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(&preset.name)
        .to_string();
    let of: Option<PluginId> = a.opt_id("of")?;
    let missing: Vec<String> = a
        .opt_str("missing")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let lost: Vec<String> = match a.opt_str("lost").map(str::trim) {
        Some(t) if !t.is_empty() => serde_json::from_str(t)
            .map_err(|e| CmdError::invalid(format!("lost を読み込めません: {e}")))?,
        _ => Vec::new(),
    };
    for s in &lost {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            s.as_str(),
            "サンプルが見つからないため外しました",
        ));
    }
    let dev = &preset.device;
    if missing.contains(&dev.uid) {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            dev.name.as_str(),
            "この PC に無いため、空のスロットにしました",
        ));
    }
    let (message, plugin) = match of {
        Some(id) => {
            let t = p.track(tid)?;
            let i = t.inserts.iter().position(|x| x.id == id).ok_or_else(|| {
                CmdError::not_found(format!("このトラックにプラグイン {id} がありません"))
            })?;
            let x = &t.inserts[i];
            if x.kind != preset.kind {
                return Err(CmdError::conflict(
                    "プリセットの種類とプラグインの種類が違います",
                ));
            }
            if !xpreset::same_device(&dev.format, &dev.uid, x) {
                return Err(CmdError::conflict("この機器のプリセットではありません"));
            }
            let x = &mut p.track_mut(tid)?.inserts[i];
            apply(x, dev, &name);
            (format!("プリセット「{name}」を読み込みました"), id)
        }
        None => match preset.kind {
            PluginKind::Effect => {
                let pos = p.track(tid)?.inserts.iter().position(|x| {
                    x.kind == PluginKind::Effect && xpreset::same_device(&dev.format, &dev.uid, x)
                });
                match pos {
                    Some(i) => {
                        let id = p.track(tid)?.inserts[i].id;
                        let x = &mut p.track_mut(tid)?.inserts[i];
                        apply(x, dev, &name);
                        (format!("プリセット「{name}」を読み込みました"), id)
                    }
                    None => {
                        let id = insert_fx(p, tid, dev, &name)?;
                        (
                            format!(
                                "「{}」を挿入し、プリセット「{name}」を読み込みました",
                                dev.name
                            ),
                            id,
                        )
                    }
                }
            }
            PluginKind::Instrument => {
                if p.track(tid)?.kind != TrackKind::Instrument {
                    return Err(CmdError::conflict(
                        "インストゥルメントはインストゥルメントトラックにのみ挿入できます",
                    ));
                }
                // The instrument of the preset's device takes it (a multi's layer too, PRJ-026);
                // with none, the first instrument's seat is replaced and the other layers stay.
                let same = p.track(tid)?.inserts.iter().position(|x| {
                    x.kind == PluginKind::Instrument
                        && xpreset::same_device(&dev.format, &dev.uid, x)
                });
                match same {
                    Some(i) => {
                        let x = &mut p.track_mut(tid)?.inserts[i];
                        let id = x.id;
                        apply(x, dev, &name);
                        (format!("プリセット「{name}」を読み込みました"), id)
                    }
                    None => {
                        let id = replace_instrument(p, tid, dev, &name)?;
                        (
                            format!(
                                "インストゥルメントを「{}」に替え、プリセット「{name}」を読み込みました",
                                dev.name
                            ),
                            id,
                        )
                    }
                }
            }
        },
    };
    rows.extend(p.fix_track_rack(tid));
    Ok(Output::with(
        message,
        json!({ "plugin": plugin.to_string(), "preset": name, "adjusted": rows }),
    ))
}

/// The preset's settings on the plug-in: parameters and state are the preset's, but for the
/// knobs no preset holds ([`xpreset::holds`]), which stay as they are; samples only when the
/// file carried them (an old `plugin.preset` file leaves them alone).
fn apply(x: &mut Plugin, dev: &PresetDevice, name: &str) {
    let mut params = dev.params.clone();
    params.retain(|k, _| xpreset::holds(x, k));
    params.extend(
        x.params
            .iter()
            .filter(|(k, _)| !xpreset::holds(x, k))
            .map(|(k, v)| (k.clone(), *v)),
    );
    x.params = params;
    x.state = dev.state.clone();
    if let Some(samples) = &dev.samples {
        x.samples = samples.clone();
    }
    x.preset = Some(name.to_string());
}

/// A new insert of the preset's device at the end of the effects (the master's too, as
/// `plugin.add` allows).
fn insert_fx(p: &mut Project, tid: TrackId, dev: &PresetDevice, name: &str) -> CmdResult<PluginId> {
    let id = PluginId(p.alloc());
    let mut x = plugin_of(dev, name, id);
    x.kind = PluginKind::Effect;
    p.track_mut(tid)?.inserts.push(x);
    Ok(id)
}

/// The preset's device on the first instrument's seat, in the place of what was there (as
/// `plugin.replace` does); the other instruments of a multi stay. The rack's seat passes to
/// it (PRJ-026); the old instrument's automation lanes and the send rows its sidechain made
/// on other strips go away with it.
fn replace_instrument(
    p: &mut Project,
    tid: TrackId,
    dev: &PresetDevice,
    name: &str,
) -> CmdResult<PluginId> {
    let id = PluginId(p.alloc());
    let mut x = plugin_of(dev, name, id);
    x.kind = PluginKind::Instrument;
    let t = p.track_mut(tid)?;
    let Some(i) = t
        .inserts
        .iter()
        .position(|x| x.kind == PluginKind::Instrument)
    else {
        t.inserts.insert(0, x);
        return Ok(id);
    };
    let before = std::mem::replace(&mut t.inserts[i], x);
    let old = before.id;
    let gone_keys: Vec<SendId> = before
        .sidechain
        .iter()
        .flat_map(|sc| sc.sources.iter().map(|s| s.id))
        .collect();
    if let Some(r) = t.rack.as_mut() {
        r.replace_plugin(old, id);
    }
    t.automation
        .retain(|l| !matches!(&l.target, AutoTarget::Param(pid, _) if *pid == old));
    for other in p.tracks.keys().copied().collect::<Vec<_>>() {
        p.track_mut(other)?
            .automation
            .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone_keys.contains(id)));
    }
    Ok(id)
}

/// A new plug-in row carrying the preset's device and settings; the knobs no preset holds
/// start at their defaults.
fn plugin_of(dev: &PresetDevice, name: &str, id: PluginId) -> Plugin {
    let mut x = Plugin {
        samples: dev.samples.clone().unwrap_or_default(),
        id,
        name: dev.name.clone(),
        kind: dev.kind,
        format: dev.format.clone(),
        uid: dev.uid.clone(),
        bypass: false,
        deactivated: false,
        params: dev.params.clone(),
        state: dev.state.clone(),
        isolation: Default::default(),
        sidechain: None,
        preset: Some(name.to_string()),
    };
    x.params = std::mem::take(&mut x.params)
        .into_iter()
        .filter(|(k, _)| xpreset::holds(&x, k))
        .collect();
    x
}
