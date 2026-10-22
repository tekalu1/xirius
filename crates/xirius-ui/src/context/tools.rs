//! An empty-space middle click opens tools; a middle drag keeps panning.
use super::{Act, Item};
use crate::studio::{Geometry, Studio};
use crate::tools::Tool;

impl Studio {
    pub(super) fn tool_item(&self, name: &'static str) -> Item {
        let context = if self.arrange.focus {
            "arrange"
        } else {
            "roll"
        };
        Item::act(
            &self.label_of(name),
            &self.key_display(context, name),
            Act::Tool(name),
        )
    }

    pub(crate) fn middle_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.input_map.middle_origin = Some((x, y));
        self.input_map.middle_moved = false;
        self.pan_begin(g, x, y)
    }

    pub(crate) fn middle_up(&mut self, g: &Geometry) -> bool {
        let held = self.pan_end();
        let Some((x, y)) = self.input_map.middle_origin.take() else {
            return held;
        };
        if self.input_map.middle_moved || !held {
            return held;
        }
        let roll = self.roll_input(g).filter(|ctx| ctx.body.contains(x, y));
        let empty = if let Some(ctx) = roll.as_ref() {
            matches!(
                self.roll.context_spot(ctx, x, y),
                Some(crate::roll::menu::Spot::Grid)
            )
        } else {
            matches!(self.arrange_spot(g, x, y), crate::arrange::Spot::Space(_))
        };
        if !empty {
            return held;
        }
        self.arrange.focus = roll.is_none();
        let names: &[&'static str] = if roll.is_some() {
            &[
                "tool.select",
                "tool.draw",
                "tool.cut",
                "tool.erase",
                "tool.mute",
                "tool.listen",
            ]
        } else {
            &[
                "tool.select",
                "tool.range",
                "tool.cut",
                "tool.erase",
                "tool.draw",
                "tool.mute",
                "tool.listen",
            ]
        };
        let rows = names
            .iter()
            .map(|name| self.tool_item(name).on(tool_of(name) == Some(self.tool)))
            .collect();
        self.open_context(rows, x, y);
        true
    }
}

fn tool_of(name: &str) -> Option<Tool> {
    Some(match name {
        "tool.select" => Tool::Select,
        "tool.range" => Tool::Range,
        "tool.cut" => Tool::Cut,
        "tool.erase" => Tool::Eraser,
        "tool.draw" => Tool::Draw,
        "tool.mute" => Tool::Mute,
        "tool.listen" => Tool::Audition,
        _ => return None,
    })
}
