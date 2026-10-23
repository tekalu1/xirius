use std::{collections::BTreeMap, sync::Arc};
use xirius_core::{
    auto::{AutoLane, AutoPoint, AutoTarget},
    id::{ClipId, Node, PluginId, TrackId},
    model::{Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind},
};
use xirius_plugins::{Catalog, probe};
use xirius_session::{
    build::Builder,
    export,
    graph::Select,
    host::{HostConfig, SessionHost},
    plugins::Isolation,
};

#[test]
fn automation_on_the_delayed_and_compensated_tracks_follows_the_audio_after_seek() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(dll.is_file());
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("automation-pdc-{}.wav", std::process::id()));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..6400 {
        let v = if [750, 1110, 1740, 3150, 3510, 4140].contains(&i) {
            1.0f32
        } else {
            0.0
        };
        writer.write_sample(v).unwrap();
        writer.write_sample(v).unwrap();
    }
    writer.finalize().unwrap();
    let mut p = Project::new("PDC automation");
    for i in 0..2 {
        let id = TrackId(100 + i);
        let mut track = Track::new(id, TrackKind::Audio, format!("track {i}"));
        track.clips.push(Clip {
            id: ClipId(300 + i),
            name: "impulses".into(),
            color: None,
            start: 0,
            length: 256,
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
                samples: Vec::new(),
                id: PluginId(200),
                name: "Test Latency".into(),
                kind: PluginKind::Effect,
                format: "vst3".into(),
                uid: uid.clone(),
                bypass: false,
                deactivated: false,
                params: BTreeMap::from([("1".into(), 1.0)]),
                state: None,
                isolation: PluginIsolation::InProcess,
                sidechain: None,
            });
        }
        let mut lane = AutoLane::new(AutoTarget::Mute);
        lane.points = Arc::new(
            [(0, 1.0), (38, 0.0), (96, 1.0), (134, 0.0)]
                .into_iter()
                .map(|(tick, value)| {
                    let mut pt = AutoPoint::new(tick, value);
                    pt.step = true;
                    pt
                })
                .collect(),
        );
        track.automation.push(lane);
        p.tracks.insert(id, Arc::new(track));
        p.root.push(Node::Track(id));
    }
    for from in [0, 2400] {
        let mut builder = Builder::new(48000, None);
        builder.set_catalog(Some(catalog.clone()));
        builder.set_isolation(Isolation::InProcess);
        let out = export::render(&mut builder, &p, Select::All, from, from + 2200, 0).unwrap();
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(
            out.left[750], 0.0,
            "offline pre-change impulse, seek {from}"
        );
        assert_eq!(
            out.left[1740], 2.0,
            "offline post-change impulse, seek {from}"
        );
        let (mut host, mut audio, warnings) = SessionHost::start(
            &p,
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(Isolation::InProcess),
                engine: xirius_engine::EngineConfig {
                    meter_interval: std::time::Duration::ZERO,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(audio.latency(), 256);
        assert!(host.seek_frame(from) && host.play());
        let mut live = Vec::new();
        let mut buffer = [0.0; 480 * 2];
        while live.len() < 2400 {
            audio.render(&mut buffer, 2);
            live.extend(buffer.as_chunks::<2>().0.iter().map(|v| v[0]));
            host.poll();
        }
        assert_eq!(live[750 + 256], 0.0, "live pre-change impulse, seek {from}");
        assert_eq!(
            live[1740 + 256],
            2.0,
            "live post-change impulse, seek {from}"
        );
    }
    for from in [1100, 3500] {
        let mut builder = Builder::new(48000, None);
        builder.set_catalog(Some(catalog.clone()));
        builder.set_isolation(Isolation::InProcess);
        let out = export::render(&mut builder, &p, Select::All, from, from + 600, 0).unwrap();
        assert_eq!(
            out.left[10], 2.0,
            "offline seek just after the change: {from}"
        );
        let (mut host, mut audio, _) = SessionHost::start(
            &p,
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(Isolation::InProcess),
                engine: xirius_engine::EngineConfig {
                    meter_interval: std::time::Duration::ZERO,
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
        let mut buffer = [0.0; 960];
        audio.render(&mut buffer, 2); // Leave the stopped transport notification queued.
        assert!(host.seek_frame(from) && host.play());
        host.poll();
        assert_eq!(host.status().frame, from);
        assert_eq!(
            host.plugins().local(PluginId(200)).unwrap().0.latency(),
            256
        );
        audio.render(&mut buffer, 2);
        assert_eq!(
            buffer[(10 + 256) * 2],
            2.0,
            "live seek just after the change: {from}"
        );
    }
    let mut parameter_song = p.clone();
    let track = Arc::make_mut(parameter_song.tracks.get_mut(&TrackId(100)).unwrap());
    let mut effect = track.inserts[0].clone();
    effect.id = PluginId(201);
    effect.params.insert("1".into(), 0.0);
    track.inserts.push(effect);
    let (mut host, mut audio, _) = SessionHost::start(
        &parameter_song,
        HostConfig {
            catalog: Some(catalog.clone()),
            isolation: Some(Isolation::InProcess),
            engine: xirius_engine::EngineConfig {
                meter_interval: std::time::Duration::ZERO,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    assert!(host.play());
    audio.render(&mut [0.0; 3840 * 2], 2);
    host.poll();
    assert_eq!(host.status().frame, 3840);
    // Let the real worker finish the blocks it was lent. A parameter edit must also
    // reach a track whose next audio is already prefetched, independent of scheduling.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while audio.prefetch(0).unwrap().queued_blocks < xirius_engine::prefetch::DEPTH {
        assert!(std::time::Instant::now() < end, "{:?}", audio.prefetch(0));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let track = Arc::make_mut(parameter_song.tracks.get_mut(&TrackId(100)).unwrap());
    let mut lane = AutoLane::new(AutoTarget::Param(PluginId(201), "1".into()));
    lane.points = Arc::new(vec![AutoPoint::new(0, 0.25), AutoPoint::new(144, 0.75)]);
    Arc::make_mut(&mut lane.points)[0].step = true;
    track.automation.push(lane);
    assert!(!host.sync(&parameter_song).swapped);
    // One frame fits in the already queued block: consuming it does not wake a worker
    // to process the new parameter before this assertion by chance.
    audio.render(&mut [0.0; 2], 2);
    assert_eq!(
        host.plugins().local(PluginId(201)).unwrap().0.param(1),
        Some(0.25),
        "sync uses the later insert's input time before poll"
    );
    Arc::make_mut(parameter_song.tracks.get_mut(&TrackId(100)).unwrap()).inserts[1]
        .params
        .insert("1".into(), 0.9);
    host.sync(&parameter_song);
    audio.render(&mut [0.0; 960], 2);
    assert_eq!(
        host.plugins().local(PluginId(201)).unwrap().0.param(1),
        Some(0.25),
        "the parameter lane overrides a saved knob immediately"
    );
    assert!(
        !audio.prefetch(0).unwrap().lent,
        "parameter lanes render now"
    );
    Arc::make_mut(parameter_song.tracks.get_mut(&TrackId(100)).unwrap())
        .automation
        .retain(|lane| !matches!(lane.target, AutoTarget::Param(..)));
    assert!(!host.sync(&parameter_song).swapped);
    audio.render(&mut [0.0; 1024], 2);
    assert!(audio.prefetch(0).unwrap().lent, "other lanes can run ahead");
    std::fs::remove_file(path).unwrap();
}
