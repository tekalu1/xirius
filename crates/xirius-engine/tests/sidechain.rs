//! ENG-078: the sidechain key an insert hears must be the sources' signals,
//! summed and aligned to the main signal at that insert — a keyed insert on a
//! lagging path hears an earlier key the same samples late, and one on a fast
//! path waits for its sources.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use xirius_engine::builtin::{Delay, Gain, Sine};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::mixer::PlanError;
use xirius_engine::{
    AudioIo, BusSpec, Command, KeyFrom, KeySourceSpec, KeySpec, MixSpec, NoteEvent, Output, Plan,
    ProcessSetup, Processor, RenderMode, TrackSlot, TrackSpec,
};

fn setup() -> ProcessSetup {
    ProcessSetup {
        sample_rate: 48_000.0,
        max_block: 256,
        mode: RenderMode::Offline,
    }
}

/// A source that writes its own running frame count: the value in the buffer
/// is the frame it was written at, so a delay shows up as a shift of the value.
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

/// An insert that shows what it heard: the key goes to the right channel, the
/// main signal keeps the left. Flags record whether a key arrived at all.
struct KeyPeek {
    had_key: Arc<AtomicBool>,
    had_none: Arc<AtomicBool>,
}

impl KeyPeek {
    fn new() -> (KeyPeek, Arc<AtomicBool>, Arc<AtomicBool>) {
        let had_key = Arc::new(AtomicBool::new(false));
        let had_none = Arc::new(AtomicBool::new(false));
        (
            KeyPeek {
                had_key: had_key.clone(),
                had_none: had_none.clone(),
            },
            had_key,
            had_none,
        )
    }
}

impl Processor for KeyPeek {
    fn name(&self) -> &str {
        "keypeek"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        match io.key {
            Some((_, kr)) => {
                self.had_key.store(true, Ordering::Relaxed);
                io.right.copy_from_slice(kr);
            }
            None => {
                self.had_none.store(true, Ordering::Relaxed);
                let left = io.left.to_vec();
                io.right.copy_from_slice(&left);
            }
        }
    }
}

fn ramp_track(name: &str, lags: &[usize]) -> TrackSpec {
    let mut t = TrackSpec::new(name, Box::new(Ramp(0)));
    t.inserts = lags
        .iter()
        .map(|&d| Box::new(Delay::new(d)) as Box<dyn Processor>)
        .collect();
    t
}

fn render(spec: MixSpec, blocks: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut plan = Plan::build(spec, setup(), None).unwrap();
    let mut l = vec![0.0; blocks * block];
    let mut r = vec![0.0; blocks * block];
    for b in 0..blocks {
        let pos = (b * block) as u64;
        plan.process_at(
            &mut l[b * block..(b + 1) * block],
            &mut r[b * block..(b + 1) * block],
            Some(pos),
        );
    }
    (l, r)
}

fn keyed_track(
    name: &str,
    key: KeySpec,
    after: &[usize],
) -> (TrackSpec, Arc<AtomicBool>, Arc<AtomicBool>) {
    let (peek, had_key, had_none) = KeyPeek::new();
    let mut t = ramp_track(name, &[]);
    t.inserts = std::iter::once(Box::new(peek) as Box<dyn Processor>)
        .chain(
            after
                .iter()
                .map(|&d| Box::new(Delay::new(d)) as Box<dyn Processor>),
        )
        .collect();
    t.keys = vec![key];
    (t, had_key, had_none)
}

fn src(id: u64, from: KeyFrom, pre: bool, level: f32) -> KeySourceSpec {
    KeySourceSpec {
        id,
        from,
        pre,
        level,
    }
}

