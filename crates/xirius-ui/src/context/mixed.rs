//! A right press inside a selection of several kinds (CTX-007): clips with automation points
//! in the arrangement, notes with controller points in the roll.
//!
//! A press on something that is not selected makes it the whole selection first, so the menu
//! never acts on a kind the press did not pick. A press on something that is selected, while
//! another kind is selected with it, shows only the rows both kinds have: the clipboard holds
//! one kind at a time, so 切り取り and コピー are dim, and 削除 takes both away.

use serde_json::json;

use super::{Act, Item};
use crate::studio::{Geometry, Studio};

/// What a dim 切り取り / コピー of a mixed selection says when the pointer rests on it.
const ONE_KIND: &str = "選択の種類が混在しています";

impl Studio {
    /// The rows of a mixed selection; `delete` is the row of the scene that removes both kinds.
    pub(super) fn mixed_items(&mut self, g: &Geometry, delete: Item) -> Vec<Item> {
        let dim = |mut row: Item| {
            row.enabled = false;
            row.reason = ONE_KIND.to_string();
            row
        };
        let cut = self.ctx_key(g, "edit.cut");
        let copy = self.ctx_key(g, "edit.copy");
        vec![dim(cut), dim(copy), Item::gap(), delete]
    }

    /// The 削除 row of the arrangement's mixed selection.
    pub(super) fn mixed_delete(&self) -> Item {
        Item::act("削除", &self.menu_key("clip.delete"), Act::DeletePoints)
    }

    /// 削除 of clips and automation points selected together.
    pub(crate) fn delete_points_and_clips(&mut self) -> bool {
        self.points_act(false);
        let ids: Vec<String> = self
            .picked_clips()
            .iter()
            .map(ToString::to_string)
            .collect();
        if !ids.is_empty()
            && self
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                .arrange_run("clip.delete", json!({ "ids": ids }))
                .is_some()
        {
            self.after_clips_gone();
        }
        true
    }

    /// Whether clips and automation points are selected together.
    pub(crate) fn clips_and_points_selected(&self) -> bool {
        !self.arrange.points.is_empty() && !self.arrange.clips.is_empty()
    }
}
