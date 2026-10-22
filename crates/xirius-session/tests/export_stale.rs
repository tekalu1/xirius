//! Which exported files the song has moved away from (EXP-019, EXP-020): each export keeps a
//! fingerprint of every track and bus that plays into each file, so the files a change reaches
//! read as stale, and only those, also after the song is opened again; `export.update` writes
//! those files again and leaves the others as they are. Every render is offline; nothing here
//! opens an audio device or a file window, and the files written again go to a bin of the test's
//! own folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::jobs::Store;
use xirius_session::output::OutputMode;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-stale-test-{}-{n}-{tag}",
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
fn song_in(tmp: &Path, name: &str) -> Session {
    let dir = tmp.join("song");
    if !dir.exists() {
        drop(Session::create(&dir, Project::new(name)).unwrap());
    }
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach_with(&mut s, config(tmp), extras(tmp));
    s
}

/// The names of the files in `<tmp>/bin`, sorted.
fn binned(tmp: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(tmp.join("bin"))
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
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

/// The strip and the path of each file a stems record wrote, in its order.
fn stem_files(v: &Value) -> Vec<(String, PathBuf)> {
    v["result"]["files"]
        .as_array()
        .unwrap_or_else(|| panic!("no files in {v}"))
        .iter()
        .map(|f| {
            (
                f["strip"].as_str().unwrap().to_string(),
                PathBuf::from(f["path"].as_str().unwrap()),
            )
        })
        .collect()
}

/// The bytes of a file and when it was last written.
fn snapshot(p: &Path) -> (Vec<u8>, std::time::SystemTime) {
    (
        std::fs::read(p).unwrap(),
        std::fs::metadata(p).unwrap().modified().unwrap(),
    )
}

fn path_of(files: &[(String, PathBuf)], strip: &str) -> PathBuf {
    files
        .iter()
        .find(|(s, _)| s == strip)
        .unwrap_or_else(|| panic!("no file of {strip}"))
        .1
        .clone()
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
    let mut s = song_in(dir, "Stale");
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
fn a_change_makes_only_the_files_it_plays_into_stale() {
    let dir = Dir::new("per-strip");
    let Song {
        mut s,
        keys,
        bass,
        low,
        stems,
        mix,
    } = exported(dir.path());
    for job in [&stems, &mix] {
        let v = status(&mut s, job);
        assert_eq!(v["stale"], false, "{v}");
        assert!(changed(&v).is_empty(), "{v}");
    }

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let v = status(&mut s, &stems);
    assert_eq!(v["stale"], true);
    assert_eq!(changed(&v), vec![keys.clone()]);
    assert_eq!(v["stale_tracks"][0]["name"], "Keys");
    assert!(file_stale(&v, &keys));
    assert!(!file_stale(&v, &bass), "Bass does not hear Keys: {v}");
    assert!(!file_stale(&v, &low), "nor does the bus Bass plays into");
    let m = status(&mut s, &mix);
    assert_eq!(m["stale"], true, "the mix hears everything");
    assert_eq!(changed(&m), vec![keys.clone()]);

    // A track that plays into a bus makes the bus's stem stale as well.
    run(
        &mut s,
        "track.pan.set",
        json!({ "ids": [bass], "pan": 0.5 }),
    );
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &bass));
    assert!(file_stale(&v, &low), "{v}");
    let mut both = changed(&v);
    both.sort();
    let mut want = vec![keys.clone(), bass.clone()];
    want.sort();
    assert_eq!(both, want);
}

#[test]
fn a_name_or_a_colour_is_not_a_change_of_sound() {
    let dir = Dir::new("names");
    let Song {
        mut s,
        keys,
        stems,
        mix,
        ..
    } = exported(dir.path());
    run(
        &mut s,
        "track.rename",
        json!({ "id": keys, "name": "Piano" }),
    );
    run(
        &mut s,
        "track.color.set",
        json!({ "ids": [keys], "color": "#123456" }),
    );
    for job in [&stems, &mix] {
        let v = status(&mut s, job);
        assert_eq!(v["stale"], false, "{v}");
    }
    // A changed track is named as it is called now.
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -3.0 }),
    );
    assert_eq!(status(&mut s, &mix)["stale_tracks"][0]["name"], "Piano");
}

#[test]
fn undoing_the_change_makes_the_files_current_again() {
    let dir = Dir::new("undo");
    let Song {
        mut s, keys, stems, ..
    } = exported(dir.path());
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    assert_eq!(status(&mut s, &stems)["stale"], true);
    s.undo(1).unwrap();
    let v = status(&mut s, &stems);
    assert_eq!(v["stale"], false, "{v}");
    assert!(!file_stale(&v, &keys));
}

