//! `export.midi` (EXP-026): the notes of instrument tracks as a standard MIDI file, one for all
//! the tracks or one for each, with the song's name, tempo and time signatures and each track's
//! name. The record keeps fingerprints of the notes, so moving a note makes the file stale and
//! turning a knob does not; the file comes back into the song at the head of the range written.
//! Nothing here opens an audio device, and the files written again go to a bin of the test's
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
use xirius_session::output::OutputMode;
use xirius_session::smf;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-midi-test-{}-{n}-{tag}",
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
            std::fs::rename(p, bin.join(name)).map_err(|e| e.to_string())
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

fn refused(s: &mut Session, name: &str, args: Value) -> String {
    match commands::run(s, None, name, &map(args), &RunOptions::human()) {
        Ok(r) => panic!("{name} ran: {:?}", r.data),
        Err(e) => e.message,
    }
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// An instrument track with one bar-long clip at `bar` holding one note; the track, the clip
/// and the note.
fn voice(s: &mut Session, name: &str, pitch: i64, bar: i64) -> (String, String, String) {
    let t = id_of(&run(s, "track.add.instrument", json!({ "name": name })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": format!("{bar}.1.0"), "bars": 1.0 }),
    ));
    let n = id_of(&run(
        s,
        "note.add",
        json!({ "clip": c, "at": format!("{bar}.1.0"), "pitch": pitch, "length": 480, "velocity": 100 }),
    ));
    (t, c, n)
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

fn changed(v: &Value) -> Vec<String> {
    v["stale_tracks"]
        .as_array()
        .unwrap_or_else(|| panic!("no stale_tracks in {v}"))
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect()
}

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

fn read(path: &Path) -> smf::Smf {
    smf::read_song(&std::fs::read(path).unwrap(), 960).unwrap()
}

fn names(song: &smf::Smf) -> Vec<String> {
    song.parts
        .iter()
        .map(|p| p.name.clone().unwrap_or_default())
        .collect()
}

#[test]
fn one_file_holds_every_instrument_track_with_the_tempo_and_the_meter() {
    let dir = Dir::new("one");
    let mut s = song_in(dir.path(), "Night");
    run(&mut s, "tempo.set", json!({ "bpm": 100.0 }));
    run(&mut s, "timesig.set", json!({ "num": 3, "den": 4 }));
    let (keys, ..) = voice(&mut s, "Keys", 72, 1);
    run(&mut s, "track.add.audio", json!({ "name": "Vox" }));
    let (bass, ..) = voice(&mut s, "Bass", 40, 2);

    let r = run(&mut s, "export.midi", json!({}));
    let path = PathBuf::from(r.data["path"].as_str().unwrap());
    assert_eq!(path, dir.path().join("song").join("MIDI").join("Night.mid"));
    assert_eq!(r.data["notes"], 2, "{}", r.data);
    assert_eq!(r.data["files"].as_array().unwrap().len(), 1);
    let song = read(&path);
    assert_eq!(names(&song), ["Keys", "Bass"], "the column's order");
    assert_eq!(song.parts[0].notes[0].pitch, 72);
    assert_eq!(
        (song.parts[0].notes[0].at, song.parts[0].notes[0].length),
        (0, 480)
    );
    assert_eq!(song.parts[1].notes[0].pitch, 40);
    assert_eq!(song.parts[1].notes[0].at, 3 * 960, "bar 2 of 3/4");
    assert_eq!(song.parts[1].notes[0].velocity, 100);
    assert_eq!(song.tempo[0].0, 0);
    assert!((song.tempo[0].1 - 100.0).abs() < 0.01, "{:?}", song.tempo);
    assert_eq!(song.meter[0], (0, 3, 4));
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        bytes.windows(5).any(|w| w == b"Night"),
        "the song's name is in the file"
    );

    // The record stands with the mix and the stems, and comes back as tracks.
    let job = r.data["record"].as_str().unwrap().to_string();
    let v = status(&mut s, &job);
    assert_eq!(v["command"], "export.midi");
    assert_eq!(v["stale"], false, "{v}");
    let before = s.project().tracks.len();
    let imported = run(&mut s, "export.import", json!({ "job": job }));
    assert_eq!(imported.data["tracks"], 2, "{}", imported.data);
    assert_eq!(s.project().tracks.len(), before + 2);
    s.undo(1).unwrap();
    assert_eq!(s.project().tracks.len(), before, "one step of the history");
    let _ = (keys, bass);
}

#[test]
fn each_track_gets_its_own_file_named_after_it() {
    let dir = Dir::new("each");
    let mut s = song_in(dir.path(), "Night");
    voice(&mut s, "Keys", 72, 1);
    let (bass, ..) = voice(&mut s, "Bass", 40, 1);
    let r = run(&mut s, "export.midi", json!({ "each": true }));
    let folder = dir.path().join("song").join("MIDI");
    assert_eq!(PathBuf::from(r.data["path"].as_str().unwrap()), folder);
    let files: Vec<PathBuf> = r.data["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| PathBuf::from(f["path"].as_str().unwrap()))
        .collect();
    assert_eq!(
        files,
        [folder.join("Night_Keys.mid"), folder.join("Night_Bass.mid")]
    );
    assert_eq!(r.data["files"][1]["strip"], bass.as_str());
    assert_eq!(names(&read(&files[0])), ["Keys"]);
    assert_eq!(names(&read(&files[1])), ["Bass"]);

    // --tracks picks; a track without notes to give is refused.
    let one = run(
        &mut s,
        "export.midi",
        json!({ "tracks": [bass], "out": dir.path().join("only.mid") }),
    );
    assert_eq!(names(&read(&dir.path().join("only.mid"))), ["Bass"]);
    assert_eq!(one.data["tracks"], 1);
    let vox = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Vox" })));
    let why = refused(&mut s, "export.midi", json!({ "tracks": [vox] }));
    assert!(
        why.contains("インストゥルメントトラックではありません"),
        "{why}"
    );
    let why = refused(
        &mut s,
        "export.midi",
        json!({ "out": dir.path().join("x.wav") }),
    );
    assert!(why.contains(".mid"), "{why}");
    let why = refused(
        &mut s,
        "export.midi",
        json!({ "each": true, "name": "{曲}" }),
    );
    assert!(why.contains("{トラック}"), "{why}");
}

#[test]
fn moving_a_note_makes_the_file_stale_and_a_knob_does_not() {
    let dir = Dir::new("stale");
    let mut s = song_in(dir.path(), "Night");
    let (keys, _, note) = voice(&mut s, "Keys", 72, 1);
    let (bass, ..) = voice(&mut s, "Bass", 40, 1);
    let r = run(&mut s, "export.midi", json!({ "each": true }));
    let job = r.data["record"].as_str().unwrap().to_string();
    let keys_file = PathBuf::from(r.data["files"][0]["path"].as_str().unwrap());

    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [keys], "db": -9.0 }),
    );
    let v = status(&mut s, &job);
    assert_eq!(v["stale"], false, "the volume is not in a MIDI file: {v}");

    run(&mut s, "note.move", json!({ "ids": [note], "by": 240 }));
    let v = status(&mut s, &job);
    assert_eq!(v["stale"], true, "{v}");
    assert_eq!(changed(&v), vec![keys.clone()]);
    assert!(file_stale(&v, &keys));
    assert!(!file_stale(&v, &bass));

    s.undo(1).unwrap();
    assert_eq!(status(&mut s, &job)["stale"], false);
    run(&mut s, "note.move", json!({ "ids": [note], "by": 240 }));

    // Written again under the same name; the old file goes to the bin and the row is replaced.
    let again = run(&mut s, "export.update", json!({ "job": job }));
    let new_job = again.data["record"].as_str().unwrap().to_string();
    assert_eq!(read(&keys_file).parts[0].notes[0].at, 240);
    assert!(dir.path().join("bin").join("Night_Keys.mid").is_file());
    let v = status(&mut s, &new_job);
    assert_eq!(v["stale"], false, "{v}");
    let listed = run(&mut s, "export.status", json!({})).data["jobs"].clone();
    assert!(
        !listed.as_array().unwrap().iter().any(|j| j["job"] == job),
        "the row is replaced: {listed}"
    );

    // A tempo change is in every file.
    run(&mut s, "tempo.set", json!({ "bpm": 90.0 }));
    let v = status(&mut s, &new_job);
    assert!(file_stale(&v, &keys) && file_stale(&v, &bass), "{v}");

    // A new instrument track is not in the files chosen before.
    s.undo(1).unwrap();
    voice(&mut s, "Pad", 60, 1);
    assert_eq!(status(&mut s, &new_job)["stale"], false);

    let why = refused(&mut s, "export.preview", json!({ "job": new_job }));
    assert!(why.contains("試聴できません"), "{why}");
}

