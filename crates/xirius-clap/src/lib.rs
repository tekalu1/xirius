//! CLAP plugin hosting on clack (PLG-028): bundles, one instance with its own main thread, and
//! the real-time half that the engine runs. The shape follows `xirius-vst3`, so the engine, the
//! session and the plugin host process treat both formats alike.
//!
//! - [`Bundle`] loads a `.clap` file and lists its plugins ([`Descriptor`]).
//! - [`Plugin`] is one instance. CLAP's `[main-thread]` calls all run on one thread the
//!   instance owns, so the handle can be used from any thread.
//! - [`AudioPart`] activates the plugin and calls `process` with preallocated buffers and events.
//!
//! A song keeps a CLAP plugin by its CLAP id. Where 16 bytes are expected (the plugin host's
//! wire format), [`class_key`] stands for the id.

mod audio;
mod bundle;
mod host;
mod plugin;

use std::path::{Path, PathBuf};

pub use audio::AudioPart;
pub use bundle::{Bundle, Descriptor};
pub use plugin::{Gesture, ParamInfo, Plugin};

/// The controller number of channel aftertouch, and of pitch bend (the numbers below 128 are
/// the MIDI CCs), as in `xirius-vst3`.
pub const CTRL_AFTERTOUCH: u8 = 128;
pub const CTRL_PITCH_BEND: u8 = 129;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub(crate) fn error(msg: impl Into<String>) -> Error {
    Error(msg.into())
}

/// A note for the plugin's first note input, at a frame offset within the block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    pub offset: u32,
    pub pitch: u8,
    /// 0..=1. Ignored for note-offs.
    pub velocity: f32,
    pub on: bool,
}

/// A controller change: a MIDI CC (`number` 0..=127, `value` 0..=127), channel aftertouch
/// ([`CTRL_AFTERTOUCH`], 0..=127) or pitch bend ([`CTRL_PITCH_BEND`], -8192..=8191). It reaches
/// the plugin as MIDI, so only plugins whose note input takes MIDI get it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    pub offset: u32,
    pub number: u8,
    pub value: i32,
}

/// A parameter value at a frame offset in the block, normalized to 0..1 over the parameter's
/// range like a VST3 value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamChange {
    pub offset: u32,
    pub id: u32,
    pub value: f64,
}

/// 16 bytes standing for a CLAP id: FNV-1a with 128 bits. The plugin host's wire format names
/// a plugin class with 16 bytes (a VST3 class id); for CLAP it carries this instead.
pub fn class_key(id: &str) -> [u8; 16] {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013B;
    let mut hash = OFFSET;
    for b in id.as_bytes() {
        hash ^= u128::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash.to_be_bytes()
}

/// Whether `path` names a CLAP bundle (by its extension).
pub fn is_clap(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("clap"))
}

/// The folders the CLAP spec lists for this OS, then those in `CLAP_PATH`.
pub fn default_clap_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = std::env::var_os("CLAP_PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    #[cfg(windows)]
    {
        let common = std::env::var_os("COMMONPROGRAMFILES")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
        dirs.push(common.join("CLAP"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Programs")
                    .join("Common")
                    .join("CLAP"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".clap"));
        }
        dirs.push(PathBuf::from("/usr/lib/clap"));
    }
    dirs
}

/// The `.clap` bundles under `dir`, searched recursively, sorted by path.
pub fn find_bundles(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if is_clap(&p) {
                found.push(p);
            } else if p.is_dir() {
                stack.push(p);
            }
        }
    }
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_keys_differ_and_stay_put() {
        let a = class_key("com.u-he.diva");
        assert_eq!(a, class_key("com.u-he.diva"));
        assert_ne!(a, class_key("com.u-he.Diva"));
        assert_ne!(class_key(""), [0; 16]);
    }

    #[test]
    fn clap_paths_are_known_by_extension() {
        assert!(is_clap(Path::new("C:/x/Vital.clap")));
        assert!(is_clap(Path::new("C:/x/Vital.CLAP")));
        assert!(!is_clap(Path::new("C:/x/Vital.vst3")));
    }
}
