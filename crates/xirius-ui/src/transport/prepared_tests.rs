use super::*;
use crate::{browser_io_tests::Release, song::Song, studio::Studio};
use std::sync::mpsc;
use xirius_session::output::{DeviceOutput, SharedStream};

#[test]
fn a_window_timeline_update_uses_background_preparation_and_idle_remains_responsive() {
    let root = tempfile::tempdir().unwrap();
    let wav = root.path().join("source.wav");
    xirius_session::export::write_wav(&wav, &[0.0; 2048], &[0.0; 2048], 22_050, 16).unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    let track = s
        .song
        .run("track.add.audio", serde_json::json!({"name":"A"}))
        .unwrap()["id"]
        .clone();
    s.transport.mode = OutputMode::Silent { paced: true };
    s.transport.bind_window();
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "initial preparation watchdog");
        std::thread::yield_now();
    }
    s.song
        .run(
            "clip.import",
            serde_json::json!({"file":wav, "track":track}),
        )
        .unwrap();
    let release = Release::new();
    let gate = release.0.clone();
    let prefix = root.path().to_path_buf();
    let (tx, entered) = mpsc::channel();
    let once = AtomicBool::new(false);
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "prepare_decode" && path.starts_with(&prefix) && !once.swap(true, Ordering::SeqCst)
        {
            tx.send(std::thread::current().id()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |done| !*done).unwrap());
        }
        Ok(())
    }));
    let project = s.project();
    s.transport.sync(&project, s.song.dir());
    assert!(s.transport.host().unwrap().preparation_target().is_some());
    s.transport.recording = true;
    assert!(s.poll_swap());
    assert!(s.transport.host().is_some(), "a recording keeps its player");
    assert!(s.transport.recording);
    assert!(!s.transport.preparing());
    assert!(
        entered.try_recv().is_err(),
        "no decode before recording stops"
    );
    s.transport.recording = false;
    assert!(s.poll_swap());
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(20)).unwrap(),
        std::thread::current().id()
    );
    assert!(s.idle().changed);
    assert!(
        s.tick(),
        "frames only poll preparation while material I/O is held"
    );
    let before = s.preparing.as_ref().unwrap().progress.clone();
    assert!(s.prepare_key("escape"));
    assert_eq!(s.preparing.as_ref().unwrap().progress, before);
    assert!(s.idle().changed);
    assert!(s.preparing.as_ref().unwrap().cancelling);
    release.release();
}

struct HeldOutput {
    audio: Option<xirius_engine::AudioEngine>,
    entered: mpsc::Sender<std::thread::ThreadId>,
    release: Arc<(Mutex<bool>, std::sync::Condvar)>,
}

#[test]
fn cancellation_wins_over_an_already_completed_unclaimed_player_and_retires_it_off_ui() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    s.transport.mode = OutputMode::Device;
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let mut drivers = quiet_drivers();
    drivers.shared_out = Arc::new(move |audio, _, _| {
        let entered = tx.clone();
        let release = gate.clone();
        DeviceOutput::open_with(audio, move |audio| {
            Ok(Box::new(HeldOutput {
                audio: Some(audio),
                entered,
                release,
            }))
        })
        .map(Output::Device)
    });
    s.transport.set_drivers(drivers);
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    // Do not poll/adopt the prepared job. This establishes the exact race,
    // independently of how fast output construction was.
    while !s.transport.prepared_start.as_ref().unwrap().job.ready() {
        assert!(Instant::now() < deadline, "completed candidate watchdog");
        std::thread::yield_now();
    }
    assert!(s.prepare_key("escape"));
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(20)).unwrap(),
        std::thread::current().id()
    );
    assert!(s.poll_preparing());
    assert!(s.preparing.as_ref().unwrap().cancelling);
    assert!(s.transport.host().is_none());
    assert!(!s.transport.playing);
    release.release();
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(
            Instant::now() < deadline,
            "completed candidate retirement watchdog"
        );
        std::thread::yield_now();
    }
    assert!(s.transport.host().is_none());
    assert!(s.song.dir().is_none());
}
impl SharedStream for HeldOutput {
    fn name(&self) -> String {
        "held test output".into()
    }
    fn lost(&self) -> xirius_engine::device::Lost {
        Default::default()
    }
    fn stop(mut self: Box<Self>) -> Option<xirius_engine::AudioEngine> {
        self.entered.send(std::thread::current().id()).unwrap();
        let held = self.release.0.lock().unwrap();
        drop(self.release.1.wait_while(held, |done| !*done).unwrap());
        self.audio.take()
    }
}

