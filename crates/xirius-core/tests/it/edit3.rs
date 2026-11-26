use serde_json::{Value, json};
use xirius_core::{Session, model::Note, session::RunOptions};

fn run(s: &mut Session, cmd: &str, args: Value) -> Value {
    s.run_value(cmd, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{cmd}: {e:?}"))
        .data
}
fn fixture() -> (Session, String) {
    let mut s = Session::new("edit");
    let t = run(&mut s, "track.add.instrument", json!({"name":"Keys"}))["id"].clone();
    let c = run(
        &mut s,
        "clip.add.midi",
        json!({"track":t,"at":0,"length":4000}),
    )["id"]
        .as_str()
        .unwrap()
        .to_owned();
    run(
        &mut s,
        "note.put",
        json!({"clip":c,"notes":[{"at":17,"pitch":60,"length":101,"velocity":80},{"at":487,"pitch":60,"length":220,"velocity":100}]}),
    );
    (s, c)
}
fn notes(s: &Session, c: &str) -> Vec<Note> {
    s.project()
        .clip(c.parse().unwrap())
        .unwrap()
        .1
        .notes()
        .to_vec()
}

#[test]
fn velocity_shapes_preserve_differences_and_equalize() {
    let (mut s, c) = fixture();
    for (shape, expected) in [
        ("stronger", [90, 110]),
        ("softer", [80, 100]),
        ("crescendo", [80, 110]),
        ("decrescendo", [80, 100]),
        ("equal", [64, 64]),
    ] {
        run(
            &mut s,
            "note.velocity.shape",
            json!({"ids":[c],"shape":shape,"amount":10,"value":64}),
        );
        assert_eq!(
            notes(&s, &c).iter().map(|n| n.velocity).collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn note_length_modes_and_undo() {
    let (mut s, c) = fixture();
    for (mode, expected) in [
        ("double", [202, 440]),
        ("half", [101, 220]),
        ("grid", [240, 240]),
        ("next", [470, 240]),
    ] {
        run(&mut s, "note.resize", json!({"ids":[c],"mode":mode}));
        assert_eq!(
            notes(&s, &c).iter().map(|n| n.length).collect::<Vec<_>>(),
            expected
        );
    }
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c)[0].length, 240);
}

#[test]
fn repeated_quantize_restores_original_timing_and_freeze_replaces_it() {
    let (mut s, c) = fixture();
    let before = notes(&s, &c);
    for strength in [50, 100, 50] {
        run(
            &mut s,
            "note.quantize",
            json!({"ids":[c],"strength":strength,"end":100}),
        );
    }
    run(&mut s, "note.quantize", json!({"ids":[c],"restore":true}));
    assert_eq!(notes(&s, &c), before);
    run(&mut s, "note.quantize", json!({"ids":[c],"end":100}));
    let fixed = notes(&s, &c);
    run(&mut s, "note.quantize", json!({"ids":[c],"freeze":true}));
    run(&mut s, "note.quantize", json!({"ids":[c],"grid":"1/4"}));
    run(&mut s, "note.quantize", json!({"ids":[c],"restore":true}));
    assert_eq!(notes(&s, &c), fixed);
}

#[test]
fn humanize_is_bounded_repeatable_and_one_undo() {
    let (mut s, c) = fixture();
    let before = notes(&s, &c);
    run(
        &mut s,
        "note.humanize",
        json!({"ids":[c],"seed":19,"timing":10,"velocity":4}),
    );
    let changed = notes(&s, &c);
    assert_ne!(changed, before);
    for (n, o) in changed.iter().zip(&before) {
        assert!((n.start - o.start).abs() <= 10);
        assert!((i32::from(n.velocity) - i32::from(o.velocity)).abs() <= 4);
    }
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c), before);
    run(
        &mut s,
        "note.humanize",
        json!({"ids":[c],"seed":19,"timing":10,"velocity":4}),
    );
    assert_eq!(notes(&s, &c), changed);
}

#[test]
fn joined_notes_and_split_parts_keep_music_and_undo() {
    let (mut s, c) = fixture();
    let before = notes(&s, &c);
    run(&mut s, "note.join", json!({"ids":[c]}));
    assert_eq!(notes(&s, &c).len(), 1);
    assert_eq!(notes(&s, &c)[0].length, 690);
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c), before);
    let ids = run(&mut s, "clip.split", json!({"ids":[c],"at":300}))["ids"].clone();
    let mut ids = ids.as_array().unwrap().clone();
    ids.push(json!(c));
    run(&mut s, "clip.join", json!({"ids":ids}));
    assert_eq!(notes(&s, &c), before);
}

