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
