//! A lane of a built-in's switch or choice holds only its steps (AUT-016): each way of writing
//! a point puts it on a step and makes it a stair, the line holds up to the next point, and
//! a song saved with values between the steps opens on them without being an edit.

use serde_json::{Value, json};
use xirius_core::auto::{
    AutoLane, AutoPoint, AutoTarget, MAX_LIST, ParamSteps, clap_steps, snap, vst3_steps,
};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn id_of(r: &RunResult) -> String {
    r.data["id"].as_str().expect("an id").to_string()
}

/// A track with an EQ and a Delay.
fn song() -> (Session, String, String, String) {
    let mut s = Session::new("steps");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    let delay = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Delay"}),
    ));
    (s, t, eq, delay)
}

fn lane(s: &Session, t: &str, target: &str) -> AutoLane {
    let t: TrackId = t.parse().unwrap();
    let target: AutoTarget = target.parse().unwrap();
    s.project().tracks[&t]
        .automation
        .iter()
        .find(|l| l.target == target)
        .cloned()
        .unwrap_or_else(|| panic!("no lane {target}"))
}

fn values(l: &AutoLane) -> Vec<(i64, f64)> {
    l.points.iter().map(|p| (p.tick, p.value)).collect()
}

fn stairs(l: &AutoLane) -> bool {
    l.points.iter().all(|p| p.step && p.curve == 0.0)
}

#[test]
fn the_steps_of_a_target_come_from_its_parameter() {
    let (s, t, eq, delay) = song();
    let p = s.project();
    let track = &p.tracks[&t.parse::<TrackId>().unwrap()];
    let param =
        |id: &str, name: &str| AutoTarget::Param(id.parse::<PluginId>().unwrap(), name.into());
    for name in [
        "low_cut_on",
        "b2_on",
        "b3_on",
        "b4_on",
        "b5_on",
        "high_cut_on",
    ] {
        assert_eq!(param(&eq, name).steps_on(track), Some(2), "{name}");
    }
    assert_eq!(param(&eq, "high_cut_slope").steps_on(track), Some(3));
    assert_eq!(param(&eq, "high_cut_freq").steps_on(track), None);
    for (name, n) in [("sync", 2), ("ping_pong", 2), ("division", 6), ("feel", 3)] {
        assert_eq!(param(&delay, name).steps_on(track), Some(n), "{name}");
    }
    assert_eq!(param(&delay, "time").steps_on(track), None);
    assert_eq!(AutoTarget::Mute.steps_on(track), Some(2));
    assert_eq!(AutoTarget::Volume.steps_on(track), None);
    // a parameter of a plug-in Xirius does not describe has no steps it knows of
    assert_eq!(param("plg_999", "on").steps_on(track), None);

    assert_eq!(snap((0.0, 1.0), 2, 0.49), 0.0);
    assert_eq!(snap((0.0, 1.0), 2, 0.5), 1.0);
    assert_eq!(snap((0.0, 1.0), 4, 0.4), 1.0 / 3.0);
    assert_eq!(snap((0.0, 1.0), 4, 1.7), 1.0);
}

