//! The transport and the timelines: notes and audio land on the right frames, the loop and the end
//! cut blocks where they should, held notes are released, and buses can be muted and chained.

use std::sync::{Arc, Mutex};

use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{
    AudioData, AudioIo, BusSpec, CTRL_PITCH_BEND, Command, EngineEvent, MixSpec, NoteEvent,
    NoteKind, Output, Plan, ProcessSetup, Processor, Region, RenderMode, TimedNote, Timeline,
    TrackSlot, TrackSpec,
};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};

type Log = Arc<Mutex<Vec<(u64, NoteKind)>>>;

/// Silent; writes every note it receives into `log` with the engine frame it arrived at.
struct Recorder {
    frames: u64,
    log: Log,
}

impl Processor for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        io.clear();
        let mut log = self.log.lock().unwrap();
        for e in events {
            log.push((self.frames + e.offset as u64, e.kind));
        }
        self.frames += io.frames() as u64;
    }
}

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

fn on(frame: u64, pitch: u8) -> TimedNote {
    TimedNote {
        frame,
        kind: NoteKind::On {
            pitch,
            velocity: 1.0,
        },
    }
}

fn off(frame: u64, pitch: u8) -> TimedNote {
    TimedNote {
        frame,
        kind: NoteKind::Off { pitch },
    }
}

fn recorder_plan(notes: Vec<TimedNote>) -> (Plan, Log) {
    let log: Log = Arc::default();
    let mut t = TrackSpec::new(
        "rec",
        Box::new(Recorder {
            frames: 0,
            log: log.clone(),
        }),
    );
    t.timeline = Timeline::new(notes, vec![]);
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    (Plan::build(spec, SETUP, None).unwrap(), log)
}

fn render(audio: &mut engine::AudioEngine, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut out = vec![0.0; frames * 2];
    // Odd sizes on purpose: the callback size has nothing to do with the block size.
    for chunk in out.chunks_mut(2 * 100) {
        audio.render(chunk, 2);
    }
    (
        out.iter().step_by(2).copied().collect(),
        out.iter().skip(1).step_by(2).copied().collect(),
    )
}

fn send(c: &mut engine::Controller, cmd: Command) {
    assert!(c.send(cmd).is_ok());
}

#[test]
fn stopped_transport_plays_nothing() {
    let (plan, log) = recorder_plan(vec![on(10, 60), off(50, 60)]);
    let (mut audio, _c) = engine::create(plan, EngineConfig::default());
    render(&mut audio, 300);
    assert!(log.lock().unwrap().is_empty());
    assert!(!audio.is_playing());
    assert_eq!(audio.song_position(), 0);
}

