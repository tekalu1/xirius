//! The arrangement's commands: clip edges and rows, sections that carry the
//! song with them, and automation lanes saved with the song.

use std::path::PathBuf;

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::auto::{AutoMode, AutoTarget};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};

/// A song folder named `tag`, not made yet, in a folder of the test's own that goes when the
/// guard is dropped (also when the test fails).
fn temp_dir(tag: &str) -> (tempfile::TempDir, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("xirius-arrange-test-")
        .tempdir()
        .unwrap();
    let dir = base.path().join(tag);
    (base, dir)
}

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

#[test]
fn selected_points_push_keep_relative_values_and_undo_together() {
    let mut s = Session::new("nudge");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name":"Audio"})));
    run(&mut s, "auto.lane.add", json!({"track":t,"target":"pan"}));
    for (at, value) in [(10, -0.5), (20, 0.0), (30, 0.5)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track":t,"target":"pan","at":at,"value":value}),
        );
    }
    let original = s.project().clone();
    run(
        &mut s,
        "auto.points.move",
        json!({"track":t,"target":"pan","ticks":"10,20","by":25,"value-by":0.25}),
    );
    let id: TrackId = t.parse().unwrap();
    let points = &s.project().tracks[&id].automation[0].points;
    assert_eq!(
        points.iter().map(|p| (p.tick, p.value)).collect::<Vec<_>>(),
        [(35, -0.25), (45, 0.25), (46, 0.5)]
    );
    s.undo(1).unwrap();
    assert_eq!(s.project(), &original);
    assert!(
        try_run(
            &mut s,
            "auto.points.move",
            json!({"track":t,"target":"pan","ticks":"10,99","by":5})
        )
        .is_err()
    );
    assert_eq!(s.project(), &original);
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn clip_on(p: &Project, id: &str) -> (TrackId, i64, i64) {
    let id: ClipId = id.parse().unwrap();
    let (t, c) = p.clip(id).unwrap();
    (t.id, c.start, c.length)
}

#[test]
fn trimming_the_left_edge_keeps_the_notes_where_they_sound() {
    let mut s = Session::new("trim");
    let lead = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let clip = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": lead, "at": 0, "length": 1920}),
    ));
    for (at, len) in [(0, 240), (400, 400), (1000, 100)] {
        run(
            &mut s,
            "note.add",
            json!({"clip": clip, "at": at, "pitch": 60, "length": len}),
        );
    }
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 1920,
               "points": [{"tick": 0, "value": 10}, {"tick": 300, "value": 90}, {"tick": 900, "value": 40}]}),
    );
    run(&mut s, "clip.trim", json!({"ids": [clip], "by": 480}));
    let p = s.project();
    let (_, c) = p.clip(clip.parse().unwrap()).unwrap();
    assert_eq!((c.start, c.length), (480, 1440));
    let ClipBody::Midi { notes, .. } = &c.body else {
        panic!()
    };
    // the first note is gone, the one across the edge starts at it, the last stays put
    let spans: Vec<(i64, i64)> = notes.iter().map(|n| (n.start, n.length)).collect();
    assert_eq!(spans, vec![(0, 320), (520, 100)]);
    // the controller value in force at the edge is kept there
    let pts: Vec<(i64, u8)> = c.controllers[0]
        .points
        .iter()
        .map(|x| (x.tick, x.value as u8))
        .collect();
    assert_eq!(pts, vec![(0, 90), (420, 40)]);

    // an audio clip starts later in its file, and cannot reach before its head
    let kick = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": kick, "at": 960, "length": 960, "file": "k.wav"}),
    ));
    run(&mut s, "clip.trim", json!({"ids": [a], "by": 100}));
    let (_, c) = s.project().clip(a.parse().unwrap()).unwrap();
    assert!(matches!(c.body, ClipBody::Audio { offset: 100, .. }));
    assert!(try_run(&mut s, "clip.trim", json!({"ids": [a], "by": -200})).is_err());
    run(&mut s, "clip.trim", json!({"ids": [a], "by": -100}));
    assert_eq!(clip_on(s.project(), &a).1, 960);
    s.undo(1).unwrap();
    assert_eq!(clip_on(s.project(), &a).1, 1060);
}

