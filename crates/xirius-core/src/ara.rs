//! Audio clips edited in an ARA plugin (EDT-023, PLG-032, PLG-037, ARC-025).
//!
//! Kept in a table of the project beside the tracks, keyed by clip, like `clipfx`: a song with
//! no ARA clip has nothing of it in its file. The plugin's own edits are not the song's: they
//! live in the plugin's archive, one per plugin, which the session asks the plugin for and
//! stores here as it is (base64 of the plugin's bytes, with the version of this wrapper and the
//! plugin's archive id).
//!
//! * A clip in the table plays through the plugin's region of it. The clip itself stays as it
//!   was (its file, offset and gain), so taking it out of the table plays the file again.
//! * `clone_of` is the clip a copy came from: the plugin clones that clip's modification, with
//!   its edits, when it first sees the copy. Once the archive has the copy's own modification,
//!   restoring it gives the copy its own edits back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::id::ClipId;
use crate::model::Project;

/// The version of the archive wrapper this build writes and reads up to.
pub const ARCHIVE_VERSION: u32 = 2;

#[derive(Clone, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Ara {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub clips: BTreeMap<ClipId, AraClip>,
    /// The plugins' archives, by the plugin's catalog id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub archives: BTreeMap<String, Archive>,
}

impl Ara {
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.archives.is_empty()
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct AraClip {
    /// The plugin's catalog id.
    pub plugin: String,
    /// The plugin's name when the clip was attached, for a song opened where it is missing.
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clone_of: Option<ClipId>,
}

/// A plugin's archive (the shape of `xirius_ara::archive::Archive`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Archive {
    pub version: u32,
    pub plugin: String,
    pub archive_id: String,
    /// Archive payload in base64, including the host's source-fingerprint envelope in v2.
    pub data: String,
}

impl Project {
    /// The row of `from` for a clip that is a copy of it: the copy plays through the same plugin,
    /// starting from a clone of the original's edits.
    pub fn copy_ara(&mut self, from: ClipId, to: ClipId) {
        if let Some(a) = self.ara.clips.get(&from) {
            let row = AraClip {
                clone_of: Some(from),
                ..a.clone()
            };
            self.ara.clips.insert(to, row);
        }
    }

    /// Drops the rows of clips that are gone or no longer audio, the copies' links to them, and
    /// the archives of plugins no clip uses. The session calls it after every edit.
    pub fn prune_ara(&mut self) {
        if self.ara.is_empty() {
            return;
        }
        let audio: std::collections::HashSet<ClipId> = self
            .tracks
            .values()
            .flat_map(|t| t.clips.iter())
            .filter(|c| matches!(c.body, crate::model::ClipBody::Audio { .. }))
            .map(|c| c.id)
            .collect();
        self.ara.clips.retain(|id, _| audio.contains(id));
        for row in self.ara.clips.values_mut() {
            if row.clone_of.is_some_and(|c| !audio.contains(&c)) {
                row.clone_of = None;
            }
        }
        let used: std::collections::HashSet<&str> =
            self.ara.clips.values().map(|r| r.plugin.as_str()).collect();
        let gone: Vec<String> = self
            .ara
            .archives
            .keys()
            .filter(|k| !used.contains(k.as_str()))
            .cloned()
            .collect();
        for k in gone {
            self.ara.archives.remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::TrackId;
    use crate::model::{AudioProcess, Clip, ClipBody, Track, TrackKind};
    use std::sync::Arc;

    fn song() -> Project {
        let mut p = Project::new("song");
        let mut t = Track::new(TrackId(100), TrackKind::Audio, "a");
        for id in [1, 2] {
            t.clips.push(Clip {
                id: ClipId(id),
                name: String::new(),
                color: None,
                start: 0,
                length: 960,
                muted: false,
                take: None,
                controllers: Vec::new(),
                body: ClipBody::Audio {
                    file: "a.wav".into(),
                    offset: 0,
                    gain_db: 0.0,
                    process: AudioProcess::default(),
                },
            });
        }
        p.tracks.insert(t.id, Arc::new(t));
        p
    }

    #[test]
    fn a_copy_clones_and_pruning_forgets() {
        let mut p = song();
        p.ara.clips.insert(
            ClipId(1),
            AraClip {
                plugin: "vst3:m".into(),
                name: "M".into(),
                clone_of: None,
            },
        );
        p.ara.archives.insert(
            "vst3:m".into(),
            Archive {
                version: ARCHIVE_VERSION,
                plugin: "vst3:m".into(),
                archive_id: "doc".into(),
                data: "AA==".into(),
            },
        );
        p.copy_ara(ClipId(1), ClipId(2));
        assert_eq!(p.ara.clips[&ClipId(2)].clone_of, Some(ClipId(1)));
        p.prune_ara();
        assert_eq!(p.ara.clips.len(), 2);

        Arc::make_mut(p.tracks.get_mut(&TrackId(100)).unwrap())
            .clips
            .retain(|c| c.id != ClipId(1));
        p.prune_ara();
        assert_eq!(p.ara.clips[&ClipId(2)].clone_of, None);
        assert!(p.ara.archives.contains_key("vst3:m"));

        p.ara.clips.clear();
        p.prune_ara();
        assert!(p.ara.is_empty());
    }

    #[test]
    fn a_song_without_ara_writes_nothing_of_it() {
        let p = song();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("\"ara\""));
        let back: Project = serde_json::from_str(&json).unwrap();
        assert!(back.ara.is_empty());
    }
}
