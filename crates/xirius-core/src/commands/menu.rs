//! Small commands the right-click menus need (CTX): a clip's colour, taking a clip's fades
//! or gain back, and moving one end of the loop. Each is one step of the history, run
//! the same way from the menu, the palette, the CLI and MCP.

use std::sync::Arc;

use serde_json::json;

use super::track;
use super::*;
use crate::command::EditCtx;
use crate::id::{FolderId, IdKind, Node};
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
            "選んだトラックとフォルダーを、新しいフォルダーにまとめます（先頭のものがあった場所に作ります）",
            vec![
                ArgDef::ids(IdKind::Node, "まとめるトラックやフォルダー"),
                ArgDef::text("name", "フォルダー名（省略時は Folder 1 のような名前）"),
            ],
            pack,
        ),
        edit(
            "folder.duplicate",
            "フォルダーを中のトラックごと複製します（クリップを含む。ID は新しくなります）",
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
    let picked: Vec<Node> = nodes
        .iter()
        .copied()
        .filter(|n| !inside_picked(p, *n, &nodes))
        .collect();
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
            children: Vec::new(),
        },
    );
    for n in &picked {
        p.detach(*n);
        p.children_mut(Parent::Folder(id)).push(*n);
    }
    p.children_mut(parent).insert(at, Node::Folder(id));
    p.packed.insert(id, places);
    Ok(created(&format!("フォルダー「{name}」"), id))
}

/// A copy of the folder, its tracks and its folders under new ids. The copies keep their
/// names and their routing: a send or an output still goes where the original goes.
fn copy_folder(p: &mut Project, id: FolderId) -> FolderId {
    let src = p.folders[&id].clone();
    let new = FolderId(p.alloc());
    let mut children = Vec::new();
    for n in &src.children {
        children.push(match n {
            Node::Track(t) => {
                let t = p.tracks[t].clone();
                let mut copy = track::clone_track(p, &t, false);
                copy.name = t.name.clone();
                let tid = copy.id;
                p.tracks.insert(tid, Arc::new(copy));
                Node::Track(tid)
            }
            Node::Folder(f) => Node::Folder(copy_folder(p, *f)),
        });
    }
    p.folders.insert(
        new,
        Folder {
            id: new,
            name: src.name.clone(),
            color: src.color.clone(),
            bus: src.bus,
            children,
        },
    );
    new
}

fn duplicate_folder(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: FolderId = a.id("id")?;
    p.folder(id)?;
    let new = copy_folder(p, id);
    let name = format!("{} コピー", p.folders[&id].name);
    p.folder_mut(new)?.name = name.clone();
    let (parent, i) = p.find_node(Node::Folder(id)).expect("in the column");
    p.children_mut(parent).insert(i + 1, Node::Folder(new));
    Ok(created(&format!("フォルダー「{name}」"), new))
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
