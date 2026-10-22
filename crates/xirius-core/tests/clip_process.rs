//! Tempo, speed and pitch of audio clips (EDT-007, EDT-016) and the song's tempo moving the
//! clips that do not follow it.

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::id::ClipId;
use xirius_core::model::{AudioProcess, Clip, ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

/// (start, length, offset) of an audio clip.
fn shape(p: &Project, id: &str) -> (i64, i64, i64) {
    let (_, c) = p.clip(id.parse::<ClipId>().unwrap()).unwrap();
    let ClipBody::Audio { offset, .. } = &c.body else {
        panic!("not audio");
    };
    (c.start, c.length, *offset)
}

fn process(p: &Project, id: &str) -> AudioProcess {
    let (_, c) = p.clip(id.parse::<ClipId>().unwrap()).unwrap();
    let ClipBody::Audio { process, .. } = &c.body else {
        panic!("not audio");
    };
    *process
}

/// An audio clip at bar 2, four beats long, starting a beat into its source.
fn clip(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    let id = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 1920, "length": 1920, "file": "a.wav"}),
    ));
    run(s, "clip.slip", json!({"ids": [id], "by": -480}));
    id
}

#[test]
fn the_speed_keeps_the_same_part_of_the_source_and_is_one_step() {
    let mut s = Session::new("speed");
    let a = clip(&mut s);
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));
    let n = s.history().len();
    run(&mut s, "clip.speed.set", json!({"ids": [a], "percent": 50}));
    assert_eq!(s.history().len(), n + 1);
    assert_eq!(process(s.project(), &a).speed, 50.0);
    // Half as fast: twice as long, and the offset too.
    assert_eq!(shape(s.project(), &a), (1920, 3840, 960));
    run(
        &mut s,
        "clip.speed.set",
        json!({"ids": [a], "percent": 200}),
    );
    assert_eq!(shape(s.project(), &a), (1920, 960, 240));
    run(
        &mut s,
        "clip.speed.set",
        json!({"ids": [a], "percent": 100}),
    );
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));
    assert!(process(s.project(), &a).is_plain());
    let e = try_run(
        &mut s,
        "clip.speed.set",
        json!({"ids": [a], "percent": 300}),
    );
    assert!(e.is_err());
}

#[test]
fn following_the_tempo_takes_the_songs_tempo_and_then_keeps_the_beats() {
    let mut s = Session::new("follow");
    let a = clip(&mut s);
    let b = clip(&mut s);
    run(&mut s, "clip.stretch", json!({"ids": [a], "follow": "on"}));
    let x = process(s.project(), &a);
    assert!(x.follow);
    assert_eq!(x.tempo, Some(120.0));
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));

    // The song slows to 60: the following clip keeps four beats, the other keeps two seconds.
    let n = s.history().len();
    run(&mut s, "tempo.set", json!({"bpm": 60}));
    assert_eq!(s.history().len(), n + 1, "one step");
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));
    assert_eq!(shape(s.project(), &b), (1920, 960, 240));
    // Undo puts both back.
    s.undo(1).unwrap();
    assert_eq!(shape(s.project(), &b), (1920, 1920, 480));

    // Leaving the song's tempo keeps the source's part: at 60 the source at 120 runs twice as
    // fast as the clip did.
    run(&mut s, "tempo.set", json!({"bpm": 60}));
    run(&mut s, "clip.stretch", json!({"ids": [a], "follow": "off"}));
    assert_eq!(shape(s.project(), &a), (1920, 960, 240));
    // Left out, it flips.
    run(&mut s, "clip.stretch", json!({"ids": [a]}));
    assert!(process(s.project(), &a).follow);
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));
}

#[test]
fn the_tempo_of_a_following_clip_sets_its_length() {
    let mut s = Session::new("tempo");
    let a = clip(&mut s);
    run(&mut s, "clip.stretch", json!({"ids": [a], "follow": "on"}));
    // Source at 60 in a song at 120: played twice as fast, half as long.
    run(&mut s, "clip.tempo.set", json!({"ids": [a], "bpm": 60}));
    assert_eq!(process(s.project(), &a).tempo, Some(60.0));
    assert_eq!(shape(s.project(), &a), (1920, 960, 240));
    let e = try_run(&mut s, "clip.tempo.set", json!({"ids": [a], "bpm": 0}));
    assert!(e.is_err());
}

#[test]
fn transposing_and_tuning_keep_the_length() {
    let mut s = Session::new("pitch");
    let a = clip(&mut s);
    let b = clip(&mut s);
    run(&mut s, "clip.transpose", json!({"ids": [a], "semis": 3}));
    run(&mut s, "clip.transpose", json!({"ids": [a, b], "by": 1}));
    assert_eq!(process(s.project(), &a).semis, 4);
    assert_eq!(process(s.project(), &b).semis, 1);
    run(&mut s, "clip.transpose", json!({"ids": [a], "by": 30}));
    assert_eq!(process(s.project(), &a).semis, 24, "held at the end");
    run(&mut s, "clip.tune", json!({"ids": [a], "cents": -15}));
    assert_eq!(process(s.project(), &a).cents, -15);
    assert_eq!(shape(s.project(), &a), (1920, 1920, 480));
    assert!(try_run(&mut s, "clip.tune", json!({"ids": [a], "cents": 101})).is_err());
    assert!(
        try_run(
            &mut s,
            "clip.tune",
            json!({"ids": [a], "cents": 1, "by": 1})
        )
        .is_err()
    );

    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 960}),
    ));
    // A MIDI clip keeps the value on the clip (UI-311).
    run(&mut s, "clip.transpose", json!({"ids": [m], "semis": 1}));
    assert_eq!(transpose(s.project(), &m), 1);
    assert!(try_run(&mut s, "clip.tune", json!({"ids": [m], "cents": 1})).is_err());
}

fn transpose(p: &Project, id: &str) -> i8 {
    let (_, c) = p.clip(id.parse::<ClipId>().unwrap()).unwrap();
    c.transpose()
}

#[test]
fn a_clip_from_an_older_song_plays_as_it_is() {
    let c: Clip = serde_json::from_value(json!({
        "id": "clp_1", "name": "a", "start": 0, "length": 960,
        "type": "audio", "file": "a.wav", "offset": 0, "gain_db": 0.0
    }))
    .unwrap();
    let ClipBody::Audio { process, .. } = &c.body else {
        panic!("not audio");
    };
    assert!(process.is_plain());
    // A plain clip writes nothing new.
    let v = serde_json::to_value(&c).unwrap();
    assert!(v.get("process").is_none(), "{v}");
}

#[test]
fn the_settings_are_kept_through_saving_and_opening() {
    let dir = std::env::temp_dir().join(format!("xirius-clip-process-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Session::new("save");
    let a = clip(&mut s);
    run(&mut s, "clip.stretch", json!({"ids": [a], "follow": "on"}));
    run(&mut s, "clip.tempo.set", json!({"ids": [a], "bpm": 96.5}));
    run(&mut s, "clip.speed.set", json!({"ids": [a], "percent": 75}));
    run(&mut s, "clip.transpose", json!({"ids": [a], "semis": -5}));
    run(&mut s, "clip.tune", json!({"ids": [a], "cents": 12}));
    let want = process(s.project(), &a);
    let shape_before = shape(s.project(), &a);
    s.save_as(&dir).unwrap();
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(process(s.project(), &a), want);
    assert_eq!(shape(s.project(), &a), shape_before);
    let _ = std::fs::remove_dir_all(&dir);
}