#[test]
fn notes_land_on_their_frames_across_blocks() {
    let (plan, log) = recorder_plan(vec![on(10, 60), off(70, 60), on(130, 62), off(1000, 62)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Play);
    render(&mut audio, 1200);
    let got: Vec<u64> = log.lock().unwrap().iter().map(|e| e.0).collect();
    assert_eq!(got, vec![10, 70, 130, 1000]);
    assert!(audio.is_playing());
    assert_eq!(audio.song_position(), 1200);
}

#[test]
fn seek_moves_the_song_position() {
    let (plan, log) = recorder_plan(vec![on(100, 60), on(1100, 61), off(1150, 61)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Seek(1000));
    send(&mut c, Command::Play);
    render(&mut audio, 300);
    let got: Vec<u64> = log.lock().unwrap().iter().map(|e| e.0).collect();
    // The note at song frame 1100 is 100 frames after the start of the render.
    assert_eq!(got, vec![100, 150]);
    assert_eq!(audio.song_position(), 1300);
}

#[test]
fn a_seek_acknowledgement_precedes_meters_when_the_event_queue_is_full() {
    let (plan, _) = recorder_plan(vec![]);
    let (mut audio, mut c) = engine::create(
        plan,
        EngineConfig {
            event_capacity: 1,
            meter_interval: std::time::Duration::ZERO,
            ..EngineConfig::default()
        },
    );
    render(&mut audio, 64);
    send(
        &mut c,
        Command::SeekTracked {
            frame: 1000,
            serial: 7,
        },
    );
    render(&mut audio, 64);
    c.poll(|_| {});
    render(&mut audio, 64);
    let mut ack = None;
    c.poll(|event| {
        if let EngineEvent::Transport {
            position, serial, ..
        } = event
        {
            ack = Some((position, serial));
        }
    });
    assert_eq!(ack, Some((1000, 7)));

    send(&mut c, Command::Seek(2000));
    render(&mut audio, 64);
    c.poll(|event| {
        if let EngineEvent::Transport {
            position, serial, ..
        } = event
        {
            ack = Some((position, serial));
        }
    });
    assert_eq!(ack, Some((2000, 7)));
}

#[test]
fn loop_wraps_and_notes_repeat() {
    let (plan, log) = recorder_plan(vec![on(150, 60), off(160, 60)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::SetLoop(Some((100, 300))));
    send(&mut c, Command::Play);
    render(&mut audio, 1000);
    let ons: Vec<u64> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e.1, NoteKind::On { .. }))
        .map(|e| e.0)
        .collect();
    // Song 0..300 once, then 100..300 over and over: the note at 150 comes back every 200 frames.
    assert_eq!(ons, vec![150, 350, 550, 750, 950]);
    // 1000 frames = 300 + 3×200 + 100, so the position is 100 into the loop body.
    assert_eq!(audio.song_position(), 200);
}

#[test]
fn the_end_stops_the_transport_and_reports_it() {
    let (plan, _log) = recorder_plan(vec![]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::SetEnd(Some(250)));
    send(&mut c, Command::Play);
    render(&mut audio, 500);
    assert!(!audio.is_playing());
    assert_eq!(audio.song_position(), 250);
    let mut last = None;
    c.poll(|e| {
        if let EngineEvent::Transport {
            playing, position, ..
        } = e
        {
            last = Some((playing, position));
        }
    });
    assert_eq!(last, Some((false, 250)));
}

#[test]
fn stop_releases_the_notes_that_sound() {
    let (plan, log) = recorder_plan(vec![on(10, 60), on(20, 64), off(5000, 60), off(5000, 64)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Play);
    render(&mut audio, 100);
    send(&mut c, Command::Stop);
    render(&mut audio, 100);
    let log = log.lock().unwrap();
    let offs: Vec<(u64, u8)> = log
        .iter()
        .filter_map(|e| match e.1 {
            NoteKind::Off { pitch } => Some((e.0, pitch)),
            _ => None,
        })
        .collect();
    // Both are released at the start of the first block after the stop (render 2 starts at 100;
    // the engine blocks are 64 frames, so that block starts at frame 128).
    assert_eq!(offs.len(), 2);
    assert!(offs.iter().all(|o| o.0 >= 100 && o.0 < 200));
    assert_eq!(offs[0].1, 60);
    assert_eq!(offs[1].1, 64);
}

#[test]
fn the_loop_wrap_releases_held_notes() {
    let (plan, log) = recorder_plan(vec![on(150, 60)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::SetLoop(Some((100, 200))));
    send(&mut c, Command::Play);
    render(&mut audio, 260);
    let log = log.lock().unwrap();
    let off_at = log.iter().find(|e| matches!(e.1, NoteKind::Off { .. }));
    // The note started at 150 and the loop ends at 200: it is released at the wrap.
    assert_eq!(off_at.map(|e| e.0), Some(200));
}

#[test]
fn audio_regions_play_at_their_place() {
    let data = Arc::new(AudioData {
        left: vec![0.5; 100],
        right: vec![0.25; 100],
    });
    let mut t = TrackSpec::new("audio", Box::new(Silence));
    t.timeline = Timeline::new(
        vec![],
        vec![Region {
            start: 50,
            len: 100,
            offset: 0,
            gain: 1.0,
            data,
            shape: Default::default(),
        }],
    );
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        ..MixSpec::default()
    };
    let (mut audio, mut c) =
        engine::create(Plan::build(spec, SETUP, None).unwrap(), Default::default());
    send(&mut c, Command::Play);
    let (l, r) = render(&mut audio, 300);
    for i in 0..300 {
        let (wl, wr) = if (50..150).contains(&i) {
            (0.5, 0.25)
        } else {
            (0.0, 0.0)
        };
        assert_eq!((l[i], r[i]), (wl, wr), "frame {i}");
    }
}

#[test]
fn set_timeline_replaces_and_hands_the_old_one_back() {
    let (plan, log) = recorder_plan(vec![on(10, 60)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(
        &mut c,
        Command::SetTimeline {
            track: 0,
            timeline: Box::new(Timeline::new(vec![on(20, 72)], vec![])),
        },
    );
    send(&mut c, Command::Play);
    render(&mut audio, 100);
    c.poll(|_| {});
    let got = log.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, 20);
    assert_eq!(audio.leaked_plans(), 0);
}

#[test]
fn bus_mute_silences_the_bus_and_its_sends() {
    let mut t = TrackSpec::new("t", Box::new(Dc(1.0)));
    t.output = Output::Bus(0);
    let mut bus = BusSpec::new("group");
    bus.mute = true;
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        buses: vec![bus],
        ..MixSpec::default()
    };
    let (mut audio, mut c) =
        engine::create(Plan::build(spec, SETUP, None).unwrap(), Default::default());
    let (l, _) = render(&mut audio, 200);
    assert!(l.iter().all(|s| *s == 0.0));
    send(
        &mut c,
        Command::BusMute {
            bus: 0,
            mute: false,
        },
    );
    render(&mut audio, 100); // the fade
    let (l, _) = render(&mut audio, 200);
    assert!(l.iter().all(|s| (*s - 1.0).abs() < 1e-6));
}

#[test]
fn a_bus_can_feed_another_bus_by_output_and_by_send() {
    use xirius_engine::SendSpec;
    let mut t = TrackSpec::new("t", Box::new(Dc(1.0)));
    t.output = Output::Bus(0);
    let mut a = BusSpec::new("a");
    a.gain = 0.5;
    a.output = Output::Bus(1);
    a.sends = vec![SendSpec {
        bus: 2,
        level: 0.5,
        pre_fader: false,
    }];
    let mut b = BusSpec::new("b");
    b.gain = 0.5;
    let mut fx = BusSpec::new("fx");
    fx.gain = 1.0;
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t)],
        buses: vec![a, b, fx],
        ..MixSpec::default()
    };
    let (mut audio, _c) =
        engine::create(Plan::build(spec, SETUP, None).unwrap(), Default::default());
    let (l, r) = render(&mut audio, 200);
    // a = 0.5; b = 0.5 × 0.5 = 0.25; fx = 0.5 × 0.5 = 0.25; master = b + fx.
    assert!(l.iter().all(|s| (*s - 0.5).abs() < 1e-6), "{:?}", &l[..4]);
    assert!(r.iter().all(|s| (*s - 0.5).abs() < 1e-6));
}

