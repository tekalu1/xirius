//! The splitter (ENG-074): branches of different latency meet at the merge on the same sample,
//! a muted branch is not processed, and the branch values move with `Command::BranchGain`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use xirius_engine::builtin::{Delay, Gain, Sine};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::split::{BranchSpec, Inner, SplitProc};
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, TrackSlot,
    TrackSpec, render_offline,
};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};

/// One sample of `value` at frame `at`, silence elsewhere.
struct Click {
    at: usize,
    value: f32,
    pos: usize,
}

fn click(at: usize, value: f32) -> Box<Click> {
    Box::new(Click { at, value, pos: 0 })
}

impl Processor for Click {
    fn name(&self) -> &str {
        "click"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
        let n = io.frames();
        if (self.pos..self.pos + n).contains(&self.at) {
            io.left[self.at - self.pos] = self.value;
            io.right[self.at - self.pos] = self.value;
        }
        self.pos += n;
    }
}

/// A constant on both sides.
struct Dc(f32);

impl Processor for Dc {
    fn name(&self) -> &str {
        "dc"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.left.fill(self.0);
        io.right.fill(self.0);
    }
}

/// Passes the signal through and counts the blocks it was asked to process.
struct Count(Arc<AtomicUsize>);

impl Processor for Count {
    fn name(&self) -> &str {
        "count"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, _: &mut AudioIo<'_>, _: &[NoteEvent]) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn delay(frames: usize) -> Box<dyn Processor> {
    Box::new(Delay::new(frames))
}

fn gain(g: f32) -> Box<dyn Processor> {
    Box::new(Gain::new(g))
}

fn branch(node: u64, gain: f32, inserts: Vec<Box<dyn Processor>>) -> BranchSpec {
    let mut b = BranchSpec::new(
        node,
        inserts
            .into_iter()
            .enumerate()
            .map(|(i, p)| (node * 100 + i as u64, p))
            .collect(),
    );
    b.gain = gain;
    b
}

fn plan(track: TrackSpec) -> Plan {
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(track)],
        ..MixSpec::default()
    };
    Plan::build(spec, SETUP, None).unwrap()
}

/// The frames that are not zero, with their values.
fn nonzero(buf: &[f32]) -> Vec<(usize, f32)> {
    buf.iter()
        .enumerate()
        .filter(|(_, s)| **s != 0.0)
        .map(|(i, s)| (i, *s))
        .collect()
}

#[test]
fn branches_of_different_latency_meet_on_the_same_sample() {
    // 777 frames is more than a block and not a multiple of it; the second branch is two
    // inserts, the third has none.
    let split = SplitProc::new(
        1,
        vec![
            branch(2, 1.0, vec![delay(777)]),
            branch(3, 0.5, vec![delay(5), delay(30)]),
            branch(4, 0.25, vec![]),
        ],
    );
    let mut t = TrackSpec::new("t", click(10, 1.0));
    t.inserts = vec![Box::new(split)];
    let p = plan(t);
    assert_eq!(p.latency(), 777);
    let (l, r) = render_offline(p, 2000);
    assert_eq!(nonzero(&l), vec![(787, 1.75)]);
    assert_eq!(nonzero(&r), vec![(787, 1.75)]);
}

#[test]
fn a_split_after_a_late_insert_and_beside_a_plain_track_is_compensated() {
    // The splitter's own latency joins the track's: the plain track waits for it.
    let split = SplitProc::new(
        1,
        vec![
            branch(2, 1.0, vec![delay(100)]),
            branch(3, 1.0, vec![delay(3)]),
        ],
    );
    let mut a = TrackSpec::new("split", click(0, 1.0));
    a.inserts = vec![delay(20), Box::new(split)];
    let b = TrackSpec::new("plain", click(0, 10.0));
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(a), TrackSlot::New(b)],
        ..MixSpec::default()
    };
    let p = Plan::build(spec, SETUP, None).unwrap();
    assert_eq!(p.latency(), 120);
    let (l, _) = render_offline(p, 400);
    assert_eq!(nonzero(&l), vec![(120, 12.0)]);
}

