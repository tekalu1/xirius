//! Touch returns to the lane; completed Write laps remain after a partial last lap.
use serde_json::{Value, json};
use xirius_core::{Session, auto::AutoTarget, id::TrackId, session::RunOptions};
use xirius_session::autowrite::{Pass, Phase, RETURN};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run(name, args.as_object().unwrap(), &RunOptions::human())
        .unwrap()
        .data
}
fn fixture(mode: &str) -> (Session, TrackId) {
    let mut s = Session::new("modes");
    let t = run(&mut s, "track.add.audio", json!({"name":"audio"}))["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    run(&mut s, "track.volume.set", json!({"ids":[t],"db":-6}));
    run(&mut s, "auto.mode", json!({"ids":[t],"mode":mode}));
    (s, t)
}

#[test]
fn touch_returns_to_the_lane_at_the_return_time_or_the_original_control_without_a_lane() {
    for has_lane in [false, true] {
        let (mut s, t) = fixture("touch");
        if has_lane {
            for (at, value) in [(0, -20.0), (3840, 0.0)] {
                run(
                    &mut s,
                    "auto.point.add",
                    json!({"track":t,"target":"volume","at":at,"value":value}),
                );
            }
        }
        let original = s.project_arc();
        let back = 960 + RETURN;
        let expected = s.project().tracks[&t]
            .automation
            .first()
            .and_then(|l| l.value_at(back as f64))
            .unwrap_or(-6.0);
        let mut pass = Pass::begin(s.project(), 0);
        assert!(pass.touch(t, AutoTarget::Volume, Phase::Begin, 480, Some(-35.0)));
        assert!(pass.touch(t, AutoTarget::Volume, Phase::End, 960, Some(-35.0)));
        let heard = pass.heard(s.project()).unwrap();
        run(&mut s, "auto.write", json!({"writes":pass.finish(1920)}));
        for at in 0..480 {
            let before = original.tracks[&t]
                .automation
                .first()
                .and_then(|l| l.value_at(at as f64))
                .unwrap_or(-6.0);
            assert!(
                (heard.tracks[&t].automation[0].value_at(at as f64).unwrap() - before).abs() < 1e-9
            );
            assert!(
                (s.project().tracks[&t].automation[0]
                    .value_at(at as f64)
                    .unwrap()
                    - before)
                    .abs()
                    < 1e-9
            );
        }
        for at in [960, 960 + RETURN / 2, back, back + 120] {
            let live = heard.tracks[&t].automation[0].value_at(at as f64).unwrap();
            let saved = s.project().tracks[&t].automation[0]
                .value_at(at as f64)
                .unwrap();
            assert!(
                (live - saved).abs() < 1e-9,
                "{has_lane} {at}: {live} vs {saved}"
            );
        }
        assert!((heard.tracks[&t].automation[0].value_at(960.0).unwrap() + 35.0).abs() < 1e-9);
        let lane = &s.project().tracks[&t].automation[0];
        assert!((lane.value_at(back as f64).unwrap() - expected).abs() < 1e-9);
        assert_eq!(s.project().tracks[&t].volume_db, -6.0);
    }
}

#[test]
fn a_write_lap_and_partial_next_lap_keep_held_values_in_one_undo() {
    let (mut s, t) = fixture("write");
    for at in [0, 3840, 7680] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track":t,"target":"volume","at":at,"value":-10}),
        );
    }
    let before = s.project_arc();
    let mut pass = Pass::begin(s.project(), 960);
    pass.touch(t, AutoTarget::Volume, Phase::Begin, 1200, Some(-30.0));
    pass.touch(t, AutoTarget::Volume, Phase::Change, 3000, Some(-24.0));
    let mut first_lap = Session::from_project(s.project().clone());
    run(
        &mut first_lap,
        "auto.write",
        json!({"writes":pass.clone().finish(3840)}),
    );
    pass.restart_segment(0, 3840);
    assert_eq!(pass.heard(s.project()).unwrap().tracks[&t].volume_db, -24.0);
    pass.touch(t, AutoTarget::Volume, Phase::Change, 1000, Some(-12.0));
    run(&mut s, "auto.write", json!({"writes":pass.finish(1440)}));
    let lane = &s.project().tracks[&t].automation[0];
    assert!((lane.value_at(0.0).unwrap() - (-24.0)).abs() < 1e-9);
    assert!((lane.value_at(1400.0).unwrap() - (-12.0)).abs() < 1e-9);
    assert!((lane.value_at(3200.0).unwrap() - (-24.0)).abs() < 1e-9);
    assert!((lane.value_at(7680.0).unwrap() - (-10.0)).abs() < 1e-9);
    for at in 1441..3840 {
        let expected = first_lap.project().tracks[&t].automation[0]
            .value_at(at as f64)
            .unwrap();
        assert!(
            (lane.value_at(at as f64).unwrap() - expected).abs() < 1e-9,
            "{at}"
        );
    }
    s.undo(1).unwrap();
    assert_eq!(s.project(), before.as_ref());
}

#[test]
fn write_without_a_touch_covers_a_completed_lap_after_stopping_in_the_next_lap() {
    let (mut s, t) = fixture("write");
    run(
        &mut s,
        "auto.point.add",
        json!({"track":t,"target":"volume","at":7680,"value":-30}),
    );
    let mut pass = Pass::begin(s.project(), 960);
    pass.restart_segment(0, 3840);
    run(&mut s, "auto.write", json!({"writes":pass.finish(720)}));
    let lane = &s.project().tracks[&t].automation[0];
    assert!((lane.value_at(600.0).unwrap() - (-6.0)).abs() < 1e-9);
    assert!((lane.value_at(3000.0).unwrap() - (-6.0)).abs() < 1e-9);
    assert!((lane.value_at(7680.0).unwrap() - (-30.0)).abs() < 1e-9);
}
