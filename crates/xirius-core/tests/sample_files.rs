//! ENG-077: the files a device reads (a Sampler's zones, a Drum Kit's pads) are the song's
//! files: saving gathers them as it does the clips', `clip.relink` points them elsewhere, and
//! a missing one is reported.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginIsolation, PluginKind, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn text(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// A project with one instrument track whose Sampler lists `samples`.
fn project_with_samples(samples: &[String]) -> Project {
    let mut p = Project::new("s");
    let mut t = Track::new(TrackId(1), TrackKind::Instrument, "Keys");
    t.inserts.push(Plugin {
        id: PluginId(2),
        name: "Sampler".into(),
        kind: PluginKind::Instrument,
        format: "builtin".into(),
        uid: "sampler".into(),
        bypass: false,
        deactivated: false,
        params: Default::default(),
        state: None,
        sidechain: None,
        preset: None,
        samples: samples.to_vec(),
        isolation: PluginIsolation::default(),
    });
    p.tracks.insert(TrackId(1), std::sync::Arc::new(t));
    p.root.push(xirius_core::id::Node::Track(TrackId(1)));
    p
}

fn samples(s: &Session) -> Vec<String> {
    s.project().tracks[&TrackId(1)].inserts[0].samples.clone()
}

#[test]
fn saving_gathers_a_devices_files_into_the_song_folder() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    let kick = outside.join("Kick.wav");
    fs::write(&kick, b"kick").unwrap();
    let mut s = Session::from_project(project_with_samples(&[text(&kick)]));
    let r = s
        .run_value(
            "project.save-as",
            json!({ "path": text(&song) }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(r.message.contains("1 個"), "{}", r.message);
    assert_eq!(
        fs::read(song.join("Media/Imports/Kick.wav")).unwrap(),
        b"kick"
    );
    assert_eq!(samples(&s), ["Media/Imports/Kick.wav"]);
}

#[test]
fn relink_points_a_devices_file_elsewhere() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let mut s = Session::from_project(project_with_samples(&["Media/Imports/Kick.wav".into()]));
    run(
        &mut s,
        "project.save-as",
        json!({ "path": text(&tmp.path().join("song")) }),
    );
    run(
        &mut s,
        "clip.relink",
        json!({ "files": [{ "from": "Media/Imports/Kick.wav", "to": "Media/Imports/Snare.wav" }] }),
    );
    assert_eq!(samples(&s), ["Media/Imports/Snare.wav"]);
}

#[test]
fn a_file_the_device_cannot_find_is_reported_and_stays() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let mut s = Session::from_project(project_with_samples(&["gone/kick.wav".into()]));
    let r = s
        .run_value(
            "project.save-as",
            json!({ "path": text(&tmp.path().join("song")) }),
            &RunOptions::human(),
        )
        .unwrap();
    assert!(r.message.contains("見つからない"), "{}", r.message);
    // The reference stays so it can be found later.
    assert_eq!(samples(&s), ["gone/kick.wav"]);
}

fn plugin(s: &Session) -> Plugin {
    s.project().tracks[&TrackId(1)].inserts[0].clone()
}

