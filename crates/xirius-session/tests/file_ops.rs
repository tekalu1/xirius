//! `file.rename`, `file.trash`, `file.add.folder` and `file.list` through a session (BRW-013,
//! BRW-014, BRW-004): a rename points the song's clips and frozen tracks at the new name in one
//! step of the history; a file the song uses does not go to the recycle bin. The recycle bin
//! here is a folder of the test, and every file is in the test's own folder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Map, Value, json};
use xirius_core::id::{ClipId, Node, TrackId};
use xirius_core::model::{Clip, ClipBody, Frozen, Project, Track, TrackKind};
use xirius_core::session::{RunOptions, RunResult, Session};
use xirius_core::{AiPermission, CmdError, ErrorCode};
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::{ExtConfig, ExtExtras};
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

fn audio(id: u64, file: &str) -> Clip {
    Clip {
        id: ClipId(id),
        name: "take".into(),
        color: None,
        start: 0,
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
    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }
    fn outside(&self) -> PathBuf {
        self.root.path().join("outside")
    }
    /// The files the clips and the frozen track name, in the track's order.
    fn files(&self) -> Vec<String> {
        let p = self.s.project();
        let mut out = Vec::new();
        for id in [TrackId(100), TrackId(101)] {
            let t = &p.tracks[&id];
            for c in &t.clips {
                if let ClipBody::Audio { file, .. } = &c.body {
                    out.push(file.clone());
                }
            }
            if let Some(z) = &t.frozen {
                out.push(format!("frozen:{}", z.file));
            }
        }
        out
    }
}

