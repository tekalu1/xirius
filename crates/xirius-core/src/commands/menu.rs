//! Small commands the right-click menus need (CTX): a clip's colour, taking a clip's fades
//! or gain back, and moving one end of the loop. Each is one step of the history, run
//! the same way from the menu, the palette, the CLI and MCP.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;

use super::track;
use super::*;
use crate::command::EditCtx;
use crate::id::{FolderId, IdKind, Node, TrackId};
use crate::model::{Folder, LoopRange, Parent};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "clip.color.set",
            "クリップの色を設定します（#rrggbb。none でトラックの色に戻す）",
            vec![
                ArgDef::ids(IdKind::Clip, "クリップ"),
                ArgDef::text("color", "#rrggbb または none").req(),
            ],
            |p, _, a| {
                let color = color_arg(a)?;
                let ids = a.require_ids::<ClipId>("クリップ")?;
                for id in &ids {
                    p.clip_mut(*id)?.1.color = color.clone();
                }
                Ok(Output::msg(format!(
                    "クリップ {} 件の色を設定しました",
                    ids.len()
                )))
            },
        ),
        edit(
            "clip.fade.clear",
            "オーディオクリップのフェードを外します",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            |p, cx, a| {
                let mut a = a.clone();
                a.0.insert("clear".to_string(), json!(true));
                clipfx::fade_set(p, cx, &a)
            },
        ),
        edit(
            "clip.gain.reset",
            "オーディオクリップのゲインを 0 dB に戻し、ゲインの線の点も消します",
            vec![ArgDef::ids(IdKind::Clip, "クリップ")],
            |p, cx, a| {
                let mut a = a.clone();
                a.0.insert("reset".to_string(), json!(true));
                clipfx::gain_set(p, cx, &a)
            },
        ),
        edit(
            "loop.start.set",
            "ループの始まりを指定位置にします（ループが無いときは 1 小節のループを作ります）",
            vec![ArgDef::pos("at", "位置").req()],
            |p, _, a| set_end(p, a, true),
        ),
        edit(
            "loop.end.set",
            "ループの終わりを指定位置にします（ループが無いときは頭から指定位置までのループを作ります）",
            vec![ArgDef::pos("at", "位置").req()],
            |p, _, a| set_end(p, a, false),
        ),
        edit(
            "folder.pack",
            "選んだトラックとフォルダーを、新しいフォルダーにまとめます（先頭のものがあった場所に作ります）。同じ名前のバスがフォルダーのすぐ下にでき、接続されます（1 回の取り消しで両方とも元に戻ります）",
            vec![
                ArgDef::ids(IdKind::Node, "まとめるトラックやフォルダー"),
                ArgDef::text("name", "フォルダー名（省略時は Folder 1 のような名前）"),
            ],
            pack,
        ),
        edit(
            "folder.duplicate",
            "フォルダーを中のトラックごと複製します（クリップを含む。ID は新しくなります）。フォルダーと一緒に作ったバスも複製し、複製どうしを接続します",
            vec![ArgDef::id("id", IdKind::Folder, "フォルダー").req()],
            duplicate_folder,
        ),
    ]
}

/// Is `node` inside one of the folders in `picked`, at any depth?
fn inside_picked(p: &Project, node: Node, picked: &[Node]) -> bool {
    let mut at = node;
    while let Some((Parent::Folder(f), _)) = p.find_node(at) {
        if picked.contains(&Node::Folder(f)) {
            return true;
        }
        at = Node::Folder(f);
    }
    false
}

fn pack(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let nodes = track::node_ids(p, a)?;
    // what sits inside a picked folder goes along with it
    let mut picked: Vec<Node> = Vec::new();
    for n in nodes
        .iter()
        .copied()
        .filter(|n| !inside_picked(p, *n, &nodes))
    {
        picked.push(n);
        // a folder's own bus goes in with it, and stays under it
        if let Node::Folder(f) = n
            && let Some(b) = p.attached_bus(f)
            && !nodes.contains(&Node::Track(b))
        {
            picked.push(Node::Track(b));
        }
    }
    // the first one in column order has the lowest index of its parent
    let (parent, at) = p.find_node(picked[0]).expect("in the column");
    let name = match a.opt_str("name") {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => track::unique_name(p, "Folder"),
    };
    // where the tracks were, so that unpacking puts them back (also across a save)
    let places: Vec<crate::packing::Place> = picked
        .iter()
        .filter_map(|n| match (n, p.find_node(*n)) {
            (Node::Track(t), Some((parent, index))) => Some(crate::packing::Place {
                track: *t,
                parent: match parent {
                    Parent::Root => None,
                    Parent::Folder(f) => Some(f),
                },
                index,
            }),
            _ => None,
        })
        .collect();
    let id = FolderId(p.alloc());
    p.folders.insert(
        id,
        Folder {
            id,
            name: name.clone(),
            color: None,
            bus: None,
            made_bus: None,
            children: Vec::new(),
        },
    );
    for n in &picked {
        p.detach(*n);
        p.children_mut(Parent::Folder(id)).push(*n);
    }
    p.children_mut(parent).insert(at, Node::Folder(id));
    p.packed.insert(id, places);
    // the bus made with the folder; a folder whose bus would close a loop stays without one
    let bus = track::make_folder_bus(p, id, name.clone());
    if p.check_routing().is_err() {
        p.delete_track(bus)?;
        let folder = p.folder_mut(id)?;
        folder.bus = None;
        folder.made_bus = None;
        return Ok(created(&format!("フォルダー「{name}」"), id));
    }
    Ok(Output::with(
        format!("フォルダー「{name}」 {id} とバス {bus} を作成しました"),
        json!({ "id": id.to_string(), "bus": bus.to_string() }),
    ))
}

