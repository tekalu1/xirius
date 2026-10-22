use crate::{
    popup::PopupKind,
    studio::{Geometry, Studio},
};
use serde_json::json;
use std::path::Path;

impl Studio {
    pub fn import_groove(&mut self, path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let notes = crate::smf::read(&bytes, self.project().ppq)?;
        let last = notes
            .iter()
            .map(|n| n.at + n.length)
            .max()
            .ok_or("グルーヴにノートがありません")?;
        let bar = i64::from(self.project().ppq) * 4;
        let period = (last + bar - 1) / bar * bar;
        let hits: Vec<_> = notes
            .iter()
            .map(|n| json!({"at":n.at,"velocity":n.velocity}))
            .collect();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        self.song.run(
            "view.groove.import",
            json!({"name":name,"period":period,"hits":hits}),
        )?;
        self.quant.groove_on = true;
        self.quant.groove = None;
        self.save_quant_settings();
        self.refresh_popup();
        Ok(())
    }

    pub(crate) fn groove_drop(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        paths: &[std::path::PathBuf],
    ) -> bool {
        if !self
            .popup
            .as_ref()
            .is_some_and(|p| p.kind == PopupKind::Quantize && p.layout(g.size).rect.contains(x, y))
        {
            return false;
        }
        if let Some(path) = paths.first()
            && let Err(e) = self.import_groove(path)
        {
            self.error = Some(e);
        }
        true
    }
}
