//! The two small menus of the tool bar: the ▾ right of the arrow, which lists the tools the
//! arrow turns into while Ctrl is held (and in the roll the arrow's mode, UI-286), and the button
//! right of the pencil, which lists the shapes the pencil draws (UI-128). Both run the command of
//! the same name (`tool.alt`, `prefs.set`, `tool.draw.shape`), so the CLI and MCP do the same.

use serde_json::json;
use xirius_core::commands::{ALT_TOOLS, ROLL_ALT_TOOLS};
use xirius_core::shape::Shape;

use crate::context::Item;
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::{Area, Tool};
use crate::topbar::BarButton;
use xirius_design::Layout;

impl Studio {
    /// Opens the menu under a button of the tool bar.
    fn open_bar_menu(&mut self, g: &Geometry, button: BarButton, items: Vec<Item>) {
        let r = self.bar_layout(g.size.0).rect(button);
        self.open_context(items, r.x, r.bottom() - Layout::TOP_BAR_H);
    }

    /// The ▾ of the arrow: the tools it can turn into, the one in effect checked, and in the
    /// roll the arrow's mode (UI-286). The choice of tool is the `<area>.select.hold` row of the
    /// mouse table, where `tool.alt` writes it; the roll's mode is `edit.arrow.mode`.
    pub(crate) fn open_alt_menu(&mut self, g: &Geometry) {
        let now = self.alt_tool();
        let area = self.tool_area();
        let (area_name, names) = match area {
            Area::Arrange => ("arrange", ALT_TOOLS),
            Area::Roll => ("roll", ROLL_ALT_TOOLS),
        };
        let mut items: Vec<Item> = names
            .iter()
            .filter_map(|name| Tool::named(name))
            .map(|tool| {
                self.ctx_setting(
                    tool.label(),
                    "tool.alt",
                    json!({"area": area_name, "tool": tool.name()}),
                )
                .on(tool == now)
            })
            .collect();
        if area == Area::Roll {
            items.push(Item::gap());
            for (value, label, on) in [
                ("extended", "拡張", self.roll.extended),
                ("basic", "基本", !self.roll.extended),
            ] {
                items.push(
                    self.ctx_setting(
                        label,
                        "prefs.set",
                        json!({"key": "edit.arrow.mode", "value": value}),
                    )
                    .on(on),
                );
            }
        }
        self.open_bar_menu(g, BarButton::AltTool, items);
    }

    /// The pencil's shapes, the chosen one checked; the transform frame stands apart.
    pub(crate) fn open_shape_menu(&mut self, g: &Geometry) {
        let now = self.song.session().state.view.draw_shape;
        let mut items = Vec::new();
        for shape in Shape::ALL {
            if shape == Shape::Transform {
                items.push(Item::gap());
            }
            items.push(
                self.ctx_setting(
                    shape.label(),
                    "tool.draw.shape",
                    json!({"shape": shape.name()}),
                )
                .on(shape == now),
            );
        }
        self.open_bar_menu(g, BarButton::Shape, items);
    }

    /// The wheel over the pencil or its shape button: the next or the previous shape.
    pub(crate) fn bar_shape_wheel(&mut self, g: &Geometry, x: f32, y: f32, delta: f32) -> bool {
        let over = self.hit(g, x, y);
        if !matches!(
            over,
            Hit::Bar(BarButton::Shape | BarButton::Tool(Tool::Draw))
        ) {
            return false;
        }
        if delta != 0.0 {
            let arg = if delta < 0.0 { "next" } else { "prev" };
            // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
            if let Err(e) = self.song.set("tool.draw.shape", json!({ arg: true })) {
                self.error = Some(e);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn studio() -> (Studio, Geometry) {
        let mut s = Studio::sample();
        s.drawer_open = false;
        s.choose_editor();
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        (s, g)
    }

    fn shape(s: &Studio) -> Shape {
        s.song.session().state.view.draw_shape
    }

    fn press(s: &mut Studio, g: &Geometry, b: BarButton) {
        let (x, y) = s.center_of(g, Hit::Bar(b)).expect("on the bar");
        assert_eq!(s.hit(g, x, y), Hit::Bar(b));
        assert!(s.press(g, Hit::Bar(b)));
    }

    #[test]
    fn the_arrow_has_a_menu_of_alternative_tools() {
        let (mut s, g) = studio();
        press(&mut s, &g, BarButton::AltTool);
        let rows = s.context_rows();
        let labels: Vec<_> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels.len(), ALT_TOOLS.len());
        assert!(rows.iter().all(|r| r.enabled), "{rows:?}");
        // The tool in effect carries the check.
        let on: Vec<_> = rows.iter().filter(|r| r.on).collect();
        assert_eq!(on.len(), 1);
        assert_eq!(on[0].label, s.alt_tool().label());
        // Choosing a row runs `tool.alt`, the command the mouse table shares.
        let i = rows
            .iter()
            .position(|r| r.label == Tool::Eraser.label())
            .expect("the eraser is listed");
        assert!(s.context_pick(&g, i));
        assert_eq!(s.alt_tool(), Tool::Eraser);
        assert!(s.bar_layout(g.size.0).rect(BarButton::AltTool).w > 0.0);
    }

    #[test]
    fn ui_286_the_rolls_arrow_menu_lists_its_alternative_tools_and_extended_or_basic() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 1000.0);
        s.resized(&g);
        assert_eq!(s.tool_area(), Area::Roll);
        assert_eq!(s.alt_tool(), Tool::Draw);
        // The arrow starts basic (2026-10-21).
        assert!(!s.bar_view().arrow_extended);
        press(&mut s, &g, BarButton::AltTool);
        let rows = s.context_rows();
        let labels: Vec<_> = rows
            .iter()
            .filter(|r| !r.gap)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec![
                "鉛筆",
                "消しゴム",
                "分割",
                "ミュート",
                "試聴",
                "拡張",
                "基本"
            ]
        );
        assert_eq!(rows.iter().filter(|r| r.gap).count(), 1);
        let checked: Vec<_> = rows
            .iter()
            .filter(|r| r.on)
            .map(|r| r.label.as_str())
            .collect();
        assert_eq!(checked, vec!["鉛筆", "基本"]);

