//! Export audition hears the written file; import is a single history step; trash uses the OS.
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use xirius_core::{Session, model::Project, session::RunOptions};
use xirius_session::{commands, ext::ExtConfig, output::OutputMode};

fn run(s: &mut Session, name: &str, data: serde_json::Value) -> serde_json::Value {
    s.run(name, data.as_object().unwrap(), &RunOptions::human())
        .unwrap()
        .data
}

fn fixture(tag: &str) -> (Session, PathBuf, String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../target/export-actions-{tag}-{}",
        std::process::id()
    ));
    // An earlier run that failed before its cleanup leaves its folder, and a later process can
    // be given the same id.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("source.wav");
    let mut wav = hound::WavWriter::create(
        &source,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for n in 0..96000 {
        wav.write_sample((n as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin() * 0.2)
            .unwrap();
    }
    wav.finalize().unwrap();
    let mut s = Session::with_registry(Project::new("actions"), commands::shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::Silent { paced: true },
            catalog: Some(Arc::default()),
            scan: xirius_plugins::ScanOptions {
                dirs: vec![],
                cache: dir.join("plugins.json"),
                ..Default::default()
            },
        },
    );
    s.save_as(dir.join("song")).unwrap();
    run(&mut s, "clip.import", json!({"file":source,"at":0}));
    let exported = run(
        &mut s,
        "export.mixdown",
        json!({"out":dir.join("export.wav"),"from":0,"to":3840,"background":true}),
    );
    let job = exported["job"].as_str().unwrap().to_string();
    let start = Instant::now();
    loop {
        let status = run(&mut s, "export.status", json!({"job":job}));
        if status["state"] == "done" {
            break;
        }
        assert_eq!(status["state"], "running", "{status}");
        assert!(start.elapsed() < Duration::from_secs(60));
        std::thread::sleep(Duration::from_millis(2));
    }
    (s, dir, job)
}

