use super::*;
use serde_json::json;
use std::sync::Condvar;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(30);
const ID: OpenId = OpenId {
    job: 1,
    generation: 4,
};

struct Gate {
    release: Arc<(Mutex<bool>, Condvar)>,
    entered: mpsc::Receiver<()>,
    trace: Arc<Mutex<Vec<(String, PathBuf, std::thread::ThreadId)>>>,
    progress: Arc<OpenProgress>,
}

impl Gate {
    fn new(op: &'static str, suffix: &'static str) -> Self {
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (tx, entered) = mpsc::channel();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let progress = Arc::new(OpenProgress::default());
        let freed = release.clone();
        let events = trace.clone();
        let once = AtomicBool::new(false);
        *progress.hook.lock().unwrap() = Some(Arc::new(move |kind, path| {
            events.lock().unwrap().push((
                kind.to_owned(),
                path.to_path_buf(),
                std::thread::current().id(),
            ));
            if kind == op && path.ends_with(suffix) && !once.swap(true, Ordering::Relaxed) {
                tx.send(()).unwrap();
                let (lock, cv) = &*freed;
                drop(
                    cv.wait_while(lock.lock().unwrap(), |released| !*released)
                        .unwrap(),
                );
            }
        }));
        Self {
            release,
            entered,
            trace,
            progress,
        }
    }

    fn reached(&self) {
        self.entered
            .recv_timeout(LIMIT)
            .expect("worker did not reach I/O gate");
    }
    fn release(&self) {
        let (lock, cv) = &*self.release;
        *lock.lock().unwrap() = true;
        cv.notify_all();
    }
    fn off_caller(&self) {
        let caller = std::thread::current().id();
        assert!(
            self.trace
                .lock()
                .unwrap()
                .iter()
                .all(|(_, _, thread)| *thread != caller)
        );
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        self.release();
    }
}

fn start(old: Session, path: &Path, gate: &Gate) -> OpenJob {
    OpenJob::start_with(
        old,
        OpenRequest::Song(path.to_path_buf()),
        ID,
        gate.progress.clone(),
    )
    .unwrap_or_else(|_| panic!("spawn failed"))
}
fn finished(job: &OpenJob) -> OpenOutcome {
    job.outcome
        .recv_timeout(LIMIT)
        .expect("worker did not return session")
}
fn adopt(job: &OpenJob) -> OpenOutcome {
    assert_eq!(job.ready.recv_timeout(LIMIT).expect("no ready").id, ID);
    job.decide(ID, true);
    finished(job)
}
fn song(dir: &Path) -> Session {
    Session::create(dir, crate::model::Project::new("fixture")).unwrap()
}

#[test]
fn worker_cutoff_before_return_recovers_original_session_and_lane() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let original = song(&dir);
    let uses = original.uses_id();
    let target = dir.clone();
    let once = AtomicBool::new(false);
    let _observe = crate::io_test::observe(Arc::new(move |op, path| {
        if op == "handoff" && path == target && !once.swap(true, Ordering::Relaxed) {
            panic!("injected worker-side channel cutoff");
        }
        Ok(())
    }));
    let job = OpenJob::start(original, OpenRequest::Song(dir.clone()), ID)
        .unwrap_or_else(|_| panic!("spawn"));
    let out = job
        .finish_blocking()
        .expect("the receiver must recover its original session");
    assert!(out.result.is_err());
    assert_eq!(out.session.uses_id(), uses);
    assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
}

#[test]
fn cutoff_during_old_retirement_returns_the_committed_real_session() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let original = song(&source);
    drop(song(&target));
    let marker = source.join("session.lock");
    let once = AtomicBool::new(false);
    let _observe = crate::io_test::observe(Arc::new(move |op, path| {
        if op == "remove" && path == marker && !once.swap(true, Ordering::Relaxed) {
            panic!("injected retirement cutoff");
        }
        Ok(())
    }));
    let job = OpenJob::start(original, OpenRequest::Song(target.clone()), ID)
        .unwrap_or_else(|_| panic!("spawn"));
    let out = adopt(&job);
    assert!(
        out.result.is_ok(),
        "a committed replacement must finish the screen switch"
    );
    assert_eq!(out.session.store.as_ref().unwrap().dir, target);
    assert!(out.session.lane.is_some());
    assert!(super::super::lane::try_hold(&target, 0).unwrap().is_none());
    assert_eq!(out.session.notices.len(), 1);
}

