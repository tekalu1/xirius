//! A MIDI clip's transpose (UI-311): the clip keeps the value, the notes stay as written, the
//! value is added when the clip plays, and 「トランスポーズをノートに反映」 moves the notes
//! and clears the value in one step.

use serde_json::{Value, json};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, xirius_core::CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn clip(p: &Project, id: &str) -> Clip {
    p.clip(id.parse::<ClipId>().unwrap()).unwrap().1.clone()
}

fn pitches(p: &Project, id: &str) -> Vec<u8> {
    clip(p, id).notes().iter().map(|n| n.pitch).collect()
}

/// An instrument track with a MIDI clip of the three notes 60, 64 and 67.
fn midi(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({})));
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
    c
}

#[test]
fn the_value_stays_on_the_clip_and_the_notes_do_not_move() {
    let mut s = Session::new("transpose");
    let c = midi(&mut s);
    let before = pitches(s.project(), &c);
    assert_eq!(before, [60, 64, 67]);
    assert_eq!(clip(s.project(), &c).transpose(), 0);
    let n = s.history().len();

    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 2}));
    assert_eq!(clip(s.project(), &c).transpose(), 2);
    assert_eq!(pitches(s.project(), &c), before);
    // --by moves the value, and the value holds at the end of the range.
    run(&mut s, "clip.transpose", json!({"ids": [c], "by": -3}));
    assert_eq!(clip(s.project(), &c).transpose(), -1);
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 30}));
    assert_eq!(clip(s.project(), &c).transpose(), 24);
    run(&mut s, "clip.transpose", json!({"ids": [c], "by": -100}));
    assert_eq!(clip(s.project(), &c).transpose(), -24);
    // Back to 0 is the key the notes are written in.
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 0}));
    assert_eq!(clip(s.project(), &c).transpose(), 0);
    assert_eq!(pitches(s.project(), &c), before);
    // One step each time.
    assert_eq!(s.history().len(), n + 5);
}

#[test]
fn writing_it_into_the_notes_moves_them_and_clears_the_value_in_one_step() {
    let mut s = Session::new("write");
    let c = midi(&mut s);
    let before = pitches(s.project(), &c);
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 5}));
    let n = s.history().len();
    run(&mut s, "clip.transpose.write", json!({"ids": [c]}));
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(clip(s.project(), &c).transpose(), 0);
    assert_eq!(
        pitches(s.project(), &c),
        before.iter().map(|p| p + 5).collect::<Vec<_>>()
    );
    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 5);
    assert_eq!(pitches(s.project(), &c), before);
    // A value of 0 writes nothing.
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 0}));
    let n = s.history().len();
    let out = run(&mut s, "clip.transpose.write", json!({"ids": [c]}));
    assert!(
        out.message.contains("反映するものがありません"),
        "{}",
        out.message
    );
    assert_eq!(s.history().len(), n);
}

#[test]
fn a_note_the_value_would_take_off_the_keyboard_is_refused_and_nothing_changes() {
    let mut s = Session::new("range");
    let c = midi(&mut s);
    run(
        &mut s,
        "note.add",
        json!({"clip": c, "pitch": 120, "at": 1440, "length": 240}),
    );
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 12}));
    let before = (clip(s.project(), &c), pitches(s.project(), &c));
    assert!(try_run(&mut s, "clip.transpose.write", json!({"ids": [c]})).is_err());
    assert_eq!((clip(s.project(), &c), pitches(s.project(), &c)), before);
}

#[test]
fn an_audio_clip_keeps_its_own_transpose_and_a_midi_clip_is_not_tuned() {
    let mut s = Session::new("both");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "a.wav"}),
    ));
    let c = midi(&mut s);
    run(&mut s, "clip.transpose", json!({"ids": [a], "semis": 3}));
    let ClipBody::Audio { process, .. } = &clip(s.project(), &a).body else {
        panic!("not audio");
    };
    assert_eq!(process.semis, 3);
    // clip.tune stays for audio clips only.
    assert!(try_run(&mut s, "clip.tune", json!({"ids": [c], "cents": 1})).is_err());
    assert!(try_run(&mut s, "clip.transpose.write", json!({"ids": [a]})).is_err());
}

