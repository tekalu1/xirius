use super::*;

impl Studio {
    pub(super) fn link_at(&self, g: &Geometry, x: f32, y: f32, spot: &Spot) -> bool {
        if !self.song.session().state.view.link {
            return false;
        }
        let (track, clip) = match spot {
            Spot::Clip { track, clip, .. }
            | Spot::Fx {
                track,
                clip,
                part: clipfx::FxPart::Line { .. },
            } => (*track, *clip),
            _ => return false,
        };
        let Some(row) = self.tree().row_of(RowKind::Track(track)) else {
            return false;
        };
        if row.h < Layout::TRACK_ROW_H {
            return false;
        }
        let p = self.project();
        let Ok((_, c)) = p.clip(clip) else {
            return false;
        };
        let r = self.clip_rect(g, row, c);
        x > r.x + Arrange::GRIP && x < r.right() - Arrange::GRIP && y < r.y + r.h / 2.0
    }

    pub(super) fn smart_click(&mut self, g: &Geometry, d: &Drag) -> bool {
        if d.tool != Tool::Select || d.mods.shift || d.mods.alt || d.mods.ctrl {
            return false;
        }
        let (track, clip) = match d.spot {
            Spot::Clip { track, clip, .. }
            | Spot::Fx {
                track,
                clip,
                part: clipfx::FxPart::Boundary(_),
            } => (track, clip),
            _ => return false,
        };
        let p = self.project();
        let Some(row) = self.tree().row_of(RowKind::Track(track)) else {
            return false;
        };
        let Ok((_, c)) = p.clip(clip) else {
            return false;
        };
        let r = self.clip_rect(g, row, c);
        if d.from.1 < r.bottom() - 8.0 {
            return false;
        }
        let end = c.start + c.length;
        if (self.tick_x(g, end) - d.from.0).abs() < Arrange::GRIP
            && let Some(next) = p.tracks[&track]
                .clips
                .iter()
                .find(|n| n.id != clip && n.start == end)
        {
            let ids = json!([clip.to_string(), next.id.to_string()]);
            if let Some(data) = self.arrange_run("clip.join", json!({"ids":ids})) {
                self.select_made(&data);
            }
            return true;
        }
        let at = self.snap_tick(d.tick, false);
        if at > c.start && at < end {
            if let Some(data) =
                self.arrange_run("clip.split", json!({"ids":[clip.to_string()],"at":at}))
            {
                self.select_made(&data);
            }
            return true;
        }
        false
    }

    pub(super) fn sweep_tools(&mut self, g: &Geometry, x: f32, y: f32) {
        let Some(d) = self.arrange.drag.as_ref() else {
            return;
        };
        if !matches!(d.tool, Tool::Eraser | Tool::Mute) {
            return;
        }
        let from = d.at;
        let count = ((x - from.0).hypot(y - from.1) / 4.0).ceil().max(1.0) as usize;
        for i in 1..=count {
            let f = i as f32 / count as f32;
            let spot = self.arrange_spot(g, from.0 + (x - from.0) * f, from.1 + (y - from.1) * f);
            if let Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } = spot {
                self.arrange.swept.insert((track, clip));
            }
        }
    }

    pub(super) fn finish_tools(&mut self, g: &Geometry, d: &Drag) -> bool {
        if matches!(d.tool, Tool::Eraser | Tool::Mute)
            && matches!(d.spot, Spot::Clip { .. } | Spot::Space(_))
        {
            let ids: Vec<_> = self
                .arrange
                .swept
                .iter()
                .map(|(_, c)| c.to_string())
                .collect();
            if !ids.is_empty() {
                self.arrange_run(
                    if d.tool == Tool::Eraser {
                        "clip.delete"
                    } else {
                        "clip.mute"
                    },
                    json!({"ids":ids}),
                );
            }
            self.arrange.swept.clear();
            return true;
        }
        if d.tool == Tool::Draw
            && matches!(d.spot, Spot::Band)
            && let Some(plan) = self.draw_section_plan(g, d)
            && let Some((command, args)) = plan.command
            && let Some(data) = self.arrange_run(command, args)
        {
            self.select_section_made(&data);
            return true;
        }
        if d.tool == Tool::Draw && matches!(d.spot, Spot::Space(_)) {
            if let Some(plan) = self.draw_part_plan(g, d)
                && let Some((command, args)) = plan.command
                && let Some(data) = self.arrange_run(command, args)
                && let Some(id) = data["id"].as_str()
            {
                self.select_made(&json!({"ids":[id]}));
            }
            return true;
        }
        false
    }

    /// The pencil on the empty part of the band: the bars from the press to the pointer become
    /// a section (a press alone makes one bar), stopping where the next section begins.
    pub(super) fn draw_section_plan(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        let p = self.project();
        let map = p.time_map();
        let bar_of = |t: Tick| map.to_bbt(t.max(0)).0;
        let (a, b) = (bar_of(d.tick), bar_of(self.tick_at(g, d.at.0)));
        let start = map.bar_start(a.min(b));
        let mut end = map.bar_start(a.max(b) + 1);
        // The next section is a wall (ARR-007): the picture stops where the command will.
        if let Some(wall) = p
            .sections
            .iter()
            .map(|s| s.start)
            .filter(|s| *s > start)
            .min()
        {
            end = end.min(wall);
        }
        let length = end - start;
        if length <= 0 {
            return None;
        }
        let bars = length as f64 / self.bar_ticks() as f64;
        Some(Plan {
            command: Some((
                "section.add",
                json!({ "name": self.fresh_section_name(), "at": start, "length": length }),
            )),
            select: None,
            tip: format!("{bars:.0} 小節"),
            preview: Preview::NewSection { start, end },
        })
    }

    pub(super) fn draw_part_plan(&self, g: &Geometry, d: &Drag) -> Option<Plan> {
        let Spot::Space(RowKind::Track(track)) = d.spot else {
            return None;
        };
        if self.project().track(track).ok()?.kind != TrackKind::Instrument {
            return None;
        }
        let a = self.snap_tick(d.tick, d.mods.shift);
        let b = self.snap_tick(self.tick_at(g, d.at.0).max(0), d.mods.shift);
        let start = a.min(b);
        let length = (a - b).abs().max(self.tools().snap.step(&self.meter()));
        Some(Plan {
            command: Some((
                "clip.add.midi",
                json!({"track":track.to_string(),"at":start,"length":length}),
            )),
            select: None,
            tip: format!("長さ {:.2} 小節", length as f64 / self.bar_ticks() as f64),
            preview: Preview::Clips(vec![Ghost {
                track,
                start,
                length,
                color: crate::draw::track_color(self.project().track(track).ok()?),
            }]),
        })
    }
}
