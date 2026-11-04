//! A chain preset (PLG-065) put on a track, the edit behind `preset.load` of an fx-chain,
//! instrument-fx or multi-instrument file: the chain gets the song's ids, goes in at a place
//! of the rack (or takes the instrument's seat with its FX), and its macros land on the
//! track's — in one step of the history.

use std::collections::HashSet;

use serde_json::json;

use super::rack::{chain_mut, height, insert_at, place, plugins_in, rack_of};
use super::*;
use crate::auto::AutoTarget;
use crate::chain_preset::{ChainKind, ChainPreset, free};
use crate::controls::{ControlId, MacroControls};
use crate::id::{AnyId, PluginId, SendId, TrackId};
use crate::model::{Track, TrackKind};
use crate::rack::{MAX_SPLIT_DEPTH, RackNode, Source};
use crate::track_preset::{Adjust, AdjustKind};

/// Puts `preset` on the track `--track`: at `--at` (`track`, `after:<id>`,
/// `branch:<id>[:<index>]`), over what is there with `--replace`. `missing` are the uids this
/// PC does not have: they stay as empty slots and their maps go.
pub(super) fn put(
    p: &mut Project,
    a: &Args,
    preset: ChainPreset,
    mut rows: Vec<Adjust>,
    missing: &[String],
) -> CmdResult<Output> {
    let tid: TrackId = a.id("track")?;
    let kind = p.track(tid)?.kind;
    let name = preset.name.clone();
    let mut chain = preset.chain;
    let gone: HashSet<PluginId> = chain
        .devices
        .iter()
        .filter(|x| missing.contains(&x.uid))
        .map(|x| {
            rows.push(Adjust::new(
                AdjustKind::Rejected,
                x.name.as_str(),
                "この PC に無いため、空のスロットにしました",
            ));
            x.id
        })
        .collect();
    let lost = chain.controls.retarget(|t| match t {
        AutoTarget::Param(id, _) if gone.contains(id) => None,
        _ => Some(t.clone()),
    });
    if lost > 0 {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            "マクロの割り当て",
            format!("{lost} 件は、この PC に無い機器を指すため外しました"),
        ));
    }
    let new_ids = chain.renumber(&mut || p.alloc());
    let mut ids: Vec<PluginId> = new_ids.values().copied().collect();
    ids.sort();
    let at = a.opt_str("at").map(str::trim).unwrap_or("track");
    let replace = a.flag("replace");
    let mut moved = Vec::new();
    // The sidechain send rows of what the load takes off the track, with their lanes.
    let gone_keys: Vec<SendId>;
    match preset.kind {
        ChainKind::Fx => {
            if replace && at.starts_with("after:") {
                return Err(CmdError::invalid(
                    "--replace は --at track か branch:<分岐> と使います",
                ));
            }
            let t = p.track_mut(tid)?;
            let r = rack_of(t);
            // A layer's FX is a chain too: `branch:<layer>` means it.
            let at = match at.strip_prefix("branch:") {
                Some(rest) => {
                    let id = rest.split(':').next().unwrap_or_default();
                    let layer = id
                        .parse::<AnyId>()
                        .ok()
                        .is_some_and(|id| match (&r.source, id) {
                            (Some(Source::Multi { layers, .. }), AnyId::RackNode(n)) => {
                                layers.iter().any(|l| l.id == n)
                            }
                            _ => false,
                        });
                    if layer {
                        format!("layer:{rest}")
                    } else {
                        at.to_string()
                    }
                }
                None => at.to_string(),
            };
            let (c, mut index, depth) = place(r, &at)?;
            let tall = chain
                .rack
                .fx
                .iter()
                .map(|n| match n {
                    RackNode::Split(s) => height(s),
                    RackNode::Plugin(_) => 0,
                })
                .max()
                .unwrap_or(0);
            if depth + tall > MAX_SPLIT_DEPTH {
                return Err(CmdError::conflict(format!(
                    "スプリッターの入れ子は {MAX_SPLIT_DEPTH} 段までです"
                )));
            }
            let mut taken = Vec::new();
            if replace {
                let old = std::mem::take(chain_mut(r, c).expect("place found it"));
                plugins_in(&old, &mut taken);
                index = 0;
            }
            for (k, n) in std::mem::take(&mut chain.rack.fx).into_iter().enumerate() {
                insert_at(r, c, index + k, n);
            }
            let mut controls = std::mem::take(&mut t.controls);
            let laned: Vec<ControlId> = t
                .automation
                .iter()
                .filter_map(|l| match l.target {
                    AutoTarget::Control(c) => Some(c),
                    _ => None,
                })
                .collect();
            merge(
                &mut controls,
                &chain.controls,
                &laned,
                &name,
                &mut rows,
                &mut moved,
            );
            t.controls = controls;
            gone_keys = take_off(t, &taken);
            t.inserts.extend(chain.devices);
        }
        ChainKind::Instrument | ChainKind::Multi => {
            if kind != TrackKind::Instrument {
                return Err(CmdError::conflict(
                    "インストゥルメントはインストゥルメントトラックにのみ読み込めます",
                ));
            }
            if at != "track" {
                return Err(CmdError::invalid(format!(
                    "{} のプリセットは --at track だけで読み込めます",
                    preset.kind.name()
                )));
            }
            let t = p.track_mut(tid)?;
            let taken: Vec<PluginId> = t.inserts.iter().map(|x| x.id).collect();
            gone_keys = take_off(t, &taken);
            t.inserts = chain.devices;
            t.rack = Some(chain.rack);
            let page = t.controls.page.take();
            let xy = t.controls.xy;
            t.controls = MacroControls {
                page,
                xy,
                ..chain.controls
            };
            drop_control_lanes(t, &mut rows);
        }
    }
    // Its devices' values and maps of older keys under the song's, as a song is read and a
    // device preset goes in (the fourth review's low 8).
    p.fix_param_keys();
    settle(p, tid, &gone_keys, &mut rows)?;
    let mut message = format!("プリセット「{name}」を読み込みました");
    for m in &moved {
        message.push('。');
        message.push_str(m);
    }
    Ok(Output::with(
        message,
        json!({
            "preset": name,
            "kind": preset.kind.name(),
            "plugins": ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "moved": moved,
            "adjusted": rows,
        }),
    ))
}

