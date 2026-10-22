//! Track controls use the same selection as the track column and its menu.

use crate::studio::Studio;
use crate::tree::RowKind;
use serde_json::{Value, json};
use xirius_core::id::TrackId;

impl Studio {
    pub fn track_targets(&self, clicked: TrackId) -> Vec<TrackId> {
        let p = self.project();
        let mut ids = vec![clicked];
        if !clicked.is_master() && self.arrange.tracks.contains(&RowKind::Track(clicked)) {
            for row in &self.arrange.tracks {
                match row {
                    RowKind::Track(t) if !t.is_master() => ids.push(*t),
                    RowKind::Folder(f) => ids.extend(p.tracks_in(*f)),
                    _ => {}
                }
            }
        }
        ids.retain(|t| p.tracks.contains_key(t));
        let mut seen = std::collections::HashSet::new();
        ids.retain(|t| seen.insert(*t));
        ids
    }

    pub(crate) fn track_ids(&self, clicked: TrackId) -> Value {
        json!(
            self.track_targets(clicked)
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        )
    }

    pub(crate) fn track_toggle(&mut self, command: &str, clicked: TrackId) -> bool {
        let ids = self.track_ids(clicked);
        self.command(|song| song.run(command, json!({"ids": ids})).map(|_| ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Hit;
    #[test]
    fn grouped_switches_change_every_selected_track_in_one_undo_step() {
        let mut s = Studio::sample();
        let ids: Vec<_> = s
            .project()
            .tracks
            .keys()
            .copied()
            .filter(|t| !t.is_master())
            .take(2)
            .collect();
        s.arrange.tracks = ids.iter().copied().map(RowKind::Track).collect();
        let g = s.geometry(1440.0, 820.0);
        let before = s.song.steps();
        s.press(&g, Hit::Mute(ids[0]));
        assert!(ids.iter().all(|t| s.project().tracks[t].mute));
        assert_eq!(s.song.steps(), before + 1);
        s.undo(&g);
        assert!(ids.iter().all(|t| !s.project().tracks[t].mute));
        s.press(&g, Hit::Solo(ids[0]));
        assert!(ids.iter().all(|t| s.project().tracks[t].solo));
    }

    #[test]
    fn an_unselected_control_targets_only_its_own_track() {
        let mut s = Studio::sample();
        let ids: Vec<_> = s
            .project()
            .tracks
            .keys()
            .copied()
            .filter(|t| !t.is_master())
            .take(3)
            .collect();
        s.arrange.tracks = ids[..2].iter().copied().map(RowKind::Track).collect();
        assert_eq!(s.track_targets(ids[2]), [ids[2]]);
        assert_eq!(s.track_targets(TrackId::MASTER), [TrackId::MASTER]);
    }
}
