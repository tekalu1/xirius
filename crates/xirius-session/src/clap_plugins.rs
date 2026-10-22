//! CLAP plugins a song names (PLG-028), next to the VST3 ones of [`crate::plugins`].
//!
//! The song keeps a CLAP plugin like a VST3 one: `format` is `clap`, `uid` its CLAP id, the
//! state as text ([`xirius_plugins::encode_state`], with the plugin's bytes as the component
//! part) and the parameters by numeric id or title. In a host process it is opened through
//! the same messages as a VST3 plugin, so it gets the same isolation, crash handling and
//! restart. In this process it plays without its editor: the song's changes to its values
//! take a new mix.

use std::path::Path;

use xirius_core::model::Plugin;
use xirius_engine::Processor;
use xirius_engine::clap_plugin::ClapProcessor;
use xirius_vst3::PluginState;

/// Whether the song's plugin is a CLAP one.
pub fn is_clap(x: &Plugin) -> bool {
    x.format.eq_ignore_ascii_case("clap")
}

/// Whether the song's plugin is one [`crate::plugins::PluginLoader`] opens (VST3 or CLAP).
pub fn is_hosted(x: &Plugin) -> bool {
    crate::plugins::is_vst3(x) || is_clap(x)
}

/// The 16 bytes that name the plugin in a host process.
pub(crate) fn class_key(x: &Plugin) -> [u8; 16] {
    xirius_clap::class_key(&x.uid)
}

/// The names a plugin gives its keys.
pub(crate) type NoteNames = Vec<(u8, String)>;

/// The plugin in this process, with the song's state and parameters applied, and the names it
/// gives its keys. Parameters it does not have go to `warnings`.
pub(crate) fn open_in_process(
    path: &Path,
    x: &Plugin,
    state: Option<&PluginState>,
    params: &[(String, f64)],
    warnings: &mut Vec<String>,
) -> Result<(Box<dyn Processor>, NoteNames), String> {
    let bundle = xirius_clap::Bundle::load(path).map_err(|e| e.to_string())?;
    let mut plugin = xirius_clap::Plugin::new(&bundle, &x.uid)
        .map_err(|e| format!("{} を読み込めません: {e}", x.name))?;
    if let Some(state) = state.filter(|s| !s.component.is_empty()) {
        plugin
            .set_state(&state.component)
            .map_err(|e| format!("{} の状態を復元できません: {e}", x.name))?;
    }
    for (key, value) in params {
        match param_id(&plugin, key) {
            Some(id) => {
                plugin.set_param(id, value.clamp(0.0, 1.0));
            }
            None => warnings.push(format!("{}: パラメーター {key} が見つかりません", x.name)),
        }
    }
    let names = plugin.note_names();
    let part = plugin
        .audio()
        .map_err(|e| format!("{} を再生できません: {e}", x.name))?;
    // The plugin's main thread stays while the processor plays.
    Ok((Box::new(ClapProcessor::new(part)), names))
}

/// A parameter by numeric id, else by title (case-insensitive), as a song names it.
fn param_id(plugin: &xirius_clap::Plugin, key: &str) -> Option<u32> {
    let params = plugin.params();
    key.parse::<u32>()
        .ok()
        .filter(|id| params.iter().any(|p| p.id == *id))
        .or_else(|| {
            params
                .iter()
                .find(|p| p.title.eq_ignore_ascii_case(key))
                .map(|p| p.id)
        })
}