#[test]
fn clips_move_and_copy_across_rows_of_the_column() {
    let mut s = Session::new("rows");
    let a = id_of(&run(&mut s, "track.add.audio", json!({"name": "A"})));
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "F"})));
    let b = id_of(&run(
        &mut s,
        "track.add.audio",
        json!({"name": "B", "into": f}),
    ));
    let synth = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let clip = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": a, "at": 0, "length": 960, "file": "x.wav"}),
    ));
    // the folder row is skipped
    run(
        &mut s,
        "clip.move",
        json!({"ids": [clip], "rows": 1, "by": 480}),
    );
    assert_eq!(clip_on(s.project(), &clip), (b.parse().unwrap(), 480, 960));
    // an instrument track does not take audio, and there is nothing above A
    assert!(try_run(&mut s, "clip.move", json!({"ids": [clip], "rows": 1})).is_err());
    assert!(try_run(&mut s, "clip.move", json!({"ids": [clip], "rows": -2})).is_err());
    let _ = synth;

    // Alt-drag: one copy at the offset, on another row
    let r = run(
        &mut s,
        "clip.duplicate",
        json!({"ids": [clip], "by": 1920, "rows": -1}),
    );
    let copy = id_of(&r);
    assert_eq!(clip_on(s.project(), &copy), (a.parse().unwrap(), 2400, 960));
    assert_eq!(s.project().tracks.len(), 4, "master + 3");
}

#[test]
fn reordering_sections_carries_every_tracks_clips_and_automation() {
    let mut s = Session::new("sections");
    let bar = 3840;
    let verse = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Verse", "at": 0, "bars": 2}),
    ));
    let chorus = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Chorus", "at": 2 * bar, "bars": 1}),
    ));
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let in_verse = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 2 * bar, "file": "v.wav"}),
    ));
    let in_chorus = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 2 * bar + 10, "length": 100, "file": "c.wav"}),
    ));
    let after = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 5 * bar, "length": 100, "file": "o.wav"}),
    ));
    for (at, v) in [(0, -10.0), (2 * bar + 10, 0.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": "volume", "at": at, "value": v}),
        );
    }
    let was = s.project().track(t.parse().unwrap()).unwrap().automation[0].clone();
    run(&mut s, "section.reorder", json!({"id": chorus, "index": 0}));
    let p = s.project();
    assert_eq!(clip_on(p, &in_chorus).1, 10);
    assert_eq!(clip_on(p, &in_verse).1, bar);
    assert_eq!(clip_on(p, &after).1, 5 * bar, "past the last section stays");
    let starts: Vec<(String, i64)> = p
        .sections
        .iter()
        .map(|x| (x.id.to_string(), x.start))
        .collect();
    assert_eq!(starts, vec![(chorus.clone(), 0), (verse.clone(), bar)]);
    let lane = &p.track(t.parse().unwrap()).unwrap().automation[0];
    // each block carries its part of the line: the Chorus's stretch is first, then the
    // Verse's, which keeps the climb it had instead of being bent toward the Chorus
    let at = |l: &xirius_core::auto::AutoLane, t: i64| l.curve_at(t as f64).unwrap();
    for t in [0, 10, 1000, bar - 1] {
        assert!((at(lane, t) - at(&was, t + 2 * bar)).abs() < 1e-6, "{t}");
    }
    for t in [bar, bar + 1000, 2 * bar, 3 * bar - 1] {
        assert!((at(lane, t) - at(&was, t - bar)).abs() < 1e-6, "{t}");
    }
    s.undo(1).unwrap();
    assert_eq!(clip_on(s.project(), &in_chorus).1, 2 * bar + 10);
}

#[test]
fn automation_lanes_are_saved_with_the_song_and_open_again() {
    let (_dir_tmp, dir) = temp_dir("auto-roundtrip");
    let mut s = Session::new("AutoRT");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let fx = id_of(&run(&mut s, "track.add.bus", json!({"name": "Rev"})));
    let send = id_of(&run(&mut s, "send.add", json!({"ids": [t], "to": fx})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    let targets = [
        "volume".to_string(),
        "pan".to_string(),
        "mute".to_string(),
        format!("send:{send}"),
        format!("param:{eq}:gain"),
    ];
    for (i, target) in targets.iter().enumerate() {
        let (lo, hi) = target.parse::<AutoTarget>().unwrap().range();
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": target, "at": 0, "value": lo}),
        );
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": target, "at": 960 * (i + 1), "value": hi, "curve": 0.25}),
        );
    }
    run(
        &mut s,
        "auto.lane.hide",
        json!({"track": t, "target": "mute"}),
    );
    run(&mut s, "auto.mode", json!({"ids": [t], "mode": "touch"}));
    s.save_as(&dir).unwrap();
    let before = s.project().clone();
    drop(s);

    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(text.contains("\"automation\""));
    assert!(text.contains(&format!("\"param:{eq}:gain\"")));
    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().content_hash(), before.content_hash());
    let track = s.project().track(t.parse().unwrap()).unwrap();
    assert_eq!(track.auto_mode, AutoMode::Touch);
    assert_eq!(track.automation.len(), 5);
    assert!(track.automation[2].hidden);
    assert_eq!(track.automation[4].points[1].curve, 0.25);
}

