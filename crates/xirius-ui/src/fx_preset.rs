//! Preset file actions available on every insert, including VST3 inserts.
use crate::draw::{Align, Pen};
use crate::studio::{FileRequest, Rect, Studio};
use serde_json::json;
use std::path::Path;
use xirius_core::id::PluginId;
use xirius_design::{FontSize, Radius, Weight};

#[derive(Clone, Copy, Debug)]
pub struct Panel {
    pub id: PluginId,
    pub rect: Rect,
}

pub fn part(row: Rect) -> Rect {
    Rect::new(row.right() - 32.0, row.y, 16.0, row.h)
}

impl Studio {
    pub fn preset_panel_down(&mut self, x: f32, y: f32) -> bool {
        let Some(panel) = self.mixer.preset_panel.take() else {
            return false;
        };
        if panel.rect.contains(x, y) {
            self.file_request = Some(if y < panel.rect.y + 26.0 {
                FileRequest::PresetOpen(panel.id)
            } else {
                FileRequest::PresetSave(panel.id)
            });
            return true;
        }
        false
    }
    pub fn finish_preset_file(&mut self, request: FileRequest, path: &Path) -> bool {
        let (id, name) = match request {
            FileRequest::PresetOpen(id) => (id, "plugin.preset.open"),
            FileRequest::PresetSave(id) => (id, "plugin.preset.save"),
            _ => return false,
        };
        if name == "plugin.preset.save"
            && let Some(host) = self.transport.host_mut()
            && let Err(e) = self.song.collect_plugin_edits(host)
        {
            self.error = Some(e);
            return true;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            name,
            json!({"id":id.to_string(),"file":path.to_string_lossy()}),
        );
        true
    }
}
pub fn paint(pen: &mut Pen, s: &Studio) {
    let Some(panel) = s.mixer.preset_panel else {
        return;
    };
    let c = pen.c;
    pen.round(panel.rect, c.paper, Radius::M);
    for (i, name) in ["プリセットを読み込む…", "プリセットを保存…"]
        .iter()
        .enumerate()
    {
        let r = Rect::new(
            panel.rect.x + 8.0,
            panel.rect.y + i as f32 * 26.0,
            panel.rect.w - 16.0,
            26.0,
        );
        pen.text(
            name,
            FontSize::Small,
            Weight::Regular,
            c.ink_strong,
            r,
            Align::Left,
        );
    }
}
