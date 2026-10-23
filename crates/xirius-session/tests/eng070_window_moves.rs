//! A plugin whose window is open moves to another host process (its isolation changes): the
//! window opens on the new instance, in the new host, when the new mix comes in (ENG-070).
//! Until then the old window stays, and the window counts as open, so the screen does not take
//! it for one the person closed.
//!
//! Skips when `xirius-plugin-host` has not been built (`cargo test --workspace` builds it).
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use xirius_core::id::{Node, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_engine::server::{PluginLink, PluginServer, find_host};
use xirius_plugins::{Catalog, probe};
use xirius_session::{host::HostConfig, host::SessionHost, plugins::Isolation};

/// How long the test plugin takes to be made, so a new mix stays pending a while.
const OPEN: Duration = Duration::from_millis(900);
const PID: PluginId = PluginId(1);

fn track(isolation: PluginIsolation, uid: &str) -> Arc<Track> {
    let mut t = Track::new(TrackId(100), TrackKind::Audio, "T".to_string());
    t.inserts.push(Plugin {
        samples: Vec::new(),
        id: PID,
        name: "Test".into(),
        kind: PluginKind::Effect,
        format: "vst3".into(),
        uid: uid.to_string(),
        bypass: false,
        deactivated: false,
        params: BTreeMap::new(),
        state: None,
        isolation,
        sidechain: None,
        preset: None,
    });
    Arc::new(t)
}

/// True when the instance that plays has its window open in its host: closing the window
/// there reads the plugin's state once more, which no other message does here.
fn window_in_host(server: &PluginServer, link: &PluginLink) -> bool {
    let seen = link.states_seen();
    server.close_editor(link);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if link.states_seen() > seen {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn the_window_moves_with_the_plugin_to_another_host() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    // SAFETY: the only test of this binary; the hosts it starts read them.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
        // The windows open off every screen: nothing comes to the front.
        std::env::set_var(xirius_vst3::OFFSCREEN_ENV, "1");
    }
    xirius_test_vst3::fixture_marker();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let entries = probe(&deps.join("xirius_test_vst3.dll")).unwrap();
    let uid = entries[0].uid.clone();
    let mut p = Project::new("window-moves");
    let t = track(PluginIsolation::Dedicated, &uid);
    p.root.push(Node::Track(t.id));
    p.tracks.insert(t.id, t);
    let (mut host, mut audio, warnings) = SessionHost::start(
        &p,
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: Vec::new(),
            })),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    host.set_nonblocking(true);
    let old_server = host.plugins().server_of(PID).unwrap().clone();
    let old_link = host.plugins().link(PID).unwrap();
    let old_pid = old_server.pid().expect("its host runs");
    host.plugins().editor(PID, true, None).unwrap();

    // The plugin moves to the host the plugins share.
    let mut p2 = p.clone();
    let t = track(PluginIsolation::Shared, &uid);
    p2.tracks.insert(t.id, t);
    assert!(!host.sync_later(&p2).swapped);
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        // While the new instance opens, the window stays open where it is and counts as open.
        assert!(
            host.plugin_statuses()
                .iter()
                .any(|(id, _, s)| *id == PID && s.editor_open),
            "the window counts as open while the new instance opens"
        );
        if let Some(r) = host.step_swap() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(report.swapped, "{:?}", report.warnings);
    audio.render(&mut [0.0; 512], 2);
    host.controller().poll(|_| {});

    let server = host.plugins().server_of(PID).unwrap().clone();
    let link = host.plugins().link(PID).unwrap();
    assert!(!Arc::ptr_eq(&link, &old_link), "a new link in the new host");
    assert_ne!(
        server.pid(),
        Some(old_pid),
        "the plugin runs in another host"
    );
    assert!(server.status(&link).editor_open);
    assert!(
        window_in_host(&server, &link),
        "the window is open on the new instance"
    );
}