#[test]
fn a_stair_is_set_saved_opened_again_and_taken_back() {
    let (_dir_tmp, dir) = temp_dir("auto-stair");
    let mut s = Session::new("Stair");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    for (at, value) in [(0, -12.0), (960, 0.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": "volume", "at": at, "value": value}),
        );
    }
    let lane = |s: &Session| s.project().track(t.parse().unwrap()).unwrap().automation[0].clone();
    let straight = s.project().content_hash();
    run(
        &mut s,
        "auto.segment.shape",
        json!({"track": t, "target": "volume", "index": 0, "curve": 0.0, "step": true}),
    );
    assert!(lane(&s).points[0].step);
    assert_eq!(lane(&s).value_at(959.0), Some(-12.0));
    s.save_as(&dir).unwrap();
    let text = std::fs::read_to_string(dir.join("project.json")).unwrap();
    assert!(text.contains("\"step\": true") || text.contains("\"step\":true"));
    let stair = s.project().content_hash();
    drop(s);

    let mut s = Session::open(&dir).unwrap();
    assert_eq!(s.project().content_hash(), stair);
    assert_eq!(lane(&s).value_at(480.0), Some(-12.0));
    // A curve takes the stair away; one undo brings it back.
    run(
        &mut s,
        "auto.segment.shape",
        json!({"track": t, "target": "volume", "index": 0, "curve": 0.6}),
    );
    assert!(!lane(&s).points[0].step);
    assert!(lane(&s).value_at(480.0).unwrap() > -12.0);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(s.project().content_hash(), stair);
    assert_ne!(stair, straight);
}

#[test]
fn lanes_keep_their_shape_when_retargeted_and_points_stay_in_order() {
    let mut s = Session::new("lanes");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "auto.lane.add",
        json!({"track": t, "target": "pan"}),
    );
    assert!(
        try_run(
            &mut s,
            "auto.lane.add",
            json!({"track": t, "target": "pan"})
        )
        .is_err(),
        "one lane per target"
    );
    assert!(
        try_run(
            &mut s,
            "auto.lane.add",
            json!({"track": t, "target": "send:snd_999"})
        )
        .is_err(),
        "the send must exist"
    );
    for (at, v) in [(0, -1.0), (480, 0.5), (960, 1.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": "pan", "at": at, "value": v}),
        );
    }
    // A point pushes its neighbours and retains their values.
    run(
        &mut s,
        "auto.point.move",
        json!({"track": t, "target": "pan", "index": 1, "at": 5000}),
    );
    let tid: TrackId = t.parse().unwrap();
    let ticks: Vec<i64> = s.project().track(tid).unwrap().automation[0]
        .points
        .iter()
        .map(|x| x.tick)
        .collect();
    assert_eq!(ticks, vec![0, 5000, 5001]);
    // out of range is refused
    assert!(
        try_run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": "pan", "at": 10, "value": 3.0})
        )
        .is_err()
    );
    // pan -1..1 becomes mute 0/1 by height
    run(
        &mut s,
        "auto.lane.target",
        json!({"track": t, "target": "pan", "to": "volume"}),
    );
    let lane = &s.project().track(tid).unwrap().automation[0];
    assert_eq!(lane.target, AutoTarget::Volume);
    assert!(lane.points[0].value <= -100.0);
    assert!((lane.points[2].value - xirius_core::model::MAX_DB).abs() < 1e-9);
}

