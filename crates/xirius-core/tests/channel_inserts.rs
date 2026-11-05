//! A channel's inserts copied onto other channels (MIX-045): the effects and the splitters in
//! their order and state, in place of the target's or after them, in one step. The instrument
//! area, the sends, the fader and the automation stay.

use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::clipboard::Clipboard;
use xirius_core::error::ErrorCode;
use xirius_core::id::TrackId;
use xirius_core::model::{Frozen, KeySource, PluginKind, Track};
use xirius_core::rack::{Rack, RackNode, Source};
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

/// The compressor's own saved state, as its window last left it.
const STATE: &str = "Z2x1ZQ==";

/// A: Compressor (bypassed, with a preset name, a value and a state), then a splitter of two branches
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
    x.state = Some(STATE.into());
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
    assert_eq!(comp.state.as_deref(), Some(STATE));
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

/// Every plug-in, send row (aux and sidechain) and rack part id in the song, as numbers (they
/// share one id space).
fn all_ids(s: &Session) -> Vec<u64> {
    fn walk(nodes: &[RackNode], out: &mut Vec<u64>) {
        for n in nodes {
            if let RackNode::Split(sp) = n {
                out.push(sp.id.0);
                for b in &sp.branches {
                    out.push(b.id.0);
                    walk(&b.fx, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for t in s.project().tracks.values() {
        out.extend(t.sends.iter().map(|x| x.id.0));
        for x in &t.inserts {
            out.push(x.id.0);
            let keys = x.sidechain.iter().flat_map(|sc| &sc.sources);
            out.extend(keys.map(|k| k.id.0));
        }
        if let Some(r) = &t.rack {
            walk(&r.fx, &mut out);
            if let Some(Source::Multi { id, layers }) = &r.source {
                out.push(id.0);
                for l in layers {
                    out.push(l.id.0);
                    walk(&l.fx, &mut out);
                }
            }
        }
    }
    out
}

fn sources(s: &Session, t: &str, name: &str) -> Vec<KeySource> {
    track(s, t)
        .inserts
        .iter()
        .find(|x| x.name == name)
        .and_then(|x| x.sidechain.clone())
        .map_or_else(Vec::new, |sc| sc.sources)
}

#[test]
fn a_sidechain_copied_gets_rows_of_its_own_and_loses_a_source_it_cannot_keep() {
    let Song { mut s, a, b, .. } = song();
    let kick = id(&run(&mut s, "track.add.audio", json!({ "name": "Kick" })));
    let comp = track(&s, &a).inserts[0].id.to_string();
    let delay = track(&s, &b).inserts[0].id.to_string();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({ "id": comp, "from": kick }),
    );
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({ "id": comp, "from": b }),
    );
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({ "id": delay, "from": kick }),
    );
    // The delay's key row on Kick's strip has a lane.
    let delay_key = sources(&s, &b, "Delay")[0].id;
    let mut p = s.project().clone();
    Arc::make_mut(p.tracks.get_mut(&tid(&kick)).unwrap())
        .automation
        .push(xirius_core::auto::AutoLane::new(AutoTarget::Send(
            delay_key,
        )));
    let mut s = Session::from_project(p);

    let out = run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b], "from": a }),
    );
    // B cannot key itself: that source is left out, and said so
    assert_eq!(out["adjusted"], json!(1));
    let copied = sources(&s, &b, "Compressor");
    let source = sources(&s, &a, "Compressor");
    assert_eq!(copied.len(), 1);
    assert_eq!(copied[0].track, tid(&kick));
    assert_ne!(copied[0].id, source[0].id, "a row of its own");
    assert_eq!(source.len(), 2, "the source keeps its own");
    // the replaced delay's key row went, and its lane with it
    assert!(
        track(&s, &kick)
            .automation
            .iter()
            .all(|l| l.target != AutoTarget::Send(delay_key))
    );
    s.project().validate().unwrap();
}

#[test]
fn a_source_gone_since_the_copy_is_left_out_of_the_paste() {
    let Song { mut s, a, b, .. } = song();
    let kick = id(&run(&mut s, "track.add.audio", json!({ "name": "Kick" })));
    let comp = track(&s, &a).inserts[0].id.to_string();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({ "id": comp, "from": kick }),
    );
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    run(&mut s, "track.delete", json!({ "ids": [kick] }));
    let out = run(&mut s, "channel.inserts.paste", json!({ "ids": [b] }));
    assert_eq!(out["adjusted"], json!(1));
    assert!(sources(&s, &b, "Compressor").is_empty());
    s.project().validate().unwrap();
}