#[test]
fn clips_give_their_range_and_come_back_at_its_head() {
    let dir = Dir::new("clips");
    let mut s = song_in(dir.path(), "Night");
    voice(&mut s, "Keys", 72, 1);
    let (bass, clip, _) = voice(&mut s, "Bass", 40, 3);
    let r = run(&mut s, "export.midi", json!({ "clips": [clip] }));
    assert_eq!(
        (r.data["from"].as_i64(), r.data["to"].as_i64()),
        (Some(7680), Some(11520))
    );
    let path = PathBuf::from(r.data["path"].as_str().unwrap());
    let song = read(&path);
    assert_eq!(names(&song), ["Bass"], "only the clip's track");
    assert_eq!(song.parts[0].notes[0].at, 0, "from the head of the range");

    let job = r.data["record"].as_str().unwrap().to_string();
    let before = s.project_arc();
    run(&mut s, "export.import", json!({ "job": job }));
    let added = s
        .project()
        .tracks
        .values()
        .find(|t| !before.tracks.contains_key(&t.id))
        .cloned()
        .unwrap();
    assert_eq!(
        added.name, "Bass 2",
        "named after the file's track, apart from the one there"
    );
    assert_eq!(
        added.clips[0].start, 7680,
        "at the head of the range written"
    );
    assert_eq!(added.clips[0].notes()[0].pitch, 40);

    let audio = id_of(&run(&mut s, "track.add.audio", json!({ "name": "Vox" })));
    let why = refused(&mut s, "export.midi", json!({ "tracks": [audio, bass] }));
    assert!(why.contains("Vox"), "{why}");
}
