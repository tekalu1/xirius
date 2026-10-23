//! The commands that shape a rack (ENG-074): splitters, branches, moving a node inside the
//! rack and flattening it back. Each is one undo step, and a splitter left with one branch
//! folds back into a line.

use serde_json::{Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Project;
use xirius_core::rack::{RackNode, SplitMode};
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

/// Runs an edit, checks it is one undo step that undoes to the song before and redoes to the
/// song after, and leaves the song after.
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

fn effect(s: &mut Session, t: &str, name: &str) -> String {
    id(&run(
        s,
        "plugin.add",
        json!({ "track": t, "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ))
}

/// A track with effects A, B and C.
fn song() -> (Session, String, Vec<String>) {
    let mut s = Session::new("rack-edit");
    let t = id(&run(&mut s, "track.add.audio", json!({ "name": "T" })));
    let fx = ["A", "B", "C"].map(|n| effect(&mut s, &t, n)).to_vec();
    (s, t, fx)
}

fn tid(t: &str) -> TrackId {
    t.parse().expect("a track id")
}

fn pid(x: &str) -> PluginId {
    x.parse().expect("a plug-in id")
}

fn names(s: &Session, t: &str) -> Vec<String> {
    s.project().tracks[&tid(t)]
        .inserts
        .iter()
        .map(|x| x.name.clone())
        .collect()
}

#[test]
fn a_split_is_built_and_every_edit_is_one_undo_step() {
    let (mut s, t, fx) = song();
    let v = step(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("after:{}", fx[0]) }),
    );
    let split = id(&v);
    let branches: Vec<String> = v["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_str().unwrap().to_string())
        .collect();
    assert_eq!(branches.len(), 2, "two branches by default");
    {
        let r = s.project().tracks[&tid(&t)].rack.clone().expect("a rack");
        assert_eq!(r.fx.len(), 4, "A, the split, B, C");
        assert!(matches!(r.fx[1], RackNode::Split(_)));
    }

    step(
        &mut s,
        "rack.node.move",
        json!({ "id": fx[1], "to": branches[0], "index": 0 }),
    );
    step(
        &mut s,
        "rack.node.move",
        json!({ "id": fx[2], "to": branches[1] }),
    );
    let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
    let RackNode::Split(sp) = &r.fx[1] else {
        panic!("the split stays second: {:?}", r.fx)
    };
    assert_eq!(r.fx.len(), 2);
    assert_eq!(sp.branches[0].fx, vec![RackNode::Plugin(pid(&fx[1]))]);
    assert_eq!(sp.branches[1].fx, vec![RackNode::Plugin(pid(&fx[2]))]);
    assert_eq!(
        names(&s, &t),
        ["A", "B", "C"],
        "inserts keep the flattened order"
    );

    step(
        &mut s,
        "rack.branch.set",
        json!({ "id": branches[1], "gain": -6.0, "pan": 0.5, "mute": "on" }),
    );
    let b = {
        let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
        let RackNode::Split(sp) = &r.fx[1] else {
            panic!()
        };
        sp.branches[1].clone()
    };
    assert_eq!((b.gain_db, b.pan, b.mute), (-6.0, 0.5, true));

    let third = id(&step(&mut s, "rack.branch.add", json!({ "split": split })));
    step(
        &mut s,
        "rack.split.mode",
        json!({ "id": split, "mode": "frequency" }),
    );
    {
        let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
        let RackNode::Split(sp) = &r.fx[1] else {
            panic!()
        };
        assert_eq!(sp.mode, SplitMode::Frequency);
        assert_eq!(sp.crossovers.len(), 2, "the defaults fill the crossovers");
    }
    step(
        &mut s,
        "rack.split.mode",
        json!({ "id": split, "mode": "frequency", "crossovers": "300, 3000" }),
    );
    step(&mut s, "rack.branch.remove", json!({ "id": third }));
    step(
        &mut s,
        "rack.split.mode",
        json!({ "id": split, "mode": "normal" }),
    );

    step(&mut s, "rack.flatten", json!({ "track": t }));
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
    assert_eq!(names(&s, &t), ["A", "B", "C"]);
}

#[test]
fn removing_a_branch_takes_its_plugins_and_one_branch_folds_to_a_line() {
    let (mut s, t, fx) = song();
    let v = run(&mut s, "rack.split.add", json!({ "track": t }));
    let b0 = v["branches"][0].as_str().unwrap().to_string();
    let b1 = v["branches"][1].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[0], "to": b0 }));
    run(&mut s, "rack.node.move", json!({ "id": fx[1], "to": b1 }));
    assert_eq!(names(&s, &t), ["C", "A", "B"]);
    let out = s
        .run_value(
            "rack.branch.remove",
            json!({ "id": b1 }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(out.message.contains("1 件"), "{}", out.message);
    // B went with its branch; A's branch is the only one left and is a line again.
    assert_eq!(names(&s, &t), ["C", "A"]);
    assert!(
        s.project().tracks[&tid(&t)].rack.is_none(),
        "a one-branch splitter is a line, and a line needs no rack"
    );
}

#[test]
fn a_split_goes_into_a_branch_and_nests_no_deeper_than_four() {
    let (mut s, t, _) = song();
    let mut branch = run(&mut s, "rack.split.add", json!({ "track": t }))["branches"][0]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..3 {
        let v = run(
            &mut s,
            "rack.split.add",
            json!({ "track": t, "at": format!("branch:{branch}:0") }),
        );
        branch = v["branches"][0].as_str().unwrap().to_string();
    }
    let e = fails(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("branch:{branch}:0") }),
    );
    assert!(e.contains("4 段"), "{e}");
}

#[test]
fn moves_that_would_break_the_rack_are_refused() {
    let (mut s, t, fx) = song();
    let outer = run(&mut s, "rack.split.add", json!({ "track": t }));
    let outer_id = id(&outer);
    let ob = outer["branches"][0].as_str().unwrap().to_string();
    let inner = run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("branch:{ob}:0") }),
    );
    let ib = inner["branches"][0].as_str().unwrap().to_string();
    let e = fails(
        &mut s,
        "rack.node.move",
        json!({ "id": outer_id, "to": ib }),
    );
    assert!(e.contains("自分の分岐"), "{e}");

    let other = id(&run(&mut s, "track.add.audio", json!({ "name": "U" })));
    let e = fails(
        &mut s,
        "rack.node.move",
        json!({ "id": fx[0], "to": other }),
    );
    assert!(e.contains("plugin.move"), "{e}");
    let e = fails(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "mode": "channel", "branches": 3 }),
    );
    assert!(e.contains("channel"), "{e}");
    let ch = id(&run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "mode": "channel" }),
    ));
    let e = fails(&mut s, "rack.branch.add", json!({ "split": ch }));
    assert!(e.contains("channel"), "{e}");
    let e = fails(&mut s, "rack.branch.set", json!({ "id": ob }));
    assert!(e.contains("--gain"), "{e}");

    // An instrument stays the source.
    let it = id(&run(&mut s, "track.add.instrument", json!({ "name": "I" })));
    let synth = id(&run(
        &mut s,
        "plugin.add",
        json!({ "track": it, "name": "Synth", "kind": "instrument", "format": "builtin", "uid": "Synth" }),
    ));
    let v = run(
        &mut s,
        "rack.split.add",
        json!({ "track": it, "at": format!("after:{synth}") }),
    );
    let b = v["branches"][0].as_str().unwrap().to_string();
    let e = fails(&mut s, "rack.node.move", json!({ "id": synth, "to": b }));
    assert!(e.contains("インストゥルメント"), "{e}");
}