#[test]
fn zero_tracks_and_retiring_output_never_hold_the_ui_or_reopen_before_stop_completes() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    s.transport.mode = OutputMode::Device;
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let opens = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = opens.clone();
    let mut drivers = quiet_drivers();
    drivers.shared_out = Arc::new(move |audio, _, _| {
        count.fetch_add(1, Ordering::SeqCst);
        let tx = tx.clone();
        let gate = gate.clone();
        DeviceOutput::open_with(audio, move |audio| {
            Ok(Box::new(HeldOutput {
                audio: Some(audio),
                entered: tx,
                release: gate,
            }))
        })
        .map(Output::Device)
    });
    s.transport.set_drivers(drivers);
    s.prepare_song();
    assert_eq!(s.preparing.as_ref().unwrap().progress.units.len(), 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "initial output watchdog");
        std::thread::yield_now();
    }
    assert!(s.transport.host().is_some());
    assert!(!s.transport.playing, "preparation never starts playback");
    s.transport.reset();
    let stopped_on = entered.recv_timeout(Duration::from_secs(20)).unwrap();
    assert_ne!(stopped_on, std::thread::current().id());
    assert!(s.transport.host().is_none());
    s.prepare_song();
    for size in [(1280.0, 800.0), (900.0, 600.0)] {
        let g = s.geometry(size.0, size.1);
        assert!(s.poll_preparing());
        assert_eq!(s.preparing.as_ref().unwrap().layout(g.size).rect.w, 300.0);
        assert_eq!(opens.load(Ordering::SeqCst), 1);
    }
    s.prepare_key("escape");
    assert!(s.preparing.as_ref().unwrap().cancelling);
    assert!(s.poll_preparing());
    assert!(
        s.preparing.is_some(),
        "old output still belongs to its stop worker"
    );
    release.release();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(Instant::now() < deadline, "stop cleanup watchdog");
        std::thread::yield_now();
    }
    assert!(s.song.dir().is_none());
    assert!(s.transport.host().is_none());
    assert_eq!(
        opens.load(Ordering::SeqCst),
        1,
        "cancelled output creation is never adopted or reopened"
    );
}

pub(crate) fn quiet_drivers() -> Drivers {
    Drivers {
        exclusive_out: None,
        shared_out: Arc::new(|audio, _, _| {
            Ok(Output::Silent(xirius_session::output::SilentOutput::start(
                audio, true,
            )))
        }),
        exclusive_in: None,
        shared_in: Arc::new(|_, _, _| Err("no test input".into())),
        devices: Arc::new(Vec::new),
    }
}

fn missing_plugin(s: &mut Studio, track: serde_json::Value) {
    s.song.run("plugin.add", serde_json::json!({
        "track":track, "name":"Missing", "format":"vst3", "uid":"00000000000000000000000000000000"
    })).unwrap();
}

#[test]
fn declared_main_thread_requirement_is_an_explicit_local_failure_without_changing_the_song() {
    let mut s = Studio::new(Song::empty());
    let track = s
        .song
        .run(
            "track.add.audio",
            serde_json::json!({"name":"Main thread only"}),
        )
        .unwrap()["id"]
        .clone();
    missing_plugin(&mut s, track);
    let mut entry: xirius_plugins::Entry = serde_json::from_value(serde_json::json!({
        "uid":"00000000000000000000000000000000", "name":"Main only", "vendor":"Fixture",
        "kind":"effect", "format":"vst3", "category":"Fx", "version":"1", "path":"unused.vst3"
    }))
    .unwrap();
    assert!(
        !entry.requires_process_main,
        "older catalogues remain readable"
    );
    entry.requires_process_main = true;
    let entry: xirius_plugins::Entry =
        serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
    assert!(entry.requires_process_main);
    s.transport.set_catalog(Arc::new(Catalog {
        entries: vec![entry],
        failures: Vec::new(),
    }));
    s.transport.mode = OutputMode::Device;
    s.transport.set_drivers(quiet_drivers());
    let before = s.project();
    s.prepare_song();
    wait_failed(&mut s, 1);
    let reason = &s.preparing.as_ref().unwrap().failures[0].reason;
    assert!(reason.contains("主スレッド"), "{reason}");
    assert!(reason.contains("別プロセス"), "{reason}");
    assert!(Arc::ptr_eq(&before, &s.project()));
    assert!(
        before
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .all(|p| !p.isolation.is_separate())
    );
    assert!(s.transport.host().is_none());
}

