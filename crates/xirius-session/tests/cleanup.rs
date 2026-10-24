//! `project.cleanup` (REC-016, CTX-009): a recording an undo took out stays while a redo can
//! bring it back, and goes to the recycle bin once no state of the song names it, neither the
//! song, the history, the saved song nor a version. Anything it cannot read, an open recording
//! or an AI's work copy stops it before a file is moved. The recycle bin here is a folder; the
//! recordings come from the extension's silent player, so no device is opened.
//!
//! The cleanup of a closed song (REC-026) opens no file of a song with no recording, and of one
//! with recordings each state and version once, none of the blobs they point to: the files
//! opened are counted, each of which may take seconds on a cloud drive.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use xirius_core::command::Output;
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_core::store::{self, Store};
use xirius_core::{AiPermission, CmdError, CmdResult};
use xirius_session::cleanup::{self, Closed, Trash};
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
    let mut s = Session::with_registry(Project::new("clean"), shared_registry());
    attach(&mut s, dir);
    s.save_as(dir.0.join("song")).unwrap();
    s
}

/// [`song`] opened again.
fn reopened(dir: &Dir) -> Session {
    let mut s = Session::open_with(dir.0.join("song"), shared_registry()).unwrap();
    attach(&mut s, dir);
    s
}

fn attach(s: &mut Session, dir: &Dir) {
    let trash = bin_of(dir);
    commands::attach_with(
        s,
        ExtConfig {
            output: OutputMode::Silent { paced: true },
            ..ExtConfig::default()
        },
        ExtExtras {
            trash: Some(trash),
            ..ExtExtras::default()
        },
    );
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

/// The closed song's recycle bin, as [`song`]'s.
fn bin_of(dir: &Dir) -> Trash {
    let bin = dir.0.join("trash");
    Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    })
}

