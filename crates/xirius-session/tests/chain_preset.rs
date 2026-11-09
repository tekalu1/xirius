//! The chain presets (PLG-065): an FX chain, an instrument with its FX or a whole
//! multi-instrument saved as one `.xpreset` under its kind's folder, listed only for that
//! kind, and loaded at a place of the rack in one undoable step with the song's own ids; the
//! chain's macros land on free knobs of the track, and a device this PC lacks stays as an
//! empty slot.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::auto::AutoTarget;
use xirius_core::controls::ControlId;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Project, Track};
use xirius_core::rack::{RackNode, Source};
use xirius_core::session::RunOptions;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

struct Rig {
    dir: PathBuf,
    s: Session,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(tag: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("xirius-chains-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::with_registry(Project::new("chains"), shared_registry());
    commands::attach_with(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
        ExtExtras {
            presets: Some(dir.clone()),
            places_file: Some(dir.join("browser.json")),
            track_presets: Some(dir.join("tracks")),
            ..ExtExtras::default()
        },
    );
    Rig { dir, s }
}

#[test]
fn favourite_notifications_use_names_and_custom_stores_do_not_import_global_favourites() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("browser.json");
    let mut s = Session::with_registry(Project::new("isolated"), shared_registry());
    commands::attach_with(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
        ExtExtras {
            places_file: Some(file.clone()),
            track_presets: Some(root.path().join("tracks")),
            ..ExtExtras::default()
        },
    );
    for id in [
        "device:sampler",
        "preset:missing-private-location",
        "device:uninstalled",
    ] {
        let out = s
            .run_value(
                "browser.preset.favourite",
                json!({"entry": id, "on": true}),
                &RunOptions::human(),
            )
            .unwrap();
        assert!(!out.message.contains(id), "{}", out.message);
        if id == "device:sampler" {
            assert!(out.message.contains("Sampler"));
        }
        assert!(
            out.data["favourites"]
                .as_array()
                .unwrap()
                .contains(&json!(id))
        );
        let saved = xirius_session::places::load(&file).unwrap();
        assert!(
            !saved.legacy_favourites_imported,
            "a custom store without a preset root has no legacy source"
        );
    }
    let before = xirius_session::places::load(&file).unwrap().favourites;
    for command in ["browser.preset.list", "location.list"] {
        run(&mut s, command, json!({}));
        let saved = xirius_session::places::load(&file).unwrap();
        assert!(!saved.legacy_favourites_imported);
        assert_eq!(saved.favourites, before);
    }
}

#[test]
fn factory_favourite_removal_clears_legacy_alias_and_survives_reopening() {
    let mut r = rig("factory-favourite-alias");
    let list = run(&mut r.s, "browser.preset.list", json!({}));
    let entry = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["source"] == "Factory")
        .unwrap();
    let id = entry["id"].as_str().unwrap().to_owned();
    let alias = format!(
        "preset:{}/{}",
        entry["device"].as_str().unwrap(),
        entry["name"].as_str().unwrap()
    );
    let file = r.dir.join("browser.json");
    let mut prefs = xirius_session::places::load(&file).unwrap();
    prefs
        .favourites
        .extend([id.clone(), alias.clone(), "device:uninstalled".into()]);
    xirius_session::places::save(&file, &prefs).unwrap();
    for on in [false, true, true, false] {
        let mut args = json!({"entry": id});
        args[if on { "on" } else { "off" }] = json!(true);
        run(&mut r.s, "browser.preset.favourite", args);
        let saved = xirius_session::places::load(&file).unwrap();
        assert_eq!(saved.favourites.contains(&id), on);
        assert!(!saved.favourites.contains(&alias));
        assert!(saved.favourites.contains("device:uninstalled"));
        let list = run(&mut r.s, "browser.preset.list", json!({}));
        assert_eq!(
            list["favourites"].as_array().unwrap().contains(&json!(id)),
            on
        );
    }
    let mut reopened = Session::with_registry(Project::new("reopened"), shared_registry());
    commands::attach_with(
        &mut reopened,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
        ExtExtras {
            presets: Some(r.dir.clone()),
            places_file: Some(file),
            track_presets: Some(r.dir.join("tracks")),
            ..ExtExtras::default()
        },
    );
    let list = run(&mut reopened, "browser.preset.list", json!({}));
    let saved = list["favourites"].as_array().unwrap();
    assert!(!saved.contains(&json!(id)) && !saved.contains(&json!(alias)));
    assert!(saved.contains(&json!("device:uninstalled")));
}

