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
    if plugin.kind == PluginKind::Instrument
        && (dest.kind != TrackKind::Instrument
            || dest
                .inserts
                .iter()
                .any(|x| x.kind == PluginKind::Instrument && (copy || x.id != id)))
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
    } else {
        let src = p.track_mut(from)?;
        src.inserts.remove(pos);
        if from != to {
            src.automation
                .retain(|l| !matches!(l.target, AutoTarget::Param(x,_) if x==id));
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
    dest.inserts.insert(at, plugin);
    if copy || from != to {
        dest.automation.extend(lanes.into_iter().map(|mut l| {
            if let AutoTarget::Param(x, _) = &mut l.target {
                *x = new;
            }
            l
        }));
    }
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