#[test]
fn an_inverted_branch_cancels_the_delayed_one_exactly() {
    let split = SplitProc::new(
        1,
        vec![
            branch(2, 1.0, vec![delay(301)]),
            branch(3, 1.0, vec![gain(-1.0)]),
        ],
    );
    let mut t = TrackSpec::new("t", Box::new(Sine::new(997.0, 0.5)));
    t.inserts = vec![Box::new(split)];
    let (l, r) = render_offline(plan(t), 4800);
    assert!(l.iter().all(|&s| s == 0.0), "{:?}", nonzero(&l).first());
    assert!(r.iter().all(|&s| s == 0.0));
}

#[test]
fn nested_splits_line_up_too() {
    let inner = SplitProc::new(
        10,
        vec![branch(11, 1.0, vec![delay(50)]), branch(12, 1.0, vec![])],
    );
    let outer = SplitProc::new(
        1,
        vec![
            BranchSpec::new(2, vec![(20, delay(7)), (10, Box::new(inner))]),
            branch(3, 1.0, vec![]),
        ],
    );
    let mut t = TrackSpec::new("t", click(0, 1.0));
    t.inserts = vec![Box::new(outer)];
    let p = plan(t);
    assert_eq!(p.latency(), 57);
    let (l, _) = render_offline(p, 200);
    assert_eq!(nonzero(&l), vec![(57, 3.0)]);
}

#[test]
fn a_single_neutral_branch_is_bit_identical_to_its_chain() {
    let make = || -> Vec<Box<dyn Processor>> { vec![gain(0.37), delay(13)] };
    let mut plain = TrackSpec::new("t", Box::new(Sine::new(441.0, 0.7)));
    plain.inserts = make();
    let mut split = TrackSpec::new("t", Box::new(Sine::new(441.0, 0.7)));
    split.inserts = vec![Box::new(SplitProc::new(1, vec![branch(2, 1.0, make())]))];
    let a = render_offline(plan(plain), 3000);
    let b = render_offline(plan(split), 3000);
    let bits = |v: &[f32]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&a.0), bits(&b.0));
    assert_eq!(bits(&a.1), bits(&b.1));
}

#[test]
fn a_muted_branch_is_not_processed() {
    let counted = Arc::new(AtomicUsize::new(0));
    let mut muted = branch(2, 1.0, vec![Box::new(Count(counted.clone())), gain(5.0)]);
    muted.mute = true;
    let split = SplitProc::new(1, vec![muted, branch(3, 0.5, vec![])]);
    let mut t = TrackSpec::new("t", click(10, 1.0));
    t.inserts = vec![Box::new(split)];
    let (l, _) = render_offline(plan(t), 1000);
    assert_eq!(counted.load(Ordering::Relaxed), 0);
    assert_eq!(nonzero(&l), vec![(10, 0.5)]);
}

/// A track of a constant 0.5 through `split`, rendered block by block as it comes.
fn engine_with(split: SplitProc) -> (engine::AudioEngine, engine::Controller) {
    engine_from(Box::new(Dc(0.5)), split)
}

/// A track of `source` through `split`, rendered block by block as it comes.
fn engine_from(
    source: Box<dyn Processor>,
    split: SplitProc,
) -> (engine::AudioEngine, engine::Controller) {
    let mut t = TrackSpec::new("t", source);
    t.inserts = vec![Box::new(split)];
    t.prefetch = false;
    let setup = ProcessSetup {
        mode: RenderMode::Realtime,
        ..SETUP
    };
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    engine::create(
        Plan::build(spec, setup, None).unwrap(),
        EngineConfig::default(),
    )
}

/// Renders `blocks` blocks and gives back the left channel.
fn blocks(audio: &mut engine::AudioEngine, blocks: usize) -> Vec<f32> {
    let mut out = vec![0.0; SETUP.max_block * blocks * 2];
    audio.render(&mut out, 2);
    out.iter().step_by(2).copied().collect()
}

