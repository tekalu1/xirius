//! Sidechain keys a plug-in takes from other channels (MIX-047). The
//! sources live on the receiving plug-in, so `plugin.sidechain.*` names the
//! plug-in and `--from` the channel the key comes from; `send.add --to
//! <plugin>` is the same add seen from the sending side.
use super::*;
use crate::auto::AutoTarget;
use crate::id::{IdKind, PluginId, SendId, TrackId};
use crate::model::{KEY_MAX_DB, KEY_MIN_DB, KeyMode, KeySource, Sidechain};

pub(super) fn defs() -> Vec<CommandDef> {
    let plugin = || ArgDef::id("id", IdKind::Plugin, "キーを受けるプラグイン").req();
    let from = || ArgDef::id("from", IdKind::Track, "送り元のチャンネル").req();
    vec![
        edit(
            "plugin.sidechain.add",
            "プラグインのサイドチェーンに送り元を追加します（既定はポストフェーダーのセンド。--output は送り元の出力そのもので、量は掛かりません）",
            vec![
                plugin(),
                from(),
                ArgDef::flag("pre", "プリフェーダーで送る"),
                ArgDef::flag("output", "送り元の出力そのものを送る"),
                ArgDef::float("level", "送る量（dB、-60〜6。--output では使えません）")
                    .range(KEY_MIN_DB, KEY_MAX_DB),
            ],
            |p, _, a| {
                add_source(
                    p,
                    a.id("id")?,
                    a.id("from")?,
                    a.flag("output"),
                    a.flag("pre"),
                    a.opt_f64("level"),
                )
            },
        ),
        destroy(
            "plugin.sidechain.remove",
            "サイドチェーンの送り元を外します",
            vec![plugin(), from()],
            |p, _, a| remove_source(p, a.id("id")?, a.id("from")?),
        ),
        edit(
            "plugin.sidechain.set",
            "サイドチェーンの送り元の量やプリ／ポストを変更します",
            vec![
                plugin(),
                from(),
                ArgDef::float("level", "送る量（dB）").range(KEY_MIN_DB, KEY_MAX_DB),
                ArgDef::choice(
                    "position",
                    &["pre", "post"],
                    "プリフェーダーかポストフェーダーか",
                ),
            ],
            |p, _, a| {
                set_source(
                    p,
                    a.id("id")?,
                    a.id("from")?,
                    a.opt_f64("level"),
                    a.opt_str("position"),
                )
            },
        ),
        edit(
            "plugin.sidechain.enable",
            "サイドチェーンの入・切（Int / Ext）をします",
            vec![
                plugin(),
                ArgDef::choice("state", &["on", "off"], "入・切").req(),
            ],
            |p, _, a| {
                let id: PluginId = a.id("id")?;
                let on = a.str("state")? == "on";
                let tid = owner(p, id)?;
                let t = p.track_mut(tid)?;
                let x = t.inserts.iter_mut().find(|x| x.id == id).expect("found");
                let Some(sc) = &mut x.sidechain else {
                    return Err(CmdError::not_found(format!(
                        "プラグイン {id} にサイドチェーンがありません"
                    )));
                };
                sc.on = on;
                Ok(Output::msg(format!(
                    "{id} のサイドチェーンを{}",
                    onoff_word(on)
                )))
            },
        ),
        edit(
            "plugin.sidechain.swap",
            "サイドチェーンのフィルターの Low Cut と High Cut を入れ替えます（Off は Off に）",
            vec![plugin()],
            |p, _, a| swap_cuts(p, a.id("id")?),
        ),
        read(
            "plugin.sidechain.list",
            "サイドチェーンの送り元を一覧します",
            vec![plugin()],
            |p, _, _, a| {
                let id: PluginId = a.id("id")?;
                let t = p.track(owner(p, id)?)?;
                let x = t.inserts.iter().find(|x| x.id == id).expect("found");
                let empty = Sidechain {
                    on: false,
                    sources: Vec::new(),
                };
                let sc = x.sidechain.as_ref().unwrap_or(&empty);
                let sources: Vec<Value> = sc.sources.iter().map(|s| source_json(p, s)).collect();
                Ok(Output::with(
                    format!(
                        "{id} のサイドチェーン: {}・送り元 {} 件",
                        if sc.on { "オン" } else { "オフ" },
                        sources.len()
                    ),
                    json!({
                        "id": id.to_string(),
                        "on": sc.on,
                        "sources": sources,
                    }),
                ))
            },
        ),
    ]
}

