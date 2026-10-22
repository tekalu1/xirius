//! KEY-001 / OPS-014: the commands the Studio Pro key table asks for — where the play position can
//! jump, nudging clips by the grid or a bar, cutting on every grid line and at the loop.
use serde_json::json;
use xirius_core::nav::{self, TARGETS};
use xirius_core::session::{RunOptions, Session};

fn song() -> (Session, String) {
    let mut s = Session::new("key rows");
    let opts = RunOptions::human();
    let track = s
        .run_value("track.add.instrument", json!({"name": "Keys"}), &opts)
        .unwrap()
        .data["id"]
        .as_str()
        .unwrap()
        .to_string();
    (s, track)
}

fn add_clip(s: &mut Session, track: &str, at: i64, length: i64) -> String {
    s.run_value(
        "clip.add.midi",
        json!({"track": track, "at": at, "length": length}),
        &RunOptions::human(),
    )
    .unwrap()
    .data["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn clips(s: &Session) -> Vec<(i64, i64)> {
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
fn the_jump_targets_land_on_markers_the_loop_beats_bars_and_clip_edges() {
    let (mut s, track) = song();
    let opts = RunOptions::human();
    add_clip(&mut s, &track, 3840, 1920);
    s.run_value("marker.add", json!({"at": 960}), &opts)
        .unwrap();
    s.run_value("marker.add", json!({"at": 7680}), &opts)
        .unwrap();
    s.run_value("loop.set", json!({"start": 1920, "end": 5760}), &opts)
        .unwrap();
    let p = s.project();
    let at = |from, to| nav::destination(p, from, to).unwrap();
    assert_eq!(at(2000, "start"), 0);
    assert_eq!(at(0, "end"), 5760);
    assert_eq!(at(0, "loop-start"), 1920);
    assert_eq!(at(0, "loop-end"), 5760);
    assert_eq!(at(0, "next-marker"), 960);
    assert_eq!(at(960, "next-marker"), 7680);
    assert_eq!(at(8000, "prev-marker"), 7680);
    assert_eq!(at(7680, "prev-marker"), 960);
    assert_eq!(at(0, "marker-2"), 7680);
    // 4/4 at 960 ticks a beat: a bar is 3840 ticks.
    assert_eq!(at(100, "next-beat"), 960);
    assert_eq!(at(1000, "prev-beat"), 960);
    assert_eq!(at(960, "prev-beat"), 0);
    assert_eq!(at(100, "next-bar"), 3840);
    assert_eq!(at(3840, "next-bar"), 7680);
    assert_eq!(at(4000, "prev-bar"), 3840);
    assert_eq!(at(3840, "prev-bar"), 0);
    assert_eq!(at(0, "next-event"), 3840);
    assert_eq!(at(3840, "next-event"), 5760);
    assert_eq!(at(5760, "prev-event"), 3840);
}

#[test]
fn a_jump_with_nowhere_to_go_says_so_and_bad_names_are_refused() {
    let (s, _) = song();
    let p = s.project();
    for to in ["next-marker", "prev-marker", "loop-start", "marker-1"] {
        let e = nav::destination(p, 0, to).unwrap_err();
        assert!(!e.to_string().is_empty(), "{to}");
    }
    assert!(nav::destination(p, 0, "sideways").is_err());
    assert!(nav::destination(p, 0, "marker-0").is_err());
    assert!(nav::destination(p, 0, "marker-x").is_err());
    // Every documented name is understood (the ones that need a target fail with a reason, not "unknown").
    for to in TARGETS {
        if let Err(e) = nav::destination(p, 0, to) {
            assert!(!e.to_string().contains("使えません"), "{to}");
        }
    }
}

#[test]
fn nudge_moves_by_the_grid_or_a_bar_and_one_undo_restores() {
    let (mut s, track) = song();
    let opts = RunOptions::human();
    let id = add_clip(&mut s, &track, 3840, 960);
    let before = s.project().clone();
    s.run_value(
        "clip.nudge",
        json!({"ids": [id], "dir": "right", "grid": "1/16"}),
        &opts,
    )
    .unwrap();
    assert_eq!(clips(&s), [(3840 + 240, 960)]);
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(s.project(), &before);
    s.run_value(
        "clip.nudge",
        json!({"ids": [id], "dir": "left", "unit": "bar"}),
        &opts,
    )
    .unwrap();
    assert_eq!(clips(&s), [(0, 960)]);
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(s.project(), &before);
}

#[test]
fn nudge_will_not_go_before_the_start_of_the_song() {
    let (mut s, track) = song();
    let opts = RunOptions::human();
    let id = add_clip(&mut s, &track, 0, 960);
    let before = s.project().clone();
    assert!(
        s.run_value(
            "clip.nudge",
            json!({"ids": [id], "dir": "left", "grid": 120}),
            &opts
        )
        .is_err()
    );
    assert_eq!(s.project(), &before);
}

#[test]
fn split_at_grid_cuts_on_every_line_inside_the_clip_in_one_step() {
    let (mut s, track) = song();
    let opts = RunOptions::human();
    let id = add_clip(&mut s, &track, 100, 1000);
    let before = s.project().clone();
    s.run_value("clip.split.grid", json!({"ids": [id], "grid": 480}), &opts)
        .unwrap();
    // Lines at 480 and 960 fall inside 100..1100.
    assert_eq!(clips(&s), [(100, 380), (480, 480), (960, 140)]);
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(s.project(), &before);
    // A grid wider than the clip has no line inside it.
    let small = add_clip(&mut s, &track, 1000, 100);
    assert!(
        s.run_value(
            "clip.split.grid",
            json!({"ids": [small], "grid": 1920}),
            &opts
        )
        .is_err()
    );
}

#[test]
fn split_loop_cuts_at_both_ends_of_the_loop() {
    let (mut s, track) = song();
    let opts = RunOptions::human();
    let id = add_clip(&mut s, &track, 0, 3840);
    assert!(
        s.run_value(
            "clip.split.range",
            json!({"ids": [id], "loop": true}),
            &opts
        )
        .is_err(),
        "no loop yet"
    );
    s.run_value("loop.set", json!({"start": 960, "end": 1920}), &opts)
        .unwrap();
    let before = s.project().clone();
    s.run_value(
        "clip.split.range",
        json!({"ids": [id], "loop": true}),
        &opts,
    )
    .unwrap();
    assert_eq!(clips(&s), [(0, 960), (960, 960), (1920, 1920)]);
    s.run_value("history.undo", json!({}), &opts).unwrap();
    assert_eq!(s.project(), &before);
}
