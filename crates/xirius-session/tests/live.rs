//! Notes played by hand (UI-074): the engine is never given a device; the test pulls the audio.

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_engine::NoteKind;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};

fn song() -> Session {
    let mut s = Session::with_registry(Project::new("live"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn add(s: &mut Session, args: Value) -> TrackId {
    let Value::Object(args) = args else {
        unreachable!()
    };
    let r = commands::run(
        s,
        None,
        "track.add.with-instrument",
        &args,
        &RunOptions::human(),
    )
    .unwrap();
    r.data["id"].as_str().unwrap().parse().unwrap()
}

fn peak(audio: &mut xirius_engine::AudioEngine, frames: usize) -> f32 {
    let mut out = vec![0.0f32; 480 * 2];
    let mut loudest = 0.0f32;
    for _ in 0..frames / 480 {
        audio.render(&mut out, 2);
        loudest = out.iter().fold(loudest, |m, x| m.max(x.abs()));
    }
    loudest
}

#[test]
fn a_note_played_by_hand_sounds_on_the_tracks_instrument_until_let_go() {
    let mut s = song();
    let t = add(&mut s, json!({ "instrument": "Sampler", "arm": true }));
    let (mut host, mut audio, warnings) =
        SessionHost::start(s.project(), HostConfig::default()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(peak(&mut audio, 4800), 0.0, "silent before a key");
    assert!(host.note(
        t,
        NoteKind::On {
            pitch: 60,
            velocity: 0.8
        }
    ));
    assert!(
        peak(&mut audio, 4800) > 0.01,
        "the sampler sounds while stopped"
    );
    assert!(host.note(t, NoteKind::Off { pitch: 60 }));
    peak(&mut audio, 48_000);
    assert!(peak(&mut audio, 4800) < 1e-4, "quiet once let go");
}

#[test]
fn a_track_the_engine_does_not_have_takes_no_note() {
    let mut s = song();
    add(&mut s, json!({ "instrument": "Sampler" }));
    let (mut host, _audio, _) = SessionHost::start(s.project(), HostConfig::default()).unwrap();
    assert!(!host.note(TrackId(999_999), NoteKind::Off { pitch: 60 }));
}