#[test]
fn a_samplers_files_come_and_go_with_their_values_in_one_step_each() {
    let mut s = Session::from_project(project_with_samples(&[]));
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "files": [{ "file": "a_C2.wav" }, { "file": "b_C3.wav" }, { "file": "c_C4.wav" }] }),
    );
    assert_eq!(samples(&s), ["a_C2.wav", "b_C3.wav", "c_C4.wav"]);
    // A zone's value is set by its key, and only for a file the device has.
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "FILE3.start", "plain": "25" }),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "file2.reverse", "plain": "on",
                "also": [{ "param": "file3.end", "value": 0.5 }] }),
    );
    let e = s
        .run_value(
            "plugin.param.set",
            json!({ "id": "plg_2", "param": "file4.start", "value": 0.5 }),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("file4.start"), "{}", e.message);
    let rows = run(&mut s, "plugin.param.list", json!({ "id": "plg_2" }));
    let start = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "file3.start")
        .expect("the zone's value is listed");
    assert_eq!(start["text"], "25 %");

    // Taking the first file away moves the others' values down with them.
    run(
        &mut s,
        "plugin.samples.remove",
        json!({ "id": "plg_2", "index": 1 }),
    );
    let x = plugin(&s);
    assert_eq!(x.samples, ["b_C3.wav", "c_C4.wav"]);
    assert_eq!(x.params.get("file1.reverse"), Some(&1.0));
    assert!((x.params["file2.start"] - 0.25).abs() < 1e-9);
    assert_eq!(x.params.get("file2.end"), Some(&0.5));
    assert!(!x.params.contains_key("file3.start"));
    run(
        &mut s,
        "plugin.samples.set",
        json!({ "id": "plg_2", "index": 2, "file": "d_C4.wav" }),
    );
    assert_eq!(samples(&s), ["b_C3.wav", "d_C4.wav"]);
    assert!((plugin(&s).params["file2.start"] - 0.25).abs() < 1e-9);

    // Each is one step of the history.
    run(&mut s, "history.undo", json!({}));
    assert_eq!(samples(&s), ["b_C3.wav", "c_C4.wav"]);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(samples(&s), ["a_C2.wav", "b_C3.wav", "c_C4.wav"]);
    assert!(plugin(&s).params.contains_key("file3.start"));
    run(&mut s, "history.redo", json!({}));
    run(
        &mut s,
        "plugin.samples.remove",
        json!({ "id": "plg_2", "all": true }),
    );
    let x = plugin(&s);
    assert!(x.samples.is_empty());
    assert!(
        x.params.keys().all(|k| !k.starts_with("file")),
        "{:?}",
        x.params
    );
}

#[test]
fn a_kits_files_fill_free_pads_by_name_and_keep_their_pads() {
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        let x = &mut t.inserts[0];
        x.name = "Drum Kit".into();
        x.uid = "drum-kit".into();
    }
    let mut s = Session::from_project(p);
    let r = run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "files": [
            { "file": "kick_v2.wav" }, { "file": "kick_v1.wav" }, { "file": "snare.wav" },
            { "file": "hat_rr1.wav" }, { "file": "hat_rr2.wav" } ] }),
    );
    assert_eq!(r["pads"], json!(["A1", "A2", "A3"]));
    let x = plugin(&s);
    assert_eq!(
        x.samples,
        [
            "kick_v1.wav",
            "kick_v2.wav",
            "snare.wav",
            "hat_rr1.wav",
            "hat_rr2.wav"
        ]
    );
    let def = xirius_params::find("drum-kit").unwrap();
    let pads: Vec<usize> = (0..x.samples.len())
        .map(|i| def.file_pad(&x.params, i))
        .collect();
    assert_eq!(pads, [0, 0, 1, 2, 2]);
    assert_eq!(x.params.get("A3.layer_mode"), Some(&(1.0 / 3.0)));
    // A pad's value goes by its name.
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "a2.choke", "plain": "Self" }),
    );
    assert!(plugin(&s).params.contains_key("A2.choke"));

    // The next ones go on the pads from the one asked for.
    let r = run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "A4",
                "files": [{ "file": "tom1.wav" }, { "file": "tom2.wav" }] }),
    );
    assert_eq!(r["pads"], json!(["A4", "A5"]));
    // --layers with a pad: on it, though it has files; how it picks among them stays.
    let r = run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "A3", "layers": true,
                "files": [{ "file": "hat_rr3.wav" }] }),
    );
    assert_eq!(r["pads"], json!(["A3"]));
    let x = plugin(&s);
    assert_eq!(x.params.get("A3.layer_mode"), Some(&(1.0 / 3.0)));
    // Taking a pad's files away leaves the others on their pads.
    run(
        &mut s,
        "plugin.samples.remove",
        json!({ "id": "plg_2", "pad": "A1" }),
    );
    let x = plugin(&s);
    let mut pads: Vec<usize> = (0..x.samples.len())
        .map(|i| def.file_pad(&x.params, i))
        .collect();
    pads.sort_unstable();
    assert_eq!(pads, [1, 2, 2, 2, 3, 4]);
    // A bank's 16 pads at once; the others stay.
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "B1", "files": [{ "file": "crash.wav" }] }),
    );
    run(
        &mut s,
        "plugin.samples.remove",
        json!({ "id": "plg_2", "bank": "a" }),
    );
    let x = plugin(&s);
    assert_eq!(x.samples, ["crash.wav"]);
    assert_eq!(def.file_pad(&x.params, 0), 16);
    // L2-smp-2: the pads whose files went lost their own values, so a file dropped there
    // later does not sound with the old Choke.
    assert!(!x.params.contains_key("A2.choke"), "{:?}", x.params);
    assert!(!x.params.contains_key("A3.layer_mode"), "{:?}", x.params);
    for bad in [
        json!({ "id": "plg_2", "bank": "Z" }),
        json!({ "id": "plg_2", "bank": "B", "pad": "B1" }),
    ] {
        assert!(
            s.run_value("plugin.samples.remove", bad, &RunOptions::human())
                .is_err()
        );
    }
    assert!(
        s.run_value(
            "plugin.samples.add",
            json!({ "id": "plg_2", "file": "x.wav", "pad": "Z9" }),
            &RunOptions::human()
        )
        .is_err()
    );
}

