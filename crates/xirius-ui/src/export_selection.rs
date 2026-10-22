//! The arrange selection supplies the export panel's selected range.
use crate::{
    export::Range,
    popup::PopupKind,
    studio::{Geometry, Studio},
};
use xirius_core::id::ClipId;

impl Studio {
    pub(crate) fn export_clip_ids(&self) -> Vec<ClipId> {
        self.arrange.clips.iter().map(|(_, id)| *id).collect()
    }

    pub(crate) fn open_selection_export(&mut self, g: &Geometry) -> bool {
        if self.export_clip_ids().is_empty() {
            return false;
        }
        if !self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::Export)
        {
            self.open_export(self.export_anchor(g));
        }
        self.export.range = Range::Selection;
        self.export.fixed_span = None;
        self.refresh_popup();
        true
    }
}
