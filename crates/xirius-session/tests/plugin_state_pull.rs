//! A plugin that changes its patch without saying so (Keyscape does) still reaches the song
//! when the song is written or left (PLG-057). The fixture's "Latency" parameter is such a
//! change: it moves the plugin's state and raises neither a gesture nor the values-changed
//! flag, so the notifications the song waits for never come. Nothing here opens an audio
//! device: the engine is pulled by the test.
//!
//! The remote case skips when `xirius-plugin-host` has not been built (`cargo test
//! --workspace` builds it).

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use xirius_core::model::{PluginIsolation, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::server::find_host;
use xirius_plugins::{Catalog, decode_state, encode_state, probe};
use xirius_session::commands::{self, shared_registry};
use xirius_session::host::{
    HostConfig, PULL_WAIT, SessionHost, collect_plugin_states, pull_plugin_states,
};
use xirius_session::plugins::Isolation;

/// "Latency" 0.5 is a delay of 128, which the fixture saves as 4 little-endian bytes.
const EDITED: [u8; 4] = 128u32.to_le_bytes();

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(raw) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &raw, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn state_of(s: &Session) -> Option<Vec<u8>> {
    let state = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .state
        .clone()?;
    let s = decode_state(&state).unwrap();
    assert_eq!(s.component, s.controller, "the fixture keeps both alike");
    Some(s.component)
}

/// A song with the fixture on a track, the plugin id, and the catalog that has it.
fn song(isolation: PluginIsolation) -> (Session, Arc<Catalog>) {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    assert!(
        dll.exists(),
        "the fixture's cdylib is built with its dev dependency"
    );
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let mut s = Session::with_registry(Project::new("pull"), shared_registry());
    let t = run(&mut s, "track.add.audio", json!({ "name": "A" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Test Latency", "format": "vst3", "uid": uid }),
    );
    let id = r.data["id"].as_str().unwrap().to_string();
    run(
        &mut s,
        "plugin.isolation.set",
        json!({ "ids": id, "mode": isolation.name() }),
    );
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    (s, catalog)
}

fn pull_check(isolation: Isolation, mode: PluginIsolation) {
    let (mut s, catalog) = song(mode);
    let id = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;
    run(
        &mut s,
        "plugin.param.set",
        json!({"id":id.to_string(),"param":"1","value":0.25}),
    );
    let (mut host, mut audio, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(isolation),
            ..HostConfig::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut buffer = [0.0; 256 * 2];
    for _ in 0..4 {
        audio.render(&mut buffer, 2);
        host.poll();
    }
    let opts = RunOptions::human();

    // The plugin's patch changes without a word.
    if let Some(link) = host.plugins().link(id) {
        let server = host.plugins().server_of(id).unwrap();
        server.set_param(&link, "Latency", 0.5);
    } else {
        let (plugin, _) = host
            .plugins()
            .local(id)
            .expect("the instance is in process");
        assert!(plugin.set_param(1, 0.5));
    }
    for _ in 0..40 {
        audio.render(&mut buffer, 2);
        host.poll();
        std::thread::sleep(Duration::from_millis(10));
    }

    // What the song waits for does not come: the song still has the old state.
    let before = s.cursor();
    let passive = collect_plugin_states(&mut s, &mut host, &opts).unwrap();
    assert!(passive.result.is_none(), "no notification, no state");
    assert_eq!(state_of(&s), None);
    assert_eq!(s.cursor(), before);

    // Asked, the plugin gives it: one step, and the song has the changed state.
    let pulled = pull_plugin_states(&mut s, &mut host, &opts, PULL_WAIT).unwrap();
    assert!(pulled.result.is_some(), "the changed state came");
    assert_eq!(state_of(&s), Some(EDITED.to_vec()));
    assert_eq!(
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == id)
            .unwrap()
            .params["1"],
        0.5,
        "captured values must replace stale named overrides for the next open"
    );
    assert!(
        pulled.params.is_empty(),
        "a capture snapshot never records automation gestures"
    );
    assert_eq!(s.cursor(), before + 1, "one step");
    let state = encode_state(&xirius_vst3::PluginState {
        component: EDITED.to_vec(),
        controller: EDITED.to_vec(),
    });
    assert_eq!(
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .next()
            .unwrap()
            .state
            .as_deref(),
        Some(state.as_str())
    );

    // Asked again with nothing changed, it adds nothing.
    let again = pull_plugin_states(&mut s, &mut host, &opts, PULL_WAIT).unwrap();
    assert!(again.result.is_none());
    assert_eq!(s.cursor(), before + 1);
}

#[test]
fn a_silent_patch_change_in_a_plugin_in_the_app_reaches_the_song_when_it_is_asked() {
    pull_check(Isolation::InProcess, PluginIsolation::InProcess);
}

#[test]
fn a_silent_patch_change_in_a_plugin_in_the_host_process_reaches_the_song_when_it_is_asked() {
    let Some(exe) = find_host() else {
        eprintln!("skipped: xirius-plugin-host is not built");
        return;
    };
    pull_check(Isolation::Separate(exe), PluginIsolation::Shared);
}

#[test]
fn a_live_timeline_edit_keeps_the_remote_slot_and_its_unreported_patch() {
    let exe = find_host().expect("build xirius-plugin-host for the remote regression");
    let (mut s, catalog) = song(PluginIsolation::Shared);
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| !t.inserts.is_empty())
        .unwrap()
        .id;
    let id = s.project().tracks[&track].inserts[0].id;
    let (mut host, mut audio, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(catalog),
            isolation: Some(Isolation::Separate(exe)),
            threaded_local: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut buffer = [0.0; 512];
    for _ in 0..8 {
        audio.render(&mut buffer, 2);
        host.poll();
    }
    let link = host.plugins().link(id).unwrap();
    let server = host.plugins().server_of(id).unwrap().clone();
    let slot = server.current(&link).unwrap().slot;
    server.set_param(&link, "Latency", 0.5);
    // This fixture reports a latency change, but no state/value change. Consume
    // that independent PDC request to isolate an unrelated timeline edit here.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while link.take_latency_change() != Some(128) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let root = tempfile::tempdir().unwrap();
    let wav = root.path().join("unrelated.wav");
    xirius_session::export::write_wav(&wav, &[0.25; 8192], &[0.25; 8192], 48_000, 16).unwrap();
    run(
        &mut s,
        "clip.add.audio",
        json!({"file":wav,"track":track.to_string(),"at":0,"length":1920}),
    );
    host.set_windowed();
    host.sync(s.project());
    assert!(host.preparation_target().is_none());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while host.swap_pending() {
        host.step_swap();
        audio.render(&mut buffer, 2);
        host.poll();
        assert!(
            host.preparation_target().is_none(),
            "material edits do not replace plugins"
        );
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(Arc::ptr_eq(&link, &host.plugins().link(id).unwrap()));
    assert_eq!(
        server.current(&link).unwrap().slot,
        slot,
        "no SDK/editor replacement"
    );
    assert_eq!(state_of(&s), None, "the patch had no notification");
    pull_plugin_states(&mut s, &mut host, &RunOptions::human(), PULL_WAIT).unwrap();
    assert_eq!(state_of(&s), Some(EDITED.to_vec()));

    // A changed insert row really needs a new plan. Capture the still-running
    // slot explicitly before making its replacement; keep the output alive.
    server.set_param(&link, "Latency", 0.75);
    run(
        &mut s,
        "plugin.add",
        json!({"track":track.to_string(),"name":"Gain",
        "kind":"effect","format":"builtin","uid":"gain"}),
    );
    host.sync(s.project());
    // Offline rendering must not reach the short fixture's natural end while
    // the background SDK capture runs. This tests a continuing transport.
    assert!(
        host.controller()
            .send(xirius_engine::Command::SetEnd(None))
            .is_ok()
    );
    assert!(host.play());
    while host.swap_pending() {
        if let Some(report) = host.step_swap() {
            assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        }
        audio.render(&mut buffer, 2);
        host.poll();
        assert!(std::time::Instant::now() < deadline, "live graph watchdog");
        std::thread::yield_now();
    }
    assert!(host.poll().playing, "graph publication preserves playback");
    pull_plugin_states(&mut s, &mut host, &RunOptions::human(), PULL_WAIT).unwrap();
    assert_eq!(state_of(&s), Some(192u32.to_le_bytes().to_vec()));
}

#[test]
fn a_plugin_that_was_never_created_keeps_its_saved_state_after_silent_continuation() {
    let Some(exe) = find_host() else {
        eprintln!("skipped: xirius-plugin-host is not built");
        return;
    };
    let (mut s, catalog) = song(PluginIsolation::Shared);
    let id = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;
    let original = 64u32.to_le_bytes().to_vec();
    let state = encode_state(&xirius_vst3::PluginState {
        component: original.clone(),
        controller: original.clone(),
    });
    run(
        &mut s,
        "plugin.state.set",
        json!({"ids":id.to_string(), "state":state}),
    );
    let root = tempfile::tempdir().unwrap();
    let mut entries = catalog.entries.clone();
    for entry in &mut entries {
        entry.path = root.path().join("not-installed.vst3");
    }
    let mut starting = xirius_session::starting::Starting::new(
        s.project(),
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: vec![],
            })),
            isolation: Some(Isolation::Separate(exe)),
            threaded_local: true,
            ..HostConfig::default()
        },
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !starting.step() {
        assert!(
            std::time::Instant::now() < deadline,
            "missing plugin watchdog"
        );
        std::thread::yield_now();
    }
    let (mut host, _audio, failures) = starting.finish_prepared(s.project(), |_, _| {}).unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(host.unavailable_tracks().len(), 1);
    let before = s.cursor();
    let pulled = pull_plugin_states(&mut s, &mut host, &RunOptions::human(), PULL_WAIT).unwrap();
    assert!(pulled.result.is_none());
    assert_eq!(state_of(&s), Some(original));
    assert_eq!(s.cursor(), before, "no runtime instance made any edit");
}
