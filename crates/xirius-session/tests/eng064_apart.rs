//! The plugins that run in host processes of their own are asked for together when a song's
//! player starts (ENG-064): the first step does not wait for their answers, the answers are
//! counted as they come, and the plugins open at once, not one after another.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, probe};
use xirius_session::{host::HostConfig, plugins::Isolation, starting::Starting};

/// How long the test plugin takes to be made (in each host).
const OPEN: Duration = Duration::from_millis(700);
const PLUGINS: u64 = 4;

#[test]
fn plugins_in_hosts_of_their_own_open_at_once() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    // SAFETY: the only test of this binary; the hosts it starts read it.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
    }
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let entries = probe(&deps.join("xirius_test_vst3.dll")).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut p = Project::new("apart");
    for i in 0..PLUGINS {
        let id = TrackId(100 + i);
        let mut t = Track::new(id, TrackKind::Audio, format!("T{i}"));
        t.inserts.push(Plugin {
            samples: Vec::new(),
            id: PluginId(1 + i),
            name: format!("Test {i}"),
            kind: PluginKind::Effect,
            format: "vst3".into(),
            uid: uid.clone(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::new(),
            state: None,
            isolation: PluginIsolation::Dedicated,
            sidechain: None,
            preset: None,
        });
        p.tracks.insert(id, Arc::new(t));
        p.root.push(Node::Track(id));
    }
    let began = Instant::now();
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..Default::default()
        },
    );
    assert!(!s.step(), "the first step only asks");
    let asked = began.elapsed();
    assert!(
        asked < OPEN,
        "asking does not wait for the answers: {asked:?}"
    );
    assert_eq!(s.progress().done, 0);
    let mut seen = vec![0];
    let deadline = Instant::now() + Duration::from_secs(60);
    while !s.step() {
        let done = s.progress().done;
        if seen.last() != Some(&done) {
            seen.push(done);
        }
        assert!(Instant::now() < deadline, "{seen:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
    let all = began.elapsed();
    let (host, _audio, warnings) = s.finish(&p).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let took: Vec<Duration> = (1..=PLUGINS)
        .map(|id| host.plugin_timings().opened(id).expect("an opening time"))
        .collect();
    eprintln!("asked in {asked:?}, all open in {all:?}, each {took:?}, counts {seen:?}");
    for t in &took {
        assert!(*t >= OPEN, "each took its time in its host: {took:?}");
    }
    let one_after_another: Duration = took.iter().sum();
    assert!(
        all < one_after_another * 3 / 4,
        "they opened at once: {all:?} for {took:?}"
    );
}
