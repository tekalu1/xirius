//! PLG-069: a routed pad reaches its bus before the track insert and fader.

use xirius_engine::builtin::{Delay, Gain, Sine};
use xirius_engine::multi::{LayerSpec, MultiProc};
use xirius_engine::processor::{AudioIo, NoteEvent, Processor};
use xirius_engine::{
    BusSpec, MixSpec, Plan, ProcessSetup, RenderMode, TrackSlot, TrackSpec, render_offline,
};

struct SplitSource;

impl Processor for SplitSource {
    fn name(&self) -> &str {
        "split source"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        let frames = io.frames();
        io.clear();
        io.left.fill(0.25);
        io.right.fill(0.25);
        let route = io
            .pad_routes
            .as_deref_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r.plugin == 0 && r.pad == 0)
            .unwrap();
        route.left[..frames].fill(0.5);
        route.right[..frames].fill(0.5);
    }
}

#[test]
fn a_pad_output_reaches_its_bus_before_track_processing() {
    let mut track = TrackSpec::new("kit", Box::new(SplitSource));
    track.inserts.push(Box::new(Gain::new(0.0)));
    track.pad_outputs.push((0, 0, 0));
    let mix = MixSpec {
        tracks: vec![TrackSlot::New(track)],
        buses: vec![BusSpec::new("pad")],
        ..MixSpec::default()
    };
    let plan = Plan::build(
        mix,
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (left, _) = render_offline(plan, 64);
    assert!(left.iter().all(|sample| (*sample - 0.5).abs() < 1e-6));
}

#[test]
fn a_routed_pad_inside_a_multi_instrument_reaches_its_bus() {
    let mut layer = LayerSpec::new(2, Box::new(SplitSource), Vec::new());
    layer.gain = 0.5;
    let mut track = TrackSpec::new("multi kit", Box::new(MultiProc::new(1, vec![layer])));
    track.inserts.push(Box::new(Gain::new(0.0)));
    track.pad_outputs.push((0, 0, 0));
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            buses: vec![BusSpec::new("pad")],
            ..MixSpec::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (left, _) = render_offline(plan, 64);
    assert!(
        left.iter().all(
            |sample| (*sample - 0.5 * 0.5 * xirius_engine::mixer::pan_gains(0.0).0).abs() < 1e-6
        )
    );
}

fn render(
    track_gain: f32,
    track_mute: bool,
    bus_gain: f32,
    bus_mute: bool,
    routed: bool,
) -> Vec<f32> {
    let mut track = TrackSpec::new("kit", Box::new(SplitSource));
    track.gain = track_gain;
    track.mute = track_mute;
    if routed {
        track.pad_outputs.push((0, 0, 0));
    }
    let mut bus = BusSpec::new("pad");
    bus.gain = bus_gain;
    bus.mute = bus_mute;
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            buses: vec![bus],
            ..MixSpec::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    render_offline(plan, 64).0
}

#[test]
fn track_strip_skips_bus_pad_but_mute_reaches_it() {
    let normal = render(1.0, false, 1.0, false, true);
    let track_down = render(0.0, false, 1.0, false, true);
    let bus_down = render(1.0, false, 0.0, false, true);
    let bus_muted = render(1.0, false, 1.0, true, true);
    let track_muted = render(1.0, true, 1.0, false, true);
    assert!(normal.iter().all(|s| (*s - 0.75).abs() < 1e-6));
    assert!(track_down.iter().all(|s| (*s - 0.5).abs() < 1e-6));
    assert!(bus_down.iter().all(|s| (*s - 0.25).abs() < 1e-6));
    assert_eq!(bus_down, bus_muted);
    assert!(track_muted.iter().all(|s| *s == 0.0));
}

struct ClickSplit {
    first: bool,
}

impl Processor for ClickSplit {
    fn name(&self) -> &str {
        "click split"
    }
    fn prepare(&mut self, _: &ProcessSetup) {
        self.first = true;
    }
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
        let route = io.pad_routes.as_deref_mut().unwrap().first_mut().unwrap();
        if self.first {
            io.left[0] = 0.25;
            io.right[0] = 0.25;
            route.left[0] = 0.5;
            route.right[0] = 0.5;
            self.first = false;
        }
    }
}

#[test]
fn a_bus_pad_and_main_pad_meet_on_the_same_frame_with_different_insert_delays() {
    let mut track = TrackSpec::new("kit", Box::new(ClickSplit { first: true }));
    track.pad_outputs.push((0, 0, 0));
    track.inserts.push(Box::new(Delay::new(41)));
    let mut bus = BusSpec::new("pad");
    bus.inserts.push(Box::new(Delay::new(13)));
    let plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(track)],
            buses: vec![bus],
            ..MixSpec::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 64,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (left, right) = render_offline(plan, 128);
    assert_eq!(
        left.iter()
            .enumerate()
            .filter(|(_, x)| **x != 0.0)
            .map(|(i, x)| (i, *x))
            .collect::<Vec<_>>(),
        [(41, 0.75)]
    );
    assert_eq!(right, left);
}

#[test]
fn other_tracks_keep_prefetch_and_parallel_audio_is_identical() {
    let build = |workers| {
        let mut kit = TrackSpec::new("kit", Box::new(SplitSource));
        kit.pad_outputs.push((0, 0, 0));
        kit.prefetch = true;
        let mut tracks = vec![TrackSlot::New(kit)];
        for i in 0..8 {
            let mut other = TrackSpec::new(
                format!("other {i}"),
                Box::new(Sine::new(220.0 + i as f64, 0.1)),
            );
            other.prefetch = true;
            tracks.push(TrackSlot::New(other));
        }
        let mut plan = Plan::build(
            MixSpec {
                tracks,
                buses: vec![BusSpec::new("pad")],
                ..MixSpec::default()
            },
            ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 64,
                mode: RenderMode::Realtime,
            },
            None,
        )
        .unwrap();
        plan.set_workers(workers).unwrap();
        plan
    };
    let (mut serial, mut parallel) = (build(1), build(4));
    assert!(
        parallel.prefetch_status(0).is_none(),
        "routed kit renders in its level"
    );
    assert!(
        parallel.prefetch_status(1).is_some(),
        "unrelated track can prefetch"
    );
    assert_eq!(parallel.workers(), 4);
    for at in (0..256).step_by(64) {
        let (mut a, mut b, mut c, mut d) = ([0.0; 64], [0.0; 64], [0.0; 64], [0.0; 64]);
        serial.process_at(&mut a, &mut b, Some(at));
        parallel.process_at(&mut c, &mut d, Some(at));
        assert_eq!((a, b), (c, d));
    }
}
