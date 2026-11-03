//! The plugin's archive as a song keeps it (PLG-037, ARC-025).
//!
//! The bytes are the plugin's own; the host adds a version of this wrapper, the plugin it
//! belongs to and the archive id the plugin gave (`documentArchiveID`), so a song opened by a
//! later Xirius or with another version of the plugin can tell whether it may restore it.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

/// The version of this wrapper. A higher one is not read.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Archive {
    pub version: u32,
    /// The catalog id of the plugin.
    pub plugin: String,
    /// The plugin's archive id, for its compatible ids on restore.
    pub archive_id: String,
    /// The plugin's bytes, in base64.
    pub data: String,
}

impl Archive {
    pub fn new(plugin: &str, archive_id: &str, bytes: &[u8]) -> Archive {
        Archive {
            version: VERSION,
            plugin: plugin.to_string(),
            archive_id: archive_id.to_string(),
            data: STANDARD.encode(bytes),
        }
    }

    /// The plugin's bytes, or `None` when this wrapper is newer than this build or broken.
    pub fn bytes(&self) -> Option<Vec<u8>> {
        if self.version == 0 || self.version > VERSION {
            return None;
        }
        STANDARD.decode(&self.data).ok()
    }

    /// Whether a factory with `archive_id` and `compatible` ids can restore this archive.
    pub fn readable_by(&self, archive_id: &str, compatible: &[String]) -> bool {
        self.archive_id == archive_id || compatible.contains(&self.archive_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_keeps_its_bytes_and_says_its_version() {
        let a = Archive::new("vst3:abc", "com.example.doc", &[0, 1, 2, 255]);
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"version\":1"));
        let b: Archive = serde_json::from_str(&json).unwrap();
        assert_eq!(b.bytes().unwrap(), vec![0, 1, 2, 255]);
        assert!(b.readable_by("com.example.doc", &[]));
        assert!(b.readable_by("com.example.doc2", &["com.example.doc".into()]));
        assert!(!b.readable_by("com.other", &[]));
    }

    #[test]
    fn a_newer_archive_is_not_read() {
        let mut a = Archive::new("p", "id", b"x");
        a.version = VERSION + 1;
        assert_eq!(a.bytes(), None);
    }
}
