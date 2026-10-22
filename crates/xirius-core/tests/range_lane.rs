//! UI-241, AUT-010: a stretch of one automation lane or of the tempo row is made a line, a curve
//! or a flat run, moved up or down, or scaled and tilted, with the line outside left alone.
use serde_json::{Value, json};
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

/// A pan lane that zigzags between -0.5 and 0.5 every 480 ticks, from 0 to 1920.
fn song() -> (Session, String) {
    let mut s = Session::new("range-lane");
    let t = run(&mut s, "track.add.audio", json!({}))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let points: Vec<Value> = (0..=4)
        .map(|i| json!({"tick": i * 480, "value": if i % 2 == 0 { -0.5 } else { 0.5 }}))
        .collect();
    run(
        &mut s,
        "auto.draw",
        json!({"track": t, "target": "pan", "shape": "free", "from": 0, "to": 1920,
               "points": points, "step": 480}),
    );
    (s, t)
}

/// The points of the pan lane up to tick 1920, values rounded to hundredths.
fn lane(s: &Session, t: &str) -> Vec<(i64, f64)> {
    s.project().tracks[&t.parse().unwrap()].automation[0]
        .points
        .iter()
        .filter(|p| p.tick <= 1920)
        .map(|p| (p.tick, (p.value * 100.0).round() / 100.0))
        .collect()
}

fn span(t: &str, from: i64, to: i64) -> Value {
    json!({"track": t, "target": "pan", "from": from, "to": to})
}