#[test]
fn a_duplicated_track_keeps_lanes_pointing_at_its_own_sends_and_plugins() {
    let mut s = Session::new("dup");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let fx = id_of(&run(&mut s, "track.add.bus", json!({})));
    let f = id_of(&run(&mut s, "folder.add", json!({"name": "F"})));
    let send = id_of(&run(&mut s, "send.add", json!({"ids": [t], "to": fx})));
    let eq = id_of(&run(
        &mut s,
        "plugin.add",
        json!({"track": t, "name": "EQ"}),
    ));
    for target in [format!("send:{send}"), format!("param:{eq}:gain")] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": target, "at": 0, "value": 0.0}),
        );
    }
    let copy = id_of(&run(
        &mut s,
        "track.duplicate",
        json!({"ids": [t], "into": f}),
    ));
    let p = s.project();
    let c = p.track(copy.parse().unwrap()).unwrap();
    assert_eq!(
        p.folder(f.parse().unwrap()).unwrap().children.len(),
        1,
        "the copy went into the folder"
    );
    for lane in &c.automation {
        assert!(
            lane.target.exists_on(c),
            "{} points at the copy",
            lane.target
        );
        assert!(!lane.target.to_string().contains(&send));
        assert!(!lane.target.to_string().contains(&eq));
    }
}

fn lane_points(p: &Project, t: &str, target: AutoTarget) -> Vec<(i64, f64)> {
    p.track(t.parse().unwrap())
        .unwrap()
        .automation
        .iter()
        .find(|l| l.target == target)
        .map(|l| l.points.iter().map(|x| (x.tick, x.value)).collect())
        .unwrap_or_default()
}

#[test]
fn reordering_sections_carries_their_tempo_and_meter_changes() {
    let mut s = Session::new("tempo-sections");
    let (bar, bar3) = (3840, 2880);
    run(&mut s, "tempo.set", json!({"bpm": 100}));
    // Verse: 2 bars of 4/4. Chorus: 2 bars of 3/4 at 140. Outro: 1 bar, still 3/4 and 140.
    run(&mut s, "timesig.set", json!({"num": 3, "den": 4, "bar": 3}));
    run(&mut s, "tempo.set", json!({"bpm": 140, "at": 2 * bar}));
    let verse = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Verse", "at": 0, "length": 2 * bar}),
    ));
    let chorus = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Chorus", "at": 2 * bar, "length": 2 * bar3}),
    ));
    let outro_at = 2 * bar + 2 * bar3;
    let outro = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Outro", "at": outro_at, "length": bar3}),
    ));
    run(&mut s, "section.reorder", json!({"id": chorus, "index": 0}));
    let p = s.project();
    let starts: Vec<(String, i64)> = p
        .sections
        .iter()
        .map(|x| (x.id.to_string(), x.start))
        .collect();
    assert_eq!(
        starts,
        vec![
            (chorus.clone(), 0),
            (verse.clone(), 2 * bar3),
            (outro.clone(), outro_at)
        ]
    );
    let tempo: Vec<(i64, f64)> = p.tempo.iter().map(|t| (t.tick, t.bpm)).collect();
    // the chorus opens the song at its own tempo, the verse keeps 100 after
    // it, and the outro is played as fast as before
    assert_eq!(
        tempo,
        vec![(0, 140.0), (2 * bar3, 100.0), (outro_at, 140.0)]
    );
    let meters: Vec<(u32, u32, u32)> = p.timesig.iter().map(|t| (t.bar, t.num, t.den)).collect();
    // the chorus's two bars of 3/4 lead, the verse is 4/4 from bar 3, the
    // outro is in 3/4 again from bar 5, where it starts
    assert_eq!(meters, vec![(1, 3, 4), (3, 4, 4), (5, 3, 4)]);
    assert_eq!(p.time_map().bar_start(5), outro_at);
    s.undo(1).unwrap();
    let back: Vec<(i64, f64)> = s.project().tempo.iter().map(|t| (t.tick, t.bpm)).collect();
    assert_eq!(back, vec![(0, 100.0), (2 * bar, 140.0)]);
}

#[test]
fn a_section_stretched_into_the_next_stops_just_before_it() {
    let mut s = Session::new("sections-wall");
    let bar = 3840;
    let a = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "A", "at": 0, "bars": 2}),
    ));
    let b = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "B", "at": 4 * bar, "bars": 2}),
    ));
    run(&mut s, "section.resize", json!({"id": a, "bars": 8}));
    let p = s.project();
    assert_eq!(p.sections[0].length, 4 * bar, "stops at B's start");
    assert!(p.sections[0].start + p.sections[0].length <= p.sections[1].start);
    // already against it: nothing to do
    assert!(try_run(&mut s, "section.resize", json!({"id": a, "bars": 6})).is_err());
    // the last section has nothing after it
    run(&mut s, "section.resize", json!({"id": b, "bars": 10}));
    assert_eq!(s.project().sections[1].length, 10 * bar);
    // shrinking is never stopped
    run(&mut s, "section.resize", json!({"id": a, "bars": 1}));
    assert_eq!(s.project().sections[0].length, bar);
}

