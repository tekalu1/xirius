//! Fades, gain and crossfades of audio clips (EDT-002, EDT-017).

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::clipfx::{Fade, fade_gain};
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

fn cid(s: &str) -> ClipId {
    s.parse().unwrap()
}

/// (start, length, offset) of an audio clip.
fn shape(p: &Project, id: &str) -> (i64, i64, i64) {
    let (_, c) = p.clip(cid(id)).unwrap();
    let ClipBody::Audio { offset, .. } = &c.body else {
        panic!("not audio");
    };
    (c.start, c.length, *offset)
}

fn gain(p: &Project, id: &str) -> f64 {
    let (_, c) = p.clip(cid(id)).unwrap();
    let ClipBody::Audio { gain_db, .. } = &c.body else {
        panic!("not audio");
    };
    *gain_db
}

fn resolved(p: &Project, id: &str) -> xirius_core::clipfx::Resolved {
    let (t, c) = p.clip(cid(id)).unwrap();
    p.resolve_fx(t, c)
}

fn one(s: &mut Session, len: i64) -> (String, String) {
    let t = id_of(&run(s, "track.add.audio", json!({})));
    let a = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": len, "file": "a.wav"}),
    ));
    (t, a)
}

/// Two clips that touch at 960, the right one with material before its start.
fn pair(s: &mut Session) -> (String, String, String) {
    let (t, a) = one(s, 960);
    let b = id_of(&run(
        s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 960, "file": "b.wav"}),
    ));
    run(s, "clip.slip", json!({"ids": [b], "by": -480}));
    (t, a, b)
}

#[test]
fn a_fade_is_set_per_end_and_one_step() {
    let mut s = Session::new("fade");
    let (_, a) = one(&mut s, 960);
    let n = s.history().len();
    run(
        &mut s,
        "clip.fade.set",
        json!({"ids": [a], "in": 240, "out": 120, "curve": 0.5}),
    );
    assert_eq!(s.history().len(), n + 1);
    let r = resolved(s.project(), &a);
    assert_eq!(
        r.fade_in,
        Fade {
            length: 240,
            curve: 0.5
        }
    );
    assert_eq!(
        r.fade_out,
        Fade {
            length: 120,
            curve: 0.5
        }
    );

    // one end on its own leaves the other as it was
    run(&mut s, "clip.fade.set", json!({"ids": [a], "out": 0}));
    let r = resolved(s.project(), &a);
    assert_eq!(r.fade_in.length, 240);
    assert!(r.fade_out.is_none());

    // a curve alone bends what is there
    run(
        &mut s,
        "clip.fade.set",
        json!({"ids": [a], "in_curve": -0.5}),
    );
    assert_eq!(resolved(s.project(), &a).fade_in.curve, -0.5);

    run(&mut s, "clip.fade.set", json!({"ids": [a], "clear": true}));
    assert!(resolved(s.project(), &a).is_plain());
    assert!(
        s.project().clip_fx.is_empty(),
        "a clip with nothing keeps no row"
    );

    s.undo(1).unwrap();
    assert_eq!(resolved(s.project(), &a).fade_in.curve, -0.5);
}

#[test]
fn a_fade_that_does_not_fit_is_refused_and_changes_nothing() {
    let mut s = Session::new("fade-bad");
    let (_, a) = one(&mut s, 960);
    run(&mut s, "clip.fade.set", json!({"ids": [a], "in": 500}));
    assert!(try_run(&mut s, "clip.fade.set", json!({"ids": [a], "out": 500})).is_err());
    assert!(try_run(&mut s, "clip.fade.set", json!({"ids": [a], "in": -1})).is_err());
    assert!(try_run(&mut s, "clip.fade.set", json!({"ids": [a], "curve": 1.5})).is_err());
    assert!(try_run(&mut s, "clip.fade.set", json!({"ids": [a]})).is_err());
    let r = resolved(s.project(), &a);
    assert_eq!((r.fade_in.length, r.fade_out.length), (500, 0));

    // a MIDI clip has none
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let m = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 960}),
    ));
    assert!(try_run(&mut s, "clip.fade.set", json!({"ids": [m], "in": 10})).is_err());
    assert!(try_run(&mut s, "clip.gain.set", json!({"ids": [m], "db": -3.0})).is_err());
}

