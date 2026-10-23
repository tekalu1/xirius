//! The device presets kept as `.xpreset` files (PLG-063): saved from a plug-in, listed one
//! per device with `Default` first, loaded in one undoable step, renamed and removed as
//! plain files; the factory presets are copied in once (the app's start, or the first save
//! or load) and
//! `preset.factory.restore` adds back only what is missing. Old `plugin.preset` files read
//! the way a song does (PRJ-007), and a file that is not a preset is not read at all.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::command::Risk;
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track};
use xirius_core::rack::{Layer, Rack, Source};
use xirius_core::session::RunOptions;
use xirius_core::store::sha256_hex;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

struct Rig {
    dir: PathBuf,
    s: Session,
}

fn session(dir: &Path) -> Session {
    session_with(dir, Project::new("presets"))
}

fn session_with(dir: &Path, p: Project) -> Session {
    let mut s = Session::with_registry(p, shared_registry());
    commands::attach_with(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            ..ExtConfig::default()
        },
        ExtExtras {
            presets: Some(dir.to_path_buf()),
            ..ExtExtras::default()
        },
    );
    s
}

fn rig(tag: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("xirius-xpresets-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Rig {
        dir: dir.clone(),
        s: session(&dir),
    }
}

fn run(s: &mut Session, n: &str, a: Value) -> Value {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"))
        .data
}

fn fails(s: &mut Session, n: &str, a: Value) -> String {
    match s.run_value(n, a, &RunOptions::human()) {
        Ok(o) => panic!("{n} went through: {:?}", o.data),
        Err(e) => e.message,
    }
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

/// The EQ's `low_gain` of the first EQ-ish insert on the track.
fn param(s: &Session, t: &str, uid: &str, key: &str) -> f64 {
    track(s, t)
        .inserts
        .iter()
        .find(|x| x.uid == uid)
        .unwrap_or_else(|| panic!("no {uid} on {t}"))
        .params[key]
}

fn preset_names(s: &mut Session, device: &str) -> Vec<String> {
    run(s, "preset.list", json!({ "device": device }))["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_saved_preset_is_an_xpreset_in_the_devices_folder_and_lists_under_it() {
    let mut r = rig("save");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "plugin.param.set",
        json!({ "id": eq, "param": "low_gain", "value": 0.8 }),
    );
    let saved = run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "暖かく" }),
    );
    let file = r.dir.join("fx").join("eq").join("暖かく.xpreset");
    assert!(file.is_file(), "{saved:?}");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["format"], "xirius-preset");
    assert_eq!(v["version"], 1);
    assert_eq!(v["kind"], "fx");
    assert_eq!(v["name"], "暖かく");
    assert_eq!(v["devices"][0]["uid"], "eq");
    assert_eq!(v["devices"][0]["params"]["low_gain"], 0.8);

    let names = preset_names(&mut r.s, "eq");
    assert_eq!(names[0], "Default", "{names:?}");
    let mut rest = names[1..].to_vec();
    let sorted = rest.clone();
    rest.sort();
    assert_eq!(rest, sorted, "the rest are by name: {names:?}");
    assert!(names.contains(&"暖かく".to_string()), "{names:?}");
}

#[test]
fn loading_a_preset_is_one_undo() {
    let mut r = rig("undo");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "A" }),
    );
    run(
        &mut r.s,
        "plugin.param.set",
        json!({ "id": eq, "param": "low_gain", "value": 0.9 }),
    );
    let before = r.s.project().clone();
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "A", "of": eq }),
    );
    let x = &track(&r.s, &t).inserts[0];
    assert_eq!(x.params["low_gain"], 0.5);
    assert_eq!(x.preset.as_deref(), Some("A"));
    run(&mut r.s, "history.undo", json!({}));
    assert_eq!(*r.s.project(), before, "one undo takes the load back");
    assert_eq!(param(&r.s, &t, "eq", "low_gain"), 0.9);
    run(&mut r.s, "history.redo", json!({}));
    assert_eq!(param(&r.s, &t, "eq", "low_gain"), 0.5);
}

#[test]
fn the_first_run_copies_the_factory_presets_in_once() {
    let mut r = rig("seed");
    // Listing writes nothing; the app's start (or the first save or load) copies them.
    assert_eq!(preset_names(&mut r.s, "eq"), ["Default"]);
    xirius_session::xpreset::seed(&r.dir).unwrap();
    for uid in ["eq", "compressor", "delay", "reverb", "limiter"] {
        for name in ["やさしく", "強め", "声を前に"] {
            assert!(
                r.dir
                    .join("fx")
                    .join(uid)
                    .join(format!("{name}.xpreset"))
                    .is_file(),
                "{uid}/{name}"
            );
        }
    }
    // The Sampler and the Drum Kit have only their Default (PLG-064).
    assert!(!r.dir.join("instrument").exists());
    assert_eq!(preset_names(&mut r.s, "eq").len(), 4);
    // A factory file the user deleted does not come back on the next start or command.
    let gone = r.dir.join("fx").join("eq").join("やさしく.xpreset");
    std::fs::remove_file(&gone).unwrap();
    xirius_session::xpreset::seed(&r.dir).unwrap();
    let (t, of) = eq_track(&mut r);
    save(&mut r, &t, &of, "mine");
    assert!(!gone.exists(), "only factory.restore brings it back");
}