/// Two sources with different insert latencies key one insert whose own chain
/// is faster than both: the key waits for the slowest source, and the main
/// signal is delayed by exactly the same amount at the insert.
#[test]
fn the_key_and_the_main_signal_meet_at_the_insert() {
    // The sources feed only the key: a zero fader keeps their own output out
    // of the mix, and the pre taps still see their post-insert signals.
    let mut a = ramp_track("a", &[30]);
    a.gain = 0.0;
    let mut b = ramp_track("b", &[64]);
    b.gain = 0.0;
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 7,
            on: true,
            sources: vec![
                src(1, KeyFrom::Track(0), true, 2.0),
                src(2, KeyFrom::Track(1), true, 1.0),
            ],
        },
        &[40],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(a), TrackSlot::New(b), TrackSlot::New(r)],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 8, 256);
    assert!(had_key.load(Ordering::Relaxed));
    // The pre taps carry the post-insert signals: a's (i-30) delayed by 34 and
    // b's (i-64) as it is, so both are (i-64) at the insert — the time the
    // main signal was delayed to as well. Levels 2.0 and 1.0, then the row's
    // own 40: out_r[i] = (2+1) * out_l[i], exactly, as the values are whole.
    for i in 0..l.len() {
        assert_eq!(out_r[i], l[i] * 3.0, "right {i}");
    }
    assert_eq!(l[104], 0.0);
    assert_eq!(l[105], 1.0);
}

/// A source that is slower than the receiver's insert position: the main
/// signal waits for it, so the whole path — not only the key — stays aligned.
#[test]
fn the_main_signal_waits_for_a_slow_source() {
    let mut a = ramp_track("a", &[96]);
    a.gain = 0.0;
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 3,
            on: true,
            sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
        },
        &[40],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(a), TrackSlot::New(r)],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 8, 256);
    assert!(had_key.load(Ordering::Relaxed));
    // The key's time is a's (i-96); r's main waited from 0 to 96, then the 40:
    // out_l[i] = i-136 and the key is the same samples, so the channels match
    // over the whole render.
    for i in 0..l.len() {
        assert_eq!(out_r[i], l[i], "right {i}");
    }
    assert_eq!(l[136], 0.0);
    assert_eq!(l[137], 1.0);
}

/// A pre tap is taken after the inserts and before the fader (the mute still
/// applies); a post tap after fader and pan.
#[test]
fn pre_and_post_taps_differ_by_the_fader() {
    let run = |pre: bool| {
        let mut s = ramp_track("s", &[]);
        s.inserts = vec![Box::new(Gain::new(0.5))];
        s.gain = 0.25;
        let (r, had_key, _) = keyed_track(
            "r",
            KeySpec {
                insert: 0,
                plugin: 11,
                on: true,
                sources: vec![src(1, KeyFrom::Track(0), pre, 1.0)],
            },
            &[],
        );
        let spec = MixSpec {
            tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
            ..MixSpec::default()
        };
        let (l, out_r) = render(spec, 4, 256);
        assert!(had_key.load(Ordering::Relaxed));
        (l, out_r)
    };
    // The receiver's main is its own ramp (i), and the source's post-fader
    // signal (0.125 i) reaches the master on its own: out_l = 1.125 i. The
    // right channel adds the peek's key: pre 0.5 i, post 0.125 i.
    let (l, pre_r) = run(true);
    let (_, post_r) = run(false);
    for i in 0..l.len() {
        assert_eq!(l[i], (i as f32) * 1.125, "left {i}");
        assert_eq!(pre_r[i], (i as f32) * 0.625, "pre {i}");
        assert_eq!(post_r[i], (i as f32) * 0.25, "post {i}");
    }
}

/// The key of a muted source's pre tap is silent too: the mute is applied to
/// the pre-fader lane the tap reads.
#[test]
fn a_muted_source_keys_silence() {
    let mut s = ramp_track("s", &[]);
    s.mute = true;
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 5,
            on: true,
            sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
        },
        &[],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
        ..MixSpec::default()
    };
    let (_, out_r) = render(spec, 4, 256);
    assert!(had_key.load(Ordering::Relaxed));
    assert!(out_r.iter().all(|&x| x == 0.0));
}

