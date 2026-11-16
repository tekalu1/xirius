//! A Drum Kit pad's song-level output (PLG-069).

use std::sync::Arc;

use serde_json::json;
use xirius_params::{PAD_NAMES, pad_of};

use super::*;
use crate::command::EditCtx;
use crate::id::{IdKind, PluginId, TrackId};
use crate::model::{Track, TrackKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "plugin.pad_output.set",
        "ドラムキットのパッドの出力先を Main、バス、または新しいバスに設定します。",
        vec![
            ArgDef::id("id", IdKind::Plugin, "ドラムキット").req(),
            ArgDef::objects("pads", "出力先を変えるパッド [{pad}]").req(),
            ArgDef::text("to", "Main、バスの ID、または new").req(),
            ArgDef::text(
                "active",
                "新しいバスの名前に使うパッド。省略時は最後のパッド",
            ),
        ],
        set,
    )]
}

fn set(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: PluginId = a.id("id")?;
    let owner = p
        .tracks
        .iter()
        .find(|(_, t)| t.inserts.iter().any(|x| x.id == id))
        .map(|(tid, _)| *tid)
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))?;
    let x = p
        .track(owner)?
        .inserts
        .iter()
        .find(|x| x.id == id)
        .expect("found");
    if x.builtin().is_none_or(|d| d.uid != "drum-kit") {
        return Err(CmdError::invalid(
            "パッドの出力先はドラムキットにだけ使えます",
        ));
    }
    let list =
        a.0.get("pads")
            .and_then(Value::as_array)
            .ok_or_else(|| CmdError::invalid("--pads に [{pad}] を指定してください"))?;
    let pads: Vec<usize> = list
        .iter()
        .map(|o| {
            let name = o
                .get("pad")
                .and_then(Value::as_str)
                .ok_or_else(|| CmdError::invalid("--pads に [{pad}] を指定してください"))?;
            pad_of(name).ok_or_else(|| {
                CmdError::invalid(format!("パッドは A1〜H16 で指定してください（`{name}`）"))
            })
        })
        .collect::<CmdResult<_>>()?;
    if pads.is_empty() {
        return Err(CmdError::invalid(
            "--pads に 1 つ以上のパッドを指定してください",
        ));
    }
    let active = match a.opt_str("active") {
        Some(name) => pad_of(name).ok_or_else(|| {
            CmdError::invalid(format!("パッドは A1〜H16 で指定してください（`{name}`）"))
        })?,
        None => *pads.last().expect("nonempty"),
    };
    if !pads.contains(&active) {
        return Err(CmdError::invalid(
            "--active は --pads の中から指定してください",
        ));
    }
    let to = a.str("to")?;
    let bus = if to.eq_ignore_ascii_case("main") {
        None
    } else if to.eq_ignore_ascii_case("new") {
        let x = p
            .track(owner)?
            .inserts
            .iter()
            .find(|x| x.id == id)
            .expect("found");
        let def = x.builtin().expect("drum kit");
        let name = x
            .samples
            .iter()
            .enumerate()
            .find(|(i, _)| def.file_pad(&x.params, *i) == active)
            .map(|(_, file)| super::samples::layer_of(file).0)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| PAD_NAMES[active].to_string());
        let (parent, at) = p
            .find_node(Node::Track(owner))
            .ok_or_else(|| CmdError::not_found(format!("トラック {owner} が見つかりません")))?;
        let mut after = at + 1;
        while let Some(Node::Track(tid)) = p.children(parent).get(after) {
            if p.track(*tid)?.kind != TrackKind::Bus {
                break;
            }
            after += 1;
        }
        let tid = TrackId(p.alloc());
        let mut track = Track::new(tid, TrackKind::Bus, name);
        track.color = Some(p.next_track_color());
        p.tracks.insert(tid, Arc::new(track));
        p.children_mut(parent).insert(after, Node::Track(tid));
        Some(tid)
    } else {
        let tid: TrackId = to.parse().map_err(CmdError::invalid)?;
        p.bus(tid)?;
        Some(tid)
    };
    let x = p
        .track_mut(owner)?
        .inserts
        .iter_mut()
        .find(|x| x.id == id)
        .expect("found");
    for pad in &pads {
        if let Some(tid) = bus {
            x.pad_outputs.insert(*pad, tid);
        } else {
            x.pad_outputs.remove(pad);
        }
    }
    p.check_routing()?;
    Ok(Output::with(
        format!(
            "{} パッドの出力先を {} に設定しました",
            pads.len(),
            bus.map_or_else(
                || "Main".to_string(),
                |b| p
                    .track(b)
                    .map_or_else(|_| b.to_string(), |t| t.name.clone())
            )
        ),
        json!({"plugin": id.to_string(), "pads": pads.iter().map(|pad| PAD_NAMES[*pad]).collect::<Vec<_>>(), "output": bus.map_or_else(|| "main".to_string(), |b| b.to_string())}),
    ))
}