#[test]
fn factory_files_are_plain_files_and_restore_adds_only_what_is_missing() {
    let mut r = rig("factory");
    xirius_session::xpreset::seed(&r.dir).unwrap();
    let mild = r.dir.join("fx").join("eq").join("やさしく.xpreset");
    // The user's edit of a factory file is kept: restore never overwrites.
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&mild).unwrap()).unwrap();
    v["devices"][0]["params"]["low_gain"] = json!(0.99);
    std::fs::write(&mild, v.to_string()).unwrap();
    let strong = r.dir.join("fx").join("eq").join("強め.xpreset");
    std::fs::remove_file(&strong).unwrap();
    let out = run(
        &mut r.s,
        "preset.factory.restore",
        json!({ "device": "eq" }),
    );
    assert_eq!(out["added"].as_array().unwrap().len(), 1, "{out:?}");
    assert!(strong.is_file());
    let again: Value = serde_json::from_str(&std::fs::read_to_string(&mild).unwrap()).unwrap();
    assert_eq!(
        again["devices"][0]["params"]["low_gain"], 0.99,
        "an existing file is never overwritten"
    );
    // Renaming and deleting them is the same as any saved preset.
    run(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "声を前に", "name": "前へ", "device": "eq" }),
    );
    assert!(r.dir.join("fx").join("eq").join("前へ.xpreset").is_file());
    run(
        &mut r.s,
        "preset.remove",
        json!({ "preset": "前へ", "device": "eq" }),
    );
    assert!(!r.dir.join("fx").join("eq").join("前へ.xpreset").exists());
}

#[test]
fn a_name_shared_by_every_device_loads_for_the_device_on_the_track() {
    let mut r = rig("shared");
    let t = audio(&mut r.s, "Vocal");
    let _eq = plugin(&mut r.s, &t, "EQ");
    xirius_session::xpreset::seed(&r.dir).unwrap();
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "強め" }),
    );
    let x = &track(&r.s, &t).inserts[0];
    assert_eq!(x.uid, "eq");
    assert_eq!(x.preset.as_deref(), Some("強め"));
    // And without such a device on the track the name is honestly ambiguous.
    let bare = audio(&mut r.s, "Plain");
    let m = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": bare, "preset": "強め" }),
    );
    assert!(m.contains("ほかの機器"), "{m}");
}

#[test]
fn a_preset_for_a_device_not_on_the_track_inserts_it() {
    let mut r = rig("insert");
    let t = audio(&mut r.s, "A");
    let eq = plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Vocal EQ" }),
    );
    let b = audio(&mut r.s, "B");
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": b, "preset": "Vocal EQ" }),
    );
    let x = &track(&r.s, &b).inserts[0];
    assert_eq!(x.uid, "eq");
    assert_eq!(x.preset.as_deref(), Some("Vocal EQ"));
}

#[test]
fn an_instrument_preset_takes_the_instruments_seat() {
    let mut r = rig("inst");
    let t = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let sampler = plugin(&mut r.s, &t, "Sampler");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "instrument", "name": "Piano A" }),
    );
    let other = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "Drums" }),
    ));
    plugin(&mut r.s, &other, "Drum Kit");
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": other, "preset": "Piano A" }),
    );
    let t = track(&r.s, &other);
    let inst: Vec<_> = t
        .inserts
        .iter()
        .filter(|x| x.kind == xirius_core::model::PluginKind::Instrument)
        .collect();
    assert_eq!(inst.len(), 1);
    assert_eq!(inst[0].uid, "sampler");
    assert_eq!(inst[0].preset.as_deref(), Some("Piano A"));
    // And the instrument's seat is first.
    assert_eq!(t.inserts[0].uid, "sampler");
    let _ = sampler;
}

#[test]
fn default_is_the_devices_own_defaults_not_a_file() {
    let mut r = rig("default");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "plugin.param.set",
        json!({ "id": eq, "param": "low_gain", "value": 0.9 }),
    );
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "Default", "of": eq }),
    );
    let x = &track(&r.s, &t).inserts[0];
    let def = xirius_params::EQ
        .defaults()
        .find(|(k, _)| *k == "low_gain")
        .unwrap()
        .1;
    assert_eq!(x.params["low_gain"], def);
    assert_eq!(x.preset.as_deref(), Some("Default"));
    // It cannot be saved over, renamed away or removed.
    for (n, a) in [
        ("preset.remove", json!({ "preset": "Default" })),
        ("preset.rename", json!({ "preset": "Default", "name": "x" })),
    ] {
        let m = fails(&mut r.s, n, a);
        assert!(!m.is_empty());
    }
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Default" }),
    );
    assert!(m.contains("使えません"), "{m}");
}

#[test]
fn an_old_plugin_preset_file_loads_the_same_way() {
    let mut r = rig("legacy");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    let file = r.dir.join("warm.json");
    std::fs::create_dir_all(&r.dir).unwrap();
    std::fs::write(
        &file,
        json!({
            "version": 1, "format": "builtin", "uid": "eq", "kind": "effect",
            "params": { "low_gain": 0.7 },
        })
        .to_string(),
    )
    .unwrap();
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": file.to_string_lossy(), "of": eq }),
    );
    assert_eq!(param(&r.s, &t, "eq", "low_gain"), 0.7);
}

#[test]
fn a_sloppy_file_is_read_the_way_a_song_is() {
    let mut r = rig("prj007");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    std::fs::create_dir_all(&r.dir).unwrap();
    let file = r.dir.join("odd.xpreset");
    std::fs::write(
        &file,
        json!({
            "format": "xirius-preset", "version": 9, "kind": "fx", "name": "Odd",
            "tomorrow": true,
            "devices": [{
                "name": "EQ", "kind": "effect", "format": "builtin", "uid": "eq",
                "params": { "low_gain": 4.2, "mystery": "loud" },
            }],
        })
        .to_string(),
    )
    .unwrap();
    let out = run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": file.to_string_lossy(), "of": eq }),
    );
    let rows = out["adjusted"].as_array().unwrap();
    let kinds: Vec<&str> = rows.iter().map(|x| x["kind"].as_str().unwrap()).collect();
    for k in ["dropped", "rounded"] {
        assert!(kinds.contains(&k), "{k} in {rows:?}");
    }
    assert_eq!(param(&r.s, &t, "eq", "low_gain"), 1.0);
}

