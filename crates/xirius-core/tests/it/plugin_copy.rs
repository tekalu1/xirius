//! Effects moved and copied between channels, and cut, copied, pasted and duplicated with the
//! shared `edit.*` commands (CTX-041, CTX-042). One command is one undo.

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::error::ErrorCode;
use xirius_core::id::{PluginId, TrackId};
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

fn effect(s: &mut Session, track: &str, name: &str) -> String {
    id(&run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ))
}

fn names(s: &Session, track: &str) -> Vec<String> {
    let t: TrackId = track.parse().expect("a track id");
    s.project().tracks[&t]
        .inserts
        .iter()
        .map(|x| x.name.clone())
        .collect()
}

fn hash(s: &Session) -> String {
    s.project().content_hash()
}

/// Two audio channels, A with a "Compressor" and an "EQ", B with nothing.
fn song() -> (Session, String, String, String, String) {
    let mut s = Session::new("plugins");
    let a = id(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    let comp = effect(&mut s, &a, "Compressor");
    let eq = effect(&mut s, &a, "EQ");
    (s, a, b, comp, eq)
}

#[test]
fn a_duplicate_sits_right_after_the_original_and_is_one_undo() {
    let (mut s, a, _, comp, _) = song();
    let before = hash(&s);
    let out = run(&mut s, "edit.duplicate", json!({ "plugin": comp }));
    assert_eq!(out["ids"].as_array().unwrap().len(), 1);
    assert_eq!(names(&s, &a), ["Compressor", "Compressor", "EQ"]);
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
    s.redo(1).unwrap();
    assert_eq!(names(&s, &a), ["Compressor", "Compressor", "EQ"]);
}

#[test]
fn a_copy_pastes_at_the_end_of_another_channel() {
    let (mut s, a, b, comp, _) = song();
    run(&mut s, "edit.copy", json!({ "plugin": comp }));
    assert_eq!(
        names(&s, &a),
        ["Compressor", "EQ"],
        "copying changes nothing"
    );
    run(&mut s, "edit.paste", json!({ "track": b }));
    assert_eq!(names(&s, &b), ["Compressor"]);
    run(&mut s, "edit.paste", json!({ "track": b }));
    assert_eq!(names(&s, &b), ["Compressor", "Compressor"]);
    // The copies are plugins of their own.
    let ids: Vec<PluginId> = s.project().tracks[&b.parse::<TrackId>().unwrap()]
        .inserts
        .iter()
        .map(|x| x.id)
        .collect();
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[0].to_string(), comp);
}

#[test]
fn a_cut_takes_the_plugin_off_and_pastes_it_elsewhere() {
    let (mut s, a, b, comp, _) = song();
    run(&mut s, "edit.cut", json!({ "plugin": comp }));
    assert_eq!(names(&s, &a), ["EQ"]);
    run(&mut s, "edit.paste", json!({ "track": b }));
    assert_eq!(names(&s, &b), ["Compressor"]);
}

#[test]
fn a_paste_with_no_channel_goes_next_to_the_original() {
    let (mut s, a, _, comp, _) = song();
    run(&mut s, "edit.copy", json!({ "plugin": comp }));
    run(&mut s, "edit.paste", json!({}));
    assert_eq!(names(&s, &a), ["Compressor", "Compressor", "EQ"]);
}

#[test]
fn an_instrument_is_not_copied_or_moved() {
    let mut s = Session::new("inst");
    let t = id(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    let inst = id(&run(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "Sampler", "kind": "instrument", "format": "builtin" }),
    ));
    assert_eq!(
        fail(&mut s, "edit.copy", json!({ "plugin": inst })),
        ErrorCode::Conflict
    );
    assert_eq!(
        fail(&mut s, "plugin.move", json!({ "id": inst, "track": b })),
        ErrorCode::Conflict
    );
    assert_eq!(
        fail(&mut s, "plugin.copy_to", json!({ "id": inst, "track": b })),
        ErrorCode::Conflict
    );
    assert_eq!(names(&s, &t), ["Sampler"]);
}

#[test]
fn copy_to_adds_a_copy_with_its_values_and_a_new_id() {
    let (mut s, a, b, comp, _) = song();
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": comp, "param": "threshold", "value": -30.0 }),
    );
    let before = hash(&s);
    let out = run(&mut s, "plugin.copy_to", json!({ "id": comp, "track": b }));
    let new: PluginId = id(&out).parse().unwrap();
    assert_ne!(new.to_string(), comp);
    assert_eq!(names(&s, &a), ["Compressor", "EQ"]);
    assert_eq!(names(&s, &b), ["Compressor"]);
    let tb: TrackId = b.parse().unwrap();
    let tc: PluginId = comp.parse().unwrap();
    let ta: TrackId = a.parse().unwrap();
    let there = &s.project().tracks[&tb].inserts[0];
    let here = s.project().tracks[&ta]
        .inserts
        .iter()
        .find(|x| x.id == tc)
        .unwrap();
    assert_eq!(there.params, here.params);
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
}

#[test]
fn move_to_takes_the_plugin_and_its_lane_and_is_one_undo() {
    let (mut s, a, b, comp, _) = song();
    let target = format!("param:{comp}:threshold");
    run(
        &mut s,
        "auto.lane.add",
        json!({ "track": a, "target": target }),
    );
    let before = hash(&s);
    run(&mut s, "plugin.move", json!({ "id": comp, "track": b }));
    assert_eq!(names(&s, &a), ["EQ"]);
    assert_eq!(names(&s, &b), ["Compressor"]);
    let ta: TrackId = a.parse().unwrap();
    let tb: TrackId = b.parse().unwrap();
    let is_it = |l: &xirius_core::auto::AutoLane| matches!(&l.target, AutoTarget::Param(p, _) if p.to_string() == comp);
    assert!(!s.project().tracks[&ta].automation.iter().any(is_it));
    assert!(s.project().tracks[&tb].automation.iter().any(is_it));
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
}

#[test]
fn move_to_the_same_channel_is_a_reorder() {
    let (mut s, a, _, comp, _) = song();
    run(
        &mut s,
        "plugin.move",
        json!({ "id": comp, "track": a, "index": 1 }),
    );
    assert_eq!(names(&s, &a), ["EQ", "Compressor"]);
}

#[test]
fn the_clipboard_says_it_holds_a_plugin() {
    let (mut s, _, _, comp, _) = song();
    run(&mut s, "edit.copy", json!({ "plugin": comp }));
    let out = run(&mut s, "edit.clipboard", json!({}));
    assert_eq!(out["kind"], "plugins");
    assert_eq!(out["count"], 1);
}

#[test]
fn a_new_bus_can_come_with_its_effect_in_one_undo() {
    let (mut s, a, _, _, _) = song();
    let before = hash(&s);
    let bus = id(&run(
        &mut s,
        "track.add.bus",
        json!({ "name": "Fx", "plugin": "Reverb", "format": "builtin", "uid": "Reverb" }),
    ));
    assert_eq!(names(&s, &bus), ["Reverb"]);
    assert_ne!(hash(&s), before);
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
    // The sources still go to the bus with the effect.
    let bus = id(&run(
        &mut s,
        "track.add.bus",
        json!({ "sources": [a], "plugin": "Reverb" }),
    ));
    assert_eq!(names(&s, &bus), ["Reverb"]);
    assert_eq!(
        fail(&mut s, "track.add.bus", json!({ "plugin": " " })),
        ErrorCode::InvalidArgs
    );
}