#[test]
fn browser_saves_subtrees_copies_factory_and_applies_in_one_step() {
    let mut r = rig("browser-subtrees");
    let t = instrument(&mut r.s, "Keys", "Sampler");
    let inst = track(&r.s, &t).inserts[0].id.to_string();
    plugin(&mut r.s, &t, "EQ");
    let save = json!({"track": t, "kind": "instrument", "of": inst, "name": "Warm", "folder": "Keys", "include_fx": true});
    let out = run(&mut r.s, "browser.preset.save", save.clone());
    assert_eq!(out["has_fx"], true);
    assert!(fails(&mut r.s, "browser.preset.save", save.clone()).contains("上書き"));
    let mut over = save.clone();
    over["overwrite"] = json!(true);
    run(&mut r.s, "browser.preset.save", over);
    let mut forbidden = save;
    forbidden["source"] = json!("Factory");
    assert!(!fails(&mut r.s, "browser.preset.save", forbidden).is_empty());
    let list = run(
        &mut r.s,
        "browser.preset.list",
        json!({"query": "warm user keys sampler"}),
    );
    let entries = list["entries"].as_array().unwrap();
    let entry = entries.iter().find(|e| e["name"] == "Warm").unwrap()["id"].clone();
    let factory = entries.iter().find(|e| e["source"] == "Factory").unwrap()["id"].clone();
    let copy = run(
        &mut r.s,
        "browser.preset.copy",
        json!({"entry": factory, "name": "Copy", "folder": "Room"}),
    );
    assert!(Path::new(copy["file"].as_str().unwrap()).exists());
    let fav = run(
        &mut r.s,
        "browser.preset.favourite",
        json!({"entry": entry}),
    );
    assert!(fav["favourites"].as_array().unwrap().contains(&entry));
    let unfav = run(
        &mut r.s,
        "browser.preset.favourite",
        json!({"entry": entry}),
    );
    assert!(!unfav["favourites"].as_array().unwrap().contains(&entry));
    let prefs_file = r.dir.join("browser.json");
    let mut prefs = xirius_session::places::load(&prefs_file).unwrap();
    let old = "preset:sampler/Warm";
    prefs.favourites.insert(old.into());
    xirius_session::places::save(&prefs_file, &prefs).unwrap();
    let removed = run(
        &mut r.s,
        "browser.preset.favourite",
        json!({"entry": entry}),
    );
    assert!(
        !removed["favourites"]
            .as_array()
            .unwrap()
            .contains(&json!(old))
    );
    assert!(
        !xirius_session::places::load(&prefs_file)
            .unwrap()
            .favourites
            .contains(old)
    );
    let restored = run(&mut r.s, "browser.preset.favourite", json!({"entry": old}));
    assert!(restored["favourites"].as_array().unwrap().contains(&entry));
    assert!(
        !restored["favourites"]
            .as_array()
            .unwrap()
            .contains(&json!(old))
    );
    let target = instrument(&mut r.s, "Target", "Sampler");
    plugin(&mut r.s, &target, "Delay");
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry": entry, "track": target, "target": "instrument"}),
    );
    assert_eq!(uids(&r.s, &target), ["sampler", "eq"]);
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry": entry, "target": "new"}),
    );
    let made =
        r.s.project()
            .tracks
            .values()
            .find(|t| t.name == "Warm")
            .unwrap();
    assert_eq!(made.inserts[0].preset.as_deref(), Some("Warm"));
    let plain = run(
        &mut r.s,
        "browser.preset.save",
        json!({"track": t, "of": inst, "kind": "instrument", "name": "Plain"}),
    );
    assert_eq!(plain["has_fx"], false);
    let list = run(&mut r.s, "browser.preset.list", json!({}));
    let plain = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Plain")
        .unwrap()["id"]
        .clone();
    plugin(&mut r.s, &target, "Reverb");
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry": plain, "track": target}),
    );
    assert_eq!(uids(&r.s, &target), ["sampler", "eq", "reverb"]);
}

fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}

#[test]
fn browser_saving_a_layer_keeps_only_its_fx_tree_and_internal_macros() {
    let mut r = rig("browser-layer-scope");
    let t = instrument(&mut r.s, "Stack", "Sampler");
    run(
        &mut r.s,
        "rack.layer.add",
        json!({"track":t,"name":"Drum Kit","uid":"drum-kit"}),
    );
    let x = track(&r.s, &t);
    let Some(Source::Multi { layers, .. }) = &x.rack.as_ref().unwrap().source else {
        panic!("multi source");
    };
    let layer = layers[0].id.to_string();
    let inst = layers[0].instrument.to_string();
    let split = run(
        &mut r.s,
        "rack.split.add",
        json!({"track":t,"at":format!("layer:{layer}")}),
    );
    let branch = split["branches"][0].as_str().unwrap();
    let eq = plugin(&mut r.s, &t, "EQ");
    run(&mut r.s, "rack.node.move", json!({"id":eq,"to":branch}));
    plugin(&mut r.s, &t, "Reverb");
    run(
        &mut r.s,
        "control.assign",
        json!({"track":t,"control":"k1","target":format!("param:{eq}:low_gain")}),
    );
    run(
        &mut r.s,
        "control.assign",
        json!({"track":t,"control":"k2","target":"volume"}),
    );
    let saved = run(
        &mut r.s,
        "browser.preset.save",
        json!({"track":t,"of":inst,"kind":"instrument","include_fx":true,"name":"Layer"}),
    );
    let raw: Value =
        serde_json::from_slice(&std::fs::read(saved["file"].as_str().unwrap()).unwrap()).unwrap();
    let (preset, _) = xirius_core::chain_preset::ChainPreset::parse(&raw, None).unwrap();
    assert_eq!(
        preset
            .chain
            .devices
            .iter()
            .map(|d| d.uid.as_str())
            .collect::<Vec<_>>(),
        ["sampler", "eq"]
    );
    assert!(matches!(
        preset.chain.rack.fx.as_slice(),
        [RackNode::Split(_)]
    ));
    assert_eq!(preset.chain.controls.get(ControlId::Knob(0)).maps.len(), 1);
    assert!(
        preset
            .chain
            .controls
            .get(ControlId::Knob(1))
            .maps
            .is_empty()
    );
}

#[test]
fn browser_reads_legacy_files_and_places_fx_at_each_destination() {
    let mut r = rig("browser-fx-targets");
    let source = audio(&mut r.s, "Source");
    let eq = plugin(&mut r.s, &source, "EQ");
    run(
        &mut r.s,
        "preset.save",
        json!({"track":source,"kind":"fx","of":eq,"name":"Legacy EQ"}),
    );
    let list = run(
        &mut r.s,
        "browser.preset.list",
        json!({"tab":"effects","query":"legacy eq"}),
    );
    let entry = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Legacy EQ")
        .unwrap()["id"]
        .clone();
    assert!(
        list["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == entry)
            .unwrap()["folder"]
            .as_array()
            .unwrap()
            .is_empty(),
        "legacy kind/device directories are not user folders"
    );
    let target = audio(&mut r.s, "Target");
    let delay = plugin(&mut r.s, &target, "Delay");
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":entry,"track":target,"target":"insert","at":format!("after:{delay}")}),
    );
    assert_eq!(uids(&r.s, &target), ["delay", "eq"]);
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":entry,"track":target,"target":"effect","of":delay}),
    );
    assert_eq!(uids(&r.s, &target), ["eq", "eq"]);
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":entry,"track":target,"target":"track"}),
    );
    assert_eq!(uids(&r.s, &target), ["eq", "eq", "eq"]);
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":entry,"target":"new"}),
    );
    assert!(
        !fails(
            &mut r.s,
            "browser.preset.apply",
            json!({"entry":entry,"track":target,"target":"instrument"})
        )
        .is_empty()
    );
    run(
        &mut r.s,
        "browser.preset.save",
        json!({"track":target,"kind":"track","name":"Channel"}),
    );
    let list = run(&mut r.s, "browser.preset.list", json!({"tab":"tracks"}));
    let channel = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Channel")
        .unwrap()["id"]
        .clone();
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":channel,"track":source,"target":"track"}),
    );
    assert_eq!(uids(&r.s, &source), ["eq", "eq", "eq"]);
}