#[test]
fn a_file_that_is_not_a_preset_is_not_read() {
    let mut r = rig("notmine");
    let t = audio(&mut r.s, "Vocal");
    let _eq = plugin(&mut r.s, &t, "EQ");
    std::fs::create_dir_all(&r.dir).unwrap();
    // Studio Pro's `.preset` is XML, a VST preset is bytes: neither is JSON to read.
    let file = r.dir.join("studio.preset");
    std::fs::write(&file, b"<?xml version=\"1.0\"?><Preset name=\"x\"/>").unwrap();
    let m = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": file.to_string_lossy() }),
    );
    assert!(m.contains("読み込めません"), "{m}");
    let json = r.dir.join("thing.json");
    std::fs::write(&json, json!({ "song": "not a preset" }).to_string()).unwrap();
    let m = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": json.to_string_lossy() }),
    );
    assert!(!m.is_empty());
}

#[test]
fn rename_and_remove_are_safe_and_say_no() {
    let mut r = rig("files");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    for n in ["One", "Two"] {
        run(
            &mut r.s,
            "preset.save",
            json!({ "track": t, "kind": "fx", "of": eq, "name": n }),
        );
    }
    let m = fails(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "One", "name": "Two", "device": "eq" }),
    );
    assert!(m.contains("もうあります"), "{m}");
    run(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "One", "name": "Three", "device": "eq" }),
    );
    assert!(!r.dir.join("fx").join("eq").join("One.xpreset").exists());
    let file = r.dir.join("fx").join("eq").join("Three.xpreset");
    assert!(file.is_file());
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(v["name"], "Three", "the name inside moved too");
    // A dry run of a remove writes nothing away.
    let before = std::fs::read_to_string(&file).unwrap();
    r.s.run_value(
        "preset.remove",
        json!({ "preset": "Three", "device": "eq" }),
        &RunOptions {
            dry_run: true,
            ..RunOptions::human()
        },
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    run(
        &mut r.s,
        "preset.remove",
        json!({ "preset": "Three", "device": "eq" }),
    );
    assert!(!file.exists());
    let m = fails(
        &mut r.s,
        "preset.remove",
        json!({ "preset": "Three", "device": "eq" }),
    );
    assert!(m.contains("見つかりません"), "{m}");
}

#[test]
fn what_cannot_be_saved_says_so() {
    let mut r = rig("saverr");
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "name": "X" }),
    );
    assert!(m.contains("--of"), "{m}");
    let keys = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    let sampler = plugin(&mut r.s, &keys, "Sampler");
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": keys, "kind": "fx", "of": sampler, "name": "X" }),
    );
    assert!(m.contains("エフェクト"), "{m}");
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "  " }),
    );
    assert!(m.contains("名前"), "{m}");
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "instrument", "name": "X" }),
    );
    assert!(m.contains("インストゥルメント"), "{m}");
}

#[test]
fn the_commands_carry_the_risk_the_plan_asks_for() {
    let reg = shared_registry();
    let risk = |n: &str| reg.get(n).unwrap_or_else(|| panic!("{n}")).risk;
    assert_eq!(risk("preset.save"), Risk::Guarded);
    assert_eq!(risk("preset.remove"), Risk::Guarded);
    assert_eq!(risk("preset.rename"), Risk::Guarded);
    assert_eq!(risk("preset.factory.restore"), Risk::Guarded);
    assert_eq!(risk("preset.load"), Risk::Write);
    assert_eq!(risk("preset.list"), Risk::Read);
    assert!(reg.get("preset.put").is_some());
}

// ---- what the review found (X-*, Y-*, Z-*) ------------------------------------------

/// An audio track with an EQ on it.
fn eq_track(r: &mut Rig) -> (String, String) {
    let t = audio(&mut r.s, "Vocal");
    let eq = plugin(&mut r.s, &t, "EQ");
    (t, eq)
}

fn save(r: &mut Rig, t: &str, of: &str, name: &str) -> Value {
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": of, "name": name }),
    )
}

/// The `.xpreset` file names in a device folder, as the disk spells them.
fn files(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xpreset"))
        .collect();
    v.sort();
    v
}

/// The name inside a preset file.
fn inner_name(p: &Path) -> String {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["name"].as_str().unwrap().to_string()
}

fn eq_dir(r: &Rig) -> PathBuf {
    r.dir.join("fx").join("eq")
}

/// The files of the folder spelled like `name` in any letter case.
fn spelled(dir: &Path, name: &str) -> Vec<String> {
    files(dir)
        .into_iter()
        .filter(|n| n.to_lowercase() == name.to_lowercase())
        .collect()
}

fn loads(r: &mut Rig, t: &str, of: &str, name: &str) {
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": name, "of": of }),
    );
    let x = track(&r.s, t);
    let x = x.inserts.iter().find(|x| x.id.to_string() == of).unwrap();
    assert_eq!(x.preset.as_deref(), Some(name));
}

#[test]
fn x1_a_rename_that_only_changes_the_letter_case_keeps_the_preset() {
    let mut r = rig("x1");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Warm");
    run(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "Warm", "name": "warm", "device": "eq" }),
    );
    assert_eq!(spelled(&eq_dir(&r), "warm.xpreset"), ["warm.xpreset"]);
    assert_eq!(inner_name(&eq_dir(&r).join("warm.xpreset")), "warm");
    loads(&mut r, &t, &eq, "warm");
    // The same through the file's path.
    let path = eq_dir(&r).join("warm.xpreset");
    run(
        &mut r.s,
        "preset.rename",
        json!({ "preset": path.to_string_lossy(), "name": "WARM" }),
    );
    assert_eq!(spelled(&eq_dir(&r), "warm.xpreset"), ["WARM.xpreset"]);
    assert_eq!(inner_name(&eq_dir(&r).join("WARM.xpreset")), "WARM");
}