#[test]
fn bus_send_level_can_be_changed() {
    use xirius_engine::SendSpec;
    let mut t = TrackSpec::new("t", Box::new(Dc(1.0)));
    t.mute = false;
    t.gain = 0.0;
    t.output = Output::Bus(0);
    let mut a = BusSpec::new("a");
    a.sends = vec![SendSpec {
        bus: 1,
        level: 0.0,
        pre_fader: true,
    }];
    a.mute = false;
    let fx = BusSpec::new("fx");
    let mut t2 = TrackSpec::new("t2", Box::new(Dc(1.0)));
    t2.output = Output::Bus(0);
    let spec = MixSpec {
        tracks: vec![TrackSlot::New(t), TrackSlot::New(t2)],
        buses: vec![a, fx],
        ..MixSpec::default()
    };
    let (mut audio, mut c) =
        engine::create(Plan::build(spec, SETUP, None).unwrap(), Default::default());
    // Bus a (1.0) reaches the master directly; its send to fx is 0.
    let (l, _) = render(&mut audio, 128);
    assert!(l.iter().all(|s| (*s - 1.0).abs() < 1e-6));
    send(
        &mut c,
        Command::BusSendLevel {
            bus: 0,
            send: 0,
            level: 0.5,
        },
    );
    render(&mut audio, 128);
    let (l, _) = render(&mut audio, 128);
    assert!(l.iter().all(|s| (*s - 1.5).abs() < 1e-6));
}