fn fails(s: &mut Session, n: &str, a: Value) -> String {
    match s.run_value(n, a, &RunOptions::human()) {
        Ok(o) => panic!("{n} went through: {:?}", o.data),
        Err(e) => e.message,
    }
}

#[test]
fn browser_missing_device_retains_opaque_state_and_original_file() {
    let mut r = rig("browser-missing-state");
    let t = audio(&mut r.s, "Source");
    let eq = plugin(&mut r.s, &t, "EQ");
    let saved = run(
        &mut r.s,
        "preset.save",
        json!({"track":t,"kind":"fx","of":eq,"name":"Unavailable"}),
    );
    let path = Path::new(saved["file"].as_str().unwrap());
    let mut value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    value["devices"][0]["uid"] = json!("org.example.unavailable");
    value["devices"][0]["format"] = json!("clap");
    value["devices"][0]["state"] = json!("AAECAwQ=");
    value["devices"][0]["params"] = json!({"opaque-parameter":0.42});
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(path, &bytes).unwrap();
    let list = run(&mut r.s, "browser.preset.list", json!({"tab":"effects"}));
    let entry = list["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "Unavailable")
        .unwrap()["id"]
        .clone();
    let target = audio(&mut r.s, "Target");
    step(
        &mut r.s,
        "browser.preset.apply",
        json!({"entry":entry,"track":target}),
    );
    let x = track(&r.s, &target);
    assert_eq!(x.inserts[0].uid, "org.example.unavailable");
    assert_eq!(x.inserts[0].state.as_deref(), Some("AAECAwQ="));
    assert_eq!(x.inserts[0].params["opaque-parameter"], 0.42);
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// Runs an edit and checks it is one undo step there and back.
fn step(s: &mut Session, n: &str, a: Value) -> Value {
    let before: Project = s.project().clone();
    let k = s.steps().len();
    let v = run(s, n, a);
    assert_eq!(s.steps().len(), k + 1, "{n} is one step");
    let after: Project = s.project().clone();
    assert_ne!(after, before, "{n} changed the song");
    s.undo(1).expect("undo");
    assert_eq!(*s.project(), before, "{n} undoes in one step");
    s.redo(1).expect("redo");
    assert_eq!(*s.project(), after, "{n} redoes");
    v
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn track(s: &Session, t: &str) -> Arc<Track> {
    s.project().tracks[&t.parse::<TrackId>().unwrap()].clone()
}

fn audio(s: &mut Session, name: &str) -> String {
    id(&run(s, "track.add.audio", json!({ "name": name })))
}

/// An instrument track playing only the built-in `device`.
fn instrument(s: &mut Session, name: &str, device: &str) -> String {
    let t = id(&run(s, "track.add.instrument", json!({ "name": name })));
    let old: Vec<String> = track(s, &t)
        .inserts
        .iter()
        .map(|x| x.id.to_string())
        .collect();
    for x in old {
        run(s, "plugin.remove", json!({ "id": x }));
    }
    plugin(s, &t, device);
    t
}

/// A built-in on the track, under its canonical uid and kind.
fn plugin(s: &mut Session, t: &str, name: &str) -> String {
    let def = xirius_params::find(name).unwrap_or_else(|| panic!("no builtin {name}"));
    let kind = match def.kind {
        xirius_params::Kind::Effect => "effect",
        xirius_params::Kind::Instrument => "instrument",
    };
    id(&run(
        s,
        "plugin.add",
        json!({ "track": t, "name": name, "kind": kind, "format": "builtin", "uid": def.uid }),
    ))
}

fn uids(s: &Session, t: &str) -> Vec<String> {
    track(s, t).inserts.iter().map(|x| x.uid.clone()).collect()
}

fn fx(s: &Session, t: &str) -> Vec<RackNode> {
    track(s, t).rack.as_ref().expect("a rack").fx.clone()
}

fn knob(s: &Session, t: &str, k: u8) -> (Option<String>, Vec<AutoTarget>) {
    let c = track(s, t).controls.get(ControlId::Knob(k)).clone();
    (c.name, c.maps.into_iter().map(|m| m.target).collect())
}

fn names(s: &mut Session, kind: Option<&str>) -> Vec<String> {
    let a = match kind {
        Some(k) => json!({ "kind": k }),
        None => json!({}),
    };
    run(s, "preset.list", a)["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

/// A Compressor, then a splitter: EQ on one branch, Delay on the other. Knob 1 drives the
/// EQ's low gain, knob 2 the fader (which is not in the chain).
fn wide(s: &mut Session) -> (String, String, String) {
    let t = audio(s, "Vocal");
    plugin(s, &t, "Compressor");
    let split = run(s, "rack.split.add", json!({ "track": t }));
    let b: Vec<String> = split["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    let eq = plugin(s, &t, "EQ");
    run(s, "rack.node.move", json!({ "id": eq, "to": b[0] }));
    let delay = plugin(s, &t, "Delay");
    run(s, "rack.node.move", json!({ "id": delay, "to": b[1] }));
    run(
        s,
        "control.assign",
        json!({ "track": t, "control": "k1", "target": format!("param:{eq}:low_gain") }),
    );
    run(
        s,
        "control.assign",
        json!({ "track": t, "control": "k2", "target": "volume" }),
    );
    (t, eq, id(&split))
}

fn file(dir: &Path, kind: &str, name: &str) -> PathBuf {
    dir.join(kind).join(format!("{name}.xpreset"))
}

fn save_wide(r: &mut Rig) -> (String, String, String) {
    let w = wide(&mut r.s);
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": w.0, "kind": "fx-chain", "name": "Wide" }),
    );
    w
}

#[test]
fn an_fx_chain_is_saved_under_its_kind_and_listed_only_for_it() {
    let mut r = rig("save");
    let (t, _, _) = wide(&mut r.s);
    let n = r.s.steps().len();
    let out = run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx-chain", "name": "Wide" }),
    );
    assert_eq!(r.s.steps().len(), n, "a save is no edit");
    assert_eq!(out["dropped"], json!(1), "the fader's map stays behind");
    let path = file(&r.dir, "fx-chain", "Wide");
    assert!(path.is_file(), "{path:?}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(v["kind"], json!("fx-chain"));
    assert!(names(&mut r.s, Some("fx-chain")).contains(&"Wide".to_string()));
    assert!(!names(&mut r.s, None).contains(&"Wide".to_string()));
    assert!(names(&mut r.s, Some("instrument-fx")).is_empty());
    // One branch, or one splitter, is a chain too.
    let RackNode::Split(split) = fx(&r.s, &t)[1].clone() else {
        panic!("{:?}", fx(&r.s, &t))
    };
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx-chain", "of": split.branches[1].id.to_string(), "name": "Echo" }),
    );
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx-chain", "of": split.id.to_string(), "name": "Split" }),
    );
    assert_eq!(names(&mut r.s, Some("fx-chain")), ["Echo", "Split", "Wide"]);
}

#[test]
fn a_chain_loads_in_one_step_with_new_ids_and_its_macro_moves_to_a_free_knob() {
    let mut r = rig("load");
    let (_, eq, split) = save_wide(&mut r);
    let b = audio(&mut r.s, "Guitar");
    let reverb = plugin(&mut r.s, &b, "Reverb");
    run(
        &mut r.s,
        "control.assign",
        json!({ "track": b, "control": "k1", "target": format!("param:{reverb}:mix") }),
    );
    let out = step(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Wide" }),
    );
    assert_eq!(out["moved"], json!(["Macro 1 を Macro 2 に移しました"]));
    assert_eq!(uids(&r.s, &b), ["reverb", "compressor", "eq", "delay"]);
    let rack = fx(&r.s, &b);
    let [
        RackNode::Plugin(first),
        RackNode::Plugin(_),
        RackNode::Split(s),
    ] = &rack[..]
    else {
        panic!("{rack:?}")
    };
    assert_eq!(first.to_string(), reverb);
    assert_ne!(s.id.to_string(), split, "the song's own ids");
    let new_eq = track(&r.s, &b).inserts[2].id;
    assert_ne!(new_eq.to_string(), eq);
    assert!(matches!(s.branches[0].fx[..], [RackNode::Plugin(x)] if x == new_eq));
    // the track's own knob stays, the chain's lands on the next one with the preset's name
    assert_eq!(knob(&r.s, &b, 0).1.len(), 1);
    assert_eq!(
        knob(&r.s, &b, 1),
        (
            Some("Wide".to_string()),
            vec![AutoTarget::Param(new_eq, "low_gain".into())]
        )
    );
    // after a device
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Wide", "at": format!("after:{reverb}") }),
    );
    let rack = fx(&r.s, &b);
    assert_eq!(rack.len(), 5);
    assert!(matches!(rack[1], RackNode::Plugin(_)));
    assert!(matches!(rack[2], RackNode::Split(_)));
}

#[test]
fn replace_takes_the_old_chain_off_and_after_with_replace_says_no() {
    let mut r = rig("replace");
    save_wide(&mut r);
    let b = audio(&mut r.s, "Guitar");
    let reverb = plugin(&mut r.s, &b, "Reverb");
    run(
        &mut r.s,
        "control.assign",
        json!({ "track": b, "control": "k3", "target": format!("param:{reverb}:mix") }),
    );
    let e = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Wide", "at": format!("after:{reverb}"), "replace": true }),
    );
    assert!(e.contains("--replace"), "{e}");
    step(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Wide", "replace": true }),
    );
    assert_eq!(uids(&r.s, &b), ["compressor", "eq", "delay"]);
    assert!(
        knob(&r.s, &b, 2).1.is_empty(),
        "the Reverb's map went with it"
    );
    assert_eq!(knob(&r.s, &b, 0).1.len(), 1, "the chain's took knob 1");
}

#[test]
fn a_chain_too_deep_for_the_place_is_refused() {
    let mut r = rig("deep");
    let t = audio(&mut r.s, "Deep");
    let outer = run(&mut r.s, "rack.split.add", json!({ "track": t }));
    let b0 = outer["branches"][0].as_str().unwrap().to_string();
    run(
        &mut r.s,
        "rack.split.add",
        json!({ "track": t, "at": format!("branch:{b0}") }),
    );
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx-chain", "of": id(&outer), "name": "Two" }),
    );
    let u = audio(&mut r.s, "Target");
    let mut at = "track".to_string();
    for _ in 0..3 {
        let s = run(&mut r.s, "rack.split.add", json!({ "track": u, "at": at }));
        at = format!("branch:{}", s["branches"][0].as_str().unwrap());
    }
    let e = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": u, "preset": "Two", "at": at }),
    );
    assert!(e.contains("4 段"), "{e}");
}

