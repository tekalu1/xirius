//! A channel's inserts copied or pasted with what their plug-ins hold now (MIX-045). A song
//! keeps a plug-in's state as it was last collected, and a plug-in turned in its own window
//! does not always say so: before a copy or a paste the plug-ins it reads (the source's) and
//! the ones it replaces (the target's) are asked for their state.
//!
//! - A copy leaves the song and its history as they were: what the source's plug-ins answer
//!   goes in the clipboard's copy of them only.
//! - A paste puts what they answer in the song at the cursor, with no step of its own, and
//!   then pastes: the paste is the one step an undo takes back, and it brings the replaced
//!   plug-ins back as they answered. A paste that would be refused (onto its own source, onto
//!   frozen channels) is refused at once, without asking anything.
//!
//! The asking never waits on the window's thread: the command waits in [`Wait`] and each frame
//! looks whether the answers have come, the keys and the pointer held meanwhile as during a
//! save. A plug-in that has not answered after [`PULL_WAIT`] (as long as a save waits), or
//! whose host cannot answer, is not waited for: it goes on with the state the song last
//! collected, as before it was asked, and the ones that did answer are still read.
//!
//! The wait belongs to the song it began in. Leaving that song or closing the window runs the
//! command first, with what has answered ([`Studio::inserts_wait_flush`]), so the paste is in
//! the song the question whether to save asks about; a song replaced by another way drops it
//! ([`Studio::inserts_wait_drop`]), never running it in the next song, nor the input held
//! meanwhile.
//!
//! A step of a macro waits the same way: the macro goes on once the command has run, as one of
//! its steps, and stops when it could not. A command that could not run after its wait is
//! said, never dropped without a word.

use std::collections::HashSet;
use std::time::Instant;

use serde_json::Value;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::session::RunOptions;
use xirius_session::host::{PULL_WAIT, collect_pulled_in_place, peek_pulled_states};
use xirius_session::plugins::StatePull;

use crate::studio::Studio;

/// A copy or a paste of inserts held until its plug-ins have said what they hold.
pub(crate) struct Wait {
    command: String,
    args: Value,
    pull: StatePull,
    since: Instant,
    /// A step of the macro running when it began: it runs as that step.
    in_macro: bool,
}

impl Wait {
    fn copy(&self) -> bool {
        self.command == "channel.inserts.copy"
    }
}

impl Studio {
    /// Asks the plug-ins `command` reads or replaces for their state. True when the command
    /// was taken here: it waits for them (and runs from [`Studio::poll_inserts_wait`]), or
    /// every answer was in and it ran. False when the caller runs it as it is: nothing to
    /// ask, no host, or a paste that will be refused.
    pub(crate) fn inserts_wait_begin(&mut self, command: &str, args: &Value) -> bool {
        if self.inserts_wait.is_some() || self.inserts_read {
            return false;
        }
        let ids = self.inserts_plugins(command, args);
        if ids.is_empty() {
            return false;
        }
        let copy = command == "channel.inserts.copy";
        let dry = RunOptions::human().dry();
        let session = self.song.session_mut();
        // design-lint: allow(ui-blocking-io) MIX-045: a dry run only checks the paste against the song in memory; it writes nothing and touches no file or plug-in.
        if !copy && session.run_value(command, args.clone(), &dry).is_err() {
            // its error is said as it would be: nothing was asked, nothing put in the song
            return false;
        }
        let Some(host) = self.transport.host_mut() else {
            return false;
        };
        let pull = if copy {
            host.begin_peek_states_of(&ids)
        } else {
            host.begin_pull_states_of(&ids)
        };
        let wait = Wait {
            command: command.to_string(),
            args: args.clone(),
            pull,
            since: crate::wait::now(),
            in_macro: self.macro_stepping,
        };
        if wait.pull.ready() && wait.pull.error().is_none() {
            // only built-in effects, or plug-ins that had nothing to read: no frame to wait
            self.inserts_finish(wait);
        } else {
            self.inserts_wait = Some(wait);
        }
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
        self.inserts_wait_flush();
        true
    }