/// The channel whose inserts hold the plug-in.
fn owner(p: &Project, id: PluginId) -> CmdResult<TrackId> {
    super::track::plugin_owner(p, id)
}

/// Swap (PLG-071): the Low Cut takes the High Cut's frequency and the High Cut the Low
/// Cut's, in one step, as Studio Pro's button does. An Off becomes the other's Off, and a
/// frequency past the other's range lands on its end.
fn swap_cuts(p: &mut Project, id: PluginId) -> CmdResult<Output> {
    let tid = owner(p, id)?;
    let x = p
        .track_mut(tid)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found");
    let cuts = x
        .builtin()
        .and_then(|d| Some((d.param("low_cut")?.1, d.param("high_cut")?.1)));
    let Some((low, high)) = cuts else {
        return Err(CmdError::not_found(format!(
            "{} に Low Cut と High Cut がありません",
            x.name
        )));
    };
    let plain = |s: &xirius_params::ParamSpec| {
        s.to_plain(
            x.params
                .get(s.id)
                .copied()
                .unwrap_or_else(|| s.default_norm()),
        )
    };
    let (lc, hc) = (plain(low), plain(high));
    let (low_lo, low_hi) = low.range();
    let (high_lo, high_hi) = high.range();
    let to_low = if high.is_off(hc) {
        low_lo
    } else {
        hc.clamp(low_lo, low_hi)
    };
    let to_high = if low.is_off(lc) {
        high_hi
    } else {
        lc.clamp(high_lo, high_hi)
    };
    x.params.insert(low.id.to_string(), low.to_norm(to_low));
    x.params.insert(high.id.to_string(), high.to_norm(to_high));
    Ok(Output::msg(format!(
        "{id} の Low Cut を {}、High Cut を {} にしました",
        low.text(to_low),
        high.text(to_high)
    )))
}

fn source_json(p: &Project, s: &KeySource) -> Value {
    let name = p
        .tracks
        .get(&s.track)
        .map(|t| t.name.clone())
        .unwrap_or_default();
    match &s.mode {
        KeyMode::Send {
            pre,
            level_db,
            muted_level,
        } => json!({
            "id": s.id.to_string(),
            "track": s.track.to_string(),
            "name": name,
            "mode": "send",
            "pre": pre,
            "level_db": level_db,
            "muted": muted_level.is_some(),
        }),
        KeyMode::Output => json!({
            "id": s.id.to_string(),
            "track": s.track.to_string(),
            "name": name,
            "mode": "output",
        }),
    }
}

