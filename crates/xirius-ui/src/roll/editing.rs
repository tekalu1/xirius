use super::*;

impl Roll {
    pub(super) fn restore_timing(&self, ctx: &Ctx) -> Out {
        let ids = self.target_ids(ctx, true);
        if ids.is_empty() {
            return Out::used();
        }
        Out::edit(Edit::new(
            "note.quantize",
            json!({"ids":ids_json(&ids),"restore":true}),
            Then::Keep,
        ))
    }
    pub fn arrow_again(&mut self) {
        self.extended = !self.extended;
    }

    pub fn in_scale(&self, pitch: u8) -> bool {
        self.scale
            .is_some_and(|(root, mask)| mask & (1 << ((pitch + 12 - root % 12) % 12)) != 0)
    }

    pub(super) fn join_under(&self, ctx: &Ctx, v: &View, x: f32, y: f32) -> Option<Out> {
        if !self.extended || v.row_h < 12.0 || self.mods.alt || self.mods.ctrl {
            return None;
        }
        let pitch = v.pitch_at(y);
        let row_y = v.y(pitch.clamp(0, 127) as u8);
        if y < row_y + v.row_h - 4.0 {
            return None;
        }
        for left in ctx.notes().iter().filter(|n| i32::from(n.pitch) == pitch) {
            let end = left.start + left.length;
            if (v.x(end) - x).abs() > 5.0 {
                continue;
            }
            let right = ctx
                .notes()
                .iter()
                .find(|n| n.id != left.id && n.pitch == left.pitch && n.start == end)?;
            return Some(Out::edit(Edit::new(
                "note.join",
                json!({"ids":ids_json(&[left.id, right.id])}),
                Then::Select,
            )));
        }
        None
    }

    pub(super) fn part_up(&self, ctx: &Ctx, v: &View, d: &Drag, x: f32) -> Out {
        if !d.moved {
            return Out::used();
        }
        let a = ctx.snapped(self.beat_px, self.mods.shift, v.tick(d.press.0)) + ctx.clip.start;
        let b = ctx.snapped(self.beat_px, self.mods.shift, v.tick(x)) + ctx.clip.start;
        let start = a.min(b).max(0);
        let end = a.max(b);
        let Some((track, _)) = ctx.project.find_clip(ctx.clip.id) else {
            return Out::used();
        };
        if end <= start {
            return Out::used();
        }
        Out::edit(Edit::new(
            "clip.add.midi",
            json!({"track":track.to_string(),"at":start,"length":end-start}),
            Then::Part,
        ))
    }
}
