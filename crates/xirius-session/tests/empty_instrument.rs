//! Empty sources use the same silent graph for timeline, audition and MIDI capture.
//! No windows or audio devices are opened.
use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_engine::record::MockMidi;
use xirius_engine::{AudioEngine, NoteKind};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::record::RecordOptions;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    commands::run(
        s,
        None,
        name,
        args.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap()
    .data
}

fn song(instrument: Option<&str>) -> (Session, TrackId) {
    let mut s = Session::with_registry(Project::new("empty"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    let r = match instrument {
        Some(name) => run(
            &mut s,
            "track.add.with-instrument",
            json!({"instrument": name}),
        ),
        None => run(&mut s, "track.add.instrument", json!({})),
    };
    let t = r["id"].as_str().unwrap().parse().unwrap();
    run(&mut s, "track.arm", json!({"ids": [t], "on": true}));
    (s, t)
}

fn pull(host: &mut SessionHost, audio: &mut AudioEngine) -> f32 {
    let mut peak = 0.0f32;
    for _ in 0..100 {
        let mut out = [123.0; 960];
        audio.render(&mut out, 2);
        peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        host.poll();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    peak
}

#[test]
fn empty_sources_are_silent_on_playback_audition_and_recorded_midi() {
    for instrument in [None, Some("Sampler"), Some("Drum Kit")] {
        let (mut s, t) = song(instrument);
        assert!(
            s.project().tracks[&t]
                .inserts
                .iter()
                .all(|x| x.samples.is_empty())
        );
        let c = run(
            &mut s,
            "clip.add.midi",
            json!({"track": t, "at": 0, "bars": 1}),
        );
        run(
            &mut s,
            "note.add",
            json!({"clip": c["id"], "at": 0, "pitch": 36, "length": 960, "velocity": 127}),
        );
        let (mut host, mut audio, warnings) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        // Keyboard and piano-roll audition enter through SessionHost::note.
        assert!(host.note(
            t,
            NoteKind::On {
                pitch: 60,
                velocity: 1.0
            }
        ));
        assert_eq!(pull(&mut host, &mut audio), 0.0, "audition: {instrument:?}");
        host.note(t, NoteKind::Off { pitch: 60 });
        host.play();
        assert_eq!(pull(&mut host, &mut audio), 0.0, "timeline: {instrument:?}");
        host.stop();
        host.record_start(
            s.project(),
            None,
            RecordOptions {
                at: Some(0),
                midi: Some(Box::new(MockMidi::new(vec![
                    (0, [0x90, 36, 127]),
                    (24000, [0x80, 36, 0]),
                ]))),
                ..RecordOptions::default()
            },
        )
        .unwrap();
        assert_eq!(pull(&mut host, &mut audio), 0.0, "MIDI: {instrument:?}");
        assert!(!host.live_notes().is_empty(), "MIDI input was consumed");
        let levels = host.take_levels();
        assert!(!levels.is_empty());
        assert!(
            levels
                .values()
                .all(|v| v.peak == [0.0; 2] && v.rms == [0.0; 2])
        );
        host.record_stop();
    }
}

#[test]
fn a_saved_pad_keeps_its_sound_and_empty_pads_stay_silent() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("hit.wav");
    let samples = vec![0.25; 48000];
    xirius_session::export::write_wav(&file, &samples, &samples, 48000, 32).unwrap();
    let (mut s, t) = song(Some("Drum Kit"));
    let id = s.project().tracks[&t].inserts[0].id;
    run(
        &mut s,
        "plugin.samples.add",
        json!({"id": id, "file": file, "pad": "B5"}),
    );
    s.save_as(tmp.path().join("song")).unwrap();
    let loaded = Session::open_with(tmp.path().join("song"), shared_registry()).unwrap();
    assert!(!loaded.is_dirty());
    assert_eq!(
        loaded.project().tracks[&t].inserts[0].params,
        s.project().tracks[&t].inserts[0].params
    );
    let (mut host, mut audio, warnings) = SessionHost::start(
        loaded.project(),
        HostConfig {
            dir: loaded.dir().map(std::path::Path::to_path_buf),
            ..HostConfig::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    host.note(
        t,
        NoteKind::On {
            pitch: 37,
            velocity: 1.0,
        },
    );
    assert_eq!(pull(&mut host, &mut audio), 0.0, "unassigned pad");
    host.note(
        t,
        NoteKind::On {
            pitch: 56,
            velocity: 1.0,
        },
    );
    assert!(pull(&mut host, &mut audio) > 0.01, "saved assigned pad");
}

#[test]
fn a_saved_empty_source_stays_empty_without_becoming_dirty() {
    for instrument in [None, Some("Sampler"), Some("Drum Kit")] {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, t) = song(instrument);
        s.save_as(tmp.path().join("song")).unwrap();
        let loaded = Session::open_with(tmp.path().join("song"), shared_registry()).unwrap();
        assert!(!loaded.is_dirty());
        assert_eq!(
            loaded.project().tracks[&t].inserts,
            s.project().tracks[&t].inserts
        );
        let (mut host, mut audio, _) =
            SessionHost::start(loaded.project(), HostConfig::default()).unwrap();
        host.note(
            t,
            NoteKind::On {
                pitch: 36,
                velocity: 1.0,
            },
        );
        assert_eq!(pull(&mut host, &mut audio), 0.0);
    }
}

#[test]
fn empty_multi_and_layers_with_empty_samplers_are_silent() {
    use std::sync::Arc;
    use xirius_core::id::NodeId;
    use xirius_core::rack::{Rack, Source};
    let (s, t) = song(None);
    let mut p = s.project().clone();
    let id = NodeId(p.alloc());
    Arc::make_mut(p.tracks.get_mut(&t).unwrap()).rack = Some(Rack {
        source: Some(Source::Multi {
            id,
            layers: Vec::new(),
        }),
        fx: Vec::new(),
    });
    let (mut host, mut audio, _) = SessionHost::start(&p, HostConfig::default()).unwrap();
    host.note(
        t,
        NoteKind::On {
            pitch: 60,
            velocity: 1.0,
        },
    );
    host.play();
    assert_eq!(pull(&mut host, &mut audio), 0.0);
    let (mut s, t) = song(Some("Sampler"));
    run(
        &mut s,
        "rack.layer.add",
        json!({"track": t, "name": "Drum Kit"}),
    );
    let (mut host, mut audio, _) = SessionHost::start(s.project(), HostConfig::default()).unwrap();
    host.note(
        t,
        NoteKind::On {
            pitch: 36,
            velocity: 1.0,
        },
    );
    host.play();
    assert_eq!(pull(&mut host, &mut audio), 0.0);
}

#[test]
fn removing_the_last_sample_does_not_restore_a_substitute_sound() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("tone_C3.wav");
    let samples = vec![0.25; 48000];
    xirius_session::export::write_wav(&file, &samples, &samples, 48000, 32).unwrap();
    for instrument in ["Sampler", "Drum Kit"] {
        let (mut s, t) = song(Some(instrument));
        let id = s.project().tracks[&t].inserts[0].id;
        let (mut host, mut audio, _) =
            SessionHost::start(s.project(), HostConfig::default()).unwrap();
        run(
            &mut s,
            "plugin.samples.add",
            json!({"id": id, "file": file}),
        );
        host.sync(s.project());
        pull(&mut host, &mut audio);
        host.note(
            t,
            NoteKind::On {
                pitch: 36,
                velocity: 1.0,
            },
        );
        assert!(pull(&mut host, &mut audio) > 0.0);
        run(
            &mut s,
            "plugin.samples.remove",
            json!({"id": id, "index": 1}),
        );
        host.sync(s.project());
        pull(&mut host, &mut audio);
        host.note(
            t,
            NoteKind::On {
                pitch: 36,
                velocity: 1.0,
            },
        );
        assert_eq!(pull(&mut host, &mut audio), 0.0);
    }
}
