//! UI-128: the pencil's shape (`tool.draw.shape`) and the strokes it lays down in an automation
//! lane, a controller lane and the tempo row (`auto.draw`, `cc.draw`, `tempo.draw`).

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::model::CtrlKind;
use xirius_core::session::RunOptions;
use xirius_core::shape::Shape;

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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("xirius-draw-shape-{}", std::process::id()));
    let dir = base.join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&base).unwrap();
    dir
}

#[test]
fn the_pencil_shape_is_chosen_by_name_and_cycled_without_a_history_step() {
    let mut s = Session::new("t");
    assert_eq!(s.state.view.draw_shape, Shape::Free);
    let steps = s.steps().len();
    run(&mut s, "tool.draw.shape", json!({"shape": "sine"}));
    assert_eq!(s.state.view.draw_shape, Shape::Sine);
    run(&mut s, "tool.draw.shape", json!({"next": true}));
    assert_eq!(s.state.view.draw_shape, Shape::Transform);
    run(&mut s, "tool.draw.shape", json!({"next": true}));
    assert_eq!(s.state.view.draw_shape, Shape::Free);
    run(&mut s, "tool.draw.shape", json!({"prev": true}));
    assert_eq!(s.state.view.draw_shape, Shape::Transform);
    fails(&mut s, "tool.draw.shape", json!({}));
    fails(&mut s, "tool.draw.shape", json!({"shape": "zigzag"}));
    assert_eq!(s.steps().len(), steps);
}

#[test]
fn the_pencil_shape_survives_save_and_reopen_and_an_old_view_still_opens() {
    let mut s = Session::new("t");
    run(&mut s, "tool.draw.shape", json!({"shape": "saw"}));
    let dir = temp_dir("shape-save");
    s.save_as(&dir).unwrap();
    drop(s);
    let reopened = Session::open(&dir).unwrap();
    assert_eq!(reopened.state.view.draw_shape, Shape::Saw);
    drop(reopened);
    // A view written before the shape existed has no such key.
    let path = dir.join("view.json");
    let mut view: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    view.as_object_mut().unwrap().remove("draw_shape").unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&view).unwrap()).unwrap();
    let old = Session::open(&dir).unwrap();
    assert_eq!(old.state.view.draw_shape, Shape::Free);
}

#[test]
fn a_stroke_in_a_controller_lane_is_one_step_with_a_value_per_grid_step() {
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
    let steps = s.steps().len();
    run(
        &mut s,
        "cc.draw",
        json!({"clip": clip, "lane": "cc1", "shape": "line", "from": 0, "to": 960,
               "value": 0, "end-value": 120, "step": 240}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    let lane = |s: &Session| {
        let p = s.project();
        let c = p.tracks.values().flat_map(|t| &t.clips).next().unwrap();
        c.controllers
            .iter()
            .find(|l| l.kind == CtrlKind::parse("cc1").unwrap())
            .unwrap()
            .points
            .iter()
            .map(|q| (q.tick, q.value))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lane(&s),
        vec![(0, 0), (240, 30), (480, 60), (720, 90), (960, 120)]
    );
    // A square wave: two values, one per half period, held by steps.
    run(
        &mut s,
        "cc.draw",
        json!({"clip": clip, "lane": "cc1", "shape": "square", "from": 0, "to": 960,
               "value": 64, "amp": 32, "period": 480, "step": 240}),
    );
    assert_eq!(
        lane(&s),
        vec![(0, 96), (240, 32), (480, 96), (720, 32), (960, 96)]
    );
    s.undo(1).unwrap();
    assert_eq!(lane(&s).len(), 5);
    assert_eq!(lane(&s)[1], (240, 30));
}

#[test]
fn a_freehand_stroke_is_thinned_in_an_automation_lane_and_read_at_the_grid_with_a_step() {
    let mut s = Session::new("t");
    let t = id(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    let points: Vec<Value> = (0..=20)
        .map(|i| {
            let v = if i <= 10 {
                i as f64 / 10.0
            } else {
                (20 - i) as f64 / 10.0
            };
            json!({"tick": i * 48, "value": v * 0.8 - 0.4})
        })
        .collect();
    run(
        &mut s,
        "auto.draw",
        json!({"track": t, "target": "pan", "shape": "free", "from": 0, "to": 960,
               "points": points}),
    );
    let lane_points = |s: &Session| {
        s.project().tracks[&t.parse().unwrap()].automation[0]
            .points
            .iter()
            // The point just after the stroke keeps the line outside it where it was.
            .filter(|p| p.tick <= 960)
            .map(|p| (p.tick, (p.value * 100.0).round() / 100.0))
            .collect::<Vec<_>>()
    };
    assert_eq!(lane_points(&s), vec![(0, -0.4), (480, 0.4), (960, -0.4)]);
    run(
        &mut s,
        "auto.draw",
        json!({"track": t, "target": "pan", "shape": "free", "from": 0, "to": 960,
               "points": points, "step": 240}),
    );
    assert_eq!(
        lane_points(&s),
        vec![(0, -0.4), (240, 0.0), (480, 0.4), (720, 0.0), (960, -0.4)]
    );
    fails(
        &mut s,
        "auto.draw",
        json!({"track": t, "target": "pan", "shape": "free", "from": 0, "to": 960}),
    );
    fails(
        &mut s,
        "auto.draw",
        json!({"track": t, "target": "pan", "shape": "sine", "from": 960, "to": 960, "value": 0}),
    );
}

#[test]
fn a_wave_in_the_tempo_row_keeps_the_first_point_and_gets_undone_in_one_step() {
    let mut s = Session::new("t");
    run(&mut s, "tempo.set", json!({"at": 7680, "bpm": 100}));
    let before = s.project().tempo.clone();
    let steps = s.steps().len();
    run(
        &mut s,
        "tempo.draw",
        json!({"shape": "triangle", "from": 960, "to": 3840, "value": 120, "amp": 20,
               "period": 1920}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    let tempo = s.project().tempo.clone();
    assert_eq!(tempo[0].tick, 0);
    assert_eq!(tempo[0].bpm, before[0].bpm);
    // The point at 7680 is outside the stroke and stays.
    assert_eq!(tempo.last().unwrap().tick, 7680);
    // A triangle goes up, down and up again: peaks of 140 and troughs of 100.
    let drawn: Vec<(i64, f64)> = tempo
        .iter()
        .filter(|t| (960..=3840).contains(&t.tick))
        .map(|t| (t.tick, t.bpm.round()))
        .collect();
    assert_eq!(drawn[0], (960, 120.0));
    assert!(drawn.iter().any(|d| d.1 == 140.0));
    assert!(drawn.iter().any(|d| d.1 == 100.0));
    s.undo(1).unwrap();
    assert_eq!(s.project().tempo, before);
    // A square wave jumps instead of gliding.
    run(
        &mut s,
        "tempo.draw",
        json!({"shape": "square", "from": 960, "to": 3840, "value": 120, "amp": 20,
               "period": 1920}),
    );
    assert!(
        s.project()
            .tempo
            .iter()
            .filter(|t| t.tick > 0 && t.tick < 3840)
            .all(|t| !t.ramp)
    );
}
