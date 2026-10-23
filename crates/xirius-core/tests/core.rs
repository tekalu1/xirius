//! Tests of the core through its public API: invariants, every command's undo /
//! redo round trip, files, versions, AI permission and the speed budget.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

use serde_json::{Value, json};
use xirius_core::command::Author;
use xirius_core::error::ErrorCode;
use xirius_core::model::PluginIsolation;
use xirius_core::session::{AiPermission, RunOptions, RunResult, Session};
use xirius_core::{CmdError, Registry};

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-core-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args_text}: {e:?}", args_text = ""))
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

/// The song's content with the id counter left out: a restored version keeps
/// the counter moving forward on purpose, so ids are never reused.
fn content(s: &Session) -> String {
    let mut p = s.project().clone();
    p.next_id = 0;
    p.content_hash()
}

fn hash(s: &Session) -> String {
    s.project().content_hash()
}

fn code(r: Result<RunResult, CmdError>) -> ErrorCode {
    r.expect_err("should fail").code
}

// ---- the model's rules -------------------------------------------------------------------

#[test]
fn output_loops_are_rejected() {
    let mut s = Session::new("loop");
    let a = id_of(&run(&mut s, "track.add.bus", json!({"name": "A"})));
    let b = id_of(&run(&mut s, "track.add.bus", json!({"name": "B"})));
    let c = id_of(&run(&mut s, "track.add.bus", json!({"name": "C"})));
    run(&mut s, "track.output.set", json!({"ids": [a], "to": b}));
    run(&mut s, "track.output.set", json!({"ids": [b], "to": c}));
    let before = hash(&s);
    // C -> A closes the loop A -> B -> C -> A.
    assert_eq!(
        code(try_run(
            &mut s,
            "track.output.set",
            json!({"ids": [c], "to": a})
        )),
        ErrorCode::Conflict
    );
    // A bus cannot feed itself.
    assert_eq!(
        code(try_run(
            &mut s,
            "track.output.set",
            json!({"ids": [a], "to": a})
        )),
        ErrorCode::Conflict
    );
    assert_eq!(hash(&s), before, "a rejected command changes nothing");
    assert_eq!(s.steps().len(), 5, "and leaves no history step");
}

#[test]
fn send_loops_are_rejected() {
    let mut s = Session::new("sends");
    let fx = id_of(&run(&mut s, "track.add.bus", json!({"name": "FX"})));
    let bus = id_of(&run(&mut s, "track.add.bus", json!({"name": "Sub"})));
    run(&mut s, "track.output.set", json!({"ids": [fx], "to": bus}));
    // Sub -> FX would loop through FX's output.
    assert_eq!(
        code(try_run(&mut s, "send.add", json!({"ids": [bus], "to": fx}))),
        ErrorCode::Conflict
    );
    // A plain track may send to a bus, and to many.
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    run(&mut s, "send.add", json!({"ids": [t], "to": fx}));
    run(
        &mut s,
        "send.add",
        json!({"ids": [t], "to": bus, "pre": true}),
    );
    assert_eq!(
        s.project().track(t.parse().unwrap()).unwrap().sends.len(),
        2
    );
}

#[test]
fn a_new_bus_and_its_sends_are_one_step() {
    let mut s = Session::new("new bus");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let u = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let before = hash(&s);
    let steps = s.steps().len();
    let r = run(
        &mut s,
        "send.add.bus",
        json!({"ids": [t, u], "name": "Space", "level": -6}),
    );
    s.project().validate_deep().unwrap();
    assert_eq!(s.steps().len(), steps + 1);
    let bus: xirius_core::id::TrackId = r.data["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(r.data["sends"].as_array().unwrap().len(), 2);
    let p = s.project();
    assert_eq!(p.track(bus).unwrap().name, "Space");
    for id in [&t, &u] {
        let sends = &p.track(id.parse().unwrap()).unwrap().sends;
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].to, bus);
        assert_eq!(sends[0].level_db, -6.0);
        assert!(!sends[0].pre);
    }
    // One undo takes the bus and the sends away together.
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
    assert!(s.project().track(bus).is_err());
    // The master cannot send, and nothing is made when it is asked to.
    assert!(try_run(&mut s, "send.add.bus", json!({"ids": ["master"]})).is_err());
    assert_eq!(hash(&s), before);
}

#[test]
fn a_new_bus_for_a_folder_is_one_step() {
    let mut s = Session::new("folder new bus");
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "Drums"})));
    let kick = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"name": "Kick", "into": f}),
    ));
    let before = hash(&s);
    let steps = s.steps().len();
    let r = run(
        &mut s,
        "folder.bus.add",
        json!({"id": f, "name": "Drum bus"}),
    );
    s.project().validate_deep().unwrap();
    assert_eq!(s.steps().len(), steps + 1);
    let bus: xirius_core::id::TrackId = r.data["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(r.data["folder"].as_str(), Some(f.as_str()));
    let p = s.project();
    assert_eq!(p.track(bus).unwrap().name, "Drum bus");
    assert_eq!(p.folders[&f.parse().unwrap()].bus, Some(bus));
    // The bus is made outside the folder (at the top), so the folder can play to it.
    assert!(p.root.contains(&xirius_core::id::Node::Track(bus)));
    let _ = kick;
    // Without a name, one is made up.
    let g = id_of(&run(&mut s, "folder.add", json!({})));
    let r2 = run(&mut s, "folder.bus.add", json!({"id": g}));
    let other: xirius_core::id::TrackId = r2.data["id"].as_str().unwrap().parse().unwrap();
    assert!(!s.project().track(other).unwrap().name.is_empty());
    s.undo(2).unwrap();
    // One undo takes the bus and the connection away together.
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
    assert!(s.project().track(bus).is_err());
    // Made inside the folder itself, the bus would be a loop: nothing is made.
    assert_eq!(
        code(try_run(
            &mut s,
            "folder.bus.add",
            json!({"id": f, "into": f})
        )),
        ErrorCode::Conflict
    );
    assert_eq!(hash(&s), before);
    // A folder that does not exist makes nothing either.
    assert!(try_run(&mut s, "folder.bus.add", json!({"id": "fld_999"})).is_err());
    assert_eq!(hash(&s), before);
}

#[test]
fn folder_bus_loops_are_rejected() {
    let mut s = Session::new("folder bus");
    let bus = id_of(&run(&mut s, "track.add.bus", json!({})));
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "Drums"})));
    run(&mut s, "track.move", json!({"ids": [bus], "into": f}));
    // The bus lives inside the folder, so the folder cannot output to it.
    assert_eq!(
        code(try_run(
            &mut s,
            "folder.bus.set",
            json!({"id": f, "bus": bus})
        )),
        ErrorCode::Conflict
    );
}

#[test]
fn folders_nest_without_limit_and_cannot_enter_themselves() {
    let mut s = Session::new("nest");
    let mut parent = "root".to_string();
    let mut chain = Vec::new();
    for i in 0..40 {
        let f = id_of(&run(
            &mut s,
            "folder.add",
            json!({"name": format!("F{i}"), "into": parent}),
        ));
        chain.push(f.clone());
        parent = f;
    }
    let t = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"into": parent, "name": "deep"}),
    ));
    s.project().validate_deep().unwrap();
    let id = t.parse().unwrap();
    assert_eq!(
        s.project()
            .ancestors(xirius_core::id::Node::Track(id))
            .len(),
        40
    );
    // A folder cannot be moved into its own descendant.
    assert_eq!(
        code(try_run(
            &mut s,
            "track.move",
            json!({"ids": [chain[0]], "into": chain[39]})
        )),
        ErrorCode::Conflict
    );
    // Moving a track out and in again works at any depth.
    run(&mut s, "track.move", json!({"ids": [t], "out": true}));
    assert_eq!(
        s.project()
            .ancestors(xirius_core::id::Node::Track(id))
            .len(),
        39
    );
    run(&mut s, "track.move", json!({"ids": [t], "into": chain[5]}));
    assert_eq!(
        s.project()
            .ancestors(xirius_core::id::Node::Track(id))
            .len(),
        6
    );
}

#[test]
fn folder_has_no_sound_but_can_output_to_a_bus() {
    let mut s = Session::new("fb");
    let bus = id_of(&run(&mut s, "track.add.bus", json!({"name": "Drum bus"})));
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "Drums"})));
    let t = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"into": f, "name": "Kick"}),
    ));
    run(&mut s, "folder.bus.set", json!({"id": f, "bus": bus}));
    let tid = t.parse().unwrap();
    let out = s.project().effective_output(tid).unwrap();
    assert_eq!(
        out.to_string(),
        bus,
        "a track in the folder inherits the bus"
    );
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [t], "to": "master"}),
    );
    assert_eq!(
        s.project().effective_output(tid).unwrap().to_string(),
        "master"
    );
    // A folder cannot take a plug-in or a clip: it is not a track id.
    assert!(try_run(&mut s, "plugin.add", json!({"track": f, "name": "EQ"})).is_err());
    assert!(try_run(&mut s, "clip.add.midi", json!({"track": f, "at": 0})).is_err());
}

#[test]
fn ids_are_stable_and_never_reused() {
    let mut s = Session::new("ids");
    let a = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": a, "at": 0, "bars": 2}),
    ));
    let n1 = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 0, "pitch": 60}),
    ));
    run(&mut s, "track.rename", json!({"id": a, "name": "Piano"}));
    run(&mut s, "track.move", json!({"ids": [a], "by": 0}));
    // Editing never changes ids.
    assert!(s.project().track(a.parse().unwrap()).is_ok());
    assert!(s.project().find_clip(c.parse().unwrap()).is_some());
    // Undo the note, then add another: the new id is not the old one.
    s.undo(1).unwrap(); // (the no-op move made no step, so this undoes the rename)
    s.undo(1).unwrap();
    let n2 = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 0, "pitch": 62}),
    ));
    assert_ne!(n1, n2);
    // Redo is gone after a new edit, but the ids stay unique after a reopen too.
    let (_dir_tmp, dir) = temp_dir("ids");
    s.save_as(&dir).unwrap();
    drop(s);
    let mut s = Session::open(&dir).unwrap();
    let n3 = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 0, "pitch": 64}),
    ));
    assert!(n3 != n1 && n3 != n2);
}

