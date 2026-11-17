//! CLAP plugins a song names (PLG-028), next to the VST3 ones of [`crate::plugins`].
//!
//! The song keeps a CLAP plugin like a VST3 one: `format` is `clap`, `uid` its CLAP id, the
//! state as text ([`xirius_plugins::encode_state`], with the plugin's bytes as the component
//! part) and the parameters by numeric id or title. In a host process it is opened through
//! the same messages as a VST3 plugin, so it gets the same isolation, crash handling and
//! restart.

use xirius_core::model::Plugin;

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
