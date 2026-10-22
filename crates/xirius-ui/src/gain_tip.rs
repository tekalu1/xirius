//! What a normalize raised a clip by (EDT-016): a tip over the middle of each clip it changed,
//! 「+3.1 dB」, for [`SHOWN`]. The gain line moves with it, so nothing else is said.

use std::time::{Duration, Instant};

use serde_json::Value;
use xirius_core::id::ClipId;
use xirius_design::{FontSize, Radius, Space, Weight};

use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::RowKind;

/// How long the tip stays.
pub const SHOWN: Duration = Duration::from_millis(1600);
const H: f32 = 22.0;
/// Above the clip's top.
const LIFT: f32 = 4.0;

#[derive(Clone, Debug, PartialEq)]
pub struct GainTip {
    /// Each clip changed, and by how many dB.
    pub clips: Vec<(ClipId, f64)>,
    pub since: Instant,
}

/// What the tip says for a change of `db`.
pub fn text(db: f64) -> String {
    format!("{db:+.1} dB")
}

impl Studio {
    /// A command ran from the screen: a normalize leaves its tip.
    pub(crate) fn after_command(&mut self, command: &str, data: &Value) {
        self.after_file_command(command, data);
        if matches!(command, "prefs.set" | "prefs.reset") {
            self.apply_edit_prefs();
            return;
        }
        if command == "quantize.program" {
            // The panel shows what the program set.
            self.choose_editor();
            return;
        }
        if command != "clip.normalize" {
            return;
        }
        let clips = data["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| Some((c["id"].as_str()?.parse().ok()?, c["db"].as_f64()?)))
            .collect();
        self.gain_tip = Some(GainTip {
            clips,
            since: Instant::now(),
        });
    }

    /// The tip, while it shows.
    pub fn gain_tip_shown(&self) -> Option<&GainTip> {
        self.gain_tip.as_ref().filter(|t| t.since.elapsed() < SHOWN)
    }

    /// Lets go of the tip shown long enough. True when it went.
    pub fn expire_gain_tip(&mut self) -> bool {
        if self.gain_tip.is_some() && self.gain_tip_shown().is_none() {
            self.gain_tip = None;
            return true;
        }
        false
    }

    /// Where the tip of each clip is, with what it says.
    pub fn gain_tip_rects(&self, g: &Geometry) -> Vec<(Rect, String)> {
        let Some(tip) = self.gain_tip_shown() else {
            return Vec::new();
        };
        let p = self.project();
        let tree = self.tree();
        let mut out = Vec::new();
        for (id, db) in &tip.clips {
            let Ok((t, clip)) = p.clip(*id) else {
                continue;
            };
            let Some(row) = tree.row_of(RowKind::Track(t.id)) else {
                continue;
            };
            let r = self.clip_rect(g, row, clip);
            let said = text(*db);
            let w = crate::browser::text_width(&said, FontSize::Caption) + Space::X2.px() * 2.0;
            let x =
                (r.x + r.w / 2.0 - w / 2.0).clamp(g.lanes.x, (g.lanes.right() - w).max(g.lanes.x));
            let y = (r.y - H - LIFT).max(g.lanes.y);
            out.push((Rect::new(x, y, w, H), said));
        }
        out
    }
}

pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    for (r, said) in studio.gain_tip_rects(g) {
        pen.floating(r, pen.c.surface_3, Radius::S);
        pen.digits(
            &said,
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink_strong,
            r,
            Align::Center,
        );
    }
}