#[test]
fn x2_names_that_spell_the_same_file_name_do_not_overwrite_each_other() {
    let mut r = rig("x2");
    let (t, eq) = eq_track(&mut r);
    // 1. Characters Windows does not allow in a file name.
    save(&mut r, &t, &eq, "a:b");
    save(&mut r, &t, &eq, "a?b");
    // 3. Two long names sharing their first 100 characters.
    let long_a = format!("{}A", "あ".repeat(100));
    let long_b = format!("{}B", "あ".repeat(100));
    save(&mut r, &t, &eq, &long_a);
    save(&mut r, &t, &eq, &long_b);
    let names = preset_names(&mut r.s, "eq");
    for n in ["a:b", "a?b", long_a.as_str(), long_b.as_str()] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
        loads(&mut r, &t, &eq, n);
    }
    // 4. A rename to the name another file's name is made of leaves that one alone.
    save(&mut r, &t, &eq, "One");
    run(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "One", "name": "a_b", "device": "eq" }),
    );
    let names = preset_names(&mut r.s, "eq");
    for n in ["a:b", "a_b"] {
        assert!(names.iter().any(|x| x == n), "{n} in {names:?}");
        loads(&mut r, &t, &eq, n);
    }
}

#[test]
fn x2_a_name_that_differs_only_in_case_is_not_overwritten_unless_asked() {
    let mut r = rig("x2case");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Warm");
    run(
        &mut r.s,
        "plugin.param.set",
        json!({ "id": eq, "param": "low_gain", "value": 0.8 }),
    );
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "WARM" }),
    );
    assert!(
        m.contains("もうあります") && m.contains("--overwrite"),
        "{m}"
    );
    assert_eq!(inner_name(&eq_dir(&r).join("Warm.xpreset")), "Warm");
    // Asked for, it overwrites that preset's own file: one file, the new name and values.
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "WARM", "overwrite": true }),
    );
    assert_eq!(spelled(&eq_dir(&r), "warm.xpreset"), ["WARM.xpreset"]);
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(eq_dir(&r).join("WARM.xpreset")).unwrap())
            .unwrap();
    assert_eq!(v["name"], "WARM");
    assert_eq!(v["devices"][0]["params"]["low_gain"], 0.8);
}

#[test]
fn x2_a_file_of_another_preset_at_the_place_is_never_written_over() {
    let mut r = rig("x2other");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine");
    save(&mut r, &t, &eq, "One");
    // The user renamed the file in the explorer; the name inside is still "Mine".
    std::fs::rename(
        eq_dir(&r).join("Mine.xpreset"),
        eq_dir(&r).join("Target.xpreset"),
    )
    .unwrap();
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Target" }),
    );
    assert!(m.contains("Target.xpreset"), "{m}");
    let m = fails(
        &mut r.s,
        "preset.rename",
        json!({ "preset": "One", "name": "Target", "device": "eq" }),
    );
    assert!(m.contains("Target.xpreset"), "{m}");
    assert_eq!(inner_name(&eq_dir(&r).join("Target.xpreset")), "Mine");
    assert_eq!(inner_name(&eq_dir(&r).join("One.xpreset")), "One");
    // 5. By path, onto the file of another preset.
    save(&mut r, &t, &eq, "B");
    let one = eq_dir(&r).join("One.xpreset");
    let m = fails(
        &mut r.s,
        "preset.rename",
        json!({ "preset": one.to_string_lossy(), "name": "B" }),
    );
    assert!(m.contains("もうあります"), "{m}");
    assert_eq!(inner_name(&eq_dir(&r).join("B.xpreset")), "B");
    assert_eq!(inner_name(&one), "One");
}

#[test]
fn x3_restore_does_not_write_over_a_file_the_list_does_not_show_as_that_preset() {
    let mut r = rig("x3");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine"); // the first copy is made by now
    let strong = eq_dir(&r).join("強め.xpreset");
    assert!(strong.is_file());
    std::fs::remove_file(&strong).unwrap();
    std::fs::rename(eq_dir(&r).join("Mine.xpreset"), &strong).unwrap();
    let out = run(
        &mut r.s,
        "preset.factory.restore",
        json!({ "device": "eq" }),
    );
    assert_eq!(inner_name(&strong), "Mine", "{out:?}");
    assert!(
        out["skipped"].to_string().contains("強め.xpreset"),
        "says what it left: {out:?}"
    );
    // A file that cannot be read is left alone too.
    std::fs::write(&strong, b"{ not json").unwrap();
    run(
        &mut r.s,
        "preset.factory.restore",
        json!({ "device": "eq" }),
    );
    assert_eq!(std::fs::read(&strong).unwrap(), b"{ not json");
}

#[test]
fn x3_the_first_copy_does_not_write_over_a_file_that_cannot_be_read() {
    let mut r = rig("x3seed");
    std::fs::create_dir_all(eq_dir(&r)).unwrap();
    let strong = eq_dir(&r).join("強め.xpreset");
    std::fs::write(&strong, b"{ half written").unwrap();
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine"); // makes the first copy
    assert!(eq_dir(&r).join("やさしく.xpreset").is_file());
    assert_eq!(std::fs::read(&strong).unwrap(), b"{ half written");
}

#[test]
fn y1_two_files_of_one_name_on_one_device_say_to_use_the_path() {
    let mut r = rig("y1");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Warm");
    let warm = eq_dir(&r).join("Warm.xpreset");
    let copy = eq_dir(&r).join("Warm - コピー.xpreset");
    std::fs::copy(&warm, &copy).unwrap();
    for (n, a) in [
        (
            "preset.load",
            json!({ "track": t, "preset": "Warm", "of": eq }),
        ),
        ("preset.remove", json!({ "preset": "Warm", "device": "eq" })),
    ] {
        let m = fails(&mut r.s, n, a);
        assert!(!m.contains("ほかの機器"), "{n}: {m}");
        assert!(m.contains("パス") && m.contains("コピー"), "{n}: {m}");
    }
    // The path picks one.
    run(
        &mut r.s,
        "preset.remove",
        json!({ "preset": copy.to_string_lossy() }),
    );
    assert!(!copy.exists() && warm.is_file());
    // A save of a name already there under another file name goes to that file.
    std::fs::rename(&warm, eq_dir(&r).join("old-warm.xpreset")).unwrap();
    let m = fails(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Warm" }),
    );
    assert!(m.contains("もうあります"), "{m}");
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": eq, "name": "Warm", "overwrite": true }),
    );
    assert!(!warm.exists(), "no second Warm");
    assert_eq!(inner_name(&eq_dir(&r).join("old-warm.xpreset")), "Warm");
}

