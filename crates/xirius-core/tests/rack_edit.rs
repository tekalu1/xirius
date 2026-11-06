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
    // The approved words; how many plug-ins went is in the data (MIX-045).
    assert_eq!(out.message, "スプリッターを外しました");
    assert_eq!(
        (&out.data["folded"], &out.data["plugins"]),
        (&json!(true), &json!(1))
    );
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
fn removing_one_of_two_branches_takes_the_splitter_off_and_undo_brings_it_back() {
    // ✖ always removes the branch (2026-11-06): with two, the splitter comes off and the branch
    // left goes back in series, its level, pan and mute with it; undo brings all of it back.
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
    let out = s
        .run_value(
            "rack.branch.remove",
            json!({ "id": b0 }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(
        out.message.contains("スプリッターを外しました"),
        "{}",
        out.message
    );
    assert_eq!(names(&s, &t), ["C", "B"]);
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
    s.undo(1).expect("undo");
    assert_eq!(*s.project(), before, "undo brings the splitter back");
}

#[test]
fn removing_one_of_three_branches_keeps_the_splitter_and_the_ai_is_not_refused() {
    let (mut s, t, fx) = song();
    let v = run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "branches": 3 }),
    );
    let b2 = v["branches"][2].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[0], "to": b2 }));
    let out = s
        .run_value(
            "rack.branch.remove",
            json!({ "id": b2 }),
            &RunOptions::human(),
        )
        .unwrap();
    // The approved words even with a plug-in in it; the count is in the data (MIX-045).
    assert_eq!(out.message, "分岐を削除しました");
    assert_eq!(
        (&out.data["folded"], &out.data["plugins"]),
        (&json!(false), &json!(1))
    );
    let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
    let RackNode::Split(sp) = r.fx.last().unwrap() else {
        panic!("the splitter stays")
    };
    assert_eq!(sp.branches.len(), 2);
    // The AI's removal (MCP) goes the same way: one of two takes the splitter off.
    let b1 = sp.branches[1].id.to_string();
    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": sp.branches[0].id.to_string(), "mute": "on" }),
    );
    let out = s
        .run_value(
            "rack.branch.remove",
            json!({ "id": b1 }),
            &RunOptions::ai("mcp"),
        )
        .unwrap();
    assert!(
        out.message.contains("スプリッターを外しました"),
        "{}",
        out.message
    );
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
}

#[test]
fn a_splitter_has_no_upper_limit_on_branches() {
    let (mut s, t, _) = song();
    let split = id(&run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "branches": 6 }),
    ));
    for _ in 0..57 {
        run(&mut s, "rack.branch.add", json!({ "split": split }));
    }
    step(&mut s, "rack.branch.add", json!({ "split": split }));
    step(
        &mut s,
        "rack.split.mode",
        json!({ "id": split, "mode": "frequency" }),
    );
    let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
    let RackNode::Split(sp) = r.fx.last().unwrap() else {
        panic!()
    };
    assert_eq!(sp.branches.len(), 64);
    assert_eq!(sp.crossovers.len(), 63);
    s.project().validate().unwrap();
}

/// The crossovers of the split `split` on `t`.
fn crossovers(s: &Session, t: &str, split: &str) -> Vec<f64> {
    let r = s.project().tracks[&tid(t)].rack.clone().unwrap();
    match r
        .fx
        .iter()
        .find(|n| matches!(n, RackNode::Split(x) if x.id.to_string() == split))
    {
        Some(RackNode::Split(x)) => x.crossovers.clone(),
        _ => panic!("no split {split}"),
    }
}

