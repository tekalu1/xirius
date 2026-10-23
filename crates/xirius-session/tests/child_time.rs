//! A plugin in its own process is timed there (ENG-060): its reading is the plugin host's time
//! around the plugin, marked so, and `plugin.stats` says where it was measured. One in the
//! engine's process is timed where it runs, as before.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{ClipId, Node, PluginId, TrackId};
use xirius_core::model::{
    Clip, ClipBody, Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind,
};
use xirius_engine::{Command, server::find_host};
use xirius_plugins::{Catalog, probe};
use xirius_session::{
    host::{HostConfig, SessionHost},
    perf,
    plugins::Isolation,
};

#[test]
fn a_plugin_in_its_own_process_reads_as_the_time_it_took_there() {
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
        .prefix("xirius-child-time-")
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
    for i in 0..48_000 {
        let v = ((i as f32) * 0.05).sin() * 0.25;
        writer.write_sample(v).unwrap();
        writer.write_sample(v).unwrap();
    }
    writer.finalize().unwrap();
    let pid = PluginId(200);
    for remote in [false, true] {
        let isolation = if remote {
            let Some(exe) = find_host() else {
                eprintln!("the child's time needs the plugin host binary");
                continue;
            };
            Isolation::Separate(exe)
        } else {
            Isolation::InProcess
        };
        let mut p = Project::new("child time");
        let id = TrackId(100);
        let mut track = Track::new(id, TrackKind::Audio, "Audio".to_string());
        track.clips.push(Clip {
            id: ClipId(300),
            name: "tone".into(),
            color: None,
            start: 0,
            length: 3840,
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
        track.inserts.push(Plugin {
            samples: Vec::new(),
            id: pid,
            name: "Test".into(),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: uid.clone(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::new(),
            state: None,
            isolation: PluginIsolation::InProcess,
            sidechain: None,
            preset: None,
        });
        p.tracks.insert(id, Arc::new(track));
        p.root.push(Node::Track(id));
        let (mut host, mut audio, warnings) = SessionHost::start(
            &p,
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(isolation),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        host.controller().send(Command::Play).ok().unwrap();
        let timings = host.plugin_timings();
        let mut buffer = [0.0; 256 * 2];
        // The host's blocks come back a few blocks late: render until it has reported some.
        let deadline = Instant::now() + Duration::from_secs(30);
        let enough =
            |t: &xirius_engine::timing::Timings| t.reading(pid.0).is_some_and(|r| r.blocks >= 4);
        while !enough(&timings) && Instant::now() < deadline {
            host.poll();
            audio.render(&mut buffer, 2);
        }
        let read = timings.reading(pid.0).expect("a reading");
        assert!(read.blocks >= 4, "remote {remote}: {read:?}");
        assert_eq!(read.child, remote, "{read:?}");
        assert!(read.mean_us > 0.0, "remote {remote}: {read:?}");
        let rows = perf::loads(&p, &timings, None);
        let json = perf::rows_json(&rows, &BTreeSet::new());
        assert_eq!(
            json[0]["measured_in"],
            if remote { "plugin-host" } else { "engine" }
        );
        host.controller().send(Command::Stop).ok().unwrap();
    }
}