fn wait_failed(s: &mut Studio, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        s.poll_preparing();
        let card = s.preparing.as_ref().expect("failure keeps the veil");
        assert!(card.failure.is_none(), "{:?}", card.failure);
        if card.failures.len() == count {
            return;
        }
        assert!(Instant::now() < deadline, "failure watchdog");
        std::thread::yield_now();
    }
}

#[test]
fn a_window_on_demand_silent_start_blocks_before_its_first_frame_without_decoding_on_ui() {
    let root = tempfile::tempdir().unwrap();
    let wav = root.path().join("source.wav");
    xirius_session::export::write_wav(&wav, &[0.0; 2048], &[0.0; 2048], 22_050, 16).unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    s.song
        .run("clip.import", serde_json::json!({"file":wav}))
        .unwrap();
    s.transport.mode = OutputMode::Silent { paced: true };
    s.transport.bind_window();
    let release = Release::new();
    let gate = release.0.clone();
    let prefix = root.path().to_path_buf();
    let (tx, entered) = mpsc::channel();
    let once = AtomicBool::new(false);
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "prepare_decode" && path.starts_with(&prefix) && !once.swap(true, Ordering::SeqCst)
        {
            tx.send(std::thread::current().id()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |done| !*done).unwrap());
        }
        Ok(())
    }));
    let project = s.project();
    assert!(s.transport.start_host(&project, s.song.dir()).is_err());
    assert!(
        s.preparing.is_none(),
        "the first frame has not created the card yet"
    );
    assert!(s.input_blocked());
    assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
    assert!(!s.start_save());
    assert!(!s.toggle_play());
    assert!(!s.toggle_record());
    assert!(!s.request_open(crate::opening::Request::New));
    assert!(Arc::ptr_eq(&project, &s.project()));
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(20)).unwrap(),
        std::thread::current().id()
    );
    assert!(
        s.prepare_key("escape"),
        "Esc is accepted even before the first painted frame"
    );
    assert!(s.preparing.as_ref().unwrap().cancelling);
    release.release();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(Instant::now() < deadline, "silent start cleanup watchdog");
        std::thread::yield_now();
    }
    assert!(s.song.dir().is_none());
    assert!(s.transport.host().is_none());
}

#[test]
fn retry_discards_a_material_failure_that_returns_after_the_new_player_is_ready() {
    let root = tempfile::tempdir().unwrap();
    let wav = root.path().join("source.wav");
    xirius_session::export::write_wav(&wav, &[0.0; 2048], &[0.0; 2048], 48_000, 16).unwrap();
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    s.song
        .run("clip.import", serde_json::json!({"file":wav}))
        .unwrap();
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let (done, retired) = mpsc::channel();
    let once = AtomicBool::new(false);
    let prefix = root.path().to_path_buf();
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "prepare_decode" && path.starts_with(&prefix) && !once.swap(true, Ordering::SeqCst)
        {
            tx.send(()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |done| !*done).unwrap());
            done.send(()).unwrap();
            return Err(std::io::Error::other("old material failure"));
        }
        Ok(())
    }));
    s.transport.mode = OutputMode::Device;
    s.transport.set_drivers(quiet_drivers());
    s.prepare_song();
    entered.recv_timeout(Duration::from_secs(20)).unwrap();
    // Exercise the transport boundary directly while an obsolete worker is held.
    assert!(s.transport.retry_preparation(&s.project(), s.song.dir()));
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(
            Instant::now() < deadline,
            "replacement preparation watchdog"
        );
        std::thread::yield_now();
    }
    assert!(s.transport.host().unwrap().unavailable_tracks().is_empty());
    release.release();
    retired.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(!s.poll_preparing());
    assert!(s.error.is_none());
    assert!(s.transport.host().unwrap().unavailable_tracks().is_empty());
}

#[test]
fn retry_reuses_healthy_sdk_and_materials_while_only_the_failed_decode_waits() {
    retained_retry(false);
}

#[test]
fn cancelling_a_retained_retry_discards_its_late_decode_and_closes_the_song() {
    retained_retry(true);
}

