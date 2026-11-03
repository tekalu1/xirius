//! The ARA 2 host of Xirius (PLG-015, PLG-031 to PLG-037, PLG-047, PLG-058).
//!
//! ARA lets a plugin read a clip's audio, analyse it and play it back edited.
//! The plugin runs in this process (PLG-015): it reads the host's audio from its own threads.
//!
//! - [`sys`] is the C API of `third_party/ara_api/ARAInterface.h`, bound by hand.
//! - [`host`] is the host's side: the five controllers the plugin calls back (PLG-031).
//! - [`document`] drives one plugin's document controller: the document, audio sources,
//!   audio modifications and playback regions, edited between `beginEditing` and
//!   `endEditing` (PLG-032), analysis and notes (PLG-035), archives (PLG-037).
//! - [`vst3`] and [`clap`] find a plugin's ARA factory and bind its instances to a document
//!   with roles (PLG-033). AU is not built on Windows ([`au`]).
//! - [`contain`] keeps a fault or a hang inside the plugin from taking the app down
//!   (PLG-058, `docs/ara-host.md`).
//! - [`service`] is the ARA thread: it owns one plugin's document and renderers, takes the
//!   song's ARA clips, and hands back each clip's sound rendered through the plugin.
//! - [`archive`] is the versioned JSON a song keeps the plugin's archive in (ARC-025).
//! - [`fake`] is a small ARA plugin written in Rust, for tests.

pub mod archive;
pub mod au;
pub mod clap;
pub mod contain;
pub mod document;
pub mod fake;
pub mod host;
pub mod service;
pub mod sys;
pub mod vst3;

pub use archive::Archive;
pub use document::{Document, Note};
pub use host::SourceAudio;
pub use service::{ClipSpec, ClipState, Phase, Service, Song};

/// An error from the plugin or the binding, as a sentence for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
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