/// A key taken from a bus: the bus's rendered signal, summed into the key.
#[test]
fn a_bus_can_be_the_key_source() {
    let mut s = ramp_track("s", &[]);
    s.gain = 0.5;
    s.output = Output::Bus(0);
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 9,
            on: true,
            sources: vec![src(1, KeyFrom::Bus(0), false, 1.0)],
        },
        &[],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
        buses: vec![BusSpec::new("fx")],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 4, 256);
    assert!(had_key.load(Ordering::Relaxed));
    // The master hears the bus's 0.5i on top of r's i on the left; the right
    // is the peek's key (the bus's 0.5i) plus the bus's own 0.5i.
    for i in 0..l.len() {
        assert_eq!(l[i], (i as f32) * 1.5, "left {i}");
        assert_eq!(out_r[i], i as f32, "right {i}");
    }
}

/// A keyed insert on the master hears its sources the same way.
#[test]
fn the_master_can_take_a_key() {
    let s = ramp_track("s", &[]);
    let (peek, had_key, _) = KeyPeek::new();
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(s)],
        master_inserts: vec![Box::new(peek)],
        master_keys: vec![KeySpec {
            insert: 0,
            plugin: 21,
            on: true,
            sources: vec![src(1, KeyFrom::Track(0), false, 0.5)],
        }],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 4, 256);
    assert!(had_key.load(Ordering::Relaxed));
    for i in 0..l.len() {
        assert_eq!(out_r[i], l[i] * 0.5, "right {i}");
    }
}

/// `set_key_on(false)` gives the insert its own signal for a key;
/// `set_key_level` moves a source's contribution.
#[test]
fn key_commands_move_the_live_values() {
    let s = ramp_track("s", &[]);
    let (r, had_key, had_none) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 42,
            on: true,
            sources: vec![src(77, KeyFrom::Track(0), false, 1.0)],
        },
        &[],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
        ..MixSpec::default()
    };
    let setup = ProcessSetup {
        mode: RenderMode::Realtime,
        ..setup()
    };
    let (mut audio, mut control) = engine::create(
        Plan::build(spec, setup, None).unwrap(),
        EngineConfig::default(),
    );
    let mut out = vec![0.0; 256 * 2];
    audio.render(&mut out, 2);
    assert!(had_key.load(Ordering::Relaxed));
    control
        .send(Command::KeyOn {
            plugin: 42,
            on: false,
        })
        .ok()
        .unwrap();
    audio.render(&mut out, 2);
    control.poll(|_| {});
    assert!(had_none.load(Ordering::Relaxed));
    control
        .send(Command::KeyOn {
            plugin: 42,
            on: true,
        })
        .ok()
        .unwrap();
    control
        .send(Command::KeyLevel {
            send: 77,
            level: 0.5,
        })
        .ok()
        .unwrap();
    for _ in 0..4 {
        audio.render(&mut out, 2);
        control.poll(|_| {});
    }
    assert!(out.iter().any(|&x| x != 0.0));
}

/// A keyed track is never lent to a prefetch worker (its inserts need this
/// block's key, not a key rendered ahead), and playback gives the export's
/// samples (ENG-078).
#[test]
fn a_keyed_track_is_not_prefetched_and_matches_the_export() {
    let make = |had_key: &mut Arc<AtomicBool>| {
        let mut a = ramp_track("a", &[]);
        a.gain = 0.0;
        let (mut r, key, _) = keyed_track(
            "r",
            KeySpec {
                insert: 0,
                plugin: 3,
                on: true,
                sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
            },
            &[],
        );
        *had_key = key;
        r.prefetch = true;
        MixSpec {
            tracks: vec![TrackSlot::New(a), TrackSlot::New(r)],
            ..MixSpec::default()
        }
    };
    let mut dummy = Arc::new(AtomicBool::new(false));
    let (want_l, want_r) = render(make(&mut dummy), 8, 256);

    let mut had_key = Arc::new(AtomicBool::new(false));
    let (mut audio, mut control) = engine::create(
        Plan::build(
            make(&mut had_key),
            ProcessSetup {
                mode: RenderMode::Realtime,
                ..setup()
            },
            None,
        )
        .unwrap(),
        EngineConfig::default(),
    );
    control.send(Command::Play).ok().unwrap();
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for _ in 0..8 {
        let mut out = vec![0.0; 256 * 2];
        audio.render(&mut out, 2);
        l.extend(out.iter().step_by(2).copied());
        r.extend(out.iter().skip(1).step_by(2).copied());
    }
    assert_eq!(audio.prefetch(1), None, "a keyed track is never lent out");
    assert!(had_key.load(Ordering::Relaxed));
    assert_eq!(l, want_l);
    assert_eq!(r, want_r);
}

