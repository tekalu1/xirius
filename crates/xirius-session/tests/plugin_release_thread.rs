//! A plugin in the app is terminated on the thread that made it, not on the output's thread,
//! when the song's player goes (ARC-044). iZotope Trash 2 ties a hidden window to the thread
//! that made it, and when another thread released it, the DLL was gone before the window
//! closed and the app crashed on the window's message. The transport drops the session host
//! before the output, so the engine the output plays holds the last reference to the
//! instance. Nothing here opens an audio device: the output is the silent one. One test in its
//! own process, so no other test counts the fixture's terminations meanwhile.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use xirius_core::model::{PluginIsolation, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, probe};
use xirius_session::commands::{self, shared_registry};
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::output::{Output, OutputMode};
use xirius_session::plugins::Isolation;
use xirius_vst3::Placement;

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let Value::Object(raw) = args else {
        unreachable!()
    };
    commands::run(s, None, name, &raw, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// The plugin's window while the song's player goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Window {
    None,
    Open,
    /// Opened and closed before the player goes.
    Closed,
}

/// The instances the fixture has terminated, and of those, the ones terminated on a thread
/// other than the one that made them.
fn terminations(fixture: &libloading::Library) -> (u32, u32) {
    // SAFETY: the fixture's export, with the ABI it is declared with.
    let both = unsafe {
        let f = fixture
            .get::<unsafe extern "system" fn() -> u64>(b"FixtureTerminations")
            .unwrap();
        f()
    };
    (both as u32, (both >> 32) as u32)
}

#[test]
fn a_plugin_in_the_app_is_terminated_on_its_own_thread_when_the_player_goes() {
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
    // SAFETY: the fixture runs nothing when it is loaded; it stays loaded with the host's.
    let fixture = unsafe { libloading::Library::new(&dll) }.unwrap();
    let entries = probe(&dll).unwrap();
    let uid = entries[0].uid.clone();
    let catalog = Arc::new(Catalog {
        entries,
        failures: Vec::new(),
    });
    let mut s = Session::with_registry(Project::new("release"), shared_registry());
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
        json!({ "ids": id, "mode": PluginIsolation::InProcess.name() }),
    );

    let plugin = s
        .project()
        .tracks
        .values()
        .flat_map(|t| &t.inserts)
        .next()
        .unwrap()
        .id;

    // The transport's order (the host first), the output first, the transport's order with the
    // plugin's window open, which the screen closes after the player goes, and with the window
    // opened and closed before. The window opens off every screen and is never shown.
    let cases = [
        (true, Window::None),
        (false, Window::None),
        (true, Window::Open),
        (true, Window::Closed),
    ];
    for (host_first, window) in cases {
        let (made_before, away_before) = terminations(&fixture);
        let (mut host, audio, warnings) = SessionHost::start(
            s.project(),
            HostConfig {
                catalog: Some(catalog.clone()),
                isolation: Some(Isolation::InProcess),
                ..HostConfig::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(
            host.plugins().local(plugin).is_some(),
            "the instance is in the app"
        );
        let output = Output::start(OutputMode::Silent { paced: true }, audio)
            .unwrap()
            .unwrap();
        let mut editor = (window != Window::None).then(|| {
            let (p, _) = host.plugins().local(plugin).unwrap();
            p.open_editor_at("Xirius Test Latency", Placement::offscreen())
                .expect("the fixture has a window")
        });
        for _ in 0..10 {
            host.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        if window == Window::Closed {
            drop(editor.take());
        }
        if host_first {
            drop(host);
            drop(output);
        } else {
            drop(output);
            drop(host);
        }
        drop(editor);
        let (made, away) = terminations(&fixture);
        assert_eq!(made, made_before + 1, "the instance was terminated");
        assert_eq!(
            away, away_before,
            "terminated on the thread that made it (host first: {host_first}, window: {window:?})"
        );
    }
}