#[test]
fn moving_a_clip_carries_the_automation_under_it() {
    let mut s = Session::new("follow");
    let a = id_of(&run(&mut s, "track.add.audio", json!({"name": "A"})));
    let b = id_of(&run(&mut s, "track.add.audio", json!({"name": "B"})));
    let clip = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": a, "at": 960, "length": 960, "file": "x.wav"}),
    ));
    // a ramp under the clip, flat -20 before and after
    for (at, v) in [(0, -20.0), (1200, -6.0), (1700, 0.0), (3000, -20.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": a, "target": "volume", "at": at, "value": v}),
        );
    }
    let before_at = |p: &Project, t: f64| {
        p.track(a.parse().unwrap()).unwrap().automation[0]
            .value_at(t)
            .unwrap()
    };
    let outside: Vec<f64> = [500.0, 2500.0]
        .iter()
        .map(|t| before_at(s.project(), *t))
        .collect();
    run(&mut s, "clip.move", json!({"ids": [clip], "by": 3840}));
    let p = s.project();
    let pts = lane_points(p, &a, AutoTarget::Volume);
    // the inner points went 3840 later
    assert!(pts.contains(&(1200 + 3840, -6.0)), "{pts:?}");
    assert!(pts.contains(&(1700 + 3840, 0.0)), "{pts:?}");
    assert!(!pts.iter().any(|x| x.0 == 1200 || x.0 == 1700), "{pts:?}");
    // the line outside the old place is as it was
    for (t, v) in [500.0, 2500.0].iter().zip(&outside) {
        assert!((before_at(p, *t) - v).abs() < 1e-6, "at {t}");
    }
    // one step back
    s.undo(1).unwrap();
    assert!(lane_points(s.project(), &a, AutoTarget::Volume).contains(&(1200, -6.0)));

    // to another track: the volume goes along
    run(&mut s, "clip.move", json!({"ids": [clip], "track": b}));
    let pb = lane_points(s.project(), &b, AutoTarget::Volume);
    assert!(pb.contains(&(1200, -6.0)), "{pb:?}");
    s.undo(1).unwrap();

    // stay: the old way
    run(
        &mut s,
        "clip.move",
        json!({"ids": [clip], "by": 3840, "automation": "stay"}),
    );
    assert!(lane_points(s.project(), &a, AutoTarget::Volume).contains(&(1200, -6.0)));
}

#[test]
fn one_pass_of_writing_is_one_step() {
    let mut s = Session::new("write");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "auto.point.add",
        json!({"track": t, "target": "volume", "at": 0, "value": -10.0}),
    );
    run(
        &mut s,
        "auto.write",
        json!({"writes": [
            {"track": t, "target": "volume", "from": 960, "to": 1920,
             "points": [{"tick": 960, "value": -3.0}, {"tick": 1920, "value": -10.0}]},
            {"track": t, "target": "pan", "from": 0, "to": 1920,
             "points": [{"tick": 0, "value": -1.0}, {"tick": 1920, "value": 1.0}]}
        ]}),
    );
    let p = s.project();
    assert_eq!(
        lane_points(p, &t, AutoTarget::Volume),
        vec![
            (0, -10.0),
            (959, -10.0),
            (960, -3.0),
            (1920, -10.0),
            (1921, -10.0)
        ]
    );
    assert_eq!(
        lane_points(p, &t, AutoTarget::Pan),
        vec![(0, -1.0), (1920, 1.0), (1921, 0.0)]
    );
    s.undo(1).unwrap();
    let p = s.project();
    assert_eq!(lane_points(p, &t, AutoTarget::Volume), vec![(0, -10.0)]);
    assert!(lane_points(p, &t, AutoTarget::Pan).is_empty());
    // a point outside its stretch is refused
    assert!(
        try_run(
            &mut s,
            "auto.write",
            json!({"writes": [{"track": t, "target": "pan", "from": 0, "to": 10,
                               "points": [{"tick": 20, "value": 0.0}]}]}),
        )
        .is_err()
    );
}