#[test]
fn auto_puts_a_short_fade_on_both_ends() {
    let mut s = Session::new("fade-auto");
    let (_, a) = one(&mut s, 960);
    run(&mut s, "clip.fade.set", json!({"ids": [a], "auto": true}));
    let r = resolved(s.project(), &a);
    let short = i64::from(s.project().ppq) / 32;
    assert_eq!((r.fade_in.length, r.fade_out.length), (short, short));
}

#[test]
fn gain_is_absolute_or_relative_and_held_to_the_range() {
    let mut s = Session::new("gain");
    let (_, a) = one(&mut s, 960);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "db": -6.0}));
    assert_eq!(gain(s.project(), &a), -6.0);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "by": 2.5}));
    assert_eq!(gain(s.project(), &a), -3.5);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "by": 100.0}));
    assert_eq!(gain(s.project(), &a), 12.0);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "db": -500.0}));
    assert_eq!(gain(s.project(), &a), -120.0);
    assert!(try_run(&mut s, "clip.gain.set", json!({"ids": [a]})).is_err());
    assert!(
        try_run(
            &mut s,
            "clip.gain.set",
            json!({"ids": [a], "db": 0.0, "by": 1.0})
        )
        .is_err()
    );
    run(&mut s, "clip.gain.set", json!({"ids": [a], "reset": true}));
    assert_eq!(gain(s.project(), &a), 0.0);
}

#[test]
fn several_clips_change_in_one_step() {
    let mut s = Session::new("gain-many");
    let (t, a) = one(&mut s, 480);
    let b = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 480, "file": "b.wav"}),
    ));
    let n = s.history().len();
    run(&mut s, "clip.gain.set", json!({"ids": [a, b], "by": -4.0}));
    assert_eq!(s.history().len(), n + 1);
    assert_eq!((gain(s.project(), &a), gain(s.project(), &b)), (-4.0, -4.0));
    s.undo(1).unwrap();
    assert_eq!((gain(s.project(), &a), gain(s.project(), &b)), (0.0, 0.0));
}

#[test]
fn gain_points_stay_in_order_and_replace_at_one_place() {
    let mut s = Session::new("points");
    let (_, a) = one(&mut s, 960);
    let r = run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 600, "db": -6.0}),
    );
    assert_eq!(r.data["index"], 0);
    let r = run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 200, "db": 3.0}),
    );
    assert_eq!(r.data["index"], 0);
    // no db: the line's value there (between the two, in dB)
    let r = run(&mut s, "clip.gain.point.add", json!({"id": a, "at": 400}));
    assert_eq!(r.data["index"], 1);
    let pts = &s.project().fx(cid(&a)).unwrap().points;
    assert_eq!(
        pts.iter().map(|p| p.tick).collect::<Vec<_>>(),
        [200, 400, 600]
    );
    assert!((pts[1].db - (-1.5)).abs() < 1e-9, "{}", pts[1].db);

    // the same place replaces
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 400, "db": 0.0}),
    );
    assert_eq!(s.project().fx(cid(&a)).unwrap().points.len(), 3);

    // a point cannot pass its neighbours
    let r = run(
        &mut s,
        "clip.gain.point.move",
        json!({"id": a, "index": 1, "at": 9000}),
    );
    assert_eq!(r.data["at"], 599);
    run(
        &mut s,
        "clip.gain.point.move",
        json!({"id": a, "index": 1, "at": 0, "db": 5.0}),
    );
    let pts = &s.project().fx(cid(&a)).unwrap().points;
    assert_eq!((pts[1].tick, pts[1].db), (201, 5.0));

    assert!(try_run(&mut s, "clip.gain.point.add", json!({"id": a, "at": 5000})).is_err());
    assert!(
        try_run(
            &mut s,
            "clip.gain.point.delete",
            json!({"id": a, "index": 9})
        )
        .is_err()
    );
    run(
        &mut s,
        "clip.gain.point.delete",
        json!({"id": a, "index": 0}),
    );
    assert_eq!(s.project().fx(cid(&a)).unwrap().points.len(), 2);
    run(&mut s, "clip.gain.set", json!({"ids": [a], "reset": true}));
    assert!(s.project().clip_fx.is_empty());
}