#[test]
fn stale_files_stay_stale_when_the_song_is_opened_again() {
    let dir = Dir::new("reopen");
    let Song {
        s,
        keys,
        bass,
        stems,
        mix,
        ..
    } = exported(dir.path());
    drop(s);
    // Opened again as it was: nothing is stale.
    let mut s = song_in(dir.path(), "Stale");
    assert_eq!(status(&mut s, &stems)["stale"], false);
    assert_eq!(status(&mut s, &mix)["stale"], false);

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    s.save().unwrap();
    drop(s);
    let mut s = song_in(dir.path(), "Stale");
    let v = status(&mut s, &stems);
    assert_eq!(v["stale"], true, "{v}");
    assert_eq!(changed(&v), vec![keys.clone()]);
    assert!(file_stale(&v, &keys));
    assert!(!file_stale(&v, &bass));
    assert_eq!(status(&mut s, &mix)["stale"], true);
}

#[test]
fn a_new_track_counts_only_when_it_has_something_to_play() {
    let dir = Dir::new("new-track");
    let Song {
        mut s, stems, mix, ..
    } = exported(dir.path());
    run(&mut s, "track.add.instrument", json!({ "name": "Empty" }));
    assert_eq!(status(&mut s, &mix)["stale"], false);
    let lead = voice(&mut s, "Lead", 79);
    let m = status(&mut s, &mix);
    assert_eq!(m["stale"], true);
    assert_eq!(changed(&m), vec![lead]);
    // The stems export did not list Lead, so its files are as they were.
    assert_eq!(status(&mut s, &stems)["stale"], false);
}

#[test]
fn a_tempo_change_makes_every_file_stale() {
    let dir = Dir::new("tempo");
    let Song {
        mut s,
        keys,
        bass,
        stems,
        ..
    } = exported(dir.path());
    run(&mut s, "tempo.set", json!({ "bpm": 90.0 }));
    let v = status(&mut s, &stems);
    assert_eq!(v["stale"], true);
    assert_eq!(v["song_changed"], true, "{v}");
    assert!(file_stale(&v, &keys));
    assert!(file_stale(&v, &bass));
}

#[test]
fn a_record_from_before_fingerprints_says_nothing_of_staleness() {
    let dir = Dir::new("old-record");
    let Song { s, stems, .. } = exported(dir.path());
    drop(s);
    let store = Store::of_song(&dir.path().join("song"));
    let mut v = store.read(&stems).unwrap();
    v.as_object_mut().unwrap().remove("base");
    store.write(&v).unwrap();
    let mut s = song_in(dir.path(), "Stale");
    let v = status(&mut s, &stems);
    assert_eq!(v["state"], "done");
    assert!(v["stale"].is_null(), "{v}");
}

#[test]
fn update_writes_again_only_the_stems_that_are_stale() {
    let dir = Dir::new("update-stale");
    let Song {
        mut s,
        keys,
        bass,
        low,
        stems,
        ..
    } = exported(dir.path());
    let files = stem_files(&status(&mut s, &stems));
    let keys_file = path_of(&files, &keys);
    let keys_was = snapshot(&keys_file).0;
    let kept: Vec<_> = [&bass, &low]
        .iter()
        .map(|id| {
            let p = path_of(&files, id);
            let was = snapshot(&p);
            (p, was)
        })
        .collect();
    // The clock moves on, so a file written again reads as written later.
    std::thread::sleep(std::time::Duration::from_millis(30));

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let r = run(&mut s, "export.update", json!({ "job": stems }));
    let new = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &new);
    assert_eq!(v["stale"], false, "{v}");
    assert_eq!(
        stem_files(&v),
        files,
        "the record keeps every file in its order"
    );
    assert_ne!(snapshot(&keys_file).0, keys_was, "Keys is written again");
    for (p, was) in &kept {
        assert_eq!(&snapshot(p), was, "{} is left as it was", p.display());
    }
    assert_eq!(
        binned(dir.path()),
        vec![
            keys_file
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        ],
        "only the file written again goes to the bin"
    );
    let jobs = run(&mut s, "export.status", json!({})).data["jobs"].clone();
    assert!(
        jobs.as_array()
            .unwrap()
            .iter()
            .all(|j| j["job"] != stems.as_str()),
        "the old line leaves the list: {jobs}"
    );
    // Also when the song is opened again.
    s.save().unwrap();
    drop(s);
    let mut s = song_in(dir.path(), "Stale");
    assert_eq!(status(&mut s, &new)["stale"], false);
}

#[test]
fn a_file_named_is_written_again_alone_and_the_others_stay_stale() {
    let dir = Dir::new("update-file");
    let Song {
        mut s,
        keys,
        bass,
        low,
        stems,
        ..
    } = exported(dir.path());
    let files = stem_files(&status(&mut s, &stems));
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    run(
        &mut s,
        "track.pan.set",
        json!({ "ids": [bass], "pan": 0.5 }),
    );
    let bass_file = path_of(&files, &bass);
    let name = bass_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let r = run(
        &mut s,
        "export.update",
        json!({ "job": stems, "file": name }),
    );
    let new = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &new);
    assert!(!file_stale(&v, &bass), "{v}");
    assert!(file_stale(&v, &keys), "Keys was not written again: {v}");
    assert!(
        file_stale(&v, &low),
        "Low still holds the Bass of before: {v}"
    );
    assert_eq!(binned(dir.path()), vec![name.clone()]);

    // The rest, from the new line.
    let r = run(&mut s, "export.update", json!({ "job": new }));
    let last = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &last);
    assert_eq!(v["stale"], false, "{v}");
    assert_eq!(binned(dir.path()).len(), 3);

    // A name the export did not write is refused.
    let e = commands::run(
        &mut s,
        None,
        "export.update",
        &map(json!({ "job": last, "file": "nothing.wav" })),
        &RunOptions::human(),
    )
    .unwrap_err();
    assert!(e.message.contains("nothing.wav"), "{}", e.message);
}

