//! The group edits of the piano roll: velocity humanize and transform, strum, split on the grid,
//! and the quantize programs (EDT-022, EDT-054, EDT-060, EDT-076).

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::session::RunOptions;

/// (start, length, pitch, velocity) of every note, in order.
fn notes(s: &Session) -> Vec<(i64, i64, u64, u64)> {
    let mut v: Vec<_> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.notes().iter())
        .map(|n| (n.start, n.length, u64::from(n.pitch), u64::from(n.velocity)))
        .collect();
    v.sort();
    v
}

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn rig(put: Value) -> (Session, String) {
    let mut s = Session::new("t");
    let t = run(&mut s, "track.add.instrument", json!({"name": "T"}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let clip = run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": "1.1.0", "bars": 2}),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(&mut s, "note.put", json!({"clip": clip, "notes": put}));
    (s, clip)
}

#[test]
fn velocity_humanize_moves_only_the_velocities_and_stays_inside_the_amount() {
    let (mut s, clip) = rig(json!([
        {"at": 0, "pitch": 60, "length": 240, "velocity": 64},
        {"at": 480, "pitch": 62, "length": 240, "velocity": 64},
        {"at": 960, "pitch": 64, "length": 240, "velocity": 64},
    ]));
    run(
        &mut s,
        "note.velocity.humanize",
        json!({"ids": [clip], "amount": 6, "seed": 7}),
    );
    let after = notes(&s);
    assert_eq!(
        after.iter().map(|n| (n.0, n.1, n.2)).collect::<Vec<_>>(),
        vec![(0, 240, 60), (480, 240, 62), (960, 240, 64)]
    );
    assert!(after.iter().all(|n| (58..=70).contains(&n.3)), "{after:?}");
    assert!(after.iter().any(|n| n.3 != 64), "{after:?}");
}

#[test]
fn velocity_transform_scales_about_the_mean_and_tilts_along_time() {
    let (mut s, clip) = rig(json!([
        {"at": 0, "pitch": 60, "length": 240, "velocity": 40},
        {"at": 480, "pitch": 60, "length": 240, "velocity": 80},
        {"at": 960, "pitch": 60, "length": 240, "velocity": 120},
    ]));
    run(
        &mut s,
        "note.velocity.transform",
        json!({"ids": [clip], "scale": 50}),
    );
    let v: Vec<u64> = notes(&s).iter().map(|n| n.3).collect();
    assert_eq!(v, vec![60, 80, 100]);
    run(
        &mut s,
        "note.velocity.transform",
        json!({"ids": [clip], "tilt": 20}),
    );
    let v: Vec<u64> = notes(&s).iter().map(|n| n.3).collect();
    assert_eq!(v, vec![60, 90, 120]);
}

#[test]
fn strum_spreads_a_chord_from_the_lowest_note_and_the_other_way_when_negative() {
    let chord = json!([
        {"at": 480, "pitch": 67, "length": 480, "velocity": 90},
        {"at": 480, "pitch": 60, "length": 480, "velocity": 90},
        {"at": 480, "pitch": 64, "length": 480, "velocity": 90},
    ]);
    let (mut s, clip) = rig(chord.clone());
    run(&mut s, "note.strum", json!({"ids": [clip], "spread": 60}));
    let by_pitch: Vec<(u64, i64, i64)> = {
        let mut v: Vec<_> = notes(&s).iter().map(|n| (n.2, n.0, n.1)).collect();
        v.sort();
        v
    };
    assert_eq!(
        by_pitch,
        vec![(60, 480, 480), (64, 510, 450), (67, 540, 420)]
    );

    let (mut s, clip) = rig(chord);
    run(&mut s, "note.strum", json!({"ids": [clip], "spread": -60}));
    let mut v: Vec<_> = notes(&s).iter().map(|n| (n.2, n.0)).collect();
    v.sort();
    assert_eq!(v, vec![(60, 540), (64, 510), (67, 480)]);
    assert!(
        s.run_value(
            "note.strum",
            json!({"ids": [clip], "spread": 10}),
            &RunOptions::human()
        )
        .is_err(),
        "notes apart in time are not a chord"
    );
}

#[test]
fn split_on_the_grid_cuts_a_note_at_every_line_it_crosses() {
    let (mut s, clip) = rig(json!([
        {"at": 100, "pitch": 60, "length": 800, "velocity": 90},
        {"at": 1000, "pitch": 62, "length": 200, "velocity": 90},
    ]));
    run(
        &mut s,
        "note.split.grid",
        json!({"ids": [clip], "grid": 480}),
    );
    let n = notes(&s);
    let of = |pitch: u64| -> Vec<(i64, i64)> {
        n.iter()
            .filter(|x| x.2 == pitch)
            .map(|x| (x.0, x.1))
            .collect()
    };
    assert_eq!(of(60), vec![(100, 380), (480, 420)]);
    assert_eq!(of(62), vec![(1000, 200)], "a note crossing no line stays");
}

#[test]
fn the_quantize_programs_switch_the_grid_and_keep_what_is_saved_over_them() {
    let mut s = Session::new("t");
    let out = run(&mut s, "quantize.program", json!({"program": "A"}));
    assert_eq!(out["grid"], "1/4");
    let out = run(&mut s, "quantize.program", json!({"program": "G"}));
    assert_eq!(out["grid"], "1/16");
    assert_eq!(s.state.view.grid, "1/16");
    assert_eq!(s.state.view.quantize["swing"], 50);
    s.state.view.grid = "1/32".into();
    run(
        &mut s,
        "quantize.program",
        json!({"program": "B", "save": true}),
    );
    let out = run(&mut s, "quantize.program", json!({"program": "B"}));
    assert_eq!(out["grid"], "1/32");
}