#[test]
fn every_way_of_writing_puts_a_switch_on_its_steps() {
    let (mut s, t, eq, delay) = song();
    let on = format!("param:{eq}:high_cut_on");
    // a click of the pencil, between the steps
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": on, "at": 0, "value": 0.3}),
    );
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": on, "at": 960, "value": 0.7}),
    );
    let l = lane(&s, &t, &on);
    assert_eq!(values(&l), vec![(0, 0.0), (960, 1.0)]);
    assert!(stairs(&l));
    // the line holds up to the next point: no value between, on at 960 only
    assert_eq!(l.curve_at(959.0), Some(0.0));
    assert_eq!(l.curve_at(960.0), Some(1.0));

    // a point moved
    run(
        &mut s,
        "auto.point.move",
        json!({"track": t, "target": on, "index": 1, "at": 1920, "value": 0.45}),
    );
    assert_eq!(values(&lane(&s, &t, &on)), vec![(0, 0.0), (1920, 0.0)]);
    run(
        &mut s,
        "auto.point.move",
        json!({"track": t, "target": on, "index": 1, "at": 1920, "value": 0.55}),
    );
    assert_eq!(values(&lane(&s, &t, &on)), vec![(0, 0.0), (1920, 1.0)]);

    // a line drawn (a ramp, a sine) is a stair of its steps
    for shape in ["line", "sine"] {
        run(
            &mut s,
            "auto.draw",
            json!({"track": t, "target": on, "shape": shape, "from": 3840, "to": 7680,
                   "value": 0.0, "end-value": 1.0, "amp": 0.5, "period": 960}),
        );
        let l = lane(&s, &t, &on);
        assert!(stairs(&l), "{shape}");
        assert!(
            l.points.iter().all(|p| p.value == 0.0 || p.value == 1.0),
            "{shape}: {:?}",
            values(&l)
        );
        for tick in (0..8000).step_by(37) {
            let v = l.curve_at(f64::from(tick)).unwrap();
            assert!(v == 0.0 || v == 1.0, "{shape} at {tick}: {v}");
        }
    }

    // a pass written from a control
    run(
        &mut s,
        "auto.write",
        json!({"writes": [{"track": t, "target": on, "from": 9600, "to": 11520,
               "points": [{"tick": 9600, "value": 0.2}, {"tick": 10560, "value": 0.8},
                          {"tick": 11520, "value": 0.6}]}]}),
    );
    let l = lane(&s, &t, &on);
    assert!(stairs(&l));
    assert!(l.points.iter().all(|p| p.value == 0.0 || p.value == 1.0));

    // a choice of 6 holds its own steps
    let division = format!("param:{delay}:division");
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": division, "at": 0, "value": 0.33}),
    );
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": division, "at": 960, "value": 0.95}),
    );
    let l = lane(&s, &t, &division);
    assert_eq!(values(&l), vec![(0, 0.4), (960, 1.0)]);
    assert!(stairs(&l));
    // a value that glides still glides
    let freq = format!("param:{eq}:high_cut_freq");
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": freq, "at": 0, "value": 0.3}),
    );
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": freq, "at": 960, "value": 0.7}),
    );
    let l = lane(&s, &t, &freq);
    assert_eq!(values(&l), vec![(0, 0.3), (960, 0.7)]);
    assert!(l.points.iter().all(|p| !p.step));
    assert!((l.curve_at(480.0).unwrap() - 0.5).abs() < 1e-9);
}

#[test]
fn a_switch_lane_comes_back_from_the_file_and_an_old_one_opens_on_its_steps() {
    let base = tempfile::Builder::new()
        .prefix("xirius-core-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join("steps");
    let (mut s, t, eq, _) = song();
    let on = format!("param:{eq}:low_cut_on");
    for (at, v) in [(0, 0.8), (1920, 0.1), (3840, 0.9)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": on, "at": at, "value": v}),
        );
    }
    let written = lane(&s, &t, &on);
    assert_eq!(values(&written), vec![(0, 1.0), (1920, 0.0), (3840, 1.0)]);
    s.save_as(&dir).unwrap();
    let saved = s.project().content_hash();
    drop(s);

    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().content_hash(), saved);
    assert_eq!(lane(&s, &t, &on), written);
    assert!(!s.is_dirty());
    drop(s);

    // a song an older Xirius saved: values between the steps on a sloped line, as its
    // file and its history keep them
    let mut p = Session::open(&dir).unwrap().project().clone();
    let tid: TrackId = t.parse().unwrap();
    let target: AutoTarget = on.parse().unwrap();
    let track = std::sync::Arc::make_mut(p.tracks.get_mut(&tid).unwrap());
    let l = track
        .automation
        .iter_mut()
        .find(|l| l.target == target)
        .unwrap();
    l.points = std::sync::Arc::new(vec![
        AutoPoint::new(0, 0.3),
        AutoPoint {
            curve: 0.5,
            ..AutoPoint::new(1920, 0.7)
        },
        AutoPoint::new(3840, 0.2),
    ]);
    let before = AutoLane {
        points: l.points.clone(),
        ..AutoLane::new(target.clone())
    };
    let old = base.path().join("old");
    Session::from_project(p).save_as(&old).unwrap();
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(old.join("project.json")).unwrap()).unwrap();
    assert!(
        file.to_string().contains("0.7"),
        "the file keeps the old values"
    );
    let mut s = Session::open(&old).unwrap();
    let l = lane(&s, &t, &on);
    assert!(stairs(&l));
    assert!(l.points.iter().all(|p| p.value == 0.0 || p.value == 1.0));
    // it sounds as it did: the parameter took the nearest step of the line all along, and
    // the stair is on that step at every tick (on from where the line crossed the middle)
    for tick in 0..=4000 {
        let tick = f64::from(tick);
        let was = snap((0.0, 1.0), 2, before.curve_at(tick).unwrap());
        assert_eq!(l.curve_at(tick).unwrap(), was, "at {tick}");
    }
    assert_eq!(
        values(&l),
        vec![(0, 0.0), (960, 1.0), (1920, 1.0), (2228, 0.0), (3840, 0.0)]
    );
    assert!(!s.is_dirty(), "opening it is no edit");
    // and an edit of it keeps it so
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": on, "at": 5760, "value": 0.6}),
    );
    assert_eq!(
        values(&lane(&s, &t, &on)),
        vec![
            (0, 0.0),
            (960, 1.0),
            (1920, 1.0),
            (2228, 0.0),
            (3840, 0.0),
            (5760, 1.0)
        ]
    );
    assert!(s.is_dirty());
}

