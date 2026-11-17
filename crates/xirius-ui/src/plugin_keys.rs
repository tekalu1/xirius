use crate::studio::Studio;

impl Studio {
    pub(crate) fn poll_editor_keys(&mut self) -> bool {
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
        let requests = remote;
        let mut changed = false;
        for _ in 0..requests {
            changed |= self.toggle_play();
        }
        changed
    }
}
