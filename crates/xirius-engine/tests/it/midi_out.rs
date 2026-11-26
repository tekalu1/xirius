//! A track whose output is a MIDI port plays its notes and controllers to it (ENG-028). The port
//! is a mock: no device is opened.

use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::midi_out::{MockLog, MockMidiOut, Sent};
use xirius_engine::{
    AudioIo, CTRL_PITCH_BEND, Command, MidiTarget, MixSpec, NoteEvent, NoteKind, Plan,
    ProcessSetup, Processor, RenderMode, TimedNote, Timeline, TrackSlot, TrackSpec,
};

const SETUP: ProcessSetup = ProcessSetup {
    sample_rate: 48_000.0,
    max_block: 64,
    mode: RenderMode::Offline,
};

/// What an instrument that has gone to a MIDI port leaves behind.
struct Quiet;

impl Processor for Quiet {
    fn name(&self) -> &str {
        "quiet"
    }
    fn prepare(&mut self, _: &ProcessSetup) {}
    fn process(&mut self, _: &mut AudioIo<'_>, _: &[NoteEvent]) {}
    fn timeline_only(&self) -> bool {
        true
    }
}

fn note(frame: u64, kind: NoteKind) -> TimedNote {
    TimedNote { frame, kind }
}

fn on(pitch: u8) -> NoteKind {
    NoteKind::On {
        pitch,
        velocity: 100.0 / 127.0,
    }
}

fn track(port: &str, channel: u8, notes: Vec<TimedNote>) -> TrackSpec {
    let mut t = TrackSpec::new("midi", Box::new(Quiet));
    t.midi = Some(Box::new(MidiTarget {
        port: port.into(),
        channel,
    }));
    t.timeline = Timeline::new(notes, vec![]);
    t
}

fn plan(tracks: Vec<TrackSpec>) -> Plan {
    let spec = MixSpec {
        tracks: tracks.into_iter().map(TrackSlot::New).collect(),
        ..MixSpec::default()
    };
    Plan::build(spec, SETUP, None).unwrap()
}

fn render(audio: &mut engine::AudioEngine, frames: usize) {
    let mut out = vec![0.0; frames * 2];
    audio.render(&mut out, 2);
}

fn started(
    tracks: Vec<TrackSpec>,
    ports: &[&str],
) -> (engine::AudioEngine, engine::Controller, MockLog) {
    let (mut audio, mut control) = engine::create(plan(tracks), EngineConfig::default());
    let (out, log) = MockMidiOut::new(ports);
    control
        .send(Command::SetMidiOutput(Some(Box::new(out))))
        .ok()
        .unwrap();
    control.send(Command::Play).ok().unwrap();
    render(&mut audio, 64);
    (audio, control, log)
}

#[test]
fn notes_and_controllers_reach_the_port_at_their_frame() {
    let notes = vec![
        note(100, on(60)),
        note(200, NoteKind::Off { pitch: 60 }),
        note(150, NoteKind::control(64, 127)),
        note(160, NoteKind::control(CTRL_PITCH_BEND, 0)),
    ];
    let (mut audio, _c, mut log) = started(vec![track("Synth", 2, notes)], &["Other", "Synth"]);
    // The first block was 0..64 (nothing yet); the rest runs 64..320 in one callback.
    assert!(log.take().is_empty());
    render(&mut audio, 256);
    let got = log.take();
    // Offsets are frames into the callback: song frame minus the 64 already played.
    assert_eq!(
        got,
        vec![
            Sent {
                port: 1,
                offset: 100 - 64,
                msg: [0x92, 60, 100]
            },
            Sent {
                port: 1,
                offset: 150 - 64,
                msg: [0xb2, 64, 127]
            },
            Sent {
                port: 1,
                offset: 160 - 64,
                msg: [0xe2, 0, 64]
            },
            Sent {
                port: 1,
                offset: 200 - 64,
                msg: [0x82, 60, 0]
            },
        ]
    );
}

#[test]
fn a_hand_played_note_goes_to_the_port_too() {
    let (mut audio, mut control, mut log) = started(vec![track("A", 0, vec![])], &["A"]);
    control
        .send(Command::Note {
            track: 0,
            kind: on(64),
        })
        .ok()
        .unwrap();
    render(&mut audio, 64);
    assert_eq!(
        log.take(),
        vec![Sent {
            port: 0,
            offset: 0,
            msg: [0x90, 64, 100]
        }]
    );
}

#[test]
fn a_port_that_is_not_there_gets_nothing_and_the_audio_still_runs() {
    let (mut audio, _c, mut log) =
        started(vec![track("Gone", 0, vec![note(10, on(60))])], &["Other"]);
    render(&mut audio, 128);
    assert!(log.take().is_empty());
}

#[test]
fn stopping_lets_every_sounding_note_go() {
    let (mut audio, mut control, mut log) = started(
        vec![track("A", 0, vec![note(70, on(60)), note(80, on(64))])],
        &["A"],
    );
    render(&mut audio, 64);
    let sounding = log.take();
    assert_eq!(sounding.len(), 2);
    control.send(Command::Stop).ok().unwrap();
    render(&mut audio, 64);
    let released = log.take();
    let mut pitches: Vec<u8> = released
        .iter()
        .filter(|s| s.msg[0] == 0x80)
        .map(|s| s.msg[1])
        .collect();
    pitches.sort_unstable();
    assert_eq!(pitches, vec![60, 64]);
}

#[test]
fn a_track_taken_out_by_a_new_plan_lets_its_notes_go() {
    let (mut audio, mut control, mut log) =
        started(vec![track("A", 0, vec![note(70, on(60))])], &["A"]);
    render(&mut audio, 64);
    assert_eq!(log.take().len(), 1);
    // The new plan has the track again, but built fresh (not kept): the old one's notes end.
    control
        .swap(MixSpec {
            tracks: vec![TrackSlot::New(track("A", 0, vec![]))],
            ..MixSpec::default()
        })
        .unwrap();
    render(&mut audio, 64);
    let got = log.take();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].msg, [0x80, 60, 0]);
}

#[test]
fn a_kept_track_keeps_its_port_and_its_notes_across_a_swap() {
    let (mut audio, mut control, mut log) =
        started(vec![track("A", 0, vec![note(70, on(60))])], &["A"]);
    render(&mut audio, 64);
    log.take();
    control
        .swap(MixSpec {
            tracks: vec![TrackSlot::Keep(0)],
            ..MixSpec::default()
        })
        .unwrap();
    render(&mut audio, 64);
    assert!(log.take().is_empty(), "nothing ends, nothing restarts");
}

#[test]
fn the_audio_thread_allocates_nothing_while_sending() {
    // assert_no_alloc runs inside render; a lot of messages through it must not trip it.
    let notes: Vec<TimedNote> = (0..200)
        .map(|i| note(i * 3, on(40 + (i % 40) as u8)))
        .collect();
    let (mut audio, _c, mut log) = started(vec![track("A", 0, notes)], &["A"]);
    render(&mut audio, 1024);
    assert!(!log.take().is_empty());
}
