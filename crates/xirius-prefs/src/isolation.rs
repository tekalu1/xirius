//! Stable plugin process preferences (PLG-022).
use crate::Prefs;
use xirius_core::model::PluginIsolation;

/// UTF-8 bytes encoded as hex avoid TOML path separators and preserve CLAP's case-sensitive IDs.
pub fn key(format: &str, uid: &str) -> String {
    let uid = if format.eq_ignore_ascii_case("vst3") {
        uid.to_ascii_uppercase()
    } else {
        uid.to_owned()
    };
    let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
    format!("plugins.isolation.{}:{hex}", format.to_ascii_lowercase())
}

pub fn resolve(
    prefs: &Prefs,
    format: &str,
    uid: &str,
    fallback: PluginIsolation,
) -> PluginIsolation {
    let key = key(format, uid);
    if !prefs.values.contains_key(&key) {
        return fallback;
    }
    match prefs.text(&key).as_str() {
        "same" | "shared" | "song" | "inprocess" | "in_process" | "default" => {
            PluginIsolation::Shared
        }
        "dedicated" => PluginIsolation::Dedicated,
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_identity_preserves_clap_case_and_reads_old_mode_as_default() {
        assert_eq!(key("VST3", "abcd"), key("vst3", "ABCD"));
        assert_ne!(key("clap", "Abcd"), key("clap", "abcd"));
        assert_ne!(key("vst3", "abcd"), key("clap", "abcd"));
        let mut prefs = Prefs::default();
        let k = key("vst3", "abcd");
        prefs.set(&k, &serde_json::json!("same")).unwrap();
        assert_eq!(
            resolve(&prefs, "vst3", "abcd", PluginIsolation::Dedicated),
            PluginIsolation::Shared
        );
        let mut problems = vec![];
        // An old mode survives persistence as the default placement.
        let table = toml::from_str(&prefs.to_toml()).unwrap();
        let restored = Prefs::from_toml(&table, &mut problems);
        assert!(problems.is_empty());
        assert_eq!(
            resolve(&restored, "vst3", "abcd", PluginIsolation::Dedicated),
            PluginIsolation::Shared
        );
    }

    #[test]
    fn missing_setting_uses_song_choice_but_legacy_song_means_default() {
        let prefs = Prefs::default();
        assert_eq!(
            resolve(&prefs, "clap", "fixture", PluginIsolation::Dedicated),
            PluginIsolation::Dedicated
        );
        let mut prefs = Prefs::default();
        prefs
            .set(&key("clap", "fixture"), &serde_json::json!("song"))
            .unwrap();
        assert!(prefs.values.contains_key(&key("clap", "fixture")));
        assert_eq!(
            resolve(&prefs, "clap", "fixture", PluginIsolation::Dedicated),
            PluginIsolation::Shared
        );
    }
}