#[test]
fn an_instrument_with_its_fx_takes_the_seat_and_only_on_an_instrument_track() {
    let mut r = rig("inst");
    let t = instrument(&mut r.s, "Keys", "Sampler");
    plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "instrument-fx", "name": "Keys Chain" }),
    );
    let u = instrument(&mut r.s, "Drums", "Drum Kit");
    plugin(&mut r.s, &u, "Reverb");
    step(
        &mut r.s,
        "preset.load",
        json!({ "track": u, "preset": "Keys Chain" }),
    );
    assert_eq!(uids(&r.s, &u), ["sampler", "eq"]);
    let x = track(&r.s, &u);
    let sampler = x.inserts[0].id;
    assert_ne!(sampler, track(&r.s, &t).inserts[0].id);
    // a plain chain needs no rack; one kept names the new instrument
    assert!(
        x.rack
            .as_ref()
            .is_none_or(|r| r.source == Some(Source::Single(sampler))),
        "{:?}",
        x.rack
    );
    let v = audio(&mut r.s, "Audio");
    let e = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": v, "preset": "Keys Chain" }),
    );
    assert!(e.contains("インストゥルメントトラック"), "{e}");
    let e = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": u, "preset": "Keys Chain", "at": format!("after:{sampler}") }),
    );
    assert!(e.contains("--at track"), "{e}");
}

