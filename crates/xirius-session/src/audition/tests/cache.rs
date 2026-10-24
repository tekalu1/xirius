use super::gates::{LetGo, barrier, reader};
use super::*;
use std::sync::atomic::Ordering::SeqCst;

#[test]
fn cached_sound_does_not_wait_for_stat_and_stale_sound_is_stopped_before_open() {
    let d = dir("optimistic-review");
    let file = wav(d.path(), "a.wav", 2.0, 48_000);
    let gate = reader();
    let _stat = LetGo(gate.stat.clone());
    let _open = LetGo(gate.open.clone());
    gate.open.release();
    let a = Audition::with(gate.clone(), Limits::default());
    a.play(&file, &d.path().join("peaks"), SILENT);
    until(&a, "warm the cache", |n| n.phase == Phase::Ended);
    gate.block_stat.store(true, SeqCst);
    a.play(&file, &d.path().join("peaks"), SILENT);
    gate.stat.entered(1);
    barrier(&a);
    // Stat is held until the test has observed the cached cue. A wall-clock sleep
    // cannot make this pass when the implementation waits for metadata first.
    until(&a, "cached cue while stat is blocked", |n| {
        n.kept && n.phase != Phase::Loading
    });
    a.stop();
    gate.stat.release();
}

fn lead(limits: Limits) -> Lead {
    let (tx, rx) = mpsc::channel();
    let (voice, _) = mpsc::channel();
    Lead {
        rx,
        tx,
        voice,
        shared: Arc::new(Shared {
            now: Mutex::new(Now::default()),
            kept: Mutex::new((vec![], 0)),
            halted: AtomicU64::new(0),
        }),
        reader: Arc::new(Disk),
        limits,
        kept: VecDeque::new(),
        bytes: 0,
        reading: vec![],
        ahead: vec![],
        tried: vec![],
        last: None,
        want: None,
        readers: 0,
    }
}

#[test]
fn cache_limits_count_owned_capacity_including_peaks() {
    let mut left = Vec::with_capacity(4096);
    left.extend([0.5; 4]);
    let mut right = Vec::with_capacity(8192);
    right.extend([0.5; 4]);
    let mut peaks = Peaks::from_samples(48_000, &left, &right);
    peaks.levels[0].reserve(4096);
    let loaded = Loaded {
        stamp: Stamp { size: 100, time: 0 },
        audio: Arc::new(AudioData { left, right }),
        peaks: Arc::new(peaks),
    };
    let mut lead = lead(Limits {
        total: 2048,
        one: 1024,
    });
    lead.keep(Path::new("a.wav"), &loaded, false);
    assert!(
        lead.kept.is_empty(),
        "spare capacity belongs to the cache budget"
    );
}
