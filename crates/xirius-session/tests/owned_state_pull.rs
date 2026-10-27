//! The real loader/host path while the local VST3 SDK getState call is held.
//! No editor, device, window or OS input is used.
#![cfg(windows)]

use std::os::windows::ffi::OsStrExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_plugins::{Catalog, decode_state, probe};
use xirius_session::commands::{self, shared_registry};
use xirius_session::host::{
    HostConfig, SessionHost, collect_plugin_states, collect_pulled_states, pull_plugin_states,
};
use xirius_session::plugins::Isolation;

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
        // SAFETY: the loader retains this DLL; the fixture exports this exact ABI.
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
        // SAFETY: live event, queried without a wait.
        unsafe { WaitForSingleObject(self.entered, 0) == WAIT_OBJECT_0 }
    }
    fn release(&self) {
        // SAFETY: live manual reset event.
        assert_ne!(unsafe { SetEvent(self.release) }, 0);
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        // SAFETY: a successful test waits for all calls before disarming. If an
        // assertion unwinds, release the SDK but retain handles until process exit.
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
fn local_state_collection_and_changes_return_while_sdk_is_held_and_stale_state_is_rejected() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let catalog = Arc::new(Catalog {
        entries: probe(&dll).unwrap(),
        failures: Vec::new(),
    });
    let mut song = Session::with_registry(Project::new("owner-state"), shared_registry());
    let opts = RunOptions::human();
    let track = commands::run(
        &mut song,
        None,
        "track.add.audio",
        json!({"name":"A"}).as_object().unwrap(),
        &opts,
    )
    .unwrap()
    .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    commands::run(
        &mut song,
        None,
        "plugin.add",
        json!({"track":track,"name":"Fixture","format":"vst3","uid":catalog.entries[0].uid})
            .as_object()
            .unwrap(),
        &opts,
    )
    .unwrap();
    let tid = *song
        .project()
        .tracks
        .iter()
        .find(|(_, t)| !t.inserts.is_empty())
        .unwrap()
        .0;
    let id = song.project().tracks[&tid].inserts[0].id;
    consecutive_changes_keep_unexecuted_deltas(song.project(), tid, &catalog, &dll);
    let (mut host, mut audio, warnings) = SessionHost::start(
        song.project(),
        HostConfig {
            threaded_local: true,
            catalog: Some(catalog),
            isolation: Some(Isolation::InProcess),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        host.plugins().local(id).is_none(),
        "the UI cannot borrow an SDK Plugin"
    );
    assert!(host.plugins().opened(id));
    let gate = Gate::new(&dll);
    let old = host.begin_pull_states();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.entered() {
        assert!(!old.ready());
        assert!(Instant::now() < deadline, "SDK gate watchdog");
        std::thread::yield_now();
    }
    assert!(!old.ready());
    let abandoned = host.begin_pull_states();
    abandoned.cancel();
    assert!(abandoned.ready());
    assert!(abandoned.error().is_some());
    assert!(
        collect_plugin_states(&mut song, &mut host, &opts)
            .unwrap()
            .result
            .is_none()
    );
    assert!(
        pull_plugin_states(&mut song, &mut host, &opts, Duration::ZERO).is_err(),
        "an incomplete capture must not report save success"
    );

    let mut changed = song.project().clone();
    Arc::make_mut(changed.tracks.get_mut(&tid).unwrap()).inserts[0]
        .params
        .insert("1".into(), 0.25);
    let synced = host.sync(&changed);
    assert!(!synced.swapped);
    assert!(synced.warnings.is_empty(), "{:?}", synced.warnings);
    assert!(
        !old.ready(),
        "independent receipts remain held by the SDK gate"
    );
    let current = host.begin_pull_states();
    assert!(!current.ready());
    gate.release();
    let mut buffer = [0.0f32; 512];
    let deadline = Instant::now() + Duration::from_secs(10);
    while !current.ready() {
        audio.render(&mut buffer, 2);
        host.poll();
        assert!(Instant::now() < deadline, "new capture watchdog");
        std::thread::yield_now();
    }
    assert_eq!(current.error(), None);
    collect_pulled_states(&mut song, &mut host, &current, &opts).unwrap();
    assert!(
        collect_pulled_states(&mut song, &mut host, &old, &opts)
            .unwrap()
            .result
            .is_none()
    );
    let state = decode_state(
        song.project().tracks[&tid].inserts[0]
            .state
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state.component,
        64u32.to_le_bytes(),
        "the held old reply must never replace the newer state"
    );
    assert_eq!(state.controller, state.component);

    // A change that has not reached the song must survive a different knob's
    // edit. Reapplying the old snapshot or every cached parameter loses it.
    assert!(host.plugins().set_live_param(id, "1", 0.5));
    audio.render(&mut buffer, 2);
    let mut another = song.project().clone();
    Arc::make_mut(another.tracks.get_mut(&tid).unwrap()).inserts[0]
        .params
        .insert(xirius_test_vst3::CRASH_PARAMETER.to_string(), 0.0);
    let report = host.sync(&another);
    assert!(!report.swapped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let pull = host.begin_pull_states();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pull.ready() {
        audio.render(&mut buffer, 2);
        host.poll();
        assert!(Instant::now() < deadline, "delta capture watchdog");
        std::thread::yield_now();
    }
    assert_eq!(pull.error(), None);
    collect_pulled_states(&mut song, &mut host, &pull, &opts).unwrap();
    let state = decode_state(
        song.project().tracks[&tid].inserts[0]
            .state
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state.component,
        128u32.to_le_bytes(),
        "unchanged state and parameters must not be resent"
    );
    drop(gate);
    let gate = Gate::new(&dll);
    host.set_nonblocking(true);
    let mut expanded = song.project().clone();
    let added = xirius_core::id::TrackId(9_000_000);
    expanded.root.push(xirius_core::id::Node::Track(added));
    expanded.tracks.insert(
        added,
        Arc::new(xirius_core::model::Track::new(
            added,
            xirius_core::model::TrackKind::Audio,
            "new track",
        )),
    );
    assert!(!host.sync_later(&expanded).swapped);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.entered() {
        assert!(host.step_swap().is_none());
        assert!(Instant::now() < deadline, "preload snapshot gate watchdog");
        std::thread::yield_now();
    }
    // Every frame returns even while the previous instance's SDK is held.
    for _ in 0..8 {
        assert!(host.step_swap().is_none());
        assert!(host.swap_pending());
    }
    assert_eq!(host.plugins().owner_retirements().len(), 1);
    gate.release();
    let deadline = Instant::now() + Duration::from_secs(10);
    let report = loop {
        if let Some(report) = host.step_swap() {
            break report;
        }
        audio.render(&mut buffer, 2);
        host.poll();
        assert!(Instant::now() < deadline, "preload completion watchdog");
        std::thread::yield_now();
    };
    assert!(report.swapped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    drop(gate);
    // Successful getState may be collected while a different plugin's init is
    // held. It must not cancel the pending graph and enter synchronous build.
    assert!(host.plugins().set_live_param(id, "1", 0.75));
    audio.render(&mut buffer, 2);
    let captured = host.begin_pull_states();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !captured.ready() {
        assert!(Instant::now() < deadline, "capture watchdog");
        std::thread::yield_now();
    }
    assert_eq!(captured.error(), None);
    let added = commands::run(
        &mut song,
        None,
        "track.add.audio",
        json!({"name":"pending"}).as_object().unwrap(),
        &opts,
    )
    .unwrap()
    .data["id"]
        .clone();
    let uid = song.project().tracks[&tid].inserts[0].uid.clone();
    commands::run(
        &mut song,
        None,
        "plugin.add",
        json!({"track":added,"name":"Fixture","format":"vst3","uid":uid})
            .as_object()
            .unwrap(),
        &opts,
    )
    .unwrap();
    let gate = Gate::named(&dll, c"FixtureOpenGate");
    assert!(!host.sync_later(song.project()).swapped);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.entered() {
        let _ = host.step_swap();
        assert!(Instant::now() < deadline, "init watchdog");
        std::thread::yield_now();
    }
    assert!(
        collect_pulled_states(&mut song, &mut host, &captured, &opts)
            .unwrap()
            .result
            .is_some()
    );
    assert!(
        host.swap_pending(),
        "collection must keep asynchronous reconstruction"
    );
    // In the actual window facade even finishing preloads may not construct a
    // timeline/plan: the preparation worker consumes the requested project.
    host.set_windowed();
    assert!(host.preparation_target().is_some());
    assert!(host.step_swap().is_none());
    gate.release();
    assert!(host.plugins().set_live_param(id, "1", 0.625));
    audio.render(&mut buffer, 2);
    drop(audio);
    let mut starting = host
        .prepare_again(song.project(), Default::default())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !starting.step() {
        assert!(Instant::now() < deadline, "retained loader watchdog");
        std::thread::yield_now();
    }
    let (mut host, _audio, failures) = starting.finish_prepared(song.project(), |_, _| {}).unwrap();
    assert!(failures.is_empty());
    let captured = host.begin_pull_states();
    while !captured.ready() {
        assert!(Instant::now() < deadline, "retained state watchdog");
        std::thread::yield_now();
    }
    collect_pulled_states(&mut song, &mut host, &captured, &opts).unwrap();
    let state = decode_state(
        song.project().tracks[&tid].inserts[0]
            .state
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        state.component,
        160u32.to_le_bytes(),
        "background reconstruction preserves unreported live patches"
    );
}

fn consecutive_changes_keep_unexecuted_deltas(
    project: &Project,
    tid: xirius_core::id::TrackId,
    catalog: &Arc<Catalog>,
    dll: &std::path::Path,
) {
    for state_first in [false, true] {
        let (mut host, _audio, warnings) = SessionHost::start(
            project,
            HostConfig {
                threaded_local: true,
                catalog: Some(catalog.clone()),
                isolation: Some(Isolation::InProcess),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(warnings.is_empty());
        let gate = Gate::new(dll);
        let held = host.begin_pull_states();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !gate.entered() {
            assert!(!held.ready());
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let mut desired = project.clone();
        let plugin = &mut Arc::make_mut(desired.tracks.get_mut(&tid).unwrap()).inserts[0];
        if state_first {
            plugin.state = Some(xirius_plugins::encode_state(&xirius_vst3::PluginState {
                component: 192u32.to_le_bytes().to_vec(),
                controller: 192u32.to_le_bytes().to_vec(),
            }));
        } else {
            plugin.params.insert("1".into(), 0.25);
        }
        let first = host.sync(&desired);
        assert!(!first.swapped && first.warnings.is_empty());
        Arc::make_mut(desired.tracks.get_mut(&tid).unwrap()).inserts[0]
            .params
            .insert(xirius_test_vst3::CRASH_PARAMETER.to_string(), 0.0);
        let second = host.sync(&desired);
        assert!(!second.swapped && second.warnings.is_empty());
        let current = host.begin_pull_states();
        assert!(!current.ready());
        gate.release();
        while !current.ready() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(current.error(), None);
        let mut collected = Session::with_registry(desired, shared_registry());
        collect_pulled_states(&mut collected, &mut host, &current, &RunOptions::human()).unwrap();
        let plugin = &collected.project().tracks[&tid].inserts[0];
        let expected = if state_first { 192u32 } else { 64 };
        assert_eq!(
            decode_state(plugin.state.as_deref().unwrap())
                .unwrap()
                .component,
            expected.to_le_bytes(),
            "the first queued state/value must survive the later different-key change"
        );
        if !state_first {
            assert_eq!(plugin.params["1"], f64::from(expected) / 256.0);
        }
        assert_eq!(
            plugin.params[&xirius_test_vst3::CRASH_PARAMETER.to_string()],
            0.0
        );
    }
}
