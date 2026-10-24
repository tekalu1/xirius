use super::*;
use crate::studio::{FileRequest, Studio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, mpsc};
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(60);
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Release {
    fn new() -> Self {
        Self(Arc::new((Mutex::new(false), Condvar::new())))
    }
    fn release(&self) {
        let (lock, cv) = &*self.0;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.release();
    }
}
fn wait(gate: &Arc<(Mutex<bool>, Condvar)>) {
    let (lock, cv) = &**gate;
    drop(
        cv.wait_while(lock.lock().unwrap(), |released| !*released)
            .unwrap(),
    );
}

#[test]
fn worker_cutoff_through_song_returns_the_real_session() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut song = Song::empty();
    song.save_as(&dir).unwrap();
    song.run_human("track.add.audio", json!({"name":"unsaved"}))
        .unwrap();
    let uses = song.session.uses_id();
    let project = song.project();
    let target = dir.clone();
    let once = AtomicBool::new(false);
    let _observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "handoff" && path == target && !once.swap(true, Ordering::Relaxed) {
            panic!("injected worker cutoff");
        }
        Ok(())
    }));
    assert!(song.open(&dir).is_err());
    assert_eq!(song.session.uses_id(), uses);
    assert!(Arc::ptr_eq(&project, &song.project()));
    // Another session must get another lane; the recovered session still owns lane 0.
    let other = Session::open(&dir).unwrap();
    assert_eq!(other.store().unwrap().lane, 1);
    song.run_human("track.add.audio", json!({"name":"after recovery"}))
        .unwrap();
}

#[test]
fn unrecoverable_receiver_error_detaches_and_blocks_the_presentation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut song = Song::empty();
    song.save_as(&dir).unwrap();
    let real = song.take_session_for_save();
    let index = std::fs::read(dir.join("history/index.json")).unwrap();
    assert!(song.finish_open(Err(mpsc::RecvError)).is_err());
    assert!(song.session.dir().is_none());
    let cursor = song.session.cursor();
    let current = song.project();
    assert!(song.session.jump(cursor).is_err());
    assert!(song.session.undo(1).is_err());
    assert!(song.session.redo(1).is_err());
    assert!(song.discard_unsaved().is_err());
    assert_eq!(song.session.cursor(), cursor);
    assert!(Arc::ptr_eq(&current, &song.project()));
    assert!(
        song.run_human("track.add.audio", json!({"name":"forbidden"}))
            .is_err()
    );
    assert!(song.save_as(&tmp.path().join("copy")).is_err());
    assert!(song.session.save().is_err());
    assert!(song.session.autosave_if_due(u64::MAX).is_err());
    let mut preview = song.session.save_preview();
    assert!(preview.save_as(tmp.path().join("preview-copy")).is_err());
    assert_eq!(
        std::fs::read(dir.join("history/index.json")).unwrap(),
        index
    );
    assert!(!tmp.path().join("copy").exists());
    drop(real);
}

#[test]
fn song_and_studio_entrances_have_no_caller_store_io() {
    for entrance in ["song", "same", "version", "studio", "studio-version"] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        let version_dir = tmp.path().join("version");
        let mut song = Song::empty();
        song.save_as(&source).unwrap();
        let version = song.session.save_version("snapshot").unwrap();
        let mut other = Song::empty();
        other.save_as(&target).unwrap();
        drop(other);
        let caller = std::thread::current().id();
        let root = tmp.path().to_path_buf();
        let events = Arc::new(Mutex::new(Vec::new()));
        let log = events.clone();
        let _observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if path.starts_with(&root) {
                log.lock()
                    .unwrap()
                    .push((op.to_string(), std::thread::current().id()));
            }
            Ok(())
        }));
        // A caller-side query is visible even with no OpenProgress installed.
        assert!(xirius_core::store::Store::is_song(&target));
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|(_, thread)| *thread == caller)
        );
        events.lock().unwrap().clear();
        match entrance {
            "song" => song.open(&target).unwrap(),
            "same" => song.open(&source).unwrap(),
            "version" => song
                .open_version_as_song(&version.id, &version_dir)
                .unwrap(),
            _ => {
                let mut studio = Studio::sample();
                studio.song = song;
                if entrance == "studio-version" {
                    studio.finish_file_request(FileRequest::OpenVersion(version.id), &version_dir);
                } else {
                    studio.finish_file_request(FileRequest::Open, &target);
                }
                assert!(studio.error.is_none(), "{:?}", studio.error);
                // Finish the observation before ordinary teardown of the adopted song.
                let trace = events.lock().unwrap().clone();
                assert!(
                    trace.iter().all(|(_, thread)| *thread != caller),
                    "{entrance}: {trace:?}"
                );
                drop(_observation);
                continue;
            }
        }
        let trace = events.lock().unwrap().clone();
        assert!(
            trace.iter().all(|(_, thread)| *thread != caller),
            "{entrance}: {trace:?}"
        );
    }
}

