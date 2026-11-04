//! Macro controls, splitter branches, layers and keys kept as the song is saved and opened
//! again, and a target a macro control moves held to the control (MIX-039).

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::controls::ControlId;
use xirius_core::id::TrackId;
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn volume(s: &Session, t: &str) -> f64 {
    let t: TrackId = t.parse().unwrap();
    s.project().track(t).unwrap().volume_db
}

#[test]
fn a_target_a_macro_moves_stays_the_knobs() {
    let mut s = Session::new("macros");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    let u = id_of(&run(&mut s, "track.add.audio", json!({"name": "Keys"})));
    let fx = id_of(&run(&mut s, "track.add.bus", json!({"name": "FX"})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    run(
        &mut s,
        "send.add",
        json!({"ids": [t], "to": fx, "level": -6}),
    );
    let tid: TrackId = t.parse().unwrap();
    let send = s.project().track(tid).unwrap().sends[0].id.to_string();
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": "volume"}),
    );
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k2", "target": format!("send:{send}")}),
    );
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k3", "target": format!("param:{eq}:gain")}),
    );
    run(
        &mut s,
        "control.set",
        json!({"track": t, "control": "k1", "value": 0.5}),
    );
    let by_knob = volume(&s, &t);
    let steps = s.steps().len();

    // the fader, the send and the knob themselves say who moves them and change nothing
    for (name, args) in [
        ("track.volume.set", json!({"ids": [t], "db": -6.0})),
        ("track.volume.set", json!({"ids": [t], "by": 3.0})),
        ("send.set", json!({"id": send, "level": -20.0})),
        (
            "plugin.param.set",
            json!({"id": eq, "param": "gain", "value": 0.9}),
        ),
    ] {
        let e = try_run(&mut s, name, args).expect_err(name);
        assert!(
            e.message.contains("マクロのつまみ"),
            "{name}: {}",
            e.message
        );
        assert!(
            e.message.contains("K1") || e.message.contains("K2") || e.message.contains("K3"),
            "{name}: {}",
            e.message
        );
    }
    assert_eq!(s.steps().len(), steps);
    assert_eq!(volume(&s, &t), by_knob);

    // two faders at once: the free one moves, the held one stays, and it says so
    let r = run(
        &mut s,
        "track.volume.set",
        json!({"ids": [t, u], "db": -3.0}),
    );
    assert_eq!(volume(&s, &u), -3.0);
    assert_eq!(volume(&s, &t), by_knob);
    assert!(r.message.contains("変えませんでした"), "{}", r.message);

    // the knob moves it, and once let go the fader is the hand's again
    run(
        &mut s,
        "control.set",
        json!({"track": t, "control": "k1", "value": 1.0}),
    );
    assert_ne!(volume(&s, &t), by_knob);
    run(
        &mut s,
        "control.unassign",
        json!({"track": t, "control": "k1", "target": "volume"}),
    );
    run(&mut s, "track.volume.set", json!({"ids": [t], "db": -6.0}));
    assert_eq!(volume(&s, &t), -6.0);
}

/// A song with every routing part this work added.
fn build(s: &mut Session) -> (String, String, String, String) {
    let t = id_of(&run(s, "track.add.audio", json!({"name": "Vocal"})));
    let kick = id_of(&run(s, "track.add.audio", json!({"name": "Kick"})));
    let comp = id_of(&run(s, "plugin.add", json!({"track": t, "name": "Comp"})));
    let eq = id_of(&run(s, "plugin.add", json!({"track": t, "name": "EQ"})));
    let split = run(s, "rack.split.add", json!({"track": t, "branches": 2}));
    let branch = split.data["branches"][1].as_str().unwrap().to_string();
    run(
        s,
        "rack.branch.set",
        json!({"id": branch, "gain": -4.5, "pan": 0.25}),
    );
    run(
        s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": format!("param:{eq}:gain"),
               "min": 0.2, "max": 0.8, "curve": 0.3}),
    );
    run(
        s,
        "control.assign",
        json!({"track": t, "control": "k2", "target": format!("branch:{branch}:gain")}),
    );
    run(
        s,
        "control.set",
        json!({"track": t, "control": "k1", "value": 0.6}),
    );
    run(
        s,
        "control.rename",
        json!({"track": t, "control": "k1", "name": "Tone"}),
    );
    run(
        s,
        "control.xy.assign",
        json!({"track": t, "axis": "y", "control": "k2"}),
    );
    run(
        s,
        "plugin.sidechain.add",
        json!({"id": comp, "from": kick, "output": true}),
    );
    let lt = id_of(&run(s, "track.add.instrument", json!({"name": "Layers"})));
    run(
        s,
        "plugin.add",
        json!({"track": lt, "name": "Sampler", "kind": "instrument"}),
    );
    let layer = id_of(&run(
        s,
        "rack.layer.add",
        json!({"track": lt, "name": "Sampler", "keys": "C1-B2"}),
    ));
    run(
        s,
        "rack.layer.set",
        json!({"id": layer, "gain": -3.0, "transpose": 12}),
    );
    (t, kick, branch, lt)
}

