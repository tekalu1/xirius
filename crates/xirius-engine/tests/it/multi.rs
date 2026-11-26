//! The multi-instrument (ENG-075): notes go to the layers whose key and velocity ranges take
//! them, a note-off only to the layers its note-on reached, each layer moved by its transpose,
//! and the layers summed with their gain, pan and mute, lined up to the longest chain.

use std::sync::{Arc, Mutex};

use xirius_engine::builtin::{Delay, Gain};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::multi::{LayerSpec, MultiProc};
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, NoteKind, Plan, ProcessSetup, Processor, RenderMode,
    TrackSlot, TrackSpec,
};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};

type Heard = Arc<Mutex<Vec<NoteEvent>>>;

/// An instrument that keeps what it is handed and plays `level` while a note is held (a click
/// of `level` at frame 0 of the first block for `click`).
struct Ear {
    heard: Heard,
    level: f32,
    held: usize,
    click: bool,
    clicked: bool,
}

fn ear(level: f32) -> (Box<Ear>, Heard) {
    let heard = Heard::default();
    let e = Ear {
        heard: heard.clone(),
        level,
        held: 0,
        click: false,
        clicked: false,
    };
    (Box::new(e), heard)
}

impl Processor for Ear {
    fn name(&self) -> &str {
        "ear"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        self.heard.lock().unwrap().extend_from_slice(events);
        for e in events {
            match e.kind {
                NoteKind::On { .. } => self.held += 1,
                NoteKind::Off { .. } => self.held = self.held.saturating_sub(1),
                NoteKind::Control { .. } => {}
            }
        }
        if self.click {
            if !self.clicked {
                io.left[0] = self.level;
                io.right[0] = self.level;
                self.clicked = true;
            }
            return;
        }
        let v = if self.held > 0 { self.level } else { 0.0 };
        io.left.fill(v);
        io.right.fill(v);
    }
}

fn on(pitch: u8, velocity: u8) -> NoteEvent {
    NoteEvent {
        offset: 0,
        kind: NoteKind::On {
            pitch,
            velocity: f32::from(velocity) / 127.0,
        },
    }
}

fn off(pitch: u8) -> NoteEvent {
    NoteEvent {
        offset: 0,
        kind: NoteKind::Off { pitch },
    }
}

fn layer(node: u64, inst: Box<dyn Processor>, keys: (u8, u8), velocity: (u8, u8)) -> LayerSpec {
    let mut l = LayerSpec::new(node, inst, Vec::new());
    l.keys = keys;
    l.velocity = velocity;
    l
}

/// One block of `m` with `events`: (left, right).
fn block(m: &mut MultiProc, events: &[NoteEvent]) -> (Vec<f32>, Vec<f32>) {
    let n = SETUP.max_block;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    m.process(&mut AudioIo::new(&mut l, &mut r), events);
    (l, r)
}

fn pitches(heard: &Heard) -> Vec<(bool, u8)> {
    heard
        .lock()
        .unwrap()
        .drain(..)
        .filter_map(|e| match e.kind {
            NoteKind::On { pitch, .. } => Some((true, pitch)),
            NoteKind::Off { pitch } => Some((false, pitch)),
            NoteKind::Control { .. } => None,
        })
        .collect()
}

#[test]
fn notes_go_to_the_layers_whose_key_and_velocity_ranges_take_them() {
    let (low, low_heard) = ear(0.1);
    let (high, high_heard) = ear(0.2);
    let (soft, soft_heard) = ear(0.4);
    let mut m = MultiProc::new(
        1,
        vec![
            layer(2, low, (36, 59), (1, 127)),
            layer(3, high, (60, 127), (1, 127)),
            layer(4, soft, (0, 127), (1, 63)),
        ],
    );
    m.prepare(&SETUP);
    block(&mut m, &[on(40, 100), on(72, 30), on(59, 64), on(60, 63)]);
    assert_eq!(pitches(&low_heard), vec![(true, 40), (true, 59)]);
    assert_eq!(pitches(&high_heard), vec![(true, 72), (true, 60)]);
    assert_eq!(pitches(&soft_heard), vec![(true, 72), (true, 60)]);
}

#[test]
fn a_note_off_goes_only_to_the_layers_its_note_on_reached() {
    let (a, a_heard) = ear(0.1);
    let (b, b_heard) = ear(0.2);
    let mut m = MultiProc::new(
        1,
        vec![
            layer(2, a, (0, 127), (1, 63)),
            layer(3, b, (0, 127), (64, 127)),
        ],
    );
    m.prepare(&SETUP);
    block(&mut m, &[on(60, 100)]);
    // A soft note of the same pitch and the off of the first: the off goes to b only, though a
    // would take a soft note.
    block(&mut m, &[off(60)]);
    assert_eq!(pitches(&a_heard), vec![]);
    assert_eq!(pitches(&b_heard), vec![(true, 60), (false, 60)]);
    // An off that no note-on reached goes nowhere.
    block(&mut m, &[off(61)]);
    assert!(pitches(&a_heard).is_empty() && pitches(&b_heard).is_empty());
    // Controllers go to every layer.
    let cc = NoteEvent {
        offset: 3,
        kind: NoteKind::Control {
            number: 64,
            value: 127,
        },
    };
    block(&mut m, &[cc]);
    assert_eq!(a_heard.lock().unwrap().as_slice(), &[cc]);
    assert_eq!(b_heard.lock().unwrap().as_slice(), &[cc]);
}

