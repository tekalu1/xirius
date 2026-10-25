use super::*;
use crate::{saving::Leave, song::Song};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};

struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Release {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }
    fn release(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}
fn finish(s: &mut Studio) {
    let end = Instant::now() + Duration::from_secs(15);
    while s.opening.is_some() {
        assert!(Instant::now() < end, "open never returned");
        s.poll_opening();
        std::thread::yield_now();
    }
    s.keep.settle_recent();
    s.keep.poll_recent();
}
#[test]
fn every_entry_returns_before_worker_io_and_blocks_input_before_the_veil() {
    for entry in [
        "file",
        "recent",
        "new",
        "sample",
        "dawproject",
        "startup",
        "command",
        "version",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let target = tmp.path().join("target");
        let mut s = Studio::new(Song::empty());
        s.song.save_as(&old).unwrap();
        s.song
            .run("track.add.audio", json!({"name":"retained"}))
            .unwrap();
        s.song.flush_writes();
        let before = s.project();
        let version = s.song.session_mut().save_version("version").unwrap();
        let mut other = Song::empty();
        other.save_as(&target).unwrap();
        drop(other);
        let root = tmp.path().to_path_buf();
        let ui = std::thread::current().id();
        let release = Release::new();
        let gate = release.0.clone();
        let (tx, entered) = mpsc::channel();
        let seen = AtomicBool::new(false);
        let (trace_tx, trace) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if path.starts_with(&root) {
                trace_tx.send(std::thread::current().id()).unwrap();
                if op == "writer" && !seen.swap(true, Ordering::SeqCst) {
                    tx.send(()).unwrap();
                    let guard = gate.0.lock().unwrap();
                    drop(gate.1.wait_while(guard, |done| !*done).unwrap());
                }
            }
            Ok(())
        }));
        match entry {
            "new" => s.start_leaving(Leave::New),
            "sample" => s.start_leaving(Leave::Sample),
            "startup" => s.start_keeping(crate::keep::Keep::at(tmp.path().join("keep"))),
            "command" => {
                assert!(s.run_command("project.open", json!({"path":target})));
            }
            "version" => s.start_file_request(
                FileRequest::OpenVersion(version.id),
                &tmp.path().join("copy"),
            ),
            "dawproject" => {
                assert!(s.dawproject_open(&tmp.path().join("missing.dawproject")));
            }
            _ => s.start_file_request(FileRequest::Open, &target),
        }
        assert!(s.input_blocked(), "{entry}");
        let since = s.opening.as_ref().unwrap().since;
        assert!(
            !s.opening
                .as_ref()
                .unwrap()
                .visible(since + Duration::from_millis(199))
        );
        assert!(
            s.opening
                .as_ref()
                .unwrap()
                .visible(since + Duration::from_millis(200))
        );
        s.poll_opening();
        entered.recv_timeout(Duration::from_secs(10)).expect(entry);
        assert!(s.tick());
        assert!(s.idle().changed);
        assert!(!s.run_command("track.add.audio", json!({"name":"blocked"})));
        assert!(!s.request_open(Request::New));
        assert_eq!(s.toast.as_ref().unwrap().text, BUSY);
        let g = s.geometry(1200.0, 800.0);
        s.key(&g, "escape", false, false);
        assert!(s.opening.as_ref().unwrap().cancelled);
        assert_eq!(s.opening.as_ref().unwrap().title(), CANCELLING);
        assert!(Arc::ptr_eq(&before, &s.project()));
        assert!(!s.request_close());
        assert!(s.opening.as_ref().unwrap().confirm_close);
        s.opening_key("escape");
        assert!(!s.opening.as_ref().unwrap().confirm_close);
        assert!(
            trace.try_iter().all(|thread| thread != ui),
            "{entry}: caller did I/O"
        );
        release.release();
        finish(&mut s);
        assert!(
            Arc::ptr_eq(&before, &s.project()),
            "{entry}: lost the old session"
        );
        assert!(!s.transport.playing);
        drop(observation);
    }
}
#[test]
fn progress_is_throttled_independently_from_the_veil_and_freezes_on_cancel() {
    let mut s = Studio::new(Song::empty());
    s.request_open(Request::New);
    let open = s.opening.as_mut().unwrap();
    let t = open.since;
    assert!(!open.visible(t + Duration::from_millis(199)));
    assert!(open.visible(t + Duration::from_millis(200)));
    assert_eq!(count(1234567), "1,234,567");
    s.cancel_opening();
    let frozen = s.opening.as_ref().unwrap().detail();
    s.opening
        .as_mut()
        .unwrap()
        .update(t + Duration::from_secs(10), false);
    assert_eq!(s.opening.as_ref().unwrap().detail(), frozen);
    finish(&mut s);
}
#[test]
fn stale_ready_is_aborted_before_adoption() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let mut song = Song::empty();
    song.save_as(&target).unwrap();
    drop(song);
    let mut s = Studio::new(Song::empty());
    let old = s.project();
    s.request_open_file(FileRequest::Open, &target);
    s.poll_opening();
    s.open_generation += 1;
    finish(&mut s);
    assert!(Arc::ptr_eq(&old, &s.project()));
}
#[test]
fn closing_is_a_confirmation_and_disconnects_without_waiting_for_a_session() {
    let mut s = Studio::new(Song::empty());
    s.request_open(Request::New);
    assert!(!s.request_close());
    s.opening_key("escape");
    assert!(!s.leaving_ready);
    assert!(!s.request_close());
    s.opening_key("tab");
    s.opening_key("enter");
    assert!(s.opening.is_none());
    assert!(s.request_close());
}
#[test]
fn connected_mcp_answers_only_after_adoption_and_rejects_edits_while_blocked() {
    use xirius_mcp::Host;
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target");
    let mut song = Song::empty();
    song.save_as(&target).unwrap();
    drop(song);
    let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
    let file = target.clone();
    let call = std::thread::spawn(move || {
        bridge.run(
            "project.open",
            json!({"path":file}).as_object().unwrap(),
            &xirius_core::session::RunOptions::human(),
            false,
        )
    });
    let end = Instant::now() + Duration::from_secs(10);
    let job = loop {
        if let Ok(job) = jobs.try_recv() {
            break job;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    };
    let mut s = Studio::new(Song::empty());
    s.ai_job(job);
    assert!(s.opening.is_some());
    assert!(!call.is_finished());
    assert!(!s.run_command("track.add.audio", json!({"name":"blocked"})));
    finish(&mut s);
    assert!(call.join().unwrap().is_ok());
    assert_eq!(s.song.dir().as_deref(), Some(target.as_path()));
}

#[test]
fn dawproject_cancel_cleans_only_its_exclusive_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let media = tmp.path().join("media");
    std::fs::create_dir(&media).unwrap();
    let sentinel = media.join("user.wav");
    std::fs::write(&sentinel, b"user audio").unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../xirius-core/src/dawproject/testdata/reference/AudioWARPEDWithFades.dawproject");
    let release = Release::new();
    let gate = release.0.clone();
    let into = media.clone();
    let (tx, entered) = mpsc::channel();
    let seen = AtomicBool::new(false);
    let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "write" && path.starts_with(&into) && !seen.swap(true, Ordering::SeqCst) {
            tx.send(path.to_path_buf()).unwrap();
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |done| !*done).unwrap());
        }
        Ok(())
    }));
    let mut s = Studio::new(Song::empty());
    let old = s.project();
    assert!(s.request_open(Request::Core(OpenRequest::Dawproject {
        path,
        media: media.clone()
    })));
    s.poll_opening();
    let extraction = entered
        .recv_timeout(Duration::from_secs(10))
        .expect("extracting audio");
    let private = extraction.parent().unwrap().to_path_buf();
    assert_ne!(private, media);
    s.cancel_opening();
    release.release();
    finish(&mut s);
    assert!(Arc::ptr_eq(&old, &s.project()));
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"user audio");
    assert!(!private.exists());
    assert!(media.exists());
    drop(observation);
}

