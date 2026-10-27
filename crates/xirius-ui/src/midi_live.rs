//! Live MIDI delivery to the selected instrument, using the recording input interface.
use crate::studio::Studio;
use std::collections::BTreeMap;
use xirius_core::id::TrackId;
use xirius_engine::NoteKind;
use xirius_engine::record::{MidiInput, note_of};

/// The tracks a held key was sent to, with the pitch each really got (`None`: it could not
/// sound there).
type HeldNotes = Vec<(TrackId, Option<u8>)>;

#[derive(Default)]
pub struct LiveMidi {
    pub opener: Option<crate::midi_open::Opener>,
    pub opening: Option<crate::midi_open::Opening>,
    pub generation: u64,
    pub retiring: Vec<xirius_session::background::Job<()>>,
    pub connection_end: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub retirement_failed: bool,
    pub opening_for_record: bool,
    pub opening_error: Option<String>,
    pub input: Option<Box<dyn MidiInput>>,
    /// The notes held, by port and the key's own pitch: every track the note was sent to and
    /// the pitch it really got there (`None`: the track's value took it off the keyboard, so
    /// nothing sounded). The off uses the sent pitch, whatever the track's transpose is by
    /// then; the tracks are kept all the same, so a track that did not sound does not make the
    /// note look stale (UI-312).
    held: BTreeMap<(usize, u8), HeldNotes>,
    routed: BTreeMap<usize, Vec<TrackId>>,
    pub resume: bool,
    /// The knobs and pads of the controller (OPS-007).
    pub assign: crate::midi_assign::Assign,
    pub retry_at: Option<std::time::Instant>,
    /// The notes sent, for the tests: (track, kind).
    #[cfg(test)]
    pub sent: Vec<(TrackId, NoteKind)>,
}

/// The note a played key sounds on `track`: the track's transpose may take it off the keyboard
/// (then it does not sound), and the track's velocity is applied (UI-312).
fn heard(p: &xirius_core::model::Project, track: TrackId, kind: NoteKind) -> Option<NoteKind> {
    let t = p.tracks.get(&track)?;
    match kind {
        NoteKind::On { pitch, velocity } => {
            let pitch = xirius_session::build::playable(t.transpose, pitch)?;
            let velocity = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
            let velocity =
                f32::from(xirius_core::model::track_velocity(velocity, t.velocity)) / 127.0;
            Some(NoteKind::On { pitch, velocity })
        }
        NoteKind::Off { pitch } => Some(NoteKind::Off {
            pitch: xirius_session::build::playable(t.transpose, pitch)?,
        }),
        other => Some(other),
    }
}

impl Studio {
    /// Plays `kind` on `track`, keeping it for the tests to read (UI-312).
    fn live_note(&mut self, p: &xirius_core::model::Project, track: TrackId, kind: NoteKind) {
        #[cfg(test)]
        self.live_midi.sent.push((track, kind));
        if let Err(e) = self.transport.note(p, self.song.dir(), track, kind) {
            self.error = Some(e);
        }
    }

    pub fn live_midi_input(&mut self, input: Box<dyn MidiInput>) {
        if let Some(previous) = self.live_midi.opening_error.take()
            && self.error.as_ref() == Some(&previous)
        {
            self.error = None;
        }
        self.live_midi.resume = false;
        self.live_midi.retry_at = None;
        self.release_midi();
        if let Some(previous) = self.live_midi.input.take() {
            self.retire_midi_input(previous);
        }
        self.song
            .io
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .view
            .ports = input.ports();
        self.live_midi.connection_end = input.closed();
        self.live_midi.input = Some(input);
    }

    pub fn release_midi(&mut self) {
        let p = self.project();
        for tracks in std::mem::take(&mut self.live_midi.routed).values() {
            for &track in tracks {
                self.live_note(&p, track, NoteKind::control(64, 0));
            }
        }
        for (_, notes) in std::mem::take(&mut self.live_midi.held) {
            for (track, pitch) in notes {
                if let Some(pitch) = pitch {
                    self.live_note(&p, track, NoteKind::Off { pitch });
                }
            }
        }
    }

