//! A field turned again and again (a wheel over the transpose, UI-319): the turn is one step of
//! the history and one state on the disk, and the screen's edits do not write the whole song
//! out for `state_hash`.

use serde_json::{Value, json};
use xirius_core::id::ClipId;
use xirius_core::model::{Clip, Project};
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

fn clip(p: &Project, id: &str) -> Clip {
    p.clip(id.parse::<ClipId>().unwrap()).unwrap().1.clone()
}

/// An instrument track with a MIDI clip.
fn midi(s: &mut Session) -> String {
    let t = id_of(&run(s, "track.add.instrument", json!({})));
    id_of(&run(
        s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 1920}),
    ))
}

fn turn(s: &mut Session, c: &str, notches: usize) {
    s.begin_turn();
    for _ in 0..notches {
        run(s, "clip.transpose", json!({"ids": [c], "by": 1}));
    }
    s.end_group();
}

#[test]
fn a_turn_is_one_step_and_one_undo() {
    let mut s = Session::new("turn");
    let c = midi(&mut s);
    let steps = s.steps().len();

    turn(&mut s, &c, 12);
    assert_eq!(clip(s.project(), &c).transpose(), 12);
    assert_eq!(s.steps().len(), steps + 1);
    // The step says what the whole turn did.
    let last = s.steps().last().unwrap();
    assert_eq!(last.command, "clip.transpose");
    assert_eq!(last.args["by"], 12);

    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 0);
    s.redo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 12);

    // The next turn is a step of its own.
    turn(&mut s, &c, 3);
    assert_eq!(s.steps().len(), steps + 2);
    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 12);
}

#[test]
fn a_turn_after_an_undo_replaces_the_steps_ahead_once() {
    let mut s = Session::new("turn");
    let c = midi(&mut s);
    turn(&mut s, &c, 2);
    let steps = s.steps().len();
    s.undo(1).unwrap();

    turn(&mut s, &c, 5);
    assert_eq!(s.steps().len(), steps);
    assert_eq!(clip(s.project(), &c).transpose(), 5);
    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 0);
}

#[test]
fn a_group_that_is_not_a_turn_keeps_each_step() {
    let mut s = Session::new("group");
    let c = midi(&mut s);
    let steps = s.steps().len();
    s.begin_group();
    for _ in 0..3 {
        run(&mut s, "clip.transpose", json!({"ids": [c], "by": 1}));
    }
    s.end_group();
    assert_eq!(s.steps().len(), steps + 3);
    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 0);
}

#[test]
fn another_command_in_a_turn_is_a_step_of_its_own() {
    let mut s = Session::new("turn");
    let c = midi(&mut s);
    let steps = s.steps().len();
    s.begin_turn();
    run(&mut s, "clip.transpose", json!({"ids": [c], "by": 1}));
    run(&mut s, "clip.transpose", json!({"ids": [c], "semis": 4}));
    run(&mut s, "clip.rename", json!({"id": c, "name": "B"}));
    s.end_group();
    assert_eq!(s.steps().len(), steps + 2);
    // An absolute value stays as it was given.
    assert_eq!(s.steps()[steps].args["semis"], 4);
    s.undo(1).unwrap();
    assert_eq!(clip(s.project(), &c).transpose(), 0);
}

#[test]
fn the_screens_edits_go_without_state_hash() {
    let mut s = Session::new("hash");
    let c = midi(&mut s);
    let r = run(&mut s, "clip.transpose", json!({"ids": [c], "by": 1}));
    assert!(r.data["state_hash"]["after"].is_string(), "{:?}", r.data);

    s.hash_people_edits(false);
    let r = run(&mut s, "clip.transpose", json!({"ids": [c], "by": 1}));
    assert!(r.data.is_object(), "{:?}", r.data);
    assert!(r.data.get("state_hash").is_none(), "{:?}", r.data);
    let mut dry = RunOptions::human();
    dry.dry_run = true;
    let r = s
        .run_value("clip.transpose", json!({"ids": [c], "by": 1}), &dry)
        .unwrap();
    assert!(r.data.get("state_hash").is_none(), "{:?}", r.data);

    // The AI's edits still answer with it.
    let r = s
        .run_value(
            "clip.transpose",
            json!({"ids": [c], "by": 1}),
            &RunOptions::ai("test"),
        )
        .unwrap();
    assert!(r.data["state_hash"]["after"].is_string(), "{:?}", r.data);
}
