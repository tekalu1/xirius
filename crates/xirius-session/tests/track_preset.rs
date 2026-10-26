//! The track presets kept in files (MIX-030): saved from a track, listed, put on tracks, made into
//! a track, and updated; a file from another version or edited by hand is read the way a song is
//! (PRJ-007), and the plugins this PC does not have stay as empty slots.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};
use xirius_core::{
    Session,
    id::TrackId,
    model::{Project, Track},
    session::RunOptions,
};
use xirius_plugins::{Catalog, Entry};
use xirius_session::{
    commands::{self, shared_registry},
    ext::ExtConfig,
    output::OutputMode,
};

/// `XIRIUS_TRACK_PRESETS` is one for the process; the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

struct Rig {
    _turn: MutexGuard<'static, ()>,
    dir: PathBuf,
    s: Session,
}

fn plugin_entry(uid: &str) -> Entry {
    Entry {
        uid: uid.into(),
        name: "Installed".into(),
        vendor: "Test".into(),
        kind: xirius_core::model::PluginKind::Effect,
        format: "vst3".into(),
        category: "Fx".into(),
        version: "1".into(),
        path: PathBuf::from("Installed.vst3"),
        ara: false,
        requires_process_main: false,
    }
}

fn session(catalog: Option<Catalog>) -> Session {
    let mut s = Session::with_registry(Project::new("presets"), shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: false },
            catalog: catalog.map(Arc::new),
            ..ExtConfig::default()
        },
    );
    s
}

fn rig(tag: &str) -> Rig {
    let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let dir =
        std::env::temp_dir().join(format!("xirius-track-presets-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    unsafe { std::env::set_var("XIRIUS_TRACK_PRESETS", &dir) };
    Rig {
        _turn: turn,
        dir,
        s: session(None),
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

fn effect(s: &mut Session, t: &str, name: &str) {
    run(
        s,
        "plugin.add",
        json!({ "track": t, "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    );
}

/// A track "Vocal" with a compressor and an EQ, -6 dB, pan 0.25 and a colour.
fn vocal(r: &mut Rig) -> String {
    let t = id(&run(
        &mut r.s,
        "track.add.audio",
        json!({ "name": "Vocal" }),
    ));
    effect(&mut r.s, &t, "Compressor");
    effect(&mut r.s, &t, "EQ");
    run(
        &mut r.s,
        "track.volume.set",
        json!({ "ids": [t], "db": -6.0 }),
    );
    run(
        &mut r.s,
        "track.pan.set",
        json!({ "ids": [t], "pan": 0.25 }),
    );
    run(
        &mut r.s,
        "track.color.set",
        json!({ "ids": [t], "color": "#ff8800" }),
    );
    t
}

#[test]
fn a_saved_preset_is_listed_and_put_on_a_track_in_one_undo() {
    let mut r = rig("apply");
    let v = vocal(&mut r);
    let saved = run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Vocal chain" }),
    );
    assert!(r.dir.join("Vocal chain.json").is_file());
    assert_eq!(saved["name"], "Vocal chain");
    assert_eq!(track(&r.s, &v).preset.as_deref(), Some("Vocal chain"));
    let list = run(&mut r.s, "track.preset.list", json!({}));
    let rows = list["presets"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "Vocal chain");
    assert_eq!(rows[0]["inserts"], 2);

    let a = id(&run(&mut r.s, "track.add.audio", json!({ "name": "Lead" })));
    let b = id(&run(&mut r.s, "track.add.audio", json!({ "name": "Back" })));
    let before = r.s.project().clone();
    let out = run(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [a, b], "preset": "Vocal chain" }),
    );
    assert_eq!(out["adjusted"], json!([]));
    for t in [&a, &b] {
        let t = track(&r.s, t);
        assert_eq!(
            t.inserts
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>(),
            ["Compressor", "EQ"]
        );
        assert_eq!(t.volume_db, -6.0);
        assert_eq!(t.pan, 0.25);
        assert_eq!(t.color.as_deref(), Some("#ff8800"));
        assert_eq!(t.preset.as_deref(), Some("Vocal chain"));
    }
    assert_eq!(track(&r.s, &a).name, "Lead");
    run(&mut r.s, "history.undo", json!({}));
    assert_eq!(*r.s.project(), before, "one undo takes it all back");
    run(&mut r.s, "history.redo", json!({}));
    assert_eq!(track(&r.s, &b).inserts.len(), 2);
}

#[test]
fn a_preset_makes_a_track_named_after_it() {
    let mut r = rig("add");
    let v = vocal(&mut r);
    run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Lead vocal" }),
    );
    let before = r.s.project().clone();
    let made = run(
        &mut r.s,
        "track.preset.add",
        json!({ "preset": "Lead vocal" }),
    );
    let t = track(&r.s, &id(&made));
    assert_eq!(t.name, "Lead vocal");
    assert_eq!(t.inserts.len(), 2);
    assert_eq!(t.volume_db, -6.0);
    assert_eq!(t.preset.as_deref(), Some("Lead vocal"));
    assert_eq!(made["adjusted"], json!([]));
    // A second one does not take the same name.
    let again = run(
        &mut r.s,
        "track.preset.add",
        json!({ "preset": "Lead vocal" }),
    );
    assert_ne!(track(&r.s, &id(&again)).name, "Lead vocal");
    run(&mut r.s, "history.undo", json!({}));
    run(&mut r.s, "history.undo", json!({}));
    assert_eq!(*r.s.project(), before);
}

#[test]
fn update_writes_the_track_back_over_the_preset_it_came_from() {
    let mut r = rig("update");
    let v = vocal(&mut r);
    run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Vox" }),
    );
    run(
        &mut r.s,
        "track.volume.set",
        json!({ "ids": [v], "db": -2.0 }),
    );
    run(&mut r.s, "track.preset.update", json!({ "track": v }));
    let other = id(&run(
        &mut r.s,
        "track.add.audio",
        json!({ "name": "Other" }),
    ));
    run(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [other], "preset": "Vox" }),
    );
    assert_eq!(track(&r.s, &other).volume_db, -2.0);
    // Still one preset, not a second.
    let list = run(&mut r.s, "track.preset.list", json!({}));
    assert_eq!(list["presets"].as_array().unwrap().len(), 1);
}

