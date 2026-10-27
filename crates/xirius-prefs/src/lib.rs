//! The user's settings (SET-001, KEY-003, 06 §3): `prefs.toml`, `keymap.toml` and
//! `mousemap.toml` in one folder, and the commands that read and change them.
//!
//! The screen, the CLI and the MCP server change the settings only through the commands
//! (`prefs.set`, `keymap.set`, `mousemap.set`, ...), answered by [`PrefsExt`], so what one of
//! them changes the others see.

pub mod catalog;
pub mod chord;
pub mod cmdline;
pub mod export;
pub mod ext;
pub mod import;
pub mod keymap;
mod lines;
pub mod macro_ext;
pub mod macros;
pub mod midimap;
pub mod mousemap;
pub mod prefs;
pub mod store;
pub mod studio_pro;
mod tomlw;

pub use chord::{Chord, Os};
pub use ext::{PrefsExt, is_settings_command, targets_audio_device};
pub use keymap::Keymap;
pub use macros::Macros;
pub use midimap::Midimap;
pub use mousemap::Mousemap;
pub use prefs::Prefs;
pub use store::{File, Settings, Shared, config_dir, config_path};

/// Adds the settings' commands to a session.
pub fn attach(session: &mut xirius_core::Session, shared: Shared) {
    session.add_extension(Box::new(PrefsExt::new(shared)));
}
