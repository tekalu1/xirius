//! Commands answered outside core (ARC-014, ARC-016).
//!
//! Core keeps one command table, and the CLI, the MCP tools and the UI are all made from it.
//! Some commands need more than the song: exporting and playing need the audio engine, listing
//! plugins needs the disk. Such a command is defined like any other, with
//! [`Handler::Session`](crate::command::Handler::Session), in the table the session is made
//! with; an [`Extension`] added with [`Session::add_extension`](crate::Session::add_extension)
//! answers it. The session checks the permission, normalises the arguments and records the run
//! as for its own commands, so every caller gets the same behaviour.
//!
//! An extension sees the song but does not change it; it is told when the song has changed so
//! it can follow (a running engine, for one). What a run leaves for the song (the takes of a
//! recording) it hands back as commands of the table ([`Extension::take_edits`]), which the
//! session runs as edits of its own.

use std::path::Path;
use std::sync::Arc;

use crate::command::{Args, Author, CommandDef, Output, Registry};
use crate::error::CmdResult;
use crate::model::{Plugin, Project};

/// What an extension is given on each call.
pub struct ExtContext<'a> {
    pub project: &'a Arc<Project>,
    /// The song folder, `None` for a song not saved yet.
    pub dir: Option<&'a Path>,
    pub author: &'a Author,
    pub registry: &'a Arc<Registry>,
    /// An AI's work copy is open; its states live only in memory (`project.cleanup` waits).
    pub work_copy: bool,
    /// The history lane the session holds in its song folder (PRJ-023): with
    /// [`others_open`](crate::session::others_open), whether another process has the song open.
    pub lane: Option<u32>,
    /// The session's id among those whose files [`crate::in_use`] notes.
    pub uses: u64,
}

pub trait Extension: Send {
    /// Whether this extension answers `name`.
    fn answers(&self, name: &str) -> bool;

    /// Whether a window is there to do what the screen alone can (a key action, a view
    /// change). The CLI's extensions leave this false.
    fn has_screen(&self) -> bool {
        false
    }

    /// Runs a command. The arguments are already checked against the table. With `dry`, it
    /// reports what it would do and does nothing.
    fn run(
        &mut self,
        cx: &ExtContext<'_>,
        def: &CommandDef,
        args: &Args,
        dry: bool,
    ) -> CmdResult<Output>;

    /// Called after a command changed the song (an edit, an undo, opening another song).
    fn song_changed(&mut self, _cx: &ExtContext<'_>) {}

    /// Fills arguments the caller left out from what this extension keeps, before they are
    /// checked (EDT-135: `--ripple` of the clip edits follows the setting `edit.ripple`). What
    /// the caller gave is left as it is.
    fn fill_args(&self, _def: &CommandDef, _raw: &mut serde_json::Map<String, serde_json::Value>) {}

    /// Commands of the table (name, arguments) the last successful `run` left to change the
    /// song with. The session runs them right after it, so one run is one step of the history
    /// (REC-016: `record.stop` places its takes with `record.take.add`).
    fn take_edits(&mut self) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
        Vec::new()
    }

    /// Whether the edits `name` leaves are taken back and done again together (a macro's
    /// steps), not one at a time.
    fn groups_edits(&self, _name: &str) -> bool {
        false
    }

    /// Whether this PC has the VST3 or CLAP `plugin`; `None` when this extension cannot tell
    /// (EXP-006: a song read from another DAW names the plugins it has no copy of).
    fn has_plugin(&mut self, _plugin: &Plugin) -> Option<bool> {
        None
    }

    /// What it has to tell since it was last asked (each with the field `event` naming it),
    /// taken from it. The session hands them with the result of the run
    /// ([`RunResult::events`](crate::session::RunResult::events)).
    fn take_events(&mut self) -> Vec<serde_json::Value> {
        Vec::new()
    }
}
