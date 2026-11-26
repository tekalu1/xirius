//! `midi.import` (UI-076): each track of a file is a new instrument track, a song without clips
//! takes the file's tempo and time signatures, and the whole import is one step of the history.
use serde_json::json;
use std::{path::PathBuf, sync::Arc};
use xirius_core::{Session, model::ClipBody, model::Project, session::RunOptions};
use xirius_session::smf::build::{file, mtrk};
use xirius_session::{commands, ext::ExtConfig, output::OutputMode};

fn session(dir: &std::path::Path) -> Session {
    let mut s = Session::with_registry(Project::new("midi"), commands::shared_registry());
    commands::attach(
        &mut s,
        ExtConfig {
            output: OutputMode::None,
            catalog: Some(Arc::default()),
            scan: xirius_plugins::ScanOptions {
                dirs: vec![],
                cache: dir.join("plugins.json"),
                ..Default::default()
            },
        },
    );
    s
}

/// Melody and Bass at 100 BPM in 3/4 (two bars), then 4/4, at 480 a quarter.
fn band(dir: &std::path::Path) -> PathBuf {
    let conductor = mtrk(&[
        (0, &[0xff, 0x51, 0x03, 0x09, 0x27, 0xc0]),
        (0, &[0xff, 0x58, 0x04, 3, 2, 24, 8]),
        (2880, &[0xff, 0x58, 0x04, 4, 2, 24, 8]),
    ]);
    let melody = mtrk(&[
        (0, &[0xff, 0x03, 0x06, b'M', b'e', b'l', b'o', b'd', b'y']),
        (0, &[0x90, 72, 100]),
        (480, &[0x80, 72, 0]),
    ]);
    let bass = mtrk(&[
        (0, &[0xff, 0x03, 0x04, b'B', b'a', b's', b's']),
        (480, &[0x91, 36, 110]),
        (960, &[0x81, 36, 0]),
    ]);
    let path = dir.join("band.mid");
    std::fs::write(&path, file(1, 480, &[conductor, melody, bass])).unwrap();
    path
}

fn run(s: &mut Session, data: serde_json::Value) -> serde_json::Value {
    s.run(
        "midi.import",
        data.as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap()
    .data
}

#[test]
fn a_song_without_clips_takes_the_files_tempo_and_meter_in_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let path = band(dir.path());
    let mut s = session(dir.path());
    let before = s.project_arc();
    let ppq = i64::from(s.project().ppq);
    let out = run(&mut s, json!({"file": path}));
    assert_eq!(out["tracks"], 2);
    let p = s.project();
    let tracks: Vec<_> = p
        .tracks
        .values()
        .filter(|t| !before.tracks.contains_key(&t.id))
        .map(|t| {
            let ClipBody::Midi { notes, .. } = &t.clips[0].body else {
                panic!("a MIDI clip")
            };
            let plugin = t.inserts[0].name.clone();
            let notes: Vec<_> = notes.iter().map(|n| (n.start, n.pitch)).collect();
            (t.name.clone(), plugin, t.clips[0].name.clone(), notes)
        })
        .collect();
    assert_eq!(
        tracks,
        [
            (
                "Melody".into(),
                "Sampler".into(),
                "band".into(),
                vec![(0, 72)]
            ),
            (
                "Bass".into(),
                "Sampler".into(),
                "band".into(),
                vec![(ppq, 36)]
            ),
        ]
    );
    assert_eq!(
        p.tempo.iter().map(|t| (t.tick, t.bpm)).collect::<Vec<_>>(),
        [(0, 100.0)]
    );
    assert_eq!(
        p.timesig
            .iter()
            .map(|t| (t.bar, t.num, t.den))
            .collect::<Vec<_>>(),
        [(1, 3, 4), (3, 4, 4)]
    );
    // The clips are whole bars of the file's meter.
    let melody = p.tracks.values().find(|t| t.name == "Melody").unwrap();
    assert_eq!(melody.clips[0].length, 3 * ppq);
    s.undo(1).unwrap();
    assert_eq!(s.project(), before.as_ref());
}

#[test]
fn a_song_with_clips_keeps_its_tempo_and_meter_unless_told() {
    let dir = tempfile::tempdir().unwrap();
    let path = band(dir.path());
    let mut s = session(dir.path());
    let had = s.project().tracks.len();
    run(&mut s, json!({"file": path}));
    s.run(
        "tempo.set",
        json!({"bpm": 90}).as_object().unwrap(),
        &RunOptions::human(),
    )
    .unwrap();
    let bar5 = s.project().time_map().bar_start(5);
    run(&mut s, json!({"file": path, "at": bar5}));
    assert_eq!(s.project().tracks.len(), had + 4);
    assert_eq!(
        s.project().tempo.iter().map(|t| t.bpm).collect::<Vec<_>>(),
        [90.0]
    );
    let names: Vec<_> = s
        .project()
        .tracks
        .values()
        .skip(had)
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["Melody", "Bass", "Melody 2", "Bass 2"]);
    // `file` sets them from where the parts go.
    run(&mut s, json!({"file": path, "at": bar5, "tempo": "file"}));
    let p = s.project();
    assert_eq!(
        p.tempo.iter().map(|t| (t.tick, t.bpm)).collect::<Vec<_>>(),
        [(0, 90.0), (bar5, 100.0)]
    );
    assert_eq!(
        p.timesig
            .iter()
            .map(|t| (t.bar, t.num, t.den))
            .collect::<Vec<_>>(),
        [(1, 3, 4), (3, 4, 4), (5, 3, 4), (7, 4, 4)]
    );
}

#[test]
fn a_file_without_notes_or_not_midi_is_refused_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = session(dir.path());
    let before = s.project_arc();
    let empty = dir.path().join("empty.mid");
    std::fs::write(&empty, file(1, 480, &[mtrk(&[])])).unwrap();
    let text = dir.path().join("text.mid");
    std::fs::write(&text, "not midi").unwrap();
    for (path, says) in [
        (empty, "ノートがありません"),
        (text, "MIDI ファイルではありません"),
    ] {
        let e = s
            .run(
                "midi.import",
                json!({"file": path}).as_object().unwrap(),
                &RunOptions::human(),
            )
            .unwrap_err();
        assert!(e.message.contains(says), "{}", e.message);
    }
    assert_eq!(s.project(), before.as_ref());
}
