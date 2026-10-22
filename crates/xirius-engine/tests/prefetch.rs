//! The instrument and inserts of a track are rendered ahead by a worker (ENG-015): the audio is
//! the same as when the audio thread renders them, an edit or a track that takes live input
//! brings the chain home, and an export never prefetches.

use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{
    AudioIo, Command, MixSpec, NoteEvent, NoteKind, Plan, ProcessSetup, Processor, RenderMode,
    TimedNote, Timeline, TrackSlot, TrackSpec,
};

const RATE: f64 = 48_000.0;

fn setup(mode: RenderMode) -> ProcessSetup {
    ProcessSetup {
        sample_rate: RATE,
        max_block: 64,
        mode,
    }
}

/// A sine with an amplitude that dies away after the note-off: it keeps a phase and a level
/// from block to block, so it cannot be mixed ahead and thrown away.
struct Synth {
    phase: f64,
    step: f64,
    level: f32,
    held: bool,
}

impl Synth {
    fn new() -> Self {
        Self {
            phase: 0.0,
            step: 0.0,
            level: 0.0,
            held: false,
        }
    }
}

impl Processor for Synth {
    fn name(&self) -> &str {
        "synth"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let mut next = 0;
        for i in 0..io.frames() {
            while next < events.len() && events[next].offset as usize == i {
                match events[next].kind {
                    NoteKind::On { pitch, velocity } => {
                        let hz = 440.0 * 2f64.powf((pitch as f64 - 69.0) / 12.0);
                        self.step = std::f64::consts::TAU * hz / RATE;
                        self.level = velocity;
                        self.held = true;
                    }
                    NoteKind::Off { .. } => self.held = false,
                    NoteKind::Control { .. } => {}
                }
                next += 1;
            }
            if !self.held {
                self.level *= 0.9995;
            }
            let s = (self.phase.sin() as f32) * self.level;
            self.phase = (self.phase + self.step) % std::f64::consts::TAU;
            io.left[i] = s;
            io.right[i] = s * 0.5;
        }
    }
}

/// A one-pole low-pass: its state is the last output.
struct Smooth {
    last: [f32; 2],
}

impl Processor for Smooth {
    fn name(&self) -> &str {
        "smooth"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        for i in 0..io.frames() {
            self.last[0] += 0.2 * (io.left[i] - self.last[0]);
            self.last[1] += 0.2 * (io.right[i] - self.last[1]);
            io.left[i] = self.last[0];
            io.right[i] = self.last[1];
        }
    }
}

/// A level that is the held pitch: a note-off and nothing else puts it back to rest.
struct Hold {
    level: f32,
}

impl Processor for Hold {
    fn name(&self) -> &str {
        "hold"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let mut next = 0;
        for i in 0..io.frames() {
            while next < events.len() && events[next].offset as usize == i {
                match events[next].kind {
                    NoteKind::On { pitch, .. } => self.level = pitch as f32 / 127.0,
                    NoteKind::Off { .. } => self.level = 0.0,
                    NoteKind::Control { .. } => {}
                }
                next += 1;
            }
            io.left[i] = self.level;
            io.right[i] = self.level;
        }
    }
}

/// Panics on a prefetch worker, plays fine anywhere else.
struct Fragile;

impl Processor for Fragile {
    fn name(&self) -> &str {
        "fragile"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
        let on_worker = std::thread::current()
            .name()
            .is_some_and(|n| n.starts_with("xirius-ahead"));
        assert!(!on_worker, "this instrument does not like workers");
        io.left.fill(0.25);
        io.right.fill(0.25);
    }
}

fn on(pitch: u8) -> xirius_engine::NoteKind {
    NoteKind::On {
        pitch,
        velocity: 100.0 / 127.0,
    }
}

fn song() -> Timeline {
    let n = |frame, kind| TimedNote { frame, kind };
    Timeline::new(
        vec![
            n(100, on(60)),
            n(3_000, NoteKind::Off { pitch: 60 }),
            n(3_500, on(67)),
            n(9_000, NoteKind::Off { pitch: 67 }),
            n(9_500, on(72)),
        ],
        vec![],
    )
}

fn synth_track(prefetch: bool) -> TrackSpec {
    let mut t = TrackSpec::new("synth", Box::new(Synth::new()));
    t.inserts.push(Box::new(Smooth { last: [0.0; 2] }));
    t.timeline = song();
    t.prefetch = prefetch;
    t
}

fn plan(mode: RenderMode, track: TrackSpec) -> Plan {
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(track)],
        ..MixSpec::default()
    };
    Plan::build(spec, setup(mode), None).unwrap()
}

fn hold_track() -> TrackSpec {
    let n = |frame, kind| TimedNote { frame, kind };
    let mut t = TrackSpec::new("hold", Box::new(Hold { level: 0.0 }));
    t.timeline = Timeline::new(
        vec![n(640, on(60)), n(100_000, NoteKind::Off { pitch: 60 })],
        vec![],
    );
    t
}