fn retained_retry(cancel: bool) {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::AtomicUsize;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    let root = tempfile::tempdir().unwrap();
    let dll = root.path().join("retry.vst3");
    std::fs::copy(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("xirius_test_vst3.dll"),
        &dll,
    )
    .unwrap();
    let catalog = Arc::new(Catalog {
        entries: xirius_plugins::probe(&dll).unwrap(),
        failures: vec![],
    });
    let wide: Vec<_> = dll.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the fixture module remains loaded, and this export has this ABI.
    let creations = unsafe {
        std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn() -> u32,
        >(
            GetProcAddress(
                GetModuleHandleW(wide.as_ptr()),
                c"FixtureCreations".as_ptr().cast(),
            )
            .unwrap(),
        )
    };
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.path().join("song")).unwrap();
    let mut ids = Vec::new();
    let mut files = Vec::new();
    for name in ["healthy", "retry"] {
        let wav = root.path().join(format!("{name}.wav"));
        xirius_session::export::write_wav(&wav, &[0.25; 4096], &[0.25; 4096], 22_050, 16).unwrap();
        s.song
            .run("clip.import", serde_json::json!({"file":wav}))
            .unwrap();
        let p = s.project();
        let t = p
            .tracks
            .values()
            .rfind(|t| t.kind == TrackKind::Audio)
            .unwrap();
        ids.push(t.id);
        let xirius_core::model::ClipBody::Audio { file, .. } = &t.clips[0].body else {
            panic!()
        };
        files.push(s.song.dir().unwrap().join(file));
    }
    s.song
        .run(
            "plugin.add",
            serde_json::json!({"track":ids[0].to_string(), "name":"Healthy",
        "format":"vst3", "uid":catalog.entries[0].uid}),
        )
        .unwrap();
    let allow = Arc::new(AtomicBool::new(false));
    let allowed = allow.clone();
    let healthy_reads = Arc::new(AtomicUsize::new(0));
    let reads = healthy_reads.clone();
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op != "prepare_decode" {
            return Ok(());
        }
        if path == files[0] {
            reads.fetch_add(1, Ordering::SeqCst);
        }
        if path == files[1] {
            if !allowed.load(Ordering::Acquire) {
                return Err(std::io::Error::other("temporarily unavailable"));
            }
            tx.send(std::thread::current().id()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |done| !*done).unwrap());
        }
        Ok(())
    }));
    s.transport.mode = OutputMode::Device;
    s.transport.set_catalog(catalog);
    s.transport.set_drivers(quiet_drivers());
    s.prepare_song();
    wait_failed(&mut s, 1);
    assert_eq!(
        s.preparing.as_ref().unwrap().failures[0].track,
        Some(ids[1])
    );
    assert_eq!(healthy_reads.load(Ordering::SeqCst), 1);
    // SAFETY: this read-only fixture function is valid while its module is kept.
    let made = unsafe { creations() };
    assert!(made > 0);
    s.prepare_retry();
    wait_failed(&mut s, 1);
    // SAFETY: the candidate is still retained by its worker.
    assert_eq!(unsafe { creations() }, made);
    allow.store(true, Ordering::Release);
    s.prepare_retry();
    assert_ne!(
        entered.recv_timeout(Duration::from_secs(20)).unwrap(),
        std::thread::current().id()
    );
    for size in [(1280.0, 800.0), (960.0, 600.0)] {
        s.poll_preparing();
        let card = s.preparing.as_ref().unwrap();
        assert!(card.failures.is_empty());
        assert_eq!(card.progress.done, 1);
        assert_eq!(card.layout(size).rect.w, 300.0);
        assert!(!s.toggle_play());
    }
    // SAFETY: see above. Retrying must not create another healthy component/controller.
    assert_eq!(unsafe { creations() }, made);
    if cancel {
        assert!(s.prepare_key("escape"));
        s.poll_preparing();
        assert!(s.preparing.as_ref().unwrap().cancelling);
        assert!(s.transport.host().is_none());
    }
    release.release();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() || s.opening.is_some() {
        s.poll_preparing();
        s.poll_opening();
        assert!(Instant::now() < deadline, "retained candidate watchdog");
        std::thread::yield_now();
    }
    assert_eq!(healthy_reads.load(Ordering::SeqCst), 1);
    if cancel {
        assert!(s.transport.host().is_none());
        assert!(s.song.dir().is_none());
        return;
    }
    assert!(s.transport.host().unwrap().unavailable_tracks().is_empty());
    // SAFETY: the adopted host still owns this private fixture.
    assert_eq!(unsafe { creations() }, made);
    assert!(!s.transport.playing);
}