#[test]
fn n1_a_save_onto_a_name_two_files_hold_says_a_way_out_that_works() {
    let mut r = rig("n1");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Warm");
    let warm = eq_dir(&r).join("Warm.xpreset");
    let copy = eq_dir(&r).join("Warm - コピー.xpreset");
    std::fs::copy(&warm, &copy).unwrap();
    let args = json!({ "track": t, "kind": "fx", "of": eq, "name": "Warm", "overwrite": true });
    let m = fails(&mut r.s, "preset.save", args.clone());
    assert!(
        m.contains("コピー") && m.contains("preset.remove") && m.contains("preset.rename"),
        "{m}"
    );
    assert!(
        !m.contains("--preset にファイルのパスを指定してください"),
        "save has no --preset: {m}"
    );
    // Doing what it says gets the save through.
    run(
        &mut r.s,
        "preset.remove",
        json!({ "preset": copy.to_string_lossy() }),
    );
    run(&mut r.s, "preset.save", args);
    assert!(warm.is_file() && !copy.exists());
}

#[test]
fn y2_a_name_every_device_has_loads_onto_an_empty_track_with_device() {
    let mut r = rig("y2");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine"); // the first copy is made by now
    let bare = audio(&mut r.s, "Plain");
    let m = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": bare, "preset": "強め" }),
    );
    assert!(m.contains("--device"), "{m}");
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": bare, "preset": "強め", "device": "Compressor" }),
    );
    let x = &track(&r.s, &bare).inserts[0];
    assert_eq!(x.uid, "compressor");
    assert_eq!(x.preset.as_deref(), Some("強め"));
    // Default names its device the same way.
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": bare, "preset": "Default", "device": "compressor" }),
    );
    assert_eq!(
        track(&r.s, &bare).inserts[0].preset.as_deref(),
        Some("Default")
    );
}

fn instrument(id: PluginId, format: &str, uid: &str) -> Plugin {
    Plugin {
        id,
        name: uid.to_string(),
        kind: PluginKind::Instrument,
        format: format.into(),
        uid: uid.into(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        samples: Vec::new(),
        isolation: Default::default(),
        sidechain: None,
        preset: None,
    }
}

/// An instrument track whose multi has three layers: a CLAP synth, the Sampler and
/// another CLAP synth; before it, a Drum Kit and a Sampler track, each with a preset
/// saved ("Kit A", "Keys A"). Returns the rack track and the layers' plug-in ids.
fn multi_rig(r: &mut Rig) -> (String, Vec<PluginId>) {
    for (n, dev, preset) in [
        ("Drums", "Drum Kit", "Kit A"),
        ("Keys", "Sampler", "Keys A"),
    ] {
        let t = id(&run(&mut r.s, "track.add.instrument", json!({ "name": n })));
        plugin(&mut r.s, &t, dev);
        run(
            &mut r.s,
            "preset.save",
            json!({ "track": t, "kind": "instrument", "name": preset }),
        );
    }
    let t = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "Rack" }),
    ));
    let tid: TrackId = t.parse().unwrap();
    let mut p = r.s.project().clone();
    let ids: Vec<PluginId> = (0..3).map(|_| PluginId(p.alloc())).collect();
    let node = NodeId(p.alloc());
    let layer_ids: Vec<NodeId> = (0..3).map(|_| NodeId(p.alloc())).collect();
    let tr = Arc::make_mut(p.tracks.get_mut(&tid).unwrap());
    tr.inserts = vec![
        instrument(ids[0], "clap", "x.one"),
        instrument(ids[1], "builtin", "sampler"),
        instrument(ids[2], "clap", "x.three"),
    ];
    tr.rack = Some(Rack {
        source: Some(Source::Multi {
            id: node,
            layers: ids
                .iter()
                .zip(&layer_ids)
                .map(|(x, l)| Layer {
                    id: *l,
                    name: format!("l{x}"),
                    instrument: *x,
                    fx: Vec::new(),
                    keys: (0, 127),
                    velocity: (1, 127),
                    transpose: 0,
                    gain_db: 0.0,
                    pan: 0.0,
                    mute: false,
                })
                .collect(),
        }),
        fx: Vec::new(),
    });
    r.s = session_with(&r.dir, p);
    (t, ids)
}

/// The uid of each layer's instrument (`None` when the layer lost it).
fn layers(s: &Session, t: &str) -> Vec<Option<String>> {
    let tr = track(s, t);
    let Some(Source::Multi { layers, .. }) = tr.rack.as_ref().and_then(|r| r.source.as_ref())
    else {
        panic!("no multi");
    };
    layers
        .iter()
        .map(|l| {
            tr.inserts
                .iter()
                .find(|x| x.id == l.instrument)
                .map(|x| x.uid.clone())
        })
        .collect()
}

fn some(v: &[&str]) -> Vec<Option<String>> {
    v.iter().map(|s| Some(s.to_string())).collect()
}

