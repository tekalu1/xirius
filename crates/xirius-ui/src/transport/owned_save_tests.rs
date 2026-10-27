//! No window or sound device: the actual save entry while the SDK call is held.
use super::*;
use crate::keep::SaveMark;
use crate::song::Song;
use crate::studio::FileRequest;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};

struct Gate {
    entered: HANDLE,
    release: HANDLE,
    arm: unsafe extern "system" fn(usize, usize),
}
impl Gate {
    fn new(path: &std::path::Path) -> Self {
        Self::named(path, c"FixtureStateGate")
    }
    fn named(path: &std::path::Path, name: &std::ffi::CStr) -> Self {
        let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: the loader retains this fixture; its exported function has this ABI.
        unsafe {
            let module = GetModuleHandleW(wide.as_ptr());
            let arm = std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(usize, usize),
            >(GetProcAddress(module, name.as_ptr().cast()).unwrap());
            let entered = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            let release = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            assert!(!entered.is_null() && !release.is_null());
            arm(entered as usize, release as usize);
            Self {
                entered,
                release,
                arm,
            }
        }
    }
    fn entered(&self) -> bool {
        // SAFETY: query a live event without waiting.
        unsafe { WaitForSingleObject(self.entered, 0) == WAIT_OBJECT_0 }
    }
    fn release(&self) {
        // SAFETY: live manual reset event.
        assert_ne!(unsafe { SetEvent(self.release) }, 0);
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        // SAFETY: release on every exit. On panic keep the handles valid until
        // process exit because the owner could still be inside the fixture.
        unsafe {
            SetEvent(self.release);
            if !std::thread::panicking() {
                (self.arm)(0, 0);
                CloseHandle(self.entered);
                CloseHandle(self.release);
            }
        }
    }
}

#[test]
fn held_sdk_initialization_can_be_abandoned_before_the_sdk_returns() {
    xirius_test_vst3::fixture_marker();
    let source = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let root = tempfile::tempdir().unwrap();
    let dll = root.path().join("held-init.vst3");
    std::fs::copy(source, &dll).unwrap();
    let catalog = Arc::new(Catalog {
        entries: xirius_plugins::probe(&dll).unwrap(),
        failures: Vec::new(),
    });
    let gate = Gate::named(&dll, c"FixtureOpenGate");
    let wide: Vec<_> = dll.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the private fixture remains loaded; the export returns a counter.
    let terminated = unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn() -> u64,
        >(
            GetProcAddress(
                GetModuleHandleW(wide.as_ptr()),
                c"FixtureTerminations".as_ptr().cast(),
            )
            .unwrap(),
        )
    };
    let mut s = Studio::new(Song::empty());
    let track = s
        .song
        .run("track.add.audio", json!({"name":"Held initialization"}))
        .unwrap()["id"]
        .clone();
    s.song
        .run(
            "plugin.add",
            json!({"track":track,"name":"Held","format":"vst3","uid":catalog.entries[0].uid}),
        )
        .unwrap();
    s.transport.mode = OutputMode::Device;
    s.transport.set_catalog(catalog);
    s.transport
        .set_drivers(super::prepared::tests::quiet_drivers());
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !gate.entered() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "initialization gate watchdog");
        std::thread::yield_now();
    }
    assert!(!s.run_command("track.add.audio", json!({"name":"blocked"})));
    assert!(s.prepare_key("escape"));
    assert!(s.preparing.as_ref().unwrap().cancelling);
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(
            Instant::now() < deadline,
            "cancel held initialization watchdog"
        );
        std::thread::yield_now();
    }
    assert!(s.song.dir().is_none());
    assert!(s.transport.host().is_none());
    // The fixture is still stopped. Even starting a new, empty player completes.
    s.prepare_song();
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "next preparation watchdog");
        std::thread::yield_now();
    }
    assert!(s.transport.host().is_some());
    gate.release();
    let deadline = Instant::now() + Duration::from_secs(20);
    // SAFETY: query this private fixture after release, before closing its events.
    while unsafe { terminated() } == 0 {
        assert!(
            Instant::now() < deadline,
            "abandoned initialization cleanup watchdog"
        );
        std::thread::yield_now();
    }
}