#[test]
fn deleting_a_track_cleans_up_sends_and_outputs() {
    let mut s = Session::new("del");
    let bus = id_of(&run(&mut s, "track.add.bus", json!({})));
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let u = id_of(&run(&mut s, "track.add.audio", json!({})));
    run(&mut s, "send.add", json!({"ids": [t, u], "to": bus}));
    run(&mut s, "track.output.set", json!({"ids": [t], "to": bus}));
    run(&mut s, "track.delete", json!({"ids": [bus]}));
    s.project().validate_deep().unwrap();
    for id in [t.clone(), u] {
        let tr = s.project().track(id.parse().unwrap()).unwrap();
        assert!(tr.sends.is_empty());
    }
    s.undo(1).unwrap();
    s.project().validate_deep().unwrap();
    assert_eq!(
        s.project()
            .track(t.parse().unwrap())
            .unwrap()
            .output
            .map(|o| o.to_string()),
        Some(bus)
    );
}

#[test]
fn the_master_cannot_be_removed_or_moved() {
    let mut s = Session::new("master");
    assert!(try_run(&mut s, "track.delete", json!({"ids": ["master"]})).is_err());
    assert!(try_run(&mut s, "track.move", json!({"ids": ["master"], "by": 1})).is_err());
}

// ---- every command, one step, one undo ----------------------------------------------------------

/// Runs a command, checks its undo / redo round trip, and notes it as covered.
struct Rig {
    s: Session,
    covered: BTreeSet<String>,
}

impl Rig {
    fn step(&mut self, name: &str, args: Value) -> RunResult {
        let before = hash(&self.s);
        let steps = self.s.steps().len();
        let r = self
            .s
            .run_value(name, args.clone(), &RunOptions::human())
            .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"));
        self.s
            .project()
            .validate_deep()
            .unwrap_or_else(|e| panic!("{name} {args} broke the song: {e:?}"));
        let after = hash(&self.s);
        assert_ne!(before, after, "{name} {args} changed nothing");
        assert_eq!(self.s.steps().len(), steps + 1, "{name}: exactly one step");
        assert!(r.step.is_some());
        // Undo restores the earlier state; redo brings the later one back.
        self.s.undo(1).unwrap();
        assert_eq!(hash(&self.s), before, "{name} {args}: undo");
        self.s.project().validate_deep().unwrap();
        self.s.redo(1).unwrap();
        assert_eq!(hash(&self.s), after, "{name} {args}: redo");
        // A dry run of the same thing would change nothing in the song.
        self.covered.insert(name.to_string());
        r
    }

    fn dry(&mut self, name: &str, args: Value) -> RunResult {
        let before = hash(&self.s);
        let steps = self.s.steps().len();
        let r = self
            .s
            .run_value(name, args.clone(), &RunOptions::human().dry())
            .unwrap_or_else(|e| panic!("dry {name} {args}: {e:?}"));
        assert!(r.dry_run);
        assert_eq!(hash(&self.s), before, "dry run of {name} must not edit");
        assert_eq!(self.s.steps().len(), steps);
        assert!(
            !r.changes.is_empty(),
            "dry run of {name} shows what changes"
        );
        r
    }
}

