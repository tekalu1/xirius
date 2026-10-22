//! The menus of the tempo row: a tempo point (S15) and the rest of the row (S16).

use serde_json::json;

use super::Item;
use crate::studio::{Geometry, Studio};

impl Studio {
    /// The right press on a tempo point.
    pub(crate) fn tempo_point_context(&mut self, i: usize, x: f32, y: f32) -> bool {
        self.input_map.tempo_selected = self.project().tempo.get(i).map(|t| t.id);
        self.input_map.marker_selected = None;
        self.focus_arrange(true);
        let Some(items) = self.tempo_point_items(i) else {
            return false;
        };
        self.open_context(items, x, y);
        true
    }

    /// The right press in the tempo row, off the points.
    pub(crate) fn tempo_row_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let items = self.tempo_row_items(g, x);
        self.open_context(items, x, y);
        true
    }

    /// S15: what can be done to a tempo point.
    fn tempo_point_items(&mut self, i: usize) -> Option<Vec<Item>> {
        let p = self.project();
        let t = p.tempo.get(i)?.clone();
        let next = i + 1 < p.tempo.len();
        let at = t.tick;
        let shape = |s: &mut Studio, label: &str, ramp: bool| {
            s.ctx_as(
                label,
                "tempo.set",
                json!({ "at": at, "bpm": t.bpm, "ramp": ramp }),
            )
            .on(t.ramp == ramp)
            .enabled(next)
        };
        let shapes = vec![shape(self, "階段", false), shape(self, "直線", true)];
        let mut delete = self.ctx_cmd("tempo.delete", json!({ "id": t.id.to_string() }));
        delete.key = self.key_display("arrange", "clip.delete");
        Some(vec![
            Item::ask_number(
                "BPM を入力…",
                "tempo.set",
                json!({ "at": at }),
                "bpm",
                &format!("{}", t.bpm),
            ),
            Item::sub("次のポイントまでの形", shapes).enabled(next),
            Item::gap(),
            delete,
        ])
    }

    /// S16: a stretch of the tempo row with no point under the pointer.
    fn tempo_row_items(&mut self, g: &Geometry, x: f32) -> Vec<Item> {
        let at = self.ctx_tick(g, x);
        let p = self.project();
        let bpm = p
            .tempo
            .iter()
            .rev()
            .find(|t| t.tick <= at)
            .or(p.tempo.first())
            .map_or(120.0, |t| t.bpm);
        let add = self.ctx_as(
            "ここにポイントを追加",
            "tempo.set",
            json!({ "at": at, "bpm": bpm }),
        );
        // The tempo points have no clipboard of their own, so the row stays dim.
        let paste = self
            .ctx_cmd("edit.paste", json!({ "at": at }))
            .enabled(false);
        let clear = self.ctx_cmd("tempo.clear", json!({}));
        vec![add, paste, Item::gap(), clear]
    }
}