/// After the track `tid`'s devices changed: the lanes of the sidechain send rows `keys` that
/// went, the rack mended, the lanes and macro maps that point at nothing let go.
pub(super) fn settle(
    p: &mut Project,
    tid: TrackId,
    keys: &[SendId],
    rows: &mut Vec<Adjust>,
) -> CmdResult<()> {
    if !keys.is_empty() {
        for other in p.tracks.keys().copied().collect::<Vec<_>>() {
            p.track_mut(other)?
                .automation
                .retain(|l| !matches!(&l.target, AutoTarget::Send(id) if keys.contains(id)));
        }
    }
    rows.extend(p.fix_track_rack(tid));
    let t = p.track_mut(tid)?;
    let stale: Vec<AutoTarget> = t
        .automation
        .iter()
        .filter(|l| matches!(l.target, AutoTarget::Param(..) | AutoTarget::Branch(..)))
        .filter(|l| !l.target.exists_on(t))
        .map(|l| l.target.clone())
        .collect();
    t.automation.retain(|l| !stale.contains(&l.target));
    let pruned = t.prune_controls();
    if pruned > 0 {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            "マクロの割り当て",
            format!("{pruned} 件は、指すプラグインが無くなったため外しました"),
        ));
    }
    Ok(())
}

/// Takes the plug-ins `ids` off the track with their lanes; the send ids of the sidechains
/// they had.
pub(super) fn take_off(t: &mut Track, ids: &[PluginId]) -> Vec<SendId> {
    let keys = t
        .inserts
        .iter()
        .filter(|x| ids.contains(&x.id))
        .filter_map(|x| x.sidechain.as_ref())
        .flat_map(|sc| sc.sources.iter().map(|s| s.id))
        .collect();
    t.inserts.retain(|x| !ids.contains(&x.id));
    if let Some(r) = t.rack.as_mut() {
        for id in ids {
            r.remove_plugin(*id);
        }
    }
    t.automation
        .retain(|l| !matches!(&l.target, AutoTarget::Param(id, _) if ids.contains(id)));
    keys
}

/// Whether `a` and `b` are of one family (knobs, buttons, the pads' axes).
fn same_family(a: ControlId, b: ControlId) -> bool {
    matches!(
        (a, b),
        (ControlId::Knob(_), ControlId::Knob(_))
            | (ControlId::Button(_), ControlId::Button(_))
            | (
                ControlId::X(_) | ControlId::Y(_),
                ControlId::X(_) | ControlId::Y(_)
            )
    )
}

/// An FX chain's macros onto the track's: each to its own number, else the first free one of
/// its family (said in `moved`), else left out (a row). One without a name takes the
/// preset's.
fn merge(
    into: &mut MacroControls,
    from: &MacroControls,
    laned: &[ControlId],
    name: &str,
    rows: &mut Vec<Adjust>,
    moved: &mut Vec<String>,
) {
    for (id, c) in from.iter() {
        if c.maps.is_empty() {
            continue;
        }
        // a control with a lane of its own is not free: the lane would move what comes in
        let open = |into: &MacroControls, x: ControlId| free(into, x) && !laned.contains(&x);
        let to = if open(into, id) {
            Some(id)
        } else {
            ControlId::all().find(|x| same_family(*x, id) && open(into, *x))
        };
        let Some(to) = to else {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                id.default_name(),
                "空いているコントロールが無いため、読み込みませんでした",
            ));
            continue;
        };
        let mut c = c.clone();
        if c.name.is_none() {
            c.name = Some(name.to_string());
        }
        *into.get_mut(to) = c;
        if to != id {
            moved.push(format!(
                "{} を {} に移しました",
                id.default_name(),
                to.default_name()
            ));
        }
    }
}

/// Takes off the lanes of the track's macro controls, as a preset puts its own controls in
/// their place (the old lanes would move the new ones), with a row for each.
pub(super) fn drop_control_lanes(t: &mut Track, rows: &mut Vec<Adjust>) {
    for l in &t.automation {
        if let AutoTarget::Control(c) = l.target {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{} のオートメーション", c.default_name()),
                "マクロのコントロールがプリセットのものに替わったため、外しました",
            ));
        }
    }
    t.automation
        .retain(|l| !matches!(l.target, AutoTarget::Control(_)));
}

/// Whether an effect device preset goes in as a chain of one: it was given a place.
pub(super) fn placed(a: &Args) -> bool {
    a.opt_str("at").is_some_and(|s| !s.trim().is_empty()) || a.flag("replace")
}
