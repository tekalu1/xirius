use std::f64::consts::TAU;
use xirius_engine::mixer::{BusSpec, Output};
use xirius_engine::mixer::{MixSpec, Plan, TrackSlot, TrackSpec};
use xirius_engine::multi::{LayerSpec, MultiProc};
use xirius_engine::note_input::NoteInput;
use xirius_engine::performance::Input;
use xirius_engine::processor::{AudioIo, NoteEvent, NoteKind, ProcessSetup, Processor, RenderMode};
use xirius_engine::split::{BranchSpec, SplitProc};
use xirius_engine::stock;
use xirius_engine::timeline::{TimedNote, Timeline};

#[global_allocator]
static ALLOC: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

struct Silence;

impl Processor for Silence {
    fn name(&self) -> &str {
        "silence"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
}

struct Heard {
    pitch: f32,
}

impl Processor for Heard {
    fn name(&self) -> &str {
        "heard"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let mut at = 0;
        for e in events {
            let end = (e.offset as usize).min(io.frames());
            io.left[at..end].fill(self.pitch);
            io.right[at..end].fill(self.pitch);
            at = end;
            self.pitch = match e.kind {
                NoteKind::On { pitch, .. } => pitch as f32,
                NoteKind::Off { .. } => 0.0,
                NoteKind::Control { .. } => self.pitch,
            };
        }
        io.left[at..].fill(self.pitch);
        io.right[at..].fill(self.pitch);
    }
}

fn guide() -> TrackSpec {
    let mut t = TrackSpec::new("guide", Box::new(Silence));
    t.prefetch = false;
    t.mute = true;
    t.timeline = Timeline::new(
        vec![
            TimedNote {
                frame: 5,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            },
            TimedNote {
                frame: 37,
                kind: NoteKind::Off { pitch: 60 },
            },
            TimedNote {
                frame: 37,
                kind: NoteKind::On {
                    pitch: 64,
                    velocity: 1.0,
                },
            },
            TimedNote {
                frame: 91,
                kind: NoteKind::Off { pitch: 64 },
            },
        ],
        vec![],
    );
    t
}

#[test]
fn unrelated_tracks_keep_prefetch_and_parallel_workers_with_a_note_source() {
    let mut source = guide();
    source.prefetch = true;
    let mut receiver = TrackSpec::new("voice", Box::new(Silence));
    receiver
        .inserts
        .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
    let mut tracks = vec![TrackSlot::New(source), TrackSlot::New(receiver)];
    for i in 0..8 {
        tracks.push(TrackSlot::New(TrackSpec::new(
            format!("unrelated {i}"),
            Box::new(Heard { pitch: 0.0 }),
        )));
    }
    let mut plan = Plan::build(
        MixSpec {
            tracks,
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Realtime,
        },
        None,
    )
    .unwrap();
    plan.set_workers(2).unwrap();
    assert!(
        plan.prefetch_status(2).is_some(),
        "unrelated track prefetch"
    );
    assert_eq!(plan.workers(), 2, "parallel pool survives note routing");
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    plan.process_at(&mut left, &mut right, Some(0));
    assert!(
        plan.prefetch_status(2).unwrap().lent,
        "worker has the unrelated chain"
    );
}

#[test]
fn routed_notes_render_identically_with_one_or_four_workers_and_prefetch() {
    let build = |workers| {
        let mut receiver = TrackSpec::new("voice", Box::new(Silence));
        receiver
            .inserts
            .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
        let mut tracks = vec![TrackSlot::New(guide()), TrackSlot::New(receiver)];
        for i in 0..8 {
            tracks.push(TrackSlot::New(TrackSpec::new(
                format!("unrelated {i}"),
                Box::new(Silence),
            )));
        }
        let mut plan = Plan::build(
            MixSpec {
                tracks,
                ..Default::default()
            },
            ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 32,
                mode: RenderMode::Realtime,
            },
            None,
        )
        .unwrap();
        plan.set_workers(workers).unwrap();
        plan
    };
    let (mut serial, mut parallel) = (build(1), build(4));
    assert_eq!(parallel.workers(), 4);
    for at in (0..128).step_by(32) {
        let (mut a, mut b, mut c, mut d) = ([0.0; 32], [0.0; 32], [0.0; 32], [0.0; 32]);
        serial.process_at(&mut a, &mut b, Some(at));
        parallel.process_at(&mut c, &mut d, Some(at));
        assert_eq!(a, c);
        assert_eq!(b, d);
    }
}

