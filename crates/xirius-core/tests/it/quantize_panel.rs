//! `note.quantize` with the quantize panel's settings (EDT-073), through the session.

use serde_json::{Value, json};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .to_string()
}

/// A clip at bar 2 with notes starting at `starts` (ticks into the clip).
fn clip(s: &mut Session, starts: &[(i64, u8)]) -> (String, Vec<String>) {
    let t = id(&run(s, "track.add.instrument", json!({ "name": "Keys" })));
    let c = id(&run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 3840, "bars": 1 }),
    ));
    let notes = starts
        .iter()
        .map(|(at, v)| {
            id(&run(
                s,
                "note.add",
                json!({ "clip": c, "at": 3840 + at, "pitch": 60, "length": 100, "velocity": v }),
            ))
        })
        .collect();
    (c, notes)
}

fn notes(s: &Session, c: &str) -> Vec<(i64, i64, u8)> {
    let (_, c) = s.project().clip(c.parse().unwrap()).unwrap();
    let mut v: Vec<_> = c
        .notes()
        .iter()
        .map(|n| (n.start, n.length, n.velocity))
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn without_the_panels_settings_the_quantize_is_the_plain_one() {
    let mut s = Session::new("q");
    let (c, _) = clip(&mut s, &[(120, 90), (250, 90)]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/16" }),
    );
    assert_eq!(notes(&s, &c), [(240, 100, 90), (240, 100, 90)]);
}

#[test]
fn tuplets_swing_range_and_ends_come_from_the_panel_and_undo_in_one_step() {
    let mut s = Session::new("q");
    let (c, ids) = clip(&mut s, &[(200, 90), (290, 90), (700, 90)]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [ids[0]], "grid": "1/16", "tuplet": 5 }),
    );
    assert_eq!(notes(&s, &c)[0], (192, 100, 90));
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [ids[1]], "grid": "1/16", "swing": 50 }),
    );
    assert_eq!(notes(&s, &c)[1], (300, 100, 90));
    // 700 is 20 from 720, but only 10 % of 120 may move.
    let before = notes(&s, &c);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [ids[2]], "grid": "1/16", "range": 10 }),
    );
    assert_eq!(notes(&s, &c), before);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [ids[2]], "grid": "1/16", "end": 100, "strength": 50 }),
    );
    // The start halfway to 720; the end, 800, all the way to its nearest line, 720.
    assert_eq!(notes(&s, &c)[2], (710, 10, 90));
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c), before);
}

#[test]
fn a_groove_from_another_clip_moves_starts_and_velocities() {
    let mut s = Session::new("q");
    let (groove, _) = clip(&mut s, &[(0, 120), (1000, 40)]);
    let (c, _) = clip(&mut s, &[(30, 100), (980, 100)]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/16", "groove": groove, "velocity": 100 }),
    );
    assert_eq!(notes(&s, &c), [(0, 100, 120), (1000, 100, 40)]);
    let track = s
        .project()
        .clip(c.parse().unwrap())
        .unwrap()
        .0
        .id
        .to_string();
    let empty = id(&run(
        &mut s,
        "clip.add.midi",
        json!({ "track": track, "at": 7680, "bars": 1 }),
    ));
    let err = s
        .run_value(
            "note.quantize",
            json!({ "ids": [c], "groove": empty }),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("グルーヴ"), "{err}");
}