#[test]
fn every_history_command_undoes_and_redoes() {
    let (_saved_tmp, saved) = temp_dir("all-commands");
    let mut rig = Rig {
        s: Session::new("all"),
        covered: BTreeSet::new(),
    };
    let start = hash(&rig.s);

    // tracks, buses, folders
    let kick = id_of(&rig.step("track.add.audio", json!({"name": "Kick"})));
    let lead = id_of(&rig.step("track.add.instrument", json!({"name": "Lead"})));
    let bus = id_of(&rig.step("track.add.bus", json!({"name": "Drums"})));
    let fx = id_of(&rig.step("track.add.bus", json!({"name": "Reverb"})));
    let f = id_of(&rig.step("folder.add", json!({"name": "Rhythm"})));
    let g = id_of(&rig.step("folder.add", json!({"name": "Inner", "into": f})));
    rig.step("track.rename", json!({"id": kick, "name": "Kick 2"}));
    rig.step("folder.rename", json!({"id": f, "name": "Rhythm section"}));
    rig.step("track.move", json!({"ids": [kick], "into": g}));
    rig.step("track.move", json!({"ids": [kick], "out": true}));
    rig.step("track.move", json!({"ids": [lead], "by": 1}));
    rig.step("track.move", json!({"ids": [lead], "into": f, "index": 0}));
    rig.step("track.mute", json!({"ids": [kick]}));
    rig.step("track.solo", json!({"ids": [kick], "on": true}));
    rig.step("track.solo.clear", json!({}));
    rig.step("track.mute.clear", json!({}));
    rig.step("track.arm", json!({"ids": [lead]}));
    rig.step("track.monitor", json!({"ids": [lead]}));
    rig.step("track.input.gain", json!({"ids": [lead], "db": 6}));
    rig.step("track.volume.set", json!({"ids": [kick], "db": -6.5}));
    rig.step("track.pan.set", json!({"ids": [kick], "pan": 0.25}));
    rig.step("track.transpose", json!({"ids": [lead], "semis": -12}));
    rig.step("track.velocity", json!({"ids": [lead], "percent": 10}));
    rig.step(
        "track.color.set",
        json!({"ids": [kick, f], "color": "#ff8800"}),
    );
    rig.step("track.icon.set", json!({"ids": [kick], "icon": "drums"}));
    rig.step(
        "track.input.set",
        json!({"ids": [kick], "input": "Input 1"}),
    );
    rig.step("track.output.set", json!({"ids": [kick], "to": bus}));
    rig.step("folder.bus.set", json!({"id": f, "bus": fx}));
    rig.step("folder.bus.add", json!({"id": g, "name": "Inner bus"}));
    rig.step("track.duplicate", json!({"ids": [kick]}));
    rig.step(
        "track.duplicate",
        json!({"ids": [kick], "into": f, "index": 0}),
    );
    rig.step(
        "track.duplicate",
        json!({"ids": [kick], "mode": "settings"}),
    );
    rig.step(
        "track.add.bus",
        json!({"name": "Grouped", "sources": [kick]}),
    );
    let packed = id_of(&rig.step("folder.pack", json!({"ids": [kick, lead]})));
    rig.step("folder.duplicate", json!({"id": packed}));
    rig.step("folder.unpack", json!({"id": packed}));

    // sends and plug-ins
    let send = id_of(&rig.step("send.add", json!({"ids": [kick], "to": fx, "level": -12})));
    rig.step("send.mute", json!({"id": send}));
    rig.step("send.mute", json!({"id": send}));
    rig.step("send.pre", json!({"id": send}));
    let target_bus = id_of(&rig.step("track.add.bus", json!({"name": "Send target"})));
    rig.step("send.target.set", json!({"id": send, "to": target_bus}));
    rig.step(
        "send.set",
        json!({"id": send, "level": -3, "position": "pre"}),
    );
    rig.step("send.level.set", json!({"id": send, "level": -9}));
    rig.step("send.level.reset", json!({"id": send}));
    rig.step("send.remove", json!({"ids": [send]}));
    rig.step(
        "send.add.bus",
        json!({"ids": [kick, lead], "name": "Space", "level": -6, "pre": true}),
    );
    let eq = id_of(&rig.step("plugin.add", json!({"track": kick, "name": "EQ"})));
    let comp = id_of(&rig.step("plugin.add", json!({"track": kick, "name": "Comp"})));
    rig.step("plugin.move", json!({"id": comp, "index": 0}));
    rig.step("plugin.copy_to", json!({"id":comp,"track":"master"}));
    rig.step("plugin.duplicate", json!({"id":comp}));
    rig.step("channel.settings.paste", json!({"ids":[lead],"from":kick}));
    let preset = json!({
        "version": 1, "name": "Keys", "kind": "audio", "volume_db": -3.0, "pan": 0.0,
        "inserts": [{"name": "EQ", "kind": "effect", "format": "builtin", "uid": "EQ", "bypass": false}],
        "sends": []
    })
    .to_string();
    let pt = id_of(&rig.step("track.add.audio", json!({"name": "Preset"})));
    rig.step("track.preset.put", json!({"ids":[pt],"preset":preset}));
    rig.step("track.preset.make", json!({"preset":preset}));
    rig.step("track.preset.origin", json!({"ids":[pt],"name":"Drums"}));
    let xpreset = json!({
        "format": "xirius-preset", "version": 1, "kind": "fx", "name": "Warm",
        "devices": [{"name": "EQ", "kind": "effect", "format": "builtin", "uid": "EQ",
                     "params": {"low_gain": 0.7}}]
    })
    .to_string();
    rig.step("preset.put", json!({"track": pt, "preset": xpreset}));
    let rt = id_of(&rig.step("track.add.audio", json!({"name": "Rack"})));
    let ra = id_of(&rig.step("plugin.add", json!({"track": rt, "name": "EQ"})));
    let rb = id_of(&rig.step("plugin.add", json!({"track": rt, "name": "Comp"})));
    let split = rig.step("rack.split.add", json!({"track": rt, "branches": 2}));
    let sp = id_of(&split);
    let b0 = split.data["branches"][0].as_str().unwrap().to_string();
    rig.step("rack.node.move", json!({"id": ra, "to": b0, "index": 0}));
    rig.step("rack.split.mode", json!({"id": sp, "mode": "channel"}));
    rig.step("rack.split.mode", json!({"id": sp, "mode": "normal"}));
    let b2 = id_of(&rig.step("rack.branch.add", json!({"split": sp})));
    rig.step(
        "rack.branch.set",
        json!({"id": b2, "gain": -6.0, "mute": "on"}),
    );
    rig.step("rack.branch.remove", json!({"id": b0}));
    rig.step("rack.node.move", json!({"id": rb, "to": rt, "index": 1}));
    rig.step("rack.flatten", json!({"track": rt}));
    rig.step(
        "plugin.preset.apply",
        json!({"id":comp,"values":[{"param":"threshold","value":0.3}]}),
    );
    let packed = id_of(&rig.step("folder.pack", json!({"ids":[kick]})));
    rig.step("folder.unpack", json!({"id":packed}));
    rig.step("plugin.bypass", json!({"ids": [eq]}));
    rig.step("plugin.deactivate", json!({"ids": [eq]}));
    rig.step("plugin.activate", json!({"ids": [eq]}));
    rig.step(
        "plugin.param.set",
        json!({"id": eq, "param": "gain", "value": 3.0}),
    );
    rig.step("plugin.state.set", json!({"id": eq, "state": "AAEC"}));
    let vst = id_of(&rig.step(
        "plugin.add",
        json!({"track": kick, "name": "Gain", "format": "vst3", "uid": "0123"}),
    ));
    rig.step(
        "plugin.isolation.set",
        json!({"ids": [vst], "mode": "dedicated"}),
    );
    rig.step("plugin.remove", json!({"ids": [eq]}));
    rig.step(
        "plugin.sidechain.add",
        json!({"id": comp, "from": lead, "level": -6, "pre": true}),
    );
    rig.step(
        "plugin.sidechain.set",
        json!({"id": comp, "from": lead, "position": "post", "level": -3}),
    );
    rig.step(
        "plugin.sidechain.enable",
        json!({"id": comp, "state": "off"}),
    );
    rig.step("plugin.sidechain.remove", json!({"id": comp, "from": lead}));
    rig.step("send.add", json!({"ids": [lead], "to": comp, "level": -6}));
    rig.step(
        "plugin.add",
        json!({"track": lead, "name": "Synth", "kind": "instrument"}),
    );
    rig.step(
        "plugin.replace",
        json!({"slot": "instrument", "track": lead, "name": "Sampler"}),
    );
    rig.step("plugin.replace", json!({"id": vst, "name": "Limiter"}));
    let played = rig.step(
        "track.add.with-instrument",
        json!({"instrument": "Sampler", "arm": true,
               "notes": [{"at": 0, "pitch": 60, "length": 480}]}),
    );
    rig.step(
        "plugin.preset.load",
        json!({"id": played.data["plugin"], "preset": "Bright",
               "values": [{"param": "Cutoff", "value": 0.8}]}),
    );
    rig.step(
        "midi.import.place",
        json!({"instrument": "Sampler", "at": 0,
               "parts": [{"name": "Lead", "clip": "song",
                          "notes": [{"at": 0, "pitch": 64, "length": 480}]}],
               "tempo": [{"tick": 0, "bpm": 100.0}],
               "meter": [{"bar": 1, "num": 4, "den": 4}]}),
    );

    // automation
    rig.step("auto.lane.add", json!({"track": kick, "target": "volume"}));
    rig.step(
        "auto.point.add",
        json!({"track": kick, "target": "volume", "at": 0, "value": -6.0}),
    );
    rig.step(
        "auto.point.add",
        json!({"track": kick, "target": "volume", "at": "2.1.0", "value": 0.0, "curve": 0.5}),
    );
    rig.step(
        "auto.point.move",
        json!({"track": kick, "target": "volume", "index": 1, "at": 1000, "value": -3.0}),
    );
    rig.step(
        "auto.segment.shape",
        json!({"track": kick, "target": "volume", "index": 0, "curve": -0.5}),
    );
    rig.step(
        "auto.points.move",
        json!({"track": kick, "target": "volume", "ticks": "1000", "by": 25, "value-by": -0.1}),
    );
    let ratio = format!("param:{comp}:ratio");
    rig.step(
        "auto.point.add",
        json!({"track": kick, "target": ratio, "at": 0, "value": 0.5}),
    );
    rig.step(
        "auto.lane.target",
        json!({"track": kick, "target": ratio, "to": "pan"}),
    );
    rig.step(
        "auto.point.delete",
        json!({"track": kick, "target": "pan", "index": 0}),
    );
    rig.step("auto.lane.hide", json!({"track": kick, "target": "pan"}));
    rig.step("auto.lane.add", json!({"track": kick, "target": "pan"}));
    rig.step("auto.lane.remove", json!({"track": kick, "target": "pan"}));
    rig.step("auto.mode", json!({"ids": [kick], "mode": "latch"}));
    rig.step(
        "auto.write",
        json!({"writes": [{"track": kick, "target": "volume", "from": 0, "to": 960,
                           "points": [{"tick": 0, "value": -2.0}, {"tick": 960, "value": -4.0}]}]}),
    );
    rig.step(
        "auto.point.set",
        json!({"track": kick, "target": "volume", "ticks": "0,960", "value": -5.0}),
    );
    rig.step(
        "auto.point.shape",
        json!({"track": kick, "target": "volume", "ticks": "0", "curve": 0.0, "step": true}),
    );
    rig.step("auto.clear", json!({"track": kick, "target": "volume"}));
    rig.step(
        "auto.draw",
        json!({"track": kick, "target": "volume", "shape": "sine", "from": 0, "to": 1920,
               "value": -6.0, "amp": 3.0, "period": 960}),
    );
    // a stretch of a lane (UI-241, AUT-010)
    rig.step(
        "range.shape",
        json!({"track": kick, "target": "volume", "from": 240, "to": 1440, "shape": "line"}),
    );
    rig.step(
        "range.trim",
        json!({"track": kick, "target": "volume", "from": 240, "to": 1440, "delta": 0.1}),
    );
    rig.step(
        "range.transform",
        json!({"track": kick, "target": "volume", "from": 0, "to": 1920, "scale": 0.5,
               "tilt": 0.1}),
    );

    // clips and notes
    let clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track": lead, "at": "1.1.0", "bars": 2, "name": "Hook"}),
    ));
    let audio = id_of(&rig.step(
        "clip.add.audio",
        json!({"track": kick, "at": 3840, "length": 1920, "file": "kick.wav"}),
    ));
    // a recording's takes: an audio take on each lap and a MIDI take, in one step
    let taken = rig.step(
        "record.take.add",
        json!({"takes": [
            {"track": kick, "start": 7680, "length": 1920, "file": "Recordings/take-1.wav"},
            {"track": kick, "start": 7680, "length": 960, "file": "Recordings/take-2.wav"},
            {"track": lead, "start": 7680, "length": 1920,
             "notes": [{"start": 0, "length": 240, "pitch": 60, "velocity": 99}]},
        ]}),
    );
    // the next and the previous take play in turn
    rig.step("take.next", json!({"id": taken.data["clips"][0]}));
    rig.step("take.prev", json!({"id": taken.data["clips"][0]}));
    // the earlier take is chosen to play over a part of the later one
    rig.step("take.activate", json!({"id": taken.data["clips"][0]}));
    rig.step("take.clean", json!({"id": taken.data["clips"][0]}));
    let extra = rig
        .step(
            "record.take.add",
            json!({"takes": [
                {"track": kick, "start": 7680, "length": 1920, "file": "take-extra.wav"}
            ]}),
        )
        .data["clips"][0]
        .clone();
    // the earlier take plays over a part of the one just added, which plays elsewhere
    rig.step(
        "take.comp",
        json!({"take": taken.data["clips"][0], "from": 7920, "to": 8160}),
    );
    rig.step("take.delete", json!({"id": extra}));
    // a take is copied out onto a track of its own, and the last lap is undone alone
    let laps = rig.step(
        "record.take.add",
        json!({"takes": [
            {"track": kick, "start": 15360, "length": 1920, "file": "lap-1.wav"},
            {"track": kick, "start": 15360, "length": 1920, "file": "lap-2.wav"},
        ]}),
    );
    rig.step("take.unpack", json!({"id": laps.data["clips"][0]}));
    rig.step("take.undo-lap", json!({"id": laps.data["clips"][0]}));
    // an imported file: on a new track with the file's tempo, then on an existing one
    rig.step(
        "clip.import.batch",
        json!({"files":[{"file":"a.wav","length":1920},{"file":"b.wav","length":960,"at":1920}]}),
    );
    rig.step(
        "clip.import.place",
        json!({"file": "Loops/Drums 92bpm.wav", "length": 3840, "bpm": 92}),
    );
    rig.step(
        "clip.import.place",
        json!({"file": "C:/Samples/Hit.wav", "length": 480, "track": kick, "at": 960}),
    );
    rig.step(
        "clip.relink",
        json!({"files":[{"from":"C:/Samples/Hit.wav","to":"Audio/Hit.wav"}]}),
    );
    let mut notes = Vec::new();
    for (i, pitch) in [60, 64, 67, 72].iter().enumerate() {
        notes.push(id_of(&rig.step(
            "note.add",
            json!({"clip": clip, "at": i * 500 + 3, "pitch": pitch, "length": "1/8", "velocity": 90}),
        )));
    }
    rig.step("note.move", json!({"ids": [notes[0], notes[1]], "by": 120}));
    rig.step(
        "note.put",
        json!({"clip": clip, "notes": [
            {"at": 2000, "length": 240, "pitch": 55, "velocity": 70, "muted": true},
            {"at": 2400, "length": 480, "pitch": 57, "velocity": 85}
        ]}),
    );
    rig.step("note.resize", json!({"ids": [notes[0]], "length": "1/4"}));
    rig.step("note.resize", json!({"ids": [notes[1]], "by": 30}));
    rig.step("note.quantize", json!({"ids": [clip], "grid": "1/16"}));
    rig.step(
        "note.quantize",
        json!({"ids": [notes[2]], "grid": "1/8t", "strength": 50, "lengths": true}),
    );
    rig.step("note.transpose", json!({"ids": [clip], "by": 12}));
    // a MIDI clip's own transpose, and writing it into the notes (UI-311)
    rig.step("clip.transpose", json!({"ids": [clip], "semis": 2}));
    rig.step("clip.transpose.write", json!({"ids": [clip]}));
    rig.step("note.velocity.set", json!({"ids": [notes[3]], "by": -20}));
    rig.step(
        "note.velocity.set",
        json!({"ids": [notes[0]], "value": 127}),
    );
    rig.step("note.mute", json!({"ids": [notes[1]]}));
    rig.step(
        "note.value.set",
        json!({"ids": [notes[1]], "field": "velocity", "value": 80}),
    );
    rig.step(
        "note.set",
        json!({"notes": [{"id": notes[2], "start": 40, "length": 200, "pitch": 70, "velocity": 64}]}),
    );
    let dup = rig.step(
        "note.duplicate",
        json!({"ids": [notes[2], notes[3]], "by": 1920, "pitch_by": -12}),
    );
    assert_eq!(dup.data["ids"].as_array().map(Vec::len), Some(2));
    rig.step("note.split", json!({"ids": [notes[2]], "at": 100}));
    rig.step(
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 10}, {"tick": 480, "value": 90}]}),
    );
    rig.step(
        "cc.point.set",
        json!({"clip": clip, "lane": "cc1", "ticks": "480", "value": 64}),
    );
    rig.step(
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc1", "ticks": "0", "shape": "linear"}),
    );
    rig.step(
        "cc.draw",
        json!({"clip": clip, "lane": "cc11", "shape": "triangle", "from": 0, "to": 960,
               "value": 64, "amp": 30, "period": 480}),
    );
    // cut, copy, paste (CTX-028): what a copy takes is the session's, outside the history
    let steps = rig.s.steps().len();
    run(
        &mut rig.s,
        "edit.copy",
        json!({"clip": clip, "lane": "cc1"}),
    );
    assert_eq!(rig.s.steps().len(), steps);
    rig.step("edit.paste", json!({"clip": clip, "at": 960}));
    rig.step("edit.duplicate", json!({"ids": [notes[1]]}));
    rig.step(
        "edit.cut",
        json!({"clip": clip, "lane": "cc1", "ticks": "960"}),
    );
    rig.step("edit.delete", json!({"ids": [notes[3]]}));
    rig.step("clip.rename", json!({"id": clip, "name": "Hook B"}));
    rig.step("clip.mute", json!({"ids": [audio]}));
    rig.step("clip.move", json!({"ids": [clip], "by": 480}));
    rig.step("clip.move", json!({"ids": [clip], "to": "2.1.0"}));
    rig.step("clip.resize", json!({"ids": [clip], "length": 5000}));
    rig.step("clip.split", json!({"ids": [clip], "at": 4840}));
    rig.step("clip.trim", json!({"ids": [clip], "by": 240}));
    rig.step("clip.trim", json!({"ids": [clip], "by": -120}));
    rig.step("clip.slip", json!({"ids": [audio], "by": -100}));
    let fx_track = id_of(&rig.step("track.add.audio", json!({"name": "Fx"})));
    let fx_a = id_of(&rig.step(
        "clip.add.audio",
        json!({"track": fx_track, "at": 0, "length": 1920, "file": "a.wav"}),
    ));
    let fx_b = id_of(&rig.step(
        "clip.add.audio",
        json!({"track": fx_track, "at": 1920, "length": 1920, "file": "b.wav"}),
    ));
    rig.step(
        "clip.fade.set",
        json!({"ids": [fx_a], "in": 240, "out": 120, "curve": 0.25}),
    );
    rig.step("clip.gain.set", json!({"ids": [fx_a], "db": -3.0}));
    rig.step("clip.gain.set", json!({"ids": [fx_a], "by": 1.5}));
    rig.step("clip.color.set", json!({"ids": [fx_a], "color": "#ff8800"}));
    rig.step("clip.fade.clear", json!({"ids": [fx_a]}));
    rig.step("clip.gain.reset", json!({"ids": [fx_a]}));
    rig.step(
        "clip.gain.point.add",
        json!({"id": fx_a, "at": 480, "db": -6.0}),
    );
    rig.step(
        "clip.gain.point.add",
        json!({"id": fx_a, "at": 960, "db": 3.0}),
    );
    rig.step(
        "clip.gain.point.move",
        json!({"id": fx_a, "index": 0, "at": 500, "db": -9.0}),
    );
    rig.step("clip.gain.point.delete", json!({"id": fx_a, "index": 1}));
    rig.step(
        "clip.fade.cross",
        json!({"ids": [fx_a, fx_b], "length": 120}),
    );
    rig.step(
        "clip.fade.cross",
        json!({"ids": [fx_b], "move": 30, "curve": 0.1}),
    );
    rig.step("clip.fade.cross", json!({"ids": [fx_b], "remove": true}));
    rig.step("clip.duplicate", json!({"ids": [audio], "count": 2}));
    rig.step("clip.move", json!({"ids": [audio], "by": 1}));
    rig.step("clip.quantize", json!({"ids": [audio], "grid": "1/4"}));
    rig.step("note.delete", json!({"ids": [notes[0]]}));
    let moved_to = id_of(&rig.step("track.add.audio", json!({"name": "Dest"})));
    rig.step("clip.move", json!({"ids": [audio], "track": moved_to}));
    rig.step("clip.delete", json!({"ids": [audio]}));

    let edit_track = id_of(&rig.step("track.add.instrument", json!({"name":"Edit"})));
    let edit_clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track":edit_track,"at":0,"length":1920}),
    ));
    rig.step("note.put",json!({"clip":edit_clip,"notes":[{"at":0,"pitch":60,"length":480,"velocity":80},{"at":480,"pitch":60,"length":480,"velocity":100}]}));
    rig.step(
        "note.velocity.shape",
        json!({"ids":[edit_clip],"shape":"softer"}),
    );
    rig.step("note.humanize", json!({"ids":[edit_clip],"seed":19}));
    rig.step("note.join", json!({"ids":[edit_clip]}));
    let copy = rig.step("clip.duplicate", json!({"ids":[edit_clip]})).data["ids"][0].clone();
    let chord_clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track":edit_track,"at":3840,"length":1920}),
    ));
    rig.step("note.put",json!({"clip":chord_clip,"notes":[{"at":0,"pitch":60,"length":480,"velocity":80},{"at":480,"pitch":62,"length":480,"velocity":100}]}));
    rig.step(
        "note.velocity.humanize",
        json!({"ids":[chord_clip],"seed":3}),
    );
    rig.step(
        "note.velocity.transform",
        json!({"ids":[chord_clip],"scale":50,"tilt":10}),
    );
    rig.step("note.put",json!({"clip":chord_clip,"notes":[{"at":960,"pitch":60,"length":480,"velocity":90},{"at":960,"pitch":64,"length":480,"velocity":90},{"at":960,"pitch":67,"length":480,"velocity":90}]}));
    rig.step("note.strum", json!({"ids":[chord_clip],"spread":60}));
    rig.step("note.split.grid", json!({"ids":[chord_clip],"grid":"1/16"}));
    rig.step("clip.join", json!({"ids":[edit_clip,copy]}));
    rig.step(
        "clip.split.range",
        json!({"ids":[edit_clip],"start":240,"end":480}),
    );
    let span_clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track":edit_track,"at":30720,"length":1920}),
    ));
    let copies = rig
        .step(
            "range.duplicate",
            json!({"ids":[span_clip],"start":31200,"end":31680}),
        )
        .data["ids"]
        .clone();
    rig.step(
        "range.delete",
        json!({"ids":copies,"start":31680,"end":32160}),
    );
    let cut_clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track":edit_track,"at":40960,"length":1920}),
    ));
    rig.step(
        "range.cut",
        json!({"ids":[cut_clip],"start":41200,"end":41680}),
    );
    rig.step("clip.replace", json!({"id":fx_a,"file":"replacement.wav"}));
    // tempo, speed and pitch of an audio clip (EDT-007, EDT-016)
    rig.step("clip.stretch", json!({"ids":[fx_a],"follow":"on"}));
    rig.step("clip.tempo.set", json!({"ids":[fx_a],"bpm":96}));
    rig.step("clip.speed.set", json!({"ids":[fx_a],"percent":150}));
    rig.step("clip.transpose", json!({"ids":[fx_a],"semis":-3}));
    rig.step("clip.tune", json!({"ids":[fx_a],"by":7}));
    // the clip's own loop (EDT-041)
    rig.step(
        "clip.loop.set",
        json!({"ids":[fx_a],"start":0,"length":480}),
    );
    rig.step(
        "clip.reverse.apply",
        json!({"clips":[{"id":fx_a,"reverse":true,"offset":120}]}),
    );
    rig.step(
        "clip.normalize.apply",
        json!({"clips":[{"id":fx_a,"gain_db":4.5}]}),
    );
    rig.step(
        "clip.tempo.detect.apply",
        json!({"clips":[{"id":fx_a,"bpm":128}]}),
    );
    rig.step(
        "bounce.apply",
        json!({
            "replace": [{"id": fx_a, "file": "Bounces/a-1.wav"}],
            "tracks": [{
                "after": fx_track,
                "like": fx_track,
                "name": "Fx バウンス",
                "clips": [{"name": "b", "file": "Bounces/b-1.wav", "start": 1920, "length": 2400}],
            }],
            "mute_clips": [fx_b],
        }),
    );
    let key_clip = id_of(&rig.step(
        "clip.add.midi",
        json!({"track":edit_track,"at":9600,"length":1920}),
    ));
    rig.step(
        "clip.nudge",
        json!({"ids":[key_clip],"dir":"right","grid":"1/16"}),
    );
    rig.step("clip.move.cursor", json!({"ids":[key_clip],"at":11520}));
    rig.step("clip.split.grid", json!({"ids":[key_clip],"grid":480}));
    // freezing (EXP-004)
    rig.step(
        "track.freeze.apply",
        json!({"tracks":[{"id":edit_track,"file":"Freeze/Edit-1.wav","start":0}]}),
    );
    rig.step("track.unfreeze", json!({"ids":[edit_track]}));

    // song settings
    let marker = id_of(&rig.step("marker.add", json!({"at": 3840})));
    rig.step("marker.add", json!({"at": 7680}));
    rig.step("marker.rename", json!({"id": marker, "name": "Verse"}));
    rig.step(
        "marker.color.set",
        json!({"ids": [marker], "color": "#ff8800"}),
    );
    rig.step("marker.loop", json!({"id": marker}));
    rig.step("marker.delete", json!({"ids": [marker]}));
    let t2 = id_of(&rig.step("tempo.set", json!({"at": "5.1.0", "bpm": 90, "ramp": true})));
    rig.step("tempo.set", json!({"bpm": 128}));
    rig.step("tempo.move", json!({"id":t2,"at":17280,"bpm":95}));
    rig.step(
        "project.settings",
        json!({"sample_rate": 44100, "bit_depth": 16}),
    );
    rig.step("loop.set", json!({"start": "1.1.0", "end": "3.1.0"}));
    rig.step("loop.start.set", json!({"at": 960}));
    rig.step("loop.end.set", json!({"at": 5760}));
    rig.step("loop.clear", json!({}));
    rig.step("tempo.delete", json!({"id": t2}));
    rig.step("tempo.set", json!({"at": "9.1.0", "bpm": 100}));
    rig.step(
        "tempo.draw",
        json!({"shape": "saw", "from": 960, "to": 3840, "value": 110, "amp": 20,
               "period": 960}),
    );
    rig.step(
        "range.shape",
        json!({"tempo": true, "from": 960, "to": 3840, "shape": "line"}),
    );
    rig.step(
        "range.trim",
        json!({"tempo": true, "from": 960, "to": 3840, "delta": 5}),
    );
    rig.step(
        "range.transform",
        json!({"tempo": true, "from": 960, "to": 3840, "scale": 0.5}),
    );
    rig.step("tempo.clear", json!({}));
    let ts = id_of(&rig.step("timesig.set", json!({"bar": 5, "num": 7, "den": 8})));
    rig.step("timesig.set", json!({"num": 3, "den": 4}));
    rig.step("timesig.delete", json!({"id": ts}));
    let sec = id_of(&rig.step(
        "section.add",
        json!({"name": "Verse", "at": "1.1.0", "bars": 4}),
    ));
    rig.step(
        "section.add",
        json!({"name": "Chorus", "at": "5.1.0", "bars": 4}),
    );
    rig.step("section.reorder", json!({"id": sec, "index": 1}));
    rig.step("section.rename", json!({"id": sec, "name": "Verse 1"}));
    rig.step("section.move", json!({"id": sec, "by": 960}));
    rig.step("section.resize", json!({"id": sec, "bars": 2}));
    rig.step("section.color.set", json!({"id": sec, "color": "#ff8800"}));
    let (from, len) = {
        let s = rig
            .s
            .project()
            .sections
            .iter()
            .find(|s| s.id.to_string() == sec);
        s.map(|s| (s.start, s.length)).unwrap()
    };
    rig.step("section.split", json!({"id": sec, "at": from + len / 2}));
    rig.step("section.duplicate", json!({"id": sec}));
    rig.step("loop.set", json!({"start": 960, "end": 1920}));
    rig.step("loop.duplicate", json!({}));
    rig.step("range.time.insert", json!({"start": 960, "length": 960}));
    rig.step("range.time.delete", json!({"start": 960, "end": 1920}));
    rig.s
        .run_value("section.copy", json!({"id": sec}), &RunOptions::human())
        .unwrap();
    let pasted = id_of(&rig.step("section.paste", json!({"at": 0})));
    rig.step("section.cut", json!({"id": pasted}));
    rig.step("section.delete", json!({"id": sec}));
    rig.step("project.rename", json!({"name": "Song X"}));

    // folder delete last: it reshapes the column
    rig.step("folder.delete", json!({"id": g}));
    rig.step("folder.delete", json!({"id": f, "with-tracks": true}));
    rig.step("track.delete", json!({"ids": [bus, fx]}));

    // versions are history steps too
    rig.s.save_as(saved).unwrap();
    let v = rig.s.save_version("before restore").unwrap();
    rig.step("track.add.audio", json!({"name": "After"}));
    rig.step("version.restore", json!({"id": v.id}));

    // every registered command that goes into the history was covered above
    // (the ai.* ones need a request or a question first: tests/ai_work.rs)
    let reg = Registry::standard();
    let uncovered: Vec<&str> = reg
        .all()
        .iter()
        .filter(|d| d.history && !d.name.starts_with("ai."))
        .map(|d| d.name)
        .filter(|n| !rig.covered.contains(*n))
        .collect();
    assert!(
        uncovered.is_empty(),
        "not covered by this test: {uncovered:?}"
    );

    // And the whole run unwinds to the empty song and replays forward.
    let end = hash(&rig.s);
    let steps = rig.s.steps().len() as u64;
    rig.s.undo(steps).unwrap();
    assert_eq!(hash(&rig.s), start);
    rig.s.redo(steps).unwrap();
    assert_eq!(hash(&rig.s), end);
    // Jump anywhere.
    let mid = rig.s.first_step() + steps / 2;
    rig.s.jump(mid).unwrap();
    assert_eq!(rig.s.cursor(), mid);
    rig.s.jump(rig.s.last_step()).unwrap();
    assert_eq!(hash(&rig.s), end);
}

