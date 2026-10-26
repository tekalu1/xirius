//! Offline, deterministic checks of the mixer: gain, pan, sends, buses, block chopping, events,
//! plan swaps and latency.

use std::f32::consts::FRAC_1_SQRT_2;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use xirius_engine::builtin::{Delay, Gain, Sine, SineSynth};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::mixer::NodeId;
use xirius_engine::{
    AudioIo, BusSpec, Command, EngineEvent, MixSpec, NoteEvent, NoteKind, Output, Plan,
    ProcessSetup, Processor, RenderMode, SendSpec, TrackSlot, TrackSpec, render_offline,
};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};

/// A constant signal, so every expected sample is a simple product.
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

/// Counts frames it has produced and remembers the largest block it was given.
struct Probe {
    frames: Arc<AtomicUsize>,
    largest: Arc<AtomicUsize>,
}

impl Processor for Probe {
    fn name(&self) -> &str {
        "probe"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        let n = io.frames();
        let start = self.frames.fetch_add(n, Ordering::Relaxed);
        self.largest.fetch_max(n, Ordering::Relaxed);
        for i in 0..n {
            io.left[i] = (start + i) as f32;
            io.right[i] = (start + i) as f32;
        }
    }
}

fn dc_track(value: f32) -> TrackSpec {
    TrackSpec::new("dc", Box::new(Dc(value)))
}

fn plan(tracks: Vec<TrackSpec>, buses: Vec<BusSpec>) -> Plan {
    let spec = MixSpec {
        tracks: tracks.into_iter().map(TrackSlot::New).collect(),
        buses,
        ..MixSpec::default()
    };
    Plan::build(spec, SETUP, None).unwrap()
}

fn assert_all(buf: &[f32], want: f32) {
    for (i, &s) in buf.iter().enumerate() {
        assert!((s - want).abs() < 1e-6, "sample {i}: {s} != {want}");
    }
}

#[test]
fn unavailable_tracks_and_buses_stay_silent_after_unmute_live_and_send_changes() {
    let mut failed = dc_track(20.0);
    failed.unavailable = true;
    failed.sends.push(SendSpec {
        bus: 1,
        level: 1.0,
        pre_fader: true,
    });
    let mut routed = dc_track(10.0);
    routed.output = Output::Bus(0);
    let mut failed_bus = BusSpec::new("failed bus");
    failed_bus.unavailable = true;
    // An effect which would generate sound must not run on a failed bus.
    failed_bus.inserts.push(Box::new(Dc(30.0)));
    failed_bus.sends.push(SendSpec {
        bus: 1,
        level: 1.0,
        pre_fader: true,
    });
    let mut p = plan(
        vec![dc_track(0.25), failed, routed],
        vec![failed_bus, BusSpec::new("send")],
    );
    p.set_track_mute(1, false);
    p.set_track_live(1, true);
    p.set_send_level(1, 0, 1.0);
    p.set_bus_mute(0, false);
    p.set_bus_send_level(0, 0, 1.0);
    let (l, r) = render_offline(p, 500);
    assert_all(&l, 0.25);
    assert_all(&r, 0.25);
}

#[test]
fn track_gain_scales_the_signal() {
    let mut t = dc_track(1.0);
    t.gain = 0.5;
    let (l, r) = render_offline(plan(vec![t], vec![]), 500);
    assert_all(&l, 0.5);
    assert_all(&r, 0.5);
}

#[test]
fn tracks_sum_at_the_master_and_master_gain_applies() {
    let spec = MixSpec {
        tracks: vec![
            TrackSlot::New(dc_track(0.25)),
            TrackSlot::New(dc_track(0.5)),
        ],
        master_gain: 2.0,
        ..MixSpec::default()
    };
    let (l, r) = render_offline(Plan::build(spec, SETUP, None).unwrap(), 300);
    assert_all(&l, 1.5);
    assert_all(&r, 1.5);
}

#[test]
fn pan_is_unity_at_centre_and_constant_power_at_the_sides() {
    for (pan, want_l, want_r) in [
        (0.0, 1.0, 1.0),
        (-1.0, 1.0, 0.0),
        (1.0, 0.0, 1.0),
        (0.5, FRAC_1_SQRT_2, 1.0),
        (-0.5, 1.0, FRAC_1_SQRT_2),
    ] {
        let mut t = dc_track(1.0);
        t.pan = pan;
        let (l, r) = render_offline(plan(vec![t], vec![]), 128);
        assert_all(&l, want_l);
        assert_all(&r, want_r);
    }
}

#[test]
fn inserts_run_in_order_before_the_fader() {
    let mut t = dc_track(1.0);
    t.inserts = vec![Box::new(Gain::new(0.5)), Box::new(Gain::new(0.5))];
    t.gain = 2.0;
    let (l, _) = render_offline(plan(vec![t], vec![]), 64);
    assert_all(&l, 0.5);
}