#[test]
fn presets_are_listed_in_the_order_they_were_saved_and_update_keeps_the_place() {
    let mut r = rig("order");
    let v = vocal(&mut r);
    for n in ["Zeta", "Alpha", "Mid"] {
        run(
            &mut r.s,
            "track.preset.save",
            json!({ "track": v, "name": n }),
        );
    }
    let names = |s: &mut Session| -> Vec<String> {
        run(s, "track.preset.list", json!({}))["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&mut r.s), ["Zeta", "Alpha", "Mid"]);
    // Updating the first one rewrites its file but leaves it first.
    run(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [v], "preset": "Zeta" }),
    );
    run(&mut r.s, "track.preset.update", json!({ "track": v }));
    assert_eq!(names(&mut r.s), ["Zeta", "Alpha", "Mid"]);
}

#[test]
fn a_preset_outlives_the_session_that_saved_it() {
    let mut r = rig("reopen");
    let v = vocal(&mut r);
    run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Kept" }),
    );
    let mut next = session(None);
    let t = id(&run(&mut next, "track.add.audio", json!({ "name": "New" })));
    run(
        &mut next,
        "track.preset.apply",
        json!({ "ids": [t], "preset": "Kept" }),
    );
    assert_eq!(track(&next, &t).inserts.len(), 2);
    assert_eq!(track(&next, &t).volume_db, -6.0);
}

#[test]
fn what_cannot_be_done_says_so() {
    let mut r = rig("errors");
    let v = vocal(&mut r);
    run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Once" }),
    );
    let m = fails(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "Once" }),
    );
    assert!(m.contains("もうあります"), "{m}");
    let m = fails(
        &mut r.s,
        "track.preset.save",
        json!({ "track": v, "name": "  " }),
    );
    assert!(m.contains("空"), "{m}");
    let plain = id(&run(
        &mut r.s,
        "track.add.audio",
        json!({ "name": "Plain" }),
    ));
    let m = fails(&mut r.s, "track.preset.update", json!({ "track": plain }));
    assert!(m.contains("プリセットから作った"), "{m}");
    let m = fails(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [plain], "preset": "Nothing" }),
    );
    assert!(m.contains("見つかりません"), "{m}");
    let m = fails(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": ["trk_0"], "preset": "Once" }),
    );
    assert!(!m.is_empty());
    assert_eq!(
        run(&mut r.s, "track.preset.list", json!({}))["presets"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "the failures wrote nothing"
    );
}

#[test]
fn a_name_the_file_system_refuses_still_saves() {
    let mut r = rig("names");
    let v = vocal(&mut r);
    for n in ["a/b:c", "CON", "end. "] {
        run(
            &mut r.s,
            "track.preset.save",
            json!({ "track": v, "name": n }),
        );
        let t = id(&run(&mut r.s, "track.add.audio", json!({})));
        run(
            &mut r.s,
            "track.preset.apply",
            json!({ "ids": [t], "preset": n.trim() }),
        );
        assert_eq!(track(&r.s, &t).inserts.len(), 2, "{n}");
    }
}

