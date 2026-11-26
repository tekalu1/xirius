//! What the song does not show, told (EXP-023): `export.touch` makes the files the tracks named
//! play into stale, also after the song is opened again, until they are written again; and from
//! `export.watch` on, each run that turns the files of an export stale or current again hands
//! an `export.stale` event with its result. Every render is offline; nothing here opens an audio
//! device or a file window, and the files written again go to a bin of the test's own folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-watch-test-{}-{n}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn config(dir: &Path) -> ExtConfig {
    ExtConfig {
        output: OutputMode::None,
        scan: ScanOptions {
            dirs: vec![],
            cache: dir.join("plugins.json"),
            ..ScanOptions::default()
        },
        catalog: Some(Arc::new(Catalog::default())),
    }
}

/// Files sent to the recycle bin go to `<tmp>/bin` instead.
fn extras(tmp: &Path) -> ExtExtras {
    let bin = tmp.join("bin");
    ExtExtras {
        trash: Some(Arc::new(move |p: &Path| {
            std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let mut to = bin.join(&name);
            let mut n = 1;
            while to.exists() {
                n += 1;
                to = bin.join(format!("{n}-{name}"));
            }
            std::fs::rename(p, to).map_err(|e| e.to_string())
        })),
        ..ExtExtras::default()
    }
}

/// A song folder `<tmp>/song`, made the first time and opened as the app opens it.
fn song_in(tmp: &Path) -> Session {
    let dir = tmp.join("song");
    if !dir.exists() {
        drop(Session::create(&dir, Project::new("Watch")).unwrap());
    }
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach_with(&mut s, config(tmp), extras(tmp));
    s
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {}", e.message))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn voice(s: &mut Session, name: &str, pitch: i64) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": name })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 1.0 }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": pitch, "length": 1920, "velocity": 100 }),
    );
    t
}

/// The record of `job` as `export.status` lists it.
fn status(s: &mut Session, job: &str) -> Value {
    run(s, "export.status", json!({})).data["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["job"] == job)
        .cloned()
        .unwrap_or_else(|| panic!("{job} is not listed"))
}

/// The ids of the tracks a record says changed.
fn changed(v: &Value) -> Vec<String> {
    v["stale_tracks"]
        .as_array()
        .unwrap_or_else(|| panic!("no stale_tracks in {v}"))
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

/// Whether the file of `strip` in a stems record is stale.
fn file_stale(v: &Value, strip: &str) -> bool {
    v["stale_files"]
        .as_array()
        .unwrap_or_else(|| panic!("no stale_files in {v}"))
        .iter()
        .find(|f| f["strip"] == strip)
        .unwrap_or_else(|| panic!("no file of {strip} in {v}"))["stale"]
        .as_bool()
        .unwrap()
}

/// The name of the file of `strip` in a stems record.
fn file_name(v: &Value, strip: &str) -> String {
    v["result"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["strip"] == strip)
        .unwrap_or_else(|| panic!("no file of {strip} in {v}"))["path"]
        .as_str()
        .map(|p| {
            Path::new(p)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .unwrap()
}

/// A song of Keys (to the master) and Bass (into the bus Low), its stems and its mix.
struct Song {
    s: Session,
    keys: String,
    bass: String,
    low: String,
    stems: String,
    mix: String,
}

fn exported(dir: &Path) -> Song {
    let mut s = song_in(dir);
    let keys = voice(&mut s, "Keys", 72);
    let bass = voice(&mut s, "Bass", 40);
    let low = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Low" })));
    run(
        &mut s,
        "track.output.set",
        json!({ "ids": [bass], "to": low }),
    );
    let stems = run(&mut s, "export.stems", json!({ "bit_depth": 16 })).data["record"]
        .as_str()
        .unwrap()
        .to_string();
    let mix = run(&mut s, "export.mixdown", json!({ "bit_depth": 16 })).data["record"]
        .as_str()
        .unwrap()
        .to_string();
    Song {
        s,
        keys,
        bass,
        low,
        stems,
        mix,
    }
}

#[test]
fn a_touched_track_makes_the_files_it_plays_into_stale_until_they_are_written_again() {
    let dir = Dir::new("touch");
    let Song {
        mut s,
        keys,
        bass,
        low,
        stems,
        mix,
    } = exported(dir.path());
    let before = s.project().clone();

    // Asked first: nothing is marked.
    let r = commands::run(
        &mut s,
        None,
        "export.touch",
        &map(json!({ "tracks": [keys] })),
        &RunOptions::human().dry(),
    )
    .unwrap();
    assert_eq!(r.data["jobs"].as_array().unwrap().len(), 2, "{:?}", r.data);
    assert_eq!(status(&mut s, &stems)["stale"], false);

    let r = run(&mut s, "export.touch", json!({ "tracks": [keys] }));
    let mut told = r.data["jobs"].as_array().unwrap().clone();
    told.sort_by_key(|j| j.as_str().unwrap().to_string());
    let mut want = vec![json!(stems), json!(mix)];
    want.sort_by_key(|j| j.as_str().unwrap().to_string());
    assert_eq!(told, want);
    assert_eq!(*s.project(), before, "the song itself is left as it was");
    let v = status(&mut s, &stems);
    assert_eq!(v["stale"], true, "{v}");
    assert_eq!(changed(&v), vec![keys.clone()]);
    assert!(file_stale(&v, &keys));
    assert!(!file_stale(&v, &bass), "Bass does not hear Keys: {v}");
    assert!(!file_stale(&v, &low));
    let m = status(&mut s, &mix);
    assert_eq!(m["stale"], true);
    assert_eq!(changed(&m), vec![keys.clone()]);

    // Told again, nothing new.
    let r = run(&mut s, "export.touch", json!({ "tracks": [keys] }));
    assert!(
        r.data["jobs"].as_array().unwrap().is_empty(),
        "{:?}",
        r.data
    );

    // The mark stays when the song is opened again, and a record only on disk takes one too:
    // a track into a bus marks the bus's stem.
    drop(s);
    let mut s = song_in(dir.path());
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &keys), "{v}");
    run(&mut s, "export.touch", json!({ "tracks": [bass] }));
    drop(s);
    let mut s = song_in(dir.path());
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &bass), "{v}");
    assert!(file_stale(&v, &low), "{v}");
    let mut both = changed(&v);
    both.sort();
    let mut want = vec![keys.clone(), bass.clone()];
    want.sort();
    assert_eq!(both, want);

    // Writing one file again clears its mark only; the others keep theirs.
    let name = file_name(&v, &keys);
    let r = run(
        &mut s,
        "export.update",
        json!({ "job": stems, "file": name }),
    );
    let new = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &new);
    assert!(!file_stale(&v, &keys), "{v}");
    assert!(file_stale(&v, &bass), "{v}");
    assert!(file_stale(&v, &low), "{v}");
    let r = run(&mut s, "export.update", json!({ "job": new }));
    let last = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &last);
    assert_eq!(v["stale"], false, "{v}");
    assert!(v.get("touched").is_none(), "{v}");
    let r = run(&mut s, "export.update", json!({ "job": mix }));
    let v = status(&mut s, r.data["record"].as_str().unwrap());
    assert_eq!(v["stale"], false, "{v}");
}