#[test]
fn cancelled_version_write_cleans_only_its_owned_destination_and_can_retry() {
    for foreign in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("version");
        let mut old = song(&source);
        let version = old.save_version("snapshot").unwrap();
        let gate = Gate::new("write", "manifest.toml");
        let job = OpenJob::start_with(
            old,
            OpenRequest::Version {
                id: version.id.clone(),
                destination: destination.clone(),
            },
            ID,
            gate.progress.clone(),
        )
        .unwrap_or_else(|_| panic!("spawn"));
        gate.reached();
        if foreign {
            std::fs::create_dir_all(&destination).unwrap();
            for name in ["manifest.toml", "project.json", "history/lanes/0.lock"] {
                let path = destination.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"keep").unwrap();
            }
        }
        assert!(job.cancel());
        gate.release();
        let out = finished(&job);
        assert!(out.result.is_err());
        if foreign {
            for name in ["manifest.toml", "project.json", "history/lanes/0.lock"] {
                assert_eq!(std::fs::read(destination.join(name)).unwrap(), b"keep");
            }
        } else {
            assert!(
                !destination.exists(),
                "unfinished owned destination was left behind"
            );
            let retry = OpenJob::start(
                out.session,
                OpenRequest::Version {
                    id: version.id,
                    destination: destination.clone(),
                },
                ID,
            )
            .unwrap_or_else(|_| panic!("spawn"));
            let out = adopt(&retry);
            assert!(out.result.is_ok());
            assert_eq!(out.session.store.as_ref().unwrap().dir, destination);
        }
        gate.off_caller();
    }
}

#[test]
fn failed_version_write_cleans_new_but_never_removes_existing_destination() {
    for existing in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("version");
        let mut old = song(&source);
        let version = old.save_version("snapshot").unwrap();
        if existing {
            std::fs::create_dir(&destination).unwrap();
        }
        let parent = tmp.path().to_path_buf();
        let once = AtomicBool::new(false);
        let _observe = crate::io_test::observe(Arc::new(move |op, path| {
            if op == "write"
                && path.starts_with(&parent)
                && path.ends_with("manifest.toml")
                && !once.swap(true, Ordering::Relaxed)
            {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        }));
        let uses = old.uses_id();
        let job = OpenJob::start(
            old,
            OpenRequest::Version {
                id: version.id,
                destination: destination.clone(),
            },
            ID,
        )
        .unwrap_or_else(|_| panic!("spawn"));
        let out = finished(&job);
        assert!(out.result.is_err());
        assert_eq!(out.session.uses_id(), uses);
        assert_eq!(destination.exists(), existing);
        assert!(source.join("session.lock").is_file());
    }
}

#[test]
fn gates_keep_io_and_retirement_off_caller_and_cancel_returns_exact_old_session() {
    for (op, suffix) in [
        ("read", "project.json"),
        ("metadata", "manifest.toml"),
        ("lock", "0.lock"),
        ("writer", ""),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        let mut old = song(&a);
        drop(song(&b));
        old.run_value(
            "track.add.audio",
            json!({"name":"unsaved"}),
            &super::super::RunOptions::human(),
        )
        .unwrap();
        old.run_value(
            "history.undo",
            json!({}),
            &super::super::RunOptions::human(),
        )
        .unwrap();
        old.state.view.scroll_tick = 123;
        old.write_in_background(true);
        old.hash_people_edits(false);
        let current = old.current.clone();
        let uses = old.uses_id();
        let history = old.hist.steps.clone();
        let gate = Gate::new(op, suffix);
        let job = start(old, &b, &gate);
        gate.reached();
        // This is the owning dispatch thread, still executing while I/O is held.
        // No elapsed-time claim: the gate cannot release before these calls return.
        let status = job.progress.latest().unwrap();
        assert!(!status.stage.is_empty());
        job.cancel();
        assert!(job.progress.cancelled());
        assert!(matches!(job.try_finish(), Err(mpsc::TryRecvError::Empty)));
        gate.release();
        let out = finished(&job);
        assert!(out.result.is_err());
        let restored = out.session;
        assert!(Arc::ptr_eq(&current, &restored.current));
        assert_eq!(uses, restored.uses_id());
        assert_eq!(restored.hist.steps, history);
        assert_eq!(restored.hist.cursor, 0);
        assert_eq!(restored.state.view.scroll_tick, 123);
        assert!(restored.writes_in_background());
        assert!(!restored.hash_people);
        assert_eq!(restored.lane.as_ref().unwrap().k, 0);
        assert!(a.join("session.lock").is_file());
        gate.off_caller();
    }
}