/// A copy of the folder, its tracks and its folders under new ids. The copies keep their
/// names and their routing: a send or an output still goes where the original goes. A folder
/// whose own bus is among the copied tracks plays to the copy of that bus.
fn copy_folder(p: &mut Project, id: FolderId) -> FolderId {
    let src = p.folders[&id].clone();
    let new = FolderId(p.alloc());
    let mut children = Vec::new();
    let mut made: HashMap<TrackId, TrackId> = HashMap::new();
    for n in &src.children {
        children.push(match n {
            Node::Track(t) => {
                let t = p.tracks[t].clone();
                let mut copy = track::clone_track(p, &t, false);
                copy.name = t.name.clone();
                let tid = copy.id;
                made.insert(t.id, tid);
                p.tracks.insert(tid, Arc::new(copy));
                Node::Track(tid)
            }
            Node::Folder(f) => Node::Folder(copy_folder(p, *f)),
        });
    }
    // the nested folders were copied first: each one's own bus, if it came along, replaces
    // the original as its bus
    for n in &children {
        if let Node::Folder(c) = n {
            let (bus, own) = (p.folders[c].bus, p.folders[c].made_bus);
            let f = p.folders.get_mut(c).expect("copied above");
            match (bus, own) {
                (Some(b), Some(m)) if b == m && made.contains_key(&m) => {
                    f.bus = Some(made[&m]);
                    f.made_bus = Some(made[&m]);
                }
                // a bus the copy shares with the original is not the copy's to remove
                _ => f.made_bus = None,
            }
        }
    }
    p.folders.insert(
        new,
        Folder {
            id: new,
            name: src.name.clone(),
            color: src.color.clone(),
            bus: src.bus,
            made_bus: src.made_bus,
            children,
        },
    );
    new
}

fn duplicate_folder(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: FolderId = a.id("id")?;
    p.folder(id)?;
    let own = p.own_bus(id);
    let attached = p.attached_bus(id);
    let new = copy_folder(p, id);
    let name = format!("{} コピー", p.folders[&id].name);
    p.folder_mut(new)?.name = name.clone();
    let (parent, i) = p.find_node(Node::Folder(id)).expect("in the column");
    // after the original and the bus that goes with it
    let at = i + 1 + usize::from(attached.is_some());
    p.children_mut(parent).insert(at, Node::Folder(new));
    let Some(bus) = own else {
        // not connected to a bus of its own: the copy plays where the original plays
        p.folder_mut(new)?.made_bus = None;
        return Ok(created(&format!("フォルダー「{name}」"), new));
    };
    // the copy of the bus, connected to the copy of the folder and named as it
    let t = p.tracks[&bus].clone();
    let mut copy = track::clone_track(p, &t, false);
    copy.name = if t.name == p.folders[&id].name {
        name.clone()
    } else {
        t.name.clone()
    };
    let copy_id = copy.id;
    p.tracks.insert(copy_id, Arc::new(copy));
    p.children_mut(parent).insert(at + 1, Node::Track(copy_id));
    let folder = p.folder_mut(new)?;
    folder.bus = Some(copy_id);
    folder.made_bus = Some(copy_id);
    Ok(Output::with(
        format!("フォルダー「{name}」 {new} とバス {copy_id} を作成しました"),
        json!({ "id": new.to_string(), "bus": copy_id.to_string() }),
    ))
}

/// Moves the start (or the end) of the loop to `at`.
fn set_end(p: &mut Project, a: &Args, start: bool) -> CmdResult<Output> {
    let at = tick(p, a, "at")?;
    let tm = p.time_map();
    let (n, _, _) = tm.to_bbt(at);
    let bar = tm.bar_start(n + 1) - tm.bar_start(n);
    let (s, e) = match (p.loop_range, start) {
        (Some(l), true) => (at, l.end),
        (Some(l), false) => (l.start, at),
        (None, true) => (at, at + bar),
        (None, false) => (0, at),
    };
    if s < 0 || e <= s {
        return Err(CmdError::range(
            "ループは 0 以上で、終わりを始まりより後にしてください",
        ));
    }
    p.loop_range = Some(LoopRange { start: s, end: e });
    Ok(Output::msg(format!(
        "ループ範囲を {}〜{} に設定しました",
        tm.format(s),
        tm.format(e)
    )))
}
