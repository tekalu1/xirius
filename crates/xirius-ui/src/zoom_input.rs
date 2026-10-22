//! Zoom gestures keep their own history; they never edit the song.
use crate::input_dispatch::Zoom;
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Mods;

#[derive(Clone, Copy)]
pub struct BoxZoom {
    pub from: (f32, f32),
    pub to: (f32, f32),
    before: Zoom,
    bounds: Rect,
}

impl Studio {
    pub fn zoom_box_down(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        if ((!mods.alt || !mods.shift || mods.ctrl) && !self.input_map.zoom_rect_armed)
            || self.popup.is_some()
        {
            return false;
        }
        let roll = self.roll_ctx(g).map(|ctx| self.roll.frame(ctx.body).grid);
        let bounds = if let Some(grid) = roll.filter(|r| r.contains(x, y)) {
            self.focus_arrange(false);
            grid
        } else if g.lanes.contains(x, y) {
            self.focus_arrange(true);
            g.lanes
        } else {
            return false;
        };
        self.input_map.zoom_rect_armed = false;
        self.input_map.zoom_box = Some(BoxZoom {
            from: (x, y),
            to: (x, y),
            before: self.zoom_snapshot(),
            bounds,
        });
        true
    }

    pub fn zoom_box_move(&mut self, x: f32, y: f32) -> bool {
        let Some(z) = &mut self.input_map.zoom_box else {
            return false;
        };
        z.to = (
            x.clamp(z.bounds.x, z.bounds.right()),
            y.clamp(z.bounds.y, z.bounds.bottom()),
        );
        true
    }

    pub fn zoom_box_up(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !self.zoom_box_move(x, y) {
            return false;
        }
        let z = self.input_map.zoom_box.take().unwrap();
        let width = (z.to.0 - z.from.0).abs();
        let height = (z.to.1 - z.from.1).abs();
        if width < crate::roll::SLOP || height < crate::roll::SLOP {
            return self.zoom_command(g, "view.zoom.undo", &Default::default());
        }
        let left = z.from.0.min(z.to.0) - z.bounds.x;
        let top = z.from.1.min(z.to.1) - z.bounds.y;
        let time = z.before.time * z.bounds.w / width;
        let time = if z.before.roll {
            time.clamp(crate::roll::BEAT_PX_MIN, crate::roll::BEAT_PX_MAX)
        } else {
            time.clamp(Studio::BEAT_W_MIN, self.beat_w_max())
        };
        let data = if z.before.roll {
            (z.before.data * z.bounds.h / height)
                .clamp(crate::roll::ROW_H_MIN, crate::roll::ROW_H_MAX)
        } else {
            (z.before.data * z.bounds.h / height).clamp(
                crate::arrange::Arrange::TRACK_H_MIN,
                crate::arrange::Arrange::TRACK_H_MAX,
            )
        };
        let scroll = (
            (z.before.scroll.0 + left) * time / z.before.time,
            (z.before.scroll.1 + top) * data / z.before.data,
        );
        self.remember_zoom(z.before);
        self.restore_zoom(
            g,
            Zoom {
                time,
                data,
                scroll,
                roll: z.before.roll,
            },
        );
        true
    }
}

pub fn paint(pen: &mut crate::draw::Pen, studio: &Studio) {
    if let Some(z) = studio.input_map.zoom_box {
        let r = Rect::new(
            z.from.0.min(z.to.0),
            z.from.1.min(z.to.1),
            (z.to.0 - z.from.0).abs(),
            (z.to.1 - z.from.1).abs(),
        );
        pen.fill(r, pen.c.sel);
    }
}
