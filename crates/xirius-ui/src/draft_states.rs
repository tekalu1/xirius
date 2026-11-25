//! What the plug-ins hold now, in the draft (PRJ-034). A plug-in turned in its own window
//! does not always say so, and the song keeps the state it last collected: before the automatic
//! "下書き" of every 30 s, and when a plug-in's window closes, the plug-ins are asked for their
//! state, and what they answer goes in the song at the cursor with no step of its own
//! ([`collect_pulled_in_place`]), as a paste's question does (MIX-045). The song's file does
//! not change: what differs from the save shows 未保存 (PRJ-033).
//!
//! The asking never waits on the window's thread nor on the sound's: [`Pull`] waits and each
//! beat of the idle poll looks whether the answers have come. A plug-in that has not answered
//! after [`PULL_WAIT`], or whose host cannot answer, keeps what the song had; the others are
//! still read. The input is never held for it.
//!
//! Nothing is asked while a take is recorded or an export written (the processor is busy and
//! the take is not in the song yet), nor collected while a knob or a fader is held: the
//! answers wait until it is let go. A save, a question or anything else that holds the input
//! asks for itself, and a song that goes drops what was asked for it: a late answer is
//! quarantined, never put in the next song.

use std::time::Instant;

use xirius_session::host::{PULL_WAIT, collect_pulled_in_place};
use xirius_session::plugins::StatePull;

use crate::studio::Studio;

/// A question for the plug-ins' state, waiting for its answers.
pub(crate) struct Pull {
    pull: StatePull,
    since: Instant,
}

impl Studio {
    /// Asks every plug-in for its state, unless a question is already waiting: the answers go
    /// in the song from [`Studio::draft_states_ready`].
    pub(crate) fn draft_states_begin(&mut self) {
        if self.draft_pull.is_some() {
            return;
        }
        let Some(host) = self.transport.host_mut() else {
            return;
        };
        let pull = host.begin_pull_states();
        self.draft_pull = Some(Pull {
            pull,
            since: crate::wait::now(),
        });
    }

    /// Whether the automatic draft may look at the song now (PRJ-034): no question for the
    /// plug-ins is waiting, the ones asked have answered and are in the song, or there is
    /// nothing to ask. When the draft is due, the first beat asks and a later one answers.
    pub(crate) fn draft_states_ready(&mut self, now: u64) -> bool {
        if self.draft_pull.is_none() {
            if !self.song.session().autosave_due(now) || self.transport.host().is_none() {
                return true;
            }
            if self.knob_held() {
                return false;
            }
            self.draft_states_begin();
        }
        let Some(wait) = self.draft_pull.as_ref() else {
            return true;
        };
        wait.pull.reconcile(&self.song.project());
        let waited = crate::wait::now().saturating_duration_since(wait.since);
        if (!wait.pull.ready() && waited < PULL_WAIT) || self.knob_held() {
            return false;
        }
        let wait = self.draft_pull.take().expect("looked at above");
        self.draft_states_collect(&wait.pull);
        true
    }

    /// Drops a waiting question: its song has gone or something else asks now. A late answer
    /// is quarantined, not put in a song.
    pub(crate) fn draft_states_drop(&mut self) {
        if let Some(wait) = self.draft_pull.take() {
            wait.pull.quarantine();
        }
    }

    /// Puts what the plug-ins answered in the song at the cursor. The ones that failed or are
    /// late are left out and keep what the song had.
    fn draft_states_collect(&mut self, pull: &StatePull) {
        pull.settle();
        // A collection that fails leaves the song as it was: the next draft asks again, and a
        // save asks for itself.
        if let Some(host) = self.transport.host_mut()
            && collect_pulled_in_place(self.song.session_mut(), host, pull)
                .is_ok_and(|collected| collected.changed)
        {
            self.after_edit();
        }
    }

    /// A knob, a fader or another value is held with the pointer: a state collected now would
    /// land in the middle of the move.
    fn knob_held(&self) -> bool {
        self.pointer_held
            || self.wheel_running
            || self.pan.is_some()
            || self.mixer.drag.is_some()
            || self.mixer.bar_grab.is_some()
            || self.mixer.fx_drag.is_some()
            || self.arrange.drag.is_some()
            || self.roll.drag.is_some()
    }
}

#[cfg(test)]
#[path = "draft_states_tests.rs"]
mod tests;
