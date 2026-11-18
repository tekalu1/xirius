//! The host reports a slow editor's opening before it finishes, and an error on failure.
//! The fixture window stays off every screen and never takes focus.
#![cfg(windows)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COMMAND,
};
use xirius_engine::server::{EditorOpenEvent, PluginLink, PluginServer, RemotePlugin, RemoteSpec};
use xirius_engine::{ProcessSetup, Processor, RenderMode};
use xirius_vst3::EditorAction;

mod crash_records;
use crash_records::CrashRecords;

fn wait_for(link: &PluginLink, wanted: impl Fn(&EditorOpenEvent) -> bool) -> Vec<EditorOpenEvent> {
    let seconds = std::env::var("XIRIUS_REAL_EDITOR_WAIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    let until = Instant::now() + Duration::from_secs(seconds);
    let mut events = Vec::new();
    loop {
        for event in link.take_editor_open_events() {
            let done = wanted(&event);
            events.push(event);
            if done {
                return events;
            }
        }
        assert!(
            Instant::now() < until,
            "editor report did not arrive: {events:?}"
        );
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
        std::env::set_var("XIRIUS_TEST_VST3_REQUIRE_OLE", "1");
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
        std::env::remove_var("XIRIUS_TEST_VST3_REQUIRE_OLE");
        std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_OPEN_MS");
        std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_FAIL");
    }
    assert_eq!(remote.error(), None);
    (host, link, remote)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn send_header_command(title: &str, command: usize) {
    // SAFETY: the class and title are terminated, and the command carries no pointers.
    unsafe {
        let frame = FindWindowW(wide("XiriusPluginEditor").as_ptr(), wide(title).as_ptr());
        assert!(!frame.is_null(), "the fixture editor frame is missing");
        let mut result = 0;
        let sent = SendMessageTimeoutW(
            frame,
            WM_COMMAND,
            command,
            0,
            SMTO_ABORTIFHUNG,
            5_000,
            &mut result,
        );
        assert_ne!(sent, 0, "the editor bar stopped responding");
    }
}

fn wait_for_action(link: &PluginLink, wanted: impl Fn(&EditorAction) -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if link.take_editor_actions().iter().any(&wanted) {
            return;
        }
        assert!(
            Instant::now() < until,
            "the editor command did not reach the client"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
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

#[test]
fn an_existing_editor_bar_answers_while_another_editor_opens_in_the_same_host() {
    let _records = CrashRecords::new();
    let (host, first, _first_remote) = open_fixture(false);
    host.open_editor(&first, "First fixture", None);
    wait_for(&first, |event| {
        matches!(event, EditorOpenEvent::Opened { .. })
    });

    let link = PluginLink::new();
    let mut remote = RemotePlugin::new(
        host.clone(),
        link.clone(),
        RemoteSpec {
            name: "second fixture".into(),
            bundle: std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("xirius_test_vst3.dll"),
            ..RemoteSpec::default()
        },
    );
    remote.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 128,
        mode: RenderMode::Offline,
    });
    assert_eq!(remote.error(), None);
    host.open_editor(&link, "Second fixture", None);
    wait_for(&link, |event| matches!(event, EditorOpenEvent::Opening));
    send_header_command("First fixture", 2);
    wait_for_action(&first, |action| {
        matches!(action, EditorAction::PresetMenu { .. })
    });
    wait_for(&link, |event| {
        matches!(event, EditorOpenEvent::Opened { .. })
    });
    host.close_editor(&first);
    host.close_editor(&link);
}

/// Local compatibility probe. The path is supplied by the caller and is never recorded here.
#[test]
#[ignore = "requires an installed editor bundle"]
fn two_real_editors_in_one_host_open_and_answer() {
    let Some(bundle) = std::env::var_os("XIRIUS_REAL_EDITOR_BUNDLE") else {
        return;
    };
    let _records = CrashRecords::new();
    let host = PluginServer::new(Some(PathBuf::from(env!(
        "CARGO_BIN_EXE_xirius-plugin-host"
    ))));
    let mut held = Vec::new();
    unsafe { std::env::set_var("XIRIUS_EDITOR_OFFSCREEN", "1") };
    let count = std::env::var("XIRIUS_REAL_EDITOR_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    for n in 0..count {
        let link = PluginLink::new();
        let mut remote = RemotePlugin::new(
            host.clone(),
            link.clone(),
            RemoteSpec {
                name: format!("instance {n}"),
                bundle: PathBuf::from(&bundle),
                ..RemoteSpec::default()
            },
        );
        remote.prepare(&ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Offline,
        });
        eprintln!("instance {n} prepare: {:?}", remote.error());
        assert!(remote.error().is_none());
        held.push((link, remote));
    }
    for (n, (link, _)) in held.iter().enumerate() {
        let at = Instant::now();
        let title = format!("Probe {n}");
        host.open_editor(link, &title, None);
        let events = wait_for(link, |event| {
            matches!(
                event,
                EditorOpenEvent::Opened { .. } | EditorOpenEvent::Failed { .. }
            )
        });
        eprintln!("instance {n} editor: {:?} in {:?}", events, at.elapsed());
        assert!(matches!(
            events.last(),
            Some(EditorOpenEvent::Opened { .. })
        ));
        send_header_command(&title, 2);
        wait_for_action(link, |action| {
            matches!(action, EditorAction::PresetMenu { .. })
        });
    }
    unsafe { std::env::remove_var("XIRIUS_EDITOR_OFFSCREEN") };
}