#[test]
fn macros_branches_layers_and_keys_come_back_and_opening_is_not_an_edit() {
    let base = tempfile::Builder::new()
        .prefix("xirius-core-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join("macros");
    let mut s = Session::new("Macros");
    let (t, _, _, _) = build(&mut s);
    s.save_as(&dir).unwrap();
    let saved = s.project().content_hash();
    let tid: TrackId = t.parse().unwrap();
    let before = s.project().track(tid).unwrap().clone();
    drop(s);

    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().content_hash(), saved, "every value comes back");
    assert!(!s.is_dirty(), "opening alone is no edit");
    let back = s.project().track(tid).unwrap();
    assert_eq!(back.controls, before.controls);
    assert_eq!(
        back.controls.get(ControlId::Knob(0)).name.as_deref(),
        Some("Tone")
    );
    assert_eq!(back.controls.xy_knobs()[1], ControlId::Knob(1));
    assert_eq!(back.rack, before.rack);
    drop(s);

    // a song kept with a target apart from its knob opens playing the knob, and is still
    // not an edit
    let file = dir.join("project.json");
    let mut doc: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let mut moved = false;
    walk(&mut doc, &mut |v| {
        if let Some(o) = v.as_object_mut()
            && o.get("name").and_then(Value::as_str) == Some("EQ")
            && let Some(p) = o.get_mut("params").and_then(Value::as_object_mut)
        {
            p.insert("gain".into(), json!(0.99));
            moved = true;
        }
    });
    assert!(moved, "the EQ's parameters are in the file");
    std::fs::write(&file, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    let s = Session::open(&dir).unwrap();
    let back = s.project().track(tid).unwrap();
    let eq = back.inserts.iter().find(|x| x.name == "EQ").unwrap();
    let knob = back.controls.get(ControlId::Knob(0));
    assert_eq!(
        eq.params.get("gain").copied(),
        Some(knob.maps[0].value(knob.value)),
        "the knob's value, not the file's"
    );
    assert!(!s.is_dirty());
}

fn walk(v: &mut Value, f: &mut dyn FnMut(&mut Value)) {
    f(v);
    match v {
        Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f)),
        Value::Object(o) => o.values_mut().for_each(|x| walk(x, f)),
        _ => {}
    }
}

#[test]
fn a_gone_branch_takes_its_lanes_and_a_frozen_track_keeps_its_macro_lane() {
    let mut s = Session::new("lanes");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    let tid: TrackId = t.parse().unwrap();
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    let split = run(&mut s, "rack.split.add", json!({"track": t, "branches": 2}));
    let sp = split.data["id"].as_str().unwrap().to_string();
    let branch = split.data["branches"][1].as_str().unwrap().to_string();
    let lane = format!("branch:{branch}:gain");
    run(&mut s, "auto.lane.add", json!({"track": t, "target": lane}));
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": lane, "at": 0, "value": -3.0}),
    );
    run(&mut s, "rack.split.remove", json!({"id": sp}));
    let left = &s.project().track(tid).unwrap().automation;
    assert!(
        left.iter().all(|l| l.target.to_string() != lane),
        "{left:?}"
    );

    // a frozen track: K1's lane moves the EQ it was rendered with, so it stays as it was
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": format!("param:{eq}:gain")}),
    );
    run(
        &mut s,
        "auto.lane.add",
        json!({"track": t, "target": "control:k1"}),
    );
    let before = s.project().clone();
    let mut frozen = before.clone();
    std::sync::Arc::make_mut(frozen.tracks.get_mut(&tid).unwrap()).frozen =
        Some(xirius_core::model::Frozen {
            file: "f.wav".into(),
            start: 0,
        });
    let mut after = frozen.clone();
    let t2 = std::sync::Arc::make_mut(after.tracks.get_mut(&tid).unwrap());
    t2.automation
        .retain(|l| l.target.to_string() != "control:k1");
    assert!(xirius_core::freeze::check(&frozen, &after).is_err());
}

