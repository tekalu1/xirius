//! The audio-thread path must not touch the allocator (ARC-003, ENG-004). This binary installs
//! assert_no_alloc's allocator, which counts allocations made inside `AudioEngine::render`.

use std::sync::Arc;

use assert_no_alloc::{AllocDisabler, reset_violation_count, violation_count};
use xirius_engine::builtin::{Delay, Gain, Sine, SineSynth};
use xirius_engine::click::Click;
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{
    AudioData, BusSpec, CTRL_PITCH_BEND, Command, KeyFrom, KeySourceSpec, KeySpec, MixSpec,
    NoteKind, Plan, ProcessSetup, Region, RenderMode, SendSpec, TimedNote, Timeline, TrackSlot,
    TrackSpec,
};

#[global_allocator]
static ALLOC: AllocDisabler = AllocDisabler;

fn spec() -> MixSpec {
    let mut sine = TrackSpec::new("sine", Box::new(Sine::new(220.0, 0.3)));
    sine.inserts = vec![Box::new(Gain::new(0.8))];
    sine.sends = vec![SendSpec {
        bus: 0,
        level: 0.4,
        pre_fader: true,
    }];
    let mut synth = TrackSpec::new("synth", Box::new(SineSynth::new()));
    synth.sends = vec![SendSpec {
        bus: 0,
        level: 0.4,
        pre_fader: false,
    }];
    let mut bus = BusSpec::new("fx");
    bus.inserts = vec![Box::new(Delay::new(4800))];
    let mut keyed = TrackSpec::new("keyed", Box::new(Sine::new(330.0, 0.2)));
    keyed.inserts = vec![Box::new(Gain::new(0.9))];
    keyed.keys = vec![KeySpec {
        insert: 0,
        plugin: 9001,
        on: true,
        sources: vec![
            KeySourceSpec {
                id: 9101,
                from: KeyFrom::Track(0),
                pre: true,
                level: 0.6,
            },
            KeySourceSpec {
                id: 9102,
                from: KeyFrom::Bus(0),
                pre: false,
                level: 0.3,
            },
        ],
    }];
    MixSpec {
        tracks: vec![
            TrackSlot::New(sine),
            TrackSlot::New(synth),
            TrackSlot::New(keyed),
        ],
        buses: vec![bus],
        ..MixSpec::default()
    }
}

