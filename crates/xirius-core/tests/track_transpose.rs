//! A track's transpose and velocity (UI-312): the track keeps them, the notes stay as written,
//! the values are added when it plays, and one command is one step of the history.

use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::model::{Project, Track, TrackKind, track_velocity};
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn track(p: &Project, id: &str) -> Track {
    p.tracks
        .get(&id.parse::<TrackId>().unwrap())
        .expect("the track")
        .as_ref()
        .clone()
}

/// An instrument track with a MIDI clip of the notes 60, 64 and 67.
fn midi(s: &mut Session) -> (String, String) {
    let t = id_of(&run(s, "track.add.instrument", json!({"name": "Bass"})));
    let c = id_of(&run(
        s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 1920}),
    ));
    for (at, pitch) in [(0, 60), (480, 64), (960, 67)] {
        run(
            s,
            "note.add",
            json!({"clip": c, "pitch": pitch, "at": at, "length": 240}),
        );
    }
    (t, c)
}

fn pitches(p: &Project, c: &str) -> Vec<u8> {
    p.clip(c.parse().unwrap())
        .unwrap()
        .1
        .notes()
        .iter()
        .map(|n| n.pitch)
        .collect()
}

#[test]
fn the_values_stay_on_the_track_and_the_notes_do_not_move() {
    let mut s = Session::new("track-values");
    let (t, c) = midi(&mut s);
    let before = pitches(s.project(), &c);
    assert_eq!(track(s.project(), &t).transpose, 0);
    assert_eq!(track(s.project(), &t).velocity, 0);
    let n = s.history().len();

    run(&mut s, "track.transpose", json!({"ids": [t], "semis": -12}));
    assert_eq!(track(s.project(), &t).transpose, -12);
    run(&mut s, "track.velocity", json!({"ids": [t], "percent": 10}));
    assert_eq!(track(s.project(), &t).velocity, 10);
    assert_eq!(pitches(s.project(), &c), before, "the notes stay written");
    assert_eq!(s.history().len(), n + 2, "one step each");
}

#[test]
fn the_ranges_are_held_at_their_ends() {
    let mut s = Session::new("track-range");
    let (t, _) = midi(&mut s);
    for (command, arg, given, held) in [
        ("track.transpose", "semis", 100, 64),
        ("track.transpose", "semis", -100, -64),
        ("track.velocity", "percent", 300, 100),
        ("track.velocity", "percent", -300, -100),
    ] {
        run(&mut s, command, json!({"ids": [t], arg: given}));
        let got = if command == "track.transpose" {
            track(s.project(), &t).transpose
        } else {
            track(s.project(), &t).velocity
        };
        assert_eq!(got, held, "{command} {given}");
    }
}

#[test]
fn one_undo_takes_each_value_back() {
    let mut s = Session::new("track-undo");
    let (t, _) = midi(&mut s);
    run(&mut s, "track.transpose", json!({"ids": [t], "semis": 7}));
    run(
        &mut s,
        "track.velocity",
        json!({"ids": [t], "percent": -20}),
    );
    s.undo(1).unwrap();
    assert_eq!(track(s.project(), &t).velocity, 0);
    assert_eq!(track(s.project(), &t).transpose, 7, "the other stays");
    s.undo(1).unwrap();
    assert_eq!(track(s.project(), &t).transpose, 0);
}

#[test]
fn a_saved_song_keeps_the_values_and_an_old_one_reads_zero() {
    let dir = std::env::temp_dir().join(format!("xirius-track-values-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new("track-save");
    let (t, _) = midi(&mut s);
    run(&mut s, "track.transpose", json!({"ids": [t], "semis": -5}));
    run(&mut s, "track.velocity", json!({"ids": [t], "percent": 15}));
    s.save_as(&dir).unwrap();
    let json: Value = serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap())
        .expect("project.json");
    let saved = &json["project"]["tracks"][t.as_str()];
    assert_eq!(saved["transpose"], -5);
    assert_eq!(saved["velocity"], 15);
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(track(s.project(), &t).transpose, -5);
    assert_eq!(track(s.project(), &t).velocity, 15);
    drop(s);

    // A value of 0 writes nothing, as older songs have it.
    let mut s = Session::open(&dir).unwrap();
    run(&mut s, "track.transpose", json!({"ids": [t], "semis": 0}));
    run(&mut s, "track.velocity", json!({"ids": [t], "percent": 0}));
    s.save().unwrap();
    let json: Value = serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap())
        .expect("project.json");
    let saved = &json["project"]["tracks"][t.as_str()];
    assert!(saved.get("transpose").is_none(), "{saved}");
    assert!(saved.get("velocity").is_none(), "{saved}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_song_holds_values_from_another_writer_inside_the_ranges() {
    let base = |extra: Value| -> Value {
        let mut v =
            serde_json::to_value(Track::new(TrackId(1), TrackKind::Instrument, "a")).unwrap();
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                o.insert(k.clone(), v.clone());
            }
        }
        v
    };
    // Older songs have no fields.
    let t: Track = serde_json::from_value(base(json!({}))).unwrap();
    assert_eq!((t.transpose, t.velocity), (0, 0));
    let v = serde_json::to_value(&t).unwrap();
    assert!(v.get("transpose").is_none(), "{v}");
    assert!(v.get("velocity").is_none(), "{v}");
    // Out of the ranges is held at the ends.
    for (given, held) in [(90, 64), (-90, -64)] {
        let t: Track = serde_json::from_value(base(json!({ "transpose": given }))).unwrap();
        assert_eq!(t.transpose, held);
    }
    for (given, held) in [(300, 100), (-300, -100)] {
        let t: Track = serde_json::from_value(base(json!({ "velocity": given }))).unwrap();
        assert_eq!(t.velocity, held);
    }
}

#[test]
fn duplicating_a_track_copies_both_values() {
    let mut s = Session::new("track-copy");
    let (t, _) = midi(&mut s);
    run(&mut s, "track.transpose", json!({"ids": [t], "semis": 4}));
    run(&mut s, "track.velocity", json!({"ids": [t], "percent": 25}));
    for mode in ["all", "settings"] {
        let copy = id_of(&run(
            &mut s,
            "track.duplicate",
            json!({"ids": [t], "mode": mode}),
        ));
        let copy = track(s.project(), &copy);
        assert_eq!((copy.transpose, copy.velocity), (4, 25), "{mode}");
    }
}

#[test]
fn the_schema_shows_the_ranges_and_the_values_are_rounded() {
    let reg = xirius_core::Registry::standard();
    for (command, arg, min, max) in [
        ("track.transpose", "semis", -64.0, 64.0),
        ("track.velocity", "percent", -100.0, 100.0),
    ] {
        let schema = reg.get(command).expect(command).json_schema();
        let arg = &schema["inputSchema"]["properties"][arg];
        assert_eq!(arg["minimum"], min, "{command}");
        assert_eq!(arg["maximum"], max, "{command}");
        assert!(
            arg["description"]
                .as_str()
                .unwrap_or_default()
                .contains("範囲の外は丸めます"),
            "{command}: {arg}"
        );
    }
}

#[test]
fn the_velocity_is_kept_inside_the_keyboard() {
    assert_eq!(track_velocity(100, 0), 100);
    assert_eq!(track_velocity(100, 10), 110);
    assert_eq!(track_velocity(127, 100), 127);
    assert_eq!(track_velocity(1, -100), 1);
    assert_eq!(track_velocity(64, -50), 32);
}