#[test]
fn a_track_not_in_the_song_is_refused() {
    let dir = Dir::new("touch-missing");
    let Song { mut s, keys, .. } = exported(dir.path());
    let gone = keys.clone();
    run(&mut s, "track.delete", json!({ "ids": [keys] }));
    let e = commands::run(
        &mut s,
        None,
        "export.touch",
        &map(json!({ "tracks": [gone] })),
        &RunOptions::human(),
    )
    .unwrap_err();
    assert!(e.message.contains(&gone), "{}", e.message);
}

#[test]
fn watching_hands_an_event_each_time_an_export_turns_stale_or_current() {
    let dir = Dir::new("watch");
    let Song {
        mut s,
        keys,
        bass,
        stems,
        mix,
        ..
    } = exported(dir.path());
    // Not watched: no events.
    let r = run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    assert!(r.events.is_empty(), "{:?}", r.events);
    run(&mut s, "history.undo", json!({}));

    let r = run(&mut s, "export.watch", json!({}));
    assert_eq!(r.data["watching"], true);
    let exports = r.data["exports"].as_array().unwrap();
    assert_eq!(exports.len(), 2, "{exports:?}");
    assert!(exports.iter().all(|e| e["stale"] == false), "{exports:?}");
    assert!(r.events.is_empty(), "starting tells nothing");

    // An edit: each export it reaches, once.
    let r = run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let mut jobs: Vec<String> = r
        .events
        .iter()
        .map(|e| e["job"].as_str().unwrap().to_string())
        .collect();
    jobs.sort();
    let mut want = vec![stems.clone(), mix.clone()];
    want.sort();
    assert_eq!(jobs, want, "{:?}", r.events);
    let e = r
        .events
        .iter()
        .find(|e| e["job"] == stems.as_str())
        .unwrap();
    assert_eq!(e["event"], "export.stale");
    assert_eq!(e["stale"], true);
    assert_eq!(e["stale_tracks"], json!([{ "id": keys, "name": "Keys" }]));
    let files = e["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "{files:?}");
    assert_eq!(
        files.iter().filter(|f| f["stale"] == true).count(),
        1,
        "{files:?}"
    );
    assert!(
        files
            .iter()
            .all(|f| f["name"].is_string() && f["path"].is_string())
    );

    // The same tracks again: nothing new to tell.
    let r = run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -6.0 }),
    );
    assert!(r.events.is_empty(), "{:?}", r.events);
    // Taken back to where they were written: current again.
    run(&mut s, "history.undo", json!({}));
    let r = run(&mut s, "history.undo", json!({}));
    assert_eq!(r.events.len(), 2, "{:?}", r.events);
    assert!(
        r.events.iter().all(|e| e["stale"] == false),
        "{:?}",
        r.events
    );

    // A touch tells too.
    let r = run(&mut s, "export.touch", json!({ "tracks": [bass] }));
    assert_eq!(r.events.len(), 2, "{:?}", r.events);
    assert!(r.events.iter().all(|e| e["stale"] == true));

    // Stopped: no more.
    let r = run(&mut s, "export.watch", json!({ "stop": true }));
    assert_eq!(r.data["watching"], false);
    let r = run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    assert!(r.events.is_empty(), "{:?}", r.events);
}
