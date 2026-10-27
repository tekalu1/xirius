//! The PC keys as a keyboard (UI-074): A to L are the white keys, W E T Y U O
//! the black ones and Z / X move down and up an octave, as in the mock. A key
//! sounds on the armed instrument track at once, playing or stopped, until
//! it is let go; the key it plays lights up on the roll's keyboard.

use std::time::{Duration, Instant};

use xirius_core::id::TrackId;
use xirius_core::time::Tick;
use xirius_engine::NoteKind;
use xirius_session::audio_settings::PC_KEYBOARD;

use crate::studio::Studio;
use crate::tree::RowKind;

/// Each key's distance in semitones from the C of the octave.
const KEYS: [(&str, u8); 15] = [
    ("a", 0),
    ("w", 1),
    ("s", 2),
    ("e", 3),
    ("d", 4),
    ("f", 5),
    ("t", 6),
    ("g", 7),
    ("y", 8),
    ("h", 9),
    ("u", 10),
    ("j", 11),
    ("k", 12),
    ("o", 13),
    ("l", 14),
];

/// The lowest and highest octave Z and X reach, and the one at start.
const OCTAVES: (i8, i8) = (2, 6);
const OCTAVE: i8 = 4;

/// How hard a key plays (the mock's 100 of 127).
const VELOCITY: f32 = 100.0 / 127.0;

/// A played note's velocity with the track's, in the keyboard's 1..=127 (UI-312).
fn track_velocity(percent: i8, velocity: f32) -> f32 {
    let velocity = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
    f32::from(xirius_core::model::track_velocity(velocity, percent)) / 127.0
}

/// A note that has not sounded this long after the press shows the spinner in its key.
pub(crate) const WAIT_SHOWN: Duration = Duration::from_millis(150);
/// A note waits at most this long for its instrument, as every spinner turns at most so long.
pub(crate) const WAIT_GIVE_UP: Duration = Duration::from_millis(crate::loading::GIVE_UP_MS);

/// The PC keys' state: whether they play, the octave and the keys held.
#[derive(Clone, Debug)]
pub struct PcKeys {
    /// The keys play notes (the MIDI entry turns this on; Escape off).
    pub on: bool,
    pub octave: i8,
    /// Each key held: the track it plays on, the key's own pitch (what the lit key and the roll
    /// follow) and the pitch really sent there (the track's transpose added). Letting go ends
    /// the sent note and lights the key off, even after the octave or the value moved
    /// (UI-074, UI-312).
    pub down: Vec<(String, TrackId, u8, u8)>,
    /// The note the roll's keyboard or a note being dragged sounds now, with
    /// the track it sounds on, so that it ends when the roll lets go of it.
    pub roll: Option<(TrackId, u8)>,
    /// The note the roll could not sound (no output, the player did not start): it is not
    /// tried again on every move while the roll still wants it.
    pub missed: Option<(TrackId, u8)>,
    /// The note the roll wants while its instrument is not ready to sound (ENG-064: it is
    /// opening again, or warming up), and since when. It sounds once the instrument is ready if
    /// the roll still wants it, and never after; the lit key shows a spinner meanwhile.
    pub waiting: Option<((TrackId, u8), Instant)>,
    /// The PC keys held whose instrument was not ready when they went down (ENG-065): the key,
    /// the track, the pitch and since when. Each sounds once its instrument is ready if the key
    /// is still down, and never after it came up.
    pub pc_waiting: Vec<(String, TrackId, u8, Instant)>,
    /// The audition tool is playing the clip's track alone (EDT-027): the playhead to go back to
    /// when it lets go.
    pub listening: Option<Tick>,
}

impl Default for PcKeys {
    fn default() -> PcKeys {
        PcKeys {
            on: false,
            octave: OCTAVE,
            down: Vec::new(),
            roll: None,
            missed: None,
            waiting: None,
            pc_waiting: Vec::new(),
            listening: None,
        }
    }
}

impl PcKeys {
    /// The pitch `key` plays in the octave now.
    pub fn pitch(&self, key: &str) -> Option<u8> {
        let (_, off) = KEYS.iter().find(|(k, _)| *k == key)?;
        u8::try_from(i16::from(self.octave + 1) * 12 + i16::from(*off)).ok()
    }
}