#[test]
fn a_crossfade_over_a_join_overlaps_the_two_clips() {
    let mut s = Session::new("cross");
    let (_, a, b) = pair(&mut s);
    let n = s.history().len();
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 120}),
    );
    assert_eq!(s.history().len(), n + 1);
    // centred on the join at 960; the right clip's content stays in place
    assert_eq!(shape(s.project(), &b), (900, 1020, 480 - 60));
    assert_eq!(shape(s.project(), &a), (0, 1020, 0));
    let (ra, rb) = (resolved(s.project(), &a), resolved(s.project(), &b));
    assert_eq!(
        rb.fade_in,
        Fade {
            length: 120,
            curve: 0.5
        }
    );
    assert_eq!(
        ra.fade_out,
        Fade {
            length: 120,
            curve: 0.5
        }
    );
    assert!(rb.crossed_in && ra.crossed_out);

    // the same command again keeps what it is, and is no step
    let r = run(&mut s, "clip.fade.cross", json!({"ids": [a, b]}));
    assert_eq!(shape(s.project(), &b), (900, 1020, 420));
    assert!(r.step.is_none());

    s.undo(1).unwrap();
    assert_eq!(shape(s.project(), &b), (960, 960, 480));
    assert_eq!(shape(s.project(), &a), (0, 960, 0));
    assert!(s.project().clip_fx.is_empty());
}

#[test]
fn one_selected_clip_crosses_with_its_neighbour() {
    let mut s = Session::new("cross-one");
    let (_, a, b) = pair(&mut s);
    // from the right clip: the left neighbour
    run(&mut s, "clip.fade.cross", json!({"ids": [b], "length": 60}));
    assert!(resolved(s.project(), &a).crossed_out);
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [b], "remove": true}),
    );
    assert!(!resolved(s.project(), &a).crossed_out);
    // from the left clip: the right neighbour
    run(&mut s, "clip.fade.cross", json!({"ids": [a], "length": 60}));
    assert!(resolved(s.project(), &b).crossed_in);
}

#[test]
fn a_crossfade_with_no_neighbour_or_a_gap_is_refused() {
    let mut s = Session::new("cross-bad");
    let (t, a) = one(&mut s, 480);
    assert!(try_run(&mut s, "clip.fade.cross", json!({"ids": [a]})).is_err());
    let b = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 480, "file": "b.wav"}),
    ));
    assert!(try_run(&mut s, "clip.fade.cross", json!({"ids": [a, b]})).is_err());
    assert!(
        try_run(
            &mut s,
            "clip.fade.cross",
            json!({"ids": [a], "remove": true})
        )
        .is_err(),
        "nothing to remove"
    );
    assert!(s.project().clip_fx.is_empty());
}

#[test]
fn a_crossfade_with_no_room_before_the_right_clip_extends_the_left() {
    let mut s = Session::new("cross-head");
    let (t, a) = one(&mut s, 960);
    let b = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 960, "file": "b.wav"}),
    ));
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 100}),
    );
    // b's content starts at its head, so b stays and a runs on under it
    assert_eq!(shape(s.project(), &b), (960, 960, 0));
    assert_eq!(shape(s.project(), &a), (0, 1060, 0));
    assert_eq!(resolved(s.project(), &b).fade_in.length, 100);
}

