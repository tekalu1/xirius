//! A channel's inserts copied onto other channels (MIX-045): the effects and the splitters in
//! their order and state, in place of the target's or after them, in one step. The instrument
//! area, the sends, the fader and the automation stay.

use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::clipboard::Clipboard;
use xirius_core::error::ErrorCode;
use xirius_core::id::TrackId;
use xirius_core::model::{Frozen, PluginKind, Track};
use xirius_core::rack::{Rack, RackNode};
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

fn tid(t: &str) -> TrackId {
    t.parse().expect("a track id")
}

fn track(s: &Session, t: &str) -> Arc<Track> {
    s.project().tracks[&tid(t)].clone()
}

fn effect(s: &mut Session, track: &str, name: &str) -> String {
    id(&run(
        s,
        "plugin.add",
        json!({ "track": track, "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ))
}

fn names(s: &Session, t: &str) -> Vec<String> {
    track(s, t)
        .inserts
        .iter()
        .filter(|x| x.kind == PluginKind::Effect)
        .map(|x| x.name.clone())
        .collect()
}

/// The insert area as names, a splitter as `[..|..]`.
fn shape(s: &Session, t: &str) -> String {
    fn walk(t: &Track, nodes: &[RackNode]) -> String {
        nodes
            .iter()
            .map(|n| match n {
                RackNode::Plugin(id) => t
                    .inserts
                    .iter()
                    .find(|x| x.id == *id)
                    .map_or("?".to_string(), |x| x.name.clone()),
                RackNode::Split(sp) => format!(
                    "[{}]",
                    sp.branches
                        .iter()
                        .map(|b| walk(t, &b.fx))
                        .collect::<Vec<_>>()
                        .join("|")
                ),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
    let t = track(s, t);
    let rack = t.rack.clone().unwrap_or_else(|| Rack::serial(&t));
    walk(&t, &rack.fx)
}

struct Song {
    s: Session,
    a: String,
    b: String,
    c: String,
}

/// A: Compressor (bypassed, with a preset name and a value), then a splitter of two branches
/// (EQ | Reverb), then Limiter. B: Delay. C: an instrument channel with Chorus.
fn song() -> Song {
    let mut s = Session::new("inserts");
    let a = id(&run(&mut s, "track.add.audio", json!({ "name": "A" })));
    let b = id(&run(&mut s, "track.add.audio", json!({ "name": "B" })));
    let c = id(&run(&mut s, "track.add.instrument", json!({ "name": "C" })));
    let comp = effect(&mut s, &a, "Compressor");
    let eq = effect(&mut s, &a, "EQ");
    let verb = effect(&mut s, &a, "Reverb");
    effect(&mut s, &b, "Delay");
    effect(&mut s, &c, "Chorus");
    run(
        &mut s,
        "plugin.bypass",
        json!({ "ids": [comp], "on": true }),
    );
    let split = run(
        &mut s,
        "rack.split.add",
        json!({ "track": a, "at": format!("after:{comp}"), "branches": 2 }),
    );
    let b0 = split["branches"][0].as_str().unwrap().to_string();
    let b1 = split["branches"][1].as_str().unwrap().to_string();
    run(
        &mut s,
        "rack.node.move",
        json!({ "id": eq, "to": b0, "index": 0 }),
    );
    run(
        &mut s,
        "rack.node.move",
        json!({ "id": verb, "to": b1, "index": 0 }),
    );
    effect(&mut s, &a, "Limiter");
    // A value and a preset name on the compressor, as its window leaves them.
    let mut p = s.project().clone();
    let t = Arc::make_mut(p.tracks.get_mut(&tid(&a)).unwrap());
    let x = t
        .inserts
        .iter_mut()
        .find(|x| x.name == "Compressor")
        .unwrap();
    x.params.insert("ratio".into(), 0.25);
    x.preset = Some("Glue".into());
    Song {
        s: Session::from_project(p),
        a,
        b,
        c,
    }
}

#[test]
fn a_paste_replaces_the_inserts_with_the_same_order_and_splitters() {
    let Song { mut s, a, b, .. } = song();
    assert_eq!(shape(&s, &a), "Compressor [EQ|Reverb] Limiter");
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    let out = run(&mut s, "channel.inserts.paste", json!({ "ids": [b] }));
    assert_eq!(out["ids"], json!([b]));
    assert_eq!(shape(&s, &b), "Compressor [EQ|Reverb] Limiter");
    let (ta, tb) = (track(&s, &a), track(&s, &b));
    let comp = tb.inserts.iter().find(|x| x.name == "Compressor").unwrap();
    assert!(comp.bypass);
    assert_eq!(comp.preset.as_deref(), Some("Glue"));
    assert_eq!(comp.params.get("ratio"), Some(&0.25));
    // new ids, plug-ins and splitter alike
    let src = ta.inserts.iter().find(|x| x.name == "Compressor").unwrap();
    assert_ne!(comp.id, src.id);
    let split = |t: &Track| match &t.rack.as_ref().unwrap().fx[1] {
        RackNode::Split(sp) => sp.id,
        RackNode::Plugin(_) => panic!("a splitter"),
    };
    assert_ne!(split(&ta), split(&tb));
    // the source stays
    assert_eq!(shape(&s, &a), "Compressor [EQ|Reverb] Limiter");
}

#[test]
fn append_puts_the_copy_after_what_the_channel_has() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    let out = run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b], "append": true }),
    );
    assert_eq!(out["append"], json!(true));
    assert_eq!(shape(&s, &b), "Delay Compressor [EQ|Reverb] Limiter");
}

#[test]
fn the_instrument_area_stays_and_is_not_copied() {
    let Song { mut s, a, c, .. } = song();
    run(
        &mut s,
        "plugin.add",
        json!({ "track": c, "name": "Synth", "kind": "instrument", "format": "builtin", "uid": "Synth" }),
    );
    run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [c], "from": a }),
    );
    let t = track(&s, &c);
    assert!(
        t.inserts
            .iter()
            .any(|x| x.name == "Synth" && x.kind == PluginKind::Instrument)
    );
    assert_eq!(shape(&s, &c), "Compressor [EQ|Reverb] Limiter");
    // Copied from C, nothing of the instrument goes.
    run(&mut s, "channel.inserts.copy", json!({ "id": c }));
    match s.state.clipboard.as_ref() {
        Some(Clipboard::Inserts(x)) => {
            assert!(x.plugins.iter().all(|p| p.kind == PluginKind::Effect));
            assert_eq!(x.plugins.len(), 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_paste_is_one_undo_and_leaves_the_automation_of_the_source_behind() {
    let Song { s, a, b, .. } = song();
    let delay = track(&s, &b).inserts[0].id;
    let comp = track(&s, &a).inserts[0].id;
    let mut p = s.project().clone();
    for (t, target) in [
        (&b, AutoTarget::Param(delay, "mix".into())),
        (&a, AutoTarget::Param(comp, "ratio".into())),
    ] {
        let t = Arc::make_mut(p.tracks.get_mut(&tid(t)).unwrap());
        t.automation.push(xirius_core::auto::AutoLane::new(target));
    }
    let mut s = Session::from_project(p);
    let before = s.project().content_hash();
    run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b], "from": a }),
    );
    let t = track(&s, &b);
    // the replaced delay's lane goes; the source's lanes are not copied
    assert!(
        !t.automation
            .iter()
            .any(|l| matches!(l.target, AutoTarget::Param(..)))
    );
    assert_eq!(track(&s, &a).automation.len(), 1);
    s.undo(1).unwrap();
    assert_eq!(s.project().content_hash(), before);
}