#[test]
fn commands_that_do_not_belong_in_the_history_leave_no_step() {
    let reg = Registry::standard();
    for d in reg.all() {
        if d.name.starts_with("select.") || d.name.starts_with("view.") {
            assert!(!d.history, "{} must stay out of the history", d.name);
        }
    }
    let mut s = Session::new("sel");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let steps = s.steps().len();
    run(&mut s, "select.set", json!({"ids": [t]}));
    run(&mut s, "view.zoom", json!({"h": 2.0}));
    run(&mut s, "view.grid", json!({"grid": "1/8"}));
    run(&mut s, "view.snap", json!({"off": true}));
    run(&mut s, "track.list", json!({}));
    run(&mut s, "ops.list", json!({"grep": "note"}));
    assert_eq!(s.steps().len(), steps);
    // Selection fills an omitted `ids`.
    run(&mut s, "track.mute", json!({}));
    assert!(s.project().track(t.parse().unwrap()).unwrap().mute);
}

#[test]
fn a_command_that_changes_nothing_makes_no_step() {
    let mut s = Session::new("noop");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let steps = s.steps().len();
    let r = run(&mut s, "track.volume.set", json!({"ids": [t], "db": 0}));
    assert!(r.step.is_none());
    assert_eq!(s.steps().len(), steps);
}