#[test]
fn pending_writer_finishes_before_song_reopen_touches_candidate_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut song = Song::empty();
    song.save_as(&dir).unwrap();
    let release = Release::new();
    let freed = release.0.clone();
    let (entered_tx, entered) = mpsc::channel();
    let (pending_tx, pending) = mpsc::channel();
    let (complete_tx, complete) = mpsc::channel();
    let writer_id = Mutex::new(None);
    let completed = AtomicBool::new(false);
    let worker = Arc::new(Mutex::new(None));
    let worker_id = worker.clone();
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let target = dir.clone();
    let once = AtomicBool::new(false);
    let _observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
        let thread = std::thread::current();
        let id = thread.id();
        if op == "writer" && path == target {
            *worker_id.lock().unwrap() = Some(id);
        }
        if op == "writer_pending" && *worker_id.lock().unwrap() == Some(id) {
            let _ = pending_tx.send(());
        }
        if op == "writer_complete" && *writer_id.lock().unwrap() == Some(id) {
            completed.store(true, Ordering::Release);
            let _ = complete_tx.send(());
        }
        if path.starts_with(&target) {
            log.lock().unwrap().push((op.to_string(), id));
            if op != "writer" && *worker_id.lock().unwrap() == Some(id) {
                assert!(
                    completed.load(Ordering::Acquire),
                    "candidate {op} preceded writer completion"
                );
            }
            if op == "write"
                && thread.name() == Some("xirius-history-writer")
                && !once.swap(true, Ordering::Relaxed)
            {
                *writer_id.lock().unwrap() = Some(id);
                entered_tx.send(()).unwrap();
                wait(&freed);
            }
        }
        Ok(())
    }));
    song.run_human("track.add.audio", json!({"name":"pending"}))
        .unwrap();
    entered.recv_timeout(LIMIT).unwrap();
    assert!(song.writes_pending());
    let open_dir = dir.clone();
    let ui = std::thread::spawn(move || {
        let caller = std::thread::current().id();
        let result = song.open(&open_dir);
        (song, caller, result)
    });
    pending
        .recv_timeout(LIMIT)
        .expect("the real writer never became pending at flush");
    let open_worker = worker.lock().unwrap().unwrap();
    assert!(matches!(
        complete.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, id)| *id == open_worker)
            .all(|(op, _)| op == "writer")
    );
    release.release();
    complete
        .recv_timeout(LIMIT)
        .expect("writer did not complete its actual writes");
    let (song, caller, result) = ui.join().unwrap();
    result.unwrap();
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .all(|(_, thread)| *thread != caller)
    );
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|(op, id)| op == "read" && *id == open_worker)
    );
    drop(_observation);
    drop(song);
}

#[test]
fn studio_reopens_while_actual_tidy_io_is_held_and_keeps_recordings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut song = Song::empty();
    song.save_as(&dir).unwrap();
    drop(song);
    let recordings = dir.join("Media/Recordings");
    std::fs::create_dir_all(&recordings).unwrap();
    let stray = recordings.join("stray.wav");
    std::fs::write(&stray, b"RIFF").unwrap();
    let release = Release::new();
    let freed = release.0.clone();
    let (tx, rx) = mpsc::channel();
    let target = dir.clone();
    let once = AtomicBool::new(false);
    let _observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
        if op == "read"
            && path.starts_with(&target)
            && std::thread::current().name() == Some("xirius-cleanup")
            && !once.swap(true, Ordering::Relaxed)
        {
            tx.send(()).unwrap();
            wait(&freed);
        }
        Ok(())
    }));
    let (go_tx, go) = mpsc::channel();
    let (opened_tx, opened) = mpsc::channel();
    let ui_dir = dir.clone();
    let ui = std::thread::spawn(move || {
        let mut studio = Studio::sample();
        studio.start_tidy(cleanup::Closed::saved(
            ui_dir.clone(),
            Arc::new(|_| Err("unexpected trash".into())),
        ));
        go.recv_timeout(LIMIT).unwrap();
        studio.finish_file_request(FileRequest::Open, &ui_dir);
        assert!(studio.error.is_none(), "{:?}", studio.error);
        assert_eq!(studio.song.dir().as_deref(), Some(ui_dir.as_path()));
        assert!(studio.is_tidying());
        opened_tx.send(()).unwrap();
        studio.await_tidy_for_test();
        assert!(studio.error.is_none(), "{:?}", studio.error);
    });
    rx.recv_timeout(LIMIT).unwrap();
    go_tx.send(()).unwrap();
    opened
        .recv_timeout(LIMIT)
        .expect("reopen waited for tidy I/O");
    release.release();
    ui.join().unwrap();
    assert_eq!(std::fs::read(stray).unwrap(), b"RIFF");
}
