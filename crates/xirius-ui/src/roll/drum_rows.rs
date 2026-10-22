//! The drum rows changed by hand (EDT-006): the map button's menu hides and renames them, and
//! the menu of a row's name renames, hides and moves that row. Remembered for the track, outside
//! the history.

use serde_json::json;
use xirius_core::drum::{self, RowEdits};

use super::Ctx;
use crate::context::Item;
use crate::studio::Studio;

/// A part's rows as the menus see them.
struct Part {
    clip: String,
    map: String,
    edits: RowEdits,
    used: Vec<u8>,
}

impl Part {
    fn of(ctx: &Ctx) -> Option<Part> {
        Some(Part {
            clip: ctx.clip.id.to_string(),
            map: ctx.drum.clone()?,
            edits: ctx.drum_rows.clone(),
            used: ctx.notes().iter().map(|n| n.pitch).collect(),
        })
    }

    fn shown(&self) -> Vec<u8> {
        drum::shown_rows(&self.map, &self.edits, self.used.iter().copied())
    }

    fn args(&self, pitch: u8) -> serde_json::Value {
        json!({ "clip": self.clip, "pitch": pitch })
    }

    fn rename(&self, label: &str, pitch: u8) -> Item {
        Item::ask(
            label,
            "roll.drum.rename",
            self.args(pitch),
            "name",
            &self.edits.name(pitch),
        )
    }

    /// 上へ移動 and 下へ移動, each off at its end.
    fn moves(&self, s: &mut Studio, pitch: u8) -> [Item; 2] {
        let rows = self.shown();
        let at = rows.iter().position(|p| *p == pitch).unwrap_or(0);
        let by = |by: i64| json!({ "clip": self.clip, "pitch": pitch, "by": by });
        [
            s.ctx_setting("上へ移動", "roll.drum.move", by(-1))
                .enabled(at > 0),
            s.ctx_setting("下へ移動", "roll.drum.move", by(1))
                .enabled(at + 1 < rows.len()),
        ]
    }

    fn hide(&self, s: &mut Studio, label: &str, pitch: u8, hidden: bool) -> Item {
        let mut args = self.args(pitch);
        if hidden {
            args["show"] = json!(true);
        }
        // A row with notes shows anyway, so hiding it is off until it is empty.
        s.ctx_setting(label, "roll.drum.hide", args)
            .on(hidden)
            .enabled(hidden || !self.used.contains(&pitch))
    }
}

impl Studio {
    /// The rows under the maps in the map button's menu: 隠す, 行名を変更 and the way back to the
    /// map as it comes. A row is moved from the menu of its own name.
    pub(crate) fn drum_row_items(&mut self, ctx: &Ctx) -> Vec<Item> {
        let Some(part) = Part::of(ctx) else {
            return Vec::new();
        };
        let shown = part.shown();
        let hide = drum::all_rows(&part.map, part.used.iter().copied())
            .into_iter()
            .chain(part.edits.hidden.iter().copied())
            .fold(Vec::new(), |mut all: Vec<u8>, p| {
                if !all.contains(&p) {
                    all.push(p);
                }
                all
            })
            .into_iter()
            .map(|p| {
                let hidden = part.edits.hidden.contains(&p);
                part.hide(self, &part.edits.name(p), p, hidden)
            })
            .collect();
        let names = shown
            .iter()
            .map(|p| part.rename(&part.edits.name(*p), *p))
            .collect();
        let reset = self
            .ctx_setting(
                "既定に戻す",
                "roll.drum.reset",
                json!({ "clip": part.clip }),
            )
            .enabled(!part.edits.is_empty());
        vec![
            Item::gap(),
            Item::sub("隠す", hide),
            Item::sub("行名を変更", names),
            reset,
        ]
    }

    /// The rows the menu of a drum row's name adds, all about that row: its name, hiding it and
    /// its place.
    pub(crate) fn drum_name_items(&mut self, ctx: &Ctx, pitch: u8) -> Vec<Item> {
        let Some(part) = Part::of(ctx) else {
            return Vec::new();
        };
        let [up, down] = part.moves(self, pitch);
        vec![
            Item::gap(),
            part.rename("名前を変更", pitch),
            part.hide(self, "隠す", pitch, false),
            Item::gap(),
            up,
            down,
        ]
    }
}
