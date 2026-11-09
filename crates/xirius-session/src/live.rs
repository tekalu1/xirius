//! Notes played by hand (UI-074): a PC key sends a note straight to a track's instrument,
//! playing or stopped, without going through the song.

use xirius_core::id::TrackId;
use xirius_core::model::PluginKind;
use xirius_engine::engine::Command;
use xirius_engine::processor::NoteKind;

use crate::host::SessionHost;

pub(crate) struct MouseInput {
    track: TrackId,
    input: xirius_engine::performance::Input,
    releasing: bool,
}

impl SessionHost {
    /// A slot is reserved before accepting each window press, so its release
    /// can never be lost to command-queue pressure. No on is retried.
    pub(crate) fn flush_mouse_releases(&mut self) -> bool {
        let mut i = 0;
        while i < self.mouse_inputs.len() {
            if !self.mouse_inputs[i].releasing {
                i += 1;
                continue;
            }
            let held = &self.mouse_inputs[i];
            if let Some(track) = self.track_index(held.track) {
                let input = held.input;
                if self
                    .try_live_command(Command::LiveInput { track, input })
                    .is_err()
                {
                    return false;
                }
            }
            self.mouse_inputs.swap_remove(i);
        }
        true
    }

    fn mouse_input(&mut self, track: TrackId, input: xirius_engine::performance::Input) -> bool {
        let ready = self.flush_mouse_releases();
        let found = self.mouse_inputs.iter().position(|h| {
            h.track == track && h.input.source == input.source && h.input.plugin == input.plugin
        });
        match input.kind {
            NoteKind::On { .. } => {
                if !ready
                    || found.is_some()
                    || self.mouse_inputs.len() == self.mouse_inputs.capacity()
                {
                    return false;
                }
                let Some(index) = self.track_index(track) else {
                    return false;
                };
                if self
                    .try_live_command(Command::LiveInput {
                        track: index,
                        input,
                    })
                    .is_err()
                {
                    return false;
                }
                self.mouse_inputs.push(MouseInput {
                    track,
                    input,
                    releasing: false,
                });
                true
            }
            NoteKind::Off { pitch } => {
                let Some(i) = found else {
                    return true;
                };
                if !matches!(self.mouse_inputs[i].input.kind, NoteKind::On { pitch: p, .. } | NoteKind::Off { pitch: p } if p == pitch)
                {
                    return false;
                }
                self.mouse_inputs[i].input.kind = input.kind;
                self.mouse_inputs[i].releasing = true;
                self.flush_mouse_releases();
                true
            }
            _ => self
                .track_index(track)
                .is_some_and(|track| self.send(Command::LiveInput { track, input })),
        }
    }
    pub fn release_missing_record_ports(&mut self, now: &[String]) {
        let missing = self
            .recording
            .as_ref()
            .map(|recording| recording.missing_ports(now))
            .unwrap_or_default();
        for port in missing {
            self.send(Command::ReleaseMidiPort(port));
        }
    }
    pub fn release_live_inputs(&mut self) -> bool {
        self.send(Command::ReleaseLiveInputs)
    }
    /// Sends a live input to the track's routing or one named instrument within it.
    pub fn live_input(&mut self, track: TrackId, input: xirius_engine::performance::Input) -> bool {
        if input.source & xirius_engine::performance::MOUSE != 0 {
            return self.mouse_input(track, input);
        }
        match self.track_index(track) {
            Some(track) => self.send(Command::LiveInput { track, input }),
            None => false,
        }
    }

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