#[test]
fn branch_gain_moves_a_branch_and_mute_stops_its_processing() {
    let counted = Arc::new(AtomicUsize::new(0));
    let split = SplitProc::new(
        1,
        vec![
            branch(2, 1.0, vec![Box::new(Count(counted.clone()))]),
            branch(3, 0.0, vec![]),
        ],
    );
    let (mut audio, mut control) = engine_with(split);
    let n = SETUP.max_block;
    let before = blocks(&mut audio, 4);
    assert_eq!(counted.load(Ordering::Relaxed), 4);
    assert!(before.iter().all(|&s| s == 0.5));

    // Half the level, after one block's ramp.
    control
        .send(Command::BranchGain {
            node: 2,
            gain: 0.5,
            pan: 0.0,
            mute: false,
        })
        .ok()
        .unwrap();
    let half = blocks(&mut audio, 4);
    assert!(half[..n].windows(2).all(|w| w[1] <= w[0]));
    assert!(half[n..].iter().all(|&s| s == 0.25));

    // Muted: one more block fades it out, then the branch is skipped.
    control
        .send(Command::BranchGain {
            node: 2,
            gain: 0.5,
            pan: 0.0,
            mute: true,
        })
        .ok()
        .unwrap();
    let count = counted.load(Ordering::Relaxed);
    let silent = blocks(&mut audio, 4);
    assert_eq!(counted.load(Ordering::Relaxed), count + 1);
    assert!(silent[n..].iter().all(|&s| s == 0.0));

    // The other branch, by its own id; the muted one stays skipped.
    control
        .send(Command::BranchGain {
            node: 3,
            gain: 1.0,
            pan: 0.0,
            mute: false,
        })
        .ok()
        .unwrap();
    let other = blocks(&mut audio, 4);
    assert_eq!(counted.load(Ordering::Relaxed), count + 1);
    assert!(other[n..].iter().all(|&s| s == 0.5));

    // An id no splitter has is ignored.
    control
        .send(Command::BranchGain {
            node: 99,
            gain: 0.0,
            pan: 0.0,
            mute: true,
        })
        .ok()
        .unwrap();
    assert!(blocks(&mut audio, 2).iter().all(|&s| s == 0.5));
}

fn mute(control: &mut engine::Controller, node: u64, mute: bool) {
    control
        .send(Command::BranchGain {
            node,
            gain: 1.0,
            pan: 0.0,
            mute,
        })
        .ok()
        .unwrap();
}