#[test]
fn saving_does_not_write_or_wait_for_held_state_and_only_a_complete_retry_clears_failure() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let catalog = Arc::new(Catalog {
        entries: xirius_plugins::probe(&dll).unwrap(),
        failures: Vec::new(),
    });
    let mut s = Studio::new(Song::empty());
    let track = s.song.run("track.add.audio", json!({"name":"A"})).unwrap()["id"].clone();
    s.song
        .run(
            "plugin.add",
            json!({
                "track":track,"name":"Fixture","format":"vst3","uid":catalog.entries[0].uid,
            }),
        )
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("original");
    s.song.save_as(&dir).unwrap();
    s.transport.mode = OutputMode::Device;
    s.transport.bind_window();
    s.transport.set_catalog(catalog);
    s.transport.set_drivers(Drivers {
        exclusive_out: None,
        shared_out: Arc::new(|audio, _, _| {
            Ok(Output::Silent(xirius_session::output::SilentOutput::start(
                audio, true,
            )))
        }),
        exclusive_in: None,
        shared_in: Arc::new(|_, _, _| Err("no test input".into())),
        devices: Arc::new(Vec::new),
    });
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "initial preparation watchdog");
        std::thread::yield_now();
    }
    assert!(s.error.is_none(), "{:?}", s.error);
    let host = s.transport.host().unwrap();
    assert!(!host.plugins().owner_retirements().is_empty());
    assert!(!s.transport.playing);
    let gate = Gate::new(&dll);
    let target = temp.path().join("copy");
    s.start_file_request(FileRequest::SaveAs, &target);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.entered() {
        s.poll_saving();
        assert!(s.saving.is_some());
        assert!(Instant::now() < deadline, "SDK gate watchdog");
        std::thread::yield_now();
    }
    // The event, rather than a duration, proves the SDK cannot complete.
    assert!(!target.exists());
    let project = s.project();
    assert!(!s.run_command("track.add.audio", json!({"name":"blocked"})));
    assert!(Arc::ptr_eq(&project, &s.project()));
    assert!(s.saving_key("escape"));
    assert!(s.poll_saving());
    assert!(s.saving.is_none());
    assert!(!target.exists());

    assert!(s.start_save());
    // Advance the deadline explicitly while the SDK event remains closed.
    s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
    assert!(s.poll_saving());
    assert!(s.saving.is_none());
    assert!(s.keep.plugin_state_failed);
    assert_eq!(s.save_mark(), SaveMark::Failed);
    s.save_succeeded(); // A completed disk checkpoint is insufficient.
    assert_eq!(s.save_mark(), SaveMark::Failed);
    assert!(!target.exists());
    // A connected MCP save uses this same preflight, and must not report success
    // or copy a song while its SDK cannot return state.
    for command in [
        "project.save",
        "project.save-as",
        "version.save",
        "track.add.audio",
    ] {
        use xirius_mcp::Host;
        let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
        let path = temp
            .path()
            .join(format!("mcp-{}", command.replace('.', "-")));
        let args = match command {
            "project.save-as" => json!({"path":path}),
            "version.save" => json!({"name":"mcp-held"}),
            "track.add.audio" => json!({"name":"mcp-unsaved"}),
            _ => json!({}),
        };
        let call = std::thread::spawn(move || {
            bridge.run(
                command,
                args.as_object().unwrap(),
                &xirius_core::session::RunOptions::human(),
                true,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let job = loop {
            if let Ok(job) = jobs.try_recv() {
                break job;
            }
            assert!(Instant::now() < deadline, "MCP receipt watchdog");
            std::thread::yield_now();
        };
        s.ai_job(job);
        assert!(s.saving.is_some(), "{command}: save is deferred");
        assert!(!call.is_finished(), "{command}: no early success reply");
        s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
        s.poll_saving();
        assert!(call.join().unwrap().is_err(), "{command}");
        assert_eq!(s.save_mark(), SaveMark::Failed);
        assert!(!path.exists(), "{command}: no file without SDK state");
        assert!(!s.song.versions().iter().any(|v| v.name == "mcp-held"));
    }
    // A read receipt must not request SDK state, even with the ordinary MCP
    // persistence flag while this owner's state collection remains held.
    let read = crate::review_io_tests::request_with_save(
        &mut s,
        "project.get",
        json!({}),
        xirius_core::session::RunOptions::human(),
        true,
    );
    crate::review_io_tests::finish(&mut s);
    read.join().unwrap().unwrap();
    let project = s.project();
    let export = temp.path().join("held.dawproject");
    let approval_path = temp.path().join("approved-copy");
    s.song
        .session_mut()
        .set_confirm(xirius_core::session::Confirm::OPEN_04);
    let proposal = s
        .song
        .session_mut()
        .run(
            "project.save-as",
            json!({"path":approval_path}).as_object().unwrap(),
            &xirius_core::session::RunOptions::ai("AI"),
        )
        .unwrap();
    let token = proposal.decision.unwrap().token;
    assert!(s.run_command("ai.decide", json!({"token":token,"answer":"yes"})));
    assert!(s.saving.is_some(), "approval also waits for owner state");
    s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
    s.poll_saving();
    assert!(s.saving.is_none());
    assert!(!approval_path.exists());
    assert!(
        s.song
            .session()
            .save_request(
                "ai.decide",
                json!({"token":token,"answer":"yes"}).as_object().unwrap()
            )
            .is_some(),
        "failed capture does not consume approval"
    );
    s.run_command("ai.decide", json!({"token":token,"answer":"no"}));
    assert!(s.dawproject_export(&export));
    s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
    s.poll_saving();
    assert!(s.saving.is_none());
    assert!(s.dawproject.is_none());
    assert!(!export.exists(), "failed capture never exports old state");
    assert!(s.unsaved.is_none(), "export failure is not a quit request");
    s.version_with_plugin_states("held".into());
    s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
    s.poll_saving();
    assert!(
        !s.song
            .versions()
            .iter()
            .any(|version| version.name == "held")
    );
    assert!(s.unsaved.is_none());
    assert!(!s.request_close());
    assert!(s.unsaved.is_some());
    assert!(!s.run_command("track.add.audio", json!({"name":"blocked"})));
    assert!(s.unsaved_key("escape"));
    assert!(s.unsaved.is_none());
    assert!(s.window_ask.is_none());
    assert!(Arc::ptr_eq(&project, &s.project()));
    assert!(s.request_open(crate::opening::Request::New));
    s.poll_opening();
    assert!(s.unsaved.is_some());
    assert!(s.unsaved_key("enter"), "default is Back");
    assert!(s.unsaved.is_none());
    assert!(s.opening.is_none());
    assert!(Arc::ptr_eq(&project, &s.project()));
    gate.release();
    assert!(s.start_save());
    s.wait_for_save();
    assert!(!s.keep.plugin_state_failed);
    assert!(s.keep.failed.is_none(), "{:?}", s.keep.failed);
    assert!(s.error.is_none(), "{:?}", s.error);
    drop(gate);
    let gate = Gate::new(&dll);
    assert!(s.start_save());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.entered() {
        s.poll_saving();
        assert!(Instant::now() < deadline, "second SDK gate watchdog");
        std::thread::yield_now();
    }
    s.saving.as_mut().unwrap().since -= xirius_session::host::PULL_WAIT;
    s.poll_saving();
    let retired = s.transport.host().unwrap().plugins().owner_retirements();
    assert!(!s.request_close());
    assert!(s.unsaved.is_some());
    s.answer_unsaved(true);
    s.wait_for_save();
    assert!(s.leaving_ready);
    assert!(s.keep.plugin_state_failed, "discard is never save success");
    assert!(matches!(
        s.window_ask,
        Some(crate::context::key_more::WindowAsk::Quit)
    ));
    s.window_ask = None;
    s.leaving_ready = false;
    assert!(s.request_open(crate::opening::Request::New));
    s.poll_opening();
    assert!(s.unsaved.is_some());
    s.answer_unsaved(true);
    s.wait_for_open_for_test();
    assert!(s.song.dir().is_none());
    assert!(s.unsaved.is_none());
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(
            Instant::now() < deadline,
            "new song waited on the blocked SDK owner"
        );
        std::thread::yield_now();
    }
    assert!(
        s.transport.host().is_some(),
        "the next output is ready before getState returns"
    );
    assert!(retired.iter().all(|owner| owner.audio_returned()));
    assert!(
        retired.iter().any(|owner| !owner.finished()),
        "the SDK gate is still closed"
    );
    assert!(!s.transport.playing);
    gate.release();
    let deadline = Instant::now() + Duration::from_secs(10);
    while retired.iter().any(|owner| !owner.finished()) {
        assert!(Instant::now() < deadline, "retirement watchdog");
        std::thread::yield_now();
    }
}