fn with(mut base: Value, extra: Value) -> Value {
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

#[test]
fn a_line_replaces_the_points_between_the_ends_and_leaves_the_outside() {
    let (mut s, t) = song();
    let steps = s.steps().len();
    run(
        &mut s,
        "range.shape",
        with(span(&t, 480, 1440), json!({"shape": "line"})),
    );
    assert_eq!(s.steps().len(), steps + 1);
    let now = lane(&s, &t);
    assert!(now.contains(&(480, 0.5)) && now.contains(&(1440, 0.5)));
    assert!(!now.iter().any(|p| p.0 == 960 || p.0 == 1200), "{now:?}");
    assert!(now.contains(&(0, -0.5)) && now.contains(&(1920, -0.5)));
    s.undo(1).unwrap();
    assert_eq!(lane(&s, &t).len(), 5);
}

#[test]
fn flat_holds_one_value_and_a_curve_bends_the_first_segment() {
    let (mut s, t) = song();
    run(
        &mut s,
        "range.shape",
        with(span(&t, 480, 1440), json!({"shape": "flat"})),
    );
    let now = lane(&s, &t);
    assert!(
        now.contains(&(480, 0.5)) && now.contains(&(1440, 0.5)),
        "{now:?}"
    );
    let (mut s, t) = song();
    run(
        &mut s,
        "range.shape",
        with(span(&t, 0, 480), json!({"shape": "curve", "curve": 0.7})),
    );
    let first = s.project().tracks[&t.parse().unwrap()].automation[0].points[0];
    assert_eq!((first.tick, first.curve), (0, 0.7));
}

#[test]
fn a_trim_moves_every_value_in_the_stretch_and_stops_at_the_lane_ends() {
    let (mut s, t) = song();
    // A quarter of the lane's height is half a unit of pan.
    run(
        &mut s,
        "range.trim",
        with(span(&t, 480, 1440), json!({"delta": 0.25})),
    );
    let now = lane(&s, &t);
    assert!(now.contains(&(480, 1.0)), "{now:?}");
    assert!(now.contains(&(960, 0.0)), "{now:?}");
    assert!(now.contains(&(1440, 1.0)), "{now:?}");
    // The line outside is the old one.
    assert!(now.contains(&(0, -0.5)) && now.contains(&(1920, -0.5)));
}

#[test]
fn a_transform_scales_about_the_middle_and_tilts_toward_the_end() {
    let (mut s, t) = song();
    run(
        &mut s,
        "range.transform",
        with(span(&t, 0, 1920), json!({"scale": 0.5})),
    );
    let now = lane(&s, &t);
    assert_eq!(now[0], (0, -0.25), "{now:?}");
    assert!(now.contains(&(480, 0.25)), "{now:?}");
    let (mut s, t) = song();
    run(
        &mut s,
        "range.transform",
        with(span(&t, 0, 1920), json!({"tilt": 0.2})),
    );
    let now = lane(&s, &t);
    assert_eq!(now[0], (0, -0.5), "{now:?}");
    assert!(now.contains(&(1920, -0.1)), "{now:?}");
}

#[test]
fn the_tempo_row_takes_a_line_a_flat_run_and_a_trim_and_keeps_its_first_point() {
    let mut s = Session::new("range-tempo");
    run(&mut s, "tempo.set", json!({"at": 960, "bpm": 160}));
    run(&mut s, "tempo.set", json!({"at": 1920, "bpm": 100}));
    let bpms = |s: &Session| {
        s.project()
            .tempo
            .iter()
            .map(|t| (t.tick, t.bpm.round()))
            .collect::<Vec<_>>()
    };
    run(
        &mut s,
        "range.shape",
        json!({"tempo": true, "from": 0, "to": 1920, "shape": "line"}),
    );
    assert_eq!(bpms(&s), [(0, 120.0), (1920, 100.0)]);
    run(
        &mut s,
        "range.shape",
        json!({"tempo": true, "from": 0, "to": 960, "shape": "flat", "value": 90}),
    );
    assert_eq!(bpms(&s)[0], (0, 90.0));
    run(
        &mut s,
        "range.trim",
        json!({"tempo": true, "from": 480, "to": 960, "delta": 10}),
    );
    let now = bpms(&s);
    assert!(
        now.contains(&(480, 100.0)) && now.contains(&(960, 100.0)),
        "{now:?}"
    );
    assert!(
        s.run_value(
            "range.shape",
            json!({"tempo": true, "from": 0, "to": 960, "shape": "curve"}),
            &RunOptions::human()
        )
        .is_err()
    );
}

#[test]
fn a_lane_that_is_not_there_or_a_backwards_stretch_is_refused() {
    let (mut s, t) = song();
    for args in [
        with(span(&t, 960, 960), json!({"shape": "line"})),
        json!({"track": t, "target": "volume", "from": 0, "to": 480, "shape": "line"}),
    ] {
        assert!(
            s.run_value("range.shape", args, &RunOptions::human())
                .is_err()
        );
    }
}

#[test]
fn the_preview_of_a_drag_is_what_the_command_leaves() {
    use xirius_core::commands::{RangeMove, moved_lane, moved_tempo};
    for (how, name, args) in [
        (RangeMove::Trim(0.2), "range.trim", json!({"delta": 0.2})),
        (
            RangeMove::Transform {
                scale: 0.5,
                tilt: 0.0,
            },
            "range.transform",
            json!({"scale": 0.5}),
        ),
        (
            RangeMove::Transform {
                scale: 1.0,
                tilt: -0.1,
            },
            "range.transform",
            json!({"tilt": -0.1}),
        ),
    ] {
        let (mut s, t) = song();
        let tid = t.parse().unwrap();
        let track = &s.project().tracks[&tid];
        let before = track.automation[0].clone();
        let fallback = before.target.stored_value(track);
        let shown = moved_lane(&before, 240, 1200, how, fallback).unwrap();
        run(&mut s, name, with(span(&t, 240, 1200), args));
        let after = &s.project().tracks[&tid].automation[0];
        assert_eq!(*shown.points, *after.points, "{name}");
    }

    let mut s = Session::new("range-tempo-preview");
    run(&mut s, "tempo.set", json!({"at": 960, "bpm": 160}));
    let how = RangeMove::Transform {
        scale: 1.5,
        tilt: 8.0,
    };
    let shown = moved_tempo(&s.project().tempo, 480, 1440, how);
    run(
        &mut s,
        "range.transform",
        json!({"tempo": true, "from": 480, "to": 1440, "scale": 1.5, "tilt": 8.0}),
    );
    let key = |t: &[xirius_core::model::TempoPoint]| {
        t.iter()
            .map(|t| (t.tick, t.bpm, t.ramp))
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&shown), key(&s.project().tempo));
}
