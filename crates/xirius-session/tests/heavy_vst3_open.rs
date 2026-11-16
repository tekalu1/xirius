//! Real-world check of the song-open wait (ENG-064): plugins installed on this machine, not
//! the fixture. Needs `XIRIUS_HEAVY_VST3` set to `;`-separated `.vst3` paths (3–5 heavy ones),
//! and is run with `--ignored`. A fresh process prepares the song exactly as the window does
//! after a restart (`threaded_local` owner threads, the open limit in force) and reports each
//! plugin's open time and any track marked 準備できませんでした.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_plugins::{Catalog, probe};
use xirius_session::host::HostConfig;
use xirius_session::plugins::Isolation;
use xirius_session::starting::Starting;

#[test]
#[ignore = "needs XIRIUS_HEAVY_VST3; run with --ignored"]
fn real_heavy_plugins_prepare_without_failed_tracks() {
    let paths: Vec<PathBuf> = std::env::var("XIRIUS_HEAVY_VST3")
        .expect("XIRIUS_HEAVY_VST3 lists no plugins")
        .split(';')
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    assert!(
        (3..=5).contains(&paths.len()),
        "XIRIUS_HEAVY_VST3 names 3-5 plugins: {paths:?}"
    );
    let mut entries = Vec::new();
    for p in &paths {
        assert!(p.exists(), "{p:?}");
        let probed = probe(p).unwrap_or_else(|e| panic!("probe {p:?}: {e}"));
        assert!(!probed.is_empty(), "no plugin in {p:?}");
        entries.extend(probed);
    }
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut p = Project::new("heavy-open");
    for (i, e) in catalog.entries.iter().enumerate() {
        let id = TrackId(100 + i as u64);
        let mut t = Track::new(id, TrackKind::Audio, format!("T{i}"));
        t.inserts.push(Plugin {
            pad_outputs: Default::default(),
            samples: Vec::new(),
            id: PluginId(i as u64 + 1),
            name: e.name.clone(),
            kind: PluginKind::Effect,
            format: e.format.clone(),
            uid: e.uid.clone(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::new(),
            state: None,
            isolation: PluginIsolation::InProcess,
            sidechain: None,
            note_source: None,
            preset: None,
        });
        p.tracks.insert(id, Arc::new(t));
        p.root.push(Node::Track(id));
    }
    let mut s = Starting::new(
        &p,
        HostConfig {
            catalog: Some(catalog),
            threaded_local: true,
            isolation: Some(Isolation::InProcess),
            ..Default::default()
        },
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    while !s.step() {
        assert!(Instant::now() < deadline, "the opens never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (host, _audio, failures) = s.finish_prepared(&p, |_, _| {}).unwrap();
    for (i, e) in p.tracks.values().flat_map(|t| t.inserts.iter()).enumerate() {
        eprintln!(
            "plugin[{}] {}: opened in {:?}",
            i,
            e.name,
            host.plugin_timings().opened(e.id.0)
        );
    }
    assert!(
        host.unavailable_tracks().is_empty(),
        "unavailable: {:?}",
        host.unavailable_tracks()
    );
    assert!(failures.is_empty(), "{failures:?}");
}
