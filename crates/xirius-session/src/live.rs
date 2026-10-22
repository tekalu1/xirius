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
        match self.track_index(track) {
            Some(track) => self.send(Command::Note { track, kind }),
            None => false,
        }
    }

    /// Whether the instrument of `track` sounds a note played now (ENG-064). False while the
    /// plugin in a host process that plays it is opened there, has just opened and plays dry
    /// for a moment, or waits for its host to come back. One that could not be opened is
    /// ready: nothing better is to come.
    pub fn instrument_ready(&self, track: TrackId) -> bool {
        let Some(t) = self.song().tracks.get(&track) else {
            return true;
        };
        let Some(x) = t
            .inserts
            .iter()
            .find(|x| x.kind == PluginKind::Instrument && x.runs())
        else {
            return true;
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
            return false;
        }
        match self.plugins().link(x.id) {
            Some(link) => link.live() || link.error().is_some(),
            None => true,
        }
    }
}
