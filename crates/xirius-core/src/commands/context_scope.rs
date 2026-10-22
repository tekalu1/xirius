//! Folder view and selection choices shared by the menu and command table.
use super::*;
use crate::id::{FolderId, IdKind};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![
        read(
            "view.folders",
            "すべてのフォルダーの開閉を変更します",
            vec![ArgDef::choice("state", &["open", "closed"], "開閉").req()],
            |_, s, _, a| {
                s.view.folders_closed = a.str("state")? == "closed";
                Ok(Output::msg("フォルダーの開閉を変更しました"))
            },
        ),
        read(
            "select.in_folder",
            "フォルダーの中のトラックを選択します",
            vec![ArgDef::ids(IdKind::Folder, "フォルダー")],
            |p, s, _, a| {
                let ids = a.require_ids::<FolderId>("フォルダー")?;
                let mut tracks = Vec::new();
                for id in ids {
                    p.folder(id)?;
                    for t in p.tracks_in(id) {
                        let id = AnyId::Track(t);
                        if !tracks.contains(&id) {
                            tracks.push(id);
                        }
                    }
                }
                s.selection = tracks;
                Ok(Output::msg(format!(
                    "トラック {} 本を選択しました",
                    s.selection.len()
                )))
            },
        ),
    ]
}
