//! A track that plays to a MIDI port (ENG-028): the session opens the port the song names, the
//! track makes no sound of its own, and its notes reach the port. The ports are a stand-in; no
//! MIDI device is opened.

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_engine::AudioEngine;
use xirius_engine::midi_out::{MidiOutput, MockLog, MockMidiOut, Sent};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::host::{HostConfig, SessionHost};
use xirius_session::midi_out::MidiOutMaker;

fn song() -> Session {
    let mut s = Session::with_registry(Project::new("midi"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    let args = match args {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    commands::run(s, None, name, &args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one long note of `pitch`.
fn voice(s: &mut Session, pitch: i64) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": "A" })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1.0 }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": pitch, "length": 3840, "velocity": 100 }),
    );
    t
}

fn to_port(s: &mut Session, t: &str, to: &str, channel: i64) {
    run(
        s,
        "track.output.set",
        json!({ "ids": [t], "to": to, "channel": channel }),
    );
}

/// What a stand-in maker was asked for, and the log of the output it made last.
#[derive(Clone, Default)]
struct Spy {
    asked: Arc<Mutex<Vec<Vec<String>>>>,
    log: Arc<Mutex<Option<MockLog>>>,
}

impl Spy {
    fn maker(&self, ports: &'static [&'static str]) -> MidiOutMaker {
        let spy = self.clone();
        Box::new(move |wanted, _rate| {
            spy.asked.lock().unwrap().push(wanted.to_vec());
            let (out, log) = MockMidiOut::new(ports);
            *spy.log.lock().unwrap() = Some(log);
            Ok(Box::new(out) as Box<dyn MidiOutput>)
        })
    }

    fn asked(&self) -> Vec<Vec<String>> {
        self.asked.lock().unwrap().clone()
    }

    fn sent(&self) -> Vec<Sent> {
        self.log
            .lock()
            .unwrap()
            .as_mut()
            .map(MockLog::take)
            .unwrap_or_default()
    }
}

struct Rig {
    host: SessionHost,
    audio: AudioEngine,
}

impl Rig {
    fn new(s: &Session) -> Rig {
        let (host, audio, _) = SessionHost::start(s.project(), HostConfig::default()).unwrap();
        Rig { host, audio }
    }

    /// Plays `frames` from the start and returns what was heard.
    fn play(&mut self, frames: usize) -> f64 {
        self.host.seek_frame(0);
        self.host.play();
        let mut peak = 0.0f64;
        let mut out = vec![0.0f32; 960];
        for _ in 0..frames / 480 {
            self.audio.render(&mut out, 2);
            peak = out.iter().fold(peak, |m, x| m.max(f64::from(x.abs())));
        }
        self.host.stop();
        self.audio.render(&mut out, 2);
        peak
    }
}

const PORTS: &[&str] = &["Loop A", "Loop B"];

#[test]
fn the_notes_of_a_track_reach_its_port_and_the_track_is_silent() {
    let mut s = song();
    let t = voice(&mut s, 64);
    // Heard through the built-in instrument first.
    let mut rig = Rig::new(&s);
    assert!(rig.play(4800) > 0.0);

    to_port(&mut s, &t, "midi:Loop B", 3);
    let spy = Spy::default();
    assert!(rig.host.set_midi_out(Some(spy.maker(PORTS))).is_empty());
    let report = rig.host.sync(s.project());
    assert!(report.swapped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(spy.asked(), vec![vec!["Loop B".to_string()]]);
    assert_eq!(rig.host.midi_out_ports(), vec!["Loop B".to_string()]);

    assert_eq!(rig.play(4800), 0.0, "the track makes no sound of its own");
    let sent = spy.sent();
    assert!(
        sent.iter().any(|m| m.port == 1 && m.msg == [0x92, 64, 100]),
        "{sent:?}"
    );
    assert!(sent.iter().all(|m| m.port == 1 && m.msg[0] & 0x0f == 2));
    // Stopping let go of the note.
    assert!(sent.iter().any(|m| m.msg[0] == 0x82 && m.msg[1] == 64));
}

#[test]
fn a_port_that_is_not_there_is_named_and_the_track_stays_silent() {
    let mut s = song();
    let t = voice(&mut s, 60);
    to_port(&mut s, &t, "midi:Gone", 1);
    let mut rig = Rig::new(&s);
    let spy = Spy::default();
    let warnings = rig.host.set_midi_out(Some(spy.maker(PORTS)));
    assert_eq!(
        warnings,
        vec!["MIDI 出力「Gone」が見つかりません".to_string()]
    );
    assert_eq!(rig.play(4800), 0.0);
    assert!(spy.sent().is_empty());
}

#[test]
fn without_a_way_to_open_ports_the_warning_is_given_once() {
    let mut s = song();
    let t = voice(&mut s, 60);
    to_port(&mut s, &t, "midi:Loop A", 1);
    let mut rig = Rig::new(&s);
    let warnings = rig.host.set_midi_out(None);
    assert_eq!(warnings, vec!["MIDI 出力を開けません: Loop A".to_string()]);
    // Edits that leave the ports as they are do not say it again.
    run(&mut s, "track.rename", json!({ "id": t, "name": "B" }));
    let report = rig.host.sync(s.project());
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(rig.play(4800), 0.0);
}

#[test]
fn a_new_port_is_opened_on_the_edit_and_an_audio_output_gives_it_up() {
    let mut s = song();
    let t = voice(&mut s, 60);
    to_port(&mut s, &t, "midi:Loop A", 1);
    let spy = Spy::default();
    let mut rig = Rig::new(&s);
    rig.host.set_midi_out(Some(spy.maker(PORTS)));
    assert_eq!(rig.play(2400), 0.0);
    assert!(!spy.sent().is_empty());

    // The same port again: nothing to open.
    let report = rig.host.sync(s.project());
    assert!(report.warnings.is_empty());
    assert_eq!(spy.asked().len(), 1);

    to_port(&mut s, &t, "midi:Loop B", 1);
    rig.host.sync(s.project());
    assert_eq!(
        spy.asked(),
        vec![vec!["Loop A".to_string()], vec!["Loop B".to_string()]]
    );
    assert_eq!(rig.play(2400), 0.0);
    assert!(spy.sent().iter().all(|m| m.port == 1));

    // Back to the master: the output goes, and the instrument sounds again.
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [t], "to": "master" }),
    );
    rig.host.sync(s.project());
    assert!(rig.host.midi_out_ports().is_empty());
    assert!(rig.play(4800) > 0.0);
    assert_eq!(spy.asked().len(), 2);
}
