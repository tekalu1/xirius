//! A channel's inserts copied or pasted with what their plug-ins hold now (MIX-045). A song
//! keeps a plug-in's state as it was last collected, and a plug-in turned in its own window
//! does not always say so: before a copy or a paste the plug-ins it reads (the source's) and
//! the ones it replaces (the target's) are asked for their state, and what they answer is put
//! in the song as a step of its own (プラグインの編集を反映), so the copy carries it and an undo
//! of the paste brings the replaced ones back as they were.
//!
//! The asking never waits on the window's thread: the command waits in [`Wait`] and each frame
//! looks whether the answers have come, the keys and the pointer held meanwhile as during a
//! save. A plug-in that has not answered after [`PULL_WAIT`] (as long as a save waits), or
//! whose host cannot answer, is not waited for: the command goes on with the state the song
//! last collected, as it did before it asked.

use std::collections::HashSet;
use std::time::Instant;

use serde_json::Value;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::session::RunOptions;
use xirius_session::host::{PULL_WAIT, collect_pulled_states};
use xirius_session::plugins::StatePull;

use crate::studio::Studio;

/// A copy or a paste of inserts held until its plug-ins have said what they hold.
pub(crate) struct Wait {
    command: String,
    args: Value,
    pull: StatePull,
    since: Instant,
}

impl Studio {
    /// Asks the plug-ins `command` reads or replaces for their state. True when the command
    /// now waits for them (it runs from [`Studio::poll_inserts_wait`]); false when it runs at
    /// once: nothing to ask, no host, or every answer already in.
    pub(crate) fn inserts_wait_begin(&mut self, command: &str, args: &Value) -> bool {
        if self.inserts_wait.is_some() || self.inserts_read {
            return false;
        }
        let ids = self.inserts_plugins(command, args);
        if ids.is_empty() {
            return false;
        }
        let Some(host) = self.transport.host_mut() else {
            return false;
        };
        let pull = host.begin_pull_states_of(&ids);
        let wait = Wait {
            command: command.to_string(),
            args: args.clone(),
            pull,
            since: crate::wait::now(),
        };
        if wait.pull.ready() && wait.pull.error().is_none() {
            // only built-in effects, or plug-ins that had nothing to read: no frame to wait
            self.inserts_collect(&wait.pull);
            return false;
        }
        self.inserts_wait = Some(wait);
        true
    }

    /// Runs the waiting copy or paste once its plug-ins have answered or [`PULL_WAIT`] has
    /// passed. True while it waits or when it ran.
    pub(crate) fn poll_inserts_wait(&mut self) -> bool {
        let Some(wait) = self.inserts_wait.as_ref() else {
            return false;
        };
        wait.pull.reconcile(&self.song.project());
        let waited = crate::wait::now().saturating_duration_since(wait.since);
        if !wait.pull.ready() && waited < PULL_WAIT {
            return true;
        }
        let wait = self.inserts_wait.take().expect("looked at above");
        if !wait.pull.ready() {
            // a late answer is not put in the song after the command has gone on without it
            wait.pull.quarantine();
        } else if wait.pull.error().is_some() {
            wait.pull.cancel();
        } else {
            self.inserts_collect(&wait.pull);
        }
        self.inserts_read = true;
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let ran = self.arrange_run(&wait.command, wait.args);
        self.inserts_read = false;
        if ran.is_some() {
            let (w, h) = self.viewport_size.get();
            let g = self.geometry(w, h);
            self.rebuild(&g);
        }
        true
    }

    /// Puts what the plug-ins answered in the song (a step of its own when anything changed).
    fn inserts_collect(&mut self, pull: &StatePull) {
        let Some(host) = self.transport.host_mut() else {
            return;
        };
        // when they cannot be put in, the copy goes on with the state the song had
        if collect_pulled_states(self.song.session_mut(), host, pull, &RunOptions::human()).is_ok()
        {
            self.after_edit();
        }
    }

    /// The plug-ins whose state `command` copies or throws away: the source's inserts, and
    /// the targets' when they are replaced.
    fn inserts_plugins(&self, command: &str, args: &Value) -> HashSet<PluginId> {
        let track = |v: &Value| v.as_str().and_then(|s| s.parse::<TrackId>().ok());
        let mut tracks: Vec<TrackId> = Vec::new();
        match command {
            "channel.inserts.copy" => tracks.extend(track(&args["id"])),
            "channel.inserts.paste" => {
                tracks.extend(track(&args["from"]));
                if args["append"].as_bool() != Some(true) {
                    let ids = args["ids"].as_array().map_or(&[][..], Vec::as_slice);
                    tracks.extend(ids.iter().filter_map(track));
                }
            }
            "edit.paste"
                if matches!(
                    self.song.session().state.clipboard,
                    Some(Clipboard::Inserts(_))
                ) =>
            {
                tracks.extend(track(&args["track"]));
            }
            _ => {}
        }
        let p = self.project();
        tracks
            .iter()
            .filter_map(|t| p.tracks.get(t))
            .flat_map(|t| t.inserts.iter().map(|x| x.id))
            .collect()
    }
}

#[cfg(test)]
#[path = "insert_states_tests.rs"]
mod tests;