#[test]
fn overflowing_the_note_queue_releases_notes_without_restarting_them() {
    let mut inlet = NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0);
    inlet.prepare(&ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 32,
        mode: RenderMode::Offline,
    });
    inlet.note_block(
        0,
        &[NoteEvent {
            offset: 0,
            kind: NoteKind::On {
                pitch: 64,
                velocity: 1.0,
            },
        }],
        0,
        false,
    );
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    inlet.process(
        &mut AudioIo {
            left: &mut left,
            right: &mut right,
            key: None,
        },
        &[],
    );
    assert_eq!(left[0], 64.0);
    let events = vec![
        NoteEvent {
            offset: 0,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 1.0,
            },
        };
        4097
    ];
    inlet.note_block(0, &events, 32, false);
    inlet.process(
        &mut AudioIo {
            left: &mut left,
            right: &mut right,
            key: None,
        },
        &[],
    );
    assert!(
        left.iter().all(|&x| x == 0.0),
        "overflow leaves no note held"
    );
}

fn render(reverse: bool, delay: u32) -> Vec<f32> {
    let source = usize::from(reverse);
    let mut recipient = TrackSpec::new("voice", Box::new(Silence));
    recipient.prefetch = false;
    if delay > 0 {
        recipient.source = Box::new(Latent(delay));
    }
    recipient.inserts.push(Box::new(NoteInput::new(
        Box::new(Heard { pitch: 0.0 }),
        source,
    )));
    let tracks = if reverse {
        vec![recipient, guide()]
    } else {
        vec![guide(), recipient]
    };
    let mut p = Plan::build(
        MixSpec {
            tracks: tracks.into_iter().map(TrackSlot::New).collect(),
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let mut out = vec![0.0; 128];
    let mut right = out.clone();
    for at in (0..128).step_by(32) {
        p.process_at(
            &mut out[at..at + 32],
            &mut right[at..at + 32],
            Some(at as u64),
        );
    }
    assert_eq!(out, right);
    out
}

struct Latent(u32);
impl Processor for Latent {
    fn name(&self) -> &str {
        "latent"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
    fn latency(&self) -> u32 {
        self.0
    }
}

#[test]
fn muted_guide_reaches_receiver_before_or_after_it_in_track_order() {
    let first = render(false, 0);
    assert_eq!(first, render(true, 0));
    assert_eq!(first[4], 0.0);
    assert_eq!(first[5], 60.0);
    assert_eq!(first[37], 64.0);
    assert_eq!(first[91], 0.0);
}

#[test]
fn routed_note_blocks_do_not_allocate_on_the_audio_thread() {
    let mut voice = TrackSpec::new("voice", Box::new(Silence));
    voice
        .inserts
        .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(guide()), TrackSlot::New(voice)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    assert_no_alloc::assert_no_alloc(|| {
        for at in (0..128).step_by(32) {
            plan.process_at(&mut left, &mut right, Some(at));
        }
    });
}

#[test]
fn guide_events_follow_the_receiving_chain_latency_across_blocks() {
    let out = render(false, 40);
    assert_eq!(out[44], 0.0);
    assert_eq!(out[45], 60.0);
    assert_eq!(out[77], 64.0);
}

#[test]
fn stop_seek_and_live_keys_release_the_receiving_note() {
    let mut source = TrackSpec::new("guide", Box::new(Silence));
    source.timeline = Timeline::new(
        vec![TimedNote {
            frame: 5,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 1.0,
            },
        }],
        vec![],
    );
    let mut receiver = TrackSpec::new("voice", Box::new(Silence));
    receiver
        .inserts
        .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(source), TrackSlot::New(receiver)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    plan.process_at(&mut left, &mut right, Some(0));
    assert_eq!(left[5], 60.0);
    plan.process_at(&mut left, &mut right, None);
    assert_eq!(left[0], 0.0, "stop releases the held guide note");
    plan.process_at(&mut left, &mut right, Some(0));
    assert_eq!(left[5], 60.0, "the loop start can play again");
    plan.process_at(&mut left, &mut right, Some(200));
    assert_eq!(left[0], 0.0, "a seek clears the previous note");
    assert!(plan.push_event(
        0,
        NoteEvent {
            offset: 3,
            kind: NoteKind::On {
                pitch: 67,
                velocity: 1.0
            }
        }
    ));
    plan.process_at(&mut left, &mut right, None);
    assert_eq!(left[2], 0.0);
    assert_eq!(left[3], 67.0, "a live key on the guide reaches the effect");
    assert!(plan.push_event(
        0,
        NoteEvent {
            offset: 0,
            kind: NoteKind::Off { pitch: 67 }
        }
    ));
    plan.process_at(&mut left, &mut right, None);
    assert_eq!(left[0], 0.0);
}

struct LiveGuide;
impl Processor for LiveGuide {
    fn name(&self) -> &str {
        "live guide"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.clear();
    }
    fn live_input(&mut self, _: Input) -> bool {
        true
    }
}

#[test]
fn live_keyboard_input_on_the_guide_reaches_the_effect_and_releases() {
    let guide = TrackSpec::new("guide", Box::new(LiveGuide));
    let mut voice = TrackSpec::new("voice", Box::new(Silence));
    voice
        .inserts
        .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(guide), TrackSlot::New(voice)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let input = |kind| Input {
        source: 7,
        plugin: None,
        kind,
    };
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    assert!(plan.push_live_input(
        0,
        input(NoteKind::On {
            pitch: 72,
            velocity: 1.0
        })
    ));
    plan.process_at(&mut left, &mut right, None);
    assert_eq!(left[0], 72.0);
    plan.release_live_inputs();
    plan.process_at(&mut left, &mut right, None);
    assert_eq!(left[0], 0.0);
}

#[test]
fn bus_master_splitter_and_multi_layer_receive_guide_notes() {
    for place in ["bus", "master", "split", "multi"] {
        let mut guide = TrackSpec::new("guide", Box::new(Silence));
        guide.mute = true;
        guide.timeline = Timeline::new(
            vec![TimedNote {
                frame: 5,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            }],
            vec![],
        );
        let inlet =
            || Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)) as Box<dyn Processor>;
        let mut voice = TrackSpec::new("voice", Box::new(Silence));
        let mut bus = BusSpec::new("bus");
        let mut master_inserts = Vec::new();
        match place {
            "bus" => {
                voice.output = Output::Bus(0);
                bus.inserts.push(inlet());
            }
            "master" => master_inserts.push(inlet()),
            "split" => voice.inserts.push(Box::new(SplitProc::new(
                10,
                vec![BranchSpec::new(11, vec![(12, inlet())])],
            ))),
            "multi" => {
                voice.source = Box::new(MultiProc::new(
                    20,
                    vec![LayerSpec::new(21, Box::new(Silence), vec![(22, inlet())])],
                ))
            }
            _ => unreachable!(),
        }
        let mut plan = Plan::build(
            MixSpec {
                tracks: vec![TrackSlot::New(guide), TrackSlot::New(voice)],
                buses: if place == "bus" { vec![bus] } else { vec![] },
                master_inserts,
                ..Default::default()
            },
            ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 32,
                mode: RenderMode::Offline,
            },
            None,
        )
        .unwrap();
        let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
        plan.process_at(&mut left, &mut right, Some(0));
        assert_eq!(left[4], 0.0, "{place}");
        assert!(left[5] > 20.0, "{place}: {}", left[5]);
    }
}