#[test]
fn render_commands_and_plan_swap_do_not_allocate() {
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let (mut audio, mut control) = engine::create(
        Plan::build(spec(), setup, None).unwrap(),
        EngineConfig::default(),
    );
    let mut out = vec![0.0f32; 1000 * 2];
    reset_violation_count();
    for i in 0..200u32 {
        control
            .send(Command::TrackGain {
                track: 0,
                gain: (i % 10) as f32 / 10.0,
            })
            .ok()
            .unwrap();
        control
            .send(Command::Note {
                track: 1,
                kind: if i % 2 == 0 {
                    NoteKind::On {
                        pitch: 60 + (i % 12) as u8,
                        velocity: 0.8,
                    }
                } else {
                    NoteKind::Off {
                        pitch: 60 + (i % 12) as u8,
                    }
                },
            })
            .ok()
            .unwrap();
        if i % 50 == 25 {
            let mut s = spec();
            s.tracks[0] = TrackSlot::Keep(0);
            control.swap(s).unwrap();
        }
        control
            .send(Command::KeyOn {
                plugin: 9001,
                on: i % 20 < 10,
            })
            .ok()
            .unwrap();
        control
            .send(Command::KeyLevel {
                send: 9101,
                level: (i % 8) as f32 / 8.0,
            })
            .ok()
            .unwrap();
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    assert_eq!(violation_count(), 0);
    assert!(out.iter().any(|s| *s != 0.0));
}

fn timeline(shift: u64) -> Timeline {
    let notes = (0..64u64)
        .flat_map(|i| {
            let pitch = 48 + (i % 24) as u8;
            [
                TimedNote {
                    frame: shift + i * 700,
                    kind: NoteKind::On {
                        pitch,
                        velocity: 0.7,
                    },
                },
                TimedNote {
                    frame: shift + i * 700 + 300,
                    kind: NoteKind::Off { pitch },
                },
            ]
        })
        .collect();
    let data = Arc::new(AudioData {
        left: vec![0.1; 20_000],
        right: vec![0.1; 20_000],
    });
    let region = Region {
        start: 1000,
        len: 15_000,
        offset: 100,
        gain: 0.5,
        data,
        shape: Default::default(),
    };
    Timeline::new(notes, vec![region])
}

#[test]
fn transport_loop_seek_and_timeline_swaps_do_not_allocate() {
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let mut s = spec();
    if let TrackSlot::New(t) = &mut s.tracks[1] {
        t.timeline = timeline(0);
    }
    let (mut audio, mut control) = engine::create(
        Plan::build(s, setup, None).unwrap(),
        EngineConfig::default(),
    );
    let mut out = vec![0.0f32; 1000 * 2];
    control
        .send(Command::SetLoop(Some((2_000, 9_000))))
        .ok()
        .unwrap();
    control.send(Command::Play).ok().unwrap();
    reset_violation_count();
    for i in 0..300u64 {
        match i % 40 {
            10 => control.send(Command::Seek(i * 37)).ok().unwrap(),
            20 => control
                .send(Command::SetTimeline {
                    track: 1,
                    timeline: Box::new(timeline(i)),
                })
                .ok()
                .unwrap(),
            25 => control.send(Command::Stop).ok().unwrap(),
            26 => control.send(Command::Play).ok().unwrap(),
            30 => control
                .send(Command::BusMute {
                    bus: 0,
                    mute: i % 80 == 30,
                })
                .ok()
                .unwrap(),
            // The metronome on and off; the old one goes back through the garbage queue.
            35 => {
                let click = (i % 80 == 35)
                    .then(|| Box::new(Click::new(48_000.0, vec![(0, 0)], 3_000.0, 4, 0.5)));
                control.send(Command::SetClick(click)).ok().unwrap()
            }
            _ => {}
        }
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    assert_eq!(violation_count(), 0);
    assert_eq!(audio.leaked_plans(), 0);
    assert!(audio.is_playing());
}

/// Every built-in with a processor (PLG-016): the instruments on tracks of their own, the
/// effects in a row on a track that always sounds, while notes play and every value moves.
#[test]
fn the_builtins_do_not_allocate_while_their_values_move() {
    use xirius_engine::stock;
    use xirius_params::{BUILTINS, Kind};
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let mut handles = Vec::new();
    let mut tracks = Vec::new();
    let mut effects = Vec::new();
    for def in BUILTINS {
        let Some((p, h)) = stock::create(def.uid) else {
            continue;
        };
        handles.push(h);
        match def.kind {
            Kind::Instrument => tracks.push(TrackSlot::New(TrackSpec::new(def.name, p))),
            Kind::Effect => effects.push(p),
        }
    }
    assert!(!handles.is_empty());
    let mut fx = TrackSpec::new("fx", Box::new(Sine::new(220.0, 0.5)));
    fx.inserts = effects;
    tracks.push(TrackSlot::New(fx));
    let n = tracks.len();
    let spec = MixSpec {
        tracks,
        ..MixSpec::default()
    };
    let (mut audio, mut control) = engine::create(
        Plan::build(spec, setup, None).unwrap(),
        EngineConfig::default(),
    );
    let mut out = vec![0.0f32; 1000 * 2];
    reset_violation_count();
    for i in 0..400u32 {
        if i % 5 == 0 {
            for h in &handles {
                for (k, p) in h.def().params.iter().enumerate() {
                    let v = f64::from((i / 5 + k as u32 * 3) % 11) / 10.0;
                    assert!(h.set(p.id, v));
                }
                h.set_tempo(80.0 + f64::from(i % 90));
            }
        }
        for track in 0..n {
            let pitch = 36 + ((i + track as u32 * 5) % 48) as u8;
            let kind = if i % 3 == 2 {
                NoteKind::Off { pitch }
            } else {
                NoteKind::On {
                    pitch,
                    velocity: 0.3 + (i % 7) as f32 / 10.0,
                }
            };
            control.send(Command::Note { track, kind }).ok().unwrap();
            // Bend, the sustain pedal going down and up, and the modulation wheel.
            let moves = [
                NoteKind::control(CTRL_PITCH_BEND, (i as i32 * 977) % 16_384 - 8_192),
                NoteKind::control(64, if i % 7 < 4 { 127 } else { 0 }),
                NoteKind::control(1, (i as i32 * 13) % 128),
            ];
            let kind = moves[(i as usize + track) % moves.len()];
            control.send(Command::Note { track, kind }).ok().unwrap();
        }
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    assert_eq!(violation_count(), 0);
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(out.iter().any(|s| *s != 0.0));
}

#[test]
fn the_check_would_catch_an_allocation() {
    reset_violation_count();
    let v = assert_no_alloc::assert_no_alloc(|| vec![0u8; 16]);
    assert_eq!(v.len(), 16);
    assert!(violation_count() > 0);
}

#[test]
fn recording_audio_and_midi_does_not_allocate() {
    use xirius_engine::record::{AudioLane, MockInput, MockMidi, RecordSpec, Recorder, Thru};
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let (mut audio, mut control) = engine::create(
        Plan::build(spec(), setup, None).unwrap(),
        EngineConfig::default(),
    );
    let tmp = tempfile::Builder::new()
        .prefix("xirius-noalloc-rec-")
        .tempdir()
        .unwrap();
    let dir = tmp.path().to_path_buf();
    let files = dir.clone();
    let mono = dir.clone();
    let (rec, capture) = Recorder::start(RecordSpec {
        sample_rate: 48_000,
        lanes: vec![
            AudioLane {
                channels: vec![0, 1],
                path: Box::new(move |k| files.join(format!("{k}.wav"))),
            },
            AudioLane {
                channels: vec![1],
                path: Box::new(move |k| mono.join(format!("{k}-mono.wav"))),
            },
        ],
        midi: true,
        latency: 300,
        midi_latency: 300,
        align: Box::new(|f| f.div_ceil(25) * 25),
        ring_seconds: 1.0,
        thru: vec![
            Thru {
                track: 1,
                port: None,
                transpose: 0,
                velocity: 0,
            },
            Thru {
                track: 0,
                port: Some(0),
                transpose: 0,
                velocity: 0,
            },
        ],
    })
    .unwrap();
    let events = (0..400u64)
        .map(|i| (i * 500, [if i % 2 == 0 { 0x90 } else { 0x80 }, 60, 90]))
        .collect();
    control
        .send(Command::SetInput(Some(Box::new(MockInput::loopback(
            300, 256,
        )))))
        .ok()
        .unwrap();
    control
        .send(Command::SetMidiInput(Some(Box::new(MockMidi::new(events)))))
        .ok()
        .unwrap();
    control
        .send(Command::SetLoop(Some((0, 30_000))))
        .ok()
        .unwrap();
    control.send(Command::Play).ok().unwrap();
    let mut out = vec![0.0f32; 1000 * 2];
    reset_violation_count();
    control.send(Command::Record(Some(capture))).ok().unwrap();
    for i in 0..200u64 {
        if i == 150 {
            control.send(Command::Stop).ok().unwrap();
            control.send(Command::Record(None)).ok().unwrap();
        }
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    assert_eq!(violation_count(), 0);
    assert!(!audio.is_recording());
    let r = rec.finish(std::time::Duration::from_secs(5));
    assert!(r.detached && r.errors.is_empty(), "{:?}", r.errors);
    assert!(r.takes.len() > 1, "the loop made laps");
    assert!(r.takes.iter().any(|t| !t.midi.is_empty()));
}

/// The files a built-in reads move through the ring without allocating on the audio thread
/// (ENG-077): polling a new set, playing its slots and handing the old set back.
#[test]
fn stock_devices_taking_file_sets_do_not_allocate() {
    use xirius_engine::stock;
    use xirius_engine::stock::files::{FileSet, FileSlot};
    let setup = ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Realtime,
    };
    let (sampler, h_sampler) = stock::create("sampler").unwrap();
    let (kit, h_kit) = stock::create("drum-kit").unwrap();
    let mut s = spec();
    s.tracks.clear();
    let mut a = TrackSpec::new("smp", sampler);
    a.timeline = timeline(0);
    let mut b = TrackSpec::new("kit", kit);
    b.timeline = timeline(1_000_000);
    s.tracks = vec![TrackSlot::New(a), TrackSlot::New(b)];
    let (mut audio, mut control) = engine::create(
        Plan::build(s, setup, None).unwrap(),
        EngineConfig::default(),
    );
    // The sets handed over, made before the counted part: both channels filled, one slot
    // with a file each.
    let set = |hz: f64| {
        let n = 4_800;
        FileSet {
            slots: vec![Some(FileSlot {
                name: "tone_C3.wav".into(),
                data: Arc::new(AudioData {
                    left: (0..n)
                        .map(|i| (i as f64 * hz * std::f64::consts::TAU / 48_000.0).sin() as f32)
                        .collect(),
                    right: (0..n)
                        .map(|i| (i as f64 * hz * std::f64::consts::TAU / 48_000.0).sin() as f32)
                        .collect(),
                }),
            })],
        }
    };
    let sets: Vec<FileSet> = (0..600).map(|i| set(220.0 + i as f64)).collect();
    let mut out = vec![0.0f32; 1000 * 2];
    let mut sounded = false;
    control.send(Command::Play).ok().unwrap();
    reset_violation_count();
    for (i, set) in sets.iter().enumerate() {
        if i % 7 == 0 {
            // Handing a set in is off the audio thread, so its allocation does not count.
            assert_no_alloc::permit_alloc(|| {
                h_sampler.set_files(set.clone());
                h_kit.set_files(set.clone());
            });
        }
        if i % 50 == 0 {
            // A pad's note for the kit (slot 0 answers 36).
            control
                .send(Command::Note {
                    track: 1,
                    kind: NoteKind::On {
                        pitch: 36,
                        velocity: 0.8,
                    },
                })
                .ok()
                .unwrap();
        }
        audio.render(&mut out, 2);
        control.poll(|_| {});
        sounded |= out.iter().any(|s| *s != 0.0);
    }
    assert_eq!(violation_count(), 0);
    // The voices actually played the files while the sets were swapped.
    assert!(sounded);
}
