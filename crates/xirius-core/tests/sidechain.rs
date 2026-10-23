//! MIX-047: a plug-in's sidechain sources, the commands that manage them,
//! and the routing loops their edges may never make.
use serde_json::{Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{KeyMode, Project};
use xirius_core::{Session, session::RunOptions};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .data
}

fn run_err(s: &mut Session, name: &str, args: Value) -> String {
    s.run_value(name, args, &RunOptions::human())
        .err()
        .unwrap_or_else(|| panic!("{name} unexpectedly worked"))
        .message
}

fn track(v: &Value) -> TrackId {
    v.as_str().unwrap().parse().unwrap()
}

fn song() -> (Session, TrackId, TrackId, PluginId) {
    let mut s = Session::from_project(Project::new("t"));
    let kick = track(&run(&mut s, "track.add.audio", json!({"name": "Kick"}))["id"]);
    let bass = track(&run(&mut s, "track.add.instrument", json!({"name": "Bass"}))["id"]);
    let comp: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": bass.to_string(), "name": "Compressor"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    (s, kick, bass, comp)
}

fn sidechain(p: &Project, id: PluginId) -> Option<&xirius_core::model::Sidechain> {
    p.tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)?
        .sidechain
        .as_ref()
}

#[test]
fn add_set_enable_remove_and_undo() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string(), "pre": true, "level": -6.0}),
    );
    {
        let sc = sidechain(s.project(), comp).unwrap();
        assert!(sc.on);
        assert_eq!(sc.sources.len(), 1);
        let KeyMode::Send {
            pre,
            level_db,
            muted_level,
        } = sc.sources[0].mode
        else {
            panic!("a send");
        };
        assert!(pre && level_db == -6.0 && muted_level.is_none());
    }
    run(
        &mut s,
        "plugin.sidechain.set",
        json!({"id": comp.to_string(), "from": kick.to_string(), "position": "post", "level": -12.0}),
    );
    run(
        &mut s,
        "plugin.sidechain.enable",
        json!({"id": comp.to_string(), "state": "off"}),
    );
    {
        let sc = sidechain(s.project(), comp).unwrap();
        assert!(!sc.on);
        let KeyMode::Send { pre, level_db, .. } = sc.sources[0].mode else {
            panic!("a send");
        };
        assert!(!pre && level_db == -12.0);
    }
    let list = run(
        &mut s,
        "plugin.sidechain.list",
        json!({"id": comp.to_string()}),
    );
    assert_eq!(list["sources"].as_array().unwrap().len(), 1);
    // Every one of these is a single step of the history.
    s.undo(4).unwrap();
    assert!(sidechain(s.project(), comp).is_none());
    s.redo(4).unwrap();
    assert!(sidechain(s.project(), comp).is_some());
    // Removing the last source folds the sidechain away again.
    run(
        &mut s,
        "plugin.sidechain.remove",
        json!({"id": comp.to_string(), "from": kick.to_string()}),
    );
    assert!(sidechain(s.project(), comp).is_none());
}

#[test]
fn send_add_to_a_plugin_is_the_same_add() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "send.add",
        json!({"ids": [kick.to_string()], "to": comp.to_string(), "level": -3.0}),
    );
    let sc = sidechain(s.project(), comp).unwrap();
    assert_eq!(sc.sources[0].track, kick);
}

#[test]
fn a_loop_through_a_key_is_refused() {
    // Bass's output is the Music bus; Music's plug-in may not key from Bass
    // (Bass -> Music already flows the other way).
    let (mut s, kick, bass, comp) = song();
    let music = track(&run(&mut s, "track.add.bus", json!({"name": "Music"}))["id"]);
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [bass.to_string()], "to": music.to_string()}),
    );
    let mcomp: PluginId = run(
        &mut s,
        "plugin.add",
        json!({"track": music.to_string(), "name": "Comp"}),
    )["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // Music's plug-in keyed by Kick is fine.
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": mcomp.to_string(), "from": kick.to_string()}),
    );
    // Bass's plug-in keyed by Music loops: Bass -> Music -> Bass.
    let e = run_err(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": music.to_string()}),
    );
    assert!(e.contains("ループ"), "{e}");
    // The failed add left nothing behind.
    assert!(sidechain(s.project(), comp).is_none());
    // Bass's own plug-in keying from Bass is refused before the loop check.
    let e = run_err(
        &mut s,
        "send.add",
        json!({"ids": [bass.to_string()], "to": comp.to_string()}),
    );
    assert!(e.contains("同じチャンネル") || e.contains("ループ"), "{e}");
}