/// Plays `callbacks` callbacks of `frames` frames; returns the left channel.
fn play(audio: &mut engine::AudioEngine, callbacks: usize, frames: usize, channel: &mut Vec<f32>) {
    for _ in 0..callbacks {
        let mut out = vec![0.0; frames * 2];
        audio.render(&mut out, 2);
        channel.extend(out.iter().step_by(2));
    }
}

fn started(track: TrackSpec, mode: RenderMode) -> (engine::AudioEngine, engine::Controller) {
    let (audio, mut control) = engine::create(plan(mode, track), EngineConfig::default());
    control.send(Command::Play).ok().unwrap();
    (audio, control)
}

#[test]
fn a_prefetched_track_sounds_the_same_sample_for_sample() {
    for frames in [256, 100] {
        let callbacks = 10_240 / frames;
        let (mut direct, _c) = started(synth_track(false), RenderMode::Realtime);
        let (mut ahead, _c2) = started(synth_track(true), RenderMode::Realtime);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        play(&mut direct, callbacks, frames, &mut a);
        play(&mut ahead, callbacks, frames, &mut b);
        assert_eq!(direct.prefetch(0), None, "switched off");
        let status = ahead.prefetch(0).expect("a prefetching track");
        assert!(status.lent, "{status:?}");
        assert!(status.rendered_frames >= 10_240, "{status:?}");
        assert_eq!(status.discards, 0, "nothing was edited: {status:?}");
        assert!(a.iter().any(|s| s.abs() > 0.01), "the song is not silent");
        assert_eq!(a.len(), b.len());
        let different = a.iter().zip(&b).filter(|(x, y)| x.to_bits() != y.to_bits());
        assert_eq!(different.count(), 0, "{frames} frames per callback");
    }
}

#[test]
fn the_export_is_the_same_with_or_without_prefetch() {
    // An export renders offline: no worker is used, whatever the track asks for.
    let render = |mode, prefetch| {
        let (mut audio, _c) = started(synth_track(prefetch), mode);
        let mut left = Vec::new();
        play(&mut audio, 160, 64, &mut left);
        (left, audio.prefetch(0))
    };
    let (off, none) = render(RenderMode::Offline, false);
    let (offline, status) = render(RenderMode::Offline, true);
    assert_eq!(none, None);
    assert_eq!(status, None, "an offline plan never prefetches");
    let (played, status) = render(RenderMode::Realtime, true);
    assert!(status.is_some_and(|s| s.lent));
    assert_eq!(off.len(), 10_240);
    for (name, other) in [("offline", &offline), ("played ahead", &played)] {
        let different = off
            .iter()
            .zip(other)
            .filter(|(x, y)| x.to_bits() != y.to_bits());
        assert_eq!(different.count(), 0, "{name} differs from the export");
    }
}

#[test]
fn a_track_kept_across_a_plan_swap_goes_on_without_a_break() {
    let go = |prefetch| {
        let (mut audio, mut control) = started(synth_track(prefetch), RenderMode::Realtime);
        let mut left = Vec::new();
        play(&mut audio, 40, 128, &mut left);
        control
            .swap(MixSpec {
                tracks: vec![TrackSlot::Keep(0)],
                ..MixSpec::default()
            })
            .unwrap();
        play(&mut audio, 40, 128, &mut left);
        (left, audio.prefetch(0))
    };
    let (direct, _) = go(false);
    let (ahead, status) = go(true);
    let status = status.unwrap();
    assert!(status.lent);
    assert_eq!(
        status.discards, 0,
        "the swap did not touch the track: {status:?}"
    );
    assert!(
        direct
            .iter()
            .zip(&ahead)
            .all(|(x, y)| x.to_bits() == y.to_bits())
    );
}

#[test]
fn an_edit_throws_the_prefetch_away_and_builds_it_again() {
    let (mut audio, mut control) = started(hold_track(), RenderMode::Realtime);
    let mut left = Vec::new();
    play(&mut audio, 20, 64, &mut left);
    assert!(
        (left[1_000] - 60.0 / 127.0).abs() < 1e-6,
        "the old note sounds"
    );
    let mut before = audio.prefetch(0).unwrap();
    for _ in 0..50 {
        if before.lent && before.queued_blocks + before.discards as usize > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
        before = audio.prefetch(0).unwrap();
    }
    assert!(before.lent && before.queued_blocks + before.discards as usize > 0);

    // The notes change while the transport runs: the old note ends, the new one starts at 2000.
    let n = |frame, kind| TimedNote { frame, kind };
    control
        .send(Command::SetTimeline {
            track: 0,
            timeline: Box::new(Timeline::new(vec![n(2_000, on(100))], vec![])),
        })
        .ok()
        .unwrap();
    let mut after = Vec::new();
    play(&mut audio, 40, 64, &mut after);
    let status = audio.prefetch(0).unwrap();
    assert!(status.discards >= 1, "{status:?}");
    assert!(status.lent, "it is lent again: {status:?}");
    let new_start = 2_000 - 1_280;
    assert!(
        after[..new_start].iter().all(|&s| s == 0.0),
        "the old note is gone at once"
    );
    assert!(
        after[new_start..]
            .iter()
            .all(|&s| (s - 100.0 / 127.0).abs() < 1e-6),
        "the new note is where the edit put it"
    );
}