#[test]
fn multi_instrument_track_can_supply_its_timeline_notes() {
    let mut source = guide();
    source.source = Box::new(MultiProc::new(
        30,
        vec![LayerSpec::new(31, Box::new(Silence), vec![])],
    ));
    let mut receiver = TrackSpec::new("voice", Box::new(Silence));
    receiver
        .inserts
        .push(Box::new(NoteInput::new(Box::new(Heard { pitch: 0.0 }), 0)));
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(source), TrackSlot::New(receiver)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 32,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let (mut left, mut right) = ([0.0; 32], [0.0; 32]);
    plan.process_at(&mut left, &mut right, Some(0));
    assert_eq!(left[5], 60.0);
}

struct Voice {
    frame: usize,
}
impl Processor for Voice {
    fn name(&self) -> &str {
        "voice"
    }
    fn prepare(&mut self, _: &ProcessSetup) {
        self.frame = 0;
    }
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for i in 0..io.frames() {
            let frame = self.frame + i;
            let hz = if frame < 48_000 { 270.0 } else { 340.0 };
            let sample = (0.4 * (TAU * hz * frame as f64 / 48_000.0).sin()) as f32;
            io.left[i] = sample;
            io.right[i] = sample;
        }
        self.frame += io.frames();
    }
}

fn pitch_at(signal: &[f32], low: f64, high: f64) -> f64 {
    let mut best = (0.0, f64::NEG_INFINITY);
    let mut hz = low;
    while hz <= high {
        let mut re = 0.0;
        let mut im = 0.0;
        for (i, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (TAU * i as f64 / signal.len() as f64).cos();
            let phase = TAU * hz * i as f64 / 48_000.0;
            re += f64::from(*sample) * window * phase.cos();
            im += f64::from(*sample) * window * phase.sin();
        }
        let power = re * re + im * im;
        if power > best.1 {
            best = (hz, power);
        }
        hz += 0.5;
    }
    best.0
}

