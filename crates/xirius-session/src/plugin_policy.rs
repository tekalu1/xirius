//! Legacy process preferences and catalogue lookup (PLG-078).
use xirius_core::model::{Plugin, PluginIsolation};
use xirius_plugins::{Catalog, Entry};
use xirius_prefs::Prefs;

pub fn legacy_override(
    prefs: &Prefs,
    catalog: Option<&Catalog>,
    x: &Plugin,
) -> Option<PluginIsolation> {
    let unique =
        catalog.is_some_and(|c| c.entries.iter().filter(|e| e.name == x.name).count() == 1);
    if unique {
        let key = format!("plugins.process.{}", x.name);
        if !prefs.values.contains_key(&key) {
            return None;
        }
        match prefs.text(&key).as_str() {
            "same" | "shared" | "song" | "inprocess" | "in_process" | "default" => {
                return Some(PluginIsolation::Shared);
            }
            "dedicated" => return Some(PluginIsolation::Dedicated),
            _ => {}
        }
    }
    None
}
pub fn legacy(prefs: &Prefs, catalog: Option<&Catalog>, x: &Plugin) -> PluginIsolation {
    legacy_override(prefs, catalog, x).unwrap_or(x.isolation)
}
/// A format and class pair, preserving CLAP's case-sensitive ID.
pub fn find<'a>(catalog: &'a Catalog, plugin: &Plugin) -> Option<&'a Entry> {
    catalog.entries.iter().find(|e| {
        e.format.eq_ignore_ascii_case(&plugin.format)
            && if plugin.format.eq_ignore_ascii_case("vst3") {
                e.uid.eq_ignore_ascii_case(&plugin.uid)
            } else {
                e.uid == plugin.uid
            }
    })
}
