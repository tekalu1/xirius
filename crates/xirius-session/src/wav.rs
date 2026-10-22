//! Reading an audio file into the engine's format: stereo f32 at the engine's sample rate.
//!
//! [`crate::decode`] reads the file (WAV, AIFF, FLAC, MP3, Ogg Vorbis) and converts its rate
//! with rubato, the one conversion there is (ENG-022). The result is cached per path and rate,
//! and read again when the file changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use xirius_engine::AudioData;

/// Decodes `path` and converts it to `rate`. The error is a sentence for the user.
pub fn read_wav(path: &Path, rate: u32) -> Result<AudioData, String> {
    crate::decode::read(path, rate)
}

#[derive(Default)]
pub struct WavCache {
    map: HashMap<(PathBuf, u32), (Stamp, Arc<AudioData>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl WavCache {
    pub fn load(&mut self, path: &Path, rate: u32) -> Result<Arc<AudioData>, String> {
        let stamp = std::fs::metadata(path)
            .map(|m| Stamp {
                len: m.len(),
                modified: m.modified().ok(),
            })
            .map_err(|e| format!("{} を開けません: {e}", path.display()))?;
        let key = (path.to_path_buf(), rate);
        if let Some((s, data)) = self.map.get(&key)
            && *s == stamp
        {
            return Ok(data.clone());
        }
        let data = Arc::new(read_wav(path, rate)?);
        self.map.insert(key, (stamp, data.clone()));
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn something_that_is_not_audio_is_refused_with_the_formats_named() {
        let e = read_wav(Path::new("a.txt"), 48_000).unwrap_err();
        assert!(e.contains("WAV・AIFF・FLAC"), "{e}");
    }
}