impl Studio {
    /// The track the PC keys play: the selected armed instrument track, or
    /// else the first armed one (`input_follow::armed_instruments`).
    pub fn pc_target(&self) -> Option<TrackId> {
        let armed = crate::input_follow::armed_instruments(&self.project(), None);
        if let Some(RowKind::Track(t)) = self.selected_row
            && armed.contains(&t)
        {
            return Some(t);
        }
        armed.into_iter().next()
    }

    /// The keys play while turned on or while recording, and nothing that
    /// takes letters (the palette, a popup, the browser's search) is open.
    /// 「PC のキーボード」 turned off in the MIDI settings, they never play (UI-263).
    fn pc_keys_live(&self) -> bool {
        (self.pc_keys.on || self.transport.recording)
            && self.palette.is_none()
            && self.popup.is_none()
            && !self.browser.typing
            && self.pc_keyboard_on()
    }

    /// 「PC のキーボード」 is on in the list of the MIDI inputs.
    pub(crate) fn pc_keyboard_on(&self) -> bool {
        self.song
            .settings()
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prefs
            .bool(&format!("midi.inputs.{PC_KEYBOARD}"))
    }

    /// A PC key played `pitch`: the light of 「PC のキーボード」 comes on and the note shows.
    pub(crate) fn pc_heard(&mut self, pitch: u8) {
        self.song
            .io
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .heard(PC_KEYBOARD, pitch, std::time::Instant::now());
    }

    /// A key went down. True when the PC keys took it; a key held down
    /// (`held`, the OS repeating it) is taken and plays nothing more.
    pub fn pc_key_down(&mut self, key: &str, held: bool) -> bool {
        if !self.pc_keys_live() {
            return false;
        }
        if key == "escape" && self.pc_keys.on {
            self.pc_keys_off();
            return true;
        }
        if key == "z" || key == "x" {
            let step = if key == "z" { -1 } else { 1 };
            self.pc_keys.octave = (self.pc_keys.octave + step).clamp(OCTAVES.0, OCTAVES.1);
            return true;
        }
        let Some(pitch) = self.pc_keys.pitch(key) else {
            return false;
        };
        if held || self.pc_keys.down.iter().any(|(k, ..)| k == key) {
            return true;
        }
        let Some(track) = self.pc_target() else {
            return false;
        };
        self.pc_heard(pitch);
        self.pc_play(key, track, pitch);
        true
    }

    /// A key on the MIDI settings page (UI-263): a PC key lights 「PC のキーボード」 and shows
    /// its note, and sounds on the armed track if there is one. True when it was a PC key.
    pub(crate) fn settings_pc_key(&mut self, key: &str) -> bool {
        let Some(pitch) = self.pc_keys.pitch(key) else {
            return false;
        };
        if !self.pc_keyboard_on() || self.pc_keys.down.iter().any(|(k, ..)| k == key) {
            return true;
        }
        self.pc_heard(pitch);
        if let Some(track) = self.pc_target() {
            self.pc_play(key, track, pitch);
        }
        true
    }

    /// `key` starts `pitch` on `track` until it is let go.
    fn pc_play(&mut self, key: &str, track: TrackId, pitch: u8) {
        // An instrument still opening in its host (or warming up) cannot play the note yet: it
        // is kept while the key is down and sounds when the instrument is ready (ENG-065).
        let p = self.project();
        let Some(t) = p.tracks.get(&track) else {
            return;
        };
        // The track's transpose and velocity are applied to the keys played (UI-312); a key
        // that would leave the keyboard does not sound.
        let (transpose, velocity) = (t.transpose, t.velocity);
        let Some(heard) = xirius_session::build::playable(transpose, pitch) else {
            return;
        };
        let velocity = track_velocity(velocity, VELOCITY);
        if let Ok(false) = self.transport.instrument_ready(&p, self.song.dir(), track) {
            self.pc_keys
                .down
                .push((key.to_string(), track, pitch, heard));
            self.pc_keys
                .pc_waiting
                .push((key.to_string(), track, heard, Instant::now()));
            self.roll.audition = Some(pitch);
            self.roll.audition_by_edit = false;
            return;
        }
        let kind = NoteKind::On {
            pitch: heard,
            velocity,
        };
        if self.pc_note(track, kind) {
            self.pc_keys
                .down
                .push((key.to_string(), track, pitch, heard));
            self.roll.audition = Some(pitch);
            self.roll.audition_by_edit = false;
        }
    }