#[test]
fn a_multi_instrument_comes_back_whole() {
    let mut r = rig("multi");
    let t = instrument(&mut r.s, "Stack", "Sampler");
    run(
        &mut r.s,
        "rack.layer.add",
        json!({ "track": t, "name": "Drum Kit", "uid": "drum-kit", "keys": "C1-B2" }),
    );
    let e = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "instrument-fx", "name": "No" }),
    );
    assert!(e.contains("multi-instrument"), "{e}");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "multi-instrument", "name": "Stack" }),
    );
    let u = instrument(&mut r.s, "Other", "Drum Kit");
    step(
        &mut r.s,
        "preset.load",
        json!({ "track": u, "preset": "Stack" }),
    );
    let x = track(&r.s, &u);
    let Some(Source::Multi { layers, .. }) = &x.rack.as_ref().unwrap().source else {
        panic!("{:?}", x.rack)
    };
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1].keys, (36, 59));
    let ids: Vec<PluginId> = x.inserts.iter().map(|p| p.id).collect();
    assert!(layers.iter().all(|l| ids.contains(&l.instrument)));
    assert_eq!(uids(&r.s, &u), ["sampler", "drum-kit"]);
}

#[test]
fn a_device_this_pc_lacks_keeps_its_slot_and_macro_reference() {
    let mut r = rig("missing");
    save_wide(&mut r);
    let text = std::fs::read_to_string(file(&r.dir, "fx-chain", "Wide")).unwrap();
    let b = audio(&mut r.s, "Guitar");
    let out = run(
        &mut r.s,
        "preset.put",
        json!({ "track": b, "preset": text, "missing": "eq" }),
    );
    assert_eq!(
        uids(&r.s, &b),
        ["compressor", "eq", "delay"],
        "the slot stays"
    );
    let rows = out["adjusted"].as_array().unwrap();
    let said = |what: &str, why: &str| {
        rows.iter().any(|x| {
            x["what"].as_str().unwrap().contains(what)
                && x["reason"].as_str().unwrap().contains(why)
        })
    };
    assert!(said("EQ", "この PC に無い"), "{rows:?}");
    assert!(!said("マクロ", "この PC に無い機器"), "{rows:?}");
    assert!(!knob(&r.s, &b, 0).1.is_empty());
}

