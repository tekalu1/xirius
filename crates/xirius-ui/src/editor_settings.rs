//! Settings follow the editor that was last used.
use crate::{
    studio::{DrawerTab, Geometry, Studio},
    tools::{Area, Mods, SnapMode, Tool},
};

impl Studio {
    pub(crate) fn arrow_chosen(&mut self, g: &Geometry, tool: crate::tools::Tool) {
        if tool != crate::tools::Tool::Select || self.tool != tool {
            return;
        }
        if !self.arrange.focus && self.roll_ctx(g).is_some() {
            self.roll.arrow_again();
            let value = if self.roll.extended {
                "extended"
            } else {
                "basic"
            };
            self.rec_set(
                "prefs.set",
                serde_json::json!({ "key": "edit.arrow.mode", "value": value }),
            );
            self.apply_edit_prefs();
        } else {
            self.cycle_alt_tool();
        }
    }

    /// The area the top bar's tools stand for: the roll while it is the editor in use.
    pub(crate) fn tool_area(&self) -> Area {
        if self.song.session().state.view.editor == "roll" {
            Area::Roll
        } else {
            Area::Arrange
        }
    }

    /// The top bar's boxes: the ripple switch stands only in the arrangement's bar.
    pub(crate) fn bar_layout(&self, width: f32) -> crate::topbar::BarLayout {
        crate::topbar::BarLayout::of_save(
            width,
            &self.project().name,
            self.tool_area() == Area::Arrange,
            self.draw_length_text().is_some(),
            self.save_mark() == crate::keep::SaveMark::Failed,
            &self.grid_text(),
        )
    }

    /// The tool the arrow turns into while Ctrl is held (the "alternative tool"), which the top
    /// bar underlines.
    pub(crate) fn alt_tool(&self) -> Tool {
        self.holds()
            .tool(self.tool_area(), Tool::Select, Mods::ctrl())
    }

    /// The tool in effect while modifiers are held, when it is not the picked one.
    pub(crate) fn held_tool(&self) -> Option<Tool> {
        let area = self.tool_area();
        let mods = match area {
            Area::Roll => self.roll.mods,
            Area::Arrange => self.arrange.mods,
        };
        Some(self.holds().tool(area, self.tool, mods)).filter(|t| *t != self.tool)
    }

    /// Pressing the arrow again goes to the next alternative tool (`tool.alt` with no tool).
    pub(crate) fn cycle_alt_tool(&mut self) {
        let area = match self.tool_area() {
            Area::Roll => "roll",
            Area::Arrange => "arrange",
        };
        if let Err(e) = self
            .song
            // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
            .set("tool.alt", serde_json::json!({ "area": area }))
        {
            self.error = Some(e);
        }
    }
    /// The keys go to the arrangement (`true`) or the roll; the top bar's tools follow them.
    pub(crate) fn focus_arrange(&mut self, on: bool) {
        self.arrange.focus = on;
        self.choose_editor();
    }

    pub(crate) fn choose_editor(&mut self) {
        self.roll.scale = self.song.session().state.view.scale;
        let roll = !self.arrange.focus && self.drawer_open && self.tab == DrawerTab::Roll;
        self.song.session_mut().state.view.editor = if roll { "roll" } else { "arrange" }.into();
        let settings = if roll {
            &self.song.session().state.view.roll.quantize
        } else {
            &self.song.session().state.view.quantize
        };
        self.quant = crate::quantize_panel::QuantSet::from_json(settings);
    }

    pub(crate) fn grid_mode(&self) -> SnapMode {
        if self.song.session().state.view.editor == "roll" {
            self.roll_grid_mode()
        } else {
            self.snap_mode
        }
    }

    pub(crate) fn roll_grid_mode(&self) -> SnapMode {
        match self.song.session().state.view.roll.mode.as_deref() {
            Some("adaptive") => SnapMode::Auto,
            Some("bar") => SnapMode::Bar,
            Some("quantize") => SnapMode::Quantize,
            Some("frames") => SnapMode::Frames,
            _ => self.snap_mode,
        }
    }

    pub(crate) fn set_grid_mode(&mut self, mode: SnapMode) {
        if self.song.session().state.view.editor == "roll" {
            self.song.session_mut().state.view.roll.mode = Some(
                match mode {
                    SnapMode::Auto => "adaptive",
                    SnapMode::Bar => "bar",
                    SnapMode::Quantize => "quantize",
                    SnapMode::Frames => "frames",
                }
                .into(),
            );
        } else {
            if self.song.session().state.view.roll.mode.is_none() {
                self.song.session_mut().state.view.roll.mode = Some("adaptive".into());
            }
            self.snap_mode = mode;
        }
    }

    pub(crate) fn save_quant_settings(&mut self) {
        let settings = self.quant.to_json();
        let view = &mut self.song.session_mut().state.view;
        if view.editor == "roll" {
            view.roll.quantize = settings;
        } else {
            view.quantize = settings;
        }
    }

