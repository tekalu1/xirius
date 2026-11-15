//! A probe for plugins whose getState answers different bytes every pull (UI-354): opens each
//! plugin listed in `XIRIUS_VOLATILE_VST3` (`;`-separated `.vst3` paths) on its owner thread
//! exactly as a song's preparation does, asks the state twice, and reports whether the two
//! answers' bytes differ. Run with `--ignored`; a report table goes to stderr.
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, Project, Track, TrackKind};
use xirius_plugins::{Catalog, probe};
use xirius_session::host::{HostConfig, peek_pulled_states};
use xirius_session::plugins::Isolation;
use xirius_session::starting::Starting;

#[test]
#[ignore = "needs XIRIUS_VOLATILE_VST3; run with --ignored"]
fn plugin_states_pulled_twice_report_which_differ() {
    let paths: Vec<PathBuf> = std::env::var("XIRIUS_VOLATILE_VST3")
        .expect("XIRIUS_VOLATILE_VST3 lists no plugins")
        .split(';')
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    assert!(!paths.is_empty(), "XIRIUS_VOLATILE_VST3 names no plugins");
    eprintln!("| plugin | pull | bytes | same? |");
    let mut different = 0;
    for path in &paths {
        assert!(path.exists(), "{path:?}");
        let entries = match probe(path) {
            Ok(e) if !e.is_empty() => e,
            _other => {
                eprintln!("| {} | probe failed | - | - |", path.display());
                continue;
            }
        };
        let entry = entries[0].clone();
        let kind = entry.kind;
        let mut p = Project::new("volatile-probe");
        let id = TrackId(100);
        let mut t = Track::new(id, TrackKind::Instrument, "T".to_string());
        let model_plugin = Plugin {
            samples: Vec::new(),
            id: PluginId(1),
            name: entry.name.clone(),
            kind,
            format: entry.format.clone(),
            uid: entry.uid.clone(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::new(),
            state: None,
            isolation: PluginIsolation::InProcess,
            sidechain: None,
            preset: None,
        };
        t.inserts.push(model_plugin.clone());
        p.tracks.insert(id, Arc::new(t));
        p.root.push(Node::Track(id));
        let catalog = Arc::new(Catalog {
            entries: entries.clone(),
            failures: Vec::new(),
        });
        let mut s = Starting::new(
            &p,
            HostConfig {
                catalog: Some(catalog),
                threaded_local: true,
                isolation: Some(Isolation::InProcess),
                ..Default::default()
            },
        );
        let deadline = Instant::now() + Duration::from_secs(90);
        while !s.step() {
            assert!(Instant::now() < deadline, "{} never opened", entry.name);
            std::thread::sleep(Duration::from_millis(5));
        }
        let at = Instant::now();
        let (mut host, _audio, failures) = match s.finish_prepared(&p, |_, _| {}) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("| {} | open failed {e:?} | - | - |", entry.name);
                continue;
            }
        };
        if !failures.is_empty() {
            eprintln!("| {} | prepare failed {:?} | - | - |", entry.name, failures);
            continue;
        }
        let mut answers = Vec::new();
        for _ in 0..2 {
            let pull = host.begin_pull_states();
            let wait = Instant::now() + Duration::from_secs(30);
            while !pull.ready() {
                assert!(Instant::now() < wait, "{} pull never answered", entry.name);
                std::thread::sleep(Duration::from_millis(5));
            }
            let mut plugins = vec![model_plugin.clone()];
            assert_eq!(
                peek_pulled_states(&mut host, &pull, &p, &mut plugins),
                1,
                "{} answered no state",
                entry.name
            );
            answers.push(plugins[0].state.clone().unwrap_or_default());
        }
        let bytes = |text: &str| {
            xirius_plugins::decode_state(text)
                .map(|s| s.component.len() + s.controller.len())
                .unwrap_or(0)
        };
        let (a, b) = (&answers[0], &answers[1]);
        let same = a == b;
        different += usize::from(!same);
        eprintln!(
            "| {} | open+pulls {:?} | {} vs {} | {} |",
            entry.name,
            at.elapsed(),
            bytes(a),
            bytes(b),
            if same { "same" } else { "DIFFERENT" }
        );
    }
    eprintln!("plugins whose state differed between two pulls: {different}");
}
