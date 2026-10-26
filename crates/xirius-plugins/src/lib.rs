//! The plugins installed on this machine (PLG-001, PLG-003, PLG-021, BRW-001).
//!
//! - [`scan`] looks for VST3 bundles and CLAP files (PLG-028) in the standard places and opens each one in its own
//!   short-lived process ([`child_main`]), so a plugin that crashes or hangs while loading only
//!   loses its own entry. The process runs on a desktop nobody sees, so a licence dialog does
//!   not come up in front of the user; it times out instead.
//! - The results are kept in a cache file. The next scan opens only the bundles whose files
//!   changed.
//! - [`Catalog`] is what the browser and the session read: the entries with their kind and
//!   vendor, and the bundles that failed.
//! - [`encode_state`] / [`decode_state`] turn a plugin's saved state into the text a song keeps.
//! - [`Entry::ara`] marks the plugins that use ARA ([`ara`], PLG-025); they always run in the
//!   song's process.

pub mod ara;
mod cache;
mod child;
mod clap;
mod scan;
mod spawn;
mod state;
pub mod trial;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use xirius_core::model::PluginKind;

pub use cache::{BundleRecord, CacheFile, fingerprint};
pub use child::{CHILD_ARG, child_main, probe};
pub use scan::{ChildProgram, ScanOptions, ScanReport, scan, scan_seeing, scan_until, scan_with};
pub use state::{decode_state, encode_state};

/// One plugin class a bundle offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The class id, 32 hex digits. A song refers to the plugin by it (`plugin.add --uid`).
    pub uid: String,
    pub name: String,
    pub vendor: String,
    pub kind: PluginKind,
    /// `vst3` or `clap`.
    pub format: String,
    /// The VST3 sub-categories, e.g. `Fx|Dynamics`.
    pub category: String,
    pub version: String,
    /// The bundle.
    pub path: PathBuf,
    /// It uses ARA, so it runs in the song's process only (PLG-025).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ara: bool,
    /// A known compatibility requirement. Unknown plugins are tried on a local
    /// owner; this declaration never changes the song's requested isolation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_process_main: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The process ended without an answer (an access violation, an abort).
    Crashed,
    /// It did not answer in time and was stopped (a dialog waiting for a click, a hang).
    Timeout,
    /// The bundle opened but could not be read (no binary, no factory).
    Failed,
}

/// A bundle that gave no plugins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub path: PathBuf,
    pub kind: FailureKind,
    pub error: String,
}

/// The installed plugins as of the last scan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub failures: Vec<Failure>,
}

impl Catalog {
    /// The catalogue kept in a cache file; empty when the file is missing or unreadable.
    pub fn load(cache: &Path) -> Catalog {
        CacheFile::read(cache).catalog()
    }

    /// The catalogue in the standard cache file.
    pub fn load_standard() -> Catalog {
        Catalog::load(&default_cache_path())
    }

    pub fn find(&self, uid: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.uid.eq_ignore_ascii_case(uid))
    }

    /// Entries whose name, vendor or category contain `query` (case-insensitive), of `kind`
    /// when given, ordered by name.
    pub fn search(&self, query: &str, kind: Option<PluginKind>) -> Vec<&Entry> {
        let q = query.to_lowercase();
        let mut out: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .filter(|e| {
                q.is_empty()
                    || e.name.to_lowercase().contains(&q)
                    || e.vendor.to_lowercase().contains(&q)
                    || e.category.to_lowercase().contains(&q)
            })
            .collect();
        out.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.vendor.cmp(&b.vendor))
        });
        out
    }
}

/// A class id as the 32 hex digits a song keeps.
pub fn uid_hex(cid: &[u8; 16]) -> String {
    cid.iter().map(|b| format!("{b:02X}")).collect()
}

/// Where VST3 bundles are installed on this OS, then the folders in `XIRIUS_VST3_PATH`
/// (separated like `PATH`). Folders that do not exist are left in; the scan skips them.
pub fn standard_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        let common = std::env::var_os("CommonProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
        dirs.push(common.join("VST3"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join(r"Programs\Common\VST3"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Library/Audio/Plug-Ins/VST3"));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/VST3"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".vst3"));
        }
        dirs.push(PathBuf::from("/usr/lib/vst3"));
        dirs.push(PathBuf::from("/usr/local/lib/vst3"));
    }
    if let Some(extra) = std::env::var_os("XIRIUS_VST3_PATH") {
        dirs.extend(std::env::split_paths(&extra));
    }
    // The CLAP folders (and `CLAP_PATH`); the scan finds `.clap` files in every folder.
    for d in xirius_clap::default_clap_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// The cache file: `XIRIUS_PLUGIN_CACHE`, otherwise the user's cache folder
/// (`%LOCALAPPDATA%\Xirius\plugins.json` on Windows).
pub fn default_cache_path() -> PathBuf {
    if let Some(p) = std::env::var_os("XIRIUS_PLUGIN_CACHE") {
        return PathBuf::from(p);
    }
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Xirius"))
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library/Caches/Xirius"))
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|p| p.join("xirius"))
        .unwrap_or_else(std::env::temp_dir);
    base.join("plugins.json")
}