#[test]
fn moving_inside_a_plain_track_reorders_it_without_leaving_a_rack() {
    let (mut s, t, fx) = song();
    step(
        &mut s,
        "rack.node.move",
        json!({ "id": fx[2], "to": t, "index": 0 }),
    );
    assert_eq!(names(&s, &t), ["C", "A", "B"]);
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
}

#[test]
fn removing_a_branch_does_not_drop_the_values_of_the_one_left() {
    let (mut s, t, fx) = song();
    let v = run(&mut s, "rack.split.add", json!({ "track": t }));
    let b0 = v["branches"][0].as_str().unwrap().to_string();
    let b1 = v["branches"][1].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[0], "to": b0 }));
    run(&mut s, "rack.node.move", json!({ "id": fx[1], "to": b1 }));
    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b1, "gain": -12.0, "pan": -1.0, "mute": "on" }),
    );
    let before = s.project().clone();
    // Folded to a line, the muted branch would sound again at 0 dB in the centre.
    let e = fails(&mut s, "rack.branch.remove", json!({ "id": b0 }));
    assert!(e.contains("ミュート"), "{e}");
    assert_eq!(*s.project(), before);

    // Back at its defaults, it folds as before.
    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b1, "gain": 0.0, "pan": 0.0, "mute": "off" }),
    );
    step(&mut s, "rack.branch.remove", json!({ "id": b0 }));
    assert_eq!(names(&s, &t), ["C", "B"]);
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
}