#[test]
fn a_hand_edited_map_is_held_to_its_range_and_to_one_control() {
    use xirius_core::controls::MacroControls;
    let m: MacroControls = serde_json::from_value(json!({
        "knobs": [
            {"maps": [{"target": "volume", "from": 5.0, "to": -2.0, "curve": 9.0}]},
            {"maps": [{"target": "volume"}, {"target": "pan"}]}
        ]
    }))
    .unwrap();
    let k1 = &m.get(ControlId::Knob(0)).maps;
    assert_eq!((k1[0].from, k1[0].to, k1[0].curve), (1.0, 0.0, 1.0));
    let k2 = &m.get(ControlId::Knob(1)).maps;
    assert_eq!(k2.len(), 1, "the volume stays K1's");
    assert_eq!(k2[0].target.to_string(), "pan");
}

#[test]
fn a_muted_send_a_macro_moves_stays_muted_when_the_song_opens() {
    let base = tempfile::Builder::new()
        .prefix("xirius-core-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join("muted");
    let mut s = Session::new("Muted");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    let fx = id_of(&run(&mut s, "track.add.bus", json!({"name": "FX"})));
    run(
        &mut s,
        "send.add",
        json!({"ids": [t], "to": fx, "level": -6}),
    );
    let tid: TrackId = t.parse().unwrap();
    let send = s.project().track(tid).unwrap().sends[0].id.to_string();
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": format!("send:{send}")}),
    );
    run(
        &mut s,
        "control.set",
        json!({"track": t, "control": "k1", "value": 0.25}),
    );
    run(&mut s, "send.mute", json!({"id": send, "on": true}));
    let kept = s.project().track(tid).unwrap().sends[0].clone();
    assert_eq!(kept.level_db, xirius_core::model::MIN_DB);
    let held = kept.muted_level.expect("the level is kept aside");

    // the knob moves the kept level and the send stays silent
    run(
        &mut s,
        "control.set",
        json!({"track": t, "control": "k1", "value": 0.75}),
    );
    let moved = s.project().track(tid).unwrap().sends[0].clone();
    assert_eq!(moved.level_db, xirius_core::model::MIN_DB, "still silent");
    let level = moved.muted_level.expect("still muted");
    assert_ne!(level, held, "the knob moved the kept level");
    s.save_as(&dir).unwrap();
    drop(s);

    let s = Session::open(&dir).unwrap();
    let back = &s.project().track(tid).unwrap().sends[0];
    assert_eq!(
        back.level_db,
        xirius_core::model::MIN_DB,
        "opening keeps it silent"
    );
    assert_eq!(back.muted_level, Some(level));
    assert!(!s.is_dirty(), "opening alone is no edit");
}

/// A `plugin.param.set` refused because a macro holds the parameter does not make it the
/// last one touched (learn takes only what an edit was let through for).
#[test]
fn a_refused_param_set_is_not_the_last_one_touched() {
    let mut s = Session::new("macros");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": format!("param:{eq}:gain")}),
    );
    try_run(
        &mut s,
        "plugin.param.set",
        json!({"id": eq, "param": "gain", "value": 0.9}),
    )
    .expect_err("held by the macro");
    let e = try_run(
        &mut s,
        "control.assign.touched",
        json!({"track": t, "control": "k2"}),
    )
    .expect_err("nothing was touched");
    assert!(
        e.message.contains("まだパラメーターに触れていません"),
        "{}",
        e.message
    );
}