#[test]
fn same_song_has_one_lane_owner_through_ready_abort_and_adoption() {
    for adopt_it in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("song");
        let old = song(&dir);
        let uses = old.uses_id();
        let gate = Gate::new("read", "project.json");
        let job = start(old, &dir.join("."), &gate);
        gate.reached();
        assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
        gate.release();
        job.ready.recv_timeout(LIMIT).unwrap();
        assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
        job.decide(ID, adopt_it);
        let out = finished(&job);
        assert_eq!(out.result.is_ok(), adopt_it);
        assert_eq!(out.session.lane.as_ref().unwrap().k, 0);
        assert_eq!(out.session.uses_id() == uses, !adopt_it);
        assert!(dir.join("session.lock").is_file());
        assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
        assert!(!dir.join("history/lanes/1.lock").exists());
        gate.off_caller();
    }
}

#[test]
fn stale_ready_and_cancel_after_ready_never_adopt() {
    for stale in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        let old = song(&a);
        let current = old.current.clone();
        drop(song(&b));
        let gate = Gate::new("unused", "unused");
        let job = start(old, &b, &gate);
        job.ready.recv_timeout(LIMIT).unwrap();
        if stale {
            job.decide(
                OpenId {
                    generation: 5,
                    ..ID
                },
                true,
            );
        } else {
            job.cancel();
            job.decide(ID, true);
        }
        let out = finished(&job);
        assert!(out.result.is_err());
        assert!(Arc::ptr_eq(&out.session.current, &current));
        assert!(!b.join("session.lock").exists());
        gate.off_caller();
    }
}

#[test]
fn failures_and_panics_return_lane_history_and_marker() {
    for panic in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("song");
        let old = song(&dir);
        let current = old.current.clone();
        let gate = Gate::new("unused", "unused");
        if panic {
            *gate.progress.hook.lock().unwrap() = Some(Arc::new(|op, path| {
                if op == "read" && path.ends_with("project.json") {
                    panic!("injected read panic");
                }
            }));
        } else {
            std::fs::write(dir.join("project.json"), b"broken").unwrap();
        }
        let job = start(old, &dir, &gate);
        let out = finished(&job);
        assert!(out.result.is_err());
        assert!(Arc::ptr_eq(&out.session.current, &current));
        assert!(dir.join("session.lock").is_file());
        assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
    }
}

#[test]
fn successful_load_has_six_reads_and_retires_old_marker_before_handoff() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let old = song(&a);
    let mut target = song(&b);
    target.save_version("snapshot").unwrap();
    crate::store::Store::new(&b)
        .write_view(&Default::default())
        .unwrap();
    drop(target);
    let gate = Gate::new("unused", "unused");
    let job = start(old, &b, &gate);
    let out = adopt(&job);
    assert!(out.result.is_ok());
    let reads: Vec<_> = gate
        .trace
        .lock()
        .unwrap()
        .iter()
        .filter(|(op, _, _)| op == "read")
        .map(|(_, p, _)| p.clone())
        .collect();
    // view, manifest, project, history index, cursor state, versions index:
    // six reads, down from seven with the second project hash read.
    assert_eq!(reads.len(), 6, "{reads:?}");
    assert_eq!(
        reads.iter().filter(|p| p.ends_with("project.json")).count(),
        1
    );
    assert_eq!(reads.len() as u64, job.progress.latest().unwrap().reads);
    assert!(!a.join("session.lock").exists());
    assert!(b.join("session.lock").is_file());
    assert!(
        gate.trace
            .lock()
            .unwrap()
            .iter()
            .any(|(op, path, _)| op == "remove" && path == &a.join("session.lock"))
    );
    gate.off_caller();
}