/// A plugin whose state is long enough to be kept in `history/blobs/`, renamed a few times
/// (each state points to the blob), and a version: what made each closing read minutes on a
/// cloud drive (REC-026).
fn with_blobs(s: &mut Session) {
    let t = run(s, "track.add.audio", json!({ "name": "Keys" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    let plugin = run(
        s,
        "plugin.add",
        json!({ "track": t, "name": "Sampler", "format": "vst3", "uid": "vendor/sampler" }),
    )
    .data["id"]
        .clone();
    run(
        s,
        "plugin.state.set",
        json!({ "id": plugin, "state": "AAEC".repeat(75_000) }),
    );
    for i in 0..4 {
        run(
            s,
            "track.rename",
            json!({ "id": t, "name": format!("Keys {i}") }),
        );
    }
    run(s, "version.save", json!({ "name": "keys" }));
    let blobs = s.dir().unwrap().join("history").join("blobs");
    assert!(std::fs::read_dir(&blobs).unwrap().count() > 0, "a blob");
}

/// The song in `s` closed: what its cleanup runs on, once the session is gone.
fn closed(s: Session, dir: &Dir) -> Closed {
    let c = Closed::new(
        Arc::new(s.project().clone()),
        s.dir().unwrap().to_path_buf(),
        bin_of(dir),
    );
    drop(s);
    c
}

fn moved(r: &CmdResult<Output>) -> usize {
    match r {
        Ok(out) => out.data["moved"].as_array().map_or(0, Vec::len),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn a_song_with_no_recordings_reads_none_of_its_history() {
    let dir = Dir::new("none");
    let mut s = song(&dir);
    with_blobs(&mut s);
    let c = closed(s, &dir);

    let before = store::files_read();
    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 0);
    assert_eq!(
        store::files_read() - before,
        0,
        "no file of the song opened"
    );
    // An empty take folder is the same.
    std::fs::create_dir_all(c.dir().join("Media").join("Recordings")).unwrap();
    let before = store::files_read();
    assert_eq!(moved(&c.run(&AtomicBool::new(false))), 0);
    assert_eq!(store::files_read() - before, 0);
}

#[test]
fn a_song_with_recordings_reads_each_state_and_version_once_and_no_blob() {
    let dir = Dir::new("some");
    let mut s = song(&dir);
    with_blobs(&mut s);
    let t = run(&mut s, "track.add.audio", json!({ "name": "Vox" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "track.arm", json!({ "ids": [t], "on": true }));
    let kept = record(&mut s);
    // Undone, it can come back with redo: it stays.
    let redo = record(&mut s);
    run(&mut s, "history.undo", json!({}));
    run(&mut s, "version.save", json!({ "name": "one take" }));
    let stray = s.dir().unwrap().join("Media/Recordings/stray.wav");
    std::fs::copy(&kept, &stray).unwrap();
    let c = closed(s, &dir);

    // What it has to read: each lane's index and states, the saved song, the list of
    // versions and each version, once.
    let all = Store::new(c.dir());
    let mut files = 2 + all.list_versions().unwrap().len() as u64;
    for lane in all.lanes() {
        let idx = Store::in_lane(c.dir(), lane).read_index().unwrap().unwrap();
        files += 1 + idx.steps.len() as u64 + 1;
    }
    let before = store::files_read();
    let r = c.run(&AtomicBool::new(false));
    assert_eq!(
        store::files_read() - before,
        files,
        "no blob, nothing twice"
    );
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(!stray.exists() && dir.0.join("trash/stray.wav").exists());
    assert!(kept.exists() && redo.exists(), "the take and its redo stay");
}

/// Each audio clip's `file` in the JSON `v` that is `from`, as `to`.
fn name_files(v: &mut Value, from: &str, to: &Value) {
    match v {
        Value::Object(o) => {
            for (k, val) in o.iter_mut() {
                if k == "file" && val.as_str() == Some(from) {
                    *val = to.clone();
                } else {
                    name_files(val, from, to);
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| name_files(x, from, to)),
        _ => {}
    }
}

#[test]
fn a_file_a_state_names_through_a_blob_is_read_from_the_blob_and_stays() {
    let dir = Dir::new("blob-name");
    let mut s = song(&dir);
    let t = run(&mut s, "track.add.audio", json!({ "name": "Vox" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "track.arm", json!({ "ids": [t], "on": true }));
    let take = record(&mut s);
    let song_dir = s.dir().unwrap().to_path_buf();
    let rel = |p: &Path| {
        p.strip_prefix(&song_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/")
    };
    let named = take.with_file_name("in-a-blob.wav");
    let stray = take.with_file_name("stray.wav");
    std::fs::copy(&take, &named).unwrap();
    std::fs::copy(&take, &stray).unwrap();
    let c = closed(s, &dir);

    // A state written as by hand: its clip names the file in a blob, as a long path is kept.
    let history = song_dir.join("history");
    let idx = Store::new(&song_dir).read_index().unwrap().unwrap();
    let last = history.join(format!("{}.json", idx.first + idx.steps.len() as u64));
    let mut state: Value = serde_json::from_slice(&std::fs::read(&last).unwrap()).unwrap();
    assert_eq!(state["format_version"], store::HISTORY_FORMAT_VERSION);
    let text = rel(&named);
    let hash = store::sha256_hex(text.as_bytes());
    std::fs::create_dir_all(history.join("blobs")).unwrap();
    std::fs::write(history.join("blobs").join(&hash), &text).unwrap();
    let before = state.to_string();
    name_files(&mut state, &rel(&take), &json!({ "$blob": hash }));
    assert_ne!(state.to_string(), before, "the take's clip in the state");
    std::fs::write(&last, state.to_string()).unwrap();

    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(named.exists(), "the file the blob names stays");
    assert!(!stray.exists());
    assert!(take.exists(), "the song names it");
}

#[test]
fn a_closed_song_is_left_while_it_is_open_or_once_it_is_told_to_stop() {
    let dir = Dir::new("stop");
    let s = song(&dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    std::fs::write(rec.join("stray.wav"), b"RIFF").unwrap();
    let c = closed(s, &dir);

    // Opened again (here, or by another process): nothing moves.
    let again = Session::open(c.dir()).unwrap();
    let e = c.run(&AtomicBool::new(false)).unwrap_err();
    assert!(e.message.contains(cleanup::OTHERS_OPEN), "{e:?}");
    assert!(rec.join("stray.wav").exists());
    drop(again);

    let e = c.run(&AtomicBool::new(true)).unwrap_err();
    assert!(e.message.contains(cleanup::STOPPED), "{e:?}");
    assert!(rec.join("stray.wav").exists());

    assert_eq!(moved(&c.run(&AtomicBool::new(false))), 1);
    assert!(!rec.join("stray.wav").exists());
}

/// A recycle bin that holds each file until `open` is set, as a slow drive would; `asked`
/// counts the files it was given.
fn slow_bin_of(dir: &Dir, open: &Arc<AtomicBool>, asked: &Arc<AtomicU32>) -> Trash {
    let fast = bin_of(dir);
    let (open, asked) = (open.clone(), asked.clone());
    Arc::new(move |p: &Path| {
        asked.fetch_add(1, Ordering::SeqCst);
        while !open.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        fast(p)
    })
}

/// A closed song with two recordings nothing names, its cleanup running on a thread and held
/// at the first file: the song's take folder and the cleanup.
fn held(
    dir: &Dir,
    open: &Arc<AtomicBool>,
) -> (PathBuf, std::thread::JoinHandle<CmdResult<Output>>) {
    let s = song(dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    for name in ["stray-1.wav", "stray-2.wav"] {
        a_second_of_silence(&rec.join(name));
    }
    let asked = Arc::new(AtomicU32::new(0));
    let c = Closed::new(
        Arc::new(s.project().clone()),
        s.dir().unwrap().to_path_buf(),
        slow_bin_of(dir, open, &asked),
    );
    drop(s);
    let cleanup = std::thread::spawn(move || c.run(&AtomicBool::new(false)));
    let t = Instant::now();
    while asked.load(Ordering::SeqCst) == 0 {
        assert!(t.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(5));
    }
    (rec, cleanup)
}

fn a_second_of_silence(path: &Path) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..48000 {
        w.write_sample(0i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Lets the slow bin go after a moment, from another thread.
fn release_soon(open: &Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    let open = open.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        open.store(true, Ordering::SeqCst);
    })
}

#[test]
fn a_file_another_open_song_names_or_can_come_back_to_stays_while_the_cleanup_runs() {
    let dir = Dir::new("named");
    let open = Arc::new(AtomicBool::new(false));
    let (rec, cleanup) = held(&dir, &open);

    // Another song, not saved, is given the second file while the first moves, and takes it
    // out again: an undo can bring it back.
    let mut other = Session::with_registry(Project::new("other"), shared_registry());
    commands::attach_with(&mut other, ExtConfig::default(), ExtExtras::default());
    let release = release_soon(&open);
    let file = rec.join("stray-2.wav");
    run(
        &mut other,
        "clip.import",
        json!({ "file": file.to_string_lossy(), "new-track": true }),
    );
    run(&mut other, "history.undo", json!({}));
    release.join().unwrap();
    let r = cleanup.join().unwrap();
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(
        file.exists(),
        "the file the other song can come back to stays"
    );
    assert!(!rec.join("stray-1.wav").exists());
    drop(other);
}

#[test]
fn a_closed_song_opened_again_while_its_cleanup_moves_files_keeps_the_rest() {
    let dir = Dir::new("again");
    let open = Arc::new(AtomicBool::new(false));
    let (rec, cleanup) = held(&dir, &open);

    // Opened again (here, or by another process) while the first file moves: the opening does
    // not wait for that one, and what the cleanup had still to move stays.
    let release = release_soon(&open);
    let again = Session::open(dir.0.join("song")).unwrap();
    assert!(
        !open.load(Ordering::SeqCst),
        "it opened while the file was being moved"
    );
    release.join().unwrap();
    let r = cleanup.join().unwrap();
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(rec.join("stray-2.wav").exists());
    drop(again);
}

/// A song not saved, which names nothing yet.
fn unsaved() -> Session {
    let mut s = Session::with_registry(Project::new("other"), shared_registry());
    commands::attach_with(&mut s, ExtConfig::default(), ExtExtras::default());
    s
}

#[test]
fn a_command_does_not_wait_for_the_file_the_cleanup_moves() {
    let dir = Dir::new("edit");
    let open = Arc::new(AtomicBool::new(false));
    let (rec, cleanup) = held(&dir, &open);

    // The recycle bin is slow (a cloud drive): the other song's edits go on meanwhile.
    let mut other = unsaved();
    let release = std::thread::spawn({
        let open = open.clone();
        move || {
            std::thread::sleep(Duration::from_secs(2));
            open.store(true, Ordering::SeqCst);
        }
    });
    let t = Instant::now();
    run(&mut other, "track.add.audio", json!({ "name": "Vox" }));
    run(&mut other, "history.undo", json!({}));
    let took = t.elapsed();
    assert!(
        took < Duration::from_secs(1) && !open.load(Ordering::SeqCst),
        "{took:?}"
    );
    release.join().unwrap();
    let r = cleanup.join().unwrap();
    assert_eq!(moved(&r), 2, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(!rec.join("stray-1.wav").exists() && !rec.join("stray-2.wav").exists());
    assert!(!rec.join(".xirius-moving").exists(), "nothing left aside");
}

/// `link` made to stand for the folder `to`: a junction on Windows, a symbolic link elsewhere.
fn link_folder(link: &Path, to: &Path) {
    #[cfg(windows)]
    {
        // With `\` only: `cmd` reads a `/` in a path as a switch.
        let plain = |p: &Path| p.components().collect::<PathBuf>();
        let made = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(plain(link))
            .arg(plain(to))
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(to, link).unwrap();
}

#[test]
fn a_file_another_song_names_through_a_link_to_its_folder_stays() {
    let dir = Dir::new("link");
    let s = song(&dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    a_second_of_silence(&rec.join("Take-2.wav"));
    let c = closed(s, &dir);

    // The other song names the take through a link to the take folder: written another way,
    // the same file.
    let link = dir.0.join("link");
    link_folder(&link, &rec);
    let mut other = unsaved();
    let through = link.join("Take-2.wav");
    run(
        &mut other,
        "clip.import",
        json!({ "file": through.to_string_lossy(), "new-track": true }),
    );
    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 0, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(rec.join("Take-2.wav").exists());

    // Once the other song goes, it does.
    drop(other);
    assert_eq!(moved(&c.run(&AtomicBool::new(false))), 1);
    #[cfg(windows)]
    std::fs::remove_dir(&link).unwrap();
    #[cfg(unix)]
    std::fs::remove_file(&link).unwrap();
}

#[test]
fn a_file_another_process_names_in_the_song_while_the_cleanup_runs_stays() {
    let dir = Dir::new("written");
    let open = Arc::new(AtomicBool::new(false));
    let (rec, cleanup) = held(&dir, &open);
    let song_dir = dir.0.join("song");

    // Another process opens the song, puts the second file in, saves and closes it, while the
    // first file moves: what the cleanup read before does not name it.
    let mut other = unsaved();
    let file = rec.join("stray-2.wav");
    run(
        &mut other,
        "clip.import",
        json!({ "file": file.to_string_lossy(), "new-track": true }),
    );
    let p = other.project().clone();
    drop(other);
    Store::new(&song_dir).write_project(&p).unwrap();
    open.store(true, Ordering::SeqCst);
    let r = cleanup.join().unwrap();
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(file.exists(), "the song names it now");
    assert!(!rec.join("stray-1.wav").exists());
}

#[test]
fn another_process_is_looked_for_in_the_lock_files_read_once() {
    let dir = Dir::new("locks");
    let s = song(&dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    for i in 0..3 {
        std::fs::write(rec.join(format!("locks-stray-{i}.wav")), b"RIFF").unwrap();
    }
    let c = closed(s, &dir);
    let locks = xirius_core::session::lock_files(c.dir()).len() as u64;
    assert!(locks > 0, "the song's own");

    // Before it begins, and before each file moves: the files there are, not each lane's.
    let before = xirius_core::session::locks_tried();
    assert_eq!(moved(&c.run(&AtomicBool::new(false))), 3);
    assert_eq!(
        xirius_core::session::locks_tried() - before,
        (1 + 3) * locks
    );
}

/// An armed audio track `Vox` in `s`: its id.
fn armed_vox(s: &mut Session) -> String {
    let t = run(s, "track.add.audio", json!({ "name": "Vox" })).data["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(s, "track.arm", json!({ "ids": [t], "on": true }));
    t
}

/// The folder in `rec`'s `.xirius-moving/` the process `name` puts files aside in, with its
/// lock file: held while the file is, as by a process that runs.
fn aside_folder(rec: &Path, name: &str) -> (PathBuf, std::fs::File) {
    let folder = rec.join(".xirius-moving").join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let lock = std::fs::File::create(folder.join("held.lock")).unwrap();
    (folder, lock)
}

#[test]
fn a_file_a_cleanup_left_aside_goes_back_where_it_was() {
    let dir = Dir::new("aside");
    let mut s = song(&dir);
    armed_vox(&mut s);
    let take = record(&mut s);
    let rec = take.parent().unwrap().to_path_buf();
    // Left as a cleanup the end of its process stopped would: one the song names, one not.
    let (gone, lock) = aside_folder(&rec, "1-gone");
    drop(lock);
    std::fs::create_dir_all(gone.join("2")).unwrap();
    std::fs::rename(&take, gone.join(take.file_name().unwrap())).unwrap();
    std::fs::write(gone.join("2/stray.wav"), b"RIFF").unwrap();
    let c = closed(s, &dir);

    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 1, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(take.exists(), "back where the song names it");
    assert!(!rec.join("stray.wav").exists() && dir.0.join("trash/stray.wav").exists());
    assert!(!rec.join(".xirius-moving").exists(), "nothing left aside");
}

#[test]
fn what_another_process_has_aside_is_left_to_it_and_a_listing_moves_nothing() {
    let dir = Dir::new("theirs");
    let s = song(&dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    std::fs::create_dir_all(&rec).unwrap();
    std::fs::write(rec.join("stray.wav"), b"RIFF").unwrap();
    // Another process has a file aside now, as it moves it to the recycle bin; one that went
    // down left another.
    let (theirs, lock) = aside_folder(&rec, "1-runs");
    lock.try_lock().unwrap();
    std::fs::write(theirs.join("Other-process-only-1.wav"), b"RIFF").unwrap();
    let (gone, unheld) = aside_folder(&rec, "2-gone");
    drop(unheld);
    std::fs::write(gone.join("Other-process-only-2.wav"), b"RIFF").unwrap();
    let p = s.project().clone();
    let c = closed(s, &dir);

    // The cleanup window's list: the one left, where it is; nothing moves.
    let found = cleanup::unused(&p, c.dir(), cleanup::Own::default()).unwrap();
    assert_eq!(
        found.files,
        [
            "Media/Recordings/.xirius-moving/2-gone/Other-process-only-2.wav",
            "Media/Recordings/stray.wav"
        ]
    );
    assert!(
        theirs.join("Other-process-only-1.wav").exists()
            && gone.join("Other-process-only-2.wav").exists()
    );

    // The cleanup: the other process's file stays where it put it.
    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 2, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(theirs.join("Other-process-only-1.wav").exists());
    assert!(
        dir.0.join("trash/Other-process-only-2.wav").exists()
            && dir.0.join("trash/stray.wav").exists()
    );
    assert!(!gone.exists());

    // Once that process is gone too.
    drop(lock);
    assert_eq!(moved(&c.run(&AtomicBool::new(false))), 1);
    assert!(dir.0.join("trash/Other-process-only-1.wav").exists());
    assert!(!rec.join(".xirius-moving").exists());
}

#[test]
fn a_file_taken_back_from_the_recycle_bin_goes_back_where_it_was_and_stays_that_time() {
    let dir = Dir::new("restored");
    let s = song(&dir);
    let rec = s.dir().unwrap().join("Media/Recordings");
    // The bin gives a file back where it took it from: aside, in a folder no process holds.
    let from = rec.join(".xirius-moving/1-gone");
    std::fs::create_dir_all(&from).unwrap();
    // Other tests open songs with Vox-1.wav: changing alias candidates intentionally make
    // cleanup keep a file. This test is about restoration, so give it its own name.
    let name = "Restored-only-1.wav";
    std::fs::write(from.join(name), b"RIFF").unwrap();
    let c = closed(s, &dir);

    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 0, "{:?}", r.as_ref().map(|o| &o.data));
    assert!(rec.join(name).exists(), "where the browser shows it");
    assert!(!rec.join(".xirius-moving").exists());
    // Not named by the song the next time either: it goes then.
    let r = c.run(&AtomicBool::new(false));
    assert_eq!(moved(&r), 1, "second cleanup: {r:?}");
    assert!(dir.0.join("trash").join(name).exists());
}

#[test]
fn a_take_recorded_while_a_file_of_its_name_is_aside_keeps_both() {
    let dir = Dir::new("taken");
    let mut s = song(&dir);
    let t = armed_vox(&mut s);
    // Keep unrelated parallel tests' changing Vox alias candidates out of this scenario.
    run(
        &mut s,
        "track.rename",
        json!({ "id": t, "name": "Taken-only" }),
    );
    let first = record(&mut s);
    let rec = first.parent().unwrap().to_path_buf();
    // A file nothing names, with the name the next take would have.
    let name = first.file_name().unwrap().to_string_lossy().into_owned();
    let stray = rec.join(name.replace("-1.wav", "-2.wav"));
    assert_ne!(stray, first);
    std::fs::write(&stray, b"RIFF old").unwrap();
    s.save().unwrap();
    let (entered, at_bin) = std::sync::mpsc::sync_channel(1);
    let (release, proceed) = std::sync::mpsc::sync_channel(1);
    let proceed = std::sync::Mutex::new(proceed);
    let bin: Trash = Arc::new(move |_: &Path| {
        entered
            .send(())
            .expect("test left before the bin was reached");
        proceed
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(60))
            .expect("bin waiting for the new take to finish");
        Err("no bin".into())
    });
    let c = Closed::new(
        Arc::new(s.project().clone()),
        s.dir().unwrap().to_path_buf(),
        bin,
    );
    drop(s);
    let (finished, result) = std::sync::mpsc::sync_channel(1);
    let cleanup = std::thread::spawn(move || {
        let r = c.run(&AtomicBool::new(false));
        let _ = finished.send(r);
        drop(c); // Disconnect the stage signal after publishing an early cleanup result.
    });
    if let Err(e) = at_bin.recv_timeout(Duration::from_secs(60)) {
        panic!(
            "waiting for the file to be aside at the bin: {e}; cleanup: {:?}",
            result.try_recv()
        );
    }

    // Opened again while the file is aside, and recorded on: the cleanup puts it back.
    let mut again = reopened(&dir);
    run(&mut again, "track.arm", json!({ "ids": [t], "on": true }));
    let take = record(&mut again);
    release.send(()).unwrap();
    let e = result
        .recv_timeout(Duration::from_secs(60))
        .expect("cleanup did not finish putting the old file back after the bin failed")
        .expect_err("the bin took nothing");
    cleanup.join().unwrap();
    assert!(!e.message.contains("戻せません"), "{e:?}");
    assert!(std::fs::metadata(&take).unwrap().len() > 8, "the new take");
    assert_eq!(
        std::fs::read(&stray).ok().as_deref(),
        Some(&b"RIFF old"[..]),
        "the file back where it was, and the take {}",
        take.display()
    );
    drop(again);
}