#[test]
fn a_layer_plays_its_notes_moved_by_its_transpose_and_ends_them_there() {
    let (a, a_heard) = ear(0.1);
    let mut spec = layer(2, a, (0, 127), (1, 127));
    spec.transpose = 12;
    let (b, b_heard) = ear(0.1);
    let mut down = layer(3, b, (0, 127), (1, 127));
    down.transpose = -5;
    let mut m = MultiProc::new(1, vec![spec, down]);
    m.prepare(&SETUP);
    block(&mut m, &[on(60, 100), on(124, 100), on(2, 100)]);
    block(&mut m, &[off(60), off(124), off(2)]);
    // 124 + 12 and 2 - 5 leave the keyboard: that layer does not play them.
    assert_eq!(
        pitches(&a_heard),
        vec![(true, 72), (true, 14), (false, 72), (false, 14)]
    );
    assert_eq!(
        pitches(&b_heard),
        vec![(true, 55), (true, 119), (false, 55), (false, 119)]
    );
}

#[test]
fn the_layers_sum_with_their_gain_pan_and_mute() {
    let (a, _) = ear(0.5);
    let (b, _) = ear(0.25);
    let (c, _) = ear(1.0);
    let mut la = layer(2, a, (0, 127), (1, 127));
    la.gain = 0.5;
    let mut lb = layer(3, b, (0, 127), (1, 127));
    lb.pan = 1.0;
    let mut lc = layer(4, c, (0, 127), (1, 127));
    lc.mute = true;
    let mut m = MultiProc::new(1, vec![la, lb, lc]);
    m.prepare(&SETUP);
    let (l, r) = block(&mut m, &[on(60, 100)]);
    let (pl, pr) = xirius_engine::mixer::pan_gains(1.0);
    let (cl, cr) = xirius_engine::mixer::pan_gains(0.0);
    let want_l = 0.25 * cl + 0.25 * pl;
    let want_r = 0.25 * cr + 0.25 * pr;
    assert!(l.iter().all(|s| (s - want_l).abs() < 1e-6), "{}", l[0]);
    assert!(r.iter().all(|s| (s - want_r).abs() < 1e-6), "{}", r[0]);
}

#[test]
fn a_shorter_layer_is_delayed_to_meet_the_longest_on_the_same_sample() {
    let (mut a, _) = ear(0.5);
    a.click = true;
    let (mut b, _) = ear(0.25);
    b.click = true;
    let la = LayerSpec::new(2, a, vec![(20, Box::new(Delay::new(100)))]);
    let lb = LayerSpec::new(3, b, vec![(30, Box::new(Gain::new(1.0)))]);
    let mut m = MultiProc::new(1, vec![la, lb]);
    m.prepare(&SETUP);
    assert_eq!(m.latency(), 100);
    let mut left = Vec::new();
    for _ in 0..4 {
        left.extend(block(&mut m, &[]).0);
    }
    let hits: Vec<usize> = (0..left.len()).filter(|&i| left[i] != 0.0).collect();
    assert_eq!(hits, vec![100]);
    let centre = xirius_engine::mixer::pan_gains(0.0).0;
    assert!((left[100] - 0.75 * centre).abs() < 1e-6);
    let ids: Vec<u64> = m.insert_ids().collect();
    assert_eq!(ids, vec![20, 30]);
}

#[test]
fn a_muted_layer_still_ends_the_note_it_holds() {
    let (a, a_heard) = ear(1.0);
    let mut m = MultiProc::new(1, vec![layer(2, a, (0, 127), (1, 127))]);
    m.prepare(&SETUP);
    let mut cells = Vec::new();
    m.cells(&mut cells);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].0, 2);
    block(&mut m, &[on(60, 100)]);
    cells[0].1.set(1.0, 0.0, true);
    block(&mut m, &[off(60)]);
    let (l, _) = block(&mut m, &[]);
    assert!(l.iter().all(|s| *s == 0.0));
    assert_eq!(pitches(&a_heard), vec![(true, 60), (false, 60)]);
    // Back on: nothing held, nothing heard.
    cells[0].1.set(1.0, 0.0, false);
    let (l, _) = block(&mut m, &[]);
    assert!(l.iter().all(|s| *s == 0.0));
}

#[test]
fn branch_gain_reaches_a_layer_of_a_tracks_multi_instrument() {
    let (a, _) = ear(0.5);
    let (b, _) = ear(0.25);
    let multi = MultiProc::new(
        1,
        vec![
            layer(2, a, (0, 127), (1, 127)),
            layer(3, b, (0, 127), (1, 127)),
        ],
    );
    let mut t = TrackSpec::new("t", Box::new(multi));
    t.prefetch = false;
    let setup = ProcessSetup {
        mode: RenderMode::Realtime,
        ..SETUP
    };
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    let (mut audio, mut control) = engine::create(
        Plan::build(spec, setup, None).unwrap(),
        EngineConfig::default(),
    );
    let render = |audio: &mut engine::AudioEngine| {
        let mut out = vec![0.0; SETUP.max_block * 4 * 2];
        audio.render(&mut out, 2);
        out.iter().step_by(2).copied().collect::<Vec<f32>>()
    };
    control
        .send(Command::Note {
            track: 0,
            kind: NoteKind::On {
                pitch: 60,
                velocity: 0.8,
            },
        })
        .ok()
        .unwrap();
    let both = render(&mut audio);
    let centre = xirius_engine::mixer::pan_gains(0.0).0;
    let last = *both.last().unwrap();
    assert!((last - 0.75 * centre).abs() < 1e-5, "{last}");
    control
        .send(Command::BranchGain {
            node: 3,
            gain: 1.0,
            pan: 0.0,
            mute: true,
        })
        .ok()
        .unwrap();
    let one = render(&mut audio);
    let last = *one.last().unwrap();
    assert!((last - 0.5 * centre).abs() < 1e-5, "{last}");
}