#[test]
fn frequency_crossovers_rise_inside_the_range_however_the_split_is_made() {
    use xirius_core::rack::{crossovers_fit, crossovers_kept, default_crossovers};
    for n in [6usize, 64, 512] {
        let (mut s, t, _) = song();
        // Made as a frequency split.
        let a = id(&run(
            &mut s,
            "rack.split.add",
            json!({ "track": t, "branches": n, "mode": "frequency" }),
        ));
        assert_eq!(crossovers(&s, &t, &a), default_crossovers(n), "{n}");
        // Made normal and switched.
        let b = id(&run(
            &mut s,
            "rack.split.add",
            json!({ "track": t, "branches": n }),
        ));
        step(
            &mut s,
            "rack.split.mode",
            json!({ "id": b, "mode": "frequency" }),
        );
        assert_eq!(crossovers(&s, &t, &b), default_crossovers(n), "{n}");
        // A branch added to it.
        step(&mut s, "rack.branch.add", json!({ "split": b }));
        let c = crossovers(&s, &t, &b);
        assert!(crossovers_fit(&c, n + 1), "{n}: {:?}", &c[c.len() - 4..]);
        s.project().validate().unwrap();
    }
    // Branches added one by one to a frequency split of two keep every crossover there was and
    // rise; inside the range while the top band can still be halved under 20 kHz (57 added).
    let (mut s, t, _) = song();
    let a = id(&run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "mode": "frequency" }),
    ));
    let mut was = crossovers(&s, &t, &a);
    for k in 3..=200 {
        run(&mut s, "rack.branch.add", json!({ "split": a }));
        let c = crossovers(&s, &t, &a);
        assert_eq!(c[..was.len()], was[..], "{k}");
        assert!(crossovers_kept(&c, k), "{k}: {:?}", &c[c.len() - 4..]);
        assert!(
            k > 58 || crossovers_fit(&c, k),
            "{k}: {:?}",
            &c[c.len() - 4..]
        );
        if k == 3 {
            // The first added cuts the top band and keeps the 1 kHz the user had.
            assert_eq!(c, [1000.0, 2000.0]);
        }
        was = c;
    }
    // The AI cannot type crossovers outside 20 Hz to 20 kHz.
    let b = id(&run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "branches": 3 }),
    ));
    let e = fails(
        &mut s,
        "rack.split.mode",
        json!({ "id": b, "mode": "frequency", "crossovers": "1000, 25000" }),
    );
    assert!(e.contains("20〜20000 Hz"), "{e}");
}

/// A frequency split on `t` with `c` as its crossovers, typed as the AI or an older song would.
fn frequency_split(s: &mut Session, t: &str, c: &[f64]) -> String {
    let a = id(&run(
        s,
        "rack.split.add",
        json!({ "track": t, "branches": c.len() + 1 }),
    ));
    let typed: Vec<String> = c.iter().map(f64::to_string).collect();
    run(
        s,
        "rack.split.mode",
        json!({ "id": a, "mode": "frequency", "crossovers": typed.join(", ") }),
    );
    assert_eq!(crossovers(s, t, &a), c);
    a
}

#[test]
fn a_branch_added_to_a_frequency_split_keeps_every_crossover_and_undoes() {
    use xirius_core::rack::{crossovers_fit, default_crossovers};
    let (mut s, t, _) = song();
    // A narrow top band: the new crossover takes its upper half, the two set stay.
    let a = frequency_split(&mut s, &t, &[1000.0, 19_000.0]);
    step(&mut s, "rack.branch.add", json!({ "split": a }));
    let c = crossovers(&s, &t, &a);
    assert_eq!(c[..2], [1000.0, 19_000.0]);
    assert!(crossovers_fit(&c, 4) && c[2] < 20_000.0, "{c:?}");
    // The top one right at 20 kHz: still kept, the new one above it.
    let b = frequency_split(&mut s, &t, &[300.0, 20_000.0]);
    step(&mut s, "rack.branch.add", json!({ "split": b }));
    let c = crossovers(&s, &t, &b);
    assert_eq!(c[..2], [300.0, 20_000.0]);
    assert!(c[2] > 20_000.0, "{c:?}");
    // From the defaults of 64 up to 68, and of 110 up to 115: none of those there goes back
    // to the defaults, every one stays inside the range, and each add undoes on its own.
    for (from, to) in [(64usize, 68usize), (110, 115), (111, 115)] {
        let x = id(&run(
            &mut s,
            "rack.split.add",
            json!({ "track": t, "branches": from, "mode": "frequency" }),
        ));
        let first = default_crossovers(from);
        for k in from + 1..=to {
            let was = crossovers(&s, &t, &x);
            step(&mut s, "rack.branch.add", json!({ "split": x }));
            let c = crossovers(&s, &t, &x);
            assert_eq!(c[..was.len()], was[..], "{from} {k}");
            assert_eq!(c[..first.len()], first[..], "{from} {k}");
            assert!(crossovers_fit(&c, k), "{from} {k}: {:?}", &c[c.len() - 4..]);
        }
        s.undo(1).expect("undo");
        assert_eq!(crossovers(&s, &t, &x).len(), to - 2, "{from}");
    }
    s.project().validate().unwrap();
}