#[test]
fn stale_final_success_is_disposed_on_the_worker_and_not_installed() {
    let tmp = tempfile::tempdir().unwrap();
    let olddir = tmp.path().join("old");
    let next = tmp.path().join("new");
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&olddir).unwrap();
    let mut other = Song::empty();
    other.save_as(&next).unwrap();
    drop(other);
    let release = Release::new();
    let gate = release.0.clone();
    let watch = olddir.clone();
    let (tx, entered) = mpsc::channel();
    let seen = AtomicBool::new(false);
    let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "remove" && path.starts_with(&watch) && !seen.swap(true, Ordering::SeqCst) {
            tx.send(()).unwrap();
            let guard = gate.0.lock().unwrap();
            drop(gate.1.wait_while(guard, |done| !*done).unwrap());
        }
        Ok(())
    }));
    let old = s.project();
    s.request_open_file(FileRequest::Open, &next);
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        s.poll_opening();
        if entered.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    s.open_generation += 1;
    release.release();
    finish(&mut s);
    assert!(Arc::ptr_eq(&old, &s.project()));
    assert!(!s.run_command("track.add.audio", json!({"name":"unavailable"})));
    drop(observation);
}
#[test]
fn display_and_reading_announcements_use_the_injected_clock() {
    let mut s = Studio::new(Song::empty());
    s.request_open(Request::New);
    let open = s.opening.as_mut().unwrap();
    let t = open.since;
    let snapshot = |n, stage| OpenStatus {
        reads: n,
        current: PathBuf::from(format!("history/{n}.json")),
        stage,
    };
    open.display(snapshot(1, "read"), t, false);
    assert_eq!(open.status.reads, 1); // state change is immediate
    open.display(snapshot(2, "read"), t + Duration::from_millis(249), false);
    assert_eq!(open.status.reads, 1);
    open.display(snapshot(3, "read"), t + Duration::from_millis(250), false);
    assert_eq!(open.status.reads, 3);
    assert!(open.announcement.contains("1 ファイル"));
    open.display(snapshot(4, "read"), t + Duration::from_millis(4999), false);
    assert!(open.announcement.contains("1 ファイル"));
    open.display(snapshot(5, "read"), t + Duration::from_millis(5000), false);
    assert!(open.announcement.contains("4 ファイル"));
    open.display(snapshot(6, "read"), t + Duration::from_millis(5250), true);
    assert_eq!(open.status.reads, 4);
    open.display(snapshot(7, "read"), t + Duration::from_millis(5999), true);
    assert_eq!(open.status.reads, 7);
    open.display(snapshot(8, "lock"), t + Duration::from_millis(6000), true);
    assert_eq!(open.detail(), "ロック待ち 8 ファイル");
    assert_eq!(open.announcement, open.detail());
    s.cancel_opening();
    let open = s.opening.as_mut().unwrap();
    open.display(snapshot(9, "writer"), t + Duration::from_secs(10), false);
    assert_eq!(open.detail(), "読み込み 8 ファイル");
    finish(&mut s);
}
#[test]
fn mcp_new_song_is_deferred_and_read_only_dry_run_does_not_replace() {
    use xirius_core::session::RunOptions;
    use xirius_mcp::Host;
    let mut s = Studio::sample();
    s.song
        .session_mut()
        .set_ai_permission(xirius_core::session::AiPermission::Read)
        .unwrap();
    let reply = s
        .song
        .session_mut()
        .prepare_open(
            "project.new",
            &serde_json::Map::new(),
            &RunOptions::ai("AI"),
        )
        .unwrap();
    assert!(matches!(reply,Err(reply) if reply.dry_run && reply.forced_dry_run));
    assert!(s.opening.is_none());
    assert!(s.song.is_sample());
    s.song
        .session_mut()
        .set_ai_permission(xirius_core::session::AiPermission::Edit)
        .unwrap();
    let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
    let call = std::thread::spawn(move || {
        bridge.run(
            "project.new",
            &serde_json::Map::new(),
            &RunOptions::ai("AI"),
            false,
        )
    });
    let end = Instant::now() + Duration::from_secs(10);
    let job = loop {
        if let Ok(job) = jobs.try_recv() {
            break job;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    };
    s.ai_job(job);
    assert!(s.opening.is_some());
    assert!(!call.is_finished());
    finish(&mut s);
    assert!(s.start_shown());
    assert!(call.join().unwrap().is_ok());
}

#[test]
fn esc_during_retirement_has_no_cancel_control_or_cancellation_promise() {
    let tmp = tempfile::tempdir().unwrap();
    let old = tmp.path().join("old");
    let next = tmp.path().join("next");
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&old).unwrap();
    let mut target = Song::empty();
    target.save_as(&next).unwrap();
    drop(target);
    let release = Release::new();
    let gate = release.0.clone();
    let (tx, entered) = mpsc::channel();
    let once = AtomicBool::new(false);
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "remove" && path.starts_with(&old) && !once.swap(true, Ordering::SeqCst) {
            tx.send(()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |released| !*released).unwrap());
        }
        Ok(())
    }));
    s.request_open_file(FileRequest::Open, &next);
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        s.poll_opening();
        if entered.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    let open = s.opening.as_ref().unwrap();
    assert_eq!(open.title(), ADOPTING);
    assert!(!open.can_cancel());
    let generation = s.open_generation;
    let g = s.geometry(1200.0, 800.0);
    s.key(&g, "escape", false, false);
    assert_eq!(s.open_generation, generation);
    assert!(!s.opening.as_ref().unwrap().cancelled);
    assert!(s.tick());
    assert!(s.input_blocked());
    release.release();
    finish(&mut s);
    assert_eq!(s.song.dir(), Some(next));
    drop(observe);
}