#[test]
fn y3_an_instrument_preset_on_a_multi_replaces_one_layer_only() {
    let mut r = rig("y3");
    let (t, ids) = multi_rig(&mut r);
    // The Sampler's preset goes to the layer that holds a Sampler.
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "Keys A" }),
    );
    assert_eq!(layers(&r.s, &t), some(&["x.one", "sampler", "x.three"]));
    let tr = track(&r.s, &t);
    let keys = tr.inserts.iter().find(|x| x.id == ids[1]).unwrap();
    assert_eq!(keys.preset.as_deref(), Some("Keys A"));
    // A device no layer holds takes the first instrument's seat; the others stay.
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "Kit A" }),
    );
    assert_eq!(layers(&r.s, &t), some(&["drum-kit", "sampler", "x.three"]));
    let tr = track(&r.s, &t);
    assert_eq!(
        tr.inserts
            .iter()
            .filter(|x| x.kind == PluginKind::Instrument)
            .count(),
        3
    );
    assert_eq!(tr.inserts[0].uid, "drum-kit");
}

#[test]
fn y4_an_fx_preset_loads_onto_the_master() {
    let mut r = rig("y4");
    let t = audio(&mut r.s, "Mix");
    let lim = plugin(&mut r.s, &t, "Limiter");
    save(&mut r, &t, &lim, "仕上げ");
    let master = TrackId::MASTER.to_string();
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": master, "preset": "仕上げ" }),
    );
    let m = track(&r.s, &master);
    let x = m
        .inserts
        .iter()
        .find(|x| x.uid == "limiter")
        .expect("a limiter");
    assert_eq!(x.preset.as_deref(), Some("仕上げ"));
}

#[test]
fn y5_factory_presets_carry_no_samples_and_skip_the_sampler_and_the_drum_kit() {
    let mut r = rig("y5");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine"); // the first copy is made by now
    let strong = eq_dir(&r).join("強め.xpreset");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&strong).unwrap()).unwrap();
    assert!(v["devices"][0].get("samples").is_none(), "{v}");
    for uid in ["sampler", "drum-kit"] {
        assert!(
            files(&r.dir.join("instrument").join(uid)).is_empty(),
            "{uid}"
        );
    }
    let out = run(&mut r.s, "preset.factory.restore", json!({}));
    assert!(!out["added"].to_string().contains("Sampler"), "{out:?}");
    assert!(files(&r.dir.join("instrument").join("sampler")).is_empty());
}

#[test]
fn y6_the_mark_records_what_was_copied_with_its_hash() {
    let mut r = rig("y6");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine");
    let mark: Value =
        serde_json::from_str(&std::fs::read_to_string(r.dir.join(".factory-seeded")).unwrap())
            .unwrap();
    assert!(mark["version"].as_u64().is_some(), "{mark}");
    let recorded = mark["files"].as_array().unwrap();
    assert_eq!(recorded.len(), 15, "{mark}");
    for f in recorded {
        let p = r.dir.join(f["file"].as_str().unwrap());
        assert_eq!(
            f["sha256"].as_str().unwrap(),
            sha256_hex(&std::fs::read(&p).unwrap()),
            "{f}"
        );
    }
}

#[test]
fn y6_a_newer_factory_set_removes_only_untouched_old_files_and_adds_the_new() {
    let mut r = rig("y6up");
    let ed = eq_dir(&r);
    std::fs::create_dir_all(&ed).unwrap();
    let file = |name: &str, gain: f64| {
        json!({
            "format": "xirius-preset", "version": 1, "kind": "fx", "name": name,
            "devices": [{ "name": "EQ", "kind": "effect", "format": "builtin", "uid": "eq",
                          "params": { "low_gain": gain } }],
        })
        .to_string()
    };
    let old = file("古い", 0.3);
    std::fs::write(ed.join("古い.xpreset"), &old).unwrap();
    let touched = file("手を入れた", 0.3);
    std::fs::write(ed.join("手を入れた.xpreset"), file("手を入れた", 0.9)).unwrap();
    let rec = |name: &str, body: &str| {
        json!({ "kind": "fx", "device": "eq", "name": name,
                "file": format!("fx/eq/{name}.xpreset"), "sha256": sha256_hex(body.as_bytes()) })
    };
    // Version 0 copied 古い, 手を入れた and やさしく; the user deleted やさしく since.
    let mark = json!({
        "version": 0,
        "files": [rec("古い", &old), rec("手を入れた", &touched), rec("やさしく", "gone")],
    });
    std::fs::write(r.dir.join(".factory-seeded"), mark.to_string()).unwrap();
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Mine");
    assert!(
        !ed.join("古い.xpreset").exists(),
        "untouched and gone from the set"
    );
    assert!(
        ed.join("手を入れた.xpreset").is_file(),
        "the user's edit stays"
    );
    assert!(!ed.join("やさしく.xpreset").exists(), "the user deleted it");
    assert!(ed.join("強め.xpreset").is_file(), "new in the set");
    let mark: Value =
        serde_json::from_str(&std::fs::read_to_string(r.dir.join(".factory-seeded")).unwrap())
            .unwrap();
    assert!(mark["version"].as_u64().unwrap() >= 1, "{mark}");
}

#[test]
fn y7_listing_writes_nothing() {
    let mut r = rig("y7");
    let out = run(&mut r.s, "preset.list", json!({}));
    assert!(
        out["presets"].as_array().unwrap().len() >= 7,
        "a Default each"
    );
    assert!(files(&eq_dir(&r)).is_empty());
    assert!(!r.dir.join(".factory-seeded").exists());
}

#[test]
fn y8_a_file_that_cannot_be_read_is_reported_by_the_list() {
    let mut r = rig("y8");
    std::fs::create_dir_all(eq_dir(&r)).unwrap();
    let bad = eq_dir(&r).join("壊れた.xpreset");
    std::fs::write(&bad, b"\x00\x01binary").unwrap();
    let res =
        r.s.run_value(
            "preset.list",
            json!({ "device": "eq" }),
            &RunOptions::human(),
        )
        .unwrap();
    let skipped = res.data["skipped"].as_array().expect("skipped");
    assert_eq!(skipped.len(), 1, "{:?}", res.data);
    assert!(
        skipped[0]["file"]
            .as_str()
            .unwrap()
            .ends_with("壊れた.xpreset")
    );
    assert!(!skipped[0]["reason"].as_str().unwrap().is_empty());
    assert!(
        res.message.contains("読めないファイルが 1 個"),
        "{}",
        res.message
    );
    let other = run(&mut r.s, "preset.list", json!({ "device": "compressor" }));
    assert!(other["skipped"].as_array().is_none_or(|v| v.is_empty()));
    // It can be removed by its path.
    run(
        &mut r.s,
        "preset.remove",
        json!({ "preset": bad.to_string_lossy() }),
    );
    assert!(!bad.exists());
}

