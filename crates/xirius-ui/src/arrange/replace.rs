use super::*;

impl Studio {
    pub(crate) fn replace_file_up(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(d) = self.browser.file_drag.as_ref() else {
            return false;
        };
        if !d.moved || !crate::drop_audio::is_audio(&d.path) {
            return false;
        }
        let spot = self.arrange_spot(g, x, y);
        let (track, clip) = match spot {
            Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } => (track, clip),
            _ => return false,
        };
        if self.project().tracks[&track].kind != TrackKind::Audio {
            return false;
        }
        let file = d.path.to_string_lossy().to_string();
        self.browser.file_drag = None;
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run("clip.replace", json!({"id":clip.to_string(),"file":file}));
        true
    }
}