#[test]
fn version_creation_and_failure_are_worker_operations() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let mut old = song(&a);
    let version = old.save_version("snapshot").unwrap();
    let gate = Gate::new("write", "manifest.toml");
    let job = OpenJob::start_with(
        old,
        OpenRequest::Version {
            id: version.id,
            destination: b.clone(),
        },
        ID,
        gate.progress.clone(),
    )
    .unwrap_or_else(|_| panic!("spawn"));
    gate.reached();
    gate.release();
    let out = adopt(&job);
    assert!(out.result.is_ok(), "{:?}", out.result);
    assert_eq!(out.session.store.as_ref().unwrap().dir, b);
    gate.off_caller();
    let original = out.session.current.clone();
    let failed = OpenJob::start(
        out.session,
        OpenRequest::Version {
            id: "v999".into(),
            destination: a,
        },
        ID,
    )
    .unwrap_or_else(|_| panic!("spawn"));
    let out = finished(&failed);
    assert!(out.result.is_err());
    assert!(Arc::ptr_eq(&original, &out.session.current));
}

#[test]
fn disconnect_disposes_both_sessions_on_worker() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let old = song(&a);
    drop(song(&b));
    let gate = Gate::new("read", "project.json");
    let (tx, rx) = mpsc::channel();
    *gate.progress.ended.lock().unwrap() = Some(tx);
    let job = start(old, &b, &gate);
    gate.reached();
    drop(job);
    gate.release();
    rx.recv_timeout(LIMIT).unwrap();
    assert!(!a.join("session.lock").exists());
    assert!(!b.join("session.lock").exists());
    gate.off_caller();
}

#[test]
fn duplicate_blobs_open_once_and_corrupt_blobs_keep_saved_song_fallback() {
    for corrupt in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("song");
        let mut target = song(&dir);
        // Two references to the same immutable blob in the cursor state.
        let text = "state".repeat(1000);
        let human = super::super::RunOptions::human();
        let track = target
            .run_value("track.add.audio", json!({"name":"audio"}), &human)
            .unwrap()
            .data["id"]
            .clone();
        for _ in 0..2 {
            let plugin = target
                .run_value(
                    "plugin.add",
                    json!({"track": track, "name":"fixture", "format":"vst3", "uid":"fixture"}),
                    &human,
                )
                .unwrap()
                .data["id"]
                .clone();
            target
                .run_value(
                    "plugin.state.set",
                    json!({"id":plugin,"state":text}),
                    &human,
                )
                .unwrap();
        }
        drop(target);
        let blob = dir
            .join("history/blobs")
            .join(crate::store::sha256_hex(text.as_bytes()));
        if corrupt {
            std::fs::write(&blob, b"broken").unwrap();
        }
        let gate = Gate::new("unused", "unused");
        let job = start(Session::new("old"), &dir, &gate);
        let out = adopt(&job);
        assert!(out.result.is_ok());
        assert_eq!(
            gate.trace
                .lock()
                .unwrap()
                .iter()
                .filter(|(op, path, _)| op == "read" && path == &blob)
                .count(),
            1
        );
        if corrupt {
            assert!(
                out.session
                    .notices
                    .iter()
                    .any(|n| n.contains("履歴を読み込めなかった"))
            );
        } else {
            let p = out.session.project();
            let audio = p.tracks.values().find(|t| t.name == "audio").unwrap();
            assert_eq!(audio.inserts.len(), 2);
            assert!(
                audio
                    .inserts
                    .iter()
                    .all(|x| x.state.as_deref() == Some(text.as_str()))
            );
        }
        gate.off_caller();
    }
}

#[test]
fn occupied_lanes_and_writer_failure_return_original_without_adoption() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut holders = vec![song(&dir)];
    for _ in 1..super::super::MAX_LANES {
        holders.push(Session::open(&dir).unwrap());
    }
    let original = Session::new("old");
    let uses = original.uses_id();
    let job = OpenJob::start(original, OpenRequest::Song(dir.clone()), ID)
        .unwrap_or_else(|_| panic!("spawn"));
    let out = finished(&job);
    assert_eq!(out.session.uses_id(), uses);
    assert!(out.result.is_err());
    let gate = Gate::new("unused", "unused");
    out.session
        .writer
        .keep_error(CmdError::io("injected writer failure"));
    let job = start(out.session, &dir, &gate);
    let out = finished(&job);
    assert!(out.result.is_err());
    assert_eq!(out.session.uses_id(), uses);
    assert_eq!(out.session.take_write_errors().len(), 1);
    assert!(
        gate.trace
            .lock()
            .unwrap()
            .iter()
            .all(|(op, _, _)| op == "writer")
    );
}

