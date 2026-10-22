//! `export.dawproject` (EXP-007): the whole song as a `.dawproject` another DAW opens, with the
//! audio inside by default, written next to the song unless told where. A file already there
//! goes to the bin of the test's own folder, and what the file could not carry is listed.
//! Nothing here opens an audio device.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};
use xirius_core::dawproject::{self, zip::ZipReader};
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_plugins::{Catalog, ScanOptions};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::write_wav;
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

const RATE: u32 = 48_000;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh folder under the temp dir, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-export-dawproject-test-{}-{n}-{tag}",
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

/// A song folder `<tmp>/song`, opened as the app opens it.
fn song_in(tmp: &Path, name: &str) -> Session {
    let dir = tmp.join("song");
    drop(Session::create(&dir, Project::new(name)).unwrap());
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach_with(&mut s, config(tmp), extras(tmp));
    s
}

/// A song never saved.
fn unsaved(tmp: &Path, name: &str) -> Session {
    let mut s = Session::with_registry(Project::new(name), shared_registry());
    commands::attach_with(&mut s, config(tmp), extras(tmp));
    s
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run_with(s: &mut Session, name: &str, args: Value, opts: &RunOptions) -> RunResult {
    commands::run(s, None, name, &map(args), opts)
        .unwrap_or_else(|e| panic!("{name}: {}", e.message))
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    run_with(s, name, args, &RunOptions::human())
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

/// One second of a quiet tone at `path`.
fn tone(path: &Path) {
    let x: Vec<f32> = (0..RATE as usize)
        .map(|i| 0.1 * (i as f32 / RATE as f32 * 220.0 * std::f32::consts::TAU).sin())
        .collect();
    write_wav(path, &x, &x, RATE, 32).unwrap();
}

/// An audio track "Vox" playing a tone from the first bar, and an instrument track "Keys" with
/// one note.
fn fill(s: &mut Session, tmp: &Path) {
    let file = tmp.join("tone.wav");
    tone(&file);
    let vox = id_of(&run(s, "track.add.audio", json!({ "name": "Vox" })));
    run(
        s,
        "clip.add.audio",
        json!({ "track": vox, "at": 0, "length": 1920, "file": file.to_string_lossy() }),
    );
    let keys = id_of(&run(s, "track.add.instrument", json!({ "name": "Keys" })));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({ "track": keys, "at": "1.1.0", "bars": 1.0 }),
    ));
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": "1.1.0", "pitch": 60, "length": 480, "velocity": 100 }),
    );
}

fn entries(path: &Path) -> Vec<String> {
    let f = std::fs::File::open(path).unwrap();
    ZipReader::new(std::io::BufReader::new(f))
        .unwrap()
        .names()
        .to_vec()
}

/// The files the audio clips of a song play.
fn audio_files(p: &Project) -> Vec<String> {
    p.tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter_map(|c| match &c.body {
            ClipBody::Audio { file, .. } => Some(file.clone()),
            ClipBody::Midi { .. } => None,
        })
        .collect()
}

#[test]
fn the_song_goes_next_to_itself_with_its_audio_inside() {
    let dir = Dir::new("next");
    let mut s = song_in(dir.path(), "Night Walk");
    fill(&mut s, dir.path());
    let want = dir.path().join("song").join("Night Walk.dawproject");

    let dry = run_with(
        &mut s,
        "export.dawproject",
        json!({}),
        &RunOptions::human().dry(),
    );
    assert_eq!(dry.data["path"], want.display().to_string());
    assert!(!want.exists(), "a dry run wrote the file");

    let r = run(&mut s, "export.dawproject", json!({}));
    assert_eq!(r.data["path"], want.display().to_string());
    assert_eq!(r.data["embed_audio"], true);
    assert!(r.data["skipped"].is_array(), "{}", r.data);
    assert!(want.is_file());
    let names = entries(&want);
    assert!(names.iter().any(|n| n == "project.xml"), "{names:?}");
    assert!(names.iter().any(|n| n == "metadata.xml"), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| n.starts_with("audio/")).count(),
        1,
        "{names:?}"
    );

    // Another DAW sees the same tracks, the note and the tone.
    let media = dir.path().join("media");
    let got = dawproject::import(&want, &media).unwrap();
    let mut tracks: Vec<String> = got
        .project
        .tracks
        .values()
        .map(|t| t.name.clone())
        .collect();
    tracks.sort();
    assert_eq!(tracks, ["Keys", "Master", "Vox"]);
    let files = audio_files(&got.project);
    assert_eq!(files.len(), 1);
    assert!(Path::new(&files[0]).starts_with(&media), "{files:?}");
    assert_eq!(
        std::fs::read(&files[0]).unwrap(),
        std::fs::read(dir.path().join("tone.wav")).unwrap()
    );
}

#[test]
fn a_file_of_the_same_name_goes_to_the_bin_first() {
    let dir = Dir::new("again");
    let mut s = song_in(dir.path(), "Night Walk");
    fill(&mut s, dir.path());
    let out = dir.path().join("out").join("Walk.dawproject");
    run(
        &mut s,
        "export.dawproject",
        json!({ "out": out.to_string_lossy() }),
    );
    let first = std::fs::read(&out).unwrap();
    run(&mut s, "track.add.audio", json!({ "name": "Choir" }));
    run(
        &mut s,
        "export.dawproject",
        json!({ "out": out.to_string_lossy() }),
    );
    let binned = dir.path().join("bin").join("Walk.dawproject");
    assert_eq!(std::fs::read(&binned).unwrap(), first);
    assert_ne!(std::fs::read(&out).unwrap(), first);
    assert!(!out.with_extension("dawproject.part").exists());
}

#[test]
fn external_audio_is_named_where_it_is() {
    let dir = Dir::new("external");
    let mut s = song_in(dir.path(), "Night Walk");
    fill(&mut s, dir.path());
    let out = dir.path().join("light.dawproject");
    let r = run(
        &mut s,
        "export.dawproject",
        json!({ "out": out.to_string_lossy(), "external": true }),
    );
    assert_eq!(r.data["embed_audio"], false);
    let names = entries(&out);
    assert!(!names.iter().any(|n| n.starts_with("audio/")), "{names:?}");
    let got = dawproject::import(&out, &dir.path().join("media")).unwrap();
    let files = audio_files(&got.project);
    assert_eq!(files.len(), 1);
    assert_eq!(
        std::fs::read(&files[0]).unwrap(),
        std::fs::read(dir.path().join("tone.wav")).unwrap()
    );
}

#[test]
fn a_song_not_saved_needs_a_place_and_gets_the_extension() {
    let dir = Dir::new("unsaved");
    let mut s = unsaved(dir.path(), "Sketch");
    fill(&mut s, dir.path());
    let why = refused(&mut s, "export.dawproject", json!({}));
    assert!(why.contains("--out"), "{why}");

    let out = dir.path().join("sketch");
    let r = run(
        &mut s,
        "export.dawproject",
        json!({ "out": out.to_string_lossy() }),
    );
    let want = dir.path().join("sketch.dawproject");
    assert_eq!(r.data["path"], want.display().to_string());
    assert!(want.is_file());
    assert!(r.message.contains("書き出しました"), "{}", r.message);
}
