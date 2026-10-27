//! The instrument input follows the selection (REC-024), as Studio Pro's
//! "Instrument Input Follows Selection" does: with the setting
//! 「インストゥルメントの入力を選択に合わせる」 on (`midi.follow-selection`, on
//! unless turned off), choosing an instrument track arms it and its input
//! monitor and lets every other instrument track's go, so the one selected
//! track is the one a MIDI keyboard or the PC keys sound. Choosing another
//! kind of row leaves the arms as they are, as does choosing a frozen
//! instrument track (it cannot take a take and its instrument is silent), and
//! with the setting off the selection touches nothing: the armed instrument
//! tracks take the notes.
//!
//! The arm and monitor flags ride on the song's state at the cursor
//! ([`Song::amend`]): a shadow of the selection, not an edit the person takes
//! back, so no step of the history is made.

use std::sync::Arc;

use xirius_core::id::TrackId;
use xirius_core::model::{Project, TrackKind};

use crate::studio::Studio;
use crate::tree::RowKind;

/// The instrument tracks a played note goes to: the armed ones. With `port`
/// (the name of the MIDI port the note came on), only the tracks whose input
/// names that port, or none (every port). `None` asks for all of them.
pub fn armed_instruments(p: &Project, port: Option<&str>) -> Vec<TrackId> {
    p.tracks
        .values()
        .filter(|t| {
            t.kind == TrackKind::Instrument
                && t.armed
                && port.is_none_or(|name| t.input.as_deref().is_none_or(|n| n == name))
        })
        .map(|t| t.id)
        .collect()
}

impl Studio {
    /// Selects `row`: the row the next command acts on. An instrument track so
    /// chosen takes the instrument input over (see the module's note).
    pub fn select_row(&mut self, row: Option<RowKind>) {
        // Another track's values are not where the half-typed one belongs (UI-312).
        self.field_blur();
        // A row outside the selection is chosen alone; one inside it moves the focus only (UI-340).
        if row.is_none_or(|r| !self.arrange.tracks.contains(&r)) {
            self.arrange.tracks = row.into_iter().collect();
            self.arrange.anchor = row;
            self.arrange.touched();
        }
        self.selected_row = row;
        self.follow_selection();
    }

    /// With `midi.follow-selection` on and an instrument track selected: that
    /// track becomes the one armed (and monitored) instrument track. Anything
    /// else (another kind of row, none, a frozen track, the setting off, a
    /// take recording) leaves the arms as they are. Called after every move of
    /// the history cursor (undo, redo, a jump, a version restored), since an
    /// older state's arms come back with it.
    pub(crate) fn follow_selection(&mut self) {
        if self.transport.recording {
            return;
        }
        let follow = self
            .song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prefs
            .bool("midi.follow-selection");
        let Some(RowKind::Track(id)) = self.selected_row else {
            return;
        };
        if !follow {
            return;
        }
        {
            let p = self.project();
            let right = |t: &Arc<xirius_core::model::Track>| {
                t.kind != TrackKind::Instrument || (t.armed && t.monitor) == (t.id == id)
            };
            // A frozen instrument track is not armed: it cannot take a take and its instrument
            // is silent. The arms stay as they are.
            if !p
                .tracks
                .get(&id)
                .is_some_and(|t| t.kind == TrackKind::Instrument && t.frozen.is_none())
                || p.tracks.values().all(right)
            {
                return;
            }
        }
        if let Err(e) = self.song.amend(|p| {
            for t in p.tracks.values_mut() {
                if t.kind == TrackKind::Instrument {
                    let on = t.id == id;
                    let t = Arc::make_mut(t);
                    t.armed = on;
                    t.monitor = on;
                }
            }
        }) {
            self.error = Some(e);
            return;
        }
        self.after_edit();
    }
}