/// L2-smp-2: values left past the files' end are not a new file's, and --replace takes the
/// pads' own values with their files.
#[test]
fn new_files_do_not_take_old_values() {
    let mut p = project_with_samples(&["a.wav".to_string()]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        let x = &mut t.inserts[0];
        x.name = "Drum Kit".into();
        x.uid = "drum-kit".into();
        x.params.insert("file1.pad".into(), 0.0);
        x.params.insert("file2.top".into(), 0.5);
        x.params.insert("A1.choke".into(), 1.0);
    }
    let mut s = Session::from_project(p);
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "file": "b.wav" }),
    );
    assert!(!plugin(&s).params.contains_key("file2.top"));
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "file": "c.wav", "replace": true }),
    );
    let x = plugin(&s);
    assert_eq!(x.samples, ["c.wav"]);
    assert!(!x.params.contains_key("A1.choke"), "{:?}", x.params);
}

#[test]
fn a_devices_files_and_their_values_survive_a_save() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let mut s = Session::from_project(project_with_samples(&[]));
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "file": "gone/a_C3.wav" }),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "file1.loop", "plain": "on" }),
    );
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let back = Session::open(&song).unwrap();
    let x = back.project().tracks[&TrackId(1)].inserts[0].clone();
    assert_eq!(x.samples, ["gone/a_C3.wav"]);
    assert_eq!(x.params.get("file1.loop"), Some(&1.0));
    assert!(!back.is_dirty());
}

/// L2-smp-3: a hand-written key the device knows by another spelling (`FILE01.LOOP`) is read
/// under the key the song keeps it by, so the zone loops and the value is listed once.
#[test]
fn a_hand_written_key_is_read_as_the_song_keeps_it() {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let mut s = Session::from_project(project_with_samples(&[]));
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "file": "gone/a_C3.wav" }),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "file1.loop", "plain": "on" }),
    );
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let file = song.join("project.json");
    let body = fs::read_to_string(&file).unwrap();
    assert!(body.contains("\"file1.loop\""), "{body}");
    fs::write(&file, body.replace("\"file1.loop\"", "\"FILE01.LOOP\"")).unwrap();
    let back = Session::open(&song).unwrap();
    let x = back.project().tracks[&TrackId(1)].inserts[0].clone();
    assert_eq!(x.params.get("file1.loop"), Some(&1.0));
    assert!(!x.params.contains_key("FILE01.LOOP"));
}

