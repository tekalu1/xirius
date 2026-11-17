//! The host reports a slow editor's opening before it finishes, and an error on failure.
//! The fixture window stays off every screen and never takes focus.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_engine::server::{EditorOpenEvent, PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::{ProcessSetup, Processor, RenderMode};

mod crash_records;
use crash_records::CrashRecords;

fn wait_for(link: &PluginLink, wanted: impl Fn(&EditorOpenEvent) -> bool) -> Vec<EditorOpenEvent> {
    let until = Instant::now() + Duration::from_secs(15);
    let mut events = Vec::new();
    loop {
        for event in link.take_editor_open_events() {
            let done = wanted(&event);
            events.push(event);
            if done {
                return events;
            }
        }
        assert!(Instant::now() < until, "editor report did not arrive");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn open_fixture(fail: bool) -> (PluginServer, Arc<PluginLink>, RemotePlugin) {
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let bundle = deps.join("xirius_test_vst3.dll");
    assert!(
        bundle.exists(),
        "build the test fixture before this window test"
    );
    let host = PluginServer::new(Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_xirius-plugin-host"
    ))));
    let link = PluginLink::new();
    let mut remote = RemotePlugin::new(
        host.clone(),
        link.clone(),
        RemoteSpec {
            name: "fixture".into(),
            bundle,
            ..RemoteSpec::default()
        },
    );
    // The host inherits these values when the first instance starts.
    unsafe {
        std::env::set_var("XIRIUS_EDITOR_OFFSCREEN", "1");
        std::env::set_var("XIRIUS_TEST_VST3_EDITOR_OPEN_MS", "400");
        if fail {
            std::env::set_var("XIRIUS_TEST_VST3_EDITOR_FAIL", "1");
        }
    }
    remote.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 128,
        mode: RenderMode::Offline,
    });
    unsafe {
        std::env::remove_var("XIRIUS_EDITOR_OFFSCREEN");
        std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_OPEN_MS");
        std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_FAIL");
    }
    assert_eq!(remote.error(), None);
    (host, link, remote)
}

#[test]
fn slow_editor_reports_start_then_dimensions_and_failure_reports_reason() {
    let _records = CrashRecords::new();
    let (host, link, _remote) = open_fixture(false);
    host.open_editor(&link, "Fixture", None);
    let events = wait_for(&link, |event| {
        matches!(event, EditorOpenEvent::Opened { .. })
    });
    assert_eq!(events.first(), Some(&EditorOpenEvent::Opening));
    assert!(
        matches!(events.last(), Some(EditorOpenEvent::Opened { size: (w, h), .. }) if *w > 0 && *h > 0)
    );
    host.close_editor(&link);

    let (host, link, _remote) = open_fixture(true);
    host.open_editor(&link, "Fixture", None);
    let events = wait_for(&link, |event| {
        matches!(event, EditorOpenEvent::Failed { .. })
    });
    assert_eq!(events.first(), Some(&EditorOpenEvent::Opening));
    assert!(matches!(
        events.last(),
        Some(EditorOpenEvent::Failed { reason }) if !reason.is_empty()
    ));
}
