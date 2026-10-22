use crate::{
    popup::{Action, Cell, Look, PopupKind},
    roll::{Edit, Out, Then, ids_json},
    studio::{Geometry, Rect, Studio},
};
use serde_json::json;
use xirius_design::Layout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Velocity,
    Length,
    Scale,
}

pub fn cells(kind: Kind) -> Vec<Cell> {
    let entries: &[(&str, &str)] = match kind {
        Kind::Velocity => &[
            ("強く", "stronger"),
            ("弱く", "softer"),
            ("揃える", "equal"),
            ("クレッシェンド", "crescendo"),
            ("デクレッシェンド", "decrescendo"),
        ],
        Kind::Length => &[
            ("次のノートまで", "next"),
            ("クオンタイズの長さ", "grid"),
            ("2 倍", "double"),
            ("半分", "half"),
        ],
        Kind::Scale => &[
            ("C メジャー", "major"),
            ("C マイナー", "minor"),
            ("なし", "off"),
        ],
    };
    entries
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            Cell::new(
                Rect::new(
                    Layout::POPUP_PAD,
                    Layout::POPUP_PAD + i as f32 * Layout::MENU_ROW_H,
                    220.0 - Layout::POPUP_PAD * 2.0,
                    Layout::MENU_ROW_H,
                ),
                Look::Row,
            )
            .text(*label)
            .act(Action::EditValue(kind, value))
        })
        .collect()
}

impl Studio {
    pub(crate) fn open_note_edit(&mut self, kind: Kind, anchor: Rect) {
        self.open_popup(PopupKind::NoteEdit(kind), anchor);
    }

    pub(crate) fn edit_note_value(&mut self, g: &Geometry, kind: Kind, value: &str) -> bool {
        self.popup = None;
        if kind == Kind::Scale {
            self.run_command("view.scale", json!({"scale":value,"root":0}));
            return true;
        }
        let Some(ctx) = self.roll_ctx(g) else {
            return false;
        };
        let ids = self.roll.target_ids(&ctx, false);
        if ids.is_empty() {
            return false;
        }
        let (command, args) = match kind {
            Kind::Velocity => (
                "note.velocity.shape",
                json!({"ids":ids_json(&ids),"shape":value}),
            ),
            Kind::Length => (
                "note.resize",
                json!({"ids":ids_json(&ids),"mode":value,"grid":ctx.tools.snap.grid.to_string()}),
            ),
            Kind::Scale => unreachable!(),
        };
        self.apply_roll(g, Out::edit(Edit::new(command, args, Then::Keep)));
        true
    }
}