/// A source that cannot be heard keys silence (ENG-078): a muted track's
/// post tap and a muted bus's tap stop too, exactly like a pre tap.
#[test]
fn a_silenced_source_keys_silence_for_track_and_bus() {
    for pre in [true, false] {
        let mut s = ramp_track("s", &[]);
        s.mute = true;
        let (r, had_key, _) = keyed_track(
            "r",
            KeySpec {
                insert: 0,
                plugin: 5,
                on: true,
                sources: vec![src(1, KeyFrom::Track(0), pre, 1.0)],
            },
            &[],
        );
        let spec = MixSpec {
            tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
            ..MixSpec::default()
        };
        let (_, out_r) = render(spec, 4, 256);
        assert!(had_key.load(Ordering::Relaxed));
        assert!(out_r.iter().all(|&x| x == 0.0), "muted track, pre={pre}");
    }

    // A muted bus keys the same silence.
    let mut t = ramp_track("t", &[]);
    t.output = Output::Bus(0);
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 6,
            on: true,
            sources: vec![src(1, KeyFrom::Bus(0), false, 1.0)],
        },
        &[],
    );
    let mut b = BusSpec::new("fx");
    b.mute = true;
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t), TrackSlot::New(r)],
        buses: vec![b],
        ..MixSpec::default()
    };
    let (_, out_r) = render(spec, 4, 256);
    assert!(had_key.load(Ordering::Relaxed));
    assert!(out_r.iter().all(|&x| x == 0.0), "muted bus");
}

/// A source that fills its block with one value.
struct Const(f32);

impl Processor for Const {
    fn name(&self) -> &str {
        "const"
    }

    fn prepare(&mut self, _: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        io.left.fill(self.0);
        io.right.fill(self.0);
    }
}

/// With no keys in the song the buses render in index order as before, so the
/// master's sums keep the old bit pattern exactly (ENG-078).
#[test]
fn a_song_without_keys_keeps_the_bus_order() {
    let at = |name: &str, v: f32, output| {
        let mut t = TrackSpec::new(name, Box::new(Const(v)));
        t.output = output;
        t
    };
    let bus = |name: &str, output| {
        let mut b = BusSpec::new(name);
        b.output = output;
        b
    };
    // t0 -> bus0 -> bus1 -> master; t1 -> bus2 -> master; t2 -> master.
    // Height order would put bus2 before bus1 and change the sum's last bits.
    let spec = MixSpec {
        tracks: vec![
            TrackSlot::New(at("t0", 16_777_216.0, Output::Bus(0))),
            TrackSlot::New(at("t1", 2.0, Output::Bus(2))),
            TrackSlot::New(at("t2", 1.0, Output::Master)),
        ],
        buses: vec![
            bus("b0", Output::Bus(1)),
            bus("b1", Output::Master),
            bus("b2", Output::Master),
        ],
        ..MixSpec::default()
    };
    let (l, _) = render(spec, 1, 256);
    let mut want = 0.0f32;
    want += 1.0; // t2, then the buses in index order
    want += 16_777_216.0;
    want += 2.0;
    assert_eq!(l[0].to_bits(), want.to_bits());
}

fn build_err(spec: MixSpec) -> PlanError {
    match Plan::build(spec, setup(), None) {
        Ok(_) => panic!("the plan built"),
        Err(e) => e,
    }
}

