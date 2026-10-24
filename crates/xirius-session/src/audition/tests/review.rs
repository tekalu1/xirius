use super::gates::{LetGo, reader};
use super::*;

fn barrier(a: &Audition) {
    let (tx, rx) = mpsc::channel();
    a.tx.send(Msg::Barrier(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
}

#[test]
fn canceled_io_remains_counted_until_it_returns() {
    let d = dir("bounded-review");
    let gate = reader();
    let _release = LetGo(gate.open.clone());
    let a = Audition::with(gate.clone(), Limits::default());
    for n in 0..24 {
        let file = wav(d.path(), &format!("{n}.wav"), 0.1, 48_000);
        a.play(&file, &d.path().join("peaks"), SILENT);
        a.prefetch(vec![]);
    }
    barrier(&a);
    assert!(
        Arc::strong_count(&gate) <= AHEAD + 3,
        "canceled OS calls still own readers: {}",
        Arc::strong_count(&gate)
    );
    gate.open.release();
    until(&a, "the newest pending selection finishes", |n| {
        n.phase == Phase::Ended
    });
    drop(a);
    let end = Instant::now() + Duration::from_secs(20);
    while Arc::strong_count(&gate) != 1 {
        assert!(Instant::now() < end, "readers did not exit");
        std::thread::yield_now();
    }
}

#[test]
fn closing_reaps_blocked_readers_after_the_gate_is_released() {
    let d = dir("close-reader");
    let file = wav(d.path(), "a.wav", 0.1, 48_000);
    let reader = reader();
    let weak = Arc::downgrade(&reader);
    let open = LetGo(reader.open.clone());
    let a = Audition::with(reader.clone(), Limits::default());
    a.play(&file, &d.path().join("peaks"), SILENT);
    reader.open.entered(1);
    drop(a); // This must return while open is still held.
    drop(reader);
    open.0.release();
    let end = Instant::now() + Duration::from_secs(20);
    while weak.strong_count() != 0 {
        assert!(Instant::now() < end);
        std::thread::yield_now();
    }
}
