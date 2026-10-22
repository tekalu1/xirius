//! A channel's settings copied onto other channels (MIX-007). One paste is one undo, the
//! instrument and the name stay, and the master takes neither sends nor pan.

use serde_json::{Value, json};
use xirius_core::error::ErrorCode;
use xirius_core::id::TrackId;
use xirius_core::model::{PluginKind, Project};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn fail(s: &mut Session, name: &str, args: Value) -> ErrorCode {
    s.run_value(name, args, &RunOptions::human())
        .expect_err("should fail")
        .code
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn track(s: &Session, t: &str) -> std::sync::Arc<xirius_core::model::Track> {
    let t: TrackId = t.parse().expect("a track id");
    s.project().tracks[&t].clone()
}

fn effect(s: &mut Session, track: &str, name: &str) -> String {
    id(&run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ))
}

struct Song {
    s: Session,
    a: String,
    b: String,
    c: String,
    bus: String,
}

/// A has two effects, a send to the bus, -6 dB and a pan; B has one other effect and a send of
/// its own; C is an instrument channel.
fn song() -> Song {
    let mut s = Session::new("channels");
    let a = id(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    let c = id(&run(&mut s, "track.add.instrument", json!({ "name": "C" })));
    let bus = id(&run(&mut s, "track.add.bus", json!({ "name": "Bus" })));
    effect(&mut s, &a, "Compressor");
    effect(&mut s, &a, "EQ");
    effect(&mut s, &b, "Reverb");
    run(
        &mut s,
        "send.add",
        json!({ "ids": [a], "to": bus, "level": -3.0, "pre": true }),
    );
    run(
        &mut s,
        "send.add",
        json!({ "ids": [b], "to": bus, "level": -20.0 }),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [a], "db": -6.0 }),
    );
    run(&mut s, "track.pan.set", json!({ "ids": [a], "pan": 0.5 }));
    Song { s, a, b, c, bus }
}

fn names(s: &Session, t: &str) -> Vec<String> {
    track(s, t).inserts.iter().map(|x| x.name.clone()).collect()
}

fn hash(s: &Session) -> String {
    s.project().content_hash()
}

#[test]
fn a_paste_replaces_the_effects_the_sends_the_fader_and_the_pan() {
    let Song {
        mut s, a, b, bus, ..
    } = song();
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    let out = run(&mut s, "channel.settings.paste", json!({ "ids": [b] }));
    assert_eq!(out["ids"], json!([b]));
    let t = track(&s, &b);
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert_eq!((t.volume_db, t.pan), (-6.0, 0.5));
    assert_eq!(t.name, "B");
    assert_eq!(t.sends.len(), 1);
    assert_eq!(t.sends[0].to.to_string(), bus);
    assert_eq!((t.sends[0].level_db, t.sends[0].pre), (-3.0, true));
    // new ids: the copy is not the source's effect or send
    let src = track(&s, &a);
    assert_ne!(t.inserts[0].id, src.inserts[0].id);
    assert_ne!(t.sends[0].id, src.sends[0].id);
    // the source is untouched
    assert_eq!(names(&s, &a), ["Compressor", "EQ"]);
}

#[test]
fn a_paste_to_many_channels_is_one_undo_and_one_redo() {
    let Song { mut s, a, b, c, .. } = song();
    let before = hash(&s);
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [b, c] }));
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert_eq!(track(&s, &c).volume_db, -6.0);
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
    s.redo(1).unwrap();
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
}

#[test]
fn the_instrument_of_the_destination_stays_first() {
    let Song { mut s, a, c, .. } = song();
    id(&run(
        &mut s,
        "plugin.add",
        json!({ "track": c, "name": "Synth", "kind": "instrument", "format": "builtin", "uid": "Synth" }),
    ));
    effect(&mut s, &c, "Chorus");
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [c] }));
    assert_eq!(names(&s, &c), ["Synth", "Compressor", "EQ"]);
    assert_eq!(track(&s, &c).inserts[0].kind, PluginKind::Instrument);
    // and an instrument channel hands on only its effects
    run(&mut s, "channel.settings.copy", json!({ "id": c }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [a] }));
    assert_eq!(names(&s, &a), ["Compressor", "EQ"]);
}

