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

    // The next ones go on the free pads from the one asked for.
    let r = run(
        &mut s,
        "plugin.samples.add",
        json!({ "id": "plg_2", "pad": "A2",
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
