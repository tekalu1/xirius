//! CTX-018: a folder as one stem. A folder connected to a bus is written as the bus's mix under
//! the folder's name; one without a bus is refused. Every render is offline.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::cleanup::Trash;
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::{self, Options};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-folder-test-{}-{n}",
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

/// Attached as the app does, with the recycle bin a folder `<dir>/bin`.
fn attach(s: &mut Session, dir: &Path) {
    let bin = dir.join("bin");
    let trash: Trash = Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    });
    commands::attach_with(
        s,
        ExtConfig {
            output: OutputMode::None,
            scan: ScanOptions {
                dirs: vec![],
                cache: dir.join("plugins.json"),
                ..ScanOptions::default()
            },
            catalog: Some(Arc::new(Catalog::default())),
        },
        ExtExtras {
            trash: Some(trash),
            ..ExtExtras::default()
        },
    );
}

fn song(dir: &Path) -> Session {
    let mut s = Session::with_registry(Project::new("Demo"), shared_registry());
    attach(&mut s, dir);
    s
}

/// A song saved in `<dir>/song`, whose exports are recorded.
fn saved(dir: &Path) -> Session {
    let at = dir.join("song");
    drop(Session::create(&at, Project::new("Demo")).unwrap());
    let mut s = Session::open_with(&at, shared_registry()).unwrap();
    attach(&mut s, dir);
    s
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, String> {
    commands::run(s, None, name, &map(args), &RunOptions::human()).map_err(|e| e.message)
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    try_run(s, name, args).unwrap_or_else(|e| panic!("{name}: {e}"))
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
        json!({ "track": t, "at": 0, "bars": 1 }),
    ));
    for (i, p) in [pitch, pitch + 4, pitch + 7].into_iter().enumerate() {
        run(
            s,
            "note.add",
            json!({ "clip": c, "at": i as i64 * 480, "pitch": p, "length": 3000, "velocity": 90 + i as i64 * 10 }),
        );
    }
    t
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn floats(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).unwrap();
    r.samples::<f32>().map(|x| x.unwrap()).collect()
}

/// Strings (A, B) in a folder; C outside it. `bus` connects the folder to a new bus.
fn arranged(dir: &Path, bus: bool) -> (Session, String, String) {
    arrange(song(dir), bus)
}

fn arrange(mut s: Session, bus: bool) -> (Session, String, String) {
    let a = voice(&mut s, "A", 60);
    let b = voice(&mut s, "B", 67);
    let c = voice(&mut s, "C", 72);
    let f = id_of(&run(&mut s, "folder.add", json!({ "name": "Strings" })));
    run(&mut s, "track.move", json!({ "ids": [a, b], "into": f }));
    if bus {
        run(
            &mut s,
            "folder.bus.add",
            json!({ "id": f, "name": "Strings Bus" }),
        );
    }
    (s, f, c)
}

#[test]
fn a_folder_with_a_bus_is_one_file_named_after_the_folder() {
    let dir = Dir::new();
    let (mut s, f, _) = arranged(dir.path(), true);
    let out = dir.path().join("out");
    run(
        &mut s,
        "export.stems",
        json!({ "folders": [f], "dir": out.display().to_string(), "bit_depth": 32 }),
    );
    assert_eq!(file_names(&out), vec!["Demo_Strings.wav"]);

    // What is in it is what the folder plays: the song without the track outside the folder.
    let c = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "C")
        .map(|t| t.id.to_string())
        .unwrap();
    run(&mut s, "track.mute", json!({ "ids": [c] }));
    let whole = dir.path().join("whole.wav");
    export::mixdown(
        s.project(),
        &whole,
        &Options {
            bit_depth: Some(32),
            ..Options::default()
        },
    )
    .unwrap();
    let (stem, mix) = (floats(&out.join("Demo_Strings.wav")), floats(&whole));
    assert_eq!(stem.len(), mix.len());
    let peak = stem.iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak > 0.01, "the folder stem is not silent ({peak})");
    let worst = stem
        .iter()
        .zip(&mix)
        .fold(0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < 1e-5, "the stem is the folder's sum ({worst})");
}

#[test]
fn a_folder_and_loose_tracks_make_one_file_each_and_the_name_pattern_applies() {
    let dir = Dir::new();
    let (mut s, f, c) = arranged(dir.path(), true);
    let out = dir.path().join("out");
    run(
        &mut s,
        "export.stems",
        json!({ "folders": [f], "ids": [c], "name": "st_{トラック}", "dir": out.display().to_string() }),
    );
    assert_eq!(file_names(&out), vec!["st_C.wav", "st_Strings.wav"]);
}

#[test]
fn a_folder_without_a_bus_cannot_be_one_stem() {
    let dir = Dir::new();
    let (mut s, f, _) = arranged(dir.path(), false);
    let out = dir.path().join("out");
    let said = try_run(
        &mut s,
        "export.stems",
        json!({ "folders": [f], "dir": out.display().to_string() }),
    )
    .unwrap_err();
    assert!(said.contains("Strings") && said.contains("バス"), "{said}");
    assert!(file_names(&out).is_empty(), "nothing is written");
    let said = try_run(
        &mut s,
        "export.stems",
        json!({ "folders": ["fld-9999"], "dir": out.display().to_string() }),
    )
    .unwrap_err();
    assert!(!said.is_empty());
}

#[test]
fn the_same_export_again_keeps_the_folder() {
    let dir = Dir::new();
    let (mut s, f, _) = arrange(saved(dir.path()), true);
    let out = dir.path().join("out");
    let r = run(
        &mut s,
        "export.stems",
        json!({ "folders": [f], "dir": out.display().to_string(), "overwrite": true }),
    );
    let job = r.data["record"].as_str().expect("a record id").to_string();
    let listed = run(&mut s, "export.status", json!({})).data["jobs"][0].clone();
    assert_eq!(listed["args"]["folders"], json!([f]), "{listed}");
    assert_eq!(listed["args"]["ids"], json!([]), "{listed}");
    run(&mut s, "export.update", json!({ "job": job }));
    assert_eq!(file_names(&out), vec!["Demo_Strings.wav"]);
}