    pub fn poll_live_midi(&mut self) -> bool {
        self.poll_stale_midi_open();
        let mut changed = self.knob_quiet();
        if !self.live_midi.assign.pending.is_empty() {
            let map = self.midimap();
            changed |= self.midi_pending(&map);
        }
        if self.live_midi.opening_for_record
            && !self.transport.pending_record
            && !self.transport.recording
        {
            self.invalidate_midi_open();
            self.live_midi.opening_for_record = false;
            self.live_midi.resume = true;
        }
        if (self.transport.recording || self.transport.pending_record)
            && self.live_midi.input.is_none()
        {
            return changed;
        }
        if self.live_midi.resume
            && self
                .live_midi
                .retry_at
                .is_none_or(|at| std::time::Instant::now() >= at)
        {
            if self.can_open_live_midi() && !self.midi_ports.known().is_empty() {
                changed |= self.try_live_midi_open();
            } else {
                self.live_midi.resume = false;
            }
        }
        let Some(input) = &mut self.live_midi.input else {
            return changed;
        };
        let ports = input.ports();
        let mut messages = Vec::new();
        input.read_ports(256, &mut |_, port, msg| messages.push((port, msg)));
        let prefs = self
            .song
            .settings()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .prefs
            .clone();
        let p = self.project();
        let targets = |port: usize| -> Vec<TrackId> {
            let name = ports.get(port);
            if name.is_some_and(|n| !prefs.bool(&format!("midi.inputs.{n}"))) {
                return Vec::new();
            }
            crate::input_follow::armed_instruments(&p, name.map(String::as_str))
        };
        // Release sustain as well as notes when a port or its destination changes.
        let mut left: Vec<TrackId> = Vec::new();
        for (port, tracks) in &self.live_midi.routed {
            if *tracks != targets(*port) {
                left.extend(tracks.iter().copied());
            }
        }
        for track in left {
            self.live_note(&p, track, NoteKind::control(64, 0));
        }
        self.live_midi
            .routed
            .retain(|&port, tracks| *tracks == targets(port));
        // A switched-off port or a new selection releases held notes on their original track,
        // at the pitch that was sent there (UI-312).
        let stale: Vec<_> = self
            .live_midi
            .held
            .iter()
            .filter(|((port, _), notes)| {
                let now = targets(*port);
                notes.len() != now.len() || notes.iter().zip(&now).any(|((t, _), n)| t != n)
            })
            .map(|(k, _)| *k)
            .collect();
        for key in stale {
            if let Some(notes) = self.live_midi.held.remove(&key) {
                for (track, pitch) in notes {
                    if let Some(pitch) = pitch {
                        self.live_note(&p, track, NoteKind::Off { pitch });
                    }
                }
            }
        }
        // A value that takes a held key off the keyboard stops that note now (UI-312).
        let lost: Vec<_> = self
            .live_midi
            .held
            .iter()
            .filter_map(|(&(port, pitch), notes)| {
                // Only a track that sounded can be taken off the keyboard; one that could not
                // play the key at all is not a reason to stop the others (UI-312).
                let out = notes.iter().any(|(track, sent)| {
                    sent.is_some()
                        && p.tracks.get(track).is_none_or(|t| {
                            xirius_session::build::playable(t.transpose, pitch).is_none()
                        })
                });
                out.then_some((port, pitch))
            })
            .collect();
        for key in lost {
            if let Some(notes) = self.live_midi.held.remove(&key) {
                for (track, sent) in notes {
                    if let Some(sent) = sent {
                        self.live_note(&p, track, NoteKind::Off { pitch: sent });
                    }
                }
            }
        }
        changed |= !messages.is_empty();
        let map = self.midimap();
        for (port, msg) in messages {
            if let Some(name) = ports.get(port)
                && prefs.bool(&format!("midi.inputs.{name}"))
            {
                let mut io = self.song.io.lock().unwrap_or_else(|e| e.into_inner());
                let now = std::time::Instant::now();
                match note_of(msg) {
                    Some(NoteKind::On { pitch, .. }) => io.heard(name, pitch, now),
                    _ => io.receiving(name, now),
                }
            }
            if self.midi_take(&map, ports.get(port).map(String::as_str), msg) {
                continue;
            }
            let Some(kind) = note_of(msg) else {
                continue;
            };
            let tracks = targets(port);
            self.live_midi.routed.insert(port, tracks.clone());
            match kind {
                NoteKind::On { pitch, .. } => {
                    // Every track the note went to is kept, with what it heard (if anything),
                    // so the off can use the shipped pitch and the stale test can compare the
                    // destinations alone (UI-312).
                    let sent: HeldNotes = tracks
                        .iter()
                        .map(|&track| {
                            let heard = match heard(&p, track, kind) {
                                Some(NoteKind::On { pitch, .. }) => Some(pitch),
                                _ => None,
                            };
                            (track, heard)
                        })
                        .collect();
                    self.live_midi.held.insert((port, pitch), sent);
                }
                NoteKind::Off { pitch } => {
                    let notes = self
                        .live_midi
                        .held
                        .remove(&(port, pitch))
                        .unwrap_or_default();
                    for (track, sent) in notes {
                        if let Some(sent) = sent {
                            self.live_note(&p, track, NoteKind::Off { pitch: sent });
                        }
                    }
                    continue;
                }
                _ => {}
            }
            for track in tracks {
                let Some(kind) = heard(&p, track, kind) else {
                    continue;
                };
                self.live_note(&p, track, kind);
            }
        }
        changed
    }
}

