//! A loaded `.clap` file and the plugins it offers.

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use clack_host::prelude::PluginEntry;

use crate::{Error, class_key, error};

/// One plugin a bundle offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub features: Vec<String>,
}

impl Descriptor {
    pub fn is_instrument(&self) -> bool {
        self.features.iter().any(|f| f == "instrument")
    }

    pub fn is_effect(&self) -> bool {
        self.features
            .iter()
            .any(|f| f == "audio-effect" || f == "analyzer")
    }

    pub fn key(&self) -> [u8; 16] {
        class_key(&self.id)
    }
}

fn text(s: Option<&CStr>) -> String {
    s.map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A `.clap` file with its entry initialized. Entries stay initialized until process exit:
/// callbacks can remain after the last bundle/instance, as with VST3 (ARC-043).
#[derive(Clone)]
pub struct Bundle {
    path: PathBuf,
    pub(crate) entry: PluginEntry,
    descriptors: Vec<Descriptor>,
}

// One owner per binary, independent of bundle and instance lifetimes. Clack shares and
// serializes entry initialization; retaining its handle also prevents entry deinit/unload.
struct Resident {
    key: PathBuf,
    entry: Result<PluginEntry, String>,
}
static RESIDENT: Mutex<Vec<Resident>> = Mutex::new(Vec::new());

#[cfg(windows)]
fn retain_binary(path: &Path) -> Result<(), Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a NUL-terminated DLL path. Keep this reference even if CLAP entry init fails:
    // Clack's failure cleanup may unload its own reference while a DLL callback remains.
    if unsafe { LoadLibraryW(wide.as_ptr()) }.is_null() {
        return Err(error(format!(
            "{}: {}",
            path.display(),
            std::io::Error::last_os_error()
        )));
    }
    Ok(())
}

impl Bundle {
    /// Loads the library and calls its entry's `init`. This runs the plugin's code.
    pub fn load(path: &Path) -> Result<Bundle, Error> {
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let entry = {
            let mut resident = RESIDENT.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(entry) = resident.iter().find(|entry| entry.key == key) {
                entry.entry.clone().map_err(error)?
            } else {
                #[cfg(windows)]
                retain_binary(path)?;
                // SAFETY: loading runs plugin code; the caller chose to trust this file.
                let entry = unsafe { PluginEntry::load(path) }
                    .map_err(|e| format!("{}: {e}", path.display()));
                resident.push(Resident {
                    key,
                    entry: entry.clone(),
                });
                entry.map_err(error)?
            }
        };
        let factory = entry
            .get_plugin_factory()
            .ok_or_else(|| error(format!("{}: no plugin factory", path.display())))?;
        let descriptors = factory
            .plugin_descriptors()
            .filter_map(|d| {
                let id = d.id()?.to_str().ok()?.to_string();
                Some(Descriptor {
                    id,
                    name: text(d.name()),
                    vendor: text(d.vendor()),
                    version: text(d.version()),
                    features: d
                        .features()
                        .map(|f| f.to_string_lossy().into_owned())
                        .collect(),
                })
            })
            .collect();
        Ok(Bundle {
            path: path.to_path_buf(),
            entry,
            descriptors,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn descriptors(&self) -> &[Descriptor] {
        &self.descriptors
    }

    pub fn find(&self, id: &str) -> Option<&Descriptor> {
        self.descriptors.iter().find(|d| d.id == id)
    }

    /// The plugin whose [`class_key`] is `key`, or the first when `key` is `None`.
    pub fn find_key(&self, key: Option<[u8; 16]>) -> Option<&Descriptor> {
        match key {
            Some(k) => self.descriptors.iter().find(|d| d.key() == k),
            None => self.descriptors.first(),
        }
    }
}