#[test]
fn ids_made_by_pastes_are_never_shared() {
    let Song { mut s, a, b, c } = song();
    let kick = id(&run(&mut s, "track.add.audio", json!({ "name": "Kick" })));
    let comp = track(&s, &a).inserts[0].id.to_string();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({ "id": comp, "from": kick }),
    );
    let bus = id(&run(&mut s, "track.add.bus", json!({ "name": "Bus" })));
    run(
        &mut s,
        "send.add",
        json!({ "ids": [a], "to": bus, "level": -6 }),
    );
    run(&mut s, "channel.inserts.copy", json!({ "id": a }));
    // twice onto the same channel, then onto two at once
    for _ in 0..2 {
        run(
            &mut s,
            "channel.inserts.paste",
            json!({ "ids": [b], "append": true }),
        );
    }
    let out = run(&mut s, "channel.inserts.paste", json!({ "ids": [c, kick] }));
    assert_eq!(out["ids"], json!([c, kick]));
    let ids = all_ids(&s);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    assert_eq!(
        shape(&s, &b),
        "Delay Compressor [EQ|Reverb] Limiter Compressor [EQ|Reverb] Limiter"
    );
    s.project().validate().unwrap();
}

#[test]
fn an_undo_gives_back_the_replaced_plugins_with_their_ids_and_state() {
    let Song { s, a, b, .. } = song();
    let mut p = s.project().clone();
    let t = Arc::make_mut(p.tracks.get_mut(&tid(&b)).unwrap());
    t.inserts[0].state = Some("ZGVsYXk=".into());
    t.inserts[0].params.insert("mix".into(), 0.4);
    let mut s = Session::from_project(p);
    let before = s.project().clone();
    run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b], "from": a }),
    );
    let after = s.project().clone();
    s.undo(1).unwrap();
    assert_eq!(
        *s.project(),
        before,
        "the same song, ids and states and all"
    );
    // and a redo makes the same copies again, ids included
    s.redo(1).unwrap();
    assert_eq!(*s.project(), after);
}

#[test]
fn the_layers_and_their_effects_stay_as_they_are() {
    let Song { mut s, a, c, .. } = song();
    run(
        &mut s,
        "plugin.add",
        json!({ "track": c, "name": "Synth", "kind": "instrument", "format": "builtin", "uid": "Synth" }),
    );
    run(
        &mut s,
        "rack.layer.add",
        json!({ "track": c, "name": "Synth", "uid": "Synth" }),
    );
    let layer = match &track(&s, &c).rack.as_ref().unwrap().source {
        Some(Source::Multi { layers, .. }) => layers[1].id.to_string(),
        other => panic!("{other:?}"),
    };
    let phaser = effect(&mut s, &c, "Phaser");
    run(
        &mut s,
        "rack.node.move",
        json!({ "id": phaser, "to": layer, "index": 0 }),
    );
    let source = |s: &Session| track(s, &c).rack.as_ref().unwrap().source.clone();
    let layers = source(&s);
    // The layer's Phaser is not an insert to copy.
    run(&mut s, "channel.inserts.copy", json!({ "id": c }));
    match s.state.clipboard.as_ref() {
        Some(Clipboard::Inserts(x)) => {
            let names: Vec<_> = x.plugins.iter().map(|p| p.name.as_str()).collect();
            assert_eq!(names, ["Chorus"]);
        }
        other => panic!("{other:?}"),
    }
    // Pasted over, the layers keep their instruments and their Phaser.
    run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [c], "from": a }),
    );
    assert_eq!(source(&s), layers);
    assert!(
        track(&s, &c)
            .inserts
            .iter()
            .any(|x| x.id.to_string() == phaser)
    );
    assert_eq!(shape(&s, &c), "Compressor [EQ|Reverb] Limiter");
    s.project().validate().unwrap();
}

#[test]
fn splitters_nested_as_deep_as_they_go_stay_valid_after_what_a_channel_has() {
    let Song { mut s, a, b, .. } = song();
    // B ends in a splitter; A's copy holds splitters four deep.
    run(
        &mut s,
        "rack.split.add",
        json!({ "track": b, "at": "track", "branches": 2 }),
    );
    let mut branch = match &track(&s, &a).rack.as_ref().unwrap().fx[1] {
        RackNode::Split(sp) => sp.branches[0].id.to_string(),
        RackNode::Plugin(_) => panic!("a splitter"),
    };
    for _ in 1..xirius_core::rack::MAX_SPLIT_DEPTH {
        let out = run(
            &mut s,
            "rack.split.add",
            json!({ "track": a, "at": format!("branch:{branch}:0"), "branches": 2 }),
        );
        branch = out["branches"][0].as_str().unwrap().to_string();
    }
    // a fifth level is refused
    fail(
        &mut s,
        "rack.split.add",
        json!({ "track": a, "at": format!("branch:{branch}:0"), "branches": 2 }),
    );
    let deep = shape(&s, &a);
    run(
        &mut s,
        "channel.inserts.paste",
        json!({ "ids": [b], "from": a, "append": true }),
    );
    // the copy goes after B's splitter, not in it: its levels stay as deep as they were
    assert_eq!(shape(&s, &b), format!("Delay [|] {deep}"));
    s.project().validate().unwrap();
}