/// Validation: the insert index must be in the row and strictly rising, the
/// sources must be plan nodes, and keys must not loop.
#[test]
fn bad_keys_are_rejected() {
    let mut t = ramp_track("t", &[]);
    t.keys = vec![KeySpec {
        insert: 2,
        plugin: 1,
        on: true,
        sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
    }];
    let err = build_err(MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    });
    assert_eq!(err, PlanError::KeyInsert { node: 0, insert: 2 });

    let mut t = ramp_track("t", &[4]);
    t.keys = vec![KeySpec {
        insert: 0,
        plugin: 1,
        on: true,
        sources: vec![src(1, KeyFrom::Track(9), true, 1.0)],
    }];
    let err = build_err(MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    });
    assert_eq!(err, PlanError::UnknownKey { node: 0, from: 9 });

    // Two keys on one insert.
    let mut t = ramp_track("t", &[4]);
    t.keys = vec![
        KeySpec {
            insert: 0,
            plugin: 1,
            on: true,
            sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
        },
        KeySpec {
            insert: 0,
            plugin: 2,
            on: true,
            sources: vec![src(2, KeyFrom::Track(0), true, 1.0)],
        },
    ];
    let err = build_err(MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    });
    assert_eq!(err, PlanError::KeyInsert { node: 0, insert: 0 });

    // A loop: track 0 keys bus 0, which keys track 0.
    let mut t = ramp_track("t", &[4]);
    t.keys = vec![KeySpec {
        insert: 0,
        plugin: 1,
        on: true,
        sources: vec![src(1, KeyFrom::Bus(0), true, 1.0)],
    }];
    let mut b = BusSpec::new("b");
    b.inserts = vec![Box::new(Gain::new(1.0))];
    b.keys = vec![KeySpec {
        insert: 0,
        plugin: 2,
        on: true,
        sources: vec![src(2, KeyFrom::Track(0), true, 1.0)],
    }];
    let err = build_err(MixSpec {
        tracks: vec![TrackSlot::New(t)],
        buses: vec![b],
        ..MixSpec::default()
    });
    assert!(matches!(err, PlanError::KeyLoop { .. }));
}

/// Rendering a keyed mix on a worker pool is bit-identical to serial.
#[test]
fn parallel_is_identical() {
    let make = || {
        let a = ramp_track("a", &[24]);
        let b = ramp_track("b", &[8]);
        let (r, _, _) = keyed_track(
            "r",
            KeySpec {
                insert: 0,
                plugin: 1,
                on: true,
                sources: vec![
                    src(1, KeyFrom::Track(0), true, 0.7),
                    src(2, KeyFrom::Track(1), false, 0.3),
                ],
            },
            &[16],
        );
        let c = ramp_track("c", &[]);
        let d = ramp_track("d", &[48]);
        MixSpec {
            tracks: vec![
                TrackSlot::New(a),
                TrackSlot::New(b),
                TrackSlot::New(r),
                TrackSlot::New(c),
                TrackSlot::New(d),
            ],
            ..MixSpec::default()
        }
    };
    let blocks = 8;
    let n = 256;
    let run = |workers: usize| {
        let mut plan = Plan::build(make(), setup(), None).unwrap();
        if workers > 1 {
            plan.set_workers(workers).unwrap();
        }
        let mut l = vec![0.0; blocks * n];
        let mut r = vec![0.0; blocks * n];
        for b in 0..blocks {
            plan.process_at(
                &mut l[b * n..(b + 1) * n],
                &mut r[b * n..(b + 1) * n],
                Some((b * n) as u64),
            );
        }
        (l, r)
    };
    assert_eq!(run(1), run(4));
}

/// A sine keyed by a sine: the key path adds no clicks, and the insert sees
/// the source's signal, not its own.
#[test]
fn a_real_signal_keys_an_insert() {
    let s = TrackSpec::new("s", Box::new(Sine::new(220.0, 0.5)));
    let (r, had_key, _) = keyed_track(
        "r",
        KeySpec {
            insert: 0,
            plugin: 1,
            on: true,
            sources: vec![src(1, KeyFrom::Track(0), false, 1.0)],
        },
        &[],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(s), TrackSlot::New(r)],
        ..MixSpec::default()
    };
    let (l, out_r) = render(spec, 4, 256);
    assert!(had_key.load(Ordering::Relaxed));
    // The receiver's source is a ramp; its key is the sine.
    assert!(out_r.iter().any(|&x| x.abs() > 0.01));
    assert!(out_r.iter().zip(&l).any(|(k, m)| (*k - *m).abs() > 0.01));
}

