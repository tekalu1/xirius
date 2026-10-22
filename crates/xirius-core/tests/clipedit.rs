//! Clip editing: the boundary of two touching clips, and slipping a clip's
//! content (EDT-036, EDT-037).

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::id::ClipId;
use xirius_core::model::{ClipBody, Project};
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

fn two_touching(s: &mut Session) -> (String, String, String) {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    let a = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "a.wav"}),
    ));
    let b = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 960, "file": "b.wav"}),
    ));
    // give the right clip room to be pulled earlier
    run(s, "clip.slip", json!({"ids": [b], "by": -480}));
    (t, a, b)
}

#[test]
fn the_boundary_of_two_clips_moves_both_in_one_step() {
    let mut s = Session::new("adjacent");
    let (_, a, b) = two_touching(&mut s);
    let before = s.history().len();
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [a], "by": 240, "adjacent": true}),
    );
    // the left grows, the right starts later, and its content stays in place
    assert_eq!(shape(s.project(), &a), (0, 1200, 0));
    assert_eq!(shape(s.project(), &b), (1200, 720, 480 + 240));
    assert_eq!(s.history().len(), before + 1);

    run(
        &mut s,
        "clip.resize",
        json!({"ids": [a], "by": -600, "adjacent": true}),
    );
    assert_eq!(shape(s.project(), &a), (0, 600, 0));
    assert_eq!(shape(s.project(), &b), (600, 1320, 480 - 360));

    // one undo puts both back
    s.undo(1).unwrap();
    assert_eq!(shape(s.project(), &a), (0, 1200, 0));
    assert_eq!(shape(s.project(), &b), (1200, 720, 720));
}

#[test]
fn a_boundary_move_that_cannot_be_done_changes_nothing() {
    let mut s = Session::new("adjacent-fail");
    let (_, a, b) = two_touching(&mut s);
    // the right clip would be shorter than a tick
    assert!(
        try_run(
            &mut s,
            "clip.resize",
            json!({"ids": [a], "by": 960, "adjacent": true})
        )
        .is_err()
    );
    // its content would start before the file's head
    assert!(
        try_run(
            &mut s,
            "clip.resize",
            json!({"ids": [a], "by": -500, "adjacent": true})
        )
        .is_err()
    );
    assert_eq!(shape(s.project(), &a), (0, 960, 0));
    assert_eq!(shape(s.project(), &b), (960, 960, 480));

    // a clip with nothing at its end has no boundary
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let lone = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 480, "file": "c.wav"}),
    ));
    assert!(
        try_run(
            &mut s,
            "clip.resize",
            json!({"ids": [lone], "by": 10, "adjacent": true})
        )
        .is_err()
    );
    // without the flag nothing else moves
    run(&mut s, "clip.resize", json!({"ids": [a], "by": 100}));
    assert_eq!(shape(s.project(), &b), (960, 960, 480));
}

#[test]
fn slip_moves_the_content_and_not_the_frame() {
    let mut s = Session::new("slip");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 480, "length": 960, "file": "a.wav"}),
    ));
    // the file is read from 300 on; moving the content later reads earlier
    run(&mut s, "clip.slip", json!({"ids": [a], "by": -300}));
    assert_eq!(shape(s.project(), &a), (480, 960, 300));
    run(&mut s, "clip.slip", json!({"ids": [a], "by": 100}));
    assert_eq!(shape(s.project(), &a), (480, 960, 200));
    // it cannot be pulled out past the head of the file
    assert!(try_run(&mut s, "clip.slip", json!({"ids": [a], "by": 201})).is_err());
    assert_eq!(shape(s.project(), &a), (480, 960, 200));
    s.undo(1).unwrap();
    assert_eq!(shape(s.project(), &a), (480, 960, 300));
}

#[test]
fn slip_is_for_audio_only() {
    let mut s = Session::new("slip-midi");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 960}),
    ));
    assert!(try_run(&mut s, "clip.slip", json!({"ids": [m], "by": 10})).is_err());
}