#[test]
fn closing_an_unadopted_startup_preserves_the_previous_last_open() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("song");
    let root = tmp.path().join("keep");
    let mut seed = Song::empty();
    seed.save_as(&path).unwrap();
    drop(seed);
    let keep = crate::keep::Keep::at(root.clone());
    keep.remember(&path, "song", false, 1);
    keep.closed_with(&crate::keep::LastOpen::Song(path.clone()));
    let release = Release::new();
    let gate = release.0.clone();
    let watch = path.clone();
    let (tx, entered) = mpsc::channel();
    let once = AtomicBool::new(false);
    let observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
        if op == "read" && file.starts_with(&watch) && !once.swap(true, Ordering::SeqCst) {
            tx.send(()).unwrap();
            let held = gate.0.lock().unwrap();
            drop(gate.1.wait_while(held, |released| !*released).unwrap());
        }
        Ok(())
    }));
    let mut s = Studio::new(Song::empty());
    keep.settle_recent();
    s.start_keeping(crate::keep::Keep::at(root));
    s.poll_opening();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(!s.request_close());
    s.accept_open_close();
    release.release();
    s.wait_open_close(Duration::from_secs(10));
    assert_eq!(
        keep.settled_last_open(),
        Some(crate::keep::LastOpen::Song(path))
    );
    drop(observe);
}

