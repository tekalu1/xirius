//! A new mix that opens the plugin again while another instance plays (ENG-065/066) has a
//! seat of its own for what the new instance says (ENG-067): when it cannot be opened, the one
//! heard keeps its error, answer and latency, and goes on playing until the new mix comes in.
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
            // The delay parameter: 64 samples of latency.
            params: BTreeMap::from([("1".to_string(), 0.25)]),
            state: None,
            isolation: PluginIsolation::Dedicated,
        });
    }
    Arc::new(t)
}

/// The slot of the instance that plays, from the state it reports when asked.
fn playing(server: &PluginServer, link: &PluginLink) -> u32 {
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
    server.current(link).expect("an instance plays").slot
}

/// Waits until the host has answered an opening begun after `answers` was read.
fn answered(link: &PluginLink, answers: u64) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while link.answers() <= answers {
        assert!(Instant::now() < deadline, "the opening was never answered");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_new_instance_that_cannot_be_opened_leaves_the_one_heard_as_it_is() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    let fail = std::env::temp_dir().join(format!("xirius-eng067-fail-{}", std::process::id()));
    let _ = std::fs::remove_file(&fail);
    // SAFETY: the only test of this binary; the hosts it starts read them.
    unsafe {
        std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN.as_millis().to_string());
        std::env::set_var("XIRIUS_TEST_VST3_FAIL_FILE", &fail);
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
    let mut p = Project::new("seats");
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
    let old = playing(&server, &link);
    let before = server.status(&link);
    assert_eq!(before.error, None);
    assert_eq!(link.opened().expect("it opened").latency, 64);
    assert!(before.latency >= 64, "{}", before.latency);
    let took = link.open_took();

    // A track more needs a new mix, which opens the plugin again; this time it cannot.
    let mut p2 = p.clone();
    let t = track(1, &uid, false);
    p2.root.push(Node::Track(t.id));
    p2.tracks.insert(t.id, t);
    std::fs::write(&fail, b"").unwrap();
    let answers = link.answers();
    assert!(!host.sync_later(&p2).swapped);
    assert!(host.swap_pending());
    host.step_swap();
    answered(&link, answers);

    // The one heard is as it was: no error, its answer and latency, and it still plays.
    let now = server.status(&link);
    assert_eq!(link.error(), None, "the old instance's error stays");
    assert_eq!(now.error, None);
    assert_eq!(link.opened().expect("still its answer").latency, 64);
    assert_eq!(now.latency, before.latency, "the latency heard stays");
    assert_eq!(link.take_latency_change(), None, "nothing to build again");
    assert!(
        link.opening_error().is_some(),
        "the new one's failure is on its own seat"
    );
    assert_ne!(link.open_took(), took, "its opening time is its own");
    assert_eq!(playing(&server, &link), old, "the old instance plays on");
    let wet = server.status(&link).wet_blocks;
    let deadline = Instant::now() + Duration::from_secs(30);
    while server.status(&link).wet_blocks == wet {
        assert!(Instant::now() < deadline, "the old instance is not heard");
        audio.render(&mut [0.0; 512], 2);
        std::thread::sleep(Duration::from_millis(5));
    }

    // The song goes back before the new mix came in: nothing of the failure is left.
    host.sync_later(&p);
    assert!(!host.swap_pending());
    assert_eq!(link.error(), None);
    assert_eq!(
        link.opening_error(),
        None,
        "the failed seat went with its mix"
    );
    assert_eq!(server.status(&link).latency, before.latency);

    // Again, and this time the new mix comes in: it warns of the plugin it could not open as
    // a mix built at once does, and plays it bypassed.
    let answers = link.answers();
    assert!(!host.sync_later(&p2).swapped);
    host.step_swap();
    answered(&link, answers);
    assert_eq!(link.error(), None, "not until the new mix comes in");
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        if let Some(r) = host.step_swap() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(report.swapped);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("を読み込めません（バイパスして再生します）")),
        "{:?}",
        report.warnings
    );
    assert!(
        link.error().is_some(),
        "the instance heard now could not be opened"
    );
    audio.render(&mut [0.0; 512], 2);
    host.controller().poll(|_| {});
    let _ = std::fs::remove_file(&fail);
}
