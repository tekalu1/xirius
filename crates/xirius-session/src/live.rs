//! Notes played by hand (UI-074): a PC key sends a note straight to a track's instrument,
//! playing or stopped, without going through the song.

use xirius_core::id::TrackId;
use xirius_core::model::PluginKind;
use xirius_engine::engine::Command;
use xirius_engine::processor::NoteKind;

use crate::host::SessionHost;

impl SessionHost {
    /// Plays `kind` on the instrument of `track` at the start of the next block. False when the
    /// track is not among the engine's or the engine's queue is full.
    pub fn note(&mut self, track: TrackId, kind: NoteKind) -> bool {
        // Releases and controllers (including all-notes-off) always reach the
        // old graph. A failed new source must not leave an old held voice stuck.
        if !self.live_note_allowed(track, &kind) {
            return false;
        }
        match self.track_index(track) {
            Some(track) => self.send(Command::Note { track, kind }),
            None => false,
        }
    }

    /// Shared admission rule for direct notes and routed live MIDI inputs.
    /// A failed requested plan rejects new note-ons only. Releases, controllers
    /// and panic must still reach the accepted graph to avoid stuck voices.
    pub fn live_note_allowed(&self, track: TrackId, kind: &NoteKind) -> bool {
        !matches!(kind, NoteKind::On { .. }) || self.instrument_status(track).is_ok()
    }

    /// Whether the instrument of `track` sounds a note played now (ENG-064). False while the
    /// plugin in a host process that plays it is opened there, has just opened and plays dry
    /// for a moment, or waits for its host to come back. One that could not be opened is
    /// ready: nothing better is to come.
    pub fn instrument_ready(&self, track: TrackId) -> bool {
        self.instrument_status(track) == Ok(true)
    }

    /// Ready, pending, or a failed requested source. Failure does not mean
    /// retry: only an explicit graph retry can request the same target again.
    pub fn instrument_status(&self, track: TrackId) -> Result<bool, String> {
        if (self.swap_pending() || self.failed_graph_error().is_some())
            && !self.live_plan_matches(track)
        {
            return match self.failed_graph_error() {
                Some(_) => Err("音の準備に失敗しました".into()),
                None => Ok(false),
            };
        }
        let Some(t) = self.song().tracks.get(&track) else {
            return Ok(true);
        };
        let Some(x) = t
            .inserts
            .iter()
            .find(|x| x.kind == PluginKind::Instrument && x.runs())
        else {
            return Ok(true);
        };
        // A new mix that changes this instrument is on its way (ENG-065): the one that plays
        // now is not the one the song has.
        if self.swap_pending()
            && self.played_track(track).and_then(|t| {
                t.inserts
                    .iter()
                    .find(|y| y.kind == PluginKind::Instrument && y.runs())
            }) != Some(x)
        {
            return Ok(false);
        }
        Ok(match self.plugins().link(x.id) {
            Some(link) => link.live() || link.error().is_some(),
            None => true,
        })
    }
}