/// The largest jump from one sample to the next.
fn largest_step(out: &[f32]) -> f32 {
    out.windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn unmuting_the_delayed_branch_rises_without_a_step() {
    // The empty branch is delayed by 200 frames to meet the other. Unmuted from an empty delay
    // line, it must not come in at full gain 200 frames late.
    let split = SplitProc::new(
        1,
        vec![branch(2, 1.0, vec![delay(200)]), branch(3, 1.0, vec![])],
    );
    let (mut audio, mut control) = engine_with(split);
    let n = SETUP.max_block;
    blocks(&mut audio, 8);
    mute(&mut control, 3, true);
    let muted = blocks(&mut audio, 8);
    assert!(muted[n..].iter().all(|&s| s == 0.5));
    mute(&mut control, 3, false);
    let mut out = vec![*muted.last().unwrap()];
    out.extend(blocks(&mut audio, 16));
    assert_eq!(*out.last().unwrap(), 1.0, "both branches sound again");
    let ramp = 0.5 / n as f32;
    assert!(
        largest_step(&out) <= ramp * 1.001,
        "a step of {} where the ramp is {ramp}",
        largest_step(&out)
    );
}

/// 1.0 until frame `at`, then 0.25.
struct Step {
    at: usize,
    pos: usize,
}

impl Processor for Step {
    fn name(&self) -> &str {
        "step"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for i in 0..io.frames() {
            let v = if self.pos + i < self.at { 1.0 } else { 0.25 };
            io.left[i] = v;
            io.right[i] = v;
        }
        self.pos += io.frames();
    }
}

#[test]
fn an_unmuted_branch_does_not_play_what_it_held_before_the_mute() {
    // The source drops from 1.0 to 0.25 while the delaying branch is muted; what its delay held
    // from before must not be heard when it comes back.
    let split = SplitProc::new(
        1,
        vec![branch(2, 1.0, vec![delay(200)]), branch(3, 0.0, vec![])],
    );
    let n = SETUP.max_block;
    let (mut audio, mut control) = engine_from(Box::new(Step { at: 12 * n, pos: 0 }), split);
    assert_eq!(blocks(&mut audio, 8)[7 * n..], vec![1.0; n][..]);
    mute(&mut control, 2, true);
    blocks(&mut audio, 8);
    mute(&mut control, 2, false);
    let out = blocks(&mut audio, 16);
    assert!(
        out.iter().all(|&s| s <= 0.25),
        "the old sound came back: {}",
        out.iter().fold(0.0f32, |a, &b| a.max(b))
    );
    assert_eq!(*out.last().unwrap(), 0.25);
}

#[test]
fn a_pan_moves_the_branch_between_the_sides() {
    let split = SplitProc::new(1, vec![branch(2, 1.0, vec![])]);
    let (mut audio, mut control) = engine_with(split);
    control
        .send(Command::BranchGain {
            node: 2,
            gain: 1.0,
            pan: -1.0,
            mute: false,
        })
        .ok()
        .unwrap();
    let mut out = vec![0.0; SETUP.max_block * 4 * 2];
    audio.render(&mut out, 2);
    let tail = &out[SETUP.max_block * 2..];
    assert!(tail.iter().skip(1).step_by(2).all(|&r| r.abs() < 1e-6));
    assert!(tail.iter().step_by(2).any(|&l| l != 0.0));
}

#[test]
fn inner_inserts_are_as_late_as_what_comes_before_them_in_their_branch() {
    let inner = SplitProc::new(
        10,
        vec![
            BranchSpec::new(11, vec![(111, delay(4))]),
            BranchSpec::new(12, vec![]),
        ],
    );
    let split = SplitProc::new(
        1,
        vec![
            BranchSpec::new(2, vec![(201, delay(20)), (202, gain(1.0))]),
            BranchSpec::new(3, vec![(301, gain(1.0)), (10, Box::new(inner))]),
        ],
    );
    let mut t = TrackSpec::new("t", click(0, 1.0));
    t.inserts = vec![delay(10), Box::new(split), gain(1.0)];
    let shape = plan(t).shape();
    let at = |id, offset| Inner {
        id,
        insert: 1,
        offset,
    };
    assert_eq!(
        shape.nested.tracks[0],
        vec![at(201, 0), at(202, 20), at(301, 0), at(111, 0)]
    );
    let delay_of = |id| shape.nested.track_delay(&shape.automation, 0, id);
    assert_eq!(delay_of(201), Some(10));
    assert_eq!(delay_of(202), Some(30));
    assert_eq!(delay_of(111), Some(10));
    assert_eq!(delay_of(7), None);
    // The insert after the splitter is as late as its longest branch.
    assert_eq!(shape.automation.tracks[0].insert(2), 30);
}

#[test]
fn a_kept_track_keeps_its_inner_inserts_and_its_branch_values() {
    let split = || SplitProc::new(1, vec![BranchSpec::new(2, vec![(201, delay(20))])]);
    let mut t = TrackSpec::new("t", Box::new(Dc(0.5)));
    t.inserts = vec![Box::new(split())];
    t.prefetch = false;
    let setup = ProcessSetup {
        mode: RenderMode::Realtime,
        ..SETUP
    };
    let first = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(t)],
            ..MixSpec::default()
        },
        setup,
        None,
    )
    .unwrap();
    let shape = first.shape();
    let (mut audio, mut control) = engine::create(first, EngineConfig::default());
    blocks(&mut audio, 2);
    let kept = MixSpec {
        tracks: vec![TrackSlot::Keep(0)],
        ..MixSpec::default()
    };
    let second = Plan::build(kept, setup, Some(shape.clone())).unwrap();
    assert_eq!(second.shape().nested, shape.nested);
    control
        .swap(MixSpec {
            tracks: vec![TrackSlot::Keep(0)],
            ..MixSpec::default()
        })
        .unwrap();
    blocks(&mut audio, 2);
    control.poll(|_| {});
    // The kept node's branch still answers.
    control
        .send(Command::BranchGain {
            node: 2,
            gain: 1.0,
            pan: 0.0,
            mute: true,
        })
        .ok()
        .unwrap();
    let out = blocks(&mut audio, 4);
    assert!(out[SETUP.max_block..].iter().all(|&s| s == 0.0));
}