#[test]
fn the_crossfade_follows_the_clips_and_goes_quiet_when_they_part() {
    let mut s = Session::new("cross-follow");
    let (_, a, b) = pair(&mut s);
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 120}),
    );
    // slide the centre
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [b], "move": 40, "curve": 0.2}),
    );
    assert_eq!(shape(s.project(), &b), (940, 980, 460));
    assert_eq!(shape(s.project(), &a), (0, 1060, 0));
    let rb = resolved(s.project(), &b);
    assert_eq!(
        rb.fade_in,
        Fade {
            length: 120,
            curve: 0.2
        }
    );

    // the overlap is what the clips overlap by
    run(&mut s, "clip.resize", json!({"ids": [a], "by": -30}));
    assert_eq!(resolved(s.project(), &b).fade_in.length, 90);
    // moved apart, the pair has no crossfade and the fades are plain
    run(&mut s, "clip.move", json!({"ids": [b], "by": 400}));
    let (ra, rb) = (resolved(s.project(), &a), resolved(s.project(), &b));
    assert!(ra.is_plain() && rb.is_plain());
    // moved back, it is there again
    run(&mut s, "clip.move", json!({"ids": [b], "by": -400}));
    assert_eq!(resolved(s.project(), &b).fade_in.length, 90);
}

#[test]
fn a_fade_set_by_hand_replaces_the_crossfade_on_that_side() {
    let mut s = Session::new("cross-replace");
    let (_, a, b) = pair(&mut s);
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 120}),
    );
    run(&mut s, "clip.fade.set", json!({"ids": [b], "in": 30}));
    assert!(!resolved(s.project(), &a).crossed_out);
    assert_eq!(resolved(s.project(), &b).fade_in.length, 30);
    assert_eq!(resolved(s.project(), &a).fade_out.length, 0);
}

#[test]
fn fades_and_points_go_with_a_split_a_copy_and_a_track_copy() {
    let mut s = Session::new("carry");
    let (t, a) = one(&mut s, 960);
    run(
        &mut s,
        "clip.fade.set",
        json!({"ids": [a], "in": 100, "out": 200}),
    );
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 200, "db": -6.0}),
    );
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 800, "db": 6.0}),
    );

    // a copy takes the fades and the line
    let d = id_of(&run(
        &mut s,
        "clip.duplicate",
        json!({"ids": [a], "by": 2000}),
    ));
    let fx = s.project().fx(cid(&d)).unwrap();
    assert_eq!(
        (fx.fade_in.length, fx.fade_out.length, fx.points.len()),
        (100, 200, 2)
    );

    // a split: the fade-in stays left, the fade-out goes right, the line is cut
    let r = id_of(&run(&mut s, "clip.split", json!({"ids": [a], "at": 500})));
    let (fl, fr) = (
        s.project().fx(cid(&a)).unwrap().clone(),
        s.project().fx(cid(&r)).unwrap().clone(),
    );
    assert_eq!((fl.fade_in.length, fl.fade_out.length), (100, 0));
    assert_eq!((fr.fade_in.length, fr.fade_out.length), (0, 200));
    assert_eq!(
        fl.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
        [200, 500]
    );
    assert_eq!(
        fr.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
        [0, 300]
    );
    // the value at the cut is the same on both sides
    assert!((fl.points[1].db - fr.points[0].db).abs() < 1e-9);

    // a copy of the whole track keeps a crossfade between its own clips
    let t2 = id_of(&run(&mut s, "track.add.audio", json!({})));
    let x = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t2, "at": 0, "length": 960, "file": "a.wav"}),
    ));
    let y = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t2, "at": 960, "length": 960, "file": "b.wav"}),
    ));
    run(&mut s, "clip.slip", json!({"ids": [y], "by": -200}));
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [x, y], "length": 100}),
    );
    let made = id_of(&run(&mut s, "track.duplicate", json!({"ids": [t2]})));
    let copy = s.project().track(made.parse().unwrap()).unwrap();
    let (cx, cy) = (copy.clips[0].id, copy.clips[1].id);
    let rc = s.project().resolve_fx(copy, &copy.clips[1]);
    assert!(rc.crossed_in, "the copy is crossed with its own left clip");
    assert_eq!(s.project().fx(cy).unwrap().cross.unwrap().with, cx);
    let _ = t;
}