#[test]
fn the_value_goes_through_duplicating_splitting_and_pasting() {
    let mut s = Session::new("copies");
    let c = midi(&mut s);
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 4}));

    // Duplicating keeps the value (with notes of its own).
    let copy = id_of(&run(&mut s, "clip.duplicate", json!({"ids": [c]})));
    assert_ne!(copy, c);
    assert_eq!(clip(s.project(), &copy).transpose(), 4);

    // Both halves of a split keep it.
    let right = id_of(&run(&mut s, "clip.split", json!({"ids": [c], "at": 960})));
    assert_eq!(clip(s.project(), &c).transpose(), 4);
    assert_eq!(clip(s.project(), &right).transpose(), 4);

    // Paste keeps it.
    run(&mut s, "edit.copy", json!({"ids": [c]}));
    let pasted = id_of(&run(&mut s, "edit.paste", json!({"at": 3840})));
    assert_eq!(clip(s.project(), &pasted).transpose(), 4);
    assert_eq!(pitches(s.project(), &pasted), pitches(s.project(), &c));
}

#[test]
fn a_saved_song_keeps_the_value_and_a_song_without_it_reads_as_zero() {
    let dir = std::env::temp_dir().join(format!("xirius-clip-transpose-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new("save");
    let c = midi(&mut s);
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": -3}));
    s.save_as(&dir).unwrap();
    let json: Value = serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap())
        .expect("project.json");
    assert_eq!(
        json["project"]["tracks"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|t| t["clips"].as_array().cloned().unwrap_or_default())
            .find(|x| x["id"] == c)
            .unwrap()["transpose"],
        -3
    );
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), -3);
    drop(s);

    // A value of 0 writes nothing, as older songs have it.
    let mut s = Session::open(&dir).unwrap();
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 0}));
    s.save().unwrap();
    let json: Value = serde_json::from_slice(&std::fs::read(dir.join("project.json")).unwrap())
        .expect("project.json");
    let clip_json = json["project"]["tracks"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|t| t["clips"].as_array().cloned().unwrap_or_default())
        .find(|x| x["id"] == c)
        .unwrap();
    assert!(clip_json.get("transpose").is_none(), "{clip_json}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_song_holds_a_value_from_another_writer_inside_the_range() {
    let base = |extra: Value| -> Value {
        let mut v = json!({
            "id": "clp_1", "name": "a", "start": 0, "length": 960,
            "type": "midi", "notes": []
        });
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                o.insert(k.clone(), v.clone());
            }
        }
        v
    };
    // Older songs have no field.
    let c: Clip = serde_json::from_value(base(json!({}))).unwrap();
    assert_eq!(c.transpose(), 0);
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.get("transpose").is_none(), "{v}");
    // Out of the range is held at the end.
    for (given, held) in [(40, 24), (-40, -24), (3, 3)] {
        let c: Clip = serde_json::from_value(base(json!({ "transpose": given }))).unwrap();
        assert_eq!(c.transpose(), held);
    }
}

#[test]
fn joining_puts_each_clips_value_into_its_own_notes() {
    let mut s = Session::new("join");
    let c = midi(&mut s);
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 2}));
    let copy = id_of(&run(&mut s, "clip.duplicate", json!({"ids": [c]})));
    run(
        &mut s,
        "clip.transpose",
        json!({"ids": [copy], "semis": -1}),
    );
    let res = run(&mut s, "clip.join", json!({"ids": [c, copy]}));
    let joined = res.data["ids"][0].as_str().unwrap().to_string();
    assert_eq!(clip(s.project(), &joined).transpose(), 0);
    let mut pitches = pitches(s.project(), &joined);
    pitches.sort();
    assert_eq!(pitches, vec![59, 62, 63, 66, 66, 69]);
}

#[test]
fn the_schema_shows_the_range_and_holds_a_value_outside_it() {
    let reg = xirius_core::Registry::standard();
    let s = reg.get("clip.transpose").unwrap().json_schema();
    let semis = &s["inputSchema"]["properties"]["semis"];
    assert_eq!(semis["minimum"], -24.0, "{semis}");
    assert_eq!(semis["maximum"], 24.0, "{semis}");
    assert!(
        semis["description"]
            .as_str()
            .is_some_and(|d| d.contains("範囲の外は丸めます")),
        "{semis}"
    );
}