/// A saved song whose clips use `Audio/kick.wav` twice and `<outside>/snare.wav` once, and whose
/// second track is frozen to `Audio/kick.wav`; `Audio/spare.wav` and `Loops/` are not used.
fn song(tag: &str) -> Song {
    let root = tempfile::Builder::new()
        .prefix(&format!("xirius-file-ops-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = root.path().join("song");
    let outside = root.path().join("outside");
    let snare = outside.join("snare.wav").to_string_lossy().into_owned();

    let mut p = Project::new("Files");
    p.next_id = 1000;
    let mut a = Track::new(TrackId(100), TrackKind::Audio, "A");
    a.clips.push(audio(300, "Audio/kick.wav"));
    a.clips.push(audio(301, "Audio/kick.wav"));
    a.clips.push(audio(302, &snare));
    let mut b = Track::new(TrackId(101), TrackKind::Audio, "B");
    b.frozen = Some(Frozen {
        file: "Audio/kick.wav".into(),
        start: 0,
    });
    for t in [a, b] {
        p.root.push(Node::Track(t.id));
        p.tracks.insert(t.id, Arc::new(t));
    }
    drop(Session::create(&dir, p).unwrap());
    std::fs::create_dir_all(dir.join("Audio")).unwrap();
    std::fs::create_dir_all(dir.join("Loops")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    for f in ["Audio/kick.wav", "Audio/spare.wav", "Loops/loop.wav"] {
        std::fs::write(dir.join(f), b"x").unwrap();
    }
    std::fs::write(outside.join("snare.wav"), b"x").unwrap();
    let mut s = Session::open_with(&dir, shared_registry()).unwrap();
    let bin = root.path().join("bin");
    let trash = Arc::new(move |p: &Path| {
        assert!(
            p.starts_with(bin.parent().unwrap()),
            "only the test's own files go to its bin: {}",
            p.display()
        );
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    });
    commands::attach_with(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            ..ExtConfig::default()
        },
        ExtExtras {
            trash: Some(trash),
            ..ExtExtras::default()
        },
    );
    Song { root, s }
}

#[test]
fn renaming_a_used_file_renames_it_on_the_disk_and_points_the_song_at_it_in_one_step() {
    let mut t = song("rename");
    let before = t.s.steps().len();
    let start = t.s.cursor();
    let r = run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/kick.wav", "name": "Kick 01" }),
    );
    assert!(!t.dir().join("Audio/kick.wav").exists());
    assert!(
        t.dir().join("Audio/Kick 01.wav").is_file(),
        "the extension stays"
    );
    assert!(r.message.contains("Kick 01.wav"), "{}", r.message);
    assert!(r.message.contains("3 件"), "{}", r.message);
    let snare = t.outside().join("snare.wav").to_string_lossy().into_owned();
    assert_eq!(
        t.files(),
        [
            "Audio/Kick 01.wav".to_string(),
            "Audio/Kick 01.wav".into(),
            snare.clone(),
            "frozen:Audio/Kick 01.wav".into(),
        ]
    );
    assert_eq!(t.s.steps().len(), before + 1, "one step");

    // The undo takes the name on the disk back with the references; the redo renames it again.
    let r = run(&mut t.s, "history.undo", json!({}));
    let renamed = &r.data["renamed"][0];
    assert!(
        renamed["from"].as_str().unwrap().ends_with("Kick 01.wav"),
        "{renamed}"
    );
    assert!(
        renamed["to"].as_str().unwrap().ends_with("kick.wav"),
        "{renamed}"
    );
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert!(!t.dir().join("Audio/Kick 01.wav").exists());
    assert_eq!(
        t.files(),
        [
            "Audio/kick.wav".to_string(),
            "Audio/kick.wav".into(),
            snare.clone(),
            "frozen:Audio/kick.wav".into(),
        ]
    );
    run(&mut t.s, "history.redo", json!({}));
    assert!(!t.dir().join("Audio/kick.wav").exists());
    assert!(t.dir().join("Audio/Kick 01.wav").is_file());
    assert_eq!(t.files()[0], "Audio/Kick 01.wav");
    assert_eq!(t.files()[3], "frozen:Audio/Kick 01.wav");

    // A jump in the history crosses the rename the same way.
    run(&mut t.s, "history.jump", json!({ "step": start }));
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert_eq!(t.files()[0], "Audio/kick.wav");
}

#[test]
fn a_rename_that_cannot_be_undone_on_the_disk_leaves_the_references_and_the_disk_as_they_are() {
    let mut t = song("blocked");
    run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/kick.wav", "name": "Kick 01" }),
    );
    let cursor = t.s.cursor();

    // The old name is taken again: the undo refuses and moves nothing.
    std::fs::write(t.dir().join("Audio/kick.wav"), b"new").unwrap();
    let e = try_run(&mut t.s, "history.undo", json!({})).unwrap_err();
    assert!(e.message.contains("既にある"), "{}", e.message);
    assert!(e.message.contains("取り消していません"), "{}", e.message);
    assert_eq!(t.files()[0], "Audio/Kick 01.wav");
    assert_eq!(t.files()[3], "frozen:Audio/Kick 01.wav");
    assert_eq!(
        std::fs::read(t.dir().join("Audio/kick.wav")).unwrap(),
        b"new"
    );
    assert!(t.dir().join("Audio/Kick 01.wav").is_file());
    assert_eq!(t.s.cursor(), cursor, "the history did not move");

    // The renamed file is gone: the same.
    std::fs::remove_file(t.dir().join("Audio/kick.wav")).unwrap();
    std::fs::remove_file(t.dir().join("Audio/Kick 01.wav")).unwrap();
    let e = try_run(&mut t.s, "history.undo", json!({})).unwrap_err();
    assert!(e.message.contains("見つからない"), "{}", e.message);
    assert_eq!(t.files()[0], "Audio/Kick 01.wav");

    // Once the file is back the undo goes through.
    std::fs::write(t.dir().join("Audio/Kick 01.wav"), b"x").unwrap();
    run(&mut t.s, "history.undo", json!({}));
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert_eq!(t.files()[0], "Audio/kick.wav");

    // The redo refuses the same way when the new name is taken.
    std::fs::write(t.dir().join("Audio/Kick 01.wav"), b"other").unwrap();
    let e = try_run(&mut t.s, "history.redo", json!({})).unwrap_err();
    assert!(e.message.contains("やり直していません"), "{}", e.message);
    assert_eq!(t.files()[0], "Audio/kick.wav");
    assert!(t.dir().join("Audio/kick.wav").is_file());
}

#[test]
fn an_ai_that_may_not_rename_may_not_undo_a_rename_either() {
    let mut t = song("ai");
    run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/kick.wav", "name": "Kick 01" }),
    );
    t.s.set_ai_permission(AiPermission::Edit).unwrap();
    let ai = RunOptions::ai("bot");
    let e = commands::run(&mut t.s, None, "history.undo", &Map::new(), &ai).unwrap_err();
    assert_eq!(e.code, ErrorCode::Denied, "{}", e.message);
    assert!(t.dir().join("Audio/Kick 01.wav").is_file());
    assert_eq!(t.files()[0], "Audio/Kick 01.wav");

    t.s.set_ai_permission(AiPermission::Save).unwrap();
    commands::run(&mut t.s, None, "history.undo", &Map::new(), &ai).unwrap();
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert_eq!(t.files()[0], "Audio/kick.wav");
}