    /// A key came up: the note it started ends.
    pub fn pc_key_up(&mut self, key: &str) -> bool {
        let Some(i) = self.pc_keys.down.iter().position(|(k, ..)| k == key) else {
            return false;
        };
        let (key, track, own, heard) = self.pc_keys.down.remove(i);
        // Let go of before its instrument was ready: it never sounds.
        let silent = self.pc_keys.pc_waiting.iter().any(|w| w.0 == key);
        self.pc_keys.pc_waiting.retain(|w| w.0 != key);
        if !silent {
            self.pc_note(track, NoteKind::Off { pitch: heard });
        }
        // The lit key follows the key's own pitch, not the pitch the track's value made of it.
        if self.roll.audition == Some(own) {
            self.roll.audition = self.pc_keys.down.last().map(|(_, _, p, _)| *p);
        }
        true
    }

    /// Lets every held key go and stops taking the keys.
    pub fn pc_keys_off(&mut self) {
        self.pc_keys_release();
        self.pc_keys.on = false;
    }

    /// The keys held let go of their notes, and the roll of the one it sounds; the keys still
    /// play (KEY-012: All Notes Off).
    pub(crate) fn pc_keys_release(&mut self) {
        let waiting = std::mem::take(&mut self.pc_keys.pc_waiting);
        for (key, track, _, heard) in std::mem::take(&mut self.pc_keys.down) {
            if !waiting.iter().any(|w| w.0 == key) {
                self.pc_note(track, NoteKind::Off { pitch: heard });
            }
        }
        self.roll.audition = None;
    }

    /// The roll's lit key sounds (UI-074): a press on its keyboard or on a
    /// note, and a drag from pitch to pitch, play that pitch on the clip's
    /// track until the roll lets go. A machine without an output stays silent
    /// and says nothing; the PC keys, which play too, have the say then.
    pub(crate) fn sound_roll_audition(&mut self) {
        self.listen_to_roll();
        let want = if self.pc_keys.down.is_empty() && self.pc_keys.listening.is_none() {
            self.roll_target().and_then(|t| {
                // The roll draws the notes as written; what sounds carries the clip's
                // transpose and the track's (UI-311/UI-312).
                let by = t.clip.transpose().saturating_add(t.track.transpose);
                let heard = xirius_session::build::playable(by, self.roll.sounding()?)?;
                Some((t.track.id, heard))
            })
        } else {
            None
        };
        if want.is_none() {
            self.pc_keys.missed = None;
        }
        // Let go of, or moved off, before its instrument was ready: it never sounds.
        if self.pc_keys.waiting.is_some_and(|(w, _)| Some(w) != want) {
            self.pc_keys.waiting = None;
        }
        if want == self.pc_keys.roll || (want.is_some() && want == self.pc_keys.missed) {
            return;
        }
        let p = self.project();
        let dir = self.song.dir();
        if let Some((track, pitch)) = self.pc_keys.roll.take() {
            let _ = self
                .transport
                .note(&p, dir.clone(), track, NoteKind::Off { pitch });
        }
        if let Some((track, pitch)) = want {
            match self.transport.instrument_ready(&p, dir.clone(), track) {
                Ok(true) => self.pc_keys.waiting = None,
                Ok(false) => {
                    match self.pc_keys.waiting {
                        Some((_, since)) if since.elapsed() > WAIT_GIVE_UP => {
                            self.pc_keys.waiting = None;
                            self.pc_keys.missed = want;
                        }
                        Some(_) => {}
                        None => self.pc_keys.waiting = Some(((track, pitch), Instant::now())),
                    }
                    return;
                }
                Err(_) => {
                    self.pc_keys.waiting = None;
                    self.pc_keys.missed = want;
                    return;
                }
            }
            let velocity = p
                .tracks
                .get(&track)
                .map_or(VELOCITY, |t| track_velocity(t.velocity, VELOCITY));
            let kind = NoteKind::On { pitch, velocity };
            if self.transport.note(&p, dir, track, kind).is_ok() {
                self.pc_keys.roll = want;
                self.pc_keys.missed = None;
            } else {
                self.pc_keys.missed = want;
            }
        }
    }

