//! Folder packing retains the original positions for unpacking.
use super::track;
use super::*;
use crate::command::EditCtx;
use crate::id::{FolderId, IdKind, TrackId};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "folder.unpack",
        "フォルダーのまとめを解除します（フォルダーと一緒に作ったバスは、ほかで使われていなければ一緒に消えます）",
        vec![ArgDef::id("id", IdKind::Folder, "フォルダー").req()],
        unpack,
    )]
}

fn unpack(p: &mut Project, _: &EditCtx<'_>, a: &Args) -> CmdResult<Output> {
    let id: FolderId = a.id("id")?;
    let children = p.folder(id)?.children.clone();
    let (parent, index) = p
        .find_node(Node::Folder(id))
        .ok_or_else(|| CmdError::not_found("フォルダーが見つかりません"))?;
    let made: Vec<TrackId> = p.own_bus(id).into_iter().collect();
    track::pin_bus_outputs(p, &made);
    p.detach(Node::Folder(id));
    p.folders.remove(&id);
    // before the tracks go back to their places, which count the rows as they were before
    track::remove_unused_buses(p, &made)?;
    let mut places = p.packed.remove(&id).unwrap_or_default();
    places.sort_by_key(|x| (x.parent, x.index));
    let mut remaining: Vec<_> = children
        .into_iter()
        .filter(|n| match n {
            Node::Track(t) => p.tracks.contains_key(t),
            Node::Folder(_) => true,
        })
        .collect();
    for place in places {
        let node = Node::Track(place.track);
        if let Some(i) = remaining.iter().position(|n| *n == node) {
            remaining.remove(i);
            let target = place
                .parent
                .filter(|f| p.folders.contains_key(f))
                .map_or(Parent::Root, Parent::Folder);
            insert_at(p, target, Some(place.index as i64), node)?;
        }
    }
    for (i, node) in remaining.into_iter().enumerate() {
        insert_at(p, parent, Some((index + i) as i64), node)?;
    }
    Ok(Output::msg("フォルダーのまとめを解除しました"))
}