#[test]
fn cancellation_after_committed_adoption_is_explicitly_too_late() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    drop(song(&dir));
    let old = song(&tmp.path().join("old"));
    let gate = Gate::new("remove", "session.lock");
    let job = start(old, &dir, &gate);
    job.ready.recv_timeout(LIMIT).unwrap();
    job.decide(ID, true);
    gate.reached();
    // The old session is still retiring, but the owner can process events.
    assert!(matches!(job.try_finish(), Err(mpsc::TryRecvError::Empty)));
    assert!(!job.cancel());
    assert!(!job.progress.cancelled());
    gate.release();
    let out = finished(&job);
    assert!(out.result.is_ok());
    gate.off_caller();
}

#[test]
fn cancelling_nonzero_lane_reopen_returns_it_before_next_generation_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    let first = song(&a);
    let old = Session::open(&a).unwrap();
    assert_eq!(old.lane.as_ref().unwrap().k, 1);
    drop(song(&b));
    let gate = Gate::new("read", "project.json");
    let a_job = start(old, &a, &gate);
    gate.reached();
    assert!(a_job.cancel());
    assert!(super::super::lane::try_hold(&a, 1).unwrap().is_none());
    gate.release();
    let out = finished(&a_job);
    assert!(out.result.is_err());
    assert_eq!(out.session.lane.as_ref().unwrap().k, 1);
    assert!(a.join("history/lanes/1/session.lock").is_file());
    let next = OpenId {
        job: 2,
        generation: 5,
    };
    let b_job = OpenJob::start(out.session, OpenRequest::Song(b.clone()), next)
        .unwrap_or_else(|_| panic!("spawn"));
    b_job.ready.recv_timeout(LIMIT).unwrap();
    // A delayed old approval still cannot revive the cancelled generation.
    a_job.decide(ID, true);
    assert!(a_job.progress.cancelled());
    b_job.decide(next, true);
    let out = finished(&b_job);
    assert!(out.result.is_ok());
    assert_eq!(out.id, next);
    assert_eq!(out.session.store.as_ref().unwrap().dir, b);
    drop(first);
}

#[test]
fn panicking_notification_keeps_extensions_and_original_session() {
    struct Fails;
    impl crate::Extension for Fails {
        fn answers(&self, _: &str) -> bool {
            false
        }
        fn song_changed(&mut self, _: &crate::ExtContext<'_>) {
            panic!("injected notification panic");
        }
        fn run(
            &mut self,
            _: &crate::ExtContext<'_>,
            _: &crate::CommandDef,
            _: &crate::Args,
            _: bool,
        ) -> CmdResult<crate::Output> {
            unreachable!()
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut old = song(&dir);
    old.add_extension(Box::new(Fails));
    let current = old.current.clone();
    let gate = Gate::new("unused", "unused");
    let job = start(old, &dir, &gate);
    let out = adopt(&job);
    assert!(out.result.is_err());
    assert!(Arc::ptr_eq(&current, &out.session.current));
    assert_eq!(out.session.extensions.len(), 1);
    assert!(dir.join("session.lock").is_file());
    assert!(super::super::lane::try_hold(&dir, 0).unwrap().is_none());
}

#[test]
fn version_ready_keeps_destination_private_and_preserves_another_saved_session() {
    for cancel in [true, false] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("version");
        let mut old = song(&source);
        let version = old.save_version("snapshot").unwrap();
        let job = OpenJob::start(
            old,
            OpenRequest::Version {
                id: version.id,
                destination: destination.clone(),
            },
            ID,
        )
        .unwrap_or_else(|_| panic!("spawn"));
        job.ready.recv_timeout(LIMIT).unwrap();
        assert!(
            !destination.exists(),
            "Ready must not publish the candidate"
        );
        drop(song(&destination));
        let mut other = Session::open(&destination).unwrap();
        other
            .run_value(
                "track.add.audio",
                json!({"name":"other"}),
                &super::super::RunOptions::human(),
            )
            .unwrap();
        other.save().unwrap();
        let project = std::fs::read(destination.join("project.json")).unwrap();
        let locks: Vec<_> = ["history/lanes/0.lock", "versions/index.lock"]
            .into_iter()
            .map(|name| {
                let path = destination.join(name);
                // Create a shared lock if the fixture has not needed it yet.
                if !path.exists() {
                    std::fs::write(&path, b"").unwrap();
                }
                (path.clone(), std::fs::File::open(path).unwrap())
            })
            .collect();
        if cancel {
            job.cancel();
        } else {
            job.decide(ID, true);
        }
        let out = finished(&job);
        assert!(out.result.is_err());
        assert_eq!(
            std::fs::read(destination.join("project.json")).unwrap(),
            project
        );
        for (path, handle) in locks {
            assert!(path.exists());
            assert_eq!(
                handle.metadata().unwrap().len(),
                std::fs::metadata(path).unwrap().len()
            );
        }
        assert!(
            super::super::lane::try_hold(&destination, 0)
                .unwrap()
                .is_none()
        );
        other.save().unwrap();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
    }
}

#[test]
fn fail_closed_rejects_direct_history_and_absolute_disk_renames() {
    for forward in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("before.wav");
        let to = tmp.path().join("after.wav");
        std::fs::write(&to, b"audio").unwrap();
        let mut real = Session::new("original");
        real.push_step(
            crate::model::Project::new("edited"),
            "clip.relink",
            json!({"disk":[{"from": from, "to": to}]}),
            &crate::command::Author::Human,
            String::new(),
        );
        if forward {
            real.undo(1).unwrap();
        }
        let mut preview = real.save_preview();
        preview.fail_closed();
        let current = preview.current.clone();
        let cursor = preview.cursor();
        assert!(preview.jump(if forward { 1 } else { 0 }).is_err());
        assert!(preview.undo(1).is_err());
        assert!(preview.redo(1).is_err());
        assert!(preview.discard_unsaved().is_err());
        assert_eq!(preview.cursor(), cursor);
        assert!(Arc::ptr_eq(&preview.current, &current));
        assert_eq!(from.exists(), forward);
        assert_eq!(to.exists(), !forward);
    }
}

