//! The player takes plugins that run in host processes before they have answered (ENG-065):
//! `finish_late` does not wait for them, they play dry until they are in, and what their host
//! says about them (a plugin that cannot be opened) comes afterwards.
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, probe};
use xirius_session::{host::HostConfig, plugins::Isolation, starting::Starting};

/// A private factory gate keeps even the bad class unanswered until the caller returns.
struct Held(std::path::PathBuf);
impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn plugin(id: u64, uid: &str) -> Plugin {
    Plugin {
        samples: Vec::new(),
        id: PluginId(id),
        name: format!("Test {id}"),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation: PluginIsolation::Dedicated,
        sidechain: None,
        preset: None,
    }
}

#[test]
fn the_player_does_not_wait_for_the_hosts_and_what_they_say_comes_later() {
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
    let tmp = tempfile::tempdir().unwrap();
    let dll = tmp.path().join("late.dll");
    std::fs::copy(deps.join("xirius_test_vst3.dll"), &dll).unwrap();
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    // The catalog names a class the factory does not have; only its host finds out.
    let mut gone = entries[0].clone();
    gone.uid = "0123456789abcdef0123456789abcdef".into();
    gone.path = dll.clone();
    let gone_uid = gone.uid.clone();
    let mut entries = entries;
    entries.push(gone);
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut p = Project::new("late");
    for (i, uid) in [uid.as_str(), uid.as_str(), gone_uid.as_str()]
        .iter()
        .enumerate()
    {
        let id = TrackId(100 + i as u64);
        let mut t = Track::new(id, TrackKind::Audio, format!("T{i}"));
        t.inserts.push(plugin(1 + i as u64, uid));
        p.tracks.insert(id, Arc::new(t));
        p.root.push(Node::Track(id));
    }
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..Default::default()
        },
    );
    let held = Held(dll.with_extension("gate"));
    std::fs::write(&held.0, b"hold").unwrap();
    assert!(!s.step(), "the first step only asks");
    let (mut host, _audio, warnings) = s.finish_late(&p).unwrap();
    // Do not release the factories until finish_late has returned. A synchronous
    // wait cannot pass merely because the scheduler happened to be quick.
    assert!(held.0.is_file());
    assert!(
        !dll.with_extension("timed-out").exists(),
        "factory gate timed out"
    );
    assert!(
        warnings.is_empty(),
        "nothing is said before they answer: {warnings:?}"
    );
    assert!(host.plugins().late_pending(), "they are still opening");
    let entered = Instant::now() + Duration::from_secs(60);
    while !dll.with_extension("entered").exists() {
        assert!(
            Instant::now() < entered,
            "host did not enter the factory gate"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(host.plugins().late_pending());
    drop(held);

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut said = Vec::new();
    while host.plugins().late_pending() {
        assert!(Instant::now() < deadline, "{said:?}");
        said.extend(host.take_late_warnings());
        std::thread::sleep(Duration::from_millis(10));
    }
    said.extend(host.take_late_warnings());
    eprintln!("late: {said:?}");
    assert!(
        said.iter().any(|w| w.contains("Test 3")),
        "the plugin that cannot be opened is said: {said:?}"
    );
    assert!(
        said.iter()
            .all(|w| !w.contains("Test 1") && !w.contains("Test 2")),
        "the others say nothing: {said:?}"
    );
    assert!(host.take_late_warnings().is_empty(), "each is said once");
    for id in [1, 2] {
        assert!(
            host.plugin_timings().opened(id).is_some(),
            "plugin {id}'s opening time is kept once it is in"
        );
    }
}
