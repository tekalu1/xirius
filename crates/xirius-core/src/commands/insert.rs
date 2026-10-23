//! Insert transfers preserve state and keep an instrument at the head of its track.
use super::*;
use crate::auto::AutoTarget;
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId, TrackId};
use crate::model::{PluginKind, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    let args = || {
        vec![
            ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
            ArgDef::id(
                "track",
                IdKind::Track,
                "移動先のチャンネル（省略時は現在のチャンネル）",
            ),
            ArgDef::int("index", "挿入する位置（省略時は末尾）").range(0.0, i64::MAX as f64),
        ]
    };
    vec![
        edit(
            "plugin.move",
            "インサートを移動します",
            args(),
            |p, c, a| transfer(p, c, a, false, false),
        ),
        edit(
            "plugin.copy_to",
            "インサートを別のチャンネルにコピーします",
            args(),
            |p, c, a| transfer(p, c, a, true, false),
        ),
        edit(
            "plugin.duplicate",
            "インサートを複製します",
            args(),
            |p, c, a| transfer(p, c, a, true, true),
        ),
    ]
}

fn transfer(
    p: &mut Project,
    _: &EditCtx<'_>,
    a: &Args,
    copy: bool,
    adjacent: bool,
) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let (from, pos) = p
        .tracks
        .iter()
        .find_map(|(t, x)| x.inserts.iter().position(|x| x.id == id).map(|i| (*t, i)))
        .ok_or_else(|| CmdError::not_found("プラグインが見つかりません"))?;
    let to = a.opt_id::<TrackId>("track")?.unwrap_or(from);
    let mut plugin = p.track(from)?.inserts[pos].clone();
    let dest = p.track(to)?;
    // A multi-instrument holds one instrument per layer, so the
    // one-instrument rule does not apply to it (PRJ-026).
    let multi_dest = dest
        .rack
        .as_ref()
        .is_some_and(|r| matches!(r.source, Some(crate::rack::Source::Multi { .. })));
    if plugin.kind == PluginKind::Instrument
        && (dest.kind != TrackKind::Instrument
            || (!multi_dest
                && dest
                    .inserts
                    .iter()
                    .any(|x| x.kind == PluginKind::Instrument && (copy || x.id != id))))
    {
        return Err(CmdError::conflict(
            "インストゥルメントは、インストゥルメントトラックの先頭に1つだけ挿入できます",
        ));
    }
    let lanes: Vec<_> = p
        .track(from)?
        .automation
        .iter()
        .filter(|l| matches!(l.target, AutoTarget::Param(x,_) if x==id))
        .cloned()
        .collect();
    if copy {
        plugin.id = PluginId(p.alloc());
        // The copy's send rows need ids of their own, and rows the target
        // cannot take are left out (MIX-047).
        plugin.refresh_key_ids(p);
        p.prune_key_sources(&mut plugin, to);
    } else {
        let src = p.track_mut(from)?;
        src.inserts.remove(pos);
        if from != to {
            src.automation
                .retain(|l| !matches!(l.target, AutoTarget::Param(x,_) if x==id));
            // Fixing before the plug-in lands in `to` would drop its rack
            // seat; the same-track case is fixed after the insert instead.
            p.fix_track_rack(from);
        }
    }
    let new = plugin.id;
    let dest = p.track_mut(to)?;
    let at = if plugin.kind == PluginKind::Instrument {
        0
    } else {
        let min = usize::from(
            dest.inserts
                .first()
                .is_some_and(|x| x.kind == PluginKind::Instrument),
        );
        a.opt_i64("index")
            .map(|x| x as usize)
            .unwrap_or(if adjacent {
                pos + 1
            } else {
                dest.inserts.len()
            })
            .min(dest.inserts.len())
            .max(min)
    };
    let is_instrument = plugin.kind == PluginKind::Instrument;
    dest.inserts.insert(at, plugin);
    // With a rack the chain lives there: a moved or copied effect takes the
    // asked place in the top chain (the end without --index, right after the
    // original for a duplicate), an instrument takes a free source seat.
    if let Some(r) = dest.rack.as_mut()
        && !is_instrument
    {
        r.remove_plugin(new);
        let at = if adjacent {
            r.fx.iter()
                .position(|n| matches!(n, crate::rack::RackNode::Plugin(x) if *x == id))
                .map(|i| i + 1)
                .unwrap_or(r.fx.len())
        } else {
            a.opt_i64("index")
                .map(|i| (i as usize).min(r.fx.len()))
                .unwrap_or(r.fx.len())
        };
        r.fx.insert(at, crate::rack::RackNode::Plugin(new));
    }
    if copy || from != to {
        dest.automation.extend(lanes.into_iter().map(|mut l| {
            if let AutoTarget::Param(x, _) = &mut l.target {
                *x = new;
            }
            l
        }));
    }
    p.fix_track_rack(to);
    Ok(Output::with(
        if adjacent {
            "インサートを複製しました"
        } else if copy {
            "インサートをコピーしました"
        } else {
            "インサートを移動しました"
        },
        json!({"id":new.to_string(),"track":to.to_string(),"index":at}),
    ))
}
