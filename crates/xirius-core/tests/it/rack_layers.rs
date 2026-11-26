//! The layer commands (ENG-075): `rack.layer.add` turns an instrument track into a
//! multi-instrument whose first layer is the instrument it had, `rack.layer.set` changes a
//! layer's ranges, transpose and levels, `rack.layer.remove` takes a layer away with its
//! plug-ins. Each is one undo step.

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::{PluginKind, Project};
use xirius_core::rack::{Layer, Source};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn fails(s: &mut Session, name: &str, args: Value) -> String {
    match s.run_value(name, args, &RunOptions::human()) {
        Ok(r) => panic!("{name} should fail, said {}", r.message),
        Err(e) => e.message,
    }
}

/// Runs an edit and checks it is one undo step there and back.
fn step(s: &mut Session, name: &str, args: Value) -> Value {
    let before: Project = s.project().clone();
    let n = s.steps().len();
    let v = run(s, name, args);
    assert_eq!(s.steps().len(), n + 1, "{name} is one step");
    let after: Project = s.project().clone();
    assert_ne!(after, before, "{name} changed the song");
    s.undo(1).expect("undo");
    assert_eq!(*s.project(), before, "{name} undoes in one step");
    s.redo(1).expect("redo");
    assert_eq!(*s.project(), after, "{name} redoes");
    v
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn tid(t: &str) -> TrackId {
    t.parse().expect("a track id")
}

fn layers(s: &Session, t: &str) -> Vec<Layer> {
    match s.project().tracks[&tid(t)]
        .rack
        .as_ref()
        .and_then(|r| r.source.clone())
    {
        Some(Source::Multi { layers, .. }) => layers,
        other => panic!("not a multi: {other:?}"),
    }
}

/// An instrument track playing the built-in Synth.
fn song() -> (Session, String) {
    let mut s = Session::new("layers");
    let t = id(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let has = s.project().tracks[&tid(&t)]
        .inserts
        .iter()
        .any(|x| x.kind == PluginKind::Instrument);
    if !has {
        run(
            &mut s,
            "plugin.add",
            json!({ "track": t, "name": "Synth", "kind": "instrument", "format": "builtin", "uid": "synth" }),
        );
    }
    (s, t)
}

#[test]
fn adding_a_layer_makes_the_instrument_the_first_layer_of_a_multi() {
    let (mut s, t) = song();
    let first = s.project().tracks[&tid(&t)]
        .inserts
        .iter()
        .find(|x| x.kind == PluginKind::Instrument)
        .map(|x| x.id)
        .unwrap();
    let v = step(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Synth", "uid": "synth", "keys": "C1-B2", "velocity": "1-127", "label": "Low" }),
    );
    let l = layers(&s, &t);
    assert_eq!(l.len(), 2);
    assert_eq!(l[0].instrument, first);
    assert_eq!((l[0].keys, l[0].velocity), ((0, 127), (1, 127)));
    assert_eq!(l[1].id.to_string(), id(&v));
    assert_eq!(l[1].instrument.to_string(), v["plugin"].as_str().unwrap());
    assert_eq!(l[1].name, "Low");
    // C1 is 36 and B2 is 59, with C3 at 60 as the piano roll names it (UI-335).
    assert_eq!(l[1].keys, (36, 59));
    let instruments = s.project().tracks[&tid(&t)]
        .inserts
        .iter()
        .filter(|x| x.kind == PluginKind::Instrument)
        .count();
    assert_eq!(instruments, 2);

    // A third, by numbers and with a transpose.
    step(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Synth", "uid": "synth", "keys": "60-127", "velocity": "64-127", "transpose": -12 }),
    );
    let l = layers(&s, &t);
    assert_eq!(l.len(), 3);
    assert_eq!(
        (l[2].keys, l[2].velocity, l[2].transpose),
        ((60, 127), (64, 127), -12)
    );
}

#[test]
fn a_layer_is_set_and_removed_in_one_step_each() {
    let (mut s, t) = song();
    let layer = id(&run(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Synth", "uid": "synth" }),
    ));
    step(
        &mut s,
        "rack.layer.set",
        json!({ "id": layer, "keys": "F#2-c#4", "velocity": "20-100", "transpose": 7, "gain": -6.0, "pan": 0.5, "mute": "on", "label": "Pad" }),
    );
    let l = layers(&s, &t);
    let x = l.iter().find(|l| l.id.to_string() == layer).unwrap();
    assert_eq!(x.keys, (54, 73));
    assert_eq!(x.velocity, (20, 100));
    assert_eq!(x.transpose, 7);
    assert_eq!((x.gain_db, x.pan, x.mute), (-6.0, 0.5, true));
    assert_eq!(x.name, "Pad");
    let plugin = x.instrument;

    step(&mut s, "rack.layer.remove", json!({ "id": layer }));
    let l = layers(&s, &t);
    assert_eq!(l.len(), 1);
    assert!(
        s.project().tracks[&tid(&t)]
            .inserts
            .iter()
            .all(|x| x.id != plugin),
        "the layer's instrument goes with it"
    );
    // The last layer stays.
    let last = l[0].id.to_string();
    assert!(fails(&mut s, "rack.layer.remove", json!({ "id": last })).contains("最後"));
}

#[test]
fn ranges_that_do_not_read_or_run_backwards_are_refused() {
    let (mut s, t) = song();
    let layer = id(&run(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Synth", "uid": "synth" }),
    ));
    for (arg, value) in [
        ("keys", "B3-C1"),
        ("keys", "H2-C3"),
        ("keys", "A8-B8"),
        ("velocity", "0-127"),
        ("velocity", "100-20"),
    ] {
        let n = s.steps().len();
        fails(&mut s, "rack.layer.set", json!({ "id": layer, arg: value }));
        assert_eq!(s.steps().len(), n, "{arg} {value} changed nothing");
    }
    fails(&mut s, "rack.layer.set", json!({ "id": layer }));
    // C-2 to G8 is the whole keyboard.
    run(
        &mut s,
        "rack.layer.set",
        json!({ "id": layer, "keys": "C-2-G8" }),
    );
    assert!(layers(&s, &t).iter().any(|l| l.keys == (0, 127)));
}

#[test]
fn layers_go_only_on_instrument_tracks() {
    let mut s = Session::new("layers");
    let a = id(&run(&mut s, "track.add.audio", json!({})));
    assert!(
        fails(
            &mut s,
            "rack.layer.add",
            json!({ "track": a, "name": "Synth", "uid": "synth" })
        )
        .contains("インストゥルメントトラック")
    );
}

#[test]
fn an_effect_moves_into_a_layer_and_a_splitter_is_put_there() {
    let (mut s, t) = song();
    let v = run(
        &mut s,
        "rack.layer.add",
        json!({ "track": t, "name": "Synth", "uid": "synth" }),
    );
    let second = id(&v);
    let eq = run(&mut s, "plugin.add", json!({ "track": t, "name": "EQ" }))["id"]
        .as_str()
        .unwrap()
        .to_string();
    step(&mut s, "rack.node.move", json!({ "id": eq, "to": second }));
    let l = layers(&s, &t);
    assert!(l[0].fx.is_empty());
    assert_eq!(
        l[1].fx,
        vec![xirius_core::rack::RackNode::Plugin(eq.parse().unwrap())]
    );
    assert!(
        s.project().tracks[&tid(&t)]
            .rack
            .as_ref()
            .unwrap()
            .fx
            .is_empty()
    );
    // A splitter after the EQ in that layer, and one at the head of the first layer.
    step(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("after:{eq}") }),
    );
    let first = layers(&s, &t)[0].id.to_string();
    step(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("layer:{first}:0") }),
    );
    let l = layers(&s, &t);
    assert!(matches!(
        l[0].fx[..],
        [xirius_core::rack::RackNode::Split(_)]
    ));
    assert!(matches!(
        l[1].fx[..],
        [
            xirius_core::rack::RackNode::Plugin(_),
            xirius_core::rack::RackNode::Split(_)
        ]
    ));
    assert!(
        fails(
            &mut s,
            "rack.split.add",
            json!({ "track": t, "at": "layer:999:0" })
        )
        .contains("999")
    );
}
