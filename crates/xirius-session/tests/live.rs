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

/// Gives the track's Sampler a second of a sine to play (it sounds only with a file).
fn tone(s: &mut Session, t: TrackId, dir: &std::path::Path) {
    let x: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    let file = dir.join("tone_C4.wav");
    xirius_session::export::write_wav(&file, &x, &x, 48_000, 16).unwrap();
    let id = s.project().tracks[&t].inserts[0].id.to_string();
    s.run_value(
        "plugin.samples.add",
        json!({ "id": id, "file": file.display().to_string() }),
        &RunOptions::human(),
    )
    .unwrap();
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
    let tmp = tempfile::Builder::new()
        .prefix("xirius-live-")
        .tempdir()
        .unwrap();
    tone(&mut s, t, tmp.path());
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

#[test]
fn held_sampler_survives_an_unrelated_track_graph_change_in_audio_and_snapshot() {
    for multi in [false, true] {
        held_graph_change(multi);
    }
}

fn held_graph_change(multi: bool) {
    use std::time::{Duration, Instant};
    use xirius_engine::performance::{Input, midi_source};
    let mut s = song();
    let t = add(&mut s, json!({ "instrument": "Sampler", "arm": true }));
    let tmp = tempfile::Builder::new()
        .prefix("xirius-held-graph-")
        .tempdir()
        .unwrap();
    tone(&mut s, t, tmp.path());
    if multi {
        s.run_value(
            "rack.layer.add",
            json!({ "track": t, "name": "Sampler" }),
            &RunOptions::human(),
        )
        .unwrap();
    }
    let id = s.project().tracks[&t].inserts[0].id;
    let (mut host, mut audio, warnings) =
        SessionHost::start(s.project(), HostConfig::default()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let input = |kind| Input {
        source: midi_source(0, 2),
        plugin: None,
        kind,
    };
    assert!(host.live_input(
        t,
        input(NoteKind::On {
            pitch: 60,
            velocity: 0.8
        })
    ));
    assert!(peak(&mut audio, 480) > 0.01);
    assert_eq!(host.plugins().pressed_keys(id), vec![60]);
    if multi {
        assert_eq!(host.plugins().multi_pressed_keys(t), vec![60]);
    }
    add(&mut s, json!({ "instrument": "Sampler" }));
    host.set_windowed();
    host.sync_later(s.project());
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.swap_pending() {
        assert_eq!(
            host.plugins().pressed_keys(id),
            vec![60],
            "the accepted graph remains visible while building"
        );
        host.step_swap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        peak(&mut audio, 480) > 0.01,
        "the kept instrument continues sounding after the actual swap"
    );
    assert_eq!(host.plugins().pressed_keys(id), vec![60]);
    if multi {
        assert_eq!(host.plugins().multi_pressed_keys(t), vec![60]);
    }
    assert!(host.live_input(t, input(NoteKind::Off { pitch: 60 })));
    peak(&mut audio, 48_000);
    assert!(peak(&mut audio, 480) < 1e-4);
    assert!(host.plugins().pressed_keys(id).is_empty());
    if multi {
        assert!(host.plugins().multi_pressed_keys(t).is_empty());
    }
}

#[test]
fn mouse_release_survives_host_command_pressure_without_stopping_midi() {
    use xirius_engine::Command;
    use xirius_engine::performance::{Input, MOUSE, midi_source};
    for multi in [false, true] {
        for midi in [false, true] {
            let mut s = song();
            let t = add(&mut s, json!({"instrument":"Sampler"}));
            let tmp = tempfile::Builder::new()
                .prefix("xirius-release-")
                .tempdir()
                .unwrap();
            tone(&mut s, t, tmp.path());
            let id = s.project().tracks[&t].inserts[0].id;
            if multi {
                s.run_value(
                    "rack.layer.add",
                    json!({"track":t,"name":"Sampler"}),
                    &RunOptions::human(),
                )
                .unwrap();
            }
            let (mut host, mut audio, _) =
                SessionHost::start(s.project(), HostConfig::default()).unwrap();
            let mouse = |kind| Input {
                source: MOUSE | id.0,
                plugin: if multi { None } else { Some(id.0) },
                kind,
            };
            let on = NoteKind::On {
                pitch: 60,
                velocity: 0.8,
            };
            assert!(host.live_input(t, mouse(on)));
            if midi {
                assert!(host.live_input(
                    t,
                    Input {
                        source: midi_source(0, 0),
                        plugin: None,
                        kind: on
                    }
                ));
            }
            assert!(peak(&mut audio, 480) > 0.01);
            let mut queued = 0;
            while host.controller().send(Command::MasterGain(1.0)).is_ok() {
                queued += 1;
            }
            assert!(queued >= 1024);
            assert!(host.live_input(t, mouse(NoteKind::Off { pitch: 60 })));
            assert!(
                !host.live_input(t, mouse(on)),
                "a new on must not overtake its release"
            );
            let other = add(&mut s, json!({"instrument":"Sampler"}));
            s.run_value(
                "track.move",
                json!({"ids":[other],"by":-1}),
                &RunOptions::human(),
            )
            .unwrap();
            host.set_windowed();
            host.sync_later(s.project());
            host.step_swap();
            peak(&mut audio, 480);
            host.poll();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while host.swap_pending() {
                host.step_swap();
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            peak(&mut audio, 24000);
            let heard = peak(&mut audio, 480);
            if midi {
                assert!(heard > 0.01, "another source remains audible");
                assert_eq!(host.plugins().pressed_keys(id), vec![60]);
                assert!(host.live_input(
                    t,
                    Input {
                        source: midi_source(0, 0),
                        plugin: None,
                        kind: NoteKind::Off { pitch: 60 }
                    }
                ));
                peak(&mut audio, 24000);
            } else {
                assert!(heard < 1e-4, "queued mouse release was delivered: {heard}");
            }
            assert!(peak(&mut audio, 480) < 1e-4);
            assert!(
                host.live_input(t, mouse(on)),
                "the reserved release slot is reusable"
            );
            assert!(peak(&mut audio, 480) > 0.01);
            assert!(host.live_input(t, mouse(NoteKind::Off { pitch: 60 })));
        }
    }
}