#[test]
fn the_output_mode_and_bad_sources() {
    let (mut s, kick, bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string(), "output": true}),
    );
    assert!(matches!(
        sidechain(s.project(), comp).unwrap().sources[0].mode,
        KeyMode::Output
    ));
    let e = run_err(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": bass.to_string(), "output": true, "pre": true}),
    );
    assert!(e.contains("同時に"), "{e}");
    // The receiver's own channel, the master and a missing channel are refused.
    for (from, want) in [
        (bass.to_string(), "同じチャンネル"),
        ("master".to_string(), "マスター"),
        ("trk_999".to_string(), "見つかりません"),
    ] {
        let e = run_err(
            &mut s,
            "plugin.sidechain.add",
            json!({"id": comp.to_string(), "from": from}),
        );
        assert!(e.contains(want), "{e} ~ {want}");
    }
    // A channel may be a source only once.
    let e = run_err(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string()}),
    );
    assert!(e.contains("すでに"), "{e}");
}

#[test]
fn the_source_rows_follow_what_happens_to_the_gear() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string()}),
    );
    // Duplicating the plug-in gives the copy rows of its own.
    let dup: PluginId = run(&mut s, "plugin.duplicate", json!({"id": comp.to_string()}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let a = sidechain(s.project(), comp).unwrap().sources[0].id;
    let b = sidechain(s.project(), dup).unwrap().sources[0].id;
    assert_ne!(a, b);
    // Deleting the source channel takes the rows away.
    run(&mut s, "track.delete", json!({"ids": [kick.to_string()]}));
    assert!(sidechain(s.project(), comp).is_none());
    // Two steps back the channel and the rows are whole again (the undo also
    // takes the duplicate's sidechain away with it).
    s.undo(2).unwrap();
    assert!(sidechain(s.project(), comp).is_some());
    // Removing the plug-in takes its rows with it.
    run(&mut s, "plugin.remove", json!({"ids": [comp.to_string()]}));
    assert!(sidechain(s.project(), comp).is_none());
    s.project().validate_deep().unwrap();
}

#[test]
fn a_song_without_a_sidechain_saves_and_reads_unchanged() {
    let (mut s, kick, bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string(), "level": -8.0}),
    );
    let v = serde_json::to_value(s.project()).unwrap();
    let back: Project = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(*s.project(), back);
    // A plug-in written before the field existed (no `sidechain` key) reads.
    let mut x = v["tracks"][bass.to_string().as_str()]["inserts"][0].clone();
    x.as_object_mut().unwrap().remove("sidechain");
    let plugin: xirius_core::model::Plugin = serde_json::from_value(x).unwrap();
    assert!(plugin.sidechain.is_none());
}

#[test]
fn plugin_paste_gives_the_rows_ids_of_their_own() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string()}),
    );
    // Copying the plug-in and pasting it elsewhere must not reuse the
    // source row's id (the paste used to keep it).
    run(&mut s, "edit.copy", json!({"plugin": comp.to_string()}));
    let pasted: PluginId = run(&mut s, "edit.paste", json!({"track": kick.to_string()}))["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // The source of the copy points at Kick but sits on Kick: it cannot be
    // kept, so the pasted plug-in's sidechain folds away.
    assert!(sidechain(s.project(), pasted).is_none());
    let orig = sidechain(s.project(), comp).unwrap().sources[0].id;
    run(&mut s, "edit.copy", json!({"plugin": comp.to_string()}));
    let other_track = s
        .project()
        .tracks
        .keys()
        .find(|t| **t != kick && !t.is_master())
        .unwrap()
        .to_string();
    let other: PluginId = run(&mut s, "edit.paste", json!({"track": other_track}))["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let new = sidechain(s.project(), other).unwrap().sources[0].id;
    assert_ne!(orig, new);
    s.project().validate().unwrap();
    s.project().validate_deep().unwrap();
}

#[test]
fn pasting_a_plugin_after_its_source_is_gone_drops_the_row() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string()}),
    );
    run(&mut s, "edit.copy", json!({"plugin": comp.to_string()}));
    run(&mut s, "track.delete", json!({"ids": [kick.to_string()]}));
    // The paste must not fail over the gone channel; the row is left out.
    let owner = comp_owner(&s);
    let pasted: PluginId = run(&mut s, "edit.paste", json!({"track": owner}))["ids"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(sidechain(s.project(), pasted).is_none());
    s.project().validate_deep().unwrap();
}

fn comp_owner(s: &Session) -> String {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == "Bass")
        .unwrap()
        .id
        .to_string()
}

