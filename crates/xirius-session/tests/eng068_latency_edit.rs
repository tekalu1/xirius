//! A value moved in a plugin's own window that changes its latency builds the mix again even
//! when the edit is taken into the song in the same frame (ENG-068). The screen takes the
//! window's edits into the song right after the poll that heard of the new latency, and doing
//! so synced the engine at once, which dropped the mix that had just begun for the latency:
//! the plugin went on with the new delay and the mix with the old compensation.
//!
//! The new instance is opened from what the playing one has now: with a value the song set
//! before, it must have the value moved in the window, not the one it was opened with (ENG-069).
//!
//! Skips when `xirius-plugin-host` has not been built (`cargo test --workspace` builds it).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::server::{PluginLink, PluginServer, find_host};
use xirius_plugins::{Catalog, probe};
use xirius_session::commands::{self, shared_registry};
use xirius_session::host::{HostConfig, SessionHost, collect_plugin_states};
use xirius_session::plugins::Isolation;

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(raw) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &raw, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// The delay of the instance that plays, read from the state it reports when asked.
fn delay(server: &PluginServer, link: &PluginLink) -> u32 {
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
    let state = server.current(link).and_then(|o| o.state).expect("a state");
    u32::from_le_bytes(state.component[..4].try_into().unwrap())
}

#[test]
fn a_latency_moved_in_the_window_is_compensated_when_the_edit_is_taken_at_once() {
    move_in_window(None);
}

#[test]
fn the_new_instance_has_the_value_moved_in_the_window_over_the_one_the_song_had() {
    // The song opens it with delay 192; the window moves it to 128.
    move_in_window(Some(0.75));
}

/// Opens the test plugin (with the song's value `start` for its delay, if any), moves the
/// delay to 128 in its window and runs the screen's frames until the mix plays the new latency.
fn move_in_window(start: Option<f64>) {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let mut s = Session::with_registry(Project::new("latency-edit"), shared_registry());
    let t = run(&mut s, "track.add.audio", json!({ "name": "A" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Test Latency", "format": "vst3", "uid": uid }),
    );
    run(
        &mut s,
        "plugin.isolation.set",
        json!({ "ids": r.data["id"], "mode": "dedicated" }),
    );
    if let Some(v) = start {
        run(
            &mut s,
            "plugin.param.set",
            json!({ "id": r.data["id"], "param": "1", "value": v }),
        );
    }
    let id = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;
    let (mut host, mut audio, warnings) = SessionHost::start(
        s.project(),
        HostConfig {
            catalog: Some(Arc::new(Catalog {
                entries,
                failures: Vec::new(),
            })),
            isolation: Some(Isolation::PerPlugin(Some(exe))),
            ..HostConfig::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    // As the screen's player: a new latency builds its mix while the old one plays on.
    host.set_nonblocking(true);
    let latency = |host: &SessionHost| {
        host.plugins()
            .reports()
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| r.status.latency)
    };
    let before = latency(&host).expect("the plugin plays");
    let base = before - start.map_or(0, |v| (v * 256.0).round() as u32);
    let opts = RunOptions::human();
    let mut buffer = [0.0; 256 * 2];

    // As the window does when its value is moved: the plugin's own edit, which sends the
    // edit and the new latency together.
    let link = host.plugins().link(id).unwrap();
    host.plugins()
        .server_of(id)
        .unwrap()
        .simulate_edit(&link, 1, &[0.5]);

    // A frame of the screen: the poll, the window's edits into the song, a step of the swap.
    let mut taken = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    while latency(&host) != Some(base + 128) && Instant::now() < deadline {
        audio.render(&mut buffer, 2);
        host.poll();
        taken |= collect_plugin_states(&mut s, &mut host, &opts)
            .unwrap()
            .result
            .is_some();
        host.step_swap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(taken, "the edit is in the song");
    assert_eq!(
        latency(&host),
        Some(base + 128),
        "the mix is built with the latency the plugin reports"
    );
    assert!(!host.swap_pending());
    let server = host.plugins().server_of(id).unwrap();
    let link = host.plugins().link(id).unwrap();
    assert_eq!(
        delay(server, &link),
        128,
        "the instance that plays has the value moved in the window"
    );
}
