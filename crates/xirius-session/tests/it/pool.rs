//! The song's pool through a session (BRW-004): `file.list` gives each file's kind, size and
//! clips (unused takes too), `file.collect` copies the files from outside into the song,
//! `file.locate` points the clips of a lost file at where it is now, and `select.using_file`
//! picks the clips that use a file. Every file is in the test's own folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::id::{AnyId, ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Frozen, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_core::{CmdError, ErrorCode};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::output::OutputMode;

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

fn dry(s: &mut Session, name: &str, args: Value) -> RunResult {
    let o = RunOptions {
        dry_run: true,
        ..RunOptions::human()
    };
    commands::run(s, None, name, &map(args), &o).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn audio(id: u64, file: &str) -> Clip {
    Clip {
        id: ClipId(id),
        name: "take".into(),
        color: None,
        start: id as i64 * 960,
        length: 960,
        muted: false,
        take: None,
        controllers: Vec::new(),
        body: ClipBody::Audio {
            file: file.into(),
            offset: 0,
            gain_db: 0.0,
            process: Default::default(),
        },
    }
}

struct Song {
    root: tempfile::TempDir,
    s: Session,
}

impl Song {
    fn dir(&self) -> PathBuf {
        self.root.path().join("song")
    }
    fn outside(&self) -> PathBuf {
        self.root.path().join("outside")
    }
    /// The files the clips name, in the clips' order.
    fn files(&self) -> Vec<String> {
        let p = self.s.project();
        p.tracks[&TrackId(100)]
            .clips
            .iter()
            .filter_map(|c| match &c.body {
                ClipBody::Audio { file, .. } => Some(file.clone()),
                _ => None,
            })
            .collect()
    }
}

/// A saved song whose clips name the files `files` gives (a name starting with `>` is under the
/// test's `outside` folder); the second track is frozen to `Media/Freeze/B.wav`, which is
/// there (50 bytes long). `make` lists
/// the files made on the disk (under the song, or `>` under outside), each `bytes` long.
fn song(tag: &str, files: &[&str], make: &[(&str, usize)]) -> Song {
    let root = tempfile::Builder::new()
        .prefix(&format!("xirius-pool-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = root.path().join("song");
    let outside = root.path().join("outside");
    let real = |f: &str| match f.strip_prefix('>') {
        Some(rest) => outside.join(rest),
        None => dir.join(f),
    };
    let name = |f: &str| match f.strip_prefix('>') {
        Some(rest) => text(&outside.join(rest)),
        None => f.to_string(),
    };

    let mut p = Project::new("Pool");
    p.next_id = 1000;
    let mut a = Track::new(TrackId(100), TrackKind::Audio, "A");
    for (i, f) in files.iter().enumerate() {
        a.clips.push(audio(300 + i as u64, &name(f)));
    }
    let mut b = Track::new(TrackId(101), TrackKind::Audio, "B");
    b.frozen = Some(Frozen {
        file: "Media/Freeze/B.wav".into(),
        start: 0,
    });
    for t in [a, b] {
        p.root.push(Node::Track(t.id));
        p.tracks.insert(t.id, Arc::new(t));
    }
    drop(Session::create(&dir, p).unwrap());
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(dir.join("Media/Freeze")).unwrap();
    std::fs::write(dir.join("Media/Freeze/B.wav"), vec![7u8; 50]).unwrap();
    for (f, n) in make {
        let at = real(f);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, vec![7u8; *n]).unwrap();
    }
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            ..ExtConfig::default()
        },
    );
    Song { root, s }
}

#[test]
fn the_list_gives_each_files_kind_and_size_and_the_takes_no_clip_uses() {
    let mut t = song(
        "list",
        &[
            "Media/Recordings/take1.wav",
            "Media/Bounces/A.wav",
            ">Kick.wav",
        ],
        &[
            ("Media/Recordings/take1.wav", 10),
            ("Media/Recordings/take2.wav", 20),
            ("Recordings/old.wav", 30),
            ("Media/Bounces/A.wav", 40),
            ("Media/Freeze/B.wav", 50),
            (">Kick.wav", 60),
        ],
    );
    let r = run(&mut t.s, "file.list", json!({}));
    let files = r.data["files"].as_array().unwrap();
    let row = |name: &str| {
        files
            .iter()
            .find(|f| {
                f["file"]
                    .as_str()
                    .unwrap()
                    .replace('\\', "/")
                    .ends_with(name)
            })
            .unwrap_or_else(|| panic!("{name} in {files:?}"))
    };
    let take1 = row("Media/Recordings/take1.wav");
    assert_eq!(take1["kind"], "record");
    assert_eq!(take1["clips"], 1);
    assert_eq!(take1["bytes"], 10);
    // The takes no clip uses are listed with no clip, the older folder's too.
    let take2 = row("Media/Recordings/take2.wav");
    assert_eq!(take2["kind"], "record");
    assert_eq!(take2["clips"], 0);
    assert_eq!(take2["bytes"], 20);
    assert_eq!(row("Recordings/old.wav")["clips"], 0);
    assert_eq!(row("Media/Bounces/A.wav")["kind"], "bounce");
    let frozen = row("Media/Freeze/B.wav");
    assert_eq!(frozen["kind"], "freeze");
    assert_eq!(frozen["frozen"], 1);
    let kick = row("Kick.wav");
    assert_eq!(kick["kind"], "import");
    assert_eq!(kick["external"], true);
    assert_eq!(kick["bytes"], 60);
    assert_eq!(take1["external"], false);
    assert_eq!(files.len(), 6, "{files:?}");
    assert!(
        r.message.contains("使われていない録音 2 個"),
        "{}",
        r.message
    );
}

