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
    let mut left = vec![0.0; 96_256];
    let mut right = left.clone();
    for at in (0..left.len()).step_by(512) {
        let end = (at + 512).min(left.len());
        plan.process_at(&mut left[at..end], &mut right[at..end], Some(at as u64));
    }
    assert_eq!(left, right);
    left
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