#[test]
fn a_change_of_tempo_writes_every_stem_again() {
    let dir = Dir::new("update-all");
    let Song { mut s, stems, .. } = exported(dir.path());
    let files = stem_files(&status(&mut s, &stems));
    run(&mut s, "tempo.set", json!({ "bpm": 90.0 }));
    let r = run(&mut s, "export.update", json!({ "job": stems }));
    let new = r.data["record"].as_str().unwrap().to_string();
    assert_eq!(status(&mut s, &new)["stale"], false);
    assert_eq!(binned(dir.path()).len(), files.len());
}

#[test]
fn one_file_of_an_export_is_shown_heard_and_brought_in_by_its_name() {
    let dir = Dir::new("one-file");
    let Song {
        mut s, bass, stems, ..
    } = exported(dir.path());
    let files = stem_files(&status(&mut s, &stems));
    let bass_file = path_of(&files, &bass);
    let name = bass_file
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let dry = RunOptions {
        dry_run: true,
        ..RunOptions::human()
    };
    let shown = commands::run(
        &mut s,
        None,
        "export.reveal_os",
        &map(json!({ "job": stems, "file": name })),
        &dry,
    )
    .unwrap();
    assert_eq!(
        PathBuf::from(shown.data["path"].as_str().unwrap()),
        bass_file
    );
    let before = s.project().tracks.len();
    let r = run(
        &mut s,
        "export.import",
        json!({ "job": stems, "file": bass_file }),
    );
    assert_eq!(r.data["files"].as_array().unwrap().len(), 1, "{}", r.data);
    assert_eq!(s.project().tracks.len(), before + 1);
    let heard = run(
        &mut s,
        "export.preview",
        json!({ "job": stems, "file": name, "output": "silent" }),
    );
    assert_eq!(heard.data["files"].as_array().unwrap().len(), 1);
    run(
        &mut s,
        "export.preview",
        json!({ "job": stems, "stop": true }),
    );
}

#[test]
fn an_update_in_the_background_keeps_the_files_not_written_again() {
    let dir = Dir::new("update-background");
    let Song {
        mut s, keys, stems, ..
    } = exported(dir.path());
    let files = stem_files(&status(&mut s, &stems));
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let r = run(
        &mut s,
        "export.update",
        json!({ "job": stems, "background": true }),
    );
    let job = r.data["job"].as_str().unwrap().to_string();
    let start = std::time::Instant::now();
    let v = loop {
        let v = status(&mut s, &job);
        if v["state"] != "running" {
            break v;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "{v}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert_eq!(v["state"], "done", "{v}");
    assert_eq!(v["stale"], false, "{v}");
    assert_eq!(stem_files(&v), files);
    assert_eq!(v["result"]["kept"], 2);
    assert_eq!(binned(dir.path()).len(), 1);
}

#[test]
fn a_track_that_sends_to_a_bus_makes_the_buss_stem_stale_when_it_is_edited() {
    let dir = Dir::new("sends");
    let mut s = song_in(dir.path(), "Sends");
    let keys = voice(&mut s, "Keys", 72);
    let pad = voice(&mut s, "Pad", 60);
    let verb = id_of(&run(&mut s, "track.add.bus", json!({ "name": "Verb" })));
    // Keys plays to the master and sends to Verb; Pad only plays to the master.
    let send = id_of(&run(
        &mut s,
        "send.add",
        json!({ "ids": [keys], "to": verb, "level": -6.0 }),
    ));
    let stems = run(&mut s, "export.stems", json!({ "bit_depth": 16 })).data["record"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(status(&mut s, &stems)["stale"], false);

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &keys));
    assert!(
        file_stale(&v, &verb),
        "Verb hears Keys through the send: {v}"
    );
    assert!(!file_stale(&v, &pad));
    assert_eq!(changed(&v), vec![keys.clone()]);
    s.undo(1).unwrap();
    assert_eq!(status(&mut s, &stems)["stale"], false);

    // Only the send changes: the bus hears it.
    run(&mut s, "send.set", json!({ "id": send, "level": -12.0 }));
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &verb), "{v}");
    assert!(!file_stale(&v, &pad));
    s.undo(1).unwrap();
    assert_eq!(status(&mut s, &stems)["stale"], false);

    // A send made after the export: the bus now hears a track it did not.
    run(&mut s, "send.add", json!({ "ids": [pad], "to": verb }));
    let v = status(&mut s, &stems);
    assert!(file_stale(&v, &verb), "{v}");
    assert!(
        v["stale_files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["strip"] == verb.as_str())
            .unwrap()["tracks"]
            .as_array()
            .unwrap()
            .contains(&json!(pad)),
        "{v}"
    );
}