#[test]
fn post_fader_send_follows_fader_and_pan() {
    let mut t = dc_track(1.0);
    t.gain = 0.5;
    t.pan = -1.0;
    t.sends = vec![SendSpec {
        bus: 0,
        level: 0.5,
        pre_fader: false,
    }];
    let (l, r) = render_offline(plan(vec![t], vec![BusSpec::new("fx")]), 200);
    // Direct 0.5 on the left, plus the send 0.5 × 0.5 on the left.
    assert_all(&l, 0.75);
    assert_all(&r, 0.0);
}

#[test]
fn pre_fader_send_ignores_fader_and_pan() {
    let mut t = dc_track(1.0);
    t.gain = 0.0;
    t.pan = -1.0;
    t.sends = vec![SendSpec {
        bus: 0,
        level: 0.5,
        pre_fader: true,
    }];
    let (l, r) = render_offline(plan(vec![t], vec![BusSpec::new("fx")]), 200);
    assert_all(&l, 0.5);
    assert_all(&r, 0.5);
}

#[test]
fn mute_silences_the_track_and_its_sends() {
    let mut t = dc_track(1.0);
    t.mute = true;
    t.sends = vec![
        SendSpec {
            bus: 0,
            level: 1.0,
            pre_fader: true,
        },
        SendSpec {
            bus: 0,
            level: 1.0,
            pre_fader: false,
        },
    ];
    let (l, r) = render_offline(plan(vec![t], vec![BusSpec::new("fx")]), 200);
    assert_all(&l, 0.0);
    assert_all(&r, 0.0);
}

#[test]
fn bus_applies_inserts_gain_and_pan_then_reaches_master() {
    let mut a = dc_track(0.5);
    a.output = Output::Bus(0);
    let mut b = dc_track(0.25);
    b.output = Output::Bus(0);
    let mut bus = BusSpec::new("group");
    bus.inserts = vec![Box::new(Gain::new(2.0))];
    bus.gain = 0.5;
    bus.pan = 1.0;
    let (l, r) = render_offline(plan(vec![a, b], vec![bus]), 200);
    // (0.5 + 0.25) × 2 × 0.5, all on the right.
    assert_all(&l, 0.0);
    assert_all(&r, 0.75);
}

#[test]
fn unknown_bus_is_rejected() {
    let mut t = dc_track(1.0);
    t.output = Output::Bus(3);
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    assert!(Plan::build(spec, SETUP, None).is_err());
}

#[test]
fn device_buffers_are_chopped_to_max_block() {
    let frames = Arc::new(AtomicUsize::new(0));
    let largest = Arc::new(AtomicUsize::new(0));
    let probe = Probe {
        frames: frames.clone(),
        largest: largest.clone(),
    };
    let (mut audio, _c) = engine::create(
        plan(vec![TrackSpec::new("p", Box::new(probe))], vec![]),
        EngineConfig::default(),
    );
    // 1000 frames in one callback (not a multiple of 64) on a 4-channel device.
    let mut out = vec![9.0; 1000 * 4];
    audio.render(&mut out, 4);
    assert_eq!(frames.load(Ordering::Relaxed), 1000);
    assert_eq!(largest.load(Ordering::Relaxed), 64);
    for (i, f) in out.chunks(4).enumerate() {
        assert_eq!(f, [i as f32, i as f32, 0.0, 0.0]);
    }
}

#[test]
fn mono_device_gets_the_average() {
    let mut t = dc_track(1.0);
    t.pan = -1.0;
    let (mut audio, _c) = engine::create(plan(vec![t], vec![]), EngineConfig::default());
    let mut out = vec![0.0; 100];
    audio.render(&mut out, 1);
    assert_all(&out, 0.5);
}

#[test]
fn rendering_is_deterministic() {
    let make = || {
        let mut t = TrackSpec::new("sine", Box::new(Sine::new(440.0, 0.5)));
        t.pan = 0.3;
        t.sends = vec![SendSpec {
            bus: 0,
            level: 0.3,
            pre_fader: false,
        }];
        let mut bus = BusSpec::new("fx");
        bus.inserts = vec![Box::new(Delay::new(37))];
        plan(vec![t], vec![bus])
    };
    let a = render_offline(make(), 4800);
    let b = render_offline(make(), 4800);
    assert_eq!(a, b);
}

#[test]
fn gain_change_ramps_over_one_block_without_a_jump() {
    let (mut audio, mut control) =
        engine::create(plan(vec![dc_track(1.0)], vec![]), EngineConfig::default());
    let mut out = vec![0.0; 64 * 2];
    audio.render(&mut out, 2);
    control
        .send(Command::TrackGain {
            track: 0,
            gain: 0.0,
        })
        .ok()
        .unwrap();
    audio.render(&mut out, 2);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    for w in left.windows(2) {
        assert!(w[0] - w[1] < 0.02, "jump from {} to {}", w[0], w[1]);
    }
    assert!((left[0] - 63.0 / 64.0).abs() < 1e-6);
    assert_eq!(left[63], 0.0);
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
}

