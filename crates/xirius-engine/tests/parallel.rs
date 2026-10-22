//! Joined track workers preserve serial routing and improve independent DSP (ENG-014).
use std::time::{Duration, Instant};
use xirius_engine::builtin::{Delay, Sine};
use xirius_engine::{
    AudioIo, BusSpec, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, SendSpec,
    TrackSlot, TrackSpec,
};

fn setup() -> ProcessSetup {
    ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Offline,
    }
}

fn plan(workers: usize) -> Plan {
    let tracks = (0..64)
        .map(|i| {
            let mut t = TrackSpec::new(
                format!("track {i}"),
                Box::new(Sine::new(100.0 + i as f64 * 7.0, 0.01)),
            );
            t.inserts.push(Box::new(Delay::new(i * 3)));
            t.gain = 0.5;
            t.pan = (i as f32 - 32.0) / 32.0;
            t.sends.push(SendSpec {
                bus: 0,
                level: 0.1,
                pre_fader: i % 2 == 0,
            });
            TrackSlot::New(t)
        })
        .collect();
    let mut plan = Plan::build(
        MixSpec {
            tracks,
            buses: vec![BusSpec::new("send")],
            ..Default::default()
        },
        setup(),
        None,
    )
    .unwrap();
    plan.set_workers(workers).unwrap();
    plan
}

#[test]
fn eight_workers_join_in_the_same_order_and_preserve_delay_compensation() {
    let mut serial = plan(1);
    let mut parallel = plan(8);
    assert_eq!(parallel.workers(), 8);
    assert_eq!(serial.latency(), parallel.latency());
    for block in 0..50 {
        let n = if block % 2 == 0 { 256 } else { 137 };
        let (mut a, mut b, mut c, mut d) = ([0.0; 256], [0.0; 256], [0.0; 256], [0.0; 256]);
        serial.process_at(&mut a[..n], &mut b[..n], Some(block * 256));
        parallel.process_at(&mut c[..n], &mut d[..n], Some(block * 256));
        assert_eq!(a, c);
        assert_eq!(b, d);
    }
}

#[test]
fn a_processor_panic_joins_every_range_and_poisoned_workers_are_not_reused() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct TestProc {
        fails: bool,
        done: Arc<AtomicUsize>,
    }
    impl Processor for TestProc {
        fn name(&self) -> &str {
            "join test"
        }
        fn prepare(&mut self, _: &ProcessSetup) {}
        fn process(&mut self, _: &mut AudioIo<'_>, _: &[NoteEvent]) {
            assert!(!self.fails, "processor failure");
            std::thread::sleep(Duration::from_millis(2));
            self.done.fetch_add(1, Ordering::Relaxed);
        }
    }
    for failed_track in [0, 7] {
        let done = Arc::new(AtomicUsize::new(0));
        let mut plan = Plan::build(
            MixSpec {
                tracks: (0..8)
                    .map(|i| {
                        TrackSlot::New(TrackSpec::new(
                            i.to_string(),
                            Box::new(TestProc {
                                fails: i == failed_track,
                                done: done.clone(),
                            }),
                        ))
                    })
                    .collect(),
                ..Default::default()
            },
            setup(),
            None,
        )
        .unwrap();
        plan.set_workers(8).unwrap();
        let (mut l, mut r) = ([0.0; 256], [0.0; 256]);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.process(&mut l, &mut r)))
                .is_err()
        );
        assert_eq!(
            done.load(Ordering::Relaxed),
            7,
            "all other ranges joined before unwinding"
        );
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| plan.process(&mut l, &mut r)))
                .is_err()
        );
        assert_eq!(
            done.load(Ordering::Relaxed),
            7,
            "a poisoned pool never starts another block"
        );
        drop(plan);
    }
}

struct Heavy;
impl Processor for Heavy {
    fn name(&self) -> &str {
        "independent DSP"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for (i, (l, r)) in io.left.iter_mut().zip(io.right.iter_mut()).enumerate() {
            let mut value = i as f64 * 0.01;
            for _ in 0..128 {
                value = std::hint::black_box(value.sin() + 0.1);
            }
            *l = value as f32;
            *r = *l;
        }
    }
}

fn elapsed(workers: usize) -> Duration {
    let mut p = Plan::build(
        MixSpec {
            tracks: (0..64)
                .map(|i| TrackSlot::New(TrackSpec::new(i.to_string(), Box::new(Heavy))))
                .collect(),
            ..Default::default()
        },
        setup(),
        None,
    )
    .unwrap();
    p.set_workers(workers).unwrap();
    let (mut l, mut r) = ([0.0; 256], [0.0; 256]);
    p.process(&mut l, &mut r);
    let started = Instant::now();
    for _ in 0..8 {
        p.process(&mut l, &mut r);
    }
    started.elapsed()
}

#[test]
#[ignore = "throughput benchmark; run separately on a machine with eight available cores"]
fn sixty_four_independent_tracks_render_faster_on_eight_workers() {
    let serial = elapsed(1).min(elapsed(1));
    let parallel = elapsed(8).min(elapsed(8));
    eprintln!("64 tracks: one worker {serial:?}, eight workers {parallel:?}");
    assert!(
        parallel < serial,
        "eight workers did not improve throughput"
    );
}