impl Drop for LiveMidi {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            let _ = crate::midi_open::retire(input);
        }
        if let Some(opener) = self.opener.take() {
            let _ = xirius_session::background::Job::spawn("midi-opener-retire", move || {
                drop(opener);
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::RowKind;
    use xirius_core::model::TrackKind;
    use xirius_engine::record::MockMidi;

    /// With 「インストゥルメントの入力を選択に合わせる」 on, choosing the track
    /// arms it and the note goes there; off, an unarmed track hears nothing and
    /// an armed one is the destination selected or not (REC-024).
    #[test]
    fn the_armed_instrument_track_takes_the_notes() {
        for follow in [true, false] {
            let mut s = Studio::sample();
            let p = s.project();
            let keys = p
                .tracks
                .values()
                .find(|t| t.kind == TrackKind::Instrument && t.input.is_none())
                .map(|t| t.id)
                .expect("an instrument track in the sample");
            let others: Vec<String> = p
                .tracks
                .values()
                .filter(|t| t.armed)
                .map(|t| t.id.to_string())
                .collect();
            if !others.is_empty() {
                s.song
                    .run("track.arm", serde_json::json!({"ids": others, "off": true}))
                    .unwrap();
            }
            assert!(!s.project().tracks[&keys].armed);
            assert!(
                s.project()
                    .tracks
                    .values()
                    .all(|t| !t.armed || t.kind != TrackKind::Instrument)
            );
            s.rec_set(
                "prefs.set",
                serde_json::json!({"key": "midi.follow-selection", "value": follow.to_string()}),
            );
            s.select_row(Some(RowKind::Track(keys)));
            assert_eq!(s.project().tracks[&keys].armed, follow, "follow {follow}");
            s.live_midi_input(Box::new(MockMidi::with_ports(
                vec!["Keys".into()],
                vec![(0, 0, [0x90, 60, 100])],
            )));
            s.poll_live_midi();
            let want = if follow { vec![keys] } else { Vec::new() };
            assert_eq!(s.live_midi.routed.get(&0), Some(&want), "follow {follow}");
        }
    }

    /// A track's value changed while a key is held: the off goes to the pitch that was really
    /// sent, not to the one the new value would give (UI-312).
    #[test]
    fn a_held_midi_note_is_let_go_at_the_pitch_that_was_sent() {
        let mut s = Studio::sample();
        let keys = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Instrument && t.input.is_none())
            .map(|t| t.id)
            .expect("an instrument track");
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "true"}),
        );
        s.select_row(Some(RowKind::Track(keys)));
        s.live_midi_input(Box::new(MockMidi::with_ports(
            vec!["Keys".into()],
            vec![(0, 0, [0x90, 60, 100]), (300, 0, [0x80, 60, 0])],
        )));
        s.poll_live_midi();
        assert!(
            s.live_midi
                .sent
                .iter()
                .any(|(t, k)| *t == keys && matches!(k, NoteKind::On { pitch: 60, .. })),
            "{:?}",
            s.live_midi.sent
        );
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [keys.to_string()], "semis": 12}),
            )
            .unwrap();
        s.poll_live_midi();
        assert!(
            s.live_midi
                .sent
                .contains(&(keys, NoteKind::Off { pitch: 60 })),
            "{:?}",
            s.live_midi.sent
        );
        assert!(
            !s.live_midi
                .sent
                .iter()
                .any(|(_, k)| matches!(k, NoteKind::Off { pitch: 72 })),
            "the off is not transposed"
        );
    }

    /// Two armed tracks, one of which cannot play the key (its transpose takes it off the
    /// keyboard): the other keeps sounding while the key is held. Only the tracks a note was
    /// sent to are compared, not only the ones that sounded (UI-312).
    #[test]
    fn an_armed_track_that_cannot_play_the_key_does_not_cut_the_other() {
        let mut s = Studio::sample();
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
        );
        let insts: Vec<TrackId> = s
            .project()
            .tracks
            .values()
            .filter(|t| t.kind == TrackKind::Instrument)
            .map(|t| t.id)
            .take(2)
            .collect();
        assert_eq!(insts.len(), 2, "two instrument tracks in the sample");
        let (a, b) = (insts[0], insts[1]);
        s.song
            .run(
                "track.arm",
                serde_json::json!({"ids": [a.to_string(), b.to_string()], "on": true}),
            )
            .unwrap();
        // B's transpose takes 60 off the keyboard; A plays it.
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [b.to_string()], "semis": -64}),
            )
            .unwrap();
        s.live_midi_input(Box::new(MockMidi::with_ports(
            vec!["Keys".into()],
            vec![(0, 0, [0x90, 60, 100])],
        )));
        s.poll_live_midi();
        assert!(
            s.live_midi
                .sent
                .iter()
                .any(|(t, k)| *t == a && matches!(k, NoteKind::On { .. })),
            "{:?}",
            s.live_midi.sent
        );
        let before = s.live_midi.sent.len();
        // The key stays down: a poll with nothing new must not let the sounding track go.
        s.poll_live_midi();
        assert!(
            !s.live_midi.sent[before..]
                .iter()
                .any(|(_, k)| matches!(k, NoteKind::Off { .. })),
            "{:?}",
            s.live_midi.sent
        );
    }

    /// A value that takes a held key off the keyboard stops that note now (UI-312).
    #[test]
    fn a_value_that_takes_a_held_key_off_the_keyboard_stops_it() {
        let mut s = Studio::sample();
        let keys = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Instrument && t.input.is_none())
            .map(|t| t.id)
            .expect("an instrument track");
        s.rec_set(
            "prefs.set",
            serde_json::json!({"key": "midi.follow-selection", "value": "true"}),
        );
        s.select_row(Some(RowKind::Track(keys)));
        s.live_midi_input(Box::new(MockMidi::with_ports(
            vec!["Keys".into()],
            vec![(0, 0, [0x90, 60, 100])],
        )));
        s.poll_live_midi();
        s.song
            .run(
                "track.transpose",
                serde_json::json!({"ids": [keys.to_string()], "semis": -64}),
            )
            .unwrap();
        s.poll_live_midi();
        assert!(
            s.live_midi
                .sent
                .contains(&(keys, NoteKind::Off { pitch: 60 })),
            "{:?}",
            s.live_midi.sent
        );
    }

    /// The follow-arm rides on the song's state: no step of the history is made
    /// for it (REC-024).
    #[test]
    fn following_the_selection_leaves_no_step_and_turns_the_arm_back() {
        let mut s = Studio::new(crate::song::Song::empty());
        let add = |s: &mut Studio, name: &str| -> TrackId {
            s.song
                .run(
                    "track.add.with-instrument",
                    serde_json::json!({"instrument": "Sampler", "name": name}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        };
        let (a, b) = (add(&mut s, "A"), add(&mut s, "B"));
        let steps = s.song.steps();
        s.select_row(Some(RowKind::Track(a)));
        s.select_row(Some(RowKind::Track(b)));
        let p = s.project();
        assert!(!p.tracks[&a].armed && !p.tracks[&a].monitor);
        assert!(p.tracks[&b].armed && p.tracks[&b].monitor);
        assert_eq!(s.song.steps(), steps, "no history step");
    }

    /// Undo and redo land on states the cursor saved, whose arms can be an
    /// older selection's; the input follows again, so the selected track alone
    /// stays armed (REC-024).
    #[test]
    fn undo_and_redo_keep_the_selected_track_armed() {
        let mut s = Studio::new(crate::song::Song::empty());
        let add = |s: &mut Studio, name: &str| -> TrackId {
            s.song
                .run(
                    "track.add.with-instrument",
                    serde_json::json!({"instrument": "Sampler", "name": name}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        };
        let (a, b) = (add(&mut s, "A"), add(&mut s, "B"));
        s.select_row(Some(RowKind::Track(a)));
        assert!(s.project().tracks[&a].armed);
        // An edit after the arm: undoing it lands on the A-armed state.
        s.song
            .run("track.add.audio", serde_json::json!({"name": "Vocal"}))
            .unwrap();
        s.select_row(Some(RowKind::Track(b)));
        let steps = s.song.steps();
        let g = s.geometry(1440.0, 820.0);
        assert!(s.undo(&g));
        let p = s.project();
        assert!(p.tracks[&b].armed && !p.tracks[&a].armed, "followed again");
        assert_eq!(s.song.steps(), steps, "still no step for the follow");
        assert!(s.redo(&g));
        let p = s.project();
        assert!(p.tracks[&b].armed && !p.tracks[&a].armed);
        assert_eq!(s.song.steps(), steps);
    }

    /// While a take records, moving the selection moves no arms; once it has
    /// stopped, the next move puts the arms with the selection (REC-024).
    #[test]
    fn the_selection_does_not_follow_while_recording() {
        let mut s = Studio::new(crate::song::Song::empty());
        let a: TrackId = s
            .song
            .run(
                "track.add.with-instrument",
                serde_json::json!({"instrument": "Sampler", "name": "A", "arm": true}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let b: TrackId = s
            .song
            .run(
                "track.add.with-instrument",
                serde_json::json!({"instrument": "Sampler", "name": "B"}),
            )
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        s.transport.recording = true;
        s.select_row(Some(RowKind::Track(b)));
        let p = s.project();
        assert!(
            p.tracks[&a].armed && !p.tracks[&b].armed,
            "no mid-take switch"
        );
        s.transport.recording = false;
        s.select_row(Some(RowKind::Track(b)));
        let p = s.project();
        assert!(p.tracks[&b].armed && !p.tracks[&a].armed);
    }
}
