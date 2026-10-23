//! The window of a plugin moves to the instance a new mix opened when that mix comes in
//! (ENG-066). The old instance's window is closed then, and the host reads its state once more
//! as it does for any window that closes: that state is not the plugin's any more, so it is not
//! taken as an edit of the song (ENG-067).
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

fn track(i: u64, uid: &str, plugin: bool) -> Arc<Track> {
    let mut t = Track::new(TrackId(100 + i), TrackKind::Audio, format!("T{i}"));
    if plugin {
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
    }
    Arc::new(t)
}

/// The slot of the instance that plays and the delay it has now, read from the state it
/// reports when asked.
fn playing(server: &PluginServer, link: &PluginLink) -> (u32, u32) {
    let seen = link.states_seen();
    server.request_state(link);
    let deadline = Instant::now() + Duration::from_secs(30);
    while link.states_seen() == seen {
        assert!(
            Instant::now() < deadline,
            "the instance that plays never reported its state"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let open = server.current(link).expect("an instance plays");
    let state = open.state.expect("it reported a state");
    let bytes: [u8; 4] = state.component[..4].try_into().unwrap();
    (open.slot, u32::from_le_bytes(bytes))
}

#[test]
fn the_state_the_old_window_leaves_when_it_closes_is_not_the_new_instances() {
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
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut p = Project::new("recapture");
    let t = track(0, &uid, true);
    p.root.push(Node::Track(t.id));
    p.tracks.insert(t.id, t);
    let (mut host, mut audio, warnings) = SessionHost::start(
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
    let link = host.plugins().link(PID).expect("the plugin is in a host");
    let server = host.plugins().server_of(PID).unwrap().clone();
    let (old, _) = playing(&server, &link);
    host.plugins().editor(PID, true, None).unwrap();
    assert!(server.status(&link).editor_open);

    // A track more needs a new mix, which opens the plugin again; it comes in with the window
    // open on the old instance.
    let mut p2 = p.clone();
    let t = track(1, &uid, false);
    p2.root.push(Node::Track(t.id));
    p2.tracks.insert(t.id, t);
    let answers = link.answers();
    assert!(!host.sync_later(&p2).swapped);
    host.step_swap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while link.answers() <= answers {
        assert!(Instant::now() < deadline, "the new instance never opened");
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = link.take_edits();
    let seen = link.states_seen();
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        if let Some(r) = host.step_swap() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(report.swapped, "{:?}", report.warnings);

    // The old window was closed and its state read once more, while the old mix still has
    // the old instance (the audio thread has not taken the new mix yet): it is not the
    // plugin's.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        link.states_seen(),
        seen,
        "no state came from the old instance"
    );
    assert!(!link.has_pending_state(), "nothing to take into the song");
    assert!(
        server.status(&link).editor_open,
        "the window is on the new one"
    );
    audio.render(&mut [0.0; 512], 2);
    host.controller().poll(|_| {});

    // The new instance's state is its own: a move in its window is what the plugin has.
    server.simulate_edit(&link, 1, &[0.5]);
    let deadline = Instant::now() + Duration::from_secs(30);
    let now = loop {
        let now = playing(&server, &link);
        if now.1 == 128 || Instant::now() >= deadline {
            break now;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_ne!(now.0, old, "the new instance plays");
    assert_eq!(now.1, 128);
    host.plugins().editor(PID, false, None).unwrap();
}
