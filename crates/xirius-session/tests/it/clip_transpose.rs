//! A MIDI clip's transpose reaches the sound (UI-311): the notes are added to when the clip is
//! built for playback, bounce and freeze, and when a MIDI file is written; a note that would
//! leave the keyboard is not played. Nothing here opens an audio device.

use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::id::{ClipId, NoteId, TrackId};
use xirius_core::model::{Clip, ClipBody, Note, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::NoteKind;
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::tempo::TempoMap;

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn song(name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    run(&mut s, "tempo.set", json!({ "bpm": 120 }));
    s
}

/// An instrument track with a clip of the notes 60, 64 and 67, and the ids of both.
fn midi(s: &mut Session) -> (String, String) {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": "Lead" })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1 }),
    ));
    for (at, pitch) in [(0, 60), (480, 64), (960, 67)] {
        run(
            s,
            "note.add",
            json!({ "clip": c, "at": at, "length": 240, "pitch": pitch }),
        );
    }
    (t, c)
}

/// The pitches the song's instrument track plays, in frame order.
fn played(s: &Session, track: &str) -> Vec<u8> {
    let p = s.project();
    let rate = p.settings.sample_rate;
    let tmap = TempoMap::from_project(p);
    let t = &p.tracks[&track.parse().unwrap()];
    let tl = Builder::new(rate, None).timeline(p, t, &tmap, &mut Vec::new());
    tl.notes()
        .iter()
        .filter_map(|n| match n.kind {
            NoteKind::On { pitch, .. } => Some(pitch),
            NoteKind::Off { .. } | NoteKind::Control { .. } => None,
        })
        .collect()
}

#[test]
fn the_clip_value_is_added_when_the_notes_play_and_zero_plays_them_as_written() {
    let mut s = song("play");
    let (t, c) = midi(&mut s);
    assert_eq!(played(&s, &t), [60, 64, 67]);
    run(&mut s, "clip.transpose", json!({ "ids": [c], "semis": 2 }));
    assert_eq!(played(&s, &t), [62, 66, 69]);
    // The clip's notes are still the ones written.
    assert_eq!(
        s.project().tracks[&t.parse().unwrap()].clips[0]
            .notes()
            .iter()
            .map(|n| n.pitch)
            .collect::<Vec<_>>(),
        [60, 64, 67]
    );
    run(&mut s, "clip.transpose", json!({ "ids": [c], "semis": 0 }));
    assert_eq!(played(&s, &t), [60, 64, 67]);
    // Writing the value into the notes keeps the sound, now at the notes themselves.
    run(&mut s, "clip.transpose", json!({ "ids": [c], "semis": -5 }));
    assert_eq!(played(&s, &t), [55, 59, 62]);
    run(&mut s, "clip.transpose.write", json!({ "ids": [c] }));
    assert_eq!(played(&s, &t), [55, 59, 62]);
}

#[test]
fn a_note_taken_off_the_keyboard_is_not_played() {
    let mut s = song("off-keyboard");
    let (t, c) = midi(&mut s);
    run(
        &mut s,
        "note.add",
        json!({ "clip": c, "at": 1440, "length": 240, "pitch": 120 }),
    );
    assert_eq!(played(&s, &t), [60, 64, 67, 120]);
    run(&mut s, "clip.transpose", json!({ "ids": [c], "semis": 12 }));
    assert_eq!(played(&s, &t), [72, 76, 79]);
    run(&mut s, "clip.transpose", json!({ "ids": [c], "semis": 24 }));
    assert_eq!(played(&s, &t), [84, 88, 91], "144 is off the keyboard");
}

/// A song built by hand: one instrument track with a MIDI clip of the notes 60, 64 and 67, at
/// `transpose`.
fn hand_song(transpose: i8) -> (Project, TrackId) {
    let mut p = Project::new("midi");
    let tid = TrackId(p.alloc());
    let mut t = Track::new(tid, TrackKind::Instrument, "Keys");
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
            transpose,
        },
    });
    p.tracks.insert(tid, Arc::new(t));
    (p, tid)
}

#[test]
fn a_midi_file_is_written_at_the_height_the_clip_plays() {
    let (p, t) = hand_song(2);
    let (bytes, n) =
        xirius_session::midi_export::file_bytes(&p, (0, 1920), &[t]).expect("a MIDI file");
    assert_eq!(n, 3);
    let mut notes: Vec<u8> = xirius_session::smf::read(&bytes, p.ppq)
        .expect("readable")
        .into_iter()
        .map(|n| n.pitch)
        .collect();
    notes.sort();
    assert_eq!(notes, [62, 66, 69]);
    // One that the value would take off the keyboard is left out, as it is not played.
    let (mut p, t) = hand_song(12);
    let track = Arc::make_mut(p.tracks.get_mut(&t).unwrap());
    if let ClipBody::Midi { notes, .. } = &mut track.clips[0].body {
        Arc::make_mut(notes)[2].pitch = 120;
    }
    let (bytes, n) =
        xirius_session::midi_export::file_bytes(&p, (0, 1920), &[t]).expect("a MIDI file");
    assert_eq!(n, 2);
    let mut pitches: Vec<u8> = xirius_session::smf::read(&bytes, p.ppq)
        .expect("readable")
        .into_iter()
        .map(|n| n.pitch)
        .collect();
    pitches.sort();
    assert_eq!(pitches, [72, 76], "the note off the keyboard is left out");
}