fn ctl(frame: u64, number: u8, value: i32) -> TimedNote {
    TimedNote {
        frame,
        kind: NoteKind::control(number, value),
    }
}

fn controls_of(log: &Log) -> Vec<(u64, u8, i32)> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e.1 {
            NoteKind::Control { number, value } => Some((e.0, number, value)),
            _ => None,
        })
        .collect()
}

#[test]
fn controller_changes_land_on_their_frames_between_the_notes() {
    let (plan, log) = recorder_plan(vec![
        on(100, 60),
        ctl(100, 64, 127),
        ctl(30, CTRL_PITCH_BEND, 4000),
        off(100, 59),
        ctl(500, 64, 0),
    ]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Play);
    render(&mut audio, 600);
    assert_eq!(
        controls_of(&log),
        vec![(30, CTRL_PITCH_BEND, 4000), (100, 64, 127), (500, 64, 0)]
    );
    // At one frame: the off, then the controller, then the on.
    let at_100: Vec<NoteKind> = log
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.0 == 100)
        .map(|e| e.1)
        .collect();
    assert!(matches!(at_100[0], NoteKind::Off { .. }));
    assert!(matches!(at_100[1], NoteKind::Control { .. }));
    assert!(matches!(at_100[2], NoteKind::On { .. }));
}

#[test]
fn a_seek_makes_the_controllers_take_their_values_at_the_new_place() {
    let (plan, log) = recorder_plan(vec![
        ctl(100, 64, 127),
        ctl(200, 1, 90),
        ctl(300, 64, 0),
        ctl(1500, 1, 10),
    ]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Seek(400));
    send(&mut c, Command::Play);
    render(&mut audio, 200);
    // Nothing is replayed in order: each number gets its last value before 400, at the start.
    let got = controls_of(&log);
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|g| g.0 < 64));
    assert!(got.contains(&(got[0].0, 64, 0)));
    assert!(got.iter().any(|g| g.1 == 1 && g.2 == 90));
}

#[test]
fn stop_puts_the_moved_controllers_back_to_rest() {
    let (plan, log) = recorder_plan(vec![
        ctl(10, 64, 127),
        ctl(10, CTRL_PITCH_BEND, 2000),
        ctl(10, 11, 40),
        ctl(5000, 64, 0),
    ]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::Play);
    render(&mut audio, 100);
    send(&mut c, Command::Stop);
    render(&mut audio, 100);
    let got = controls_of(&log);
    let after: Vec<_> = got.iter().filter(|g| g.0 >= 100).collect();
    assert_eq!(after.len(), 3);
    for (number, rest) in [(64, 0), (CTRL_PITCH_BEND, 0), (11, 127)] {
        assert!(
            after.iter().any(|g| g.1 == number && g.2 == rest),
            "number {number}"
        );
    }
}

#[test]
fn the_loop_wrap_chases_the_controllers_at_the_loop_start() {
    let (plan, log) = recorder_plan(vec![ctl(50, 64, 127), ctl(150, 64, 0)]);
    let (mut audio, mut c) = engine::create(plan, EngineConfig::default());
    send(&mut c, Command::SetLoop(Some((100, 200))));
    send(&mut c, Command::Play);
    render(&mut audio, 300);
    let got = controls_of(&log);
    // 0..100 plays the on at 50; the loop is entered at 100 with the pedal still down; the
    // off at 150 comes each round.
    assert_eq!(got[0], (50, 64, 127));
    assert!(got.iter().filter(|g| g.2 == 0).count() >= 2);
}
