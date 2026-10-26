use crate::{mixer::editors::Where, studio::Studio};

impl Studio {
    pub(crate) fn poll_editor_keys(&mut self) -> bool {
        let local: u32 = self
            .mixer
            .editors
            .iter()
            .map(|e| match &e.at {
                Where::Local { editor, .. } | Where::Apart { editor, .. } => {
                    editor.take_play_requests()
                }
                Where::Host { .. } => 0,
            })
            .sum();
        let remote: u32 = self
            .transport
            .host()
            .map(|h| {
                h.plugins()
                    .reports()
                    .iter()
                    .filter_map(|r| h.plugins().playing_link(r.id))
                    .map(|link| link.take_play_requests())
                    .sum()
            })
            .unwrap_or(0);
        let owned = self
            .transport
            .host()
            .map_or(0, |h| h.plugins().take_local_play_requests());
        let requests = local.saturating_add(remote).saturating_add(owned);
        let mut changed = false;
        for _ in 0..requests {
            changed |= self.toggle_play();
        }
        changed
    }
}