    /// Once a frame while the roll's note waits for its instrument: sounds it once it is ready.
    pub(crate) fn poll_waiting_key(&mut self) -> bool {
        let pc = self.poll_waiting_pc_keys();
        if self.pc_keys.waiting.is_none() {
            return pc;
        }
        self.sound_roll_audition();
        true
    }

    /// The PC keys held while their instrument was not ready: each sounds once it is, and
    /// gives up after [`WAIT_GIVE_UP`]. True while some still wait.
    fn poll_waiting_pc_keys(&mut self) -> bool {
        if self.pc_keys.pc_waiting.is_empty() {
            return false;
        }
        let p = self.project();
        let dir = self.song.dir();
        let mut waiting = std::mem::take(&mut self.pc_keys.pc_waiting);
        waiting.retain(|(key, track, pitch, since)| {
            if !self.pc_keys.down.iter().any(|(k, ..)| k == key) {
                return false;
            }
            match self.transport.instrument_ready(&p, dir.clone(), *track) {
                Ok(true) => {
                    let kind = NoteKind::On {
                        pitch: *pitch,
                        velocity: track_velocity(
                            p.tracks.get(track).map_or(0, |t| t.velocity),
                            VELOCITY,
                        ),
                    };
                    let _ = self.transport.note(&p, dir.clone(), *track, kind);
                    false
                }
                Ok(false) => since.elapsed() <= WAIT_GIVE_UP,
                Err(_) => false,
            }
        });
        self.pc_keys.pc_waiting = waiting;
        !self.pc_keys.pc_waiting.is_empty()
    }

    /// The pitch whose lit key shows the spinner: the roll's note has waited for its instrument
    /// for [`WAIT_SHOWN`] (a note that sounds sooner shows nothing).
    pub(crate) fn roll_key_waiting(&self) -> Option<u8> {
        if let Some((_, track, pitch, _)) = self
            .pc_keys
            .pc_waiting
            .iter()
            .find(|w| w.3.elapsed() >= WAIT_SHOWN)
            && self.roll_target().is_some_and(|t| t.track.id == *track)
        {
            return Some(*pitch);
        }
        let ((track, pitch), since) = self.pc_keys.waiting?;
        (since.elapsed() >= WAIT_SHOWN && self.roll_target().is_some_and(|t| t.track.id == track))
            .then_some(pitch)
    }

    /// The audition tool held in the roll (EDT-027): the song plays from where it was pressed
    /// with only the clip's track sounding, and stops, back at the old playhead, when it lets
    /// go. A song that is playing already is left alone.
    fn listen_to_roll(&mut self) {
        let want = self.roll.listen;
        match (want, self.pc_keys.listening) {
            (Some(at), None) if !self.transport.playing => {
                let Some(target) = self.roll_target() else {
                    return;
                };
                let mut solo = self.project();
                for (id, track) in &mut std::sync::Arc::make_mut(&mut solo).tracks {
                    std::sync::Arc::make_mut(track).solo = *id == target.track.id;
                }
                let dir = self.song.dir();
                self.transport.sync(&solo, dir.clone());
                // Like the lit key: a machine without an output stays silent and says nothing.
                if self.transport.play(&solo, dir, at).is_ok() {
                    self.pc_keys.listening = Some(self.playhead);
                }
            }
            (None, Some(back)) => {
                self.transport.pause();
                self.pc_keys.listening = None;
                let p = self.project();
                self.transport.sync(&p, self.song.dir());
                // design-lint: allow(ui-blocking-io) PRJ-031: transport playhead seek updates memory and submits an engine command.
                self.transport.seek(back);
            }
            _ => {}
        }
    }

    fn pc_note(&mut self, track: TrackId, kind: NoteKind) -> bool {
        let p = self.project();
        match self.transport.note(&p, self.song.dir(), track, kind) {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
}