/// N3-kit-2: files dropped on a Drum Kit's pad fill the pads from it in order, as the mock
/// does: a pad that has files gets the new ones in their place (its own values go, but its
/// Mute and Solo stay: the fourth review's low 21), and the ones past H16 do not go in, said
/// so.
#[test]
fn n3_kit_2_filled_pads_are_replaced_and_files_past_h16_left_out() {
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        let x = &mut t.inserts[0];
        x.name = "Drum Kit".into();
        x.uid = "drum-kit".into();
    }
    let mut s = Session::from_project(p);
    let old: Vec<Value> = (1..=8)
        .map(|i| json!({ "file": format!("old{i}.wav") }))
        .collect();
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "A1", "files": old }),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "a2.choke", "plain": "Self" }),
    );
    for key in ["a3.mute", "a4.solo"] {
        run(
            &mut s,
            "plugin.param.set",
            json!({ "id": "plg_2", "param": key, "plain": 1 }),
        );
    }
    assert!(plugin(&s).params.contains_key("A2.choke"));
    let held = |x: &Plugin| {
        (
            x.params.get("A3.mute").copied(),
            x.params.get("A4.solo").copied(),
        )
    };
    assert_eq!(held(&plugin(&s)), (Some(1.0), Some(1.0)));
    // 16 on A1 of a kit with 8 pads filled: all 16 go in, the 8 in place of the old.
    let new: Vec<Value> = (1..=16)
        .map(|i| json!({ "file": format!("new{i}.wav") }))
        .collect();
    let r = run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "A1", "files": new }),
    );
    assert_eq!(r["count"], json!(16));
    assert_eq!(r["replaced"].as_array().map(Vec::len), Some(8));
    assert_eq!(r["left_out"], json!(0));
    let def = xirius_params::find("drum-kit").unwrap();
    let x = plugin(&s);
    assert_eq!(x.samples.len(), 16);
    assert!(
        x.samples.iter().all(|f| f.starts_with("new")),
        "{:?}",
        x.samples
    );
    let mut pads: Vec<usize> = (0..16).map(|i| def.file_pad(&x.params, i)).collect();
    pads.sort_unstable();
    assert_eq!(pads, (0..16).collect::<Vec<_>>());
    assert!(!x.params.contains_key("A2.choke"), "{:?}", x.params);
    assert_eq!(held(&x), (Some(1.0), Some(1.0)), "{:?}", x.params);
    // 5 on H14: H14 to H16 take three, the other two are left out and said so.
    let five: Vec<Value> = (1..=5)
        .map(|i| json!({ "file": format!("end{i}.wav") }))
        .collect();
    let out = s
        .run_value(
            "plugin.samples.add",
            json!({ "id": "plg_2", "pad": "H14", "files": five }),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(out.data["pads"], json!(["H14", "H15", "H16"]));
    assert_eq!(out.data["left_out"], json!(2));
    assert!(out.message.contains("入りきらない 2 個"), "{}", out.message);
    assert_eq!(plugin(&s).samples.len(), 19);
}