#[test]
fn a_hand_made_relink_may_only_name_a_rename_in_one_folder_that_was_made() {
    let mut t = song("relink");
    let e = try_run(
        &mut t.s,
        "clip.relink",
        json!({ "files": [{ "from": "Audio/kick.wav", "to": "Loops/loop.wav" }],
                "disk": [{ "from": "Audio/kick.wav", "to": "Loops/loop.wav" }] }),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgs, "{}", e.message);
    let e = try_run(
        &mut t.s,
        "clip.relink",
        json!({ "files": [{ "from": "Audio/kick.wav", "to": "Audio/none.wav" }],
                "disk": [{ "from": "Audio/kick.wav", "to": "Audio/none.wav" }] }),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "{}", e.message);
    assert_eq!(t.files()[0], "Audio/kick.wav");
}

#[test]
fn renaming_a_folder_points_the_files_under_it_and_a_file_outside_the_song_keeps_its_full_path() {
    let mut t = song("folder");
    run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio", "name": "Drums" }),
    );
    assert!(t.dir().join("Drums/kick.wav").is_file());
    assert_eq!(t.files()[0], "Drums/kick.wav");
    assert_eq!(t.files()[3], "frozen:Drums/kick.wav");

    let outside = t.outside().to_string_lossy().into_owned();
    run(
        &mut t.s,
        "file.rename",
        json!({ "path": outside, "name": "elsewhere" }),
    );
    let moved = t.root.path().join("elsewhere").join("snare.wav");
    assert!(moved.is_file());
    assert_eq!(t.files()[2], moved.to_string_lossy());

    // Two undos take both folders back, the last rename first.
    run(&mut t.s, "history.undo", json!({ "count": 2 }));
    assert!(t.outside().join("snare.wav").is_file());
    assert!(!t.root.path().join("elsewhere").exists());
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert!(!t.dir().join("Drums").exists());
    assert_eq!(t.files()[0], "Audio/kick.wav");
    assert_eq!(
        t.files()[2],
        t.outside().join("snare.wav").to_string_lossy()
    );
}

#[test]
fn renaming_an_unused_file_leaves_the_history_alone_and_a_dry_run_leaves_the_disk_alone() {
    let mut t = song("unused");
    let before = t.s.steps().len();
    run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/spare.wav", "name": "spare 2.wav" }),
    );
    assert!(t.dir().join("Audio/spare 2.wav").is_file());
    assert_eq!(t.s.steps().len(), before);

    let mut dry = RunOptions::human();
    dry.dry_run = true;
    commands::run(
        &mut t.s,
        None,
        "file.rename",
        &map(json!({ "path": "Audio/kick.wav", "name": "k" })),
        &dry,
    )
    .unwrap();
    assert!(t.dir().join("Audio/kick.wav").is_file());
    assert_eq!(t.files()[0], "Audio/kick.wav");
}

#[test]
fn a_name_in_use_a_bad_name_and_the_songs_own_files_are_refused() {
    let mut t = song("refuse");
    let e = try_run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/spare.wav", "name": "kick" }),
    )
    .unwrap_err();
    assert!(e.message.contains("既にあります"), "{e:?}");
    let e = try_run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/spare.wav", "name": "a/b" }),
    )
    .unwrap_err();
    assert!(e.message.contains("/"), "{e:?}");
    for own in ["project.json", "manifest.toml", "history"] {
        if !t.dir().join(own).exists() {
            continue;
        }
        let e = try_run(&mut t.s, "file.rename", json!({ "path": own, "name": "x" })).unwrap_err();
        assert!(e.message.contains("曲のデータ"), "{own}: {e:?}");
        let e = try_run(&mut t.s, "file.trash", json!({ "path": own })).unwrap_err();
        assert!(e.message.contains("曲のデータ"), "{own}: {e:?}");
    }
    let song_dir = t.dir().to_string_lossy().into_owned();
    let e = try_run(
        &mut t.s,
        "file.rename",
        json!({ "path": song_dir, "name": "other" }),
    )
    .unwrap_err();
    assert!(e.message.contains("開いている曲のフォルダー"), "{e:?}");
    let parent = t.root.path().to_string_lossy().into_owned();
    let e = try_run(&mut t.s, "file.trash", json!({ "path": parent })).unwrap_err();
    assert!(e.message.contains("開いている曲のフォルダー"), "{e:?}");
    let e = try_run(
        &mut t.s,
        "file.rename",
        json!({ "path": "Audio/none.wav", "name": "x" }),
    )
    .unwrap_err();
    assert!(e.message.contains("none.wav"), "{e:?}");
}

