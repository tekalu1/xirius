//! S05 uses the take under the pointer, including the pieces left by comping.
use super::Item;
use crate::studio::Studio;
use serde_json::json;
use xirius_core::id::ClipId;
use xirius_core::takes;
use xirius_core::time::Tick;

impl Studio {
    /// The menu of a take's row: `bar` is the stretch between dividers under the pointer (none
    /// when the take is not under one).
    pub(super) fn take_context_over(
        &mut self,
        id: ClipId,
        bar: Option<(Tick, Tick)>,
        x: f32,
        y: f32,
    ) -> bool {
        let a = json!({"id": id.to_string()});
        // The stretch chosen on this take's track, if there is one, is the selection.
        let chosen = self.comp_sel.filter(|c| {
            self.project()
                .tracks
                .get(&c.track)
                .is_some_and(|t| takes::stack_of(t, id).is_some())
        });
        let span = chosen.map(|c| (c.from, c.to)).or(bar);
        let (from, to) = span.unwrap_or((0, 0));
        let activate = self.ctx_cmd("take.activate", a.clone());
        let selection = self.ctx_as(
            "選択範囲にこのテイクを使用",
            "take.comp",
            json!({"take": id.to_string(), "from": from, "to": to}),
        );
        let unpack = self.ctx_cmd("take.unpack", a.clone());
        let undo_lap = self.ctx_cmd("take.undo-lap", a.clone());
        let clean = self.ctx_cmd("take.clean", a.clone());
        let mut delete = self.ctx_cmd("take.delete", a.clone());
        delete.key = self.key_display("arrange", "clip.delete");
        self.input_map.context_delete = Some(("take.delete", a));
        self.open_context(
            vec![
                activate,
                selection,
                Item::gap(),
                unpack,
                undo_lap,
                Item::gap(),
                clean,
                delete,
            ],
            x,
            y,
        );
        true
    }

    pub(super) fn take_items(&mut self, id: ClipId) -> Vec<Item> {
        let a = json!({"id": id.to_string()});
        vec![
            self.ctx_cmd("take.activate", a.clone()),
            Item::gap(),
            self.ctx_cmd("take.clean", a.clone()),
            self.ctx_cmd("take.delete", a),
        ]
    }
}