#[test]
fn a_device_preset_with_a_place_goes_in_as_a_chain_of_one() {
    let mut r = rig("one");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Bright" }),
    );
    let comp = plugin(&mut r.s, &t, "Compressor");
    step(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "Bright", "device": "eq", "at": format!("after:{eq}") }),
    );
    assert_eq!(
        uids(&r.s, &t),
        ["eq", "eq", "compressor"],
        "in the rack's order"
    );
    let all = &track(&r.s, &t).inserts;
    assert_eq!(all[0].id.to_string(), eq);
    assert_eq!(all[2].id.to_string(), comp);
    let x = &all[1];
    assert_eq!(x.preset.as_deref(), Some("Bright"));
}

/// Review 3 中5: with its values the list gives each chain as a file keeps it, the same as a
/// track's chain the preset was loaded onto, so a window names a chain by the preset it is.
#[test]
fn the_list_with_values_gives_each_chain_to_tell_a_tracks_is_it() {
    use xirius_core::chain_preset::{Chain, ChainKind};
    let mut r = rig("shape");
    save_wide(&mut r);
    let rows = run(
        &mut r.s,
        "preset.list",
        json!({ "kind": "fx-chain", "values": true }),
    )["presets"]
        .clone();
    let wide = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Wide")
        .unwrap()["chain"]
        .clone();
    assert!(
        wide["devices"].as_array().is_some_and(|d| d.len() == 3),
        "{wide}"
    );
    let shape = |s: &Session, t: &str| {
        Chain::capture(&track(s, t), ChainKind::Fx, None)
            .unwrap()
            .0
            .shape()
    };
    let b = audio(&mut r.s, "Guitar");
    plugin(&mut r.s, &b, "Reverb");
    assert_ne!(shape(&r.s, &b), wide);
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Wide", "at": "track", "replace": true }),
    );
    assert_eq!(shape(&r.s, &b), wide, "the song's own ids aside");
    // A plug-in writing its own blob, or a new name, is not a change of what plays.
    let first = track(&r.s, &b).inserts[0].id.to_string();
    run(
        &mut r.s,
        "plugin.state.set",
        json!({ "id": first, "state": "AAEC" }),
    );
    run(
        &mut r.s,
        "plugin.rename",
        json!({ "id": first, "name": "Mine" }),
    );
    assert_eq!(shape(&r.s, &b), wide, "the blob and the name aside");
    // Its power is.
    run(
        &mut r.s,
        "plugin.bypass",
        json!({ "ids": [first], "on": true }),
    );
    assert_ne!(shape(&r.s, &b), wide, "bypassed");
    // Without the values the list stays as it was.
    let plain = run(&mut r.s, "preset.list", json!({ "kind": "fx-chain" }));
    assert!(plain["presets"][0].get("chain").is_none());
}