        // Picking an alternative tool in the roll updates the roll's Ctrl hold, not the arrangement's.
        let i = rows
            .iter()
            .position(|r| r.label == Tool::Eraser.label())
            .unwrap();
        assert!(s.context_pick(&g, i));
        assert_eq!(s.alt_tool(), Tool::Eraser);
        assert_eq!(s.bar_view().alt, Tool::Eraser);
        assert_eq!(
            s.holds()
                .tool(Area::Arrange, Tool::Select, crate::tools::Mods::ctrl()),
            Tool::Range
        );

        // Picking "拡張" updates both `roll.extended` and `edit.arrow.mode` in prefs.
        press(&mut s, &g, BarButton::AltTool);
        let i = s
            .context_rows()
            .iter()
            .position(|r| r.label == "拡張")
            .unwrap();
        assert!(s.context_pick(&g, i));
        assert!(s.roll.extended);
        assert!(s.bar_view().arrow_extended);
        let mode = s
            .song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .text("edit.arrow.mode");
        assert_eq!(mode, "extended");

        // Pressing the arrow again in the roll toggles back to basic and writes `edit.arrow.mode`.
        press(&mut s, &g, BarButton::Tool(Tool::Select));
        assert!(!s.roll.extended);
        assert!(!s.bar_view().arrow_extended);
        let mode = s
            .song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .text("edit.arrow.mode");
        assert_eq!(mode, "basic");
    }

    #[test]
    fn the_shape_button_picks_a_shape_and_the_wheel_steps_through_them() {
        let (mut s, g) = studio();
        assert_eq!(shape(&s), Shape::Free);
        press(&mut s, &g, BarButton::Shape);
        let rows = s.context_rows();
        let rows: Vec<_> = rows.iter().filter(|r| !r.gap).collect();
        assert_eq!(rows.len(), Shape::ALL.len());
        assert!(rows.iter().all(|r| r.enabled));
        assert_eq!(rows.iter().filter(|r| r.on).count(), 1);
        assert!(rows[0].on);
        let i = s
            .context_rows()
            .iter()
            .position(|r| r.label == Shape::Sine.label())
            .expect("sine is listed");
        assert!(s.context_pick(&g, i));
        assert_eq!(shape(&s), Shape::Sine);
        // The wheel over the pencil or the button: down is the next, up the previous.
        let (x, y) = s
            .center_of(&g, Hit::Bar(BarButton::Tool(Tool::Draw)))
            .unwrap();
        assert!(s.bar_shape_wheel(&g, x, y, -1.0));
        assert_eq!(shape(&s), Shape::Transform);
        let (x, y) = s.center_of(&g, Hit::Bar(BarButton::Shape)).unwrap();
        assert!(s.bar_shape_wheel(&g, x, y, 1.0));
        assert_eq!(shape(&s), Shape::Sine);
        // Elsewhere on the bar the wheel is not this button's.
        let (x, y) = s
            .center_of(&g, Hit::Bar(BarButton::Tool(Tool::Cut)))
            .unwrap();
        assert!(!s.bar_shape_wheel(&g, x, y, 1.0));
        assert_eq!(shape(&s), Shape::Sine);
    }
}