#[test]
fn points_are_cleared_in_the_loop_or_all_and_one_undo_takes_each_back() {
    let mut s = Session::new("clear");
    let t = id_of(&run(&mut s, "track.add.audio", json!({"name": "Vox"})));
    let lane = json!({"track": t, "target": "volume"});
    for at in [0, 960, 1920, 2880] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": t, "target": "volume", "at": at, "value": -6.0}),
        );
    }
    let ticks = |s: &Session| -> Vec<i64> {
        let track = s.project().track(t.parse().unwrap()).unwrap();
        track.automation[0].points.iter().map(|p| p.tick).collect()
    };
    let mut loop_args = lane.clone();
    loop_args["range"] = json!("loop");
    assert!(
        try_run(&mut s, "auto.clear", loop_args.clone()).is_err(),
        "no loop yet"
    );
    run(&mut s, "loop.set", json!({"start": 960, "end": 2880}));
    run(&mut s, "auto.clear", loop_args);
    assert_eq!(ticks(&s), [0, 2880]);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(ticks(&s), [0, 960, 1920, 2880]);
    run(&mut s, "auto.clear", lane.clone());
    assert!(ticks(&s).is_empty());
    assert!(
        try_run(&mut s, "auto.clear", lane).is_err(),
        "nothing left to clear"
    );
    run(&mut s, "history.undo", json!({}));
    assert_eq!(ticks(&s), [0, 960, 1920, 2880]);
}

#[test]
fn clearing_the_tempo_leaves_the_first_point_and_one_undo_brings_the_rest_back() {
    let mut s = Session::new("tempo clear");
    assert!(
        try_run(&mut s, "tempo.clear", json!({})).is_err(),
        "only the first point"
    );
    run(&mut s, "tempo.set", json!({"at": 3840, "bpm": 90}));
    run(
        &mut s,
        "tempo.set",
        json!({"at": 7680, "bpm": 140, "ramp": true}),
    );
    assert_eq!(s.project().tempo.len(), 3);
    run(&mut s, "tempo.clear", json!({}));
    assert_eq!(s.project().tempo.len(), 1);
    assert_eq!(s.project().tempo[0].tick, 0);
    run(&mut s, "history.undo", json!({}));
    assert_eq!(s.project().tempo.len(), 3);
}

#[test]
fn set_001_quantizing_clips_snaps_their_starts_and_carries_the_automation() {
    let mut s = Session::new("quantize");
    let a = id_of(&run(&mut s, "track.add.audio", json!({"name": "A"})));
    let near = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": a, "at": 1000, "length": 480, "file": "x.wav"}),
    ));
    let on = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": a, "at": 3840, "length": 480, "file": "y.wav"}),
    ));
    run(
        &mut s,
        "auto.point.add",
        json!({"track": a, "target": "volume", "at": 1100, "value": -6.0}),
    );
    // 1/4 of a bar is 960 ticks: 1000 goes to 960, the clip on a line stays.
    let out = run(
        &mut s,
        "clip.quantize",
        json!({"ids": [near, on], "grid": "1/4"}),
    );
    assert!(out.message.contains("2 件中 1 件"), "{}", out.message);
    assert_eq!(clip_on(s.project(), &near), (a.parse().unwrap(), 960, 480));
    assert_eq!(clip_on(s.project(), &on).1, 3840);
    let pts = lane_points(s.project(), &a, AutoTarget::Volume);
    assert!(pts.contains(&(1060, -6.0)), "{pts:?}");
    // half strength moves half of the way; one undo takes everything back
    run(&mut s, "clip.move", json!({"ids": [near], "by": 40}));
    run(
        &mut s,
        "clip.quantize",
        json!({"ids": [near], "grid": "1/4", "strength": 50}),
    );
    assert_eq!(clip_on(s.project(), &near).1, 980);
    s.undo(3).unwrap();
    assert_eq!(clip_on(s.project(), &near).1, 1000);
    // no clip: an error that says so
    assert!(try_run(&mut s, "clip.quantize", json!({"grid": "1/4"})).is_err());
}

fn meters(p: &Project) -> Vec<(u32, u32, u32)> {
    p.timesig.iter().map(|t| (t.bar, t.num, t.den)).collect()
}