#[test]
fn collecting_copies_the_outside_files_in_as_one_step_and_an_undo_keeps_the_copies() {
    let mut t = song(
        "collect",
        &[
            "Media/Recordings/take1.wav",
            ">Kick.wav",
            ">Kick.wav",
            ">Gone.wav",
        ],
        &[("Media/Recordings/take1.wav", 10), (">Kick.wav", 60)],
    );
    let kick = text(&t.outside().join("Kick.wav"));
    let step = t.s.cursor();

    let r = dry(&mut t.s, "file.collect", json!({}));
    assert_eq!(r.data["files"], json!([kick]));
    assert!(
        !t.dir().join("Media/Imports").exists(),
        "a dry run copies nothing"
    );
    assert_eq!(t.s.cursor(), step);

    let r = run(&mut t.s, "file.collect", json!({}));
    assert!(
        r.message
            .contains("ファイル 1 個を曲のフォルダーにコピーしました"),
        "{}",
        r.message
    );
    assert!(r.message.contains("Gone.wav"), "{}", r.message);
    assert_eq!(
        std::fs::read(t.dir().join("Media/Imports/Kick.wav")).unwrap(),
        vec![7u8; 60]
    );
    let f = t.files();
    assert_eq!(f[1], "Media/Imports/Kick.wav");
    assert_eq!(f[2], "Media/Imports/Kick.wav");
    assert_eq!(t.s.cursor(), step + 1, "one step of the history");

    t.s.undo(1).unwrap();
    assert_eq!(t.files()[1], kick);
    assert!(t.dir().join("Media/Imports/Kick.wav").is_file());
    t.s.redo(1).unwrap();

    // Nothing is left outside but the lost file; one named that is inside is refused.
    let e = try_run(&mut t.s, "file.collect", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo, "{}", e.message);
    let e = try_run(
        &mut t.s,
        "file.collect",
        json!({ "paths": "Media/Recordings/take1.wav" }),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo, "{}", e.message);
    let gone = text(&t.outside().join("Gone.wav"));
    let e = try_run(&mut t.s, "file.collect", json!({ "paths": gone })).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "{}", e.message);
    let e = try_run(&mut t.s, "file.collect", json!({ "paths": "No/such.wav" })).unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "{}", e.message);
}

#[test]
fn collecting_names_files_one_by_one_and_a_song_not_saved_cannot() {
    let mut t = song(
        "collect-named",
        &[">a/Loop.wav", ">b/Loop.wav", ">Pad.wav"],
        &[(">a/Loop.wav", 1), (">b/Loop.wav", 2), (">Pad.wav", 3)],
    );
    let a = text(&t.outside().join("a/Loop.wav"));
    let b = text(&t.outside().join("b/Loop.wav"));
    run(
        &mut t.s,
        "file.collect",
        json!({ "paths": format!("{a}|{}", b.replace('\\', "/")) }),
    );
    let f = t.files();
    assert_eq!(f[0], "Media/Imports/Loop.wav");
    assert_eq!(f[1], "Media/Imports/Loop 2.wav");
    assert_eq!(
        f[2],
        text(&t.outside().join("Pad.wav")),
        "not named, not copied"
    );

    let mut unsaved = Session::with_registry(Project::new("new"), shared_registry());
    commands::attach(&mut unsaved, ExtConfig::default());
    let e = try_run(&mut unsaved, "file.collect", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "{}", e.message);
    assert!(e.message.contains("保存"), "{}", e.message);
}

