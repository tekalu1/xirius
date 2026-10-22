//! Copying, cutting and pasting the selected points of a controller lane (CTX-028, KEY-003).
//!
//! The points go to the session's clipboard (`edit.copy`, `edit.cut`), the one the arrangement's
//! automation points and the CLI use too. A paste lands where [`Roll::paste_points`] is told.

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::time::Tick;

use super::{Ctx, Edit, Out, Roll, Then};

impl Roll {
    /// The selected points as the `ticks` argument of the edit commands.
    fn point_ticks(&self) -> String {
        self.points
            .iter()
            .map(Tick::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Ctrl+C (`cut` false) and Ctrl+X on selected points.
    pub(super) fn points_to_board(&mut self, ctx: &Ctx, cut: bool) -> Out {
        let Some(kind) = self.active().and_then(|l| l.kind.ctrl()) else {
            return Out::used();
        };
        let args = json!({
            "clip": ctx.clip.id.to_string(),
            "lane": kind.key(),
            "ticks": self.point_ticks(),
        });
        if cut {
            self.points.clear();
            Out::edit(Edit::new("edit.cut", args, Then::Keep))
        } else {
            Out::edit(Edit::new("edit.copy", args, Then::Keep))
        }
    }

    /// The song position under the pointer, on the grid: where a paste lands. None when the
    /// pointer has not been over the roll.
    pub fn paste_tick(&self, ctx: &Ctx) -> Option<Tick> {
        let (x, _) = self.pointer?;
        let tick = ctx.snapped(self.beat_px, false, self.view(ctx).tick(x));
        Some(ctx.clip.start + tick.clamp(0, (ctx.clip_len() - 1).max(0)))
    }

    /// Ctrl+V with points on the clipboard: into the open controller lane, the first point at
    /// the song position `at`. The pasted points are the selection afterwards.
    pub fn paste_points(&mut self, ctx: &Ctx, board: &Clipboard, at: Tick) -> Out {
        let Clipboard::Controller { points, .. } = board else {
            return Out::used();
        };
        let Some(kind) = self.active().and_then(|l| l.kind.ctrl()) else {
            return Out::used();
        };
        let shift = at - board.first_tick();
        self.points = points
            .iter()
            .map(|p| p.tick + shift - ctx.clip.start)
            .filter(|t| (0..ctx.clip_len()).contains(t))
            .collect();
        Out::edit(Edit::new(
            "edit.paste",
            json!({
                "clip": ctx.clip.id.to_string(),
                "lane": kind.key(),
                "at": at,
            }),
            Then::Keep,
        ))
    }
}