#[test]
fn dry_run_reports_changes_without_editing() {
    let mut rig = Rig {
        s: Session::new("dry"),
        covered: BTreeSet::new(),
    };
    let t = id_of(&rig.step("track.add.instrument", json!({})));
    let c = id_of(&rig.step("clip.add.midi", json!({"track": t, "at": 0})));
    for i in 0..3 {
        rig.step(
            "note.add",
            json!({"clip": c, "at": 100 + i * 7, "pitch": 60 + i}),
        );
    }
    let r = rig.dry("note.transpose", json!({"ids": [c], "by": 12}));
    assert_eq!(r.changes.count("note"), 3);
    let r = rig.dry("note.quantize", json!({"ids": [c], "grid": "1/4"}));
    assert!(!r.changes.items.is_empty());
    let r = rig.dry("track.delete", json!({"ids": [t]}));
    assert!(r.changes.count("track") >= 1);
    // The `dry_run` argument means the same as the option (the form MCP uses).
    let before = hash(&rig.s);
    let r = run(
        &mut rig.s,
        "note.transpose",
        json!({"ids": [c], "by": 1, "dry_run": true}),
    );
    assert!(r.dry_run && r.step.is_none());
    assert_eq!(hash(&rig.s), before);
}

#[test]
fn a_failed_command_in_the_middle_changes_nothing() {
    let mut s = Session::new("atomic");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let c = id_of(&run(&mut s, "clip.add.midi", json!({"track": t, "at": 0})));
    let hi = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 0, "pitch": 126}),
    ));
    let lo = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 10, "pitch": 40}),
    ));
    let before = hash(&s);
    // The first note would pass, the second would go out of range.
    assert_eq!(
        code(try_run(
            &mut s,
            "note.transpose",
            json!({"ids": [lo, hi], "by": 5})
        )),
        ErrorCode::OutOfRange
    );
    assert_eq!(hash(&s), before);
}

#[test]
fn arguments_are_checked() {
    let mut s = Session::new("args");
    assert_eq!(
        code(try_run(&mut s, "nope.nothing", json!({}))),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        code(try_run(
            &mut s,
            "track.rename",
            json!({"id": "trk_999", "name": "x"})
        )),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(try_run(
            &mut s,
            "track.volume.set",
            json!({"ids": ["master"], "db": 99})
        )),
        ErrorCode::OutOfRange
    );
    assert_eq!(
        code(try_run(&mut s, "track.rename", json!({"id": "master"}))),
        ErrorCode::InvalidArgs
    );
}

// ---- the history -------------------------------------------------------------------------------------

#[test]
fn one_history_for_people_and_ai_with_the_author_recorded() {
    let mut s = Session::new("authors");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let c = id_of(&run(&mut s, "clip.add.midi", json!({"track": t, "at": 0})));
    let r = s
        .run_value(
            "note.add",
            json!({"clip": c, "at": 0, "pitch": 60}),
            &RunOptions::ai("claude"),
        )
        .unwrap();
    assert!(r.step.is_some());
    let rows = s.history();
    let last = rows.last().unwrap();
    assert!(matches!(&last.author, Author::Ai(n) if n == "claude"));
    assert!(matches!(rows[0].author, Author::Human));
    // The note carries the AI mark, and one undo removes it.
    let note = &s.project().clip(c.parse().unwrap()).unwrap().1.notes()[0];
    assert!(note.ai);
    s.undo(1).unwrap();
    assert!(
        s.project()
            .clip(c.parse().unwrap())
            .unwrap()
            .1
            .notes()
            .is_empty()
    );
    s.redo(1).unwrap();
    // A person's edit to the same note clears the mark.
    let nid = id_of(&run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 960, "pitch": 62}),
    ));
    assert!(!s.project().clip(c.parse().unwrap()).unwrap().1.notes()[1].ai);
    let _ = nid;
    // One AI operation over many notes is still one step.
    let before = s.steps().len();
    s.run_value(
        "note.transpose",
        json!({"ids": [c], "by": 7}),
        &RunOptions::ai("claude"),
    )
    .unwrap();
    assert_eq!(s.steps().len(), before + 1);
}

#[test]
fn a_new_edit_after_undo_drops_the_redo_branch() {
    let mut s = Session::new("branch");
    run(&mut s, "track.add.audio", json!({"name": "A"}));
    run(&mut s, "track.add.audio", json!({"name": "B"}));
    s.undo(1).unwrap();
    assert!(s.can_redo());
    run(&mut s, "track.add.audio", json!({"name": "C"}));
    assert!(!s.can_redo());
    assert_eq!(s.steps().len(), 2);
}

#[test]
fn history_is_capped() {
    let mut s = Session::new("cap");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    for i in 0..1100 {
        run(
            &mut s,
            "track.rename",
            json!({"id": t, "name": format!("n{i}")}),
        );
    }
    assert!(s.steps().len() <= 1000);
    s.undo(10_000).unwrap();
    assert_eq!(s.cursor(), s.first_step());
}

// ---- AI permission ---------------------------------------------------------------------------------------