#[test]
fn a_lane_snaps_only_when_its_target_has_steps() {
    let (s, t, eq, _) = song();
    let p = s.project();
    let track = &p.tracks[&t.parse::<TrackId>().unwrap()];
    let pid: PluginId = eq.parse().unwrap();
    let mut l = AutoLane::new(AutoTarget::Param(pid, "b3_on".into()));
    std::sync::Arc::make_mut(&mut l.points)
        .extend([AutoPoint::new(0, 0.4), AutoPoint::new(960, 0.6)]);
    assert!(!l.snapped_on(track));
    assert!(l.snap_on(track));
    // the line crossed the middle half way: on from there, as it played
    assert_eq!(values(&l), vec![(0, 0.0), (480, 1.0), (960, 1.0)]);
    assert!(stairs(&l));
    assert!(l.snapped_on(track));
    assert!(!l.snap_on(track), "a second time changes nothing");

    let mut glide = AutoLane::new(AutoTarget::Param(pid, "mid1_gain".into()));
    std::sync::Arc::make_mut(&mut glide.points).push(AutoPoint::new(0, 0.4));
    assert!(!glide.snap_on(track));
    assert_eq!(values(&glide), vec![(0, 0.4)]);
}

#[test]
fn the_other_switches_of_the_built_ins_have_steps_too() {
    let mut s = Session::new("switches");
    let bus = id_of(&run(&mut s, "track.add.audio", json!({"name": "Bus"})));
    let comp = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": bus, "name": "Compressor"}),
    ));
    let mut each = vec![(bus.clone(), comp, vec![("filter", 2), ("listen", 2)])];
    for (name, keys) in [
        (
            "Sampler",
            vec![("file1.loop", 2), ("file2.reverse", 2), ("ftype", 0)],
        ),
        (
            "Drum Kit",
            vec![("A1.mute", 2), ("A1.solo", 2), ("A1.trigger", 4)],
        ),
    ] {
        let t = id_of(&run(&mut s, "track.add.instrument", json!({"name": name})));
        let id = id_of(&run(
            &mut s,
            "plugin.add",
            json!({"track": t, "name": name, "kind": "instrument"}),
        ));
        each.push((t, id, keys));
    }
    let p = s.project();
    for (t, id, keys) in each {
        let track = &p.tracks[&t.parse::<TrackId>().unwrap()];
        let pid: PluginId = id.parse().unwrap();
        for (key, n) in keys {
            let steps = AutoTarget::Param(pid, key.into()).steps_on(track);
            if n == 0 {
                assert!(steps.is_some_and(|n| n >= 2), "{key}: a choice");
            } else {
                assert_eq!(steps, Some(n), "{key}");
            }
        }
        let start = AutoTarget::Param(pid, "file1.start".into()).steps_on(track);
        assert_eq!(start, None);
    }
}

/// `p` with the lane of `target` on track `t` holding `points` as they are (as a song from an
/// older Xirius kept them), not put on the steps.
fn with_points(p: &mut Project, t: &str, target: &AutoTarget, points: Vec<AutoPoint>) {
    let tid: TrackId = t.parse().unwrap();
    let track = std::sync::Arc::make_mut(p.tracks.get_mut(&tid).unwrap());
    match track.automation.iter_mut().find(|l| l.target == *target) {
        Some(l) => l.points = std::sync::Arc::new(points),
        None => track.automation.push(AutoLane {
            points: std::sync::Arc::new(points),
            ..AutoLane::new(target.clone())
        }),
    }
}