#[test]
fn close_and_quit_during_publication_or_retirement_do_not_promise_cancellation() {
    use crate::keep::{Keep, LastOpen};
    for publication in [true, false] {
        for quit in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let (mut s, version) = version_source(tmp.path());
            let old = s.song.record_dir().unwrap();
            let destination = tmp.path().join("copy");
            let root = tmp.path().join("keep");
            s.keep = Keep::at(root.clone());
            s.keep.sync(&s.project());
            s.keep.closed_with(&LastOpen::Song(old.clone()));
            let release = Release::new();
            let held = release.0.clone();
            let watch = if publication {
                destination.clone()
            } else {
                old.clone()
            };
            let (tx, entered) = mpsc::channel();
            let once = AtomicBool::new(false);
            let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if ((publication && op == "publish" && path == watch)
                    || (!publication && op == "remove" && path.starts_with(&watch)))
                    && !once.swap(true, Ordering::SeqCst)
                {
                    tx.send(()).unwrap();
                    let guard = held.0.lock().unwrap();
                    drop(held.1.wait_while(guard, |done| !*done).unwrap());
                }
                Ok(())
            }));
            s.start_file_request(FileRequest::OpenVersion(version), &destination);
            let end = Instant::now() + Duration::from_secs(15);
            loop {
                s.poll_opening();
                if entered.try_recv().is_ok() {
                    break;
                }
                assert!(Instant::now() < end);
                std::thread::yield_now();
            }
            assert_eq!(destination.exists(), !publication);
            assert_eq!(s.opening.as_ref().unwrap().title(), ADOPTING);
            if quit {
                let g = s.geometry(1200.0, 800.0);
                assert!(s.run_parsed(&g, "app.quit".into(), serde_json::Map::new()));
            } else {
                assert!(!s.request_close());
            }
            let open = s.opening.as_ref().unwrap();
            assert!(open.confirm_close);
            assert_eq!(open.close_title(), "閉じますか");
            assert_eq!(open.close_action(), "閉じる");
            // Select the real confirmation action through the input path.
            s.opening_key("right");
            s.opening_key("enter");
            assert!(s.opening.is_none());
            assert_eq!(
                s.window_ask,
                Some(crate::context::key_more::WindowAsk::Quit)
            );
            assert_eq!(destination.exists(), !publication);
            release.release();
            s.wait_open_close(Duration::from_secs(15));
            assert!(destination.join("project.json").exists());
            assert_eq!(
                Keep::at(root).settled_last_open(),
                Some(LastOpen::Song(old))
            );
            drop(observation);
        }
    }
}

