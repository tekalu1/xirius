//! While a new mix opens its plugins (ENG-065), the instance that plays is the one the user
//! hears and moves in its window (ENG-066): a value put into it meanwhile reaches it, stays in
//! it when the new mix is not swapped in after all, and is in the new instance when it is.
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
/// The test plugin's parameter that sets its delay (0..1 is 0..256 samples).
const DELAY: u32 = 1;
const PID: PluginId = PluginId(1);

fn track(i: u64, uid: &str, plugin: bool) -> Arc<Track> {
    let mut t = Track::new(TrackId(100 + i), TrackKind::Audio, format!("T{i}"));
    if plugin {
        t.inserts.push(Plugin {
            samples: Vec::new(),
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
            preset: None,
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

/// Moves the delay in the playing instance's window and waits until it says it has it.
fn edit(server: &PluginServer, link: &PluginLink, value: f64) -> (u32, u32) {
    server.simulate_edit(link, DELAY, &[value]);
    let want = (value * 256.0).round() as u32;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let now = playing(server, link);
        if now.1 == want || Instant::now() >= deadline {
            return now;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_value_put_into_the_playing_plugin_while_a_new_mix_opens_stays_and_goes_on_to_the_new_one() {
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
    let mut p = Project::new("pending");
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
    let (old, delay) = playing(&server, &link);
    assert_eq!(delay, 0);

    // A track more needs a new mix, which opens the plugin again: it is pending a while.
    let mut p2 = p.clone();
    let t = track(1, &uid, false);
    p2.root.push(Node::Track(t.id));
    p2.tracks.insert(t.id, t);

    // While pending, a move in the window reaches the instance that plays.
    assert!(!host.sync_later(&p2).swapped);
    assert!(host.swap_pending());
    host.step_swap();
    assert_eq!(
        edit(&server, &link, 0.25),
        (old, 64),
        "the one heard has it"
    );
    assert!(host.swap_pending(), "the new mix is still opening");

    // The song goes back before the new mix came in: the instance that plays keeps it.
    host.sync_later(&p);
    assert!(!host.swap_pending());
    assert_eq!(playing(&server, &link), (old, 64), "not lost on the cancel");

    // Again, and this time the new mix comes in: the new instance has what was put in.
    assert!(!host.sync_later(&p2).swapped);
    host.step_swap();
    assert_eq!(edit(&server, &link, 0.5), (old, 128));
    let deadline = Instant::now() + Duration::from_secs(60);
    let report = loop {
        assert!(Instant::now() < deadline, "the mix never came in");
        if let Some(r) = host.step_swap() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(report.swapped, "{:?}", report.warnings);
    // From the swap the new instance is the one that plays, though the old mix lets go of the
    // old one only after the audio thread took the new mix. Asked now, the answer is its own.
    let (now, delay) = playing(&server, &link);
    assert_ne!(now, old, "the instance the new mix opened plays");
    assert_eq!(delay, 128, "with the value put into the old one");
    audio.render(&mut [0.0; 512], 2);
    host.controller().poll(|_| {});
    assert_eq!(playing(&server, &link), (now, 128));
}