#[test]
fn partial_and_all_failures_continue_only_with_failed_track_identities_silent() {
    for all in [false, true] {
        let mut s = Studio::new(Song::empty());
        let mut failed = Vec::new();
        // Duplicate long names cannot be used to attribute failures.
        let name = "同じ名前・長いトラック名🎹".repeat(30);
        for i in 0..3 {
            let id = s
                .song
                .run("track.add.audio", serde_json::json!({"name":name}))
                .unwrap()["id"]
                .clone();
            if all || i != 1 {
                failed.push(id.as_str().unwrap().parse::<TrackId>().unwrap());
                missing_plugin(&mut s, id);
            }
        }
        let project = s.project();
        s.transport.mode = OutputMode::Device;
        s.transport.set_catalog(Arc::new(Catalog::default()));
        s.transport.set_drivers(quiet_drivers());
        s.prepare_song();
        wait_failed(&mut s, failed.len());
        let card = s.preparing.as_ref().unwrap();
        assert_eq!(card.progress.done, 3 - failed.len());
        assert!(
            card.failures
                .iter()
                .all(|f| !f.reason.is_empty() && failed.contains(&f.track.unwrap()))
        );
        let layout = card.layout((960.0, 600.0));
        assert!(layout.proceed.is_some());
        assert!(layout.retry.is_some());
        assert!(
            s.transport.host().is_none(),
            "candidate is held before output adoption"
        );
        s.prepare_continue();
        let deadline = Instant::now() + Duration::from_secs(20);
        while s.preparing.is_some() {
            s.poll_preparing();
            assert!(Instant::now() < deadline, "continue watchdog");
            std::thread::yield_now();
        }
        assert!(
            Arc::ptr_eq(&project, &s.project()),
            "continuing never edits the song"
        );
        let unavailable = s.transport.host().unwrap().unavailable_tracks();
        assert_eq!(unavailable.len(), failed.len());
        for id in failed {
            assert!(s.sound_unavailable(id));
        }
        assert!(!s.transport.playing);
    }
}

#[test]
fn master_failure_never_offers_continue_and_retry_uses_a_new_result() {
    let mut s = Studio::new(Song::empty());
    let master = s
        .project()
        .tracks
        .keys()
        .find(|id| id.is_master())
        .unwrap()
        .to_string();
    missing_plugin(&mut s, serde_json::json!(master));
    s.transport.mode = OutputMode::Device;
    s.transport.set_catalog(Arc::new(Catalog::default()));
    s.transport.set_drivers(quiet_drivers());
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        s.poll_preparing();
        if s.preparing.as_ref().unwrap().failure.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "master failure watchdog");
        std::thread::yield_now();
    }
    assert!(
        s.preparing
            .as_ref()
            .unwrap()
            .layout((960.0, 600.0))
            .proceed
            .is_none()
    );
    s.prepare_continue();
    assert!(s.transport.host().is_none());
    // Changing the test's source directly models a newly available dependency;
    // user mutations remain blocked behind the failure card.
    let id = s
        .project()
        .tracks
        .values()
        .find(|t| t.id.is_master())
        .unwrap()
        .inserts[0]
        .id;
    s.song
        .run("plugin.remove", serde_json::json!({"ids":[id.to_string()]}))
        .unwrap();
    s.prepare_retry();
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "master retry watchdog");
        std::thread::yield_now();
    }
    assert!(s.transport.host().is_some());
    assert!(s.transport.host().unwrap().unavailable_tracks().is_empty());
}

#[test]
fn output_and_global_graph_failures_do_not_offer_silent_continuation() {
    for graph in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().to_path_buf();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if graph && op == "prepare_graph" && path.starts_with(&prefix) {
                return Err(std::io::Error::other("graph failed"));
            }
            Ok(())
        }));
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.transport.mode = OutputMode::Device;
        let mut drivers = quiet_drivers();
        drivers.shared_out = Arc::new(|_, _, _| Err("output failed".into()));
        s.transport.set_drivers(drivers);
        s.prepare_song();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            s.poll_preparing();
            let card = s.preparing.as_ref().unwrap();
            if let Some(reason) = &card.failure {
                assert!(
                    reason.contains(if graph {
                        "graph failed"
                    } else {
                        "output failed"
                    }),
                    "{reason}"
                );
                assert!(card.layout((960.0, 600.0)).proceed.is_none());
                break;
            }
            assert!(Instant::now() < deadline, "global failure watchdog");
            std::thread::yield_now();
        }
        s.prepare_continue();
        assert!(s.transport.host().is_none());
        assert!(s.preparing.is_some());
    }
}

