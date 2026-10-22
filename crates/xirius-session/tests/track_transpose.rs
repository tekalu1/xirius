//! A track's transpose and velocity reach the sound (UI-312): the notes are added to when the
//! track is built for playback, bounce and freeze, and when a MIDI file is written; a note that
//! would leave the keyboard is not played. Nothing here opens an audio device.

use std::sync::Arc;

use xirius_core::id::{ClipId, Node, NoteId, TrackId};
use xirius_core::model::{Clip, ClipBody, Note, Project, Track, TrackKind};
use xirius_engine::NoteKind;
use xirius_engine::record::MockMidi;
use xirius_session::build::Builder;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::record::{Input, RecordOptions};
use xirius_session::tempo::TempoMap;

fn hand_song(clip_transpose: i8, transpose: i8, velocity: i8) -> (Project, TrackId) {
    let mut p = Project::new("tracks");
    let tid = TrackId(p.alloc());
    let mut t = Track::new(tid, TrackKind::Instrument, "Keys");
    t.transpose = transpose;
    t.velocity = velocity;
    let notes: Vec<Note> = [(0, 60), (480, 64), (960, 67)]
        .into_iter()
        .map(|(start, pitch)| Note {
            id: NoteId(p.alloc()),
            start,
            length: 240,
            pitch,
            velocity: 100,
            muted: false,
            ai: false,
        })
        .collect();
    t.clips.push(Clip {
        id: ClipId(p.alloc()),
        name: "Keys 1".into(),
        color: None,
        start: 0,
        length: 1920,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Midi {
            notes: Arc::new(notes),
            transpose: clip_transpose,
        },
    });
    p.tracks.insert(tid, Arc::new(t));
    (p, tid)
}

/// The notes the track plays, in frame order: (pitch, velocity of 127).
fn played(p: &Project, track: TrackId) -> Vec<(u8, u8)> {
    let rate = p.settings.sample_rate;
    let tmap = TempoMap::from_project(p);
    let t = &p.tracks[&track];
    let tl = Builder::new(rate, None).timeline(p, t, &tmap, &mut Vec::new());
    tl.notes()
        .iter()
        .filter_map(|n| match n.kind {
            NoteKind::On { pitch, velocity } => Some((pitch, (velocity * 127.0).round() as u8)),
            NoteKind::Off { .. } | NoteKind::Control { .. } => None,
        })
        .collect()
}

#[test]
fn the_tracks_transpose_is_added_to_the_clips_and_the_notes_stay_as_written() {
    let (p, t) = hand_song(2, 3, 0);
    let pitches: Vec<u8> = played(&p, t).into_iter().map(|n| n.0).collect();
    assert_eq!(pitches, [65, 69, 72], "2 + 3 above the notes");
    // The clip's notes did not move.
    assert_eq!(
        p.tracks[&t].clips[0]
            .notes()
            .iter()
            .map(|n| n.pitch)
            .collect::<Vec<_>>(),
        [60, 64, 67]
    );
    // 0 on the track plays the clip's value alone.
    let (p, t) = hand_song(2, 0, 0);
    let pitches: Vec<u8> = played(&p, t).into_iter().map(|n| n.0).collect();
    assert_eq!(pitches, [62, 66, 69]);
}

#[test]
fn a_note_the_two_transposes_take_off_the_keyboard_is_not_played() {
    let (p, t) = hand_song(24, 64, 0);
    // 60 + 24 + 64 = 148 is off the keyboard; 64 and 67 are too.
    assert!(played(&p, t).is_empty());
    let (p, t) = hand_song(0, -64, 0);
    // 60 - 64 = -4, 64 - 64 = 0, 67 - 64 = 3: only the last two are on the keyboard.
    let pitches: Vec<u8> = played(&p, t).into_iter().map(|n| n.0).collect();
    assert_eq!(pitches, [0, 3]);
}

#[test]
fn the_tracks_velocity_is_added_to_the_notes_inside_the_keyboard() {
    let (p, t) = hand_song(0, 0, 0);
    assert_eq!(
        played(&p, t).into_iter().map(|n| n.1).collect::<Vec<_>>(),
        [100, 100, 100]
    );
    // +10% of 100 is 110; -100% is held at 1 rather than silence.
    let (p, t) = hand_song(0, 0, 10);
    assert_eq!(
        played(&p, t).into_iter().map(|n| n.1).collect::<Vec<_>>(),
        [110, 110, 110]
    );
    let (p, t) = hand_song(0, 0, -100);
    assert_eq!(
        played(&p, t).into_iter().map(|n| n.1).collect::<Vec<_>>(),
        [1, 1, 1]
    );
}