/// A plug-in moved to another track takes no macro map along (the map belongs to the track
/// it left): the map goes, and the message says so instead of dropping it unseen.
#[test]
fn moving_a_mapped_plug_in_away_says_its_map_went() {
    let mut s = Session::new("macros");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    let u = id_of(&run(&mut s, "track.add.audio", json!({"name": "Keys"})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": format!("param:{eq}:gain")}),
    );
    let r = run(&mut s, "plugin.move", json!({"id": eq, "track": u}));
    assert!(
        r.message.contains("マクロの割り当て 1 件を外しました"),
        "{}",
        r.message
    );
    let tid: TrackId = t.parse().unwrap();
    assert!(
        s.project()
            .track(tid)
            .unwrap()
            .controls
            .iter()
            .all(|(_, c)| c.maps.is_empty()),
        "the map went"
    );
}

/// A track whose rack reuses another track's node ids gets new ones on load, and its maps and
/// lanes on a branch follow the branch to its new id.
#[test]
fn a_renumbered_branch_keeps_its_maps_and_lanes() {
    use xirius_core::auto::{AutoTarget, BranchPart};
    use xirius_core::id::NodeId;
    let mut s = Session::new("macros");
    let mut tracks = Vec::new();
    for name in ["A", "B"] {
        let t = id_of(&run(&mut s, "track.add.audio", json!({"name": name})));
        run(&mut s, "plugin.add", json!({"track": t, "name": "EQ"}));
        run(&mut s, "rack.split.add", json!({"track": t}));
        tracks.push(t.parse::<TrackId>().unwrap());
    }
    let (a, b) = (tracks[0], tracks[1]);
    let ids = |p: &xirius_core::model::Project, t: TrackId| {
        p.track(t).unwrap().rack.as_ref().unwrap().node_ids()
    };
    let p0 = s.project().clone();
    let (a_ids, b_ids) = (ids(&p0, a), ids(&p0, b));
    assert_eq!(a_ids.len(), b_ids.len());
    let branch = *b_ids.last().unwrap();
    run(
        &mut s,
        "control.assign",
        json!({"track": b.to_string(), "control": "k1", "target": format!("branch:{branch}:gain")}),
    );
    run(
        &mut s,
        "auto.lane.add",
        json!({"track": b.to_string(), "target": format!("branch:{branch}:gain")}),
    );
    // B's rack takes A's ids, as a file pasted together by hand might.
    let mut p = s.project().clone();
    let to = |id: &NodeId| a_ids[b_ids.iter().position(|x| x == id).unwrap()];
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&b).unwrap());
        t.rack
            .as_mut()
            .unwrap()
            .each_node_id_mut(&mut |id| *id = to(id));
        let swap = |target: &AutoTarget| match target {
            AutoTarget::Branch(id, part) => AutoTarget::Branch(to(id), *part),
            other => other.clone(),
        };
        t.controls.retarget(|target| Some(swap(target)));
        for lane in &mut t.automation {
            lane.target = swap(&lane.target);
        }
    }
    p.fix_racks();
    let now = *ids(&p, b).last().unwrap();
    assert!(!a_ids.contains(&now), "B's branch has an id of its own");
    let t = p.track(b).unwrap();
    let want = AutoTarget::Branch(now, BranchPart::Gain);
    assert_eq!(t.controls.get(ControlId::Knob(0)).maps[0].target, want);
    assert!(
        t.automation.iter().any(|l| l.target == want),
        "the lane followed"
    );
}

/// An edit that changes one control of a track lets that control's targets stand, and still
/// puts the other controls' targets back to the value each control gives them.
#[test]
fn a_changed_control_lets_only_its_own_targets_stand() {
    use xirius_core::auto::AutoTarget;
    let mut s = Session::new("macros");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vocal"})));
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k1", "target": "volume"}),
    );
    run(
        &mut s,
        "control.assign",
        json!({"track": t, "control": "k2", "target": "pan"}),
    );
    let tid = t.parse::<TrackId>().unwrap();
    let before = s.project().clone();
    let pan = AutoTarget::Pan.stored_value(before.track(tid).unwrap());
    let mut after = before.clone();
    {
        let t = std::sync::Arc::make_mut(after.tracks.get_mut(&tid).unwrap());
        t.controls.get_mut(ControlId::Knob(0)).value = 0.2;
        AutoTarget::Volume.set_on(t, -20.0);
        AutoTarget::Pan.set_on(t, 0.7);
    }
    let held = after.hold_owned(&before);
    let t = after.track(tid).unwrap();
    assert_eq!(held.len(), 1, "{held:?}");
    assert!(held[0].contains("K2"), "{held:?}");
    assert_eq!(
        AutoTarget::Pan.stored_value(t),
        pan,
        "the pan went back to K2's value"
    );
    assert_eq!(
        AutoTarget::Volume.stored_value(t),
        Some(-20.0),
        "K1 changed, so it stands"
    );
}
