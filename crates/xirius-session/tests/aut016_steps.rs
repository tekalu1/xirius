//! The steps of a plug-in's parameters come from its host process (AUT-016): the host counts
//! them as it opens the plug-in and sends them with its parameters (`Opened.steps`), and the
//! session reads them by plug-in. A switch has two; a parameter that glides is not there.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, probe};
use xirius_session::{host::HostConfig, plugins::Isolation, starting::Starting};

#[test]
fn a_plug_in_in_a_host_tells_the_steps_of_its_switch() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let entries = probe(&xirius_test_vst3::fixture_path(&deps)).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut p = Project::new("steps");
    let id = PluginId(1);
    let tid = TrackId(100);
    let mut t = Track::new(tid, TrackKind::Audio, "T");
    t.inserts.push(Plugin {
        samples: Vec::new(),
        id,
        name: "Test".into(),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid,
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation: PluginIsolation::Dedicated,
        sidechain: None,
        preset: None,
    });
    p.tracks.insert(tid, Arc::new(t));
    p.root.push(Node::Track(tid));
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..Default::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !s.step() {
        assert!(Instant::now() < deadline, "the plug-in to open");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (mut host, _audio, warnings) = s.finish(&p).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    while host.plugins().link(id).and_then(|l| l.opened()).is_none() {
        assert!(Instant::now() < deadline, "the plug-in to report");
        host.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    // the test plug-in's "Crash" has one step (a switch); the others have 256 (they glide)
    assert_eq!(
        host.plugins().param_steps(id),
        vec![(xirius_test_vst3::CRASH_PARAMETER, 2)]
    );
    assert!(
        host.plugins()
            .param_titles(id)
            .iter()
            .any(|(p, t)| *p == xirius_test_vst3::CRASH_PARAMETER && t == "Crash")
    );
}