#[test]
fn locating_one_lost_file_takes_the_others_of_its_folder_along() {
    let mut t = song(
        "locate-to",
        &[">old/a.wav", ">old/b.wav", ">old/c.wav", ">elsewhere/d.wav"],
        &[(">new/a.wav", 1), (">new/b.wav", 2), (">new/d.wav", 3)],
    );
    let new = t.outside().join("new");
    let step = t.s.cursor();
    let old_a = text(&t.outside().join("old/a.wav"));
    let r = dry(
        &mut t.s,
        "file.locate",
        json!({ "path": old_a, "new_path": text(&new.join("a.wav")) }),
    );
    assert_eq!(r.data["files"].as_array().unwrap().len(), 2);
    assert_eq!(t.s.cursor(), step);

    let r = run(
        &mut t.s,
        "file.locate",
        json!({ "path": old_a, "new_path": text(&new.join("a.wav")) }),
    );
    assert!(
        r.message.contains("ファイル 2 個の場所を付け替えました"),
        "{}",
        r.message
    );
    // c.wav is not beside a.wav, and d.wav was in another folder.
    assert!(r.message.contains("あと 2 個"), "{}", r.message);
    assert_eq!(r.data["missing"], 2);
    let f = t.files();
    assert_eq!(Path::new(&f[0]), new.join("a.wav"));
    assert_eq!(Path::new(&f[1]), new.join("b.wav"));
    assert_eq!(Path::new(&f[2]), t.outside().join("old/c.wav"));
    assert_eq!(Path::new(&f[3]), t.outside().join("elsewhere/d.wav"));
    assert_eq!(t.s.cursor(), step + 1);
    t.s.undo(1).unwrap();
    assert_eq!(t.files()[0], old_a);
}

#[test]
fn locating_by_a_folder_finds_the_lost_files_below_it_by_name() {
    let mut t = song(
        "locate-folder",
        &[">old/a.wav", ">old/sub/b.wav", ">old/c.wav"],
        &[
            (">moved/x/a.wav", 1),
            (">moved/y/B.WAV", 2),
            ("Media/Imports/c.wav", 3),
        ],
    );
    let moved = t.outside().join("moved");
    let r = run(&mut t.s, "file.locate", json!({ "folder": text(&moved) }));
    let f = t.files();
    assert_eq!(Path::new(&f[0]), moved.join("x/a.wav"));
    if cfg!(any(windows, target_os = "macos")) {
        assert_eq!(Path::new(&f[1]), moved.join("y/B.WAV"));
        assert_eq!(r.data["missing"], 1);
    }
    // A relative folder is the song's; inside it the clip is named relative.
    let c = text(&t.outside().join("old/c.wav"));
    run(
        &mut t.s,
        "file.locate",
        json!({ "folder": "Media", "path": c }),
    );
    assert_eq!(t.files()[2], "Media/Imports/c.wav");

    for args in [
        json!({}),
        json!({ "new_path": "a", "folder": "b" }),
        json!({ "new_path": text(&moved.join("x/a.wav")) }),
    ] {
        let e = try_run(&mut t.s, "file.locate", args).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgs, "{}", e.message);
    }
    let empty = t.outside().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let e = try_run(&mut t.s, "file.locate", json!({ "folder": text(&empty) })).unwrap_err();
    assert!(
        matches!(e.code, ErrorCode::NotFound | ErrorCode::NothingToDo),
        "{e:?}"
    );
}

#[test]
fn picking_the_clips_that_use_a_file_changes_no_history() {
    let mut t = song(
        "select",
        &[
            "Media/Recordings/take1.wav",
            "Media/Bounces/A.wav",
            "Media/Recordings/take1.wav",
        ],
        &[],
    );
    let step = t.s.cursor();
    let r = run(
        &mut t.s,
        "select.using_file",
        json!({ "file": "Media\\Recordings\\take1.wav" }),
    );
    assert!(r.message.contains("クリップ 2 個"), "{}", r.message);
    assert_eq!(
        t.s.state.selection,
        vec![AnyId::Clip(ClipId(300)), AnyId::Clip(ClipId(302))]
    );
    assert_eq!(t.s.cursor(), step);
    let e = try_run(&mut t.s, "select.using_file", json!({ "file": "none.wav" })).unwrap_err();
    assert_eq!(e.code, ErrorCode::NothingToDo, "{}", e.message);
}

#[test]
fn the_commands_are_in_the_table_with_their_names() {
    let reg = shared_registry();
    for (name, label) in [
        ("file.collect", "曲のフォルダーにコピー"),
        ("file.locate", "ファイルの場所を指定"),
        ("select.using_file", "使っているクリップを選択"),
    ] {
        let def = reg.get(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(def.label(), Some(label));
    }
    for name in ["view.browser", "browser.tab"] {
        let def = reg.get(name).unwrap();
        let tab = def.args.iter().find(|a| a.name == "tab").unwrap();
        assert!(format!("{tab:?}").contains("song_files"), "{name}: {tab:?}");
    }
}