#[test]
fn a_seek_and_a_stop_bring_the_chain_home() {
    let (mut audio, mut control) = started(hold_track(), RenderMode::Realtime);
    let mut left = Vec::new();
    play(&mut audio, 30, 64, &mut left);
    assert!(audio.prefetch(0).unwrap().lent);
    control.send(Command::Seek(0)).ok().unwrap();
    let mut again = Vec::new();
    play(&mut audio, 30, 64, &mut again);
    assert!(audio.prefetch(0).unwrap().discards >= 1);
    // The song starts over: the note comes at the same frame as the first time.
    assert_eq!(&left[..1_900], &again[..1_900]);
    control.send(Command::Stop).ok().unwrap();
    let mut stopped = Vec::new();
    play(&mut audio, 4, 64, &mut stopped);
    assert!(
        !audio.prefetch(0).unwrap().lent,
        "a stopped transport renders at home"
    );
    assert!(stopped.iter().all(|&s| s == 0.0));
}

#[test]
fn a_live_track_is_not_prefetched_until_it_is_not_live() {
    let mut track = hold_track();
    track.live = true;
    let (mut audio, mut control) = started(track, RenderMode::Realtime);
    let mut left = Vec::new();
    play(&mut audio, 30, 64, &mut left);
    let status = audio.prefetch(0).unwrap();
    assert!(!status.lent, "{status:?}");
    assert_eq!(status.rendered_frames, 0);
    assert!(
        (left[1_000] - 60.0 / 127.0).abs() < 1e-6,
        "it plays all the same"
    );

    control
        .send(Command::TrackLive {
            track: 0,
            live: false,
        })
        .ok()
        .unwrap();
    play(&mut audio, 10, 64, &mut left);
    assert!(audio.prefetch(0).unwrap().lent, "not armed any more");

    control
        .send(Command::TrackLive {
            track: 0,
            live: true,
        })
        .ok()
        .unwrap();
    play(&mut audio, 1, 64, &mut left);
    let status = audio.prefetch(0).unwrap();
    assert!(
        !status.lent && status.discards >= 1,
        "armed again: {status:?}"
    );
}

#[test]
fn a_note_played_by_hand_takes_the_track_off_the_workers_until_the_transport_moves() {
    let (mut audio, mut control) = started(hold_track(), RenderMode::Realtime);
    let mut left = Vec::new();
    play(&mut audio, 20, 64, &mut left);
    assert!(audio.prefetch(0).unwrap().lent);
    control
        .send(Command::Note {
            track: 0,
            kind: on(30),
        })
        .ok()
        .unwrap();
    let mut heard = Vec::new();
    play(&mut audio, 1, 64, &mut heard);
    assert!(
        heard.iter().all(|&s| (s - 30.0 / 127.0).abs() < 1e-6),
        "the note sounds in the very next block"
    );
    let status = audio.prefetch(0).unwrap();
    assert!(status.hot && !status.lent, "{status:?}");
    play(&mut audio, 20, 64, &mut heard);
    assert!(!audio.prefetch(0).unwrap().lent, "still at home while hot");

    control.send(Command::Seek(0)).ok().unwrap();
    play(&mut audio, 4, 64, &mut heard);
    let status = audio.prefetch(0).unwrap();
    assert!(!status.hot && status.lent, "a jump ends it: {status:?}");
}

#[test]
fn a_chain_that_panics_on_a_worker_goes_on_on_the_audio_thread() {
    let (mut audio, _c) = started(
        TrackSpec::new("fragile", Box::new(Fragile)),
        RenderMode::Realtime,
    );
    let mut left = Vec::new();
    play(&mut audio, 40, 64, &mut left);
    assert!(left.iter().all(|&s| s == 0.25), "every block is there");
    let status = audio.prefetch(0).unwrap();
    assert!(status.failed && !status.lent, "{status:?}");
}

#[test]
fn a_track_that_is_a_pure_timeline_or_has_no_prefetch_asked_is_never_lent() {
    struct Plain;
    impl Processor for Plain {
        fn name(&self) -> &str {
            "plain"
        }
        fn prepare(&mut self, _: &ProcessSetup) {}
        fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
            io.clear();
        }
        fn timeline_only(&self) -> bool {
            true
        }
    }
    let (audio, _c) = started(
        TrackSpec::new("audio", Box::new(Plain)),
        RenderMode::Realtime,
    );
    assert_eq!(audio.prefetch(0), None, "its own cache serves it");
}