#[test]
fn a_whole_number_glides_and_an_old_song_keeps_it() {
    let base = tempfile::Builder::new()
        .prefix("xirius-core-test-")
        .tempdir()
        .unwrap();
    let mut s = Session::new("numbers");
    let t = id_of(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Keys"}),
    ));
    let sampler = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "Sampler", "kind": "instrument"}),
    ));
    let transpose = format!("param:{sampler}:transpose");
    let target: AutoTarget = transpose.parse().unwrap();
    let p = s.project();
    let track = &p.tracks[&t.parse::<TrackId>().unwrap()];
    // 97 semitones: the values between have a meaning (the number nearest), so it is no stair
    assert_eq!(target.steps_on(track), None);

    // a line between whole numbers stays a line, written or opened from an old song
    for (at, v) in [(0, 0.3), (960, 0.71)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": transpose, "at": at, "value": v}),
        );
    }
    let l = lane(&s, &t, &transpose);
    assert_eq!(values(&l), vec![(0, 0.3), (960, 0.71)]);
    assert!(l.points.iter().all(|p| !p.step));

    let mut p = s.project().clone();
    let old = vec![
        AutoPoint {
            curve: 0.4,
            ..AutoPoint::new(0, 0.123)
        },
        AutoPoint::new(1920, 0.876),
    ];
    with_points(&mut p, &t, &target, old.clone());
    let dir = base.path().join("old");
    Session::from_project(p).save_as(&dir).unwrap();
    let s = Session::open(&dir).unwrap();
    assert_eq!(*lane(&s, &t, &transpose).points, old);
    assert!(!s.is_dirty());
}

#[test]
fn an_edit_puts_only_the_lanes_it_wrote_on_the_steps() {
    let (s, t, eq, _) = song();
    let mut p = s.project().clone();
    let on = format!("param:{eq}:b2_on");
    // a lane an older Xirius left between the steps, not touched since
    let old = vec![AutoPoint::new(0, 0.3), AutoPoint::new(960, 0.7)];
    with_points(&mut p, &t, &on.parse().unwrap(), old.clone());
    let mut s = Session::from_project(p);

    // an edit of another lane leaves it, so the undo holds that edit alone
    let freq = format!("param:{eq}:high_cut_freq");
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": freq, "at": 0, "value": 0.5}),
    );
    assert_eq!(*lane(&s, &t, &on).points, old);
    s.undo(1).unwrap();
    assert_eq!(*lane(&s, &t, &on).points, old);

    // an edit of it makes it a stair, and its undo brings the old line back
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": on, "at": 1920, "value": 0.2}),
    );
    let l = lane(&s, &t, &on);
    assert!(stairs(&l));
    // (the old line went on to the new point, and crossed the middle on its way down)
    assert_eq!(
        values(&l),
        vec![(0, 0.0), (480, 1.0), (960, 1.0), (1345, 0.0), (1920, 0.0)]
    );
    s.undo(1).unwrap();
    assert_eq!(*lane(&s, &t, &on).points, old);
}

#[test]
fn a_plug_in_parameter_has_the_steps_its_plug_in_tells() {
    let (s, t, eq, _) = song();
    let mut p = s.project().clone();
    // a VST3 plug-in in place of the EQ (its id is kept so the lanes have a plug-in)
    let tid: TrackId = t.parse().unwrap();
    let pid: PluginId = eq.parse().unwrap();
    {
        let track = std::sync::Arc::make_mut(p.tracks.get_mut(&tid).unwrap());
        let x = track.inserts.iter_mut().find(|x| x.id == pid).unwrap();
        x.format = "vst3".into();
        x.uid = "0123456789ABCDEF0123456789ABCDEF".into();
        x.name = "Plug".into();
    }
    let mut s = Session::from_project(p);
    let steps = ParamSteps::new(
        [(
            pid,
            vec![(7, "Bypass".to_string(), 2), (9, "Mode".to_string(), 4)],
        )]
        .into(),
    );
    let track = s.project().tracks[&tid].clone();
    let param = |name: &str| AutoTarget::Param(pid, name.into());
    // by its id, or by its title
    assert_eq!(param("7").steps_with(&track, &steps), Some(2));
    assert_eq!(param("mode").steps_with(&track, &steps), Some(4));
    // one it does not name glides; without the running plug-in nothing is known
    assert_eq!(param("8").steps_with(&track, &steps), None);
    assert_eq!(param("7").steps_on(&track), None);

    s.state.param_steps = steps;
    let bypass = format!("param:{eq}:7");
    let mode = format!("param:{eq}:9");
    let other = format!("param:{eq}:8");
    for (target, at, v) in [
        (&bypass, 0, 0.2),
        (&bypass, 960, 0.9),
        (&mode, 0, 0.1),
        (&mode, 960, 0.7),
        (&other, 0, 0.1),
        (&other, 960, 0.7),
    ] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": target, "at": at, "value": v}),
        );
    }
    let l = lane(&s, &t, &bypass);
    assert_eq!(values(&l), vec![(0, 0.0), (960, 1.0)]);
    assert!(stairs(&l));
    let l = lane(&s, &t, &mode);
    assert_eq!(values(&l), vec![(0, 0.0), (960, 2.0 / 3.0)]);
    assert!(stairs(&l));
    let l = lane(&s, &t, &other);
    assert_eq!(values(&l), vec![(0, 0.1), (960, 0.7)]);
    assert!(l.points.iter().all(|p| !p.step));
}