/// The fourth review's low 8: a chain of older keys (a Sampler's `tune`, a hand-written
/// `CUTOFF`, a knob mapped to `tune`) goes in under the song's, as a device preset does —
/// the pitch kept over the wider `transpose`.
#[test]
fn a_chain_of_older_keys_goes_in_under_the_songs() {
    let mut r = rig("old-keys");
    let t = instrument(&mut r.s, "Keys", "Sampler");
    let sampler = track(&r.s, &t).inserts[0].id;
    plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "control.assign",
        json!({ "track": t, "control": "k1", "target": format!("param:{sampler}:transpose") }),
    );
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "instrument-fx", "name": "Old Keys" }),
    );
    let path = file(&r.dir, "instrument-fx", "Old Keys");
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let params = v["devices"][0]["params"].as_object_mut().unwrap();
    params.retain(|k, _| k != "transpose" && k != "cutoff");
    params.insert("tune".into(), json!(0.25));
    params.insert("CUTOFF".into(), json!(0.4));
    let body = v.to_string();
    assert!(body.contains(":transpose\""), "{body}");
    std::fs::write(&path, body.replace(":transpose\"", ":tune\"")).unwrap();
    let u = instrument(&mut r.s, "Other", "Drum Kit");
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": u, "preset": "Old Keys" }),
    );
    let x = track(&r.s, &u);
    let def = xirius_params::find("sampler").unwrap();
    let plain = |v: f64| def.param("transpose").unwrap().1.to_plain(v);
    let s = &x.inserts[0];
    assert_eq!(s.uid, "sampler");
    assert!(!s.params.contains_key("tune") && !s.params.contains_key("CUTOFF"));
    assert_eq!(plain(s.params["transpose"]), -12.0);
    assert_eq!(s.params.get("cutoff"), Some(&0.4));
    let m = &x.controls.get(ControlId::Knob(0)).maps[0];
    assert_eq!(m.target, AutoTarget::Param(s.id, "transpose".into()));
    assert_eq!((plain(m.from), plain(m.to)), (-24.0, 24.0));
    let _ = std::fs::remove_dir_all(&r.dir);
}