fn song_pitch(connected: bool) -> Vec<f32> {
    song_pitch_setup(connected, false).0
}

fn song_pitch_setup(connected: bool, limiter: bool) -> (Vec<f32>, u32) {
    let mut guide = TrackSpec::new("melody", Box::new(Silence));
    guide.mute = true;
    guide.timeline = Timeline::new(
        vec![
            TimedNote {
                frame: 0,
                kind: NoteKind::On {
                    pitch: 60,
                    velocity: 1.0,
                },
            },
            TimedNote {
                frame: 48_000,
                kind: NoteKind::Off { pitch: 60 },
            },
            TimedNote {
                frame: 48_000,
                kind: NoteKind::On {
                    pitch: 64,
                    velocity: 1.0,
                },
            },
            TimedNote {
                frame: 96_000,
                kind: NoteKind::Off { pitch: 64 },
            },
        ],
        vec![],
    );
    let mut voice = TrackSpec::new("singing", Box::new(Voice { frame: 0 }));
    let (effect, handle) = stock::create("pitch-correction").unwrap();
    let amount = handle.def().param("amount").unwrap().1;
    assert!(handle.set("amount", amount.to_norm(100.0)));
    let transition = handle.def().param("transition").unwrap().1;
    assert!(handle.set("transition", transition.to_norm(1.0)));
    let speed = handle.def().param("speed").unwrap().1;
    assert!(handle.set("speed", speed.to_norm(1.0)));
    if limiter {
        voice.inserts.push(stock::create("limiter").unwrap().0);
    }
    voice.inserts.push(if connected {
        Box::new(NoteInput::new(effect, 0))
    } else {
        effect
    });
    let mut plan = Plan::build(
        MixSpec {
            tracks: vec![TrackSlot::New(guide), TrackSlot::New(voice)],
            ..Default::default()
        },
        ProcessSetup {
            sample_rate: 48_000.0,
            max_block: 512,
            mode: RenderMode::Offline,
        },
        None,
    )
    .unwrap();
    let latency = plan.latency();
    let mut left = vec![0.0; 96_256];
    let mut right = left.clone();
    for at in (0..left.len()).step_by(512) {
        let end = (at + 512).min(left.len());
        plan.process_at(&mut left[at..end], &mut right[at..end], Some(at as u64));
    }
    assert_eq!(left, right);
    (left, latency)
}