#[test]
fn dropping_the_window_transport_returns_before_a_held_output_stop() {
    let mut s = Studio::new(Song::empty());
    s.transport.mode = OutputMode::Device;
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let mut drivers = quiet_drivers();
    drivers.shared_out = Arc::new(move |audio, _, _| {
        let entered = tx.clone();
        let release = gate.clone();
        DeviceOutput::open_with(audio, move |audio| {
            Ok(Box::new(HeldOutput {
                audio: Some(audio),
                entered,
                release,
            }))
        })
        .map(Output::Device)
    });
    s.transport.set_drivers(drivers);
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(20);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline, "output creation watchdog");
        std::thread::yield_now();
    }
    let (returned, ack) = mpsc::channel();
    let ui = std::thread::current().id();
    let cleanup = std::thread::spawn(move || {
        // Release even on failure; only the failure watchdog depends on time.
        let release = release;
        let result = ack.recv_timeout(Duration::from_secs(20));
        release.release();
        result.expect("Transport::drop waited on output stop");
        assert_ne!(entered.recv_timeout(Duration::from_secs(20)).unwrap(), ui);
    });
    drop(s);
    returned.send(()).unwrap();
    cleanup.join().unwrap();
}

#[test]
fn native_material_gates_keep_preparation_input_and_cancel_responsive_until_cleanup() {
    for op in [
        "prepare_decode",
        "prepare_convert",
        "prepare_prime",
        "prepare_graph",
    ] {
        let root = tempfile::tempdir().unwrap();
        let wav = root.path().join("source.wav");
        let large = op == "prepare_prime";
        let audio = vec![0.1; if large { 4_200_000 } else { 2048 }];
        xirius_session::export::write_wav(
            &wav,
            &audio,
            &audio,
            if large { 48_000 } else { 22_050 },
            16,
        )
        .unwrap();
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&root.path().join("song")).unwrap();
        s.song
            .run("clip.import", serde_json::json!({"file":wav}))
            .unwrap();
        s.transport.mode = OutputMode::Device;
        s.transport.set_drivers(quiet_drivers());
        let release = Release::new();
        let gate = release.0.clone();
        let watch = root.path().to_path_buf();
        let seen = AtomicBool::new(false);
        let (tx, entered) = mpsc::channel();
        let ui = std::thread::current().id();
        let observe = xirius_core::io_test::observe(Arc::new(move |event, path| {
            if event == op && path.starts_with(&watch) && !seen.swap(true, Ordering::SeqCst) {
                tx.send(std::thread::current().id()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |done| !*done).unwrap());
            }
            Ok(())
        }));
        s.prepare_song();
        assert!(s.preparing.as_ref().unwrap().background);
        let deadline = Instant::now() + Duration::from_secs(30);
        let worker = loop {
            s.poll_preparing();
            if let Ok(thread) = entered.try_recv() {
                break thread;
            }
            assert!(
                Instant::now() < deadline,
                "{op} entry watchdog: {:?}",
                s.error
            );
            std::thread::yield_now();
        };
        assert_ne!(worker, ui);
        assert!(s.transport.host().is_none());
        let original = s.project();
        for size in [(1200.0, 800.0), (880.0, 600.0)] {
            let g = s.geometry(size.0, size.1);
            assert!(s.poll_preparing());
            assert!(!s.run_command("track.add.audio", serde_json::json!({"name":"blocked"})));
            assert!(s.key(&g, "s", true, false));
            assert!(!s.toggle_play());
            assert!(!s.toggle_record());
            let layout = s.preparing.as_ref().unwrap().layout(g.size);
            assert!(layout.rect.right() <= size.0);
        }
        assert!(Arc::ptr_eq(&original, &s.project()));
        assert!(s.prepare_key("escape"));
        assert!(s.preparing.as_ref().unwrap().cancelling);
        assert!(s.poll_preparing());
        assert!(s.preparing.is_some(), "{op}: cleanup is still held");
        let card = s.preparing.as_ref().unwrap();
        let now = Instant::now();
        assert_eq!(card.bar(now), card.bar(now + Duration::from_secs(10)));
        release.release();
        let deadline = Instant::now() + Duration::from_secs(30);
        while s.preparing.is_some() || s.opening.is_some() {
            s.poll_preparing();
            s.poll_opening();
            assert!(Instant::now() < deadline, "{op} cleanup watchdog");
            std::thread::yield_now();
        }
        assert!(s.song.dir().is_none(), "{op}: cancellation closes the song");
        assert!(s.transport.host().is_none());
        assert!(!s.transport.playing);
        drop(observe);
    }
}
