use super::*;
use crate::id::ClipId;
use crate::session::RunOptions;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Gate {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn a_held_completion_receipt_leaves_the_ui_pending_query_available() {
    let root = tempfile::tempdir().unwrap();
    let mut session = Session::new("completion");
    session.save_as(root.path()).unwrap();
    session.write_in_background(true);
    let gate = Gate::default();
    let held = gate.0.clone();
    let owner = Arc::new(Mutex::new(None));
    let worker = owner.clone();
    let dir = root.path().to_owned();
    let once = std::sync::atomic::AtomicBool::new(false);
    let (tx, entered) = mpsc::channel();
    let _observe = crate::io_test::observe(Arc::new(move |op, path| {
        if op == "write" && path.starts_with(&dir) {
            *worker.lock().unwrap() = Some(thread::current().id());
        }
        let is_owner = *worker.lock().unwrap() == Some(thread::current().id());
        if op == "writer_complete" && is_owner && !once.swap(true, Ordering::SeqCst) {
            let _ = tx.send(());
            drop(
                held.1
                    .wait_while(held.0.lock().unwrap(), |go| !*go)
                    .unwrap(),
            );
        }
        Ok(())
    }));
    run(&mut session, "track.add.audio", json!({"name":"Queued"}));
    entered.recv_timeout(Duration::from_secs(30)).unwrap();
    let (ack, answer) = mpsc::channel();
    let query = thread::spawn(move || {
        let pending = session.writes_pending();
        let _ = ack.send((session, pending));
    });
    let (session, pending) = answer
        .recv_timeout(Duration::from_secs(30))
        .expect("UI Ack while completion is still held");
    assert!(
        pending,
        "completion must remain pending until its receipt is released"
    );
    query.join().unwrap();
    drop(gate);
    session.flush_writes();
}

fn run(session: &mut Session, name: &str, args: Value) -> Value {
    session
        .run_value(name, args, &RunOptions::human())
        .unwrap()
        .data
}

#[test]
fn a_turn_coalesces_queued_states_while_the_first_disk_write_is_held() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("song");
    let mut session = Session::create(&dir, Project::new("turn")).unwrap();
    let track = run(&mut session, "track.add.instrument", json!({}))["id"].clone();
    let clip = run(
        &mut session,
        "clip.add.midi",
        json!({"track":track,"at":0,"length":1920}),
    )["id"]
        .as_str()
        .unwrap()
        .parse::<ClipId>()
        .unwrap();
    let steps = session.steps().len();
    let state = dir.join("history").join(format!("{}.json", steps + 1));
    session.write_in_background(true);
    // Release before Session drops, including when an assertion fails.
    let gate = Gate::default();
    let held = gate.0.clone();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let (entered, ready) = mpsc::channel();
    let _observe = crate::io_test::observe(Arc::new(move |op, path| {
        if op == "write" && path == state && count.fetch_add(1, Ordering::SeqCst) == 0 {
            entered.send(()).unwrap();
            let (lock, changed) = &*held;
            drop(
                changed
                    .wait_while(lock.lock().unwrap(), |open| !*open)
                    .unwrap(),
            );
        }
        Ok(())
    }));
    session.begin_turn();
    let args = json!({"ids":[clip.to_string()],"by":1});
    run(&mut session, "clip.transpose", args.clone());
    // A watchdog only: correctness depends on the closed gate, never elapsed time.
    ready.recv_timeout(Duration::from_secs(30)).unwrap();
    for _ in 1..12 {
        run(&mut session, "clip.transpose", args.clone());
    }
    session.end_group();
    assert!(session.writes_pending());
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(session.steps().len(), steps + 1);
    assert_eq!(session.project().clip(clip).unwrap().1.transpose(), 12);
    drop(gate);
    session.flush_writes();
    // The first in-flight state and the last queued state, not all twelve.
    assert_eq!(writes.load(Ordering::SeqCst), 2);
    assert!(session.take_write_errors().is_empty());
    drop(_observe);
    drop(session);
    let mut session = Session::open(&dir).unwrap();
    assert_eq!(session.steps().len(), steps + 1);
    assert_eq!(session.project().clip(clip).unwrap().1.transpose(), 12);
    session.undo(1).unwrap();
    assert_eq!(session.project().clip(clip).unwrap().1.transpose(), 0);
}