#[test]
fn a_midi_file_is_written_at_the_height_and_velocity_the_track_plays() {
    let (p, t) = hand_song(2, 3, 10);
    let (bytes, n) =
        xirius_session::midi_export::file_bytes(&p, (0, 1920), &[t]).expect("a MIDI file");
    assert_eq!(n, 3);
    let mut notes = xirius_session::smf::read(&bytes, p.ppq).expect("readable");
    notes.sort_by_key(|n| n.pitch);
    assert_eq!(
        notes.iter().map(|n| n.pitch).collect::<Vec<_>>(),
        [65, 69, 72]
    );
    assert!(notes.iter().all(|n| n.velocity == 110), "{notes:?}");
}

/// A value changed after the mix was built reaches the engine that is already playing: the
/// timeline's "same" test sees the track's transpose and velocity (UI-312).
#[test]
fn a_value_changed_after_the_host_starts_is_sent_to_the_running_engine() {
    let (p, t) = hand_song(0, 0, 0);
    let (mut host, _audio, _) = SessionHost::start(&p, HostConfig::default()).expect("a host");
    let same = host.sync(&p);
    assert!(!same.swapped && same.commands == 0, "{same:?}");
    let mut changed = p.clone();
    let mut track = (*changed.tracks[&t]).clone();
    track.transpose = 12;
    track.velocity = 10;
    changed.tracks.insert(t, Arc::new(track));
    let report = host.sync(&changed);
    assert!(!report.swapped, "a value alone does not make a new mix");
    assert!(
        report.commands >= 1,
        "the timeline is sent again: {report:?}"
    );
    assert_eq!(report.deferred, 0);
}

/// While a take records, a value changed in the song goes to the monitor's Thru; with no take
/// running nothing is sent (UI-312).
#[test]
fn a_value_changed_while_recording_reaches_the_monitor() {
    let (p, t) = hand_song(0, 0, 0);
    let mut armed = p.clone();
    let mut track = (*armed.tracks[&t]).clone();
    track.armed = true;
    armed.tracks.insert(t, Arc::new(track));
    armed.root.push(Node::Track(t));
    let (mut host, _audio, _) = SessionHost::start(&armed, HostConfig::default()).expect("a host");

    let mut changed = armed.clone();
    let mut track = (*changed.tracks[&t]).clone();
    track.transpose = 5;
    changed.tracks.insert(t, Arc::new(track));
    let report = host.sync(&changed);
    assert!(!report.thru, "no recording, no monitor: {report:?}");

    let started = host
        .record_start(
            &changed,
            None,
            RecordOptions {
                input: Input::None,
                midi: Some(Box::new(MockMidi::with_ports(
                    vec!["Keys".into()],
                    Vec::new(),
                ))),
                ..Default::default()
            },
        )
        .expect("a take");
    assert!(started.midi_tracks.contains(&t), "{started:?}");

    let mut again = changed.clone();
    let mut track = (*again.tracks[&t]).clone();
    track.transpose = 7;
    track.velocity = 10;
    again.tracks.insert(t, Arc::new(track));
    let report = host.sync(&again);
    assert!(report.thru, "the monitor gets the new values: {report:?}");
    let report = host.sync(&again);
    assert!(!report.thru, "nothing changed, nothing sent: {report:?}");

    // A new mix (a track added) is built: the monitor's tracks are made again from the new
    // graph, and the value changed at the same time still reaches it (UI-312).
    let mut restructured = again.clone();
    let new = TrackId(restructured.alloc());
    restructured
        .tracks
        .insert(new, Arc::new(Track::new(new, TrackKind::Instrument, "New")));
    restructured.root.push(Node::Track(new));
    let mut track = (*restructured.tracks[&t]).clone();
    track.transpose = 9;
    restructured.tracks.insert(t, Arc::new(track));
    let report = host.sync(&restructured);
    assert!(report.swapped, "the new track made a new mix: {report:?}");
    assert!(
        report.thru,
        "the monitor follows after the swap: {report:?}"
    );
    host.record_stop();
}