#[test]
fn version_publication_collision_and_cross_volume_failure_never_copy_or_replace() {
    for fault in ["file", "empty-directory", "cross-volume", "reopen"] {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let destination = tmp.path().join("version");
        let mut old = song(&source);
        let version = old.save_version("snapshot").unwrap();
        let uses = old.uses_id();
        let target = destination.clone();
        let _observe = crate::io_test::observe(Arc::new(move |op, path| {
            if op == "publish" && path == target {
                match fault {
                    "file" => std::fs::write(&target, b"foreign")?,
                    "empty-directory" => std::fs::create_dir(&target)?,
                    "cross-volume" => return Err(std::io::ErrorKind::CrossesDevices.into()),
                    _ => {}
                }
            }
            if fault == "reopen" && op == "read" && path == target.join("project.json") {
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            Ok(())
        }));
        let job = OpenJob::start(
            old,
            OpenRequest::Version {
                id: version.id,
                destination: destination.clone(),
            },
            ID,
        )
        .unwrap_or_else(|_| panic!("spawn"));
        let out = adopt(&job);
        assert!(out.result.is_err(), "{fault}");
        assert_eq!(out.session.uses_id(), uses);
        assert!(super::super::lane::try_hold(&source, 0).unwrap().is_none());
        match fault {
            "file" => assert_eq!(std::fs::read(&destination).unwrap(), b"foreign"),
            "empty-directory" => assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0),
            "cross-volume" => assert!(!destination.exists()),
            "reopen" => {
                assert!(destination.join("project.json").is_file());
                assert!(destination.join("history/lanes/0.lock").is_file());
            }
            _ => unreachable!(),
        }
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            if fault == "cross-volume" { 1 } else { 2 },
            "private stage leaked: {fault}"
        );
    }
}

#[test]
fn cancelling_ready_version_leaves_no_destination_and_retry_saves_in_published_lane() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    let destination = tmp.path().join("version");
    let mut old = song(&source);
    let version = old.save_version("snapshot").unwrap();
    let request = OpenRequest::Version {
        id: version.id,
        destination: destination.clone(),
    };
    let job = OpenJob::start(old, request.clone(), ID).unwrap_or_else(|_| panic!("spawn"));
    job.ready.recv_timeout(LIMIT).unwrap();
    assert!(!destination.exists());
    job.cancel();
    let out = finished(&job);
    assert!(out.result.is_err());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    let retry = OpenJob::start(out.session, request, ID).unwrap_or_else(|_| panic!("spawn"));
    let mut out = adopt(&retry);
    assert!(out.result.is_ok());
    let mut other = Session::open(&destination).unwrap();
    assert_eq!(other.store().unwrap().lane, 1);
    out.session
        .run_value(
            "track.add.audio",
            json!({"name":"published"}),
            &super::super::RunOptions::human(),
        )
        .unwrap();
    out.session.save().unwrap();
    other.save().unwrap();
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2);
}