#[test]
fn pitch_correction_follows_melody_instead_of_nearest_chromatic_note() {
    let with = song_pitch(true);
    let without = song_pitch(false);
    for (start, end, target, free) in [
        (25_000, 37_000, 261.63, 277.18),
        (70_000, 82_000, 329.63, 349.23),
    ] {
        let guided = pitch_at(&with[start..end], target - 20.0, free + 20.0);
        let unguided = pitch_at(&without[start..end], target - 20.0, free + 20.0);
        assert!(
            (guided - target).abs() < 6.0,
            "guided={guided}, target={target}"
        );
        assert!(
            (unguided - free).abs() < 6.0,
            "unguided={unguided}, free={free}"
        );
    }
}

#[test]
fn measured_pitch_change_follows_the_guide_with_and_without_a_real_limiter() {
    for limiter in [false, true] {
        let (signal, latency) = song_pitch_setup(true, limiter);
        assert!(latency > 0);
        let expected = 48_000 + latency as usize;
        let classify = |center: usize| {
            let hz = pitch_at(&signal[center - 1024..center + 1024], 245.0, 345.0);
            (hz - 329.63).abs() < (hz - 261.63).abs()
        };
        let (mut lo, mut hi) = (expected - 2400, expected + 2400);
        assert!(
            !classify(lo),
            "old pitch before the change, limiter={limiter}"
        );
        assert!(
            classify(hi),
            "new pitch after the change, limiter={limiter}"
        );
        while hi - lo > 120 {
            let mid = (lo + hi) / 2;
            if classify(mid) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let error = hi.abs_diff(expected);
        eprintln!(
            "MIX-051 limiter={limiter} transition={hi} expected={expected} error_ms={:.3}",
            error as f64 / 48.0
        );
        assert!(
            error <= 720,
            "limiter={limiter}, measured={hi}, expected={expected}, error={error} frames"
        );
    }
}

#[test]
#[ignore = "manual MIX-051 render timing; run with --ignored --nocapture"]
fn thirty_two_track_note_route_render_timing() {
    use std::time::Instant;
    use xirius_engine::builtin::Sine;

    let build = |connected| {
        let mut tracks = Vec::new();
        let mut source = guide();
        source.source = Box::new(Sine::new(220.0, 0.01));
        tracks.push(TrackSlot::New(source));
        for i in 1..32 {
            let mut track = TrackSpec::new(
                format!("track {i}"),
                Box::new(Sine::new(180.0 + i as f64 * 5.0, 0.01)),
            );
            for uid in ["eq", "compressor", "limiter"] {
                track.inserts.push(stock::create(uid).unwrap().0);
            }
            if i == 1 {
                let pitch = stock::create("pitch-correction").unwrap().0;
                track.inserts.push(if connected {
                    Box::new(NoteInput::new(pitch, 0))
                } else {
                    pitch
                });
            }
            tracks.push(TrackSlot::New(track));
        }
        let mut plan = Plan::build(
            MixSpec {
                tracks,
                ..Default::default()
            },
            ProcessSetup {
                sample_rate: 48_000.0,
                max_block: 256,
                mode: RenderMode::Offline,
            },
            None,
        )
        .unwrap();
        plan.set_workers(4).unwrap();
        plan
    };
    for connected in [false, true] {
        let mut plan = build(connected);
        let (mut left, mut right) = ([0.0; 256], [0.0; 256]);
        let start = Instant::now();
        for at in (0..48_128).step_by(256) {
            plan.process_at(&mut left, &mut right, Some(at));
        }
        eprintln!(
            "MIX-051 32 tracks connected={connected} workers={} elapsed_ms={:.3}",
            plan.workers(),
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
