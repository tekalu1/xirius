//! VST3 hosting for Xirius (ARC-011, ARC-012), written in Rust on the `vst3` bindings.
//!
//! - [`Module`] loads the DLL inside a `.vst3` bundle and lists its classes.
//! - [`Plugin`] creates one audio processor class with its edit controller, connects them and
//!   keeps the UI-thread side: parameters, state and the editor window.
//! - [`AudioPart`] is the real-time side. It preallocates every buffer and event list in
//!   [`AudioPart::prepare`], so [`AudioPart::process`] does not allocate.
//!
//! Threading follows the VST3 rules: everything except `process` runs on the thread that made
//! the [`Plugin`] (the UI thread), which must also pump window messages while an editor is open.
//! Plugins may be made on several threads: loading modules and setting up and
//! tearing down instances take turns across the process, and a bundle loaded twice shares one
//! loaded module. Loaded DLLs stay initialized until process exit (ARC-043).

mod editor;
mod editor_keys;
mod host;
mod module;
mod plugin;
mod process;
mod serial;
mod stream;

pub use editor::{Caption, Capture, Editor, OFFSCREEN_ENV, Placement, capture_window};
pub use host::{ComponentHandler, Gesture, HostApplication, HostAttributeList, HostMessage};
pub use module::{ClassInfo, Module, default_vst3_dir, find_bundles};
pub use plugin::{
    AudioPart, CTRL_AFTERTOUCH, CTRL_PITCH_BEND, Control, ControlCounts, Note, ParamChange,
    ParamInfo, ParamSender, Plugin, PluginState,
};
pub use process::{EventList, ParamQueue, ParameterChanges};
pub use serial::turn_report;
pub use stream::MemoryStream;

/// Errors from loading and setting up a plugin. Calls inside `process` never return errors.
#[derive(Debug)]
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

/// Reads a NUL-terminated `char8` array from the SDK.
pub(crate) fn c_str(chars: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .map(|&c| c as u8)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads a NUL-terminated UTF-16 `String128` from the SDK.
pub(crate) fn utf16_str(chars: &[u16]) -> String {
    let end = chars.iter().position(|&c| c == 0).unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}