#[test]
fn zero_reads_are_omitted_only_for_lock_and_writer_waits() {
    let mut s = Studio::new(Song::empty());
    s.request_open(Request::New);
    let open = s.opening.as_mut().unwrap();
    for (stage, expected) in [
        ("writer", "保存待ち"),
        ("lock", "ロック待ち"),
        ("read", "読み込み 0 ファイル"),
    ] {
        open.status.stage = stage;
        assert_eq!(open.detail(), expected);
    }
    open.status.stage = "lock";
    open.status.reads = 2;
    assert_eq!(open.detail(), "ロック待ち 2 ファイル");
    s.cancel_opening();
    finish(&mut s);
}

#[test]
fn a_cancelled_startup_preserves_its_target_until_a_song_is_adopted() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("song");
    let root = tmp.path().join("keep");
    let mut seed = Song::empty();
    seed.save_as(&path).unwrap();
    drop(seed);
    let keep = crate::keep::Keep::at(root.clone());
    keep.closed_with(&crate::keep::LastOpen::Song(path.clone()));
    let mut s = Studio::new(Song::empty());
    keep.settle_recent();
    s.start_keeping(crate::keep::Keep::at(root));
    let g = s.geometry(1200.0, 800.0);
    s.key(&g, "escape", false, false);
    finish(&mut s);
    s.keep.closed_with(&crate::keep::LastOpen::Empty);
    assert_eq!(
        keep.settled_last_open(),
        Some(crate::keep::LastOpen::Song(path))
    );
    s.request_open(Request::New);
    finish(&mut s);
    assert!(!s.keep.preserves_startup_open());
    s.keep.closed_with(&crate::keep::LastOpen::Empty);
    assert_eq!(keep.settled_last_open(), Some(crate::keep::LastOpen::Empty));
}

fn start_mcp(
    s: &mut Studio,
    command: &str,
    args: serde_json::Value,
    opts: xirius_core::session::RunOptions,
) -> std::thread::JoinHandle<xirius_core::CmdResult<xirius_core::session::RunResult>> {
    use xirius_mcp::Host;
    let (bridge, mut jobs) = xirius_mcp::bridge(xirius_session::commands::shared_registry());
    let command = command.to_string();
    let call =
        std::thread::spawn(move || bridge.run(&command, args.as_object().unwrap(), &opts, false));
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(job) = jobs.try_recv() {
            s.ai_job(job);
            break;
        }
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
    call
}
fn mcp(
    s: &mut Studio,
    command: &str,
    args: serde_json::Value,
    opts: xirius_core::session::RunOptions,
) -> xirius_core::CmdResult<xirius_core::session::RunResult> {
    let call = start_mcp(s, command, args, opts);
    finish(s);
    call.join().unwrap()
}
fn version_source(root: &Path) -> (Studio, String) {
    let mut s = Studio::new(Song::empty());
    s.song.save_as(&root.join("old")).unwrap();
    s.song
        .session_mut()
        .set_ai_permission(xirius_core::AiPermission::Save)
        .unwrap();
    s.song
        .run("track.add.audio", json!({"name":"version track"}))
        .unwrap();
    let version = s.song.session_mut().save_version("snapshot").unwrap();
    s.song
        .run("track.add.audio", json!({"name":"current track"}))
        .unwrap();
    s.song.flush_writes();
    (s, version.id)
}

#[test]
fn connected_mcp_rejects_another_operator_without_losing_the_request() {
    use xirius_core::{
        command::Author,
        session::{RequestMode, RunOptions},
    };
    let mut s = Studio::sample();
    s.song
        .session_mut()
        .set_ai_permission(xirius_core::AiPermission::Save)
        .unwrap();
    s.song
        .session_mut()
        .begin_request(&Author::Ai("A".into()), "direct", RequestMode::Direct)
        .unwrap();
    let before = s.project();
    let history = s.song.session().history().len();
    for name in ["project.new", "version.open"] {
        let raw = if name == "project.new" {
            json!({})
        } else {
            json!({"id":"version","path":"unused-copy"})
        };
        assert_eq!(
            mcp(&mut s, name, raw, RunOptions::ai("B"))
                .unwrap_err()
                .code,
            xirius_core::ErrorCode::Conflict
        );
        assert!(s.opening.is_none());
        assert!(Arc::ptr_eq(&before, &s.project()));
        assert_eq!(s.song.session().history().len(), history);
        assert_eq!(s.song.session().request_info().unwrap().title, "direct");
    }
}