#[test]
fn ai_permission_gates_what_ai_may_do() {
    let mut s = Session::new("perm");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let ai = RunOptions::ai("claude");

    // Edit (default): may edit, may not save or change its own permission.
    assert!(
        s.run_value("track.rename", json!({"id": t, "name": "x"}), &ai)
            .is_ok()
    );
    assert_eq!(
        code(s.run_value("project.save", json!({}), &ai)),
        ErrorCode::Denied
    );
    assert_eq!(
        code(s.run_value("ai.permission.set", json!({"level": "save"}), &ai)),
        ErrorCode::Denied
    );
    assert_eq!(
        code(s.run_value("project.open", json!({"path": "x"}), &ai)),
        ErrorCode::Denied
    );

    // Read: edits come back as a dry run.
    s.set_ai_permission(AiPermission::Read).unwrap();
    let before = hash(&s);
    let r = s
        .run_value("track.rename", json!({"id": t, "name": "y"}), &ai)
        .unwrap();
    assert!(r.dry_run && r.forced_dry_run && r.step.is_none());
    assert_eq!(hash(&s), before);
    assert!(s.run_value("track.list", json!({}), &ai).is_ok());

    // Off: nothing, not even reading.
    s.set_ai_permission(AiPermission::Off).unwrap();
    assert_eq!(
        code(s.run_value("track.list", json!({}), &ai)),
        ErrorCode::Denied
    );
    // A person is never gated.
    assert!(try_run(&mut s, "track.rename", json!({"id": t, "name": "z"})).is_ok());

    // Save: may save.
    let (_dir_tmp, dir) = temp_dir("perm");
    s.save_as(&dir).unwrap();
    s.set_ai_permission(AiPermission::Save).unwrap();
    assert!(s.run_value("project.save", json!({}), &ai).is_ok());
}

// ---- files -----------------------------------------------------------------------------------------------------

fn build_song(s: &mut Session) {
    let kick = id_of(&run(s, "track.add.audio", json!({"name": "Kick"})));
    let lead = id_of(&run(
        s,
        "track.add.instrument",
        json!({"name": "日本語のトラック"}),
    ));
    let f = id_of(&run(s, "folder.add", json!({"name": "Group"})));
    run(s, "track.move", json!({"ids": [kick], "into": f}));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({"track": lead, "at": 0, "bars": 2}),
    ));
    for i in 0..8 {
        run(
            s,
            "note.add",
            json!({"clip": c, "at": i * 480, "pitch": 60 + i}),
        );
    }
    run(s, "tempo.set", json!({"at": "3.1.0", "bpm": 90.5}));
    run(s, "section.add", json!({"name": "A", "at": 0, "bars": 2}));
}

#[test]
fn save_and_reopen_round_trip() {
    let (_dir_tmp, dir) = temp_dir("roundtrip");
    let mut s = Session::new("RT");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    let saved = hash(&s);
    let steps = s.steps().len();
    drop(s);

    // The folder is a plain folder of readable files.
    for f in ["manifest.toml", "project.json"] {
        assert!(dir.join(f).is_file(), "{f} missing");
    }
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(
        text.contains("日本語のトラック"),
        "text is stored as UTF-8, not escaped"
    );
    assert!(
        text.lines().count() > 20,
        "pretty-printed so a diff is readable"
    );
    assert!(!dir.join("session.lock").exists(), "closed cleanly");

    let mut s = Session::open(&dir).unwrap();
    assert_eq!(hash(&s), saved);
    assert_eq!(
        s.steps().len(),
        steps,
        "the history comes back with the song"
    );
    assert!(!s.is_dirty());
    // And it is still undoable after reopening.
    s.undo(1).unwrap();
    assert_ne!(hash(&s), saved);
    s.redo(1).unwrap();
    assert_eq!(hash(&s), saved);
}

#[test]
fn a_plugin_process_is_one_undo_step_and_comes_back_with_the_song() {
    let (_dir_tmp, dir) = temp_dir("isolation");
    let mut s = Session::new("Iso");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "A"})));
    let a = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "A", "format": "vst3", "uid": "aa"}),
    ));
    let b = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "B", "format": "vst3", "uid": "bb"}),
    ));
    let builtin = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    let mode = |s: &Session, id: &str| {
        let p = &s
            .project()
            .tracks
            .values()
            .find_map(|t| t.inserts.iter().find(|x| x.id.to_string() == id));
        p.expect("plugin").isolation
    };
    // Plugins run in the engine unless told otherwise, and a song that never chose saves
    // nothing new.
    assert_eq!(mode(&s, &a), PluginIsolation::InProcess);
    s.save_as(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(!text.contains("isolation"), "{text}");

    let before = hash(&s);
    let steps = s.steps().len();
    run(
        &mut s,
        "plugin.isolation.set",
        json!({"ids": [a, b], "mode": "shared"}),
    );
    run(
        &mut s,
        "plugin.isolation.set",
        json!({"ids": [b], "mode": "dedicated"}),
    );
    assert_eq!(s.steps().len(), steps + 2, "one step per change");
    assert_eq!(mode(&s, &a), PluginIsolation::Shared);
    assert_eq!(mode(&s, &b), PluginIsolation::Dedicated);
    // A built-in plugin has no process of its own to go to.
    assert_eq!(
        code(try_run(
            &mut s,
            "plugin.isolation.set",
            json!({"ids": [builtin], "mode": "shared"}),
        )),
        ErrorCode::Conflict
    );
    assert_eq!(
        code(try_run(
            &mut s,
            "plugin.isolation.set",
            json!({"ids": [a], "mode": "elsewhere"}),
        )),
        ErrorCode::InvalidArgs
    );

    s.save().unwrap();
    let saved = hash(&s);
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(text.contains("\"isolation\": \"dedicated\""), "{text}");
    drop(s);
    let mut s = Session::open(&dir).unwrap();
    assert_eq!(hash(&s), saved);
    assert_eq!(mode(&s, &a), PluginIsolation::Shared);
    assert_eq!(mode(&s, &b), PluginIsolation::Dedicated);
    // One undo takes back one change, also after reopening.
    s.undo(1).unwrap();
    assert_eq!(mode(&s, &b), PluginIsolation::Shared);
    s.undo(1).unwrap();
    assert_eq!(hash(&s), before);
}

#[test]
fn save_writes_only_after_an_explicit_save() {
    let (_dir_tmp, dir) = temp_dir("explicit");
    let mut s = Session::new("E");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    let saved = hash(&s);
    run(&mut s, "track.add.audio", json!({"name": "unsaved"}));
    assert!(s.is_dirty());
    s.save().unwrap();
    assert!(!s.is_dirty());
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_ne!(hash(&s), saved);
    assert!(s.project().tracks.values().any(|t| t.name == "unsaved"));
}

#[test]
fn a_crash_loses_nothing_and_the_song_opens_again() {
    let (_dir_tmp, dir) = temp_dir("crash");
    let mut s = Session::new("C");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    run(&mut s, "track.add.audio", json!({"name": "after the save"}));
    let last = hash(&s);
    // The process dies: no Drop, so the lock stays behind.
    s.simulate_crash();
    assert!(dir.join("session.lock").exists());
    let s = Session::open(&dir).unwrap();
    assert_eq!(hash(&s), last, "every step is on disk as it is made");
    assert!(
        s.notices.iter().any(|n| !n.is_empty()),
        "the person is told a crash was recovered"
    );
    // The state before the crash was kept as a version.
    assert!(
        s.versions()
            .unwrap()
            .iter()
            .any(|v| format!("{:?}", v.kind).contains("Crash"))
    );
}

#[test]
fn a_stale_temp_file_does_not_stop_the_song_opening() {
    let (_dir_tmp, dir) = temp_dir("stale");
    let mut s = Session::new("S");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    let saved = hash(&s);
    drop(s);
    // A half-written file from an interrupted save sits next to the real one.
    std::fs::write(dir.join("project.json.tmp"), "{ \"broken\": ").unwrap();
    let s = Session::open(&dir).unwrap();
    assert_eq!(hash(&s), saved);
}

#[test]
fn a_corrupt_project_is_reported_not_papered_over() {
    let (_dir_tmp, dir) = temp_dir("corrupt");
    let mut s = Session::new("K");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    drop(s);
    std::fs::write(dir.join("project.json"), "{ not json").unwrap();
    let err = Session::open(&dir).err().expect("must fail");
    assert!(
        matches!(err.code, ErrorCode::Corrupt | ErrorCode::Io),
        "{err:?}"
    );
}

#[test]
fn save_as_makes_a_second_song_and_leaves_the_first() {
    let (_a_tmp, a) = temp_dir("saveas-a");
    let (_b_tmp, b) = temp_dir("saveas-b");
    let mut s = Session::new("SA");
    build_song(&mut s);
    s.save_as(&a).unwrap();
    let first = hash(&s);
    run(&mut s, "track.add.audio", json!({"name": "only in b"}));
    s.save_as(&b).unwrap();
    assert_eq!(s.dir().unwrap(), b.as_path());
    drop(s);
    assert_eq!(hash(&Session::open(&a).unwrap()), first);
    assert!(
        Session::open(&b)
            .unwrap()
            .project()
            .tracks
            .values()
            .any(|t| t.name == "only in b")
    );
}

// ---- versions ------------------------------------------------------------------------------------------------------

#[test]
fn versions_can_be_named_listed_restored_and_opened_apart() {
    let (_dir_tmp, dir) = temp_dir("versions");
    let mut s = Session::new("V");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    let v1 = s.save_version("first draft").unwrap();
    let at_v1 = content(&s);
    run(&mut s, "track.add.audio", json!({"name": "later"}));
    run(&mut s, "tempo.set", json!({"bpm": 77}));
    let v2 = s.save_version("second draft").unwrap();
    assert_ne!(v1.id, v2.id);
    let names: Vec<String> = s
        .versions()
        .unwrap()
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert!(
        names.contains(&"first draft".to_string()) && names.contains(&"second draft".to_string())
    );

    // Open an old version as its own song: the current one is untouched.
    let (_apart_tmp, apart) = temp_dir("versions-apart");
    let before = content(&s);
    let other = s.open_version_as_song(&v1.id, &apart).unwrap();
    let mut same = other.project().clone();
    same.name = s.project().name.clone();
    same.next_id = 0;
    assert_eq!(same.content_hash(), at_v1);
    assert_eq!(content(&s), before);
    assert!(apart.join("project.json").is_file());
    drop(other);

    // Restore is one undoable step in the same history.
    let n = s.steps().len();
    s.run_value(
        "version.restore",
        json!({"id": v1.id}),
        &RunOptions::human(),
    )
    .unwrap();
    assert_eq!(content(&s), at_v1);
    assert_eq!(s.steps().len(), n + 1);
    s.undo(1).unwrap();
    assert_eq!(content(&s), before);
    s.redo(1).unwrap();
    assert_eq!(content(&s), at_v1);

    // It survives a reopen, and the later version is still there to go back to.
    s.save().unwrap();
    drop(s);
    let mut s = Session::open(&dir).unwrap();
    assert_eq!(content(&s), at_v1);
    s.run_value(
        "version.restore",
        json!({"id": v2.id}),
        &RunOptions::human(),
    )
    .unwrap();
    assert_eq!(content(&s), before);
    assert_eq!(
        code(try_run(&mut s, "version.restore", json!({"id": "v999"}))),
        ErrorCode::NotFound
    );
}

