//! Xirius core: the song model, the command registry, the history and the song
//! file. It knows nothing about the UI or the audio engine.

pub mod auto;
mod auto_point_push;
mod auto_write_bounds;
pub mod autosave;
pub mod clipboard;
pub mod clipfx;
pub mod command;
pub mod commands;
pub mod dawproject;
pub mod diff;
pub mod drum;
pub mod editing;
pub mod error;
pub mod extension;
pub mod freeze;
pub mod id;
pub mod in_use;
#[cfg(any(test, feature = "test-io"))]
pub mod io_test;
pub mod marker;
pub mod model;
pub mod nav;
pub mod packing;
pub mod protocol;
pub mod quantize;
pub mod rack;
pub mod recovery;
pub mod retime;
pub mod running;
pub mod session;
pub mod shape;
pub mod store;
pub mod takes;
pub mod tempo;
pub mod time;
pub mod track_preset;
pub mod xpreset;

pub use command::{Args, Author, CommandDef, Output, Registry, Risk};
pub use error::{CmdError, CmdResult, ErrorCode};
pub use extension::{ExtContext, Extension};
pub use model::Project;
pub use session::{AiPermission, Session};