#[test]
fn approved_version_creation_is_cancellable_without_io_on_the_window_thread() {
    use xirius_core::session::{Confirm, RunOptions};
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, version) = version_source(tmp.path());
    s.song
        .session_mut()
        .set_ai_permission(xirius_core::AiPermission::Edit)
        .unwrap();
    s.song.session_mut().set_confirm(Confirm::OPEN_04);
    let copy = tmp.path().join("approved-copy");
    let held = mcp(
        &mut s,
        "version.open",
        json!({"id":version,"path":copy}),
        RunOptions::ai("AI"),
    )
    .unwrap();
    let token = held.decision.unwrap().token;
    let old = s.project();
    let dir = s.song.dir();
    let release = Release::new();
    let gate = release.0.clone();
    let watch = tmp.path().to_path_buf();
    let original = dir.clone().unwrap();
    let ui = std::thread::current().id();
    let (tx, entered) = mpsc::channel();
    let once = AtomicBool::new(false);
    let observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if path.starts_with(&watch) {
            assert_ne!(std::thread::current().id(), ui, "approval did I/O on UI");
            if op == "write"
                && !path.starts_with(&original)
                && path.ends_with("project.json")
                && !once.swap(true, Ordering::SeqCst)
            {
                tx.send(()).unwrap();
                let held = gate.0.lock().unwrap();
                drop(gate.1.wait_while(held, |released| !*released).unwrap());
            }
        }
        Ok(())
    }));
    let call = start_mcp(
        &mut s,
        "ai.decide",
        json!({"token":token,"answer":"yes"}),
        RunOptions::human(),
    );
    assert!(s.opening.is_some());
    s.poll_opening();
    entered.recv_timeout(Duration::from_secs(10)).unwrap();
    let g = s.geometry(1200.0, 800.0);
    s.key(&g, "escape", false, false);
    assert!(s.opening.as_ref().unwrap().cancelled);
    assert!(s.tick());
    assert!(s.idle().changed);
    release.release();
    finish(&mut s);
    assert_eq!(
        call.join().unwrap().unwrap_err().io_kind,
        Some(std::io::ErrorKind::Interrupted)
    );
    assert!(Arc::ptr_eq(&old, &s.project()));
    assert_eq!(s.song.dir(), dir);
    assert!(!copy.exists());
    let repeated = mcp(
        &mut s,
        "ai.decide",
        json!({"token":token,"answer":"yes"}),
        RunOptions::human(),
    );
    assert_eq!(repeated.unwrap_err().code, xirius_core::ErrorCode::NotFound);
    assert!(!copy.exists());
    let result = mcp(
        &mut s,
        "ai.decision.get",
        json!({"token":token}),
        RunOptions::human(),
    )
    .unwrap();
    assert_eq!(result.data["state"], "completed");
    assert_eq!(result.data["result"]["ok"], false);
    drop(observe);
}

