//! CTX-034: the shape of a controller point to the next one (`cc.point.shape`): a step or a
//! straight line, one history step, kept by the edits that move a point, and a plain step in the
//! song file.

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::model::{CtrlKind, CtrlPoint, CtrlShape};
use xirius_core::session::RunOptions;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn fails(s: &mut Session, name: &str, args: Value) -> String {
    match s.run_value(name, args, &RunOptions::human()) {
        Ok(_) => panic!("{name} should fail"),
        Err(e) => format!("{e:?}"),
    }
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

/// A song with a 2-bar MIDI clip holding a controller lane `lane` of three points.
fn song(lane: &str) -> (Session, String) {
    let mut s = Session::new("t");
    let t = id(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    let clip = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": "1.1.0", "bars": 2}),
    ));
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": lane, "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 10}, {"tick": 480, "value": 50}, {"tick": 960, "value": 90}]}),
    );
    (s, clip)
}

fn shapes(s: &Session, kind: CtrlKind) -> Vec<(i64, i32, CtrlShape)> {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.controllers.iter())
        .filter(|l| l.kind == kind)
        .flat_map(|l| l.points.iter())
        .map(|p| (p.tick, p.value, p.shape))
        .collect()
}

#[test]
fn a_point_holds_its_value_until_the_next_unless_it_is_made_a_line() {
    let (mut s, clip) = song("cc1");
    let kind = CtrlKind::Cc(1);
    assert!(shapes(&s, kind).iter().all(|p| p.2 == CtrlShape::Step));
    let steps = s.steps().len();
    run(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc1", "ticks": "0,480", "shape": "linear"}),
    );
    assert_eq!(s.steps().len(), steps + 1, "one history step");
    assert_eq!(
        shapes(&s, kind),
        [
            (0, 10, CtrlShape::Linear),
            (480, 50, CtrlShape::Linear),
            (960, 90, CtrlShape::Step)
        ]
    );
    s.undo(1).unwrap();
    assert!(shapes(&s, kind).iter().all(|p| p.2 == CtrlShape::Step));
    s.redo(1).unwrap();
    assert_eq!(shapes(&s, kind)[0].2, CtrlShape::Linear);
}

#[test]
fn the_last_point_has_no_next_one_and_a_missing_position_is_an_error() {
    let (mut s, clip) = song("cc1");
    let steps = s.steps().len();
    let said = fails(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc1", "ticks": "960", "shape": "linear"}),
    );
    assert!(said.contains("次のポイント"), "{said}");
    fails(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc1", "ticks": "123", "shape": "linear"}),
    );
    assert_eq!(s.steps().len(), steps, "a refused command leaves no step");
}

#[test]
fn the_sustain_pedal_is_a_step_only() {
    let (mut s, clip) = song("cc64");
    let said = fails(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc64", "ticks": "0", "shape": "linear"}),
    );
    assert!(said.contains("階段だけ"), "{said}");
    fails(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc64", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 127, "shape": "linear"}]}),
    );
    run(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc64", "ticks": "0", "shape": "step"}),
    );
}

#[test]
fn a_value_change_and_a_rewrite_keep_the_shape_of_the_points_that_stay() {
    let (mut s, clip) = song("cc1");
    let kind = CtrlKind::Cc(1);
    run(
        &mut s,
        "cc.point.shape",
        json!({"clip": clip, "lane": "cc1", "ticks": "480", "shape": "linear"}),
    );
    run(
        &mut s,
        "cc.point.set",
        json!({"clip": clip, "lane": "cc1", "ticks": "480", "value": 64}),
    );
    assert_eq!(shapes(&s, kind)[1], (480, 64, CtrlShape::Linear));
    // A rewrite that lists the shape writes it; one that leaves it out writes a step.
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 1, "shape": "linear"}, {"tick": 960, "value": 2}]}),
    );
    assert_eq!(
        shapes(&s, kind),
        [(0, 1, CtrlShape::Linear), (960, 2, CtrlShape::Step)]
    );
    let said = fails(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 1, "shape": "curved"}]}),
    );
    assert!(said.contains("curved"), "{said}");
}

#[test]
fn a_step_is_not_written_to_the_file_and_a_file_without_shapes_reads_as_steps() {
    let plain = CtrlPoint::new(480, 64);
    assert_eq!(
        serde_json::to_value(plain).unwrap(),
        json!({"tick": 480, "value": 64})
    );
    let line = CtrlPoint {
        shape: CtrlShape::Linear,
        ..plain
    };
    let written = serde_json::to_value(line).unwrap();
    assert_eq!(
        written,
        json!({"tick": 480, "value": 64, "shape": "linear"})
    );
    assert_eq!(serde_json::from_value::<CtrlPoint>(written).unwrap(), line);
    assert_eq!(
        serde_json::from_value::<CtrlPoint>(json!({"tick": 480, "value": 64})).unwrap(),
        plain
    );
    assert_eq!(
        line.to_json(),
        json!({"tick": 480, "value": 64, "shape": "linear"})
    );
    assert_eq!(plain.to_json(), json!({"tick": 480, "value": 64}));
}