/// Low 4, 9: an old Sampler's `tune` (±24 semitones, for every voice, before PLG-067) opens
/// as its `transpose` (±48) at the same pitch, its lane too; a preset of older keys
/// (`tune`, `CUTOFF`) goes in under the song's.
#[test]
fn an_old_tune_is_transpose_and_a_preset_of_old_keys_reads_as_the_song_keeps_them() {
    use xirius_core::auto::{AutoLane, AutoPoint, AutoTarget};
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        t.inserts[0].params.insert("tune".into(), 0.75);
        t.automation.push(AutoLane {
            suspended: false,
            target: AutoTarget::Param(PluginId(2), "tune".into()),
            hidden: false,
            points: std::sync::Arc::new(vec![AutoPoint {
                tick: 0,
                value: 0.25,
                curve: 0.0,
                step: false,
            }]),
        });
    }
    let mut s = Session::from_project(p);
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let body = fs::read_to_string(song.join("project.json")).unwrap();
    assert!(body.contains("\"tune\""), "{body}");
    let back = Session::open(&song).unwrap();
    let def = xirius_params::find("sampler").unwrap();
    let plain = |id: &str, v: f64| def.param(id).unwrap().1.to_plain(v);
    let x = back.project().tracks[&TrackId(1)].inserts[0].clone();
    assert!(!x.params.contains_key("tune"));
    assert_eq!(plain("transpose", x.params["transpose"]), 12.0);
    let lane = &back.project().tracks[&TrackId(1)].automation[0];
    assert_eq!(
        lane.target,
        AutoTarget::Param(PluginId(2), "transpose".into())
    );
    assert_eq!(plain("transpose", lane.points[0].value), -12.0);
    // A preset of older keys.
    let mut s = Session::from_project(project_with_samples(&[]));
    let preset = json!({
        "format": "xirius-preset", "version": 1, "kind": "instrument", "name": "Old",
        "devices": [{"name": "Sampler", "kind": "instrument", "format": "builtin",
                     "uid": "sampler", "params": {"tune": 0.25, "CUTOFF": 0.5}}]
    })
    .to_string();
    run(
        &mut s,
        "preset.put",
        json!({ "track": TrackId(1).to_string(), "preset": preset }),
    );
    let x = plugin(&s);
    assert!(!x.params.contains_key("tune") && !x.params.contains_key("CUTOFF"));
    assert_eq!(plain("transpose", x.params["transpose"]), -12.0);
    assert_eq!(x.params.get("cutoff"), Some(&0.5));
}

/// The fourth review's low 7: a lane under a hand-written key (`CUTOFF`) and a macro control
/// mapped to an old `tune` move with the values when the song opens, the map's range over
/// the wider `transpose`; a `tune` lane beside a `transpose` one gives way to it.
#[test]
fn lanes_and_macro_maps_of_older_keys_open_under_the_songs_keys() {
    use xirius_core::auto::{AutoLane, AutoPoint, AutoTarget};
    use xirius_core::controls::{ControlId, Mapping};
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let lane = |key: &str, value: f64| AutoLane {
        suspended: false,
        target: AutoTarget::Param(PluginId(2), key.into()),
        hidden: false,
        points: std::sync::Arc::new(vec![AutoPoint {
            tick: 0,
            value,
            curve: 0.0,
            step: false,
        }]),
    };
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        t.automation.push(lane("CUTOFF", 0.4));
        t.automation.push(lane("transpose", 0.6));
        t.automation.push(lane("tune", 0.25));
        t.controls.get_mut(ControlId::Knob(0)).maps.push(Mapping {
            target: AutoTarget::Param(PluginId(2), "tune".into()),
            from: 0.25,
            to: 0.75,
            curve: 0.0,
        });
    }
    let mut s = Session::from_project(p);
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let back = Session::open(&song).unwrap();
    let def = xirius_params::find("sampler").unwrap();
    let plain = |id: &str, v: f64| def.param(id).unwrap().1.to_plain(v);
    let t = &back.project().tracks[&TrackId(1)];
    let keys: Vec<(String, f64)> = t
        .automation
        .iter()
        .map(|l| match &l.target {
            AutoTarget::Param(_, k) => (k.clone(), l.points[0].value),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        keys,
        vec![("cutoff".to_string(), 0.4), ("transpose".to_string(), 0.6)]
    );
    let m = &t.controls.get(ControlId::Knob(0)).maps[0];
    assert_eq!(m.target, AutoTarget::Param(PluginId(2), "transpose".into()));
    assert_eq!(
        (plain("transpose", m.from), plain("transpose", m.to)),
        (-12.0, 12.0)
    );
}

