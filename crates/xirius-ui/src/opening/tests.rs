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