#[test]
fn version_command_json_and_current_song_match_standalone_and_approved_execution() {
    use xirius_core::session::{Confirm, RunOptions};
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, version) = version_source(tmp.path());
    let mut cli = xirius_core::Session::open(s.song.dir().unwrap()).unwrap();
    let old = s.project();
    let dir = s.song.dir();
    let cursor = s.song.session().cursor();
    let history = s.song.session().history().len();
    let mut replies = Vec::new();
    for (suffix, approved) in [("connected", false), ("approved", true)] {
        let destination = tmp.path().join(suffix);
        let raw = json!({"id":version,"path":destination});
        let run = if approved {
            s.song
                .session_mut()
                .set_ai_permission(xirius_core::AiPermission::Edit)
                .unwrap();
            s.song.session_mut().set_confirm(Confirm::OPEN_04);
            let pending = mcp(&mut s, "version.open", raw, RunOptions::ai("AI")).unwrap();
            let token = pending.decision.unwrap().token;
            let result = mcp(
                &mut s,
                "ai.decide",
                json!({"token":token,"answer":"yes"}),
                RunOptions::human(),
            )
            .unwrap();
            let again = mcp(
                &mut s,
                "ai.decide",
                json!({"token":token,"answer":"yes"}),
                RunOptions::human(),
            );
            assert_eq!(again.unwrap_err().code, xirius_core::ErrorCode::NotFound);
            let cached = mcp(
                &mut s,
                "ai.decision.get",
                json!({"token":token}),
                RunOptions::human(),
            )
            .unwrap();
            assert_eq!(
                cached.data["result"],
                xirius_mcp::persist::result_json(&result)
            );
            result
        } else {
            mcp(&mut s, "version.open", raw, RunOptions::ai("AI")).unwrap()
        };
        assert_eq!(run.data["path"], json!(destination));
        assert!(run.data["state_hash"]["before"].is_string());
        assert_eq!(
            run.data["state_hash"]["before"],
            run.data["state_hash"]["after"]
        );
        assert!(Arc::ptr_eq(&old, &s.project()));
        assert_eq!(s.song.dir(), dir);
        assert_eq!(s.song.session().cursor(), cursor);
        assert_eq!(s.song.session().history().len(), history);
        let copy = xirius_core::Session::open(&destination).unwrap();
        assert_eq!(copy.project().tracks.len() + 1, old.tracks.len());
        let mut json = xirius_mcp::persist::result_json(&run);
        json["data"]["path"] = json!("copy");
        json["message"] = json!("created copy");
        replies.push(json);
    }
    let raw = json!({"id":version,"path":tmp.path().join("cli")});
    // This is the command runner used by the independent CLI and stdio MCP.
    let run = xirius_mcp::persist::run_and_persist(
        &mut cli,
        "version.open",
        raw.as_object().unwrap(),
        &RunOptions::ai("AI"),
        false,
    )
    .unwrap();
    let mut json = xirius_mcp::persist::result_json(&run);
    json["data"]["path"] = json!("copy");
    json["message"] = json!("created copy");
    assert_eq!(replies[0], json);
    assert_eq!(replies[1], json);
    assert_eq!(cli.project(), &*old);
}

#[test]
fn dawproject_command_json_matches_the_standalone_runner() {
    use xirius_core::session::RunOptions;
    let tmp = tempfile::tempdir().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../xirius-core/src/dawproject/testdata/reference/MIDI-CC1-AutomationInClips.dawproject",
    );
    let media = tmp.path().join("media");
    let mut s = Studio::new(Song::empty());
    let mut cli = xirius_core::Session::with_registry(
        (*s.project()).clone(),
        xirius_session::commands::shared_registry(),
    );
    cli.hash_people_edits(false);
    let raw = json!({"path":path,"media":media});
    let expected = xirius_mcp::persist::run_and_persist(
        &mut cli,
        "project.open",
        raw.as_object().unwrap(),
        &RunOptions::human(),
        false,
    )
    .unwrap();
    let actual = mcp(&mut s, "project.open", raw, RunOptions::human()).unwrap();
    assert_eq!(actual.data["path"], json!(path));
    assert_eq!(actual.data["media"], json!(media));
    assert!(actual.data["skipped"].is_array());
    assert_eq!(
        xirius_mcp::persist::result_json(&actual),
        xirius_mcp::persist::result_json(&expected)
    );
    assert_eq!(s.project().as_ref(), cli.project());
}

#[test]
fn song_open_command_keeps_the_regular_response_envelope() {
    use xirius_core::session::RunOptions;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("target");
    let mut target = Song::empty();
    target.save_as(&path).unwrap();
    drop(target);
    let mut s = Studio::new(Song::empty());
    let mut cli = xirius_core::Session::with_registry(
        (*s.project()).clone(),
        xirius_session::commands::shared_registry(),
    );
    // The window deliberately omits hashes for human edits; AI responses above
    // compare the mandatory hashes using the ordinary CLI persistence runner.
    cli.hash_people_edits(false);
    let raw = json!({"path":path});
    let expected = xirius_mcp::persist::run_and_persist(
        &mut cli,
        "project.open",
        raw.as_object().unwrap(),
        &RunOptions::human(),
        false,
    )
    .unwrap();
    let actual = mcp(&mut s, "project.open", raw, RunOptions::human()).unwrap();
    assert_eq!(
        xirius_mcp::persist::result_json(&actual),
        xirius_mcp::persist::result_json(&expected)
    );
    assert_eq!(s.song.dir().as_deref(), Some(path.as_path()));
    assert_eq!(s.project().as_ref(), cli.project());
}