/// Removes the test's folder. An import asks for the file's peaks, which a thread of their own
/// writes in the song's `Peaks/`; one still writing there keeps the folder from going.
fn remove(dir: PathBuf) {
    xirius_session::peaks::wait();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn audition_reaches_the_silent_output_and_import_is_one_undo() {
    let (mut s, dir, job) = fixture("listen");
    let ids: Vec<_> = s.project().tracks.keys().map(ToString::to_string).collect();
    run(&mut s, "track.mute", json!({"ids":ids,"on":true}));
    run(&mut s, "export.preview", json!({"job":job}));
    let start = Instant::now();
    loop {
        let status = run(&mut s, "export.preview", json!({"job":job,"status":true}));
        if status["peak"].as_f64().unwrap_or(0.0) > 0.1 {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{status}");
        std::thread::sleep(Duration::from_millis(2));
    }
    run(&mut s, "export.preview", json!({"job":job,"stop":true}));
    assert_eq!(
        run(&mut s, "export.preview", json!({"job":job,"status":true}))["playing"],
        false
    );
    run(&mut s, "export.preview", json!({"job":job}));
    let start = Instant::now();
    loop {
        let status = run(&mut s, "export.preview", json!({"job":job,"status":true}));
        if status["playing"] == false {
            assert!(status["peak"].as_f64().unwrap() > 0.1);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{status}");
        std::thread::sleep(Duration::from_millis(4));
    }
    let before = s.project_arc();
    run(&mut s, "export.import", json!({"job":job,"at":3840}));
    assert_eq!(s.project().tracks.len(), before.tracks.len() + 1);
    let added = s
        .project()
        .tracks
        .values()
        .find(|t| !before.tracks.contains_key(&t.id))
        .unwrap();
    assert_eq!(added.clips[0].start, 3840);
    s.undo(1).unwrap();
    assert_eq!(s.project(), before.as_ref());
    drop(s);
    remove(dir);
}

#[test]
fn delete_moves_the_written_file_to_the_real_recycle_bin() {
    let (mut s, dir, job) = fixture("trash");
    let original = std::fs::canonicalize(dir.join("export.wav")).unwrap();
    let bytes = std::fs::read(&original).unwrap();
    assert!(dir.join("export.wav").exists());
    let since = std::time::SystemTime::now();
    run(&mut s, "export.delete", json!({"job":job}));
    assert!(!dir.join("export.wav").exists());
    #[cfg(windows)]
    verify_recycled(&original, &bytes, since);
    #[cfg(not(windows))]
    let _ = (&original, &bytes, since);
    assert_eq!(
        run(&mut s, "export.status", json!({}))["jobs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    drop(s);
    remove(dir);
}

#[cfg(windows)]
fn verify_recycled(original: &Path, bytes: &[u8], since: std::time::SystemTime) {
    let expected = original
        .display()
        .to_string()
        .trim_start_matches(r"\\?\")
        .to_lowercase();
    let drive = &expected[..2];
    let bin = PathBuf::from(format!(r"{drive}\$Recycle.Bin"));
    let mut found = None;
    'bins: for sid in std::fs::read_dir(bin).unwrap().flatten() {
        let Ok(entries) = std::fs::read_dir(sid.path()) else {
            continue;
        };
        for item in entries.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if !name.starts_with("$I") {
                continue;
            }
            if item
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t < since)
            {
                continue;
            }
            let Ok(meta) = std::fs::read(item.path()) else {
                continue;
            };
            if meta.len() < 28 {
                continue;
            }
            let version = u64::from_le_bytes(meta[..8].try_into().unwrap());
            let offset = if version == 2 { 28 } else { 24 };
            let path: Vec<_> = meta[offset..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|b| *b != 0)
                .collect();
            if String::from_utf16_lossy(&path).to_lowercase() == expected {
                found = Some((
                    item.path(),
                    item.path().with_file_name(name.replacen("$I", "$R", 1)),
                ));
                break 'bins;
            }
        }
    }
    let (metadata, content) = found.expect("generated export is a recoverable recycle-bin item");
    assert_eq!(std::fs::read(&content).unwrap(), bytes);
    std::fs::copy(&content, original).unwrap();
    assert_eq!(std::fs::read(original).unwrap(), bytes);
    std::fs::remove_file(content).unwrap();
    std::fs::remove_file(metadata).unwrap();
}

#[test]
fn import_without_a_place_goes_to_the_head_of_the_exported_range() {
    let (mut s, dir, _) = fixture("range-head");
    let mid = run(
        &mut s,
        "export.mixdown",
        json!({"out":dir.join("mid.wav"),"range":"custom","from":1920,"to":3840}),
    );
    let stems = run(
        &mut s,
        "export.stems",
        json!({"dir":dir.join("mid-stems"),"range":"custom","from":960,"to":3840}),
    );
    for (record, head) in [(&mid["record"], 1920), (&stems["record"], 960)] {
        let before = s.project_arc();
        run(&mut s, "export.import", json!({"job":record}));
        let added: Vec<_> = s
            .project()
            .tracks
            .values()
            .filter(|t| !before.tracks.contains_key(&t.id))
            .map(|t| t.clips[0].start)
            .collect();
        assert!(!added.is_empty());
        assert!(added.iter().all(|at| *at == head), "{added:?} != {head}");
        // A place given still wins.
        s.undo(1).unwrap();
        run(&mut s, "export.import", json!({"job":record,"at":0}));
        let at: Vec<_> = s
            .project()
            .tracks
            .values()
            .filter(|t| !before.tracks.contains_key(&t.id))
            .map(|t| t.clips[0].start)
            .collect();
        assert!(at.iter().all(|at| *at == 0), "{at:?}");
        s.undo(1).unwrap();
    }
    drop(s);
    remove(dir);
}

#[test]
fn stems_import_all_files_in_one_step_and_report_all_changes() {
    let (mut s, dir, _) = fixture("stems");
    run(
        &mut s,
        "clip.import",
        json!({"file":dir.join("source.wav")}),
    );
    let export = run(&mut s, "export.stems", json!({"dir":dir.join("stems")}));
    let job = export["record"].as_str().unwrap();
    let before = s.project_arc();
    let result = s
        .run(
            "export.import",
            json!({"job":job,"at":3840}).as_object().unwrap(),
            &RunOptions::human(),
        )
        .unwrap();
    assert_eq!(s.project().tracks.len(), before.tracks.len() + 2);
    assert_eq!(result.data["placed"].as_array().unwrap().len(), 2);
    assert!(!result.changes.is_empty());
    s.undo(1).unwrap();
    assert_eq!(s.project(), before.as_ref());
    drop(s);
    remove(dir);
}

#[test]
fn relative_export_paths_keep_the_written_file_when_the_song_has_a_namesake() {
    let (mut s, dir, _) = fixture("relative");
    let filename = format!("relative-export-{}.wav", std::process::id());
    let destination = std::env::current_dir().unwrap().join(&filename);
    std::fs::write(dir.join("song").join(&filename), b"not an audio file").unwrap();
    let output = run(
        &mut s,
        "export.mixdown",
        json!({"out":filename,"overwrite":true}),
    );
    assert!(Path::new(output["path"].as_str().unwrap()).is_absolute());
    run(&mut s, "export.import", json!({"job":output["record"]}));
    let added = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == destination.file_stem().unwrap().to_string_lossy())
        .unwrap();
    let xirius_core::model::ClipBody::Audio { file, .. } = &added.clips[0].body else {
        panic!()
    };
    // The written file, copied into the song as it is read (BRW-004), not the namesake.
    assert_eq!(file, &format!("Media/Imports/{filename}"));
    assert_eq!(
        std::fs::read(dir.join("song").join(file)).unwrap(),
        std::fs::read(&destination).unwrap()
    );
    drop(s);
    std::fs::remove_file(destination).unwrap();
    remove(dir);
}

#[test]
fn unfinished_records_never_adopt_an_old_destination_file() {
    let (mut s, dir, _) = fixture("unfinished");
    let before = s.project_arc();
    for state in ["cancelled", "failed", "interrupted"] {
        let job = format!("export-{state}");
        xirius_session::jobs::Store::of_song(&dir.join("song")).write(&json!({
            "job":job,"command":"export.mixdown","state":state,"path":dir.join("export.wav"),"result":null
        })).unwrap();
        for command in ["export.preview", "export.import"] {
            assert!(
                s.run(
                    command,
                    json!({"job":job}).as_object().unwrap(),
                    &RunOptions::human()
                )
                .is_err()
            );
        }
    }
    assert_eq!(s.project(), before.as_ref());
    drop(s);
    remove(dir);
}