#[test]
fn y9_a_path_outside_the_presets_folder_is_not_removed_or_renamed() {
    let mut r = rig("y9");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "Here");
    let away = std::env::temp_dir().join(format!("xirius-xpresets-{}-y9away", std::process::id()));
    let _ = std::fs::remove_dir_all(&away);
    std::fs::create_dir_all(&away).unwrap();
    let x = away.join("x.xpreset");
    std::fs::copy(eq_dir(&r).join("Here.xpreset"), &x).unwrap();
    for (n, a) in [
        ("preset.remove", json!({ "preset": x.to_string_lossy() })),
        (
            "preset.rename",
            json!({ "preset": x.to_string_lossy(), "name": "y" }),
        ),
    ] {
        let m = fails(&mut r.s, n, a);
        assert!(m.contains("置き場の外"), "{n}: {m}");
    }
    assert!(x.is_file());
    let _ = std::fs::remove_dir_all(&away);
}

#[test]
fn z2_a_relative_path_is_refused() {
    let mut r = rig("z2");
    let (t, eq) = eq_track(&mut r);
    let m = fails(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "presets/x.xpreset", "of": eq }),
    );
    assert!(m.contains("絶対"), "{m}");
}

#[test]
fn y10_samples_are_kept_as_full_paths_and_a_missing_one_is_reported() {
    let mut r = rig("y10");
    let t = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    plugin(&mut r.s, &t, "Sampler");
    let tid: TrackId = t.parse().unwrap();
    let mut p = r.s.project().clone();
    Arc::make_mut(p.tracks.get_mut(&tid).unwrap()).inserts[0].samples =
        vec!["Media/Kick.wav".into()];
    let song = std::env::temp_dir().join(format!("xirius-xpresets-{}-y10song", std::process::id()));
    let _ = std::fs::remove_dir_all(&song);
    let mut s = session_with(&r.dir, p);
    s.save_as(&song).unwrap();
    std::fs::create_dir_all(song.join("Media")).unwrap();
    let kick = song.join("Media").join("Kick.wav");
    std::fs::write(&kick, b"RIFF").unwrap();
    run(
        &mut s,
        "preset.save",
        json!({ "track": t, "kind": "instrument", "name": "Kick" }),
    );
    let file = r
        .dir
        .join("instrument")
        .join("sampler")
        .join("Kick.xpreset");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let saved = v["devices"][0]["samples"][0].as_str().unwrap().to_string();
    assert!(Path::new(&saved).is_absolute(), "{saved}");
    assert!(Path::new(&saved).is_file(), "{saved}");
    // Another song (with no folder) reads the full path.
    let other = id(&run(
        &mut r.s,
        "track.add.instrument",
        json!({ "name": "K" }),
    ));
    plugin(&mut r.s, &other, "Sampler");
    let out = run(
        &mut r.s,
        "preset.load",
        json!({ "track": other, "preset": "Kick" }),
    );
    assert!(!out["adjusted"].to_string().contains("rejected"), "{out:?}");
    assert_eq!(track(&r.s, &other).inserts[0].samples, vec![saved.clone()]);
    // Gone from the disk: left out, and said so.
    std::fs::remove_file(&kick).unwrap();
    let out = run(
        &mut r.s,
        "preset.load",
        json!({ "track": other, "preset": "Kick" }),
    );
    let rows = out["adjusted"].as_array().unwrap();
    assert!(
        rows.iter()
            .any(|x| x["kind"] == "rejected" && x["what"].as_str().unwrap().contains("Kick.wav")),
        "{rows:?}"
    );
    assert!(track(&r.s, &other).inserts[0].samples.is_empty());
    drop(s);
    let _ = std::fs::remove_dir_all(&song);
}