    /// Runs a waiting copy or paste now, with what has answered so far: the song is about to
    /// be left or the window closed, and the paste belongs to this song, before the question
    /// whether to save it. The late answers are quarantined.
    pub(crate) fn inserts_wait_flush(&mut self) {
        let Some(wait) = self.inserts_wait.take() else {
            return;
        };
        if self.inserts_finish(wait) {
            let (w, h) = self.viewport_size.get();
            let g = self.geometry(w, h);
            self.rebuild(&g);
        }
    }

    /// Drops a waiting copy or paste whose song has gone: its tracks are named in that song,
    /// and the ids repeat in the next one. A late answer is quarantined, not put in a song.
    pub(crate) fn inserts_wait_drop(&mut self) {
        if let Some(wait) = self.inserts_wait.take() {
            wait.pull.quarantine();
            // what was typed or pressed meanwhile was meant for that song too
            self.held_input_dropped.set(true);
        }
    }

    /// Whether the input held while a copy or a paste waited was dropped with it since the
    /// last look ([`Studio::inserts_wait_drop`]): the window throws it away.
    pub(crate) fn take_held_input_dropped(&self) -> bool {
        self.held_input_dropped.take()
    }

    /// Reads what the plug-ins answered and runs the command with it. True when it ran.
    fn inserts_finish(&mut self, wait: Wait) -> bool {
        // the ones that failed or are late are left out; the others are still read
        wait.pull.settle();
        if !wait.copy()
            && let Some(host) = self.transport.host_mut()
            && collect_pulled_in_place(self.song.session_mut(), host, &wait.pull).is_ok()
        {
            self.after_edit();
        }
        self.inserts_read = true;
        // a macro's step runs as the step it is, the macro waiting for it
        let stepping = self.macro_stepping;
        self.macro_stepping |= wait.in_macro;
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        let ran = self.arrange_run(&wait.command, wait.args.clone());
        self.macro_stepping = stepping;
        self.inserts_read = false;
        if wait.copy() && ran.is_some() {
            self.inserts_copy_states(&wait);
        }
        if ran.is_none() {
            // refused (said already), or the window turned to something else meanwhile
            let what = if wait.copy() {
                "コピー"
            } else {
                "貼り付け"
            };
            let reason = self.error.clone().unwrap_or_else(|| {
                format!("インサートの{what}を実行できませんでした。曲が別の処理中です")
            });
            self.error = Some(reason.clone());
            if wait.in_macro {
                self.fail_macro(reason);
            }
        }
        ran.is_some()
    }

    /// Puts what the source's plug-ins answered in the clipboard's copy of them; the song is
    /// left as it was.
    fn inserts_copy_states(&mut self, wait: &Wait) {
        let source = wait.args["id"]
            .as_str()
            .and_then(|s| s.parse::<TrackId>().ok());
        let project = self.song.project();
        let Some(host) = self.transport.host_mut() else {
            return;
        };
        if let Some(Clipboard::Inserts(copy)) = self.song.session_mut().state.clipboard.as_mut()
            && Some(copy.source) == source
        {
            peek_pulled_states(host, &wait.pull, &project, &mut copy.plugins);
        }
    }

    /// The plug-ins whose state `command` copies or throws away: the source's inserts, and
    /// the targets' when they are replaced. The targets are read as the command reads them:
    /// a list, words a macro or the palette wrote (`--ids a,b`), or the selection.
    fn inserts_plugins(&self, command: &str, args: &Value) -> HashSet<PluginId> {
        let track = |v: &Value| v.as_str().and_then(|s| s.parse::<TrackId>().ok());
        let mut tracks: Vec<TrackId> = Vec::new();
        match command {
            "channel.inserts.copy" => tracks.extend(track(&args["id"])),
            "channel.inserts.paste" => {
                tracks.extend(track(&args["from"]));
                let append = match &args["append"] {
                    Value::Bool(b) => *b,
                    Value::String(s) => s == "true",
                    _ => false,
                };
                if !append {
                    match &args["ids"] {
                        Value::Array(ids) => tracks.extend(ids.iter().filter_map(track)),
                        Value::String(ids) => tracks.extend(
                            ids.split([',', ' '])
                                .filter_map(|s| s.parse::<TrackId>().ok()),
                        ),
                        Value::Null => {
                            tracks.extend(self.song.session().state.selection.iter().filter_map(
                                |i| match i {
                                    xirius_core::id::AnyId::Track(t) => Some(*t),
                                    _ => None,
                                },
                            ))
                        }
                        _ => {}
                    }
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
