//! A plugin that is slow to open but answers is waited for while a song's plugins are
//! prepared (ENG-064): it is not a failure before the open's own limit. The window's
//! preparation opens external plugins in host processes.
mod crash_records;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_plugins::{Catalog, probe};
use xirius_session::host::HostConfig;
use xirius_session::plugins::Isolation;
use xirius_session::starting::Starting;

/// The fixture sleeps this long in its createInstance: longer than the open's answer limit
/// used to be, far under the limit a plugin is given now.
const OPEN: Duration = Duration::from_secs(2);

fn catalog() -> (Arc<Catalog>, String) {
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = xirius_test_vst3::fixture_path(&deps);
    assert!(
        dll.exists(),
        "the fixture's cdylib is built with its dev dependency"
    );
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    (
        Arc::new(Catalog {
            entries,
            failures: Vec::new(),
        }),
        uid,
    )
}

fn song(kind: PluginKind, uid: &str) -> Project {
    let mut p = Project::new("slow-open");
    let id = TrackId(100);
    let mut t = Track::new(id, TrackKind::Audio, "Gt".to_string());
    t.inserts.push(Plugin {
        pad_outputs: Default::default(),
        samples: Vec::new(),
        id: PluginId(1),
        name: "Test Latency".to_string(),
        kind,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation: PluginIsolation::Shared,
        sidechain: None,
        note_source: None,
        preset: None,
    });
    p.tracks.insert(id, Arc::new(t));
    p.root.push(Node::Track(id));
    p
}

#[test]
fn a_plugin_slow_to_open_is_waited_for_and_no_track_fails() {
    let _records = crash_records::CrashRecords::new();
    // SAFETY: the only test of this binary; the fixture reads it when an instance is made.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
    }
    let (catalog, uid) = catalog();
    let p = song(PluginKind::Effect, &uid);
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(xirius_engine::server::find_host())),
            ..Default::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !s.step() {
        assert!(Instant::now() < deadline, "the open never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (host, _audio, failures) = s.finish_prepared(&p, |_, _| {}).unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(host.unavailable_tracks().is_empty());
    assert!(host.plugin_timings().opened(1).is_some());
}

/// Dropping a preparation whose open is still in flight does not wait for the open: the job's
/// worker is abandoned, so a cancel during a slow open answers at once.
#[test]
fn dropping_preparation_does_not_wait_for_an_open_in_flight() {
    let _records = crash_records::CrashRecords::new();
    // SAFETY: the only test of this binary; the fixture reads it when an instance is made.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
    }
    let (catalog, uid) = catalog();
    let p = song(PluginKind::Effect, &uid);
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(xirius_engine::server::find_host())),
            ..Default::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !s.step() {
        assert!(Instant::now() < deadline, "the open never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    let at = Instant::now();
    drop(s);
    assert!(
        at.elapsed() < OPEN,
        "dropping waited for the open: {:?}",
        at.elapsed()
    );
}