#[test]
fn deleting_a_clip_drops_its_row_and_the_crossfades_that_pointed_at_it() {
    let mut s = Session::new("prune");
    let (_, a, b) = pair(&mut s);
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 120}),
    );
    run(&mut s, "clip.fade.set", json!({"ids": [a], "in": 50}));
    run(&mut s, "clip.delete", json!({"ids": [a]}));
    assert!(s.project().fx(cid(&a)).is_none());
    assert!(s.project().fx(cid(&b)).is_none(), "its crossfade went too");
    s.undo(1).unwrap();
    assert!(s.project().fx(cid(&a)).is_some());
    assert!(resolved(s.project(), &b).crossed_in);
}

#[test]
fn trimming_keeps_the_line_in_place_and_the_fade_on_the_edge() {
    let mut s = Session::new("trim");
    let (_, a) = one(&mut s, 960);
    // the file has material before the start of the clip
    run(&mut s, "clip.slip", json!({"ids": [a], "by": -300}));
    run(&mut s, "clip.fade.set", json!({"ids": [a], "in": 100}));
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 200, "db": -6.0}),
    );
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 600, "db": 0.0}),
    );
    run(&mut s, "clip.trim", json!({"ids": [a], "by": 400}));
    let fx = s.project().fx(cid(&a)).unwrap();
    assert_eq!(fx.fade_in.length, 100);
    // the point at 200 is gone (cut off); the line starts with its value held
    assert_eq!(
        fx.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
        [0, 200]
    );
    assert!(fx.points[0].db < 0.0 && fx.points[0].db > -6.0);
    assert_eq!(fx.points[1].db, 0.0);
}

#[test]
fn the_song_keeps_them_through_saving_and_a_song_without_them_writes_nothing() {
    let mut s = Session::new("save");
    let (_, a) = one(&mut s, 960);
    let plain = serde_json::to_value(s.project()).unwrap();
    assert!(plain.get("clip_fx").is_none());
    run(
        &mut s,
        "clip.fade.set",
        json!({"ids": [a], "in": 100, "curve": 0.5}),
    );
    run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 300, "db": -3.0}),
    );
    let v = serde_json::to_value(s.project()).unwrap();
    let back: Project = serde_json::from_value(v).unwrap();
    assert_eq!(back.clip_fx, s.project().clip_fx);
}

#[test]
fn a_change_of_fades_is_reported_as_a_change_of_the_clip() {
    let mut s = Session::new("diff");
    let (_, a) = one(&mut s, 960);
    let r = run(&mut s, "clip.fade.set", json!({"ids": [a], "in": 100}));
    assert_eq!(r.changes.count("clip"), 1);
    assert_eq!(r.changes.count("project"), 0);
    let r = run(
        &mut s,
        "clip.gain.point.add",
        json!({"id": a, "at": 5, "db": 1.0}),
    );
    assert_eq!(r.changes.count("clip"), 1);
}

#[test]
fn the_curve_laws() {
    // equal power: two fades meeting keep the power
    for x in [0.0, 0.2, 0.5, 0.8, 1.0] {
        let p = fade_gain(x, 0.5).powi(2) + fade_gain(1.0 - x, 0.5).powi(2);
        assert!((p - 1.0).abs() < 1e-9);
    }
    // a line: the two add to one
    for x in [0.0, 0.3, 1.0] {
        assert!((fade_gain(x, 0.0) + fade_gain(1.0 - x, 0.0) - 1.0).abs() < 1e-12);
    }
}
