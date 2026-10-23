//! A song without a splitter sounds as it did before splitters came in (ENG-074): the same mix,
//! rendered offline and live with values moving while it plays, gives the same bits. The hashes
//! were taken from the engine before the splitter was added.

use std::sync::Arc;

use xirius_engine::builtin::{Delay, Gain, Sine};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{
    AudioData, BusSpec, Command, EnvPoint, MixSpec, Output, Plan, ProcessSetup, Region, RenderMode,
    SendSpec, Shape, Timeline, TrackSlot, TrackSpec, render_offline,
};

const FRAMES: usize = 64 * 150;

fn setup(mode: RenderMode) -> ProcessSetup {
    ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 64,
        mode,
    }
}

/// Two tracks of different latency, one playing a region with fades and a gain line, a bus
/// fed by a pre-fader and a post-fader send with a late insert of its own, and the master's.
fn spec() -> MixSpec {
    let mut a = TrackSpec::new("a", Box::new(Sine::new(220.0, 0.4)));
    a.inserts = vec![Box::new(Gain::new(0.8)), Box::new(Delay::new(300))];
    a.pan = -0.3;
    a.sends = vec![SendSpec {
        bus: 0,
        level: 0.5,
        pre_fader: false,
    }];
    a.prefetch = false;

    let data: Vec<f32> = (0..4000)
        .map(|i| ((i as f32) * 0.013).sin() * 0.6)
        .collect();
    let region = Region {
        start: 500,
        len: 3500,
        offset: 200,
        gain: 0.9,
        data: Arc::new(AudioData {
            left: data.clone(),
            right: data.iter().map(|s| -s * 0.5).collect(),
        }),
        shape: Shape {
            fade_in: 400,
            fade_in_exp: 0.5,
            fade_out: 700,
            fade_out_exp: 1.0,
            envelope: vec![
                EnvPoint {
                    frame: 0,
                    gain: 1.0,
                },
                EnvPoint {
                    frame: 1500,
                    gain: 0.25,
                },
                EnvPoint {
                    frame: 3000,
                    gain: 0.7,
                },
            ],
        },
    };
    let mut b = TrackSpec::new("b", Box::new(Sine::new(331.0, 0.0)));
    b.timeline = Timeline::new(Vec::new(), vec![region]);
    b.inserts = vec![Box::new(Delay::new(37))];
    b.gain = 0.7;
    b.pan = 0.6;
    b.sends = vec![SendSpec {
        bus: 0,
        level: 0.8,
        pre_fader: true,
    }];
    b.prefetch = false;

    let mut bus = BusSpec::new("bus");
    bus.inserts = vec![Box::new(Delay::new(129)), Box::new(Gain::new(0.6))];
    bus.pan = 0.2;
    bus.output = Output::Master;
    MixSpec {
        tracks: vec![TrackSlot::New(a), TrackSlot::New(b)],
        buses: vec![bus],
        master_inserts: vec![Box::new(Gain::new(0.9)), Box::new(Delay::new(3))],
        master_gain: 0.8,
        ..MixSpec::default()
    }
}

/// What changes before a block of the live render.
fn moves(block: usize) -> Vec<Command> {
    match block {
        0 => vec![Command::Play],
        20 => vec![
            Command::TrackGain {
                track: 0,
                gain: 0.5,
            },
            Command::SendLevel {
                track: 1,
                send: 0,
                level: 0.3,
            },
        ],
        60 => vec![
            Command::TrackMute {
                track: 1,
                mute: true,
            },
            Command::BusPan { bus: 0, pan: -0.5 },
        ],
        100 => vec![
            Command::TrackMute {
                track: 1,
                mute: false,
            },
            Command::MasterGain(0.6),
        ],
        _ => Vec::new(),
    }
}

/// FNV-1a over the bits of every sample, left then right.
fn hash(l: &[f32], r: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in l.iter().chain(r) {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[test]
fn an_offline_render_without_a_splitter_keeps_its_bits() {
    let p = Plan::build(spec(), setup(RenderMode::Offline), None).unwrap();
    let (l, r) = render_offline(p, FRAMES);
    assert!(l.iter().any(|s| *s != 0.0));
    assert_eq!(hash(&l, &r), 0x0c88_66dc_9f83_7268, "{:#x}", hash(&l, &r));
}

#[test]
fn a_live_render_without_a_splitter_keeps_its_bits() {
    let p = Plan::build(spec(), setup(RenderMode::Realtime), None).unwrap();
    let (mut audio, mut control) = engine::create(p, EngineConfig::default());
    let mut l = Vec::with_capacity(FRAMES);
    let mut r = Vec::with_capacity(FRAMES);
    let mut out = vec![0.0; 64 * 2];
    for block in 0..FRAMES / 64 {
        for c in moves(block) {
            control.send(c).ok().unwrap();
        }
        audio.render(&mut out, 2);
        l.extend(out.iter().step_by(2));
        r.extend(out.iter().skip(1).step_by(2));
    }
    assert!(l.iter().any(|s| *s != 0.0));
    assert_eq!(hash(&l, &r), 0x9720_2473_1a83_1079, "{:#x}", hash(&l, &r));
}