/// A, B and C with A alone in the first branch and B in the second.
fn branched() -> (Session, String, Vec<String>, String, String) {
    let (mut s, t, fx) = song();
    let v = run(&mut s, "rack.split.add", json!({ "track": t }));
    let b0 = v["branches"][0].as_str().unwrap().to_string();
    let b1 = v["branches"][1].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[0], "to": b0 }));
    run(&mut s, "rack.node.move", json!({ "id": fx[1], "to": b1 }));
    (s, t, fx, b0, b1)
}

fn branch_fx(s: &Session, t: &str, at: usize) -> Vec<RackNode> {
    let r = s.project().tracks[&tid(t)].rack.clone().expect("a rack");
    let split =
        r.fx.iter()
            .find_map(|n| match n {
                RackNode::Split(sp) => Some(sp.clone()),
                _ => None,
            })
            .expect("a split");
    split.branches[at].fx.clone()
}

#[test]
fn a_duplicate_of_a_plugin_in_a_branch_stays_beside_it_in_that_branch() {
    let (mut s, t, fx, _, _) = branched();
    let v = step(&mut s, "plugin.duplicate", json!({ "id": fx[0] }));
    let copy = id(&v);
    assert_eq!(
        branch_fx(&s, &t, 0),
        vec![RackNode::Plugin(pid(&fx[0])), RackNode::Plugin(pid(&copy))],
        "the copy runs in parallel like the original"
    );
    let inserts: Vec<String> = s.project().tracks[&tid(&t)]
        .inserts
        .iter()
        .map(|x| x.id.to_string())
        .collect();
    assert_eq!(
        v["index"].as_u64().unwrap() as usize,
        inserts.iter().position(|x| *x == copy).unwrap(),
        "the index said is where the copy is"
    );
}

#[test]
fn moving_a_plugin_in_a_branch_by_index_on_its_track_points_to_rack_node_move() {
    let (mut s, t, fx, _, _) = branched();
    let before = s.project().clone();
    let e = fails(&mut s, "plugin.move", json!({ "id": fx[1], "index": 1 }));
    assert!(e.contains("rack.node.move"), "{e}");
    assert_eq!(*s.project(), before);
    // To another track it moves, as plugin.move always did.
    let u = id(&run(&mut s, "track.add.audio", json!({ "name": "U" })));
    step(&mut s, "plugin.move", json!({ "id": fx[1], "track": u }));
    assert_eq!(names(&s, &u), ["B"]);
    assert_eq!(branch_fx(&s, &t, 1), vec![]);
}
