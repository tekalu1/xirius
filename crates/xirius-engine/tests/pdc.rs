//! Plugin delay compensation (ENG-010): every path into a merge point arrives equally late,
//! checked sample by sample with fixed input.

use xirius_engine::builtin::{Delay, Gain, Sine};
use xirius_engine::mixer::{Compensation, Output, Route, compensate};
use xirius_engine::{
    AudioIo, BusSpec, MixSpec, NoteEvent, Plan, ProcessSetup, Processor, RenderMode, SendSpec,
    TrackSlot, TrackSpec, render_offline,
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

fn plan(tracks: Vec<TrackSpec>, buses: Vec<BusSpec>, master: Vec<Box<dyn Processor>>) -> Plan {
    let spec = MixSpec {
        tracks: tracks.into_iter().map(TrackSlot::New).collect(),
        buses,
        master_inserts: master,
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

fn send(bus: usize, pre_fader: bool) -> SendSpec {
    SendSpec {
        bus,
        level: 1.0,
        pre_fader,
    }
}

#[test]
fn two_tracks_with_different_latency_meet_at_the_master_together() {
    // 777 frames is more than a block and not a multiple of it.
    let mut a = TrackSpec::new("late", click(10, 1.0));
    a.inserts = vec![Box::new(Delay::new(777))];
    let b = TrackSpec::new("direct", click(10, 0.5));
    let p = plan(vec![a, b], vec![], vec![]);
    assert_eq!(p.latency(), 777);
    let (l, r) = render_offline(p, 2000);
    assert_eq!(nonzero(&l), vec![(787, 1.5)]);
    assert_eq!(nonzero(&r), vec![(787, 1.5)]);
}

#[test]
fn an_inverted_copy_cancels_exactly_after_compensation() {
    // The same sine on two tracks, one delayed by a plugin and one inverted: in phase after the
    // merge, they cancel to exact zero.
    let mut a = TrackSpec::new("delayed", Box::new(Sine::new(997.0, 0.5)));
    a.inserts = vec![Box::new(Delay::new(301))];
    let mut b = TrackSpec::new("inverted", Box::new(Sine::new(997.0, 0.5)));
    b.inserts = vec![Box::new(Gain::new(-1.0))];
    let (l, _) = render_offline(plan(vec![a, b], vec![], vec![]), 4800);
    assert!(l.iter().all(|&s| s == 0.0), "{:?}", nonzero(&l).first());
}

#[test]
fn sends_buses_and_master_inserts_are_compensated() {
    // track 0: 40 frames, out to bus 0 and pre-fader send to bus 1.
    // track 1: 0 frames, out to bus 1 and post-fader send to bus 0.
    // track 2: 5 frames, straight to the master.
    // bus 0: 100 frames, out to bus 1.  bus 1: 7 frames, out to the master.
    // master: 3 frames.
    let mut t0 = TrackSpec::new("t0", click(0, 1.0));
    t0.inserts = vec![Box::new(Delay::new(40))];
    t0.output = Output::Bus(0);
    t0.gain = 0.5;
    t0.sends = vec![send(1, true)];
    let mut t1 = TrackSpec::new("t1", click(0, 10.0));
    t1.output = Output::Bus(1);
    t1.sends = vec![send(0, false)];
    let mut t2 = TrackSpec::new("t2", click(0, 100.0));
    t2.inserts = vec![Box::new(Delay::new(5))];
    let mut b0 = BusSpec::new("b0");
    b0.inserts = vec![Box::new(Delay::new(100))];
    b0.output = Output::Bus(1);
    let mut b1 = BusSpec::new("b1");
    b1.inserts = vec![Box::new(Delay::new(7))];
    let p = plan(
        vec![t0, t1, t2],
        vec![b0, b1],
        vec![Box::new(Delay::new(3))],
    );
    // Longest: t0 → b0 → b1 → master = 40 + 100 + 7 + 3.
    assert_eq!(p.latency(), 150);
    let (tracks, buses) = p.compensation();
    assert_eq!(
        tracks,
        &[
            Compensation {
                output: 0,
                sends: vec![100]
            },
            Compensation {
                output: 140,
                sends: vec![40]
            },
            Compensation {
                output: 142,
                sends: vec![]
            },
        ]
    );
    assert_eq!(
        buses,
        &[
            Compensation {
                output: 0,
                sends: vec![]
            },
            Compensation {
                output: 0,
                sends: vec![]
            },
        ]
    );
    let (l, _) = render_offline(p, 1000);
    // t0 through the fader (0.5) and pre-fader (1.0), t1 twice (direct and via the send), t2.
    assert_eq!(nonzero(&l), vec![(150, 0.5 + 1.0 + 10.0 + 10.0 + 100.0)]);
}

#[test]
fn compensate_handles_a_bus_that_nothing_feeds() {
    let tracks = [Route {
        latency: 12,
        output: Output::Master,
        sends: vec![],
    }];
    let buses = [Route {
        latency: 30,
        output: Output::Master,
        sends: vec![],
    }];
    let (t, b, total) = compensate(&tracks, &buses, 0);
    // The empty bus is 30 late on its own, so the track waits for it.
    assert_eq!(total, 30);
    assert_eq!(t[0].output, 18);
    assert_eq!(b[0].output, 0);
}

#[test]
fn a_kept_track_keeps_its_latency_in_the_next_plan() {
    let mut a = TrackSpec::new("late", click(0, 1.0));
    a.inserts = vec![Box::new(Delay::new(50))];
    let first = plan(vec![a], vec![], vec![]);
    let spec = MixSpec {
        tracks: vec![
            TrackSlot::Keep(0),
            TrackSlot::New(TrackSpec::new("new", click(0, 1.0))),
        ],
        ..MixSpec::default()
    };
    let next = Plan::build(spec, SETUP, Some(first.shape())).unwrap();
    assert_eq!(next.latency(), 50);
    assert_eq!(next.compensation().0[1].output, 50);
    assert_eq!(next.compensation().0[0].output, 0);
}