/// The fifth review's low 4: a macro control mapped to a hand-written key (`CUTOFF`) sets the
/// song's key when the song opens, not the old one that is then dropped beside it.
#[test]
fn a_macro_on_an_older_key_sets_the_songs_key_when_the_song_opens() {
    use xirius_core::auto::AutoTarget;
    use xirius_core::controls::{ControlId, Mapping};
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        t.inserts[0].params.insert("cutoff".into(), 0.9);
        let knob = t.controls.get_mut(ControlId::Knob(0));
        knob.value = 0.5;
        knob.maps.push(Mapping {
            target: AutoTarget::Param(PluginId(2), "CUTOFF".into()),
            from: 0.0,
            to: 0.4,
            curve: 0.0,
        });
    }
    let mut s = Session::from_project(p);
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let back = Session::open(&song).unwrap();
    let t = &back.project().tracks[&TrackId(1)];
    let m = &t.controls.get(ControlId::Knob(0)).maps[0];
    assert_eq!(m.target, AutoTarget::Param(PluginId(2), "cutoff".into()));
    let x = &t.inserts[0];
    assert!((x.params["cutoff"] - 0.2).abs() < 1e-9, "{:?}", x.params);
    assert!(!x.params.contains_key("CUTOFF"), "{:?}", x.params);
}

/// The fifth review's low 5: two older keys that differ only in case (`CUTOFF`, `Cutoff`)
/// leave one lane and one macro's map on the song's key, not two.
#[test]
fn older_keys_that_differ_in_case_leave_one_lane_and_one_map() {
    use xirius_core::auto::{AutoLane, AutoPoint, AutoTarget};
    use xirius_core::controls::{ControlId, Mapping};
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let lane = |key: &str, value: f64| AutoLane {
        suspended: false,
        target: AutoTarget::Param(PluginId(2), key.into()),
        hidden: false,
        points: std::sync::Arc::new(vec![AutoPoint {
            tick: 0,
            value,
            curve: 0.0,
            step: false,
        }]),
    };
    let map = |key: &str| Mapping {
        target: AutoTarget::Param(PluginId(2), key.into()),
        from: 0.0,
        to: 1.0,
        curve: 0.0,
    };
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        t.automation.push(lane("CUTOFF", 0.4));
        t.automation.push(lane("Cutoff", 0.6));
        t.controls
            .get_mut(ControlId::Knob(0))
            .maps
            .push(map("CUTOFF"));
        t.controls
            .get_mut(ControlId::Knob(1))
            .maps
            .push(map("Cutoff"));
    }
    let mut s = Session::from_project(p);
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let back = Session::open(&song).unwrap();
    let t = &back.project().tracks[&TrackId(1)];
    let cutoff = AutoTarget::Param(PluginId(2), "cutoff".into());
    let lanes: Vec<_> = t.automation.iter().map(|l| l.target.clone()).collect();
    assert_eq!(lanes, vec![cutoff.clone()], "the first stays");
    assert_eq!(t.automation[0].points[0].value, 0.4);
    let maps: Vec<_> = t
        .controls
        .iter()
        .flat_map(|(_, c)| c.maps.iter().map(|m| m.target.clone()))
        .collect();
    assert_eq!(maps, vec![cutoff]);
}

