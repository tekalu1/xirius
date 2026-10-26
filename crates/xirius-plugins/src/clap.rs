//! CLAP bundles in the list (PLG-028). A `.clap` file is opened in the scan process like a
//! VST3 bundle; each plugin it offers becomes an entry whose `uid` is the plugin's CLAP id
//! (`com.vendor.name`), the name a song keeps.

use std::path::Path;

use xirius_core::model::PluginKind;

use crate::Entry;

/// Opens the `.clap` file in this process and lists its plugins.
pub(crate) fn probe(bundle: &Path) -> Result<Vec<Entry>, String> {
    let b = xirius_clap::Bundle::load(bundle).map_err(|e| e.to_string())?;
    let entries = b
        .descriptors()
        .iter()
        .map(|d| Entry {
            uid: d.id.clone(),
            name: d.name.clone(),
            vendor: d.vendor.clone(),
            kind: if d.is_instrument() {
                PluginKind::Instrument
            } else {
                PluginKind::Effect
            },
            format: "clap".into(),
            category: d.features.join("|"),
            version: d.version.clone(),
            path: bundle.to_path_buf(),
            // ARA's CLAP binding is not looked for yet (docs/plugin-hosting.md).
            ara: false,
            requires_process_main: false,
        })
        .collect();
    // As with VST3: the answer is what matters and the process ends without unloading.
    std::mem::forget(b);
    Ok(entries)
}
