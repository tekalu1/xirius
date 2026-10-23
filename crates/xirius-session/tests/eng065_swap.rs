//! A change of the song that needs a new mix does not hold the caller while the plugins it
//! brings open in their hosts (ENG-065): `sync_later` returns at once, the mix playing goes on,
//! and `step_swap` swaps the new one in when its plugins are open. Starting a host process
//! again does not wait for the plugins either.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, probe};
use xirius_session::{host::HostConfig, host::SessionHost, plugins::Isolation};

/// How long the test plugin takes to be made (in each host).
const OPEN: Duration = Duration::from_millis(900);

fn track(i: u64, uid: &str) -> Arc<Track> {
    let mut t = Track::new(TrackId(100 + i), TrackKind::Audio, format!("T{i}"));
    t.inserts.push(Plugin {
        id: PluginId(1 + i),
        name: format!("Test {i}"),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation: PluginIsolation::Dedicated,
        sidechain: None,
    });
    Arc::new(t)
}

#[test]
fn a_new_mix_is_built_without_holding_the_caller_and_swapped_in_when_ready() {
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
    let mut p = Project::new("swap");
    for i in 0..2 {
        let t = track(i, &uid);
        p.root.push(Node::Track(t.id));
        p.tracks.insert(t.id, t);
    }
    let (mut host, _audio, warnings) = SessionHost::start(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    host.set_nonblocking(true);

    // A third track with a plugin of its own: a new mix.
    let mut p2 = p.clone();
    let t = track(2, &uid);
    p2.root.push(Node::Track(t.id));
    p2.tracks.insert(t.id, t);
    let began = Instant::now();
    let report = host.sync_later(&p2);
    let asked = began.elapsed();
    assert!(!report.swapped, "nothing is swapped yet");
    assert!(host.swap_pending());
    assert!(asked < OPEN / 2, "sync_later does not wait: {asked:?}");
    let mut longest = Duration::ZERO;
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        let t = Instant::now();
        let r = host.step_swap();
        longest = longest.max(t.elapsed());
        if let Some(r) = r {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let took = began.elapsed();
    eprintln!("sync_later {asked:?}, longest step {longest:?}, swapped after {took:?}");
    assert!(report.swapped, "{:?}", report.warnings);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!host.swap_pending());
    assert!(took >= OPEN, "it waited for the plugin to open: {took:?}");
    assert!(longest < OPEN / 2, "no step waits for a host: {longest:?}");
    assert!(host.plugin_timings().opened(3).is_some());

    // The same song again needs nothing.
    host.sync_later(&p2);
    assert!(!host.swap_pending());

    // Starting the hosts again does not wait for the plugins to open in them.
    let began = Instant::now();
    host.restart_plugins_later().unwrap();
    let restarted = began.elapsed();
    eprintln!("restart_plugins_later {restarted:?}");
    assert!(restarted < OPEN, "the restart does not wait: {restarted:?}");
}
