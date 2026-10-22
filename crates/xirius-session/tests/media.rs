//! The song's media folder (BRW-004): recordings, bounces and freezes are written where the
//! song's settings say, a file read from outside a saved song is copied in when the song says
//! so, changing the settings moves nothing already written, and the takes of an older song in
//! `Recordings/` right under its folder are still the song's. The recordings come from the
//! extension's silent player, so no device is opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_session::commands::{self, shared_registry};
use xirius_session::export::write_wav;
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-media-test-{}-{n}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    commands::run(s, None, name, &map(args), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A song saved in `<dir>/song` whose recycle bin is `<dir>/trash`, playing into silence.
fn song(dir: &Dir) -> Session {
    let bin = dir.0.join("trash");
    let trash = Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    });
    let mut s = Session::with_registry(Project::new("media"), shared_registry());
    commands::attach_with(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: true },
            ..ExtConfig::default()
        },
        ExtExtras {
            trash: Some(trash),
            ..ExtExtras::default()
        },
    );
    s.save_as(dir.0.join("song")).unwrap();
    s
}

fn text(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Half a second of a quiet tone.
fn tone(path: &Path) {
    let x: Vec<f32> = (0..24_000).map(|i| 0.1 * (i as f32 * 0.03).sin()).collect();
    write_wav(path, &x, &x, 48_000, 32).unwrap();
}

fn audio_track(s: &mut Session, name: &str) -> String {
    run(s, "track.add.audio", json!({ "name": name })).data["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Records a moment on the armed tracks and gives the take's file as the clip names it.
fn record(s: &mut Session) -> String {
    run(s, "record.start", json!({ "at": "1.1.0" }));
    let begun = Instant::now();
    loop {
        let st = run(s, "transport.status", json!({}));
        if st.data["frame"].as_u64().unwrap_or(0) >= 7_200 {
            break;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{:?}", st.data);
        std::thread::sleep(Duration::from_millis(10));
    }
    let r = run(s, "record.stop", json!({}));
    r.data["takes"][0]["file"]
        .as_str()
        .unwrap_or_else(|| panic!("no take in {:?}", r.data))
        .to_string()
}

fn files(s: &Session) -> Vec<String> {
    s.project()
        .tracks
        .values()
        .flat_map(|t| &t.clips)
        .filter_map(|c| match &c.body {
            ClipBody::Audio { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn takes_go_where_the_song_says_and_a_change_moves_none() {
    let dir = Dir::new("takes");
    let mut s = song(&dir);
    let root = s.dir().unwrap().to_path_buf();
    let vox = audio_track(&mut s, "Vox");
    run(&mut s, "track.arm", json!({ "ids": [vox], "on": true }));

    // By default, under Media/Recordings.
    let first = record(&mut s);
    assert!(first.starts_with("Media/Recordings/"), "{first}");
    assert!(root.join(&first).is_file());

    // A folder of the song's own: the next take goes there, the first stays.
    run(
        &mut s,
        "project.settings",
        json!({ "media_folder": "Takes" }),
    );
    let second = record(&mut s);
    assert!(second.starts_with("Takes/Recordings/"), "{second}");
    assert!(root.join(&second).is_file());
    assert!(root.join(&first).is_file(), "nothing written moves");

    // A folder outside the song: the clip names it by its full path.
    let shared = dir.0.join("shared");
    run(
        &mut s,
        "project.settings",
        json!({ "media_folder": text(&shared) }),
    );
    let third = record(&mut s);
    assert!(Path::new(&third).is_absolute(), "{third}");
    assert!(
        Path::new(&third).starts_with(shared.join("Recordings")),
        "{third}"
    );
    assert!(Path::new(&third).is_file());
}

#[test]
fn bounces_and_freezes_go_where_the_song_says() {
    let dir = Dir::new("bounce");
    let mut s = song(&dir);
    let root = s.dir().unwrap().to_path_buf();
    let src = dir.0.join("a.wav");
    tone(&src);
    run(
        &mut s,
        "project.settings",
        json!({ "media_folder": "Files" }),
    );
    let t = audio_track(&mut s, "A");
    let a = run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 1920, "file": text(&src)}),
    )
    .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "clip.bounce", json!({ "ids": [a] }));
    let bounced = files(&s).remove(0);
    assert!(bounced.starts_with("Files/Bounces/"), "{bounced}");
    assert!(root.join(&bounced).is_file());

    run(&mut s, "track.freeze", json!({ "ids": [t] }));
    let frozen = s.project().tracks.values().find_map(|t| t.frozen.clone());
    let frozen = frozen.expect("frozen").file;
    assert!(frozen.starts_with("Files/Freeze/"), "{frozen}");
    assert!(root.join(&frozen).is_file());
    assert!(!root.join("Media").exists());
}

#[test]
fn reading_a_file_into_a_saved_song_copies_it_when_the_song_says_so() {
    let dir = Dir::new("import");
    let outside = dir.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let kick = outside.join("Kick.wav");
    tone(&kick);

    // A song not saved yet has no folder: the file is named where it is.
    let mut unsaved = Session::with_registry(Project::new("new"), shared_registry());
    commands::attach(&mut unsaved, ExtConfig::default());
    run(&mut unsaved, "clip.import", json!({ "file": text(&kick) }));
    assert_eq!(Path::new(&files(&unsaved)[0]), kick);

    let mut s = song(&dir);
    let root = s.dir().unwrap().to_path_buf();
    // A dry run copies nothing.
    let r = commands::run(
        &mut s,
        None,
        "clip.import",
        &map(json!({ "file": text(&kick) })),
        &RunOptions {
            dry_run: true,
            ..RunOptions::human()
        },
    )
    .unwrap();
    assert!(!r.message.contains("コピー"), "{}", r.message);
    assert!(!root.join("Media").exists());

    let r = run(&mut s, "clip.import", json!({ "file": text(&kick) }));
    assert!(r.message.contains("コピーしました"), "{}", r.message);
    assert_eq!(r.data["copied"], true);
    assert_eq!(files(&s), ["Media/Imports/Kick.wav"]);
    assert!(root.join("Media/Imports/Kick.wav").is_file() && kick.is_file());
    // The same file again is the same copy.
    run(&mut s, "clip.import", json!({ "file": text(&kick) }));
    assert_eq!(
        std::fs::read_dir(root.join("Media/Imports"))
            .unwrap()
            .count(),
        1
    );
    // Undone, the copy stays (a redo needs it).
    s.undo(1).unwrap();
    assert!(root.join("Media/Imports/Kick.wav").is_file());

    // Turned off, the file is named where it is.
    run(&mut s, "project.settings", json!({ "copy_imports": false }));
    let r = run(&mut s, "clip.import", json!({ "file": text(&kick) }));
    assert_eq!(r.data["copied"], false);
    assert!(
        files(&s).iter().any(|f| Path::new(f) == kick),
        "{:?}",
        files(&s)
    );
}

#[test]
fn cleanup_finds_the_takes_of_both_folders() {
    let dir = Dir::new("cleanup");
    let mut s = song(&dir);
    let root = s.dir().unwrap().to_path_buf();
    for folder in ["Recordings", "Media/Recordings"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        tone(&root.join(folder).join("stray.wav"));
    }
    let r = run(&mut s, "project.cleanup", json!({ "dry_run": true }));
    let listed: Vec<&str> = r.data["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        listed,
        ["Media/Recordings/stray.wav", "Recordings/stray.wav"]
    );
    let r = run(&mut s, "project.cleanup", json!({}));
    assert_eq!(r.data["moved"].as_array().unwrap().len(), 2);
}