#[test]
fn only_picks_the_parts() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(
        &mut s,
        "channel.settings.paste",
        json!({ "ids": [b], "only": "volume,pan" }),
    );
    let t = track(&s, &b);
    assert_eq!((t.volume_db, t.pan), (-6.0, 0.5));
    assert_eq!(names(&s, &b), ["Reverb"]);
    assert_eq!(t.sends[0].level_db, -20.0);
    run(
        &mut s,
        "channel.settings.paste",
        json!({ "ids": [b], "only": "inserts" }),
    );
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert_eq!(track(&s, &b).sends[0].level_db, -20.0);
    assert_eq!(
        fail(
            &mut s,
            "channel.settings.paste",
            json!({ "ids": [b], "only": "mute" })
        ),
        ErrorCode::InvalidArgs
    );
}

#[test]
fn from_copies_without_the_clipboard() {
    let Song { mut s, a, b, .. } = song();
    run(
        &mut s,
        "channel.settings.paste",
        json!({ "ids": [b], "from": a }),
    );
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
    assert!(s.state.clipboard.is_none());
}

#[test]
fn a_paste_with_nothing_copied_or_onto_itself_does_nothing() {
    let Song { mut s, a, .. } = song();
    assert_eq!(
        fail(&mut s, "channel.settings.paste", json!({ "ids": [a] })),
        ErrorCode::NothingToDo
    );
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    assert_eq!(
        fail(&mut s, "channel.settings.paste", json!({ "ids": [a] })),
        ErrorCode::NothingToDo
    );
}

#[test]
fn the_master_takes_the_fader_and_the_effects_but_neither_sends_nor_pan() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(
        &mut s,
        "channel.settings.paste",
        json!({ "ids": ["master"] }),
    );
    let m = track(&s, "master");
    assert_eq!(m.volume_db, -6.0);
    assert_eq!(m.pan, 0.0);
    assert!(m.sends.is_empty());
    assert_eq!(names(&s, "master"), ["Compressor", "EQ"]);
    // the master's settings pasted on a channel do not reset its pan or its sends
    run(&mut s, "channel.settings.copy", json!({ "id": "master" }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [b] }));
    let t = track(&s, &b);
    assert_eq!(t.volume_db, -6.0);
    assert_eq!(t.pan, 0.0);
    assert_eq!(t.sends.len(), 1, "the sends stay");
}

#[test]
fn a_send_that_would_loop_back_is_left_out_and_counted() {
    let Song { mut s, a, bus, .. } = song();
    // the bus itself takes A's settings: A's send to the bus would feed the bus into itself
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    let out = run(&mut s, "channel.settings.paste", json!({ "ids": [bus] }));
    assert_eq!(out["adjusted"], json!(1));
    assert!(track(&s, &bus).sends.is_empty());
}

#[test]
fn the_lanes_of_the_replaced_sends_go_with_them() {
    let Song { mut s, a, b, .. } = song();
    let send = track(&s, &b).sends[0].id.to_string();
    run(
        &mut s,
        "auto.lane.add",
        json!({ "track": b, "target": format!("send:{send}") }),
    );
    assert_eq!(track(&s, &b).automation.len(), 1);
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [b] }));
    assert!(
        track(&s, &b).automation.is_empty(),
        "no lane points at a send that is gone"
    );
}

#[test]
fn edit_paste_puts_the_channel_on_the_track_it_names() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    let kind = run(&mut s, "edit.clipboard", json!({}));
    assert_eq!(kind["kind"], json!("channel"));
    run(&mut s, "edit.paste", json!({ "track": b }));
    assert_eq!(names(&s, &b), ["Compressor", "EQ"]);
}

#[test]
fn the_pasted_settings_survive_a_save_and_a_reopen() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.settings.copy", json!({ "id": a }));
    run(&mut s, "channel.settings.paste", json!({ "ids": [b] }));
    let json = serde_json::to_string(s.project()).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    let t = &back.tracks[&b.parse::<TrackId>().unwrap()];
    assert_eq!(
        t.inserts
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>(),
        ["Compressor", "EQ"]
    );
    assert_eq!((t.volume_db, t.pan, t.sends.len()), (-6.0, 0.5, 1));
}