#[test]
fn automatic_save_points_are_made_and_pruned() {
    let (_dir_tmp, dir) = temp_dir("auto");
    let mut s = Session::new("AU");
    build_song(&mut s);
    s.save_as(&dir).unwrap();
    s.auto_interval_ms = 0;
    let mut made = 0;
    for i in 0..30 {
        run(&mut s, "track.add.audio", json!({"name": format!("t{i}")}));
        if s.autosave_if_due(xirius_core::store::now_ms() + i)
            .unwrap()
            .is_some()
        {
            made += 1;
        }
    }
    assert!(made > 5, "save points are made as work goes on");
    let autos = s
        .versions()
        .unwrap()
        .iter()
        .filter(|v| format!("{:?}", v.kind).contains("Auto"))
        .count();
    assert!(autos <= xirius_core::session::AUTO_KEEP);
}

// ---- the schema ----------------------------------------------------------------------------------------------------------

#[test]
fn every_command_has_a_name_in_the_right_form_and_a_json_schema() {
    let reg = Registry::standard();
    let mut seen = BTreeSet::new();
    for d in reg.all() {
        assert!(seen.insert(d.name), "{} is defined twice", d.name);
        assert!(d.name.contains('.'), "{} is not <target>.<verb>", d.name);
        assert!(
            d.name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '-' || c == '_'),
            "{}",
            d.name
        );
        assert!(!d.summary.is_empty(), "{} has no description", d.name);
        let schema = d.json_schema();
        assert_eq!(schema["name"], d.name);
        assert_eq!(schema["inputSchema"]["type"], "object");
        assert!(schema["inputSchema"]["properties"].is_object());
        assert!(!schema["description"].as_str().unwrap().is_empty());
        let mut names = BTreeSet::new();
        for a in &d.args {
            assert!(
                names.insert(a.name),
                "{}: argument {} twice",
                d.name,
                a.name
            );
        }
    }
    for must in [
        "track.add.audio",
        "track.mute",
        "clip.split",
        "note.add",
        "note.move",
        "note.quantize",
        "note.transpose",
        "note.velocity.set",
        "track.move",
        "select.set",
        "view.zoom",
        "ops.list",
        "history.undo",
        "version.restore",
    ] {
        assert!(reg.get(must).is_some(), "{must} is missing");
    }
    assert_eq!(reg.tool_schemas().len(), reg.all().len());
    assert!(
        reg.find(Some("note"))
            .iter()
            .all(|d| d.name.contains("note") || d.summary.contains("note"))
    );
}

#[test]
fn the_schema_says_which_arguments_are_required_and_their_ranges() {
    let reg = Registry::standard();
    let s = reg.get("note.transpose").unwrap().json_schema();
    let required: Vec<&str> = s["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(required.contains(&"by"));
    assert!(
        !required.contains(&"ids"),
        "ids may come from the selection"
    );
    assert_eq!(s["inputSchema"]["properties"]["by"]["type"], "integer");
    assert_eq!(s["inputSchema"]["properties"]["by"]["minimum"], -127.0);
    let n = reg.get("note.add").unwrap().json_schema();
    assert_eq!(n["inputSchema"]["properties"]["pitch"]["maximum"], 127.0);
}

// ---- speed -------------------------------------------------------------------------------------------------------------------

/// A song with one MIDI clip of 10,000 notes, in memory or in a song folder.
fn song_with_10k_notes(folder: Option<&PathBuf>) -> (Session, String) {
    let mut s = Session::new("perf");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "bars": 1000}),
    ));
    // Build 10,000 notes without 10,000 commands: put them in one project edit.
    {
        use std::sync::Arc;
        use xirius_core::model::{ClipBody, Note};
        let mut p = (*s.project_arc()).clone();
        let cid = c.parse().unwrap();
        let mut notes = Vec::with_capacity(10_000);
        for i in 0..10_000u64 {
            notes.push(Note {
                id: xirius_core::id::NoteId(p.alloc()),
                start: (i * 37 % 3_000_000) as i64 + (i % 7) as i64,
                length: 240,
                pitch: (36 + i % 60) as u8,
                velocity: 100,
                muted: false,
                ai: false,
            });
        }
        let (tid, clip) = p.clip_mut(cid).unwrap();
        let _ = tid;
        clip.length = 3_100_000;
        clip.body = ClipBody::Midi {
            notes: Arc::new(notes),
            transpose: 0,
        };
        s = match folder {
            Some(dir) => Session::create(dir, p).unwrap(),
            None => Session::from_project(p),
        };
    }
    assert_eq!(
        s.project()
            .clip(c.parse().unwrap())
            .unwrap()
            .1
            .notes()
            .len(),
        10_000
    );
    (s, c)
}

/// The same edit on a song that lives in a folder: every step is written to
/// the history before the command returns, and saving writes `project.json`.
#[test]
fn ten_thousand_notes_in_a_song_folder_cost_one_file_write_per_step() {
    let (_dir_tmp, dir) = temp_dir("perf-folder");
    let (mut s, c) = song_with_10k_notes(Some(&dir));
    let mut best_run = u128::MAX;
    let mut best_undo = u128::MAX;
    for _ in 0..5 {
        let t0 = Instant::now();
        let r = run(&mut s, "note.quantize", json!({"ids": [c], "grid": "1/16"}));
        let t1 = Instant::now();
        s.undo(1).unwrap();
        let t2 = Instant::now();
        assert!(r.step.is_some());
        best_run = best_run.min((t1 - t0).as_micros());
        best_undo = best_undo.min((t2 - t1).as_micros());
    }
    run(&mut s, "note.quantize", json!({"ids": [c], "grid": "1/16"}));
    let t0 = Instant::now();
    s.save().unwrap();
    let save_us = (Instant::now() - t0).as_micros();
    let size = std::fs::metadata(dir.join("project.json")).unwrap().len();
    let t0 = Instant::now();
    let reopened = Session::open(&dir).unwrap();
    let open_us = (Instant::now() - t0).as_micros();
    assert_eq!(
        reopened.project().content_hash(),
        s.project().content_hash()
    );
    eprintln!(
        "10k notes in a folder: quantize {best_run} us, undo {best_undo} us, save {save_us} us, \
         reopen {open_us} us, project.json {size} bytes"
    );
    assert!(best_run < 2_000_000, "a step took {best_run} us");
}

/// The CPU time the calling thread has used. On a busy machine the wall time of a step also
/// holds the time other processes had the CPU; the thread's own time is what the step costs.
#[cfg(windows)]
fn thread_cpu() -> std::time::Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: the pseudo handle of the calling thread needs no closing, and the four FILETIMEs
    // outlive the call.
    let ok = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    assert!(ok != 0, "GetThreadTimes failed");
    let ticks = |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
    // FILETIME counts 100 ns.
    std::time::Duration::from_nanos((ticks(kernel) + ticks(user)) * 100)
}

#[cfg(unix)]
fn thread_cpu() -> std::time::Duration {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `t` outlives the call.
    let r = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) };
    assert_eq!(r, 0, "clock_gettime failed");
    std::time::Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

#[test]
fn ten_thousand_notes_quantize_and_undo_within_50_ms() {
    const RUNS: u32 = 5;
    let (mut s, c) = song_with_10k_notes(None);
    let mut best_run = u128::MAX;
    let mut best_undo = u128::MAX;
    let mut best_total = u128::MAX;
    // A step is all on this thread, with no waiting: its CPU time is its cost. Windows counts
    // a thread's time in ticks of about 15.6 ms, so it is taken over all the runs at once.
    let cpu0 = thread_cpu();
    for _ in 0..RUNS {
        let t0 = Instant::now();
        let r = run(&mut s, "note.quantize", json!({"ids": [c], "grid": "1/16"}));
        let t1 = Instant::now();
        s.undo(1).unwrap();
        let t2 = Instant::now();
        assert!(r.step.is_some());
        best_run = best_run.min((t1 - t0).as_micros());
        best_undo = best_undo.min((t2 - t1).as_micros());
        best_total = best_total.min((t2 - t0).as_micros());
    }
    let cpu = ((thread_cpu() - cpu0) / RUNS).as_micros();
    eprintln!(
        "10k notes: quantize {best_run} us, undo {best_undo} us, together {best_total} us; \
         CPU {cpu} us a run"
    );
    // The 50 ms target is for the build users run. An unoptimised build is about 7 times slower
    // (23 ms against 3.5 ms on the development PC), so there it only catches a step that got 10
    // times slower. The wall time is not held to it: when the whole workspace's tests share the
    // CPU it holds the time the others had, and swings past both.
    let limit = if cfg!(debug_assertions) {
        500_000
    } else {
        50_000
    };
    assert!(cpu < limit, "quantize + undo took {cpu} us of CPU");
}

