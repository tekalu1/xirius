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
        let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: the loader retains this fixture; its exported function has this ABI.
        unsafe {
            let module = GetModuleHandleW(wide.as_ptr());
            let arm = std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(usize, usize),
            >(
                GetProcAddress(module, c"FixtureStateGate".as_ptr().cast()).unwrap()
            );
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
    let (host, audio, warnings) = SessionHost::start(
        &s.project(),
        HostConfig {
            threaded_local: true,
            catalog: Some(catalog),
            dir: Some(dir.clone()),
            isolation: Some(xirius_session::plugins::Isolation::InProcess),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    s.transport.player = Some(Player {
        host,
        out: Output::start(OutputMode::Silent { paced: true }, audio)
            .unwrap()
            .unwrap(),
        dir: Some(dir),
    });
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
    gate.release();
    let deadline = Instant::now() + Duration::from_secs(10);
    while retired.iter().any(|owner| !owner.finished()) {
        assert!(Instant::now() < deadline, "retirement watchdog");
        std::thread::yield_now();
    }
}