#[test]
fn editors_keep_separate_snap_and_grid_and_save_them() {
    let (mut s, _) = fixture();
    run(&mut s, "view.grid", json!({"editor":"roll","grid":"1/8t"}));
    run(&mut s, "view.snap", json!({"editor":"roll","off":true}));
    assert_eq!(s.state.view.grid, "1/16");
    assert!(s.state.view.snap);
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-edit3-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("edit3");
    s.save_as(&dir).unwrap();
    drop(s);
    let reopened = Session::open(&dir).unwrap();
    assert_eq!(reopened.state.view.roll.grid, "1/8t");
    assert!(!reopened.state.view.roll.snap);
    assert_eq!(reopened.state.view.grid, "1/16");
    drop(reopened);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn audio_duplication_and_split_keep_exactly_one_material_file() {
    let mut s = Session::new("shared");
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-shared-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("shared");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("source.wav");
    std::fs::write(&file, b"shared material").unwrap();
    let t = run(&mut s, "track.add.audio", json!({}))["id"].clone();
    let c = run(
        &mut s,
        "clip.add.audio",
        json!({"track":t,"file":file,"at":0,"length":3840}),
    )["id"]
        .clone();
    run(&mut s, "clip.duplicate", json!({"ids":[c],"count":5}));
    run(&mut s, "clip.split", json!({"ids":[c],"at":1920}));
    for clip in &s.project().tracks[&t.as_str().unwrap().parse().unwrap()].clips {
        let xirius_core::model::ClipBody::Audio { file: source, .. } = &clip.body else {
            panic!()
        };
        assert_eq!(std::path::Path::new(source), file);
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert_eq!(std::fs::read(&file).unwrap(), b"shared material");
    run(&mut s, "edit.cut", json!({"ids":[c]}));
    run(&mut s, "edit.paste", json!({"at":8000}));
    let song_dir = dir.join("song");
    s.save_as(&song_dir).unwrap();
    drop(s);
    let reopened = Session::open(&song_dir).unwrap();
    let track = &reopened.project().tracks[&t.as_str().unwrap().parse().unwrap()];
    assert_eq!(track.clips.len(), 7);
    assert!(track.clips.iter().all(|clip| matches!(
        &clip.body,
        xirius_core::model::ClipBody::Audio { file: source, .. } if std::path::Path::new(source) == file
    )));
    assert_eq!(
        std::fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().path().is_file())
            .count(),
        1
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"shared material");
    drop(reopened);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn timing_restore_after_part_split_and_join_uses_the_new_part_coordinates() {
    let (mut s, c) = fixture();
    let before = notes(&s, &c);
    run(&mut s, "note.quantize", json!({"ids":[c]}));
    let right = run(&mut s, "clip.split", json!({"ids":[c],"at":300}))["ids"][0].clone();
    run(
        &mut s,
        "note.quantize",
        json!({"ids":[right],"restore":true}),
    );
    assert_eq!(notes(&s, right.as_str().unwrap())[0].start, 187);
    run(&mut s, "clip.join", json!({"ids":[c,right]}));
    run(&mut s, "note.quantize", json!({"ids":[c],"restore":true}));
    assert_eq!(notes(&s, &c), before);
}

#[test]
fn external_groove_is_persisted_and_quantizes_timing_and_velocity() {
    let (mut s, c) = fixture();
    let hits = json!([{"at":0,"velocity":40},{"at":270,"velocity":110}]);
    run(
        &mut s,
        "view.groove.import",
        json!({"name":"shuffle","period":480,"hits":hits}),
    );
    run(
        &mut s,
        "note.quantize",
        json!({"ids":[c],"groove-hits":hits,"groove-period":480,"velocity":100}),
    );
    assert_eq!(notes(&s, &c)[0].velocity, 40);
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-edit3-groove-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("edit3-groove");
    s.save_as(&dir).unwrap();
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(
        s.state.view.roll.external_groove.as_ref().unwrap().name,
        "shuffle"
    );
    assert_eq!(
        s.state.view.roll.external_groove.as_ref().unwrap().hits,
        [(0, 40), (270, 110)]
    );
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}