#[test]
fn the_source_and_a_frozen_channel_take_nothing() {
    let Song { mut s, a, b, c } = song();
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    assert_eq!(
        fail(&mut s, "channel.inserts.paste", json!({ "ids": [a] })),
        ErrorCode::NothingToDo
    );
    let mut p = s.project().clone();
    Arc::make_mut(p.tracks.get_mut(&tid(&b)).unwrap()).frozen = Some(Frozen {
        file: "frozen.wav".into(),
        start: 0,
    });
    let mut s = Session::from_project(p);
    assert_eq!(
        fail(
            &mut s,
            "channel.inserts.paste",
            json!({ "ids": [b], "from": a })
        ),
        ErrorCode::NothingToDo
    );
    assert_eq!(names(&s, &b), ["Delay"]);
    // With others among the targets, the frozen one is passed over.
    let out = run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b, c], "from": a }),
    );
    assert_eq!(out["ids"], json!([c]));
    assert_eq!(names(&s, &b), ["Delay"]);
}

#[test]
fn edit_paste_with_inserts_on_the_board_replaces_the_tracks_inserts() {
    let Song { mut s, a, b, .. } = song();
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    run(&mut s, "edit.paste", json!({ "track": b }));
    assert_eq!(shape(&s, &b), "Compressor [EQ|Reverb] Limiter");
}

#[test]
fn the_master_takes_inserts_too() {
    let Song { mut s, a, .. } = song();
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    let master = TrackId::MASTER.to_string();
    run(&mut s, "channel.inserts.paste", json!({ "ids": [master] }));
    assert_eq!(shape(&s, &master), "Compressor [EQ|Reverb] Limiter");
}

#[test]
fn nothing_copied_is_nothing_to_paste() {
    let Song { mut s, b, .. } = song();
    assert_eq!(
        fail(&mut s, "channel.inserts.paste", json!({ "ids": [b] })),
        ErrorCode::NothingToDo
    );
}
