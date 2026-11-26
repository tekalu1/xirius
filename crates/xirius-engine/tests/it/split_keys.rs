//! ENG-079: a plug-in inside a splitter's branch hears its sidechain key, lined up with the
//! main signal at that plug-in: the key is aimed at the time the branch's inserts before it
//! put the main signal at, and when a source is slower the whole splitter waits, so the
//! branches stay lined up with each other.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use xirius_engine::builtin::Delay;
use xirius_engine::mixer::PlanError;
use xirius_engine::split::{BranchSpec, SplitProc};
use xirius_engine::{
    AudioIo, KeyFrom, KeySourceSpec, KeySpec, MixSpec, NoteEvent, Plan, ProcessSetup, Processor,
    RenderMode, TrackSlot, TrackSpec,
};

fn setup() -> ProcessSetup {
    ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Offline,
    }
}

/// A source that writes its own running frame count, so a delay shows as a shift of the value.
struct Ramp(u64);

impl Processor for Ramp {
    fn name(&self) -> &str {
        "ramp"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        for (i, (l, r)) in io.left.iter_mut().zip(io.right.iter_mut()).enumerate() {
            *l = (self.0 + i as u64) as f32;
            *r = *l;
        }
        self.0 += io.left.len() as u64;
    }
}

/// Puts the key on the right channel and keeps the main signal on the left; the flag records
/// that a key arrived.
struct KeyPeek(Arc<AtomicBool>);

impl Processor for KeyPeek {
    fn name(&self) -> &str {
        "keypeek"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        match io.key {
            Some((_, kr)) => {
                self.0.store(true, Ordering::Relaxed);
                io.right.copy_from_slice(kr);
            }
            None => {
                let left = io.left.to_vec();
                io.right.copy_from_slice(&left);
            }
        }
    }
}

fn delay(id: u64, frames: usize) -> (u64, Box<dyn Processor>) {
    (id, Box::new(Delay::new(frames)))
}

fn peek(id: u64) -> ((u64, Box<dyn Processor>), Arc<AtomicBool>) {
    let had = Arc::new(AtomicBool::new(false));
    ((id, Box::new(KeyPeek(had.clone()))), had)
}

/// A key source: the ramp through `lag` frames of inserts, kept out of the mix.
fn source(name: &str, lag: usize) -> TrackSpec {
    let mut t = TrackSpec::new(name, Box::new(Ramp(0)));
    t.inserts = vec![Box::new(Delay::new(lag))];
    t.gain = 0.0;
    t
}

fn key(insert: usize, plugin: u64, from: usize, id: u64) -> KeySpec {
    KeySpec {
        insert,
        plugin,
        on: true,
        sources: vec![KeySourceSpec {
            id,
            from: KeyFrom::Track(from),
            pre: true,
            level: 1.0,
        }],
    }
}

fn render(spec: MixSpec, blocks: usize) -> (Vec<f32>, Vec<f32>) {
    let block = setup().max_block;
    let mut plan = Plan::build(spec, setup(), None).unwrap();
    let mut l = vec![0.0; blocks * block];
    let mut r = vec![0.0; blocks * block];
    for b in 0..blocks {
        let at = b * block;
        plan.process_at(
            &mut l[at..at + block],
            &mut r[at..at + block],
            Some(at as u64),
        );
    }
    (l, r)
}

/// The receiver: the ramp, 10 frames of insert, then a splitter whose first branch has 20
/// frames before the keyed plug-in and whose second (muted, so only the first is heard) is 50
/// long.
fn receiver(peeked: (u64, Box<dyn Processor>)) -> TrackSpec {
    let mut quiet = BranchSpec::new(2, vec![delay(21, 50)]);
    quiet.mute = true;
    let split = SplitProc::new(
        100,
        vec![
            BranchSpec::new(1, vec![delay(11, 20), peeked, delay(12, 5)]),
            quiet,
        ],
    );
    let mut t = TrackSpec::new("r", Box::new(Ramp(0)));
    t.inserts = vec![Box::new(Delay::new(10)), Box::new(split)];
    t
}

#[test]
fn a_plug_in_in_a_branch_hears_its_key_with_the_main_signal() {
    // A fast source (5 frames) and a slow one (200): with the plug-in 30 frames into the
    // receiver, the first key waits for the main signal and the second makes it wait.
    for lag in [5, 200] {
        let (p, had) = peek(7);
        let mut r = receiver(p);
        r.keys = vec![key(1, 7, 0, 1)];
        let spec = MixSpec {
            tracks: vec![TrackSlot::New(source("s", lag)), TrackSlot::New(r)],
            ..MixSpec::default()
        };
        let (l, out_r) = render(spec, 8);
        assert!(had.load(Ordering::Relaxed), "lag {lag}: the key arrived");
        // The key is the source's ramp and the main signal the receiver's: lined up, they
        // hold the same frame count, so the channels match sample for sample.
        for i in 0..l.len() {
            assert_eq!(out_r[i], l[i], "lag {lag}, sample {i}");
        }
        // The receiver's own path: 10, then the splitter's 50 (its longest branch); a slow
        // source adds what the plug-in at 30 had to wait for it.
        let path = 60 + 200usize.saturating_sub(30) * usize::from(lag == 200);
        assert_eq!(l[path], 0.0, "lag {lag}");
        assert_eq!(l[path + 1], 1.0, "lag {lag}");
    }
}

#[test]
fn plug_ins_in_two_branches_each_hear_their_own_key() {
    let (a, had_a) = peek(7);
    let (b, had_b) = peek(8);
    let split = SplitProc::new(
        100,
        vec![
            BranchSpec::new(1, vec![delay(11, 20), a]),
            BranchSpec::new(2, vec![b, delay(22, 64)]),
        ],
    );
    let mut r = TrackSpec::new("r", Box::new(Ramp(0)));
    r.inserts = vec![Box::new(split)];
    r.keys = vec![key(0, 7, 0, 1), key(0, 8, 1, 2)];
    let spec = MixSpec {
        tracks: vec![
            TrackSlot::New(source("a", 3)),
            TrackSlot::New(source("b", 90)),
            TrackSlot::New(r),
        ],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 8);
    assert!(had_a.load(Ordering::Relaxed) && had_b.load(Ordering::Relaxed));
    // Each branch's right channel is its key and its left its main signal; both lined up, the
    // sums match.
    for i in 0..l.len() {
        assert_eq!(out_r[i], l[i], "sample {i}");
    }
    // b's 90 makes the splitter wait 90 (its plug-in is first in its branch), then the 64.
    assert_eq!(l[154], 0.0);
    assert_eq!(l[155], 2.0);
}

#[test]
fn a_key_on_a_splitter_must_name_a_plug_in_inside() {
    let (p, _) = peek(7);
    let mut r = receiver(p);
    r.keys = vec![key(1, 99, 0, 1)];
    let err = match Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(source("s", 5)), TrackSlot::New(r)],
            ..MixSpec::default()
        },
        setup(),
        None,
    ) {
        Ok(_) => panic!("built a key for a plug-in that is not there"),
        Err(e) => e,
    };
    assert_eq!(err, PlanError::KeyInsert { node: 1, insert: 1 });
}