#[test]
fn the_steps_a_plug_in_tells_are_trusted_only_as_a_switch_or_a_list() {
    // VST3: a step count of 1 is a switch; a list says so and has a list's size
    assert_eq!(vst3_steps(0, false), None);
    assert_eq!(vst3_steps(1, false), Some(2));
    assert_eq!(vst3_steps(1, true), Some(2));
    assert_eq!(vst3_steps(3, true), Some(4));
    assert_eq!(vst3_steps(96, false), None, "a whole number glides");
    assert_eq!(vst3_steps(MAX_LIST as i32 - 1, true), Some(MAX_LIST));
    assert_eq!(
        vst3_steps(MAX_LIST as i32, true),
        None,
        "too many to be a list"
    );
    assert_eq!(vst3_steps(-1, true), None);
    // CLAP: stepped over one step, or an enum over a whole number of them
    assert_eq!(clap_steps(true, false, 0.0, 1.0), Some(2));
    assert_eq!(clap_steps(true, true, 0.0, 1.0), Some(2));
    assert_eq!(clap_steps(true, true, 0.0, 3.0), Some(4));
    assert_eq!(clap_steps(true, true, 1.0, 4.0), Some(4));
    assert_eq!(
        clap_steps(true, false, -48.0, 48.0),
        None,
        "a whole number glides"
    );
    assert_eq!(clap_steps(false, true, 0.0, 1.0), None, "not stepped");
    assert_eq!(
        clap_steps(true, true, 0.0, 2.5),
        None,
        "not a whole number of steps"
    );
    assert_eq!(
        clap_steps(true, true, 0.0, 1000.0),
        None,
        "too many to be a list"
    );
    assert_eq!(clap_steps(true, true, 0.0, f64::INFINITY), None);
    assert_eq!(clap_steps(true, true, 1.0, 1.0), None);
}

#[test]
fn a_switch_pasted_on_a_lane_that_glides_takes_its_lines() {
    let (mut s, t, eq, _) = song();
    let on = format!("param:{eq}:b2_on");
    let freq = format!("param:{eq}:high_cut_freq");
    let band = format!("param:{eq}:b3_on");
    for (at, v) in [(0, 0.0), (960, 1.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": on, "at": at, "value": v}),
        );
    }
    assert!(stairs(&lane(&s, &t, &on)));
    run(
        &mut s,
        "edit.copy",
        json!({"track": t, "target": on, "ticks": "0,960"}),
    );
    // on a lane that glides, the points draw a line between them
    run(
        &mut s,
        "edit.paste",
        json!({"track": t, "target": freq, "at": 1920}),
    );
    let l = lane(&s, &t, &freq);
    assert_eq!(values(&l), vec![(1920, 0.0), (2880, 1.0)]);
    assert!(l.points.iter().all(|p| !p.step));
    // on another switch, they stay a stair
    run(
        &mut s,
        "edit.paste",
        json!({"track": t, "target": band, "at": 1920}),
    );
    let l = lane(&s, &t, &band);
    assert_eq!(values(&l), vec![(1920, 0.0), (2880, 1.0)]);
    assert!(stairs(&l));
}
