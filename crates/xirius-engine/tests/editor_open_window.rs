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
        max_block: 512,
        mode: RenderMode::Realtime,
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

fn message(hwnd: windows_sys::Win32::Foundation::HWND, msg: u32, wp: usize, lp: isize) {
    let mut result = 0;
    // SAFETY: messages to this test's host; WM_SETTEXT marshals its terminated text across IPC.
    assert_ne!(
        unsafe { SendMessageTimeoutW(hwnd, msg, wp, lp, SMTO_ABORTIFHUNG, 5000, &mut result) },
        0
    );
}

#[test]
fn host_owned_preset_messages_filter_clear_close_and_deliver_the_choice() {
    use windows_sys::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};
    use xirius_vst3::{EditorBar, PresetRow};
    let _records = CrashRecords::new();
    let (host, link, _remote) = open_fixture(false);
    host.editor_bar(
        &link,
        EditorBar {
            menu: vec![
                PresetRow {
                    key: "load:Clean".into(),
                    label: "Clean".into(),
                    enabled: true,
                    selected: true,
                    footer: false,
                },
                PresetRow {
                    key: "load:Bright".into(),
                    label: "Bright".into(),
                    enabled: true,
                    ..Default::default()
                },
                PresetRow {
                    key: "off:@save".into(),
                    label: "Save".into(),
                    footer: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    host.open_editor(&link, "Preset message fixture", None);
    wait_for(&link, |event| {
        matches!(event, EditorOpenEvent::Opened { .. })
    });
    // These checks deliberately inspect messages and ownership only. Foreground/input behavior
    // is exercised by the opt-in desktop test, with real input and normal activation rules.
    unsafe {
        let frame = FindWindowW(
            wide("XiriusPluginEditor").as_ptr(),
            wide("Preset message fixture").as_ptr(),
        );
        send_header_command("Preset message fixture", 2);
        let mut popup = std::ptr::null_mut();
        loop {
            popup = FindWindowExW(
                std::ptr::null_mut(),
                popup,
                wide("XiriusPresetList").as_ptr(),
                std::ptr::null(),
            );
            assert!(!popup.is_null(), "host popup missing");
            if GetWindow(popup, GW_OWNER) == frame {
                break;
            }
        }
        let mut frame_process = 0;
        let mut popup_process = 0;
        GetWindowThreadProcessId(frame, &mut frame_process);
        GetWindowThreadProcessId(popup, &mut popup_process);
        assert_eq!(frame_process, popup_process);
        assert_ne!(popup_process, std::process::id());
        assert_eq!(GetWindowLongW(popup, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        let edit = GetDlgItem(popup, 1);
        message(edit, WM_SETTEXT, 0, wide("bRiGhT").as_ptr() as isize);
        message(edit, WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert_eq!(GetWindowTextLengthW(edit), 0);
        assert_ne!(IsWindowVisible(popup), 0, "first Escape only clears search");
        message(edit, WM_KEYDOWN, VK_ESCAPE as usize, 0);
        assert_eq!(IsWindowVisible(popup), 0);
        send_header_command("Preset message fixture", 2);
        send_header_command("Preset message fixture", 2);
        // Reopen creates a fresh snapshot of the song's rows.
        send_header_command("Preset message fixture", 2);
        let mut popup = std::ptr::null_mut();
        loop {
            popup = FindWindowExW(
                std::ptr::null_mut(),
                popup,
                wide("XiriusPresetList").as_ptr(),
                std::ptr::null(),
            );
            assert!(!popup.is_null());
            if GetWindow(popup, GW_OWNER) == frame {
                break;
            }
        }
        let edit = GetDlgItem(popup, 1);
        message(edit, WM_SETTEXT, 0, wide("bRiGhT").as_ptr() as isize);
        message(edit, WM_KEYDOWN, VK_RETURN as usize, 0);
        assert_eq!(IsWindowVisible(popup), 0);
        wait_for_action(&link, |a| {
            *a == EditorAction::PresetPick("load:Bright".into())
        });
    }
    host.close_editor(&link);
}

#[test]
fn hosts_acknowledge_placement_and_preserve_the_requested_editor_order() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use xirius_vst3::EditorBar;
    let _records = CrashRecords::new();
    let (a, al, _ar) = open_fixture(false);
    let (b, bl, _br) = open_fixture(false);
    for (host, link, name) in [
        (&a, &al, "Stack first fixture"),
        (&b, &bl, "Stack second fixture"),
    ] {
        host.open_editor(link, name, None);
        wait_for(link, |e| matches!(e, EditorOpenEvent::Opened { .. }));
    }
    // SAFETY: query the two parked fixture frames. They never take foreground focus.
    unsafe {
        let first = FindWindowW(
            wide("XiriusPluginEditor").as_ptr(),
            wide("Stack first fixture").as_ptr(),
        );
        let second = FindWindowW(
            wide("XiriusPluginEditor").as_ptr(),
            wide("Stack second fixture").as_ptr(),
        );
        assert!(!first.is_null() && !second.is_null());
        for (ticket, stack) in [
            (51, vec![first as u64, second as u64]),
            (52, vec![second as u64, first as u64]),
        ] {
            for hwnd in &stack {
                let (host, link) = if *hwnd == first as u64 {
                    (&a, &al)
                } else {
                    (&b, &bl)
                };
                host.editor_bar(
                    link,
                    EditorBar {
                        stack: stack.clone(),
                        raise_ticket: ticket,
                        ..Default::default()
                    },
                );
                host.raise_editor(link);
                wait_for_action(link, |action| *action == EditorAction::EditorRaised(ticket));
            }
            let mut order = Vec::new();
            let mut at = GetTopWindow(std::ptr::null_mut());
            while !at.is_null() {
                if stack.contains(&(at as u64)) {
                    order.push(at as u64);
                }
                at = GetWindow(at, GW_HWNDNEXT);
            }
            assert_eq!(order, stack);
            assert!(GetWindow(first, GW_OWNER).is_null());
            assert!(GetWindow(second, GW_OWNER).is_null());
        }
    }
    a.close_editor(&al);
    b.close_editor(&bl);
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
        max_block: 512,
        mode: RenderMode::Realtime,
    });
    assert_eq!(remote.error(), None);
    host.open_editor(&link, "Second fixture", None);
    wait_for(&link, |event| matches!(event, EditorOpenEvent::Opening));
    send_header_command("First fixture", 1);
    wait_for_action(&first, |action| matches!(action, EditorAction::Power));
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
            mode: RenderMode::Realtime,
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
        send_header_command(&title, 1);
        wait_for_action(link, |action| matches!(action, EditorAction::Power));
    }
    unsafe { std::env::remove_var("XIRIUS_EDITOR_OFFSCREEN") };
}