/// `plugin.sidechain.add` and the `send.add --to <plugin>` spelling of it
/// (`from` is `ids` there). A loop the new edge would make is refused with
/// the routing's own error.
pub(super) fn add_source(
    p: &mut Project,
    plugin: PluginId,
    from: TrackId,
    output: bool,
    pre: bool,
    level: Option<f64>,
) -> CmdResult<Output> {
    if output && pre {
        return Err(CmdError::invalid(
            "--output と --pre は同時に指定できません",
        ));
    }
    if output && level.is_some() {
        return Err(CmdError::invalid(
            "--output では --level は指定できません（量は掛かりません）",
        ));
    }
    if let Some(l) = level
        && !(KEY_MIN_DB..=KEY_MAX_DB).contains(&l)
    {
        return Err(CmdError::range(format!(
            "送る量 {l} は {KEY_MIN_DB}〜{KEY_MAX_DB} dB の範囲外です"
        )));
    }
    let owner = owner(p, plugin)?;
    if from.is_master() {
        return Err(CmdError::conflict(
            "マスターはサイドチェーンの送り元にできません",
        ));
    }
    if from == owner {
        return Err(CmdError::conflict(
            "送り元と受ける機器が同じチャンネルです（同じチャンネルの音は Int で使います）",
        ));
    }
    let name = p.track(from)?.name.clone();
    let x = p
        .track(owner)?
        .inserts
        .iter()
        .find(|x| x.id == plugin)
        .expect("found");
    if x.sidechain
        .iter()
        .flat_map(|sc| &sc.sources)
        .any(|s| s.track == from)
    {
        return Err(CmdError::conflict(format!(
            "{name} はすでに {plugin} の送り元です"
        )));
    }
    let sid = SendId(p.alloc());
    let mode = if output {
        KeyMode::Output
    } else {
        KeyMode::Send {
            pre,
            level_db: level.unwrap_or(0.0),
            muted_level: None,
        }
    };
    let t = p.track_mut(owner)?;
    let x = t
        .inserts
        .iter_mut()
        .find(|x| x.id == plugin)
        .expect("found");
    let sc = x.sidechain.get_or_insert(Sidechain {
        on: true,
        sources: Vec::new(),
    });
    sc.sources.push(KeySource {
        id: sid,
        track: from,
        mode,
    });
    // The new edge may not make the routing loop.
    p.check_routing()?;
    Ok(Output::with(
        format!("{id} のサイドチェーンに {name} を追加しました", id = plugin),
        json!({ "id": sid.to_string() }),
    ))
}

fn remove_source(p: &mut Project, plugin: PluginId, from: TrackId) -> CmdResult<Output> {
    let owner = owner(p, plugin)?;
    let name = p.track(from)?.name.clone();
    let t = p.track_mut(owner)?;
    let x = t
        .inserts
        .iter_mut()
        .find(|x| x.id == plugin)
        .expect("found");
    let Some(sc) = &mut x.sidechain else {
        return Err(CmdError::not_found(format!(
            "プラグイン {plugin} にサイドチェーンがありません"
        )));
    };
    let gone: Vec<SendId> = sc
        .sources
        .iter()
        .filter(|s| s.track == from)
        .map(|s| s.id)
        .collect();
    if gone.is_empty() {
        return Err(CmdError::not_found(format!(
            "{name} は {plugin} の送り元ではありません"
        )));
    }
    sc.sources.retain(|s| s.track != from);
    if sc.sources.is_empty() {
        x.sidechain = None;
    }
    // Lanes that wrote the gone send rows are let go with them.
    if let Ok(t) = p.track_mut(from) {
        t.automation
            .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone.contains(id)));
    }
    Ok(Output::msg(format!(
        "{plugin} のサイドチェーンの送り元 {name} を外しました"
    )))
}

fn set_source(
    p: &mut Project,
    plugin: PluginId,
    from: TrackId,
    level: Option<f64>,
    position: Option<&str>,
) -> CmdResult<Output> {
    if level.is_none() && position.is_none() {
        return Err(CmdError::invalid(
            "--level か --position のどちらかが必要です",
        ));
    }
    let owner = owner(p, plugin)?;
    let name = p.track(from)?.name.clone();
    let t = p.track_mut(owner)?;
    let x = t
        .inserts
        .iter_mut()
        .find(|x| x.id == plugin)
        .expect("found");
    let s = x
        .sidechain
        .iter_mut()
        .flat_map(|sc| &mut sc.sources)
        .find(|s| s.track == from)
        .ok_or_else(|| CmdError::not_found(format!("{name} は {plugin} の送り元ではありません")))?;
    let KeyMode::Send {
        pre,
        level_db,
        muted_level,
    } = &mut s.mode
    else {
        return Err(CmdError::conflict(format!(
            "{name} は出力の送り元です（量やプリ／ポストはありません）"
        )));
    };
    if let Some(l) = level {
        if muted_level.is_some() {
            *muted_level = Some(l);
        } else {
            *level_db = l;
        }
    }
    if let Some(pos) = position {
        *pre = pos == "pre";
    }
    Ok(Output::msg(format!(
        "{plugin} の送り元 {name} を変更しました"
    )))
}