#[test]
fn an_unused_file_goes_to_the_bin_and_a_used_one_or_its_folder_does_not() {
    let mut t = song("trash");
    let r = run(&mut t.s, "file.trash", json!({ "path": "Audio/spare.wav" }));
    assert!(r.message.contains("ごみ箱"), "{}", r.message);
    assert!(!t.dir().join("Audio/spare.wav").exists());
    assert!(t.bin().join("spare.wav").is_file());

    let e = try_run(&mut t.s, "file.trash", json!({ "path": "Audio/kick.wav" })).unwrap_err();
    assert!(e.message.contains("使用中"), "{e:?}");
    let e = try_run(&mut t.s, "file.trash", json!({ "path": "Audio" })).unwrap_err();
    assert!(e.message.contains("中のファイルを曲で使用中"), "{e:?}");
    assert!(t.dir().join("Audio/kick.wav").is_file());

    let mut dry = RunOptions::human();
    dry.dry_run = true;
    commands::run(
        &mut t.s,
        None,
        "file.trash",
        &map(json!({ "path": "Loops" })),
        &dry,
    )
    .unwrap();
    assert!(t.dir().join("Loops").is_dir());
    run(&mut t.s, "file.trash", json!({ "path": "Loops" }));
    assert!(!t.dir().join("Loops").exists());
    assert!(t.bin().join("Loops/loop.wav").is_file());
}

#[test]
fn a_new_folder_is_made_once() {
    let mut t = song("folder-add");
    let r = run(
        &mut t.s,
        "file.add.folder",
        json!({ "path": "Audio", "name": "Takes" }),
    );
    assert!(r.message.contains("Takes"), "{}", r.message);
    assert!(t.dir().join("Audio/Takes").is_dir());
    let e = try_run(
        &mut t.s,
        "file.add.folder",
        json!({ "path": "Audio", "name": "Takes" }),
    )
    .unwrap_err();
    assert!(e.message.contains("既にあります"), "{e:?}");
    let e = try_run(
        &mut t.s,
        "file.add.folder",
        json!({ "path": "Audio/kick.wav", "name": "x" }),
    )
    .unwrap_err();
    assert!(e.message.contains("フォルダーではありません"), "{e:?}");
}

#[test]
fn the_list_names_each_file_the_song_uses_with_its_clips_and_whether_it_is_there() {
    let mut t = song("list");
    std::fs::remove_file(t.outside().join("snare.wav")).unwrap();
    let r = run(&mut t.s, "file.list", json!({}));
    let files = r.data["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "{}", r.data);
    let kick = files
        .iter()
        .find(|f| f["file"] == "Audio/kick.wav")
        .unwrap();
    assert_eq!(kick["clips"], 2);
    assert_eq!(kick["frozen"], 1);
    assert_eq!(kick["exists"], true);
    assert_eq!(kick["inside"], true);
    let snare = files
        .iter()
        .find(|f| f["file"] != "Audio/kick.wav")
        .unwrap();
    assert_eq!(snare["exists"], false);
    assert_eq!(snare["inside"], false);
    assert_eq!(r.data["missing"], 1);
    assert!(r.message.contains("見つかりません"), "{}", r.message);
}

#[test]
fn the_commands_are_in_the_table_with_their_names() {
    let reg = shared_registry();
    for (name, label) in [
        ("file.rename", "ファイル名を変更"),
        ("file.trash", "ごみ箱に移動"),
        ("file.add.folder", "新しいフォルダー"),
        ("file.list", "曲が使うファイル"),
    ] {
        let def = reg.get(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(def.label(), Some(label));
    }
}
