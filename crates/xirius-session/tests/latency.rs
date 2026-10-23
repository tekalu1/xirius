//! A real VST3 restartComponent(kLatencyChanged) rebuilds a playing session's PDC.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{ClipId, Node, PluginId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind,
};
use xirius_engine::{
    Command,
    server::{DEFAULT_LATENCY, find_host},
};
use xirius_plugins::{Catalog, probe};
use xirius_session::{
    host::{HostConfig, SessionHost},
    plugins::Isolation,
};

#[test]
fn a_latency_notification_rebuilds_pdc_while_the_transport_keeps_playing() {
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = deps.join("xirius_test_vst3.dll");
    assert!(
        dll.exists(),
        "the fixture's cdylib is built with its dev dependency"
    );
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let dir = tempfile::Builder::new()
        .prefix("xirius-latency-input-")
        .tempdir()
        .unwrap();
    let path = dir.path().join("input.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..8000 {
        let value = if i == 4096 { 1.0 } else { 0.0 };
        writer.write_sample(value).unwrap();
        writer.write_sample(value).unwrap();
    }
    writer.finalize().unwrap();
    let pid = PluginId(200);
    for (remote, scenario) in [
        (false, 0),
        (true, 0),
        (false, 1),
        (false, 2),
        (false, 3),
        (true, 4),
    ] {
        let Some(exe) = (if remote {
            find_host()
        } else {
            Some(dll.clone())
        }) else {
            eprintln!("remote latency check needs the plugin host binary");
            continue;
        };
        let mut p = Project::new("latency change");
        if scenario == 3 {
            let mut writer = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 48_000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            for _ in 0..8000 {
                writer.write_sample(1.0f32).unwrap();
                writer.write_sample(1.0f32).unwrap();
            }
            writer.finalize().unwrap();
        }
        for i in 0..2 {
            let id = TrackId(100 + i);
            let mut track = Track::new(id, TrackKind::Audio, i.to_string());
            track.clips.push(Clip {
                id: ClipId(300 + i),
                name: "impulse".into(),
                color: None,
                start: 0,
                length: 400,
                muted: false,
                take: None,
                controllers: Vec::new(),
                body: ClipBody::Audio {
                    file: path.to_string_lossy().into_owned(),
                    offset: 0,
                    gain_db: 0.0,
                    process: Default::default(),
                },
            });
            if i == 0 {
                track.inserts.push(Plugin {
                    id: pid,
                    name: "Test Latency".into(),
                    kind: PluginKind::Effect,
                    format: "vst3".into(),
                    uid: uid.clone(),
                    bypass: false,
                    deactivated: false,
                    params: BTreeMap::new(),
                    state: None,
                    isolation: PluginIsolation::InProcess,
                    sidechain: None,
                });
                if scenario == 3 {
                    let mut lane =
                        xirius_core::auto::AutoLane::new(xirius_core::auto::AutoTarget::Mute);
                    lane.points = Arc::new(vec![xirius_core::auto::AutoPoint::new(0, 1.0)]);
                    track.automation.push(lane);
                }
            }
            p.tracks.insert(id, Arc::new(track));
            p.root.push(Node::Track(id));
        }
        let (mut host, mut audio, warnings) = SessionHost::start(
            &p,
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(if remote {
                    Isolation::Separate(exe)
                } else {
                    Isolation::InProcess
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let initial = audio.latency();
        assert_eq!(initial, if remote { DEFAULT_LATENCY } else { 0 });
        host.controller().send(Command::Play).ok().unwrap();
        let mut buffer = [0.0; 256 * 2];
        audio.render(&mut buffer, 2);
        host.poll();
        if remote {
            let link = host.plugins().link(pid).unwrap();
            host.plugins()
                .server_of(pid)
                .unwrap()
                .simulate_edit(&link, 1, &[0.5]);
            if scenario == 4 {
                let deadline = Instant::now() + Duration::from_secs(30);
                while !link.has_pending_state() && Instant::now() < deadline {
                    audio.render(&mut buffer, 2);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(
                    link.has_pending_state(),
                    "the edited state reached the engine before renaming"
                );
                Arc::make_mut(p.tracks.get_mut(&TrackId(100)).unwrap()).inserts[0].name =
                    "Renamed".into();
                host.sync(&p);
            }
        } else if scenario == 1 {
            assert!(host.plugins().local(pid).unwrap().0.set_param(1, 0.5));
        } else if scenario == 2 {
            let state = xirius_vst3::PluginState {
                component: 128u32.to_le_bytes().to_vec(),
                controller: 128u32.to_le_bytes().to_vec(),
            };
            Arc::make_mut(p.tracks.get_mut(&TrackId(100)).unwrap()).inserts[0].state =
                Some(xirius_plugins::encode_state(&state));
            assert!(
                !host.sync(&p).swapped,
                "state is first applied to the live instance"
            );
        } else {
            host.plugins()
                .local(pid)
                .unwrap()
                .0
                .edit_like_editor(1, &[0.5]);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while audio.latency() == initial && Instant::now() < deadline {
            host.poll();
            audio.render(&mut buffer, 2);
            if scenario == 3 && audio.latency() != initial {
                assert!(
                    buffer[128 * 2..].iter().all(|v| *v == 1.0),
                    "the muted automation never returns to the base value on the new plan"
                );
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            audio.latency(),
            initial + 128,
            "the active plan received the new plugin latency"
        );
        assert!(
            audio.is_playing(),
            "a latency restart does not stop playback"
        );
        if !remote && scenario != 3 {
            // The untouched track is delayed too: the two impulses still sum at one frame.
            let at = audio.song_position();
            let mut out = [0.0; 8000 * 2];
            audio.render(&mut out, 2);
            let frame = (4096 + 128 - at) as usize;
            assert_eq!(out[frame * 2], 2.0);
            assert_eq!(out[frame * 2 + 1], 2.0);
            assert_eq!(out.iter().filter(|v| **v != 0.0).count(), 2);
        }
        drop(audio);
        drop(host);
    }
    std::fs::remove_file(path).unwrap();
}