#[test]
fn a_file_from_another_version_is_read_and_every_change_is_reported() {
    let mut r = rig("compat");
    std::fs::create_dir_all(&r.dir).unwrap();
    std::fs::write(
        r.dir.join("future.json"),
        json!({
            "version": 9,
            "name": "Future",
            "kind": "audio",
            "volume_db": 99.0,
            "pan": -3.0,
            "color": "not a colour",
            "hologram": true,
            "inserts": [
                { "name": "EQ", "format": "builtin", "uid": "EQ", "kind": "effect", "future": 1 },
                { "name": "no uid" }
            ],
            "sends": [ { "to_name": "Nowhere", "level_db": -3.0 } ]
        })
        .to_string(),
    )
    .unwrap();
    let t = id(&run(&mut r.s, "track.add.audio", json!({ "name": "T" })));
    let out = run(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [t], "preset": "Future" }),
    );
    let rows = out["adjusted"].as_array().unwrap();
    let kinds: Vec<&str> = rows.iter().map(|x| x["kind"].as_str().unwrap()).collect();
    for k in ["dropped", "rounded", "rejected"] {
        assert!(kinds.contains(&k), "{k} in {rows:?}");
    }
    let t = track(&r.s, &t);
    assert_eq!(t.volume_db, 12.0);
    assert_eq!(t.pan, -1.0);
    assert_eq!(t.inserts.len(), 1);
    assert!(t.sends.is_empty());
    // The same file through the CLI/MCP path says the same: `adjusted` is in the data.
    assert!(out["adjusted"].is_array());
}

#[test]
fn a_plugin_this_pc_does_not_have_is_kept_as_an_empty_slot_and_reported() {
    let mut r = rig("missing");
    r.s = session(Some(Catalog {
        entries: vec![plugin_entry("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")],
        failures: vec![],
    }));
    std::fs::create_dir_all(&r.dir).unwrap();
    std::fs::write(
        r.dir.join("gone.json"),
        json!({
            "version": 1,
            "name": "Gone",
            "kind": "audio",
            "volume_db": 0.0,
            "pan": 0.0,
            "inserts": [
                { "name": "Installed", "format": "vst3", "uid": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", "kind": "effect", "bypass": false },
                { "name": "Keyscape", "format": "vst3", "uid": "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB", "kind": "effect", "bypass": false },
                { "name": "EQ", "format": "builtin", "uid": "EQ", "kind": "effect", "bypass": false }
            ],
            "sends": []
        })
        .to_string(),
    )
    .unwrap();
    let t = id(&run(&mut r.s, "track.add.audio", json!({ "name": "T" })));
    let out = run(
        &mut r.s,
        "track.preset.apply",
        json!({ "ids": [t], "preset": "Gone" }),
    );
    let rows = out["adjusted"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "rejected");
    assert_eq!(rows[0]["what"], "Keyscape");
    assert!(
        rows[0]["reason"]
            .as_str()
            .unwrap()
            .contains("この PC に無い")
    );
    let names: Vec<String> = track(&r.s, &t)
        .inserts
        .iter()
        .map(|x| x.name.clone())
        .collect();
    assert_eq!(names, ["Installed", "Keyscape", "EQ"], "the slot stays");
}

#[test]
fn a_dry_run_changes_and_writes_nothing() {
    let mut r = rig("dry");
    let v = vocal(&mut r);
    let before = r.s.project().clone();
    let out =
        r.s.run_value(
            "track.preset.save",
            json!({ "track": v, "name": "Never" }),
            &RunOptions {
                dry_run: true,
                ..RunOptions::human()
            },
        )
        .unwrap();
    assert!(out.message.contains("保存します"), "{}", out.message);
    assert!(!r.dir.join("Never.json").exists());
    assert_eq!(*r.s.project(), before);
}

#[test]
fn saving_a_track_with_branches_says_they_were_saved_in_a_line() {
    let mut r = rig("branches");
    let v = vocal(&mut r);
    let fx: Vec<String> = track(&r.s, &v)
        .inserts
        .iter()
        .map(|x| x.id.to_string())
        .collect();
    let split = run(&mut r.s, "rack.split.add", json!({ "track": v }));
    let b1 = split["branches"][1].as_str().unwrap().to_string();
    run(&mut r.s, "rack.node.move", json!({ "id": fx[1], "to": b1 }));
    let saved =
        r.s.run_value(
            "track.preset.save",
            json!({ "track": v, "name": "Split chain" }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(saved.message.contains("直列"), "{}", saved.message);
    assert_eq!(saved.data["flattened"], true);
    let other = vocal(&mut r);
    let plain = run(
        &mut r.s,
        "track.preset.save",
        json!({ "track": other, "name": "Plain chain" }),
    );
    assert_eq!(plain["flattened"], false);
}
