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

/// Only the copied fixture DLL is affected, including if an assertion panics.
struct StateFailure(unsafe extern "system" fn(u32));

#[test]
fn complete_save_retries_failed_drum_target_and_keeps_notice_until_publication() {
    for (changed_patch, supersede) in [(false, false), (true, false), (true, true)] {
        complete_save_retry_case(changed_patch, supersede);
    }
}

fn complete_save_retry_case(changed_patch: bool, supersede: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    xirius_test_vst3::fixture_marker();
    let source = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let root = tempfile::tempdir().unwrap();
    let dll = root.path().join("drum-save.vst3");
    std::fs::copy(source, &dll).unwrap();
    let catalog = Arc::new(Catalog {
        entries: xirius_plugins::probe(&dll).unwrap(),
        failures: vec![],
    });
    let mut s = Studio::new(Song::empty());
    let drum = s
        .song
        .run(
            "track.add.with-instrument",
            json!({"instrument":"Drum Kit"}),
        )
        .unwrap();
    let track = drum["id"].as_str().unwrap().parse().unwrap();
    let external = s.song.run("plugin.add", json!({"track":drum["id"],"name":"Fixture","format":"vst3","uid":catalog.entries[0].uid})).unwrap();
    let external_id = external["id"].as_str().unwrap().parse().unwrap();
    let dir = root.path().join("song");
    s.song.save_as(&dir).unwrap();
    s.transport.mode = OutputMode::Device;
    s.transport.bind_window();
    s.transport.set_catalog(catalog);
    s.transport
        .set_drivers(super::prepared::tests::quiet_drivers());
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(30);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(s.error.is_none(), "{:?}", s.error);
    // Seed the serialized patch: the later successful capture is unchanged.
    assert!(s.start_save());
    s.wait_for_save();
    assert!(!s.keep.plugin_state_failed);
    let fail = Arc::new(AtomicBool::new(true));
    let failing = fail.clone();
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
        if op == "prepare_get_state" && failing.load(Ordering::SeqCst) {
            return Err(std::io::Error::other("private fixture transfer failure"));
        }
        Ok(())
    }));
    let file = root.path().join("hit.wav");
    xirius_session::export::write_wav(&file, &vec![0.25; 12000], &vec![0.25; 12000], 48000, 24)
        .unwrap();
    s.song
        .run(
            "plugin.samples.add",
            json!({"id":drum["plugin"],"file":file,"pad":"A13"}),
        )
        .unwrap();
    s.transport.sync(&s.project(), s.song.dir());
    while s.transport.host().unwrap().swap_pending() {
        s.poll_swap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(s.keep.plugin_state_failed);
    assert!(
        s.transport
            .host()
            .unwrap()
            .instrument_status(track)
            .is_err()
    );
    let ticket = s.transport.host().unwrap().failed_graph_retry().unwrap();
    assert!(!s.transport.host().unwrap().graph_retry_applied(&ticket));
    fail.store(false, Ordering::SeqCst);
    if changed_patch {
        assert!(
            s.transport
                .host()
                .unwrap()
                .plugins()
                .set_live_param(external_id, "1", 0.25)
        );
    }
    assert!(s.start_save());
    s.wait_for_save();
    assert!(
        s.keep.plugin_state_failed,
        "disk success is not graph publication"
    );
    assert!(
        s.keep.graph_retry.is_some(),
        "complete explicit save schedules one retry"
    );
    let mut applied_ticket = s.keep.graph_retry.as_ref().unwrap().0.clone();
    assert!(
        !s.transport
            .host_mut()
            .unwrap()
            .retry_failed_graph(ticket.clone()),
        "old completion cannot start another attempt"
    );
    if supersede {
        let generation = s.keep.plugin_failure_generation;
        fail.store(true, Ordering::SeqCst);
        s.song
            .run("track.add.audio", json!({"name":"New target"}))
            .unwrap();
        s.after_edit();
        while s.transport.host().unwrap().swap_pending() {
            s.poll_swap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(s.keep.plugin_failure_generation > generation);
        assert!(
            s.keep.graph_retry.is_none(),
            "superseded acknowledgment is released"
        );
        assert!(s.keep.plugin_state_failed);
        assert!(
            !s.transport
                .host()
                .unwrap()
                .graph_retry_current(&applied_ticket)
        );
        assert!(
            !s.transport
                .host_mut()
                .unwrap()
                .retry_failed_graph(applied_ticket)
        );
        fail.store(false, Ordering::SeqCst);
        assert!(s.start_save());
        s.wait_for_save();
        applied_ticket = s
            .keep
            .graph_retry
            .as_ref()
            .expect("latest failure remains retryable")
            .0
            .clone();
        assert!(
            s.keep.plugin_state_failed,
            "newest target still awaits publication"
        );
    }
    while s.transport.host().unwrap().swap_pending() {
        s.poll_swap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    s.poll_swap();
    assert!(
        s.transport
            .host()
            .unwrap()
            .graph_retry_applied(&applied_ticket),
        "status={:?}, failed={}, generation={}, reason={:?}, error={:?}",
        s.transport.host().unwrap().instrument_status(track),
        s.transport.host().unwrap().failed_graph_retry().is_some(),
        s.keep.plugin_failure_generation,
        s.keep.failed,
        s.error
    );
    assert!(s.transport.host().unwrap().instrument_ready(track));
    assert!(!s.keep.plugin_state_failed);
    assert!(s.keep.failed.is_none());
    assert!(s.toast.is_none());
    if changed_patch {
        assert!(
            !s.transport.host().unwrap().graph_retry_applied(&ticket),
            "capture changed source, so the old ticket is invalid"
        );
        let project = s.project();
        let plugin = project
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == external_id)
            .unwrap();
        assert_eq!(
            xirius_plugins::decode_state(plugin.state.as_ref().unwrap())
                .unwrap()
                .component,
            64u32.to_le_bytes(),
            "collected external patch is saved"
        );
    }
    assert!(!s.transport.host_mut().unwrap().retry_failed_graph(ticket));
}
impl StateFailure {
    fn new(path: &std::path::Path) -> Self {
        let wide: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: the probed fixture remains loaded and this export takes one u32.
        let set = unsafe {
            std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(u32),
            >(
                GetProcAddress(
                    GetModuleHandleW(wide.as_ptr()),
                    c"FixtureStateFailure".as_ptr().cast(),
                )
                .unwrap(),
            )
        };
        let failure = Self(set);
        failure.set(true);
        failure
    }
    fn set(&self, fail: bool) {
        // SAFETY: this copied DLL is still retained by the loader.
        unsafe { (self.0)(u32::from(fail)) }
    }
}
impl Drop for StateFailure {
    fn drop(&mut self) {
        self.set(false);
    }
}

#[test]
fn rejected_sdk_state_keeps_a_safe_reason_through_edits_disk_failures_and_mcp_retry() {
    use xirius_core::error::PluginStateFailure;
    use xirius_core::session::RunOptions;

    xirius_test_vst3::fixture_marker();
    let source = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let root = tempfile::tempdir().unwrap();
    let dll = root.path().join("rejected-state.vst3");
    std::fs::copy(source, &dll).unwrap();
    let catalog = Arc::new(Catalog {
        entries: xirius_plugins::probe(&dll).unwrap(),
        failures: Vec::new(),
    });
    let mut s = Studio::new(Song::empty());
    let track = s
        .song
        .run("track.add.audio", json!({"name":"Fixture"}))
        .unwrap()["id"]
        .clone();
    s.song
        .run(
            "plugin.add",
            json!({
                "track":track,"name":"Fixture","format":"vst3","uid":catalog.entries[0].uid,
            }),
        )
        .unwrap();
    let dir = root.path().join("private-fixture-song");
    s.song.save_as(&dir).unwrap();
    s.transport.mode = OutputMode::Device;
    s.transport.bind_window();
    s.transport.set_catalog(catalog);
    s.transport
        .set_drivers(super::prepared::tests::quiet_drivers());
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "fixture preparation watchdog");
        std::thread::yield_now();
    }
    assert!(s.error.is_none(), "{:?}", s.error);
    let failure = StateFailure::new(&dll);
    let before = std::fs::read(dir.join("project.json")).unwrap();
    let reason = "プラグインの状態を取得できません";
    let assert_reason = |s: &Studio, reason: &str| {
        assert!(s.keep.plugin_state_failed);
        assert_eq!(s.save_mark(), SaveMark::Failed);
        for text in [
            s.keep.failed.as_deref().unwrap(),
            &s.toast.as_ref().unwrap().text,
        ] {
            assert!(text.contains(reason), "{text}");
            assert!(!text.contains("private-fixture-song"), "{text}");
        }
    };
    assert!(s.start_save());
    s.wait_for_save();
    assert_reason(&s, reason);
    assert!(s.error.as_deref().unwrap().contains(reason));
    let since = s.toast.as_ref().unwrap().since;
    let g = s.geometry(1440.0, 820.0);
    s.press(&g, crate::studio::Hit::None);
    assert!(s.error.is_none());
    assert_reason(&s, reason);
    assert_eq!(s.toast.as_ref().unwrap().since, since);
    let call = crate::review_io_tests::request_with_save(
        &mut s,
        "project.save",
        json!({}),
        RunOptions::human(),
        true,
    );
    s.wait_for_save();
    let error = call.join().unwrap().unwrap_err();
    assert!(error.message.contains(reason));
    assert!(!error.message.contains("private-fixture-song"));
    assert_reason(&s, reason);
    assert_eq!(s.toast.as_ref().unwrap().since, since);
    assert_eq!(std::fs::read(dir.join("project.json")).unwrap(), before);

    // A real history writer failure must add its disk reason without dropping
    // the unresolved SDK failure, even after the single-line error was cleared.
    let watch = dir.join("history");
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "write" && path.starts_with(&watch) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "private-fixture-song: injected writer failure",
            ));
        }
        Ok(())
    }));
    s.song
        .run("track.add.audio", json!({"name":"Written asynchronously"}))
        .unwrap();
    s.song.flush_writes();
    let now = xirius_core::store::now_ms();
    s.keep_beat(now);
    assert_reason(&s, reason);
    assert_eq!(s.toast.as_ref().unwrap().since, since);
    assert!(
        s.keep
            .failed
            .as_deref()
            .unwrap()
            .contains("このフォルダーには書き込めない")
    );
    drop(observe);
    let autosaves = s.keep.autosaves;
    s.keep_beat(now + 31_000);
    assert_eq!(
        s.keep.autosaves,
        autosaves + 1,
        "an actual automatic checkpoint ran"
    );
    s.song.flush_writes();
    assert!(
        s.song.session().take_write_failures().is_empty(),
        "automatic checkpoint succeeded"
    );
    s.keep_beat(now + 31_001);
    assert_reason(&s, reason);

    // An ordinary edit with MCP persistence captures and writes successfully,
    // but it is not the explicit save retry that acknowledges a plugin failure.
    failure.set(false);
    let call = crate::review_io_tests::request_with_save(
        &mut s,
        "track.add.audio",
        json!({"name":"Persisted edit"}),
        RunOptions::human(),
        true,
    );
    s.wait_for_save();
    call.join().unwrap().unwrap();
    assert_reason(&s, reason);

    // Hold the completed capture's file worker and fail a real graph transfer
    // through the fixture SDK. The older save cannot
    // acknowledge it; only the following complete explicit save may do so.
    let release = crate::browser_io_tests::Release::new();
    let gate = release.0.clone();
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signalled = entered.clone();
    // Session::save writes this checkpoint on the save worker after writer.flush;
    // a history/recent-list write cannot open this gate.
    let watch = dir.join("project.json");
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "write"
            && path == watch
            && !signalled.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |done| !*done).unwrap());
        }
        Ok(())
    }));
    let call = crate::review_io_tests::request_with_save(
        &mut s,
        "project.save",
        json!({}),
        RunOptions::human(),
        true,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !entered.load(std::sync::atomic::Ordering::SeqCst) {
        s.poll_saving();
        assert!(Instant::now() < deadline, "file worker gate watchdog");
        std::thread::yield_now();
    }
    assert!(
        s.saving.is_some(),
        "the held disk write has not completed on UI"
    );
    failure.set(true);
    let mut changed = (*s.project()).clone();
    // A test-only host target makes the existing effect require a rebuild;
    // the on-disk save still owns its unchanged source snapshot.
    let row = changed
        .tracks
        .values_mut()
        .find(|t| !t.inserts.is_empty())
        .unwrap();
    Arc::make_mut(row).inserts[0].name = "Transfer target".into();
    s.transport.host_mut().unwrap().sync_later(&changed);
    while s.transport.host().unwrap().swap_pending() {
        s.poll_swap();
        assert!(Instant::now() < deadline, "transfer failure watchdog");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(s.transport.host().unwrap().has_failed_graph());
    assert_eq!(
        s.keep.plugin_state_reason,
        Some(PluginStateFailure::Transfer)
    );
    failure.set(false);
    drop(release);
    s.wait_for_save();
    call.join().unwrap().unwrap();
    assert_reason(&s, "プラグインの状態を引き継げません");
    drop(observe);
    // Returning the saved model can retarget the graph. Its recovery alone
    // must not acknowledge the newer failed save-state notification.
    while s.transport.host().unwrap().swap_pending() {
        s.poll_swap();
        assert!(Instant::now() < deadline, "saved target recovery watchdog");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_reason(&s, "プラグインの状態を引き継げません");
    let call = crate::review_io_tests::request_with_save(
        &mut s,
        "project.save",
        json!({}),
        RunOptions::human(),
        true,
    );
    s.wait_for_save();
    call.join().unwrap().unwrap();
    assert!(!s.keep.plugin_state_failed);
    assert!(s.keep.plugin_state_reason.is_none());
    assert!(s.keep.failed.is_none());
    assert!(s.toast.is_none());
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
    let source = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let temp = tempfile::tempdir().unwrap();
    let dll = temp.path().join("held-state.vst3");
    std::fs::copy(source, &dll).unwrap();
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
    let timeout_reason = "プラグインの状態取得が時間内に完了しません";
    assert!(s.error.as_deref().unwrap().contains(timeout_reason));
    assert!(s.keep.failed.as_deref().unwrap().contains(timeout_reason));
    assert!(s.toast.as_ref().unwrap().text.contains(timeout_reason));
    let g = s.geometry(1440.0, 820.0);
    let since = s.toast.as_ref().unwrap().since;
    s.press(&g, crate::studio::Hit::None);
    assert!(s.error.is_none());
    assert!(s.keep.failed.as_deref().unwrap().contains(timeout_reason));
    assert!(s.toast.as_ref().unwrap().text.contains(timeout_reason));
    assert_eq!(s.toast.as_ref().unwrap().since, since);
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
        assert!(s.keep.failed.as_deref().unwrap().contains(timeout_reason));
        assert!(s.toast.as_ref().unwrap().text.contains(timeout_reason));
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
    s.save_test_failure = true;
    assert!(s.start_save());
    s.wait_for_save();
    assert!(
        s.keep.plugin_state_failed,
        "capture success is not a complete retry"
    );
    assert!(s.keep.failed.as_deref().unwrap().contains(timeout_reason));
    assert!(s.toast.as_ref().unwrap().text.contains(timeout_reason));
    s.save_test_failure = false;
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
