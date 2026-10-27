use crate::{
    popup::PopupKind,
    studio::{Geometry, Studio},
};
use serde_json::json;
use std::path::Path;

impl Studio {
    pub fn import_groove(&mut self, path: &Path) -> Result<(), String> {
        if self.transport.is_windowed() {
            let path = path.to_owned();
            let ppq = self.project().ppq;
            let source = self.song.session().uses_id();
            let job = crate::ancillary_io::submit(move || read_groove(&path, ppq))?;
            self.view_read.groove = Some((source, job));
            return Ok(());
        }
        let args = read_groove(path, self.project().ppq)?;
        self.apply_groove(args)
    }

    pub(crate) fn poll_groove_import(&mut self) -> bool {
        if self.input_blocked() {
            return false;
        }
        let Some((source, got)) = self
            .view_read
            .groove
            .as_ref()
            .and_then(|(s, j)| j.poll().map(|r| (*s, r)))
        else {
            return false;
        };
        self.view_read.groove = None;
        if source != self.song.session().uses_id() {
            return false;
        }
        if let Err(e) = got.and_then(|args| self.apply_groove(args)) {
            self.error = Some(e);
        }
        true
    }

    fn apply_groove(&mut self, args: serde_json::Value) -> Result<(), String> {
        self.song.run("view.groove.import", args)?;
        self.quant.groove_on = true;
        self.quant.groove = None;
        self.save_quant_settings();
        self.refresh_popup();
        Ok(())
    }
}

fn read_groove(path: &Path, ppq: u32) -> Result<serde_json::Value, String> {
    #[cfg(any(test, feature = "loading-probe"))]
    xirius_core::io_test::before("groove_read", path).map_err(|e| e.to_string())?;
    // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let notes = crate::smf::read(&bytes, ppq)?;
    let last = notes
        .iter()
        .map(|n| n.at + n.length)
        .max()
        .ok_or("グルーヴにノートがありません")?;
    let bar = i64::from(ppq) * 4;
    let period = (last + bar - 1) / bar * bar;
    let hits: Vec<_> = notes
        .iter()
        .map(|n| json!({"at":n.at,"velocity":n.velocity}))
        .collect();
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(json!({"name":name,"period":period,"hits":hits}))
}

impl Studio {
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
