//! A value a macro control holds, moved in the plugin's own window, goes back to the control's
//! value in the plugin as well as in the song (MIX-039). The song kept the control's value and
//! took the window's state, so the plugin went on playing the hand's value while the song (and
//! the next time it is opened) had the control's.
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

/// The delay the playing instance has, read from the state it reports when asked.
fn delay(server: &PluginServer, link: &PluginLink) -> u32 {
    let request = server.request_state(link);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !link.capture_finished(request) {
        assert!(
            Instant::now() < deadline,
            "the plugin never reported its state"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = link
        .take_capture_edits(request)
        .unwrap()
        .state
        .expect("a state");
    link.cancel_capture(request);
    u32::from_le_bytes(state.component[..4].try_into().unwrap())
}

#[test]
fn a_value_a_macro_holds_moved_in_the_window_goes_back_to_the_macro_in_the_plugin() {
    let Some(exe) = find_host() else {
        eprintln!("opening in hosts needs the plugin host binary");
        return;
    };
    xirius_test_vst3::fixture_marker();
    let dll = xirius_test_vst3::fixture_path(std::env::current_exe().unwrap().parent().unwrap());
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let mut s = Session::with_registry(Project::new("owned-edit"), shared_registry());
    let t = run(&mut s, "track.add.audio", json!({ "name": "A" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let pid = run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Test Latency", "format": "vst3", "uid": uid }),
    )
    .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        &mut s,
        "plugin.isolation.set",
        json!({ "ids": [pid], "mode": "dedicated" }),
    );
    // The macro holds the delay at 192 (0.75 of 256).
    run(
        &mut s,
        "control.assign",
        json!({ "track": t, "control": "k1", "target": format!("param:{pid}:1") }),
    );
    run(
        &mut s,
        "control.set",
        json!({ "track": t, "control": "k1", "value": 0.75 }),
    );
    let held = |s: &Session| {
        s.project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .next()
            .unwrap()
            .params
            .get("1")
            .copied()
    };
    assert_eq!(held(&s), Some(0.75));
    let id = pid.parse().unwrap();
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
    host.set_nonblocking(true);
    let latency = |host: &SessionHost| {
        host.plugins()
            .reports()
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| r.status.latency)
    };
    let before = latency(&host).expect("the plugin plays");
    {
        let server = host.plugins().server_of(id).unwrap();
        let link = host.plugins().link(id).unwrap();
        assert_eq!(
            delay(server, &link),
            192,
            "the plugin opens with the macro's value"
        );
    }
    let steps = s.cursor();

    // The hand moves the delay to 128 in the plugin's window.
    let link = host.plugins().link(id).unwrap();
    host.plugins()
        .server_of(id)
        .unwrap()
        .simulate_edit(&link, 1, &[0.5]);

    // Frames of the screen until the edit is taken and the mix is settled again.
    let opts = RunOptions::human();
    let mut buffer = [0.0; 256 * 2];
    let mut taken = false;
    let mut moved = false;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        audio.render(&mut buffer, 2);
        host.poll();
        taken |= collect_plugin_states(&mut s, &mut host, &opts)
            .unwrap()
            .result
            .is_some();
        host.step_swap();
        moved |= latency(&host) != Some(before);
        let settled = taken && latency(&host) == Some(before) && !host.swap_pending();
        if settled || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let server = host.plugins().server_of(id).unwrap();
    let link = host.plugins().link(id).unwrap();
    let heard = delay(server, &link);
    eprintln!(
        "moved in the window to 128 (latency moved: {moved}); the song holds {:?}, the plugin plays {heard}",
        held(&s)
    );
    assert!(taken, "the window's state is in the song");
    assert_eq!(held(&s), Some(0.75), "the song keeps the macro's value");
    assert_eq!(heard, 192, "the plugin plays the macro's value again");
    assert_eq!(latency(&host), Some(before), "and the mix is built for it");
    assert_eq!(s.cursor(), steps + 1, "the window's state is one step");

    // The value sent back is not taken from the plugin as another edit.
    let calm = Instant::now() + Duration::from_millis(400);
    while Instant::now() < calm {
        audio.render(&mut buffer, 2);
        host.poll();
        let again = collect_plugin_states(&mut s, &mut host, &opts).unwrap();
        assert!(again.result.is_none(), "no edit came back");
        host.step_swap();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(held(&s), Some(0.75));
}
