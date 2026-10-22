//! The scan results kept between runs, one record per bundle with a fingerprint of its files.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::{Catalog, Entry, Failure};

/// 2: entries say whether they use ARA, so the older records are scanned again.
const VERSION: u32 = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CacheFile {
    pub version: u32,
    pub bundles: Vec<BundleRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BundleRecord {
    pub path: PathBuf,
    /// [`fingerprint`] when it was opened.
    pub fingerprint: String,
    #[serde(default)]
    pub entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
    /// How long opening it took.
    #[serde(default)]
    pub probe_ms: u64,
}

impl CacheFile {
    /// The file's contents; empty when it is missing, unreadable or of another version.
    pub fn read(path: &Path) -> CacheFile {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<CacheFile>(&b).ok())
            .filter(|c| c.version == VERSION)
            .unwrap_or_default()
    }

    /// Whether the file is there and of this version; otherwise the plugins are scanned again.
    pub fn is_current(path: &Path) -> bool {
        #[derive(Deserialize)]
        struct Head {
            version: u32,
        }
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Head>(&b).ok())
            .is_some_and(|h| h.version == VERSION)
    }

    /// Writes through a temporary file, so a reader never sees half of it.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut c = self.clone();
        c.version = VERSION;
        let text = serde_json::to_vec_pretty(&c).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    pub fn get(&self, path: &Path) -> Option<&BundleRecord> {
        self.bundles.iter().find(|b| b.path == path)
    }

    pub fn catalog(&self) -> Catalog {
        Catalog {
            entries: self
                .bundles
                .iter()
                .flat_map(|b| b.entries.iter().cloned())
                .collect(),
            failures: self
                .bundles
                .iter()
                .filter_map(|b| b.failure.clone())
                .collect(),
        }
    }
}

/// A summary of the files that make up a bundle: their names, sizes and modification times.
/// For a folder bundle only the Windows binaries (`Contents/x86_64-win`) count, so large
/// resource folders are not walked.
pub fn fingerprint(bundle: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    if bundle.is_file() {
        parts.push(stamp(bundle, Path::new("")));
    } else {
        let bin = bundle.join("Contents").join(BINARY_DIR);
        if bin.is_dir() {
            let mut stack = vec![bin.clone()];
            while let Some(d) = stack.pop() {
                let Ok(rd) = std::fs::read_dir(&d) else {
                    continue;
                };
                for e in rd.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        parts.push(stamp(&p, &bin));
                    }
                }
            }
        } else {
            parts.push(format!("no-binary {}", stamp(bundle, Path::new(""))));
        }
    }
    parts.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in parts.join("\n").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(windows)]
const BINARY_DIR: &str = "x86_64-win";
#[cfg(target_os = "macos")]
const BINARY_DIR: &str = "MacOS";
#[cfg(all(unix, not(target_os = "macos")))]
const BINARY_DIR: &str = "x86_64-linux";

fn stamp(p: &Path, base: &Path) -> String {
    let rel = p.strip_prefix(base).unwrap_or(p).to_string_lossy();
    match std::fs::metadata(p) {
        Ok(m) => {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{rel}|{}|{t}", m.len())
        }
        Err(_) => format!("{rel}|missing"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_of_another_version_is_not_current() {
        let dir_tmp = tempfile::Builder::new()
            .prefix("xirius-cache-")
            .tempdir()
            .unwrap();
        let dir = dir_tmp.path().join("cache");
        let path = dir.join("plugins.json");
        assert!(!CacheFile::is_current(&path), "missing");
        CacheFile::default().write(&path).unwrap();
        assert!(CacheFile::is_current(&path));
        std::fs::write(&path, br#"{ "version": 1, "bundles": [] }"#).unwrap();
        assert!(
            !CacheFile::is_current(&path),
            "before entries said whether they use ARA"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
