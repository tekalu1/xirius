//! Positions on the ruler and the S12 marker menu.
use super::Item;
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use serde_json::json;
use xirius_core::id::MarkerId;
use xirius_design::{FontSize, Radius, Weight};

impl Studio {
    pub(crate) fn marker_rect(&self, g: &Geometry, id: MarkerId) -> Option<Rect> {
        let p = self.project();
        let m = p.markers.iter().find(|m| m.id == id)?;
        let x = self.tick_x(g, m.tick);
        let end = p
            .markers
            .iter()
            .filter(|n| n.tick > m.tick)
            .map(|n| self.tick_x(g, n.tick))
            .next()
            .unwrap_or(g.ruler.right());
        Some(Rect::new(
            x,
            g.ruler.y + crate::ruler::LOOP_H,
            (end - x).clamp(0.0, 100.0),
            g.ruler.h - crate::ruler::LOOP_H,
        ))
    }

    pub(crate) fn marker_context(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if !g.ruler.contains(x, y) {
            return false;
        }
        let p = self.project();
        let Some(m) = p
            .markers
            .iter()
            .find(|m| self.marker_rect(g, m.id).is_some_and(|r| r.contains(x, y)))
        else {
            return false;
        };
        let id = m.id.to_string();
        self.input_map.marker_selected = Some(m.id);
        self.input_map.tempo_selected = None;
        self.focus_arrange(true);
        let rename = self.rename_item("marker.rename", id.clone(), &m.name);
        let loop_to = self.ctx_cmd("marker.loop", json!({"id": id}));
        let color = Item::colors("既定", "marker.color.set", json!({"ids": [id]}), true);
        let mut delete = self.ctx_cmd("marker.delete", json!({"ids": [id]}));
        delete.key = self.key_display("arrange", "clip.delete");
        self.open_context(vec![rename, loop_to, color, Item::gap(), delete], x, y);
        true
    }
}

pub(crate) fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    for m in &s.project().markers {
        let Some(r) = s.marker_rect(g, m.id) else {
            continue;
        };
        if r.right() < g.ruler.x || r.x > g.ruler.right() {
            continue;
        }
        let color = m
            .color
            .as_deref()
            .and_then(crate::song::palette_color)
            .map(|c| pen.c.tracks[c.index()])
            .unwrap_or(pen.c.fill);
        pen.mask(r, |pen| {
            pen.round(
                Rect::new(r.x, r.y + 3.0, r.w.min(8.0), r.h - 6.0),
                color,
                Radius::Xs,
            );
            if r.w <= 12.0 {
                return;
            }
            pen.text(
                &m.name,
                FontSize::Micro,
                Weight::Semibold,
                pen.c.ink,
                Rect::new(r.x + 12.0, r.y, r.w - 12.0, r.h),
                Align::Left,
            );
        });
    }
}

pub(crate) fn paint_seconds(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let p = s.project();
    let map = xirius_session::tempo::TempoMap::from_project(&p);
    let tm = p.time_map();
    let every = crate::ruler::bar_every(s.ticks_to_px(tm.bar_start(2) - tm.bar_start(1)));
    for (bar, tick) in s.visible_bars(g) {
        if (bar - 1) % every != 0 {
            continue;
        }
        let x = s.tick_x(g, tick);
        if p.markers.iter().any(|m| {
            s.marker_rect(g, m.id)
                .is_some_and(|r| r.contains(x + 4.0, r.y + 2.0))
        }) {
            continue;
        }
        let secs = map.seconds_at(tick).max(0.0).round() as u64;
        pen.text(
            &format!("{}:{:02}", secs / 60, secs % 60),
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink_blue,
            Rect::new(x + 4.0, g.ruler.y, 64.0, g.ruler.h),
            Align::Left,
        );
    }
}