#[test]
fn channel_settings_paste_drops_a_looping_source_instead_of_failing() {
    let (mut s, kick, bass, comp) = song();
    let drums = track(&run(&mut s, "track.add.bus", json!({"name": "Drums"}))["id"]);
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": drums.to_string()}),
    );
    // Kick's output is Drums; pasting Bass's settings onto Kick would make
    // the key edge Drums -> Kick loop.
    run(
        &mut s,
        "track.output.set",
        json!({"ids": [kick.to_string()], "to": drums.to_string()}),
    );
    let out = run(
        &mut s,
        "channel.settings.paste",
        json!({"ids": [kick.to_string()], "from": bass.to_string()}),
    );
    // The paste went through; the looping source was left out and counted.
    assert_eq!(out["adjusted"], json!(1));
    let new_comp = s.project().tracks[&kick]
        .inserts
        .iter()
        .find(|x| x.name == "Compressor")
        .unwrap();
    assert!(new_comp.sidechain.is_none());
    s.project().validate_deep().unwrap();
}

#[test]
fn send_add_to_a_plugin_checks_the_key_level_range() {
    let (mut s, kick, _bass, comp) = song();
    // -70 and +10 are inside the send range but outside the key range.
    for level in [-70.0, 10.0, -100.0] {
        let e = run_err(
            &mut s,
            "send.add",
            json!({"ids": [kick.to_string()], "to": comp.to_string(), "level": level}),
        );
        assert!(e.contains("範囲外"), "{e} for {level}");
    }
    assert!(sidechain(s.project(), comp).is_none());
    let e = run_err(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string(), "level": 7.0}),
    );
    assert!(e.contains("範囲外"), "{e}");
}

#[test]
fn a_saved_song_with_a_sidechain_opens_and_validates() {
    let (mut s, kick, _bass, comp) = song();
    run(
        &mut s,
        "plugin.sidechain.add",
        json!({"id": comp.to_string(), "from": kick.to_string(), "level": -8.0}),
    );
    let v = serde_json::to_value(s.project()).unwrap();
    let back: Project = serde_json::from_value(v).unwrap();
    back.validate().unwrap();
    back.validate_deep().unwrap();
    assert_eq!(*s.project(), back);
}

/// PLG-071: Swap keeps no value of its own. It trades the frequencies of the Low Cut and the
/// High Cut in one step, an Off for the other Off, as Studio Pro's button does.
#[test]
fn swap_trades_the_low_cut_and_the_high_cut_in_one_step() {
    let (mut s, _kick, _bass, comp) = song();
    let id = comp.to_string();
    let plain = |s: &Session, param: &str| {
        let x = s
            .project()
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == comp)
            .unwrap();
        let (_, spec) = x.builtin().unwrap().param(param).unwrap();
        let norm = x.params.get(param).copied();
        spec.to_plain(norm.unwrap_or_else(|| spec.default_norm()))
    };
    let set = |s: &mut Session, param: &str, v: &str| {
        run(
            s,
            "plugin.param.set",
            json!({"id": comp.to_string(), "param": param, "plain": v}),
        );
    };
    let at = |s: &Session, low: f64, high: f64| {
        let (l, h) = (plain(s, "low_cut"), plain(s, "high_cut"));
        assert!((l - low).abs() < low * 1e-6, "low {l}, not {low}");
        assert!((h - high).abs() < high * 1e-6, "high {h}, not {high}");
    };
    // Swap is a step, not a knob a song keeps.
    let def = xirius_params::COMPRESSOR.params;
    assert!(def.iter().all(|p| p.id != "swap"));
    set(&mut s, "low_cut", "200");
    set(&mut s, "high_cut", "5000");
    let steps = s.steps().len();
    run(&mut s, "plugin.sidechain.swap", json!({"id": id}));
    assert_eq!(s.steps().len(), steps + 1);
    at(&s, 5000.0, 200.0);
    s.undo(1).unwrap();
    at(&s, 200.0, 5000.0);
    // The Low Cut's Off (20 Hz) becomes the High Cut's Off (20 kHz), and back.
    set(&mut s, "low_cut", "Off");
    run(&mut s, "plugin.sidechain.swap", json!({"id": id}));
    at(&s, 5000.0, 20_000.0);
    run(&mut s, "plugin.sidechain.swap", json!({"id": id}));
    at(&s, 20.0, 5000.0);
    // A High Cut above the Low Cut's range lands on its top.
    set(&mut s, "high_cut", "18000");
    run(&mut s, "plugin.sidechain.swap", json!({"id": id}));
    at(&s, 16_000.0, 20_000.0);
    // Only a device with the two cuts has the swap.
    let bass = comp_owner(&s);
    let eq = run(&mut s, "plugin.add", json!({"track": bass, "name": "EQ"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let e = run_err(&mut s, "plugin.sidechain.swap", json!({"id": eq}));
    assert!(e.contains("Low Cut"), "{e}");
}
