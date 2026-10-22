//! Copying a channel's settings onto other channels (MIX-007): the effects, the sends, the fader
//! and the pan. The channel's own sound (the instrument), its name, colour, clips and routing stay
//! as they are, and one paste is one step of the history however many channels it reaches.

use std::collections::HashSet;

use serde_json::json;

use super::*;
use crate::auto::AutoTarget;
use crate::clipboard::{ChannelCopy, Clipboard};
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId, SendId, TrackId};
use crate::model::{AuxSend, PluginKind, TrackKind};

/// The parts of a channel's settings, in the order the commands name them.
pub const PARTS: [&str; 4] = ["inserts", "sends", "volume", "pan"];

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "channel.settings.copy",
            "チャンネルの設定（エフェクト・センド・ボリューム・パン）をクリップボードにコピーします",
            vec![ArgDef::id("id", IdKind::Track, "コピーするチャンネル").req()],
            |p, s, _, a| {
                let id: TrackId = a.id("id")?;
                let board = Clipboard::Channel(snapshot(p, id)?);
                let out = Output::with(
                    format!("{}の設定をコピーしました", p.track(id)?.name),
                    json!({ "kind": board.kind(), "count": board.len() }),
                );
                s.clipboard = Some(board);
                Ok(out)
            },
        ),
        edit(
            "channel.settings.paste",
            "コピーしたチャンネルの設定を、ほかのチャンネルへ貼り付けます（エフェクトは置き換えます。--from で、クリップボードを通さずに別のチャンネルから写します）",
            vec![
                ArgDef::ids(IdKind::Track, "貼り付け先のチャンネル"),
                ArgDef::id(
                    "from",
                    IdKind::Track,
                    "設定の元のチャンネル（省略時はクリップボード）",
                ),
                ArgDef::text(
                    "only",
                    "貼り付けるもの（inserts, sends, volume, pan をコンマ区切り。省略時はすべて）",
                ),
            ],
            |p, cx, a| {
                let copy = match a.opt_id::<TrackId>("from")? {
                    Some(from) => snapshot(p, from)?,
                    None => match cx.clipboard {
                        Some(Clipboard::Channel(c)) => c.clone(),
                        _ => {
                            return Err(CmdError::new(
                                crate::error::ErrorCode::NothingToDo,
                                "チャンネルの設定がコピーされていません",
                            ));
                        }
                    },
                };
                let ids = a.require_ids::<TrackId>("チャンネル")?;
                let only = parts(a.opt_str("only"))?;
                paste(p, cx, &copy, &ids, &only)
            },
        ),
    ]
}

/// What `channel.settings.copy` keeps of a channel.
pub(super) fn snapshot(p: &Project, id: TrackId) -> CmdResult<ChannelCopy> {
    let t = p.track(id)?;
    let master = t.kind == TrackKind::Master;
    Ok(ChannelCopy {
        source: id,
        master,
        // The instrument is the channel's own sound; it is not a setting to hand on.
        inserts: t
            .inserts
            .iter()
            .filter(|x| x.kind != PluginKind::Instrument)
            .cloned()
            .collect(),
        sends: if master { Vec::new() } else { t.sends.clone() },
        volume_db: t.volume_db,
        pan: t.pan,
    })
}

fn parts(only: Option<&str>) -> CmdResult<HashSet<&'static str>> {
    let Some(only) = only.filter(|s| !s.trim().is_empty()) else {
        return Ok(PARTS.into_iter().collect());
    };
    let mut out = HashSet::new();
    for w in only.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        let Some(part) = PARTS.into_iter().find(|p| *p == w) else {
            return Err(CmdError::invalid(format!(
                "「{w}」は貼り付けられるものではありません（{}）",
                PARTS.join(", ")
            )));
        };
        out.insert(part);
    }
    Ok(out)
}

/// `edit.paste` with a channel's settings on the clipboard goes to `--track`.
pub(super) fn paste_from_board(
    p: &mut Project,
    cx: &EditCtx<'_>,
    copy: &ChannelCopy,
    a: &Args,
) -> CmdResult<Output> {
    let to: TrackId = a.id("track")?;
    paste(p, cx, copy, &[to], &parts(None)?)
}

fn paste(
    p: &mut Project,
    _: &EditCtx<'_>,
    copy: &ChannelCopy,
    ids: &[TrackId],
    only: &HashSet<&'static str>,
) -> CmdResult<Output> {
    let mut done = Vec::new();
    let mut dropped = 0;
    for id in ids {
        // A channel does not take its own settings back.
        if *id == copy.source {
            continue;
        }
        let master = p.track(*id)?.kind == TrackKind::Master;
        if only.contains("volume") {
            p.track_mut(*id)?.volume_db = copy.volume_db;
        }
        // The master has no sends and no pan; a master's copy has nothing of either to give.
        if only.contains("pan") && !master && !copy.master {
            p.track_mut(*id)?.pan = copy.pan;
        }
        if only.contains("inserts") {
            replace_inserts(p, *id, copy)?;
        }
        if only.contains("sends") && !master && !copy.master {
            dropped += replace_sends(p, *id, copy)?;
        }
        done.push(id.to_string());
    }
    if done.is_empty() {
        return Err(CmdError::new(
            crate::error::ErrorCode::NothingToDo,
            "貼り付け先がコピー元だけです",
        ));
    }
    let name = p.track(copy.source).map(|t| t.name.clone()).ok();
    let message = match name {
        Some(n) => format!("{}チャンネルに{n}の設定を貼り付けました", done.len()),
        None => format!("{}チャンネルに設定を貼り付けました", done.len()),
    };
    Ok(Output::with(
        message,
        json!({ "ids": done, "adjusted": dropped }),
    ))
}

/// The channel's effects become new copies of the source's; the instrument stays.
pub(super) fn replace_inserts(p: &mut Project, to: TrackId, copy: &ChannelCopy) -> CmdResult<()> {
    let fresh: Vec<_> = copy
        .inserts
        .iter()
        .map(|x| {
            let mut x = x.clone();
            x.id = PluginId(p.alloc());
            x
        })
        .collect();
    let t = p.track_mut(to)?;
    let gone: HashSet<PluginId> = t
        .inserts
        .iter()
        .filter(|x| x.kind != PluginKind::Instrument)
        .map(|x| x.id)
        .collect();
    t.inserts.retain(|x| x.kind == PluginKind::Instrument);
    t.inserts.extend(fresh);
    t.automation
        .retain(|l| !matches!(&l.target, AutoTarget::Param(id, _) if gone.contains(id)));
    Ok(())
}

/// The channel's sends become new copies of the source's. A send that would loop back into the
/// channel, or whose bus is gone, is left out; the count of those is returned.
pub(super) fn replace_sends(p: &mut Project, to: TrackId, copy: &ChannelCopy) -> CmdResult<usize> {
    let gone: HashSet<SendId> = p.track(to)?.sends.iter().map(|s| s.id).collect();
    {
        let t = p.track_mut(to)?;
        t.sends.clear();
        t.automation
            .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if gone.contains(id)));
    }
    let mut dropped = 0;
    for s in &copy.sends {
        let usable = p
            .tracks
            .get(&s.to)
            .is_some_and(|b| b.kind == TrackKind::Bus)
            && s.to != to;
        if !usable {
            dropped += 1;
            continue;
        }
        let send = AuxSend {
            id: SendId(p.alloc()),
            ..s.clone()
        };
        p.track_mut(to)?.sends.push(send);
        if p.check_routing().is_err() {
            p.track_mut(to)?.sends.pop();
            dropped += 1;
        }
    }
    Ok(dropped)
}