#[test]
fn notes_reach_the_instrument() {
    let (mut audio, mut control) = engine::create(
        plan(
            vec![TrackSpec::new("synth", Box::new(SineSynth::new()))],
            vec![],
        ),
        EngineConfig::default(),
    );
    let mut out = vec![0.0; 480 * 2];
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
    control
        .send(Command::Note {
            track: 0,
            kind: NoteKind::On {
                pitch: 69,
                velocity: 1.0,
            },
        })
        .ok()
        .unwrap();
    audio.render(&mut out, 2);
    assert!(out.iter().any(|s| s.abs() > 0.1));
    control
        .send(Command::Note {
            track: 0,
            kind: NoteKind::Off { pitch: 69 },
        })
        .ok()
        .unwrap();
    audio.render(&mut out, 2);
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
}

#[test]
fn plan_swap_keeps_tracks_and_hands_the_old_plan_back() {
    let frames = Arc::new(AtomicUsize::new(0));
    let probe = Probe {
        frames: frames.clone(),
        largest: Arc::new(AtomicUsize::new(0)),
    };
    let (mut audio, mut control) = engine::create(
        plan(vec![TrackSpec::new("p", Box::new(probe))], vec![]),
        EngineConfig::default(),
    );
    let mut out = vec![0.0; 100 * 2];
    audio.render(&mut out, 2);

    // Keep the probe, add a bus and a second track that sends to it.
    let mut extra = dc_track(0.0);
    extra.sends = vec![SendSpec {
        bus: 0,
        level: 1.0,
        pre_fader: false,
    }];
    control
        .swap(MixSpec {
            tracks: vec![TrackSlot::Keep(0), TrackSlot::New(extra)],
            buses: vec![BusSpec::new("fx")],
            ..MixSpec::default()
        })
        .unwrap();
    // Removing a bus while keeping a track is refused.
    assert!(
        control
            .swap(MixSpec {
                tracks: vec![TrackSlot::Keep(0)],
                ..MixSpec::default()
            })
            .is_err()
    );

    audio.render(&mut out, 2);
    // The probe continued from frame 100 instead of restarting.
    assert_eq!(out[0], 100.0);
    assert_eq!(frames.load(Ordering::Relaxed), 200);

    let mut swapped = 0;
    control.poll(|e| {
        if e == EngineEvent::PlanSwapped {
            swapped += 1;
        }
    });
    assert_eq!(swapped, 1);
    assert_eq!(audio.leaked_plans(), 0);
}

#[test]
fn meters_and_position_are_reported() {
    let mut t = dc_track(0.5);
    t.pan = -1.0;
    let (mut audio, mut control) = engine::create(plan(vec![t], vec![]), EngineConfig::default());
    // 20 ms at 48 kHz is 960 frames.
    let mut out = vec![0.0; 960 * 2];
    audio.render(&mut out, 2);
    let mut master = None;
    let mut position = None;
    control.poll(|e| match e {
        EngineEvent::Meter {
            node: NodeId::Master,
            reading,
        } => master = Some(reading),
        EngineEvent::Position(p) => position = Some(p),
        _ => {}
    });
    let m = master.expect("master meter");
    assert!((m.peak[0] - 0.5).abs() < 1e-6 && m.peak[1] < 1e-6);
    assert!((m.rms[0] - 0.5).abs() < 1e-6);
    assert_eq!(position, Some(960));
}

#[test]
fn plan_reports_and_compensates_insert_latency() {
    let mut t = dc_track(1.0);
    t.inserts = vec![Box::new(Delay::new(100))];
    t.sends = vec![SendSpec {
        bus: 0,
        level: 1.0,
        pre_fader: false,
    }];
    let mut bus = BusSpec::new("fx");
    bus.inserts = vec![Box::new(Delay::new(30))];
    let p = plan(vec![t], vec![bus]);
    assert_eq!(p.latency(), 130);
    // The direct path waits for the bus path (ENG-010), so both arrive together.
    let (l, _) = render_offline(p, 300);
    assert_all(&l[..130], 0.0);
    assert_all(&l[130..], 2.0);
}

