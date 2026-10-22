//! Marker edits are single history steps, including the loop to the next marker.
use super::*;
use crate::id::{IdKind, MarkerId};
use crate::marker::Marker;
use crate::model::LoopRange;

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        edit(
            "marker.add",
            "指定位置にマーカーを追加します",
            vec![
                ArgDef::pos("at", "位置").req(),
                ArgDef::text("name", "名前"),
            ],
            |p, _, a| {
                let at = tick(p, a, "at")?;
                if at < 0 {
                    return Err(CmdError::range("マーカーの位置は 0 以上にしてください"));
                }
                let id = MarkerId(p.alloc());
                if p.markers.iter().any(|m| m.tick == at) {
                    return Err(CmdError::conflict("この位置にはマーカーがあります"));
                }
                let name = a.opt_str("name").unwrap_or("マーカー").trim();
                if name.is_empty() {
                    return Err(CmdError::invalid("マーカー名が必要です"));
                }
                p.markers.push(Marker {
                    id,
                    name: name.to_string(),
                    tick: at,
                    color: None,
                });
                p.markers.sort_by_key(|m| (m.tick, m.id));
                Ok(Output::with(
                    "マーカーを追加しました",
                    json!({"id": id.to_string()}),
                ))
            },
        ),
        edit(
            "marker.rename",
            "マーカー名を変更します",
            vec![
                ArgDef::id("id", IdKind::Marker, "マーカー").req(),
                ArgDef::text("name", "名前").req(),
            ],
            |p, _, a| {
                let id: MarkerId = a.id("id")?;
                let name = a.str("name")?.trim();
                if name.is_empty() {
                    return Err(CmdError::invalid("マーカー名が必要です"));
                }
                marker_mut(p, id)?.name = name.to_string();
                Ok(Output::msg("マーカー名を変更しました"))
            },
        ),
        edit(
            "marker.color.set",
            "マーカーの色を設定します",
            vec![
                ArgDef::ids(IdKind::Marker, "マーカー"),
                ArgDef::text("color", "#rrggbb または none").req(),
            ],
            |p, _, a| {
                let color = color_arg(a)?;
                for id in a.require_ids::<MarkerId>("マーカー")? {
                    marker_mut(p, id)?.color = color.clone();
                }
                Ok(Output::msg("マーカーの色を設定しました"))
            },
        ),
        edit(
            "marker.loop",
            "次のマーカーまでをループ範囲にします",
            vec![ArgDef::id("id", IdKind::Marker, "マーカー").req()],
            |p, _, a| {
                let id: MarkerId = a.id("id")?;
                let start = marker_mut(p, id)?.tick;
                let end = p
                    .markers
                    .iter()
                    .filter(|m| m.tick > start)
                    .map(|m| m.tick)
                    .min()
                    .ok_or_else(|| CmdError::invalid("次のマーカーがありません"))?;
                p.loop_range = Some(LoopRange { start, end });
                Ok(Output::msg("ループ範囲を設定しました"))
            },
        ),
        destroy(
            "marker.delete",
            "マーカーを削除します",
            vec![ArgDef::ids(IdKind::Marker, "マーカー")],
            |p, _, a| {
                let ids = a.require_ids::<MarkerId>("マーカー")?;
                for id in &ids {
                    marker_mut(p, *id)?;
                }
                p.markers.retain(|m| !ids.contains(&m.id));
                Ok(Output::msg("マーカーを削除しました"))
            },
        ),
    ]
}

fn marker_mut(p: &mut Project, id: MarkerId) -> CmdResult<&mut Marker> {
    p.markers
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| CmdError::not_found(format!("マーカー {id} が見つかりません")))
}
