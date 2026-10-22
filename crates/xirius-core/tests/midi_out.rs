//! A track's output can be a MIDI port (ENG-028): `track.output.set --to midi:<port>`.

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::error::ErrorCode;
use xirius_core::id::TrackId;
use xirius_core::model::MidiOut;
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn add(s: &mut Session, kind: &str) -> String {
    let r = run(s, kind, json!({}));
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .expect("an id")
        .to_string()
}

fn midi_out(s: &Session, id: &str) -> Option<MidiOut> {
    let id: TrackId = id.parse().unwrap();
    s.project().track(id).unwrap().midi_out.clone()
}

#[test]
fn an_instrument_track_can_send_to_a_midi_port_and_back() {
    let mut s = Session::new("midi");
    let t = add(&mut s, "track.add.instrument");
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "midi:Loop MIDI"}),
    );
    assert_eq!(
        midi_out(&s, &t),
        Some(MidiOut {
            port: "Loop MIDI".into(),
            channel: 1
        })
    );
    // A channel is kept when only the port changes.
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "midi:Loop MIDI", "channel": 5}),
    );
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "midi:Synth"}),
    );
    assert_eq!(
        midi_out(&s, &t),
        Some(MidiOut {
            port: "Synth".into(),
            channel: 5
        })
    );
    // An audio output takes the track back.
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "master"}),
    );
    assert_eq!(midi_out(&s, &t), None);
}

#[test]
fn the_port_is_one_history_step_and_saves_with_the_song() {
    let mut s = Session::new("midi");
    let t = add(&mut s, "track.add.instrument");
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "midi:A"}),
    );
    let text = serde_json::to_string(s.project()).unwrap();
    let back: xirius_core::Project = serde_json::from_str(&text).unwrap();
    let id: TrackId = t.parse().unwrap();
    assert_eq!(back.track(id).unwrap().midi_out.as_ref().unwrap().port, "A");
    run(&mut s, "history.undo", json!({}));
    assert_eq!(midi_out(&s, &t), None);
}

#[test]
fn only_instrument_tracks_and_real_channels_are_accepted() {
    let mut s = Session::new("midi");
    let audio = add(&mut s, "track.add.audio");
    let inst = add(&mut s, "track.add.instrument");
    for args in [
        json!({"ids": [audio], "to": "midi:A"}),
        json!({"ids": [inst], "to": "midi:"}),
        json!({"ids": [inst], "to": "midi:A", "channel": 17}),
        json!({"ids": [inst], "to": "midi:A", "channel": 0}),
    ] {
        let e = try_run(&mut s, "track.output.set", args).expect_err("refused");
        assert_eq!(e.code, ErrorCode::InvalidArgs);
    }
    assert_eq!(midi_out(&s, &inst), None);
}