#[test]
fn z1_the_load_message_does_not_start_with_a_full_stop() {
    let mut r = rig("z1");
    let (t, eq) = eq_track(&mut r);
    save(&mut r, &t, &eq, "A");
    let res =
        r.s.run_value(
            "preset.load",
            json!({ "track": t, "preset": "A", "of": eq }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(!res.message.starts_with('。'), "{}", res.message);
    assert!(res.message.contains("プリセット「A」"), "{}", res.message);
}

#[test]
fn z3_default_in_any_letter_case_is_not_a_name() {
    let mut r = rig("z3");
    let (t, eq) = eq_track(&mut r);
    for n in ["default", "DEFAULT"] {
        let m = fails(
            &mut r.s,
            "preset.save",
            json!({ "track": t, "kind": "fx", "of": eq, "name": n }),
        );
        assert!(m.contains("使えません"), "{n}: {m}");
    }
}

/// The Compressor's knobs from before the sidechain (PLG-071); the five after them are the
/// sidechain's, which no preset sets.
const COMP_OLD: [&str; 7] = [
    "threshold",
    "ratio",
    "attack",
    "release",
    "knee",
    "makeup",
    "mix",
];
const COMP_SIDECHAIN: [&str; 5] = ["gain", "filter", "low_cut", "high_cut", "listen"];

fn read_json(p: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The Compressor's sidechain knobs (PLG-071) are not a preset's: loading a factory preset,
/// `Default`, the user's own or a file someone wrote them into leaves them where they were
/// set, a saved preset is written without them, and a Compressor a preset inserts starts
/// with them at their defaults.
#[test]
fn a_compressor_preset_leaves_the_sidechain_knobs_where_they_are() {
    let mut r = rig("comp-sidechain");
    let t = audio(&mut r.s, "Bus");
    let comp = plugin(&mut r.s, &t, "Compressor");
    for (param, v) in COMP_SIDECHAIN.iter().zip(["6", "on", "200", "5000", "on"]) {
        run(
            &mut r.s,
            "plugin.param.set",
            json!({ "id": comp, "param": param, "plain": v }),
        );
    }
    run(
        &mut r.s,
        "preset.save",
        json!({ "track": t, "kind": "fx", "of": comp, "name": "mine" }),
    );
    let dir = r.dir.join("fx").join("compressor");
    let mine = read_json(&dir.join("mine.xpreset"));
    let held = mine["devices"][0]["params"].as_object().unwrap();
    let mut keys: Vec<&str> = held.keys().map(String::as_str).collect();
    keys.sort();
    let mut old = COMP_OLD.to_vec();
    old.sort();
    assert_eq!(keys, old, "{mine}");

    run(
        &mut r.s,
        "plugin.param.set",
        json!({ "id": comp, "param": "gain", "plain": "-3" }),
    );
    let knobs = |s: &Session| -> Vec<f64> {
        let x = &track(s, &t).inserts[0];
        COMP_SIDECHAIN.iter().map(|k| x.params[*k]).collect()
    };
    let set = knobs(&r.s);
    let mut written = mine.clone();
    written["name"] = json!("written");
    for k in COMP_SIDECHAIN {
        written["devices"][0]["params"][k] = json!(0.9);
    }
    std::fs::write(
        dir.join("written.xpreset"),
        serde_json::to_vec_pretty(&written).unwrap(),
    )
    .unwrap();
    let def = xirius_params::find("Compressor").unwrap();
    let strong = xirius_params::factory_presets(def)
        .into_iter()
        .find(|(n, _)| *n == "強め")
        .unwrap()
        .1;
    for name in ["強め", "Default", "mine", "written"] {
        run(
            &mut r.s,
            "preset.load",
            json!({ "track": t, "preset": name, "of": comp }),
        );
        assert_eq!(knobs(&r.s), set, "{name}");
        if name == "強め" {
            let at = param(&r.s, &t, "compressor", "threshold");
            assert!((at - strong[0]).abs() < 1e-9, "{at} {}", strong[0]);
        }
    }

    let bare = audio(&mut r.s, "Bare");
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": bare, "preset": "written", "device": "Compressor" }),
    );
    let x = &track(&r.s, &bare).inserts[0];
    for k in COMP_SIDECHAIN {
        assert!(!x.params.contains_key(k), "{k}: {:?}", x.params);
    }
}

/// The factory Compressor files hold the seven knobs it had before the sidechain (PLG-071),
/// at the values they had: each sweeping knob a share of its sweep off its default, every
/// other one the other way.
#[test]
fn the_factory_compressor_presets_hold_the_seven_knobs_they_did() {
    let r = rig("comp-factory");
    xirius_session::xpreset::seed(&r.dir).unwrap();
    let def = xirius_params::find("Compressor").unwrap();
    for (name, share) in xirius_params::FACTORY_PRESETS {
        let v = read_json(
            &r.dir
                .join("fx")
                .join("compressor")
                .join(format!("{name}.xpreset")),
        );
        let held = v["devices"][0]["params"].as_object().unwrap();
        assert_eq!(held.len(), COMP_OLD.len(), "{name}: {v}");
        for (i, k) in COMP_OLD.iter().enumerate() {
            let (at, p) = def.param(k).unwrap();
            assert_eq!(at, i, "{k} kept its place");
            let d = p.default_norm();
            let sign = if i % 2 == 1 { -1.0 } else { 1.0 };
            let want = if p.steps().is_some() {
                d
            } else {
                (d + share * sign).clamp(0.0, 1.0)
            };
            let at = held[*k].as_f64().unwrap();
            assert!((at - want).abs() < 1e-9, "{name}: {k} {at} {want}");
        }
    }
}

/// A song saved before the sidechain (PLG-071) holds the Compressor's seven knobs at 「強め」
/// and that name. It opens with them, and loading 「強め」 again moves none of its knobs.
#[test]
fn an_old_songs_compressor_opens_at_its_preset_and_reloads_it_unchanged() {
    let mut r = rig("comp-old");
    let t = audio(&mut r.s, "Bus");
    plugin(&mut r.s, &t, "Compressor");
    let def = xirius_params::find("Compressor").unwrap();
    let strong = xirius_params::factory_presets(def)
        .into_iter()
        .find(|(n, _)| *n == "強め")
        .unwrap()
        .1;
    let mut p = r.s.project().clone();
    let tid: TrackId = t.parse().unwrap();
    let x = &mut Arc::make_mut(p.tracks.get_mut(&tid).unwrap()).inserts[0];
    x.params = COMP_OLD
        .iter()
        .zip(&strong)
        .map(|(k, v)| (k.to_string(), *v))
        .collect();
    x.preset = Some("強め".into());
    let comp = x.id.to_string();
    let song = r.dir.with_extension("song");
    let _ = std::fs::remove_dir_all(&song);
    session_with(&r.dir, p).save_as(&song).unwrap();
    let opened = Session::open(&song).unwrap().project().clone();
    let _ = std::fs::remove_dir_all(&song);
    r.s = session_with(&r.dir, opened);
    let before = track(&r.s, &t).inserts[0].clone();
    assert_eq!(before.preset.as_deref(), Some("強め"));
    assert_eq!(before.params.len(), COMP_OLD.len(), "{:?}", before.params);
    run(
        &mut r.s,
        "preset.load",
        json!({ "track": t, "preset": "強め", "of": comp }),
    );
    let after = &track(&r.s, &t).inserts[0];
    assert_eq!(after.params, before.params);
    assert_eq!(after.preset.as_deref(), Some("強め"));
}