/// Counts what it was asked and what it was told (ARC-016).
struct Probe {
    runs: std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>,
    changes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl xirius_core::Extension for Probe {
    fn answers(&self, name: &str) -> bool {
        name.starts_with("probe.")
    }

    fn run(
        &mut self,
        cx: &xirius_core::ExtContext<'_>,
        def: &xirius_core::CommandDef,
        args: &xirius_core::Args,
        dry: bool,
    ) -> Result<xirius_core::command::Output, CmdError> {
        self.runs.lock().unwrap().push((def.name.to_string(), dry));
        let tracks = cx.project.tracks.len();
        let word = args.opt_str("word").unwrap_or("-").to_string();
        Ok(xirius_core::command::Output::with(
            format!("{word} / {tracks}"),
            json!({ "word": word, "tracks": tracks }),
        ))
    }

    fn song_changed(&mut self, _cx: &xirius_core::ExtContext<'_>) {
        self.changes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn an_extension_answers_the_commands_core_has_no_answer_for() {
    use std::sync::atomic::Ordering;
    use xirius_core::command::{ArgDef, CommandDef, Handler};
    use xirius_core::{Project, Risk};

    let mut defs = Registry::standard().all().to_vec();
    for (name, risk) in [("probe.look", Risk::Read), ("probe.save", Risk::Guarded)] {
        defs.push(CommandDef {
            name,
            summary: "試験の拡張",
            args: vec![ArgDef::text("word", "言葉")],
            history: false,
            risk,
            handler: Handler::Session,
            screen_only: false,
        });
    }
    defs.push(CommandDef {
        name: "orphan.look",
        summary: "答える拡張が無い",
        args: vec![],
        history: false,
        risk: Risk::Read,
        handler: Handler::Session,
        screen_only: false,
    });
    let registry = std::sync::Arc::new(Registry::from_defs(defs));
    let mut s = Session::with_registry(Project::new("ext"), registry);
    let runs = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let changes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    s.add_extension(Box::new(Probe {
        runs: runs.clone(),
        changes: changes.clone(),
    }));

    // It sees the song, gets checked arguments and does not become a history step.
    run(&mut s, "track.add.audio", json!({"name": "A"}));
    assert_eq!(changes.load(Ordering::Relaxed), 1);
    let r = run(&mut s, "probe.look", json!({"word": "hi"}));
    assert_eq!(
        r.data,
        json!({"word": "hi", "tracks": s.project().tracks.len()})
    );
    assert!(r.step.is_none());
    assert_eq!(s.steps().len(), 1);
    assert_eq!(changes.load(Ordering::Relaxed), 1, "a look is not a change");
    assert!(try_run(&mut s, "probe.look", json!({"nope": 1})).is_err());

    // Undo is a change too.
    s.run_value("history.undo", json!({}), &RunOptions::human())
        .unwrap();
    assert_eq!(changes.load(Ordering::Relaxed), 2);

    // A trial run is passed on as one.
    run(&mut s, "probe.save", json!({"dry_run": true}));
    assert_eq!(
        runs.lock().unwrap().last().cloned(),
        Some(("probe.save".to_string(), true))
    );

    // The permission check is core's: an AI without the save permission is held or refused.
    let ai = RunOptions::ai("bot");
    let held = s.run_value("probe.save", json!({}), &ai);
    assert!(
        held.as_ref().map_or(true, |r| r.decision.is_some()),
        "{held:?}"
    );
    assert!(
        s.check_permission("probe.save", &Author::Ai("bot".into()))
            .is_err()
    );
    assert!(
        s.check_permission("probe.look", &Author::Ai("bot".into()))
            .is_ok()
    );
    assert!(s.check_permission("probe.look", &Author::Human).is_ok());
    assert_eq!(
        try_run(&mut s, "orphan.look", json!({})).unwrap_err().code,
        ErrorCode::UnknownCommand
    );

    // It stays with the session when another song is opened.
    let (_dir_tmp, dir) = temp_dir("extension-other");
    Session::create(&dir, Project::new("other")).unwrap();
    let before = changes.load(Ordering::Relaxed);
    run(
        &mut s,
        "project.open",
        json!({"path": dir.display().to_string()}),
    );
    assert_eq!(s.project().name, "other");
    assert_eq!(changes.load(Ordering::Relaxed), before + 1);
    let r = run(&mut s, "probe.look", json!({}));
    assert_eq!(r.data["tracks"], json!(s.project().tracks.len()));
}

#[test]
fn track_icon_survives_save_reopen_and_undo() {
    let (_dir_tmp, dir) = temp_dir("track-icon");
    let mut s = Session::new("Icons");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "ドラム"})));
    let plain = id_of(&run(&mut s, "track.add.audio", json!({"name": "声"})));
    run(
        &mut s,
        "track.icon.set",
        json!({"ids": [t], "icon": "drums"}),
    );
    assert_eq!(
        s.project()
            .tracks
            .values()
            .filter(|t| t.icon.is_some())
            .count(),
        1
    );
    s.save_as(&dir).unwrap();
    let saved = hash(&s);
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(text.contains("\"icon\": \"drums\""), "{text}");
    assert_eq!(
        text.matches("\"icon\"").count(),
        1,
        "a track without an icon writes nothing, so old files stay byte-identical"
    );
    drop(s);

    let mut s = Session::open(&dir).unwrap();
    assert_eq!(hash(&s), saved);
    let icons: Vec<_> = s
        .project()
        .tracks
        .values()
        .map(|t| (t.name.clone(), t.icon.map(|i| i.name())))
        .collect();
    assert!(
        icons.contains(&("ドラム".to_string(), Some("drums"))),
        "{icons:?}"
    );
    assert!(icons.contains(&("声".to_string(), None)), "{icons:?}");

    run(
        &mut s,
        "track.icon.set",
        json!({"ids": [plain], "icon": "mic"}),
    );
    s.undo(1).unwrap();
    assert_eq!(hash(&s), saved);
    s.redo(1).unwrap();
    run(
        &mut s,
        "track.icon.set",
        json!({"ids": [plain], "icon": "none"}),
    );
    assert_eq!(
        code(try_run(
            &mut s,
            "track.icon.set",
            json!({"ids": [plain], "icon": "piano"})
        )),
        ErrorCode::InvalidArgs
    );
}

#[test]
fn takes_go_in_as_one_step_and_only_the_last_take_of_a_track_plays() {
    let mut s = Session::new("takes");
    let audio = id_of(&run(&mut s, "track.add.audio", json!({})));
    let inst = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let old = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": audio, "at": 0, "length": 3840, "file": "old.wav"}),
    ));
    let steps = s.steps().len();
    let r = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": audio, "start": 960, "length": 1920, "file": "Recordings/a-1.wav"},
            {"track": audio, "start": 960, "length": 1920, "file": "Recordings/a-2.wav"},
            {"track": inst, "start": 960, "length": 1920,
             "notes": [{"start": 10, "length": 100, "pitch": 64, "velocity": 80}]},
        ]}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(r.data["clips"].as_array().unwrap().len(), 3);
    let p = s.project();
    let a = &p.track(audio.parse().unwrap()).unwrap().clips;
    // The clip that was there stays as it was (row 1); the takes are rows 2 and 3, the earlier
    // one kept, muted.
    assert_eq!(a[0].id.to_string(), old);
    assert!(!a[0].muted);
    let takes: Vec<_> = a[1..]
        .iter()
        .map(|c| (c.name.as_str(), c.start, c.length, c.muted))
        .collect();
    assert_eq!(
        takes,
        vec![("Audio 1", 960, 1920, true), ("Audio 1", 960, 1920, false)]
    );
    let m = &p.track(inst.parse().unwrap()).unwrap().clips[0];
    assert!(!m.muted);
    assert_eq!(
        m.notes()
            .iter()
            .map(|n| (n.start, n.length, n.pitch, n.velocity))
            .collect::<Vec<_>>(),
        vec![(10, 100, 64, 80)]
    );
    s.undo(1).unwrap();
    assert_eq!(
        s.project()
            .track(audio.parse().unwrap())
            .unwrap()
            .clips
            .len(),
        1
    );
    assert!(
        s.project()
            .track(inst.parse().unwrap())
            .unwrap()
            .clips
            .is_empty()
    );

    // A take goes only on a track of its kind.
    let e = try_run(
        &mut s,
        "record.take.add",
        json!({"takes": [{"track": inst, "start": 0, "length": 10, "file": "x.wav"}]}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
}

/// The file of each lap, and whether it is muted: the laps are all named after the track.
fn laps_of(s: &Session, track: &str) -> Vec<(String, bool)> {
    s.project()
        .track(track.parse().unwrap())
        .unwrap()
        .clips
        .iter()
        .map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, .. } => (file.clone(), c.muted),
            _ => (String::new(), c.muted),
        })
        .collect()
}

#[test]
fn the_last_lap_is_undone_alone_and_the_one_before_plays_if_nothing_else_did() {
    let mut s = Session::new("laps");
    let audio = id_of(&run(&mut s, "track.add.audio", json!({})));
    let r = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": audio, "start": 0, "length": 1920, "file": "l-1.wav", "muted": true},
            {"track": audio, "start": 0, "length": 1920, "file": "l-2.wav", "muted": false},
            {"track": audio, "start": 0, "length": 900, "file": "l-3.wav", "muted": true},
        ]}),
    );
    let ids = &r.data["clips"];
    // The half lap on the bottom row goes; the selected lap goes on playing.
    run(&mut s, "take.undo-lap", json!({"id": ids[0]}));
    assert_eq!(
        laps_of(&s, &audio),
        [
            ("l-1.wav".to_string(), true),
            ("l-2.wav".to_string(), false)
        ]
    );
    // The selected lap is the bottom row now: undoing it hands the sound to the one above.
    run(&mut s, "take.undo-lap", json!({"id": ids[0]}));
    assert_eq!(laps_of(&s, &audio), [("l-1.wav".to_string(), false)]);
    // A take that lies over nothing has no laps to undo.
    let e = try_run(&mut s, "take.undo-lap", json!({"id": ids[0]})).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs);
    // One undo brings back one lap.
    s.undo(1).unwrap();
    assert_eq!(
        laps_of(&s, &audio),
        [
            ("l-1.wav".to_string(), true),
            ("l-2.wav".to_string(), false)
        ]
    );
}

#[test]
fn a_take_goes_to_a_new_track_next_to_its_own_and_one_undo_takes_it_back() {
    let mut s = Session::new("unpack");
    let audio = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let other = id_of(&run(&mut s, "track.add.audio", json!({"name": "Gtr"})));
    run(&mut s, "track.arm", json!({"ids": [audio], "on": true}));
    let r = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": audio, "start": 0, "length": 1920, "file": "u-1.wav"},
            {"track": audio, "start": 0, "length": 1920, "file": "u-2.wav"},
        ]}),
    );
    let before = s.project().tracks.len();
    let u = run(&mut s, "take.unpack", json!({"id": r.data["clips"][0]}));
    let p = s.project();
    assert_eq!(p.tracks.len(), before + 1);
    let new: xirius_core::id::TrackId = u.data["track"].as_str().unwrap().parse().unwrap();
    let t = p.track(new).unwrap();
    assert_eq!(t.name, "Vox テイク 1");
    assert!(!t.armed, "the copy is not armed");
    assert_eq!(t.clips.len(), 1);
    assert!(!t.clips[0].muted, "it plays on its own track");
    // The original is as it was, and the new track sits right after it.
    assert_eq!(p.track(audio.parse().unwrap()).unwrap().clips.len(), 2);
    let at = |id: &str| {
        p.root
            .iter()
            .position(|n| matches!(n, xirius_core::id::Node::Track(t) if t.to_string() == id))
            .unwrap()
    };
    assert_eq!(at(&new.to_string()), at(&audio) + 1);
    assert_eq!(at(&other), at(&audio) + 2);
    s.undo(1).unwrap();
    assert_eq!(s.project().tracks.len(), before);
}