    pub(crate) fn roll_press(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        mods: crate::tools::Mods,
        clicks: u32,
    ) -> bool {
        let Some(mut ctx) = self.roll_input(g) else {
            return false;
        };
        let v = self.roll.view(&ctx);
        if v.grid.contains(x, y) && self.roll.hit_note(&ctx, &v, x, y).is_none() && clicks == 1 {
            let tick = ctx.clip.start + v.tick(x);
            let old_start = ctx.clip.start;
            if let Some((track, _)) = ctx.project.find_clip(ctx.clip.id)
                && let Some(c) = ctx.project.tracks[&track]
                    .clips
                    .iter()
                    .find(|c| c.is_midi() && c.start <= tick && tick < c.start + c.length)
                && c.id != ctx.clip.id
            {
                let id = c.id;
                let start = c.start;
                let scroll = self.roll.scroll;
                self.select_clip_here(track, id);
                ctx = self.roll_ctx(g).unwrap();
                self.roll.sync(&ctx);
                self.roll.scroll = (
                    scroll.0 + (old_start - start) as f32 / v.ppq * v.beat_px,
                    scroll.1,
                );
            }
        }
        if !ctx.body.contains(x, y) {
            return false;
        }
        self.arrange.focus = false;
        self.choose_editor();
        let out = self.roll.down(&ctx, x, y, mods, clicks);
        let used = out.used;
        self.apply_roll(g, out);
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{tools::Mods, topbar::BarButton};

    /// The studio with the arrange as the editor in use (the sample opens on the roll).
    fn arrange_studio() -> Studio {
        let mut s = Studio::sample();
        s.arrange.focus = true;
        s.choose_editor();
        assert_eq!(s.tool_area(), Area::Arrange);
        s
    }

    #[test]
    fn the_alternative_tool_is_range_until_chosen_and_the_bar_says_so() {
        let s = arrange_studio();
        assert_eq!(s.alt_tool(), Tool::Range);
        assert_eq!(s.bar_view().alt, Tool::Range);
        assert_eq!(s.bar_view().held, None);
    }

    #[test]
    fn the_arrow_pressed_again_goes_through_the_alternative_tools() {
        let mut s = arrange_studio();
        let g = s.geometry(1440.0, 820.0);
        s.tool = Tool::Select;
        let mut seen = vec![s.alt_tool()];
        for _ in 0..xirius_core::commands::ALT_TOOLS.len() {
            assert!(s.press(&g, crate::studio::Hit::Bar(BarButton::Tool(Tool::Select))));
            seen.push(s.alt_tool());
        }
        assert_eq!(seen.first(), seen.last(), "{seen:?} goes round");
        assert_eq!(
            seen[..seen.len() - 1]
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            xirius_core::commands::ALT_TOOLS.len(),
            "{seen:?} visits every tool once"
        );
        assert_eq!(s.bar_view().alt, s.alt_tool());
    }

    #[test]
    fn a_tool_picked_by_name_is_what_ctrl_turns_the_arrow_into() {
        let mut s = arrange_studio();
        let g = s.geometry(1440.0, 820.0);
        s.song
            .set("tool.alt", serde_json::json!({ "tool": "cut" }))
            .unwrap();
        s.arrange_move(&g, g.lanes.x + 10.0, g.lanes.y + 30.0, Mods::NONE);
        assert!(s.arrange_mods(Mods::ctrl()));
        assert_eq!(s.held_tool(), Some(Tool::Cut));
        assert_eq!(s.bar_view().held, Some(Tool::Cut));
        assert!(s.arrange_mods(Mods::NONE));
        assert_eq!(s.held_tool(), None);
        // Only the difference from the default is kept, and the default is not a difference.
        s.song
            .set("tool.alt", serde_json::json!({ "tool": "range" }))
            .unwrap();
        assert_eq!(s.alt_tool(), Tool::Range);
        let settings = s.song.settings().clone();
        let settings = settings.lock().unwrap();
        assert!(
            !settings
                .mousemap
                .changed
                .contains_key("arrange.select.hold")
        );
    }

    #[test]
    fn the_roll_has_its_own_alternative_tool() {
        let mut s = Studio::sample();
        assert_eq!(
            s.holds().tool(Area::Roll, Tool::Select, Mods::ctrl()),
            Tool::Draw
        );
        assert_eq!(s.bar_view().alt, Tool::Draw);
        assert!(
            !s.bar_view().arrow_extended,
            "the arrow starts basic (2026-10-21)"
        );
        assert!(s.bar_layout(1440.0).alt.w > 0.0);
        s.song
            .set(
                "tool.alt",
                serde_json::json!({ "tool": "erase", "area": "roll" }),
            )
            .unwrap();
        assert_eq!(
            s.holds().tool(Area::Roll, Tool::Select, Mods::ctrl()),
            Tool::Eraser
        );
        assert_eq!(s.bar_view().alt, Tool::Eraser);
        assert_eq!(
            s.holds().tool(Area::Arrange, Tool::Select, Mods::ctrl()),
            Tool::Range
        );
        assert!(
            s.song
                .set("tool.alt", serde_json::json!({ "tool": "select" }))
                .is_err()
        );
    }
}