/// The bar that starts at `tick`, and its meter; none when no bar starts there.
fn bar_at(p: &Project, tick: i64) -> Option<(u32, u32, u32)> {
    let map = p.time_map();
    let (bar, ..) = map.to_bbt(tick);
    let m = map.meter_at(bar);
    (map.bar_start(bar) == tick).then_some((bar, m.num, m.den))
}

#[test]
fn a_meter_change_in_a_section_off_the_bar_lines_stays_with_its_music() {
    let mut s = Session::new("meter-off-bars");
    let (bar, bar3) = (3840, 2880);
    // 4/4, then 3/4 from bar 3. The verse is a bar and a half; the chorus starts half way
    // through bar 2, so its 3/4 begins half a bar into it.
    run(&mut s, "timesig.set", json!({"num": 3, "den": 4, "bar": 3}));
    let half = bar / 2;
    id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Verse", "at": 0, "length": bar + half}),
    ));
    let chorus_at = bar + half;
    let chorus = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Chorus", "at": chorus_at, "length": half + 2 * bar3}),
    ));
    let outro_at = 2 * bar + 2 * bar3;
    id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "Outro", "at": outro_at, "length": bar3}),
    ));
    let lead = id_of(&run(&mut s, "track.add.instrument", json!({})));
    // the chorus's riff starts on its first bar of 3/4, the outro's on its own bar
    run(
        &mut s,
        "clip.add.midi",
        json!({"track": lead, "at": 2 * bar, "length": 2 * bar3}),
    );
    run(
        &mut s,
        "clip.add.midi",
        json!({"track": lead, "at": outro_at, "length": bar3}),
    );

    run(&mut s, "section.reorder", json!({"id": chorus, "index": 0}));
    let p = s.project();
    let riff = p
        .tracks
        .values()
        .find_map(|t| t.clips.first().cloned())
        .unwrap();
    assert_eq!(riff.start, half, "the riff moved with the chorus");
    // A short bar of 2/4 opens the song, so the 3/4 starts under the riff as it did; the verse
    // comes in on a bar line in 4/4, and a short bar brings the outro's 3/4 back on its tick.
    assert_eq!(bar_at(p, half), Some((2, 3, 4)));
    assert_eq!(bar_at(p, half + 2 * bar3), Some((4, 4, 4)));
    assert_eq!(bar_at(p, outro_at), Some((6, 3, 4)));
    assert_eq!(
        meters(p),
        vec![(1, 2, 4), (2, 3, 4), (4, 4, 4), (5, 2, 4), (6, 3, 4)]
    );
    s.undo(1).unwrap();
    assert_eq!(meters(s.project()), vec![(1, 4, 4), (3, 3, 4)]);
}

#[test]
fn a_song_of_one_meter_keeps_its_bars_when_sections_off_the_bar_lines_move() {
    let mut s = Session::new("one-meter");
    let bar = 3840;
    id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "A", "at": 0, "length": 2 * bar}),
    ));
    let b = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "B", "at": 2 * bar, "length": bar + bar / 2}),
    ));
    run(&mut s, "section.reorder", json!({"id": b, "index": 0}));
    // no short bar: every bar is a bar of 4/4, as before
    let p = s.project();
    assert!(
        meters(p).iter().all(|m| (m.1, m.2) == (4, 4)),
        "{:?}",
        meters(p)
    );
    assert_eq!(p.time_map().bar_start(4), 3 * bar);
}

#[test]
fn a_duplicated_section_off_the_bar_lines_keeps_its_meter_change_on_its_tick() {
    let mut s = Session::new("meter-dup");
    let (bar, bar3) = (3840, 2880);
    run(&mut s, "timesig.set", json!({"num": 3, "den": 4, "bar": 2}));
    // half a bar of 4/4, then a bar of 3/4 from bar 2
    let at = bar / 2;
    let sec = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "S", "at": at, "length": bar / 2 + bar3}),
    ));
    run(&mut s, "section.duplicate", json!({"id": sec}));
    let p = s.project();
    let copy = at + bar / 2 + bar3;
    // the copy's half bar is a short bar of 2/4 and its 3/4 starts half a bar in, as in the
    // original
    assert_eq!(bar_at(p, copy + bar / 2), Some((4, 3, 4)));
    assert_eq!(meters(p), vec![(1, 4, 4), (2, 3, 4), (3, 2, 4), (4, 3, 4)]);
}
