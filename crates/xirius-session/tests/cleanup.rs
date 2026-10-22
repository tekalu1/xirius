//! `project.cleanup` (REC-016, CTX-009): a recording an undo took out stays while a redo can
//! bring it back, and goes to the recycle bin once no state of the song names it, neither the
//! song, the history, the saved song nor a version. Anything it cannot read, an open recording
//! or an AI's work copy stops it before a file is moved. The recycle bin here is a folder; the
//! recordings come from the extension's silent player, so no device is opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_core::{AiPermission, CmdError};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
use xirius_session::output::OutputMode;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!(
            "xirius-cleanup-test-{}-{n}-{tag}",
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

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    commands::run(s, None, name, &map(args), &RunOptions::human())
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    try_run(s, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

/// A song saved in `<dir>/song` whose recycle bin is `<dir>/trash`, playing into silence.
fn song(dir: &Dir) -> Session {
    let bin = dir.0.join("trash");
    let trash = Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    });
    let mut s = Session::with_registry(Project::new("clean"), shared_registry());
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

/// Records a moment on the armed tracks: one step, one file per track.
fn record(s: &mut Session) -> PathBuf {
    run(s, "record.start", json!({ "at": "1.1.0" }));
    played(s, 7_200);
    let r = run(s, "record.stop", json!({}));
    let file = r.data["takes"][0]["file"]
        .as_str()
        .unwrap_or_else(|| panic!("no take in {:?}", r.data));
    s.dir().unwrap().join(file)
}

/// Waits until the player is `frames` in. The silent output does not make up for time its
/// thread did not get (a busy machine), so a wait for a time may find nothing recorded.
fn played(s: &mut Session, frames: u64) {
    let begun = Instant::now();
    loop {
        let st = run(s, "transport.status", json!({}));
        if st.data["frame"].as_u64().unwrap_or(0) >= frames {
            return;
        }
        assert!(begun.elapsed() < Duration::from_secs(20), "{:?}", st.data);
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn files_in_use(s: &Session) -> Vec<String> {
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

fn listed(r: &RunResult, key: &str) -> Vec<String> {
    r.data[key]
        .as_array()
        .unwrap_or_else(|| panic!("no {key} in {:?}", r.data))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn dry(s: &mut Session) -> RunResult {
    run(s, "project.cleanup", json!({ "dry_run": true }))
}

#[test]
fn only_files_no_state_names_go_and_redo_keeps_its_own() {
    let dir = Dir::new("names");
    let mut s = song(&dir);
    let t = run(&mut s, "track.add.audio", json!({ "name": "Vox" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "track.arm", json!({ "ids": [t], "on": true }));
    let one = record(&mut s);
    let two = record(&mut s);
    assert_eq!(files_in_use(&s).len(), 2);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(
        files_in_use(&s).len(),
        1,
        "the undo took the second take out"
    );

    // Files no state names: left over from before, in a subfolder too. Others are not touched.
    let rec = s.dir().unwrap().join("Recordings");
    std::fs::create_dir_all(rec.join("old")).unwrap();
    std::fs::copy(&one, rec.join("stray.wav")).unwrap();
    std::fs::copy(&one, rec.join("old").join("Older.WAV")).unwrap();
    std::fs::write(rec.join("notes.txt"), "not a recording").unwrap();

    // The dry run lists them and moves nothing; the take only a redo reaches is kept.
    let r = dry(&mut s);
    assert!(r.dry_run);
    assert_eq!(
        listed(&r, "files"),
        ["Recordings/old/Older.WAV", "Recordings/stray.wav"]
    );
    assert_eq!(r.data["kept"], 2, "{:?}", r.data);
    assert!(listed(&r, "moved").is_empty());
    assert!(rec.join("stray.wav").exists());
    assert!(r.message.contains("2 個"), "{}", r.message);

    let r = run(&mut s, "project.cleanup", json!({}));
    assert_eq!(listed(&r, "moved").len(), 2, "{}", r.message);
    assert!(r.message.contains("ごみ箱に移動しました"), "{}", r.message);
    let bin = dir.0.join("trash");
    assert!(bin.join("stray.wav").exists() && bin.join("Older.WAV").exists());
    assert!(!rec.join("stray.wav").exists());
    assert!(one.exists() && two.exists() && rec.join("notes.txt").exists());
    assert!(listed(&dry(&mut s), "files").is_empty(), "nothing left");

    // The redo finds its file.
    run(&mut s, "history.redo", json!({}));
    assert_eq!(files_in_use(&s).len(), 2);
    assert!(two.exists());

    // A version keeps what it names after the redo is gone.
    run(&mut s, "version.save", json!({ "name": "both takes" }));
    run(&mut s, "history.undo", json!({}));
    run(&mut s, "track.add.audio", json!({ "name": "Gtr" }));
    assert!(!s.can_redo());
    assert!(listed(&dry(&mut s), "files").is_empty());

    // A take nothing names any more: undone, its redo gone, in no version.
    let three = record(&mut s);
    run(&mut s, "history.undo", json!({}));
    assert!(
        listed(&dry(&mut s), "files").is_empty(),
        "the redo needs it"
    );
    run(&mut s, "track.add.audio", json!({ "name": "Bass" }));
    let r = run(&mut s, "project.cleanup", json!({}));
    assert_eq!(
        listed(&r, "moved"),
        [format!(
            "Media/Recordings/{}",
            three.file_name().unwrap().to_string_lossy()
        )]
    );
    assert!(!three.exists() && one.exists() && two.exists());
}

#[test]
fn what_cannot_be_read_stops_it_before_a_file_moves() {
    let dir = Dir::new("unreadable");
    let mut s = song(&dir);
    run(&mut s, "track.add.audio", json!({ "name": "Vox" }));
    let rec = s.dir().unwrap().join("Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    std::fs::write(rec.join("stray.wav"), b"RIFF").unwrap();
    let first = std::fs::read_dir(s.dir().unwrap().join("history"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap() != "index.json")
        .unwrap();
    std::fs::write(&first, "{ broken").unwrap();
    let e = try_run(&mut s, "project.cleanup", json!({})).unwrap_err();
    assert!(e.message.contains("何も移動しません"), "{}", e.message);
    assert!(rec.join("stray.wav").exists());
}

#[test]
fn a_recording_or_an_ai_work_copy_holds_it_off() {
    let dir = Dir::new("held");
    let mut s = song(&dir);
    let t = run(&mut s, "track.add.audio", json!({ "name": "Vox" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "track.arm", json!({ "ids": [t], "on": true }));
    run(&mut s, "record.start", json!({}));
    let e = try_run(&mut s, "project.cleanup", json!({})).unwrap_err();
    assert!(e.message.contains("録音中"), "{}", e.message);
    run(&mut s, "record.stop", json!({}));

    s.set_ai_permission(AiPermission::Edit).unwrap();
    commands::run(
        &mut s,
        None,
        "ai.request.begin",
        &map(json!({ "title": "copy", "draft": true })),
        &RunOptions::ai("bot"),
    )
    .unwrap();
    let e = try_run(&mut s, "project.cleanup", json!({})).unwrap_err();
    assert!(e.message.contains("作業コピー"), "{}", e.message);

    // An unsaved song has no recordings to look at.
    let mut fresh = Session::with_registry(Project::new("new"), shared_registry());
    commands::attach(&mut fresh, ExtConfig::default());
    let r = run(&mut fresh, "project.cleanup", json!({}));
    assert!(listed(&r, "files").is_empty(), "{}", r.message);
}