/// A kick: loud for `long` frames every `every` frames from `first`, silent
/// between, counted on its own running frame so a beat lands on one sample.
struct Kick {
    frame: usize,
    first: usize,
    every: usize,
    long: usize,
}

impl Processor for Kick {
    fn name(&self) -> &str {
        "kick"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        for (l, r) in io.left.iter_mut().zip(io.right.iter_mut()) {
            let f = self.frame;
            let on = f >= self.first && (f - self.first) % self.every < self.long;
            *l = if on { 0.9 } else { 0.0 };
            *r = *l;
            self.frame += 1;
        }
    }
}

/// A steady pad, under the threshold of the Compressor below.
struct Pad;

impl Processor for Pad {
    fn name(&self) -> &str {
        "pad"
    }

    fn prepare(&mut self, _setup: &ProcessSetup) {}

    fn process(&mut self, io: &mut AudioIo<'_>, _events: &[NoteEvent]) {
        io.left.fill(0.05);
        io.right.fill(0.05);
    }
}

/// PLG-071: a stock Compressor on a pad, keyed by a kick on another track,
/// ducks the pad on every beat and only then. The kick's fader is down and the
/// key is its pre tap, so the master holds the pad alone.
#[test]
fn a_kick_on_another_track_ducks_the_compressor_on_its_beats() {
    const FIRST: usize = 700;
    const EVERY: usize = 24_000;
    const LONG: usize = 2_400;
    let run = |key: Option<bool>| {
        let mut kick = TrackSpec::new(
            "kick",
            Box::new(Kick {
                frame: 0,
                first: FIRST,
                every: EVERY,
                long: LONG,
            }),
        );
        kick.gain = 0.0;
        let (comp, h) = xirius_engine::stock::create("compressor").unwrap();
        for (id, v) in [
            ("threshold", -18.0),
            ("ratio", 4.0),
            ("knee", 0.0),
            ("attack", 1.0),
            ("release", 50.0),
        ] {
            let (_, spec) = h.def().param(id).unwrap();
            assert!(h.set(id, spec.to_norm(v)));
        }
        let mut pad = TrackSpec::new("pad", Box::new(Pad));
        pad.inserts = vec![comp];
        if let Some(on) = key {
            pad.keys = vec![KeySpec {
                insert: 0,
                plugin: 71,
                on,
                sources: vec![src(1, KeyFrom::Track(0), true, 1.0)],
            }];
        }
        let spec = MixSpec {
            tracks: vec![TrackSlot::New(kick), TrackSlot::New(pad)],
            ..MixSpec::default()
        };
        render(spec, 375, 256).0
    };
    let ducked = run(Some(true));
    let gr: Vec<f64> = ducked
        .iter()
        .map(|o| -20.0 * (f64::from(*o) / 0.05).log10())
        .collect();
    assert!(
        gr[..FIRST].iter().all(|g| g.abs() < 1e-5),
        "untouched before the first beat"
    );
    for b in (FIRST..ducked.len() - LONG - 2_400).step_by(EVERY) {
        assert!(gr[b - 1] < 0.01, "before beat {b}: {}", gr[b - 1]);
        assert!(gr[b] > 0.2, "on beat {b}: {}", gr[b]);
        assert!(
            gr[b + LONG - 1] > 12.0,
            "through beat {b}: {}",
            gr[b + LONG - 1]
        );
        assert!(gr[b + LONG + 2_400] < gr[b + LONG - 1] / 2.0);
    }
    // With the sidechain off, or no key at all, the pad comes through as it is.
    let off = run(Some(false));
    let none = run(None);
    assert!(
        off.iter()
            .zip(&none)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert!(none.iter().all(|x| (*x - 0.05).abs() < 1e-6));
}
