//! UI-241: the content of a range is deleted or duplicated as a whole, with the clips cut at both
//! ends of the range (one history step each).
use serde_json::{Value, json};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

/// Two tracks, each with a clip from 0 to 960.
fn song() -> (Session, Vec<String>) {
    let mut s = Session::new("range-edit");
    let mut clips = Vec::new();
    for _ in 0..2 {
        let t = run(&mut s, "track.add.audio", json!({}))["id"]
            .as_str()
            .unwrap()
            .to_string();
        let c = run(
            &mut s,
            "clip.add.audio",
            json!({"track": t, "at": 0, "length": 960, "file": "take.wav"}),
        );
        clips.push(c["id"].as_str().unwrap().to_string());
    }
    (s, clips)
}

/// Every clip as (start, length), in time order.
fn spans(s: &Session) -> Vec<(i64, i64)> {
    let mut v: Vec<_> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.start, c.length)))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn deleting_a_range_cuts_the_clips_at_its_ends_and_removes_what_lies_inside() {
    let (mut s, clips) = song();
    let steps = s.steps().len();
    run(
        &mut s,
        "range.delete",
        json!({"ids": clips, "start": 240, "end": 480}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(spans(&s), [(0, 240), (0, 240), (480, 480), (480, 480)]);
    s.undo(1).unwrap();
    assert_eq!(spans(&s), [(0, 960), (0, 960)]);
}

#[test]
fn duplicating_a_range_copies_what_lies_inside_to_just_after_it() {
    let (mut s, clips) = song();
    let first = clips[0].clone();
    run(
        &mut s,
        "range.duplicate",
        json!({"ids": [first], "start": 240, "end": 480}),
    );
    assert_eq!(
        spans(&s),
        [(0, 240), (0, 960), (240, 240), (480, 240), (480, 480)]
    );
}

#[test]
fn a_range_may_be_named_by_two_positions_or_refused_when_backwards() {
    let (mut s, clips) = song();
    run(
        &mut s,
        "range.delete",
        json!({"ids": [clips[0].clone()], "range": "1.1.240..480"}),
    );
    assert_eq!(spans(&s), [(0, 240), (0, 960), (480, 480)]);
    assert!(
        s.run_value(
            "range.delete",
            json!({"ids": [clips[1].clone()], "start": 480, "end": 240}),
            &RunOptions::human()
        )
        .is_err()
    );
}

#[test]
fn copying_a_range_leaves_the_song_alone_and_pastes_the_cut_pieces() {
    let (mut s, clips) = song();
    let steps = s.steps().len();
    run(
        &mut s,
        "range.copy",
        json!({"ids": clips, "start": 240, "end": 480}),
    );
    assert_eq!(s.steps().len(), steps);
    assert_eq!(spans(&s), [(0, 960), (0, 960)]);
    assert_eq!(run(&mut s, "edit.clipboard", json!({}))["count"], 2);
    run(&mut s, "edit.paste", json!({"at": 1920}));
    assert_eq!(spans(&s), [(0, 960), (0, 960), (1920, 240), (1920, 240)]);
}

#[test]
fn cutting_a_range_removes_it_in_one_step_and_puts_it_on_the_clipboard() {
    let (mut s, clips) = song();
    let steps = s.steps().len();
    run(
        &mut s,
        "range.cut",
        json!({"ids": clips, "start": 240, "end": 480}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(spans(&s), [(0, 240), (0, 240), (480, 480), (480, 480)]);
    assert_eq!(run(&mut s, "edit.clipboard", json!({}))["count"], 2);
    run(&mut s, "edit.paste", json!({"at": 240}));
    assert_eq!(
        spans(&s),
        [
            (0, 240),
            (0, 240),
            (240, 240),
            (240, 240),
            (480, 480),
            (480, 480)
        ]
    );
}

#[test]
fn a_range_with_no_clip_in_it_has_nothing_to_copy() {
    let (mut s, clips) = song();
    for name in ["range.copy", "range.cut"] {
        assert!(
            s.run_value(
                name,
                json!({"ids": clips, "start": 2000, "end": 2400}),
                &RunOptions::human()
            )
            .is_err()
        );
    }
}