/// The sixth review's low 6: of older keys that differ in case, the lane and the map that
/// stay are of the key whose value stays (`CUTOFF` before `Cutoff` in the keys' order),
/// whichever comes first in the song.
#[test]
fn older_keys_that_differ_in_case_leave_the_lane_and_map_of_the_value_kept() {
    use xirius_core::auto::{AutoLane, AutoPoint, AutoTarget};
    use xirius_core::controls::{ControlId, Mapping};
    let tmp = tempfile::Builder::new()
        .prefix("xirius-samples-")
        .tempdir()
        .unwrap();
    let song = tmp.path().join("song");
    let lane = |key: &str, value: f64| AutoLane {
        suspended: false,
        target: AutoTarget::Param(PluginId(2), key.into()),
        hidden: false,
        points: std::sync::Arc::new(vec![AutoPoint {
            tick: 0,
            value,
            curve: 0.0,
            step: false,
        }]),
    };
    let map = |key: &str, to: f64| Mapping {
        target: AutoTarget::Param(PluginId(2), key.into()),
        from: 0.0,
        to,
        curve: 0.0,
    };
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        let x = t.inserts.iter_mut().find(|x| x.id == PluginId(2)).unwrap();
        x.params.insert("Cutoff".into(), 0.6);
        x.params.insert("CUTOFF".into(), 0.4);
        t.automation.push(lane("Cutoff", 0.6));
        t.automation.push(lane("CUTOFF", 0.4));
        t.controls
            .get_mut(ControlId::Knob(0))
            .maps
            .push(map("Cutoff", 0.6));
        t.controls
            .get_mut(ControlId::Knob(1))
            .maps
            .push(map("CUTOFF", 0.4));
    }
    // The value that stays (the macro writes over it as the song opens).
    let mut x = p.tracks[&TrackId(1)]
        .inserts
        .iter()
        .find(|x| x.id == PluginId(2))
        .unwrap()
        .clone();
    x.fix_keys();
    assert_eq!(x.params.get("cutoff"), Some(&0.4));
    let mut s = Session::from_project(p);
    run(&mut s, "project.save-as", json!({ "path": text(&song) }));
    let back = Session::open(&song).unwrap();
    let t = &back.project().tracks[&TrackId(1)];
    let cutoff = AutoTarget::Param(PluginId(2), "cutoff".into());
    let lanes: Vec<_> = t.automation.iter().map(|l| l.target.clone()).collect();
    assert_eq!(lanes, vec![cutoff.clone()]);
    assert_eq!(t.automation[0].points[0].value, 0.4, "the lane of Cutoff");
    let maps: Vec<_> = t
        .controls
        .iter()
        .flat_map(|(_, c)| c.maps.iter().map(|m| (m.target.clone(), m.to)))
        .collect();
    assert_eq!(maps, vec![(cutoff, 0.4)], "the map of Cutoff");
}

/// The fifth review's low 11: files put in with `--replace` keep the Mute and Solo of the pads
/// they fill, as a drop that replaces does; a pad left empty loses them with its other
/// values.
#[test]
fn replaced_with_the_flag_the_pads_filled_keep_their_mute_and_solo() {
    let mut p = project_with_samples(&[]);
    {
        let t = std::sync::Arc::make_mut(p.tracks.get_mut(&TrackId(1)).unwrap());
        let x = &mut t.inserts[0];
        x.name = "Drum Kit".into();
        x.uid = "drum-kit".into();
    }
    let mut s = Session::from_project(p);
    let old: Vec<Value> = (1..=4)
        .map(|i| json!({ "file": format!("old{i}.wav") }))
        .collect();
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "files": old }),
    );
    run(
        &mut s,
        "plugin.param.set",
        json!({ "id": "plg_2", "param": "a1.choke", "plain": "Self" }),
    );
    for key in ["a2.mute", "a4.solo"] {
        run(
            &mut s,
            "plugin.param.set",
            json!({ "id": "plg_2", "param": key, "plain": 1 }),
        );
    }
    let new: Vec<Value> = (1..=2)
        .map(|i| json!({ "file": format!("new{i}.wav") }))
        .collect();
    run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "files": new, "replace": true }),
    );
    let x = plugin(&s);
    assert_eq!(x.samples, ["new1.wav", "new2.wav"]);
    assert_eq!(x.params.get("A2.mute"), Some(&1.0), "{:?}", x.params);
    assert!(!x.params.contains_key("A4.solo"), "{:?}", x.params);
    assert!(!x.params.contains_key("A1.choke"), "{:?}", x.params);
}