#[test]
fn silent_output_still_renders_meters_and_taps() {
    let (mut audio, mut control) =
        engine::create(plan(vec![dc_track(0.5)], vec![]), EngineConfig::default());
    let (tx, mut rx) = rtrb::RingBuffer::new(960 * 2);
    audio.set_tap(tx);
    audio.set_silent(true);
    let mut out = vec![1.0; 960 * 2];
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
    let mut master = None;
    control.poll(|e| {
        if let EngineEvent::Meter {
            node: NodeId::Master,
            reading,
        } = e
        {
            master = Some(reading)
        }
    });
    let m = master.expect("master meter");
    assert!((m.rms[0] - 0.5).abs() < 1e-6 && (m.rms[1] - 0.5).abs() < 1e-6);
    let tapped: Vec<f32> = rx.read_chunk(rx.slots()).unwrap().into_iter().collect();
    assert_eq!(tapped.len(), 960 * 2);
    assert_all(&tapped, 0.5);

    audio.set_silent(false);
    audio.render(&mut out, 2);
    assert_all(&out, 0.5);
}

#[test]
fn the_click_is_heard_on_the_device_but_not_tapped_or_metered_and_silent_stays_silent() {
    use xirius_engine::click::Click;
    let (mut audio, mut control) =
        engine::create(plan(vec![dc_track(0.0)], vec![]), EngineConfig::default());
    let (tx, mut rx) = rtrb::RingBuffer::new(4800 * 2);
    audio.set_tap(tx);
    let click = Click::new(48_000.0, vec![(0, 0)], 24_000.0, 4, 0.5);
    control.send(Command::SetClick(Some(Box::new(click)))).ok();
    control.send(Command::Play).ok();
    let mut out = vec![0.0; 960 * 2];
    audio.render(&mut out, 2);
    assert!(
        out.iter().any(|s| s.abs() > 0.1),
        "the click reaches the device"
    );
    let (l, r): (Vec<f32>, Vec<f32>) = out.chunks(2).map(|f| (f[0], f[1])).unzip();
    assert_eq!(l, r);
    let tapped: Vec<f32> = rx.read_chunk(rx.slots()).unwrap().into_iter().collect();
    assert_all(&tapped, 0.0);
    let mut master = None;
    control.poll(|e| {
        if let EngineEvent::Meter {
            node: NodeId::Master,
            reading,
        } = e
        {
            master = Some(reading)
        }
    });
    assert!(master.expect("master meter").rms[0] < 1e-6);

    // Silent output stays silent, click or not.
    control.send(Command::Seek(0)).ok();
    audio.set_silent(true);
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);

    // Off: nothing, and not while stopped either.
    audio.set_silent(false);
    control.send(Command::SetClick(None)).ok();
    control.send(Command::Seek(0)).ok();
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
}

#[test]
fn the_test_tone_plays_left_then_right_on_the_device_only_and_a_silent_output_still_measures_it() {
    use xirius_engine::tone::{self, Tone};
    let (mut audio, mut control) =
        engine::create(plan(vec![dc_track(0.0)], vec![]), EngineConfig::default());
    let (tx, mut rx) = rtrb::RingBuffer::new(48_000 * 4);
    audio.set_tap(tx);
    audio.set_silent(true);
    let stats = control.stats();
    let t = Tone::new(48_000.0);
    let total = t.frames() as usize;
    control.send(Command::SetTone(Some(Box::new(t)))).ok();
    // Each tenth of a second's loudest sample, left and right.
    let mut tenths = Vec::new();
    let mut out = vec![0.0; 4800 * 2];
    for _ in 0..total.div_ceil(4800) + 1 {
        audio.render(&mut out, 2);
        assert_all(&out, 0.0);
        tenths.push(stats.take_output_peak());
    }
    let db = |x: f32| 20.0 * x.log10();
    let level = |p: f32| (db(p) - tone::LEVEL_DB).abs() < 0.1;
    // 0.6 s on the left, 0.15 s of nothing, 0.6 s on the right.
    assert!(
        tenths[..6].iter().all(|p| level(p[0]) && p[1] == 0.0),
        "{tenths:?}"
    );
    assert!(
        tenths[8..13].iter().all(|p| p[0] == 0.0 && level(p[1])),
        "{tenths:?}"
    );
    assert_eq!(*tenths.last().unwrap(), [0.0; 2]);
    assert_eq!(stats.tone_left(), 0);
    // Not tapped (an export or a recording never has it).
    let tapped: Vec<f32> = rx.read_chunk(rx.slots()).unwrap().into_iter().collect();
    assert_all(&tapped, 0.0);

    // A device that is not silent hears it; `None` stops it at once.
    audio.set_silent(false);
    control
        .send(Command::SetTone(Some(Box::new(Tone::new(48_000.0)))))
        .ok();
    audio.render(&mut out, 2);
    assert!(out.chunks(2).any(|f| f[0].abs() > 0.1) && out.chunks(2).all(|f| f[1] == 0.0));
    assert!(stats.tone_left() > 0);
    control.send(Command::SetTone(None)).ok();
    audio.render(&mut out, 2);
    assert_all(&out, 0.0);
    assert_eq!(stats.tone_left(), 0);
}
