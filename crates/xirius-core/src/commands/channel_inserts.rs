//! Copying a channel's inserts onto other channels (MIX-045): the effects and splitters of the
//! insert area, in their order and with their state (bypass, values, preset name, how the
//! splitters are built). The instrument area (the instrument, the layers and their effects), the
//! sends, the fader and the automation stay as they are, and one paste is one step of the history
//! however many channels it reaches.

use std::collections::{HashMap, HashSet};

use serde_json::json;

use super::*;
use crate::auto::AutoTarget;
use crate::clipboard::{Clipboard, InsertsCopy};
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId, SendId, TrackId};
use crate::rack::{Rack, RackNode};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "channel.inserts.copy",
            "チャンネルのインサート（エフェクトとスプリッター）をクリップボードにコピーします",
            vec![ArgDef::id("id", IdKind::Track, "コピーするチャンネル").req()],
            |p, s, _, a| {
                let id: TrackId = a.id("id")?;
                let copy = snapshot(p, id)?;
                let n = copy.plugins.len();
                let board = Clipboard::Inserts(copy);
                let out = Output::with(
                    format!("{}のインサートをコピーしました", p.track(id)?.name),
                    json!({ "kind": board.kind(), "count": n }),
                );
                s.clipboard = Some(board);
                Ok(out)
            },
        ),
        edit(
            "channel.inserts.paste",
            "コピーしたインサートを、ほかのチャンネルへ貼り付けます（今のインサートと置き換えます。--append で後ろに足し、--from で、クリップボードを通さずに別のチャンネルから写します）",
            vec![
                ArgDef::ids(IdKind::Track, "貼り付け先のチャンネル"),
                ArgDef::id(
                    "from",
                    IdKind::Track,
                    "インサートの元のチャンネル（省略時はクリップボード）",
                ),
                ArgDef::flag("append", "今のインサートの後ろに足す"),
            ],
            |p, cx, a| {
                let copy = match a.opt_id::<TrackId>("from")? {
                    Some(from) => snapshot(p, from)?,
                    None => match cx.clipboard {
                        Some(Clipboard::Inserts(c)) => c.clone(),
                        _ => {
                            return Err(CmdError::new(
                                crate::error::ErrorCode::NothingToDo,
                                "インサートがコピーされていません",
                            ));
                        }
                    },
                };
                let ids = a.require_ids::<TrackId>("チャンネル")?;
                paste(p, &copy, &ids, a.flag("append"))
            },
        ),
    ]
}

/// The insert area of a track as a rack holds it (a plain line of effects when it has none).
fn area(t: &crate::model::Track) -> Vec<RackNode> {
    t.rack.clone().unwrap_or_else(|| Rack::serial(t)).fx
}

/// The plug-ins `nodes` name, in their order.
fn plugins_of(nodes: &[RackNode]) -> Vec<PluginId> {
    Rack {
        source: None,
        fx: nodes.to_vec(),
    }
    .flatten_order()
}

/// What `channel.inserts.copy` keeps of a channel.
pub(super) fn snapshot(p: &Project, id: TrackId) -> CmdResult<InsertsCopy> {
    let t = p.track(id)?;
    let fx = area(t);
    let plugins = plugins_of(&fx)
        .into_iter()
        .filter_map(|pid| t.inserts.iter().find(|x| x.id == pid).cloned())
        .collect();
    Ok(InsertsCopy {
        source: id,
        plugins,
        fx,
    })
}

/// `edit.paste` with inserts on the clipboard goes to `--track`, in place of its own.
pub(super) fn paste_from_board(
    p: &mut Project,
    _: &EditCtx<'_>,
    copy: &InsertsCopy,
    a: &Args,
) -> CmdResult<Output> {
    let to: TrackId = a.id("track")?;
    paste(p, copy, &[to], false)
}

fn paste(p: &mut Project, copy: &InsertsCopy, ids: &[TrackId], append: bool) -> CmdResult<Output> {
    let mut done = Vec::new();
    let mut dropped = 0;
    for id in ids {
        // A channel does not take its own inserts back; a frozen one keeps what it has.
        if *id == copy.source || p.track(*id)?.frozen.is_some() {
            continue;
        }
        dropped += put(p, *id, copy, append)?;
        done.push(id.to_string());
    }
    if done.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "貼り付けられるチャンネルがありません",
        ));
    }
    let message = if append {
        "インサートを後ろに貼り付けました"
    } else {
        "インサートを貼り付けました"
    };
    Ok(Output::with(
        message,
        json!({ "ids": done, "append": append, "adjusted": dropped }),
    ))
}

/// New copies of the copied inserts go into `to`'s insert area: in place of what it has, or
/// after it. A sidechain's send rows get ids of their own, and a source `to` cannot take
/// (itself, a gone channel, a loop) is left out (MIX-047); the count of those is returned.
fn put(p: &mut Project, to: TrackId, copy: &InsertsCopy, append: bool) -> CmdResult<usize> {
    let mut dropped = 0;
    let mut map = HashMap::new();
    let fresh: Vec<_> = copy
        .plugins
        .iter()
        .map(|x| {
            let mut x = x.clone();
            let id = PluginId(p.alloc());
            map.insert(x.id, id);
            x.id = id;
            x.refresh_key_ids(p);
            dropped += p.prune_key_sources(&mut x, to);
            x
        })
        .collect();
    let mut part = Rack {
        source: None,
        fx: copy.fx.clone(),
    };
    part.remap_ids(&map, &mut || p.alloc());
    let t = p.track_mut(to)?;
    let mut rack = t.rack.clone().unwrap_or_else(|| Rack::serial(t));
    let mut gone_keys = HashSet::new();
    if append {
        rack.fx.extend(part.fx);
    } else {
        let gone: HashSet<PluginId> = plugins_of(&rack.fx).into_iter().collect();
        // The send rows the replaced sidechains made on other strips go too.
        gone_keys = t
            .inserts
            .iter()
            .filter(|x| gone.contains(&x.id))
            .filter_map(|x| x.sidechain.as_ref())
            .flat_map(|sc| sc.sources.iter().map(|s| s.id))
            .collect::<HashSet<SendId>>();
        t.inserts.retain(|x| !gone.contains(&x.id));
        t.automation
            .retain(|l| !matches!(&l.target, AutoTarget::Param(id, _) if gone.contains(id)));
        rack.fx = part.fx;
    }
    t.inserts.extend(fresh);
    t.rack = Some(rack);
    if !gone_keys.is_empty() {
        for t in p.tracks.values_mut() {
            Arc::make_mut(t)
                .automation
                .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone_keys.contains(id)));
        }
    }
    p.fix_track_rack(to);
    Ok(dropped)
}