#[test]
fn switching_to_channel_drops_the_branches_after_the_second_and_undoes() {
    let (mut s, t, fx) = song();
    let v = run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "branches": 4 }),
    );
    let split = id(&v);
    let b2 = v["branches"][2].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[0], "to": b2 }));
    let before = s.project().clone();
    let out = s
        .run_value(
            "rack.split.mode",
            json!({ "id": split, "mode": "channel" }),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(out.message, "スプリッターの分け方を channel にしました");
    assert_eq!(
        (&out.data["dropped_branches"], &out.data["plugins"]),
        (&json!(2), &json!(1))
    );
    let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
    let RackNode::Split(sp) = r.fx.last().unwrap() else {
        panic!()
    };
    assert_eq!((sp.mode, sp.branches.len()), (SplitMode::Channel, 2));
    // A went with its branch.
    assert_eq!(names(&s, &t), ["B", "C"]);
    s.project().validate().unwrap();
    s.undo(1).expect("undo");
    assert_eq!(*s.project(), before);
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

#[test]
fn an_effect_added_after_a_row_lands_beside_it_in_that_chain_in_one_step() {
    // without a rack, right after the named effect
    let (mut s, t, fx) = song();
    step(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "D", "after": fx[0] }),
    );
    assert_eq!(names(&s, &t), ["A", "D", "B", "C"]);

    // in a branch, beside the effect there
    let (mut s, t, fx, _, _) = branched();
    let v = step(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "D", "after": fx[0] }),
    );
    assert_eq!(
        branch_fx(&s, &t, 0),
        vec![
            RackNode::Plugin(pid(&fx[0])),
            RackNode::Plugin(pid(&id(&v)))
        ]
    );

    // after a splitter, in the chain that holds the splitter
    let split = s.project().tracks[&tid(&t)]
        .rack
        .as_ref()
        .unwrap()
        .fx
        .iter()
        .find_map(|n| match n {
            RackNode::Split(sp) => Some(sp.id),
            _ => None,
        })
        .unwrap();
    let v = step(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "E", "after": split.to_string() }),
    );
    let fx_top = s.project().tracks[&tid(&t)].rack.clone().unwrap().fx;
    let at = fx_top
        .iter()
        .position(|n| matches!(n, RackNode::Split(sp) if sp.id == split))
        .unwrap();
    assert_eq!(fx_top[at + 1], RackNode::Plugin(pid(&id(&v))));

    // a stranger is refused and leaves the song as it was
    let u = id(&run(&mut s, "track.add.audio", json!({ "name": "U" })));
    let other = effect(&mut s, &u, "X");
    let before_u = s.project().clone();
    let e = fails(
        &mut s,
        "plugin.add",
        json!({ "track": t, "name": "F", "after": other }),
    );
    assert!(e.contains("--after"), "{e}");
    assert_eq!(*s.project(), before_u);
}

#[test]
fn removing_a_splitter_lays_its_branches_out_in_order_where_it_was() {
    let (mut s, t, _, _, b1) = branched();
    run(
        &mut s,
        "rack.branch.set",
        json!({ "id": b1, "gain": -6.0, "mute": "on" }),
    );
    let r = s.project().tracks[&tid(&t)].rack.clone().unwrap();
    let split =
        r.fx.iter()
            .find_map(|n| match n {
                RackNode::Split(x) => Some(x.id.to_string()),
                RackNode::Plugin(_) => None,
            })
            .unwrap();
    step(&mut s, "rack.split.remove", json!({ "id": split }));
    // C stayed before the split; A's branch came first, then B's
    assert_eq!(names(&s, &t), ["C", "A", "B"]);
    assert!(s.project().tracks[&tid(&t)].rack.is_none());
    let e = fails(&mut s, "rack.split.remove", json!({ "id": split }));
    assert!(e.contains(&split), "{e}");
}

#[test]
fn a_splitter_inside_a_branch_comes_out_into_that_branch() {
    let (mut s, t, fx, b0, _) = branched();
    let inner = run(
        &mut s,
        "rack.split.add",
        json!({ "track": t, "at": format!("branch:{b0}:1") }),
    );
    let ib = inner["branches"][1].as_str().unwrap().to_string();
    run(&mut s, "rack.node.move", json!({ "id": fx[2], "to": ib }));
    step(&mut s, "rack.split.remove", json!({ "id": id(&inner) }));
    assert_eq!(
        branch_fx(&s, &t, 0),
        vec![RackNode::Plugin(pid(&fx[0])), RackNode::Plugin(pid(&fx[2]))]
    );
}

#[test]
fn a_plug_in_is_renamed_in_one_step() {
    let (mut s, t, fx) = song();
    step(
        &mut s,
        "plugin.rename",
        json!({ "id": fx[1], "name": "Glue" }),
    );
    assert_eq!(names(&s, &t), ["A", "Glue", "C"]);
    let e = fails(&mut s, "plugin.rename", json!({ "id": fx[1], "name": "" }));
    assert!(!e.is_empty());
}
