//! Recording gaps retained after Stop, at the engine's latency-corrected song positions.

use serde_json::Value;
use xirius_core::{id::TrackId, model::Project, time::Tick};
use xirius_design::{FontSize, Radius, Weight};

use crate::{
    draw::{Align, Pen},
    studio::{Geometry, Rect, Studio},
    tree::Row,
};

#[derive(Default)]
pub struct Loss {
    pub tracks: Vec<TrackId>,
    pub frames: u64,
    pub blocks: u64,
    pub positions: Vec<(Tick, u64)>,
}

impl Loss {
    pub fn receive(&mut self, data: &Value, project: &Project, rate: u32) {
        self.frames = data["dropped"].as_u64().unwrap_or(0);
        self.blocks = data["dropped_blocks"].as_u64().unwrap_or(0);
        self.positions = data["dropouts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let frame = r["song_frame"].as_u64()?;
                Some((
                    xirius_session::tempo::TempoMap::from_project(project)
                        .tick_of_frame(frame, f64::from(rate)),
                    r["blocks"].as_u64().unwrap_or(0),
                ))
            })
            .collect();
    }

    pub fn label(&self, project: &Project) -> String {
        let at = self
            .positions
            .iter()
            .map(|(tick, blocks)| format!("{} ({blocks})", project.time_map().format(*tick)))
            .collect::<Vec<_>>()
            .join(" / ");
        if at.is_empty() {
            format!("録音の欠落: {} 回・{} フレーム", self.blocks, self.frames)
        } else {
            format!(
                "録音の欠落: {} 回・{} フレーム  {at}",
                self.blocks, self.frames
            )
        }
    }
}

pub fn paint(
    pen: &mut Pen,
    s: &Studio,
    p: &Project,
    g: &Geometry,
    row: &Row,
    track: TrackId,
    y: f32,
) {
    let loss = &s.transport.loss;
    if loss.frames == 0 || !loss.tracks.contains(&track) {
        return;
    }
    let r = Rect::new(
        g.lanes.x + 8.0,
        y,
        g.lanes.w - 16.0,
        crate::rec_area::INPUT_H,
    );
    pen.mask(r, |pen| {
        pen.text(
            &loss.label(p),
            FontSize::Micro,
            Weight::Medium,
            pen.c.ink_alert,
            r,
            Align::Left,
        );
    });
    for (tick, _) in &loss.positions {
        let x = s.tick_x(g, *tick);
        if x >= g.lanes.x && x <= g.lanes.right() {
            pen.round(
                Rect::new(x - 4.0, y + row.h.min(8.0), 8.0, 8.0),
                pen.c.alert,
                Radius::Pill,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn capture_without_a_song_position_does_not_become_a_false_marker() {
        let p = Project::new("test");
        let mut loss = Loss::default();
        loss.receive(
            &json!({"dropped": 512, "dropped_blocks": 2, "dropouts": [
                {"song_frame": null, "capture_frame": 0, "blocks": 1},
                {"song_frame": 48000, "capture_frame": 49000, "blocks": 1}
            ]}),
            &p,
            48000,
        );
        assert_eq!(loss.positions.len(), 1);
        assert_eq!(
            loss.positions[0].0,
            xirius_session::tempo::TempoMap::from_project(&p).tick_of_frame(48000, 48000.0)
        );
        assert!(loss.label(&p).contains("2 回・512 フレーム"));
    }
}
