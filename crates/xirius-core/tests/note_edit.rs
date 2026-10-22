//! The compound note commands (`note.set`, `note.duplicate`, `note.split`) and
//! the controller lanes (`cc.replace`): what they do, that each is one history
//! step, and that lanes survive a clip split and a save / open.

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::model::{CtrlKind, CtrlPoint};
use xirius_core::session::RunOptions;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

/// A song with one MIDI clip (2 bars) holding two notes. Returns (session, clip, notes).
fn song() -> (Session, String, Vec<String>) {
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
    let mut notes = Vec::new();
    for (at, pitch) in [(0, 60), (960, 64)] {
        notes.push(id(&run(
            &mut s,
            "note.add",
            json!({"clip": clip, "at": at, "pitch": pitch, "length": 480}),
        )));
    }
    (s, clip, notes)
}

fn notes_of(s: &Session) -> Vec<(i64, i64, u8, u8)> {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.notes().iter())
        .map(|n| (n.start, n.length, n.pitch, n.velocity))
        .collect()
}

#[test]
fn note_set_changes_each_note_to_its_own_values_in_one_step() {
    let (mut s, _, n) = song();
    let before = s.steps().len();
    run(
        &mut s,
        "note.set",
        json!({"notes": [
            {"id": n[0], "start": 10, "length": 100, "pitch": 62},
            {"id": n[1], "velocity": 30},
        ]}),
    );
    assert_eq!(s.steps().len(), before + 1);
    assert_eq!(notes_of(&s), [(10, 100, 62, 100), (960, 480, 64, 30)]);
    s.undo(1).unwrap();
    assert_eq!(notes_of(&s), [(0, 480, 60, 100), (960, 480, 64, 100)]);
}

#[test]
fn note_set_refuses_what_is_out_of_range_and_changes_nothing() {
    let (mut s, _, n) = song();
    let steps = s.steps().len();
    for bad in [
        json!({"id": n[0], "start": -1}),
        json!({"id": n[0], "length": 0}),
        json!({"id": n[0], "pitch": 128}),
        json!({"id": n[0], "velocity": 0}),
    ] {
        assert!(
            s.run_value("note.set", json!({"notes": [bad]}), &RunOptions::human())
                .is_err()
        );
    }
    assert!(
        s.run_value(
            "note.set",
            json!({"notes": [{"id": "nte_9999", "start": 1}]}),
            &RunOptions::human()
        )
        .is_err()
    );
    assert_eq!(s.steps().len(), steps);
    assert_eq!(notes_of(&s), [(0, 480, 60, 100), (960, 480, 64, 100)]);
}

#[test]
fn note_duplicate_puts_moved_copies_and_names_them() {
    let (mut s, _, n) = song();
    let out = run(
        &mut s,
        "note.duplicate",
        json!({"ids": [n[0], n[1]], "by": 1920, "pitch_by": 12}),
    );
    let ids = out["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2);
    assert!(!ids.iter().any(|i| i == &json!(n[0]) || i == &json!(n[1])));
    let all = notes_of(&s);
    assert_eq!(all.len(), 4);
    assert!(all.contains(&(1920, 480, 72, 100)) && all.contains(&(2880, 480, 76, 100)));
    s.undo(1).unwrap();
    assert_eq!(notes_of(&s).len(), 2);
}

#[test]
fn note_duplicate_off_the_keyboard_or_before_the_clip_is_refused() {
    let (mut s, _, n) = song();
    let h = RunOptions::human();
    assert!(
        s.run_value(
            "note.duplicate",
            json!({"ids": [n[0]], "by": 0, "pitch_by": 80}),
            &h
        )
        .is_err()
    );
    assert!(
        s.run_value("note.duplicate", json!({"ids": [n[0]], "by": -5}), &h)
            .is_err()
    );
    assert_eq!(notes_of(&s).len(), 2);
}

#[test]
fn note_put_adds_many_notes_in_one_step_and_names_them() {
    let (mut s, clip, _) = song();
    let steps = s.steps().len();
    let out = run(
        &mut s,
        "note.put",
        json!({"clip": clip, "notes": [
            {"at": 100, "pitch": 70, "length": 50},
            {"at": 200, "pitch": 71, "length": 50, "velocity": 40},
        ]}),
    );
    assert_eq!(out["ids"].as_array().unwrap().len(), 2);
    assert_eq!(s.steps().len(), steps + 1);
    let all = notes_of(&s);
    assert!(all.contains(&(100, 50, 70, 100)) && all.contains(&(200, 50, 71, 40)));
    s.undo(1).unwrap();
    assert_eq!(notes_of(&s).len(), 2);
}

#[test]
fn note_put_refuses_a_note_outside_the_clip_or_the_keyboard_and_adds_none() {
    let (mut s, clip, _) = song();
    let h = RunOptions::human();
    let steps = s.steps().len();
    for bad in [
        json!({"at": -1, "pitch": 60, "length": 10}),
        json!({"at": 2 * 3840, "pitch": 60, "length": 10}),
        json!({"at": 0, "pitch": 128, "length": 10}),
        json!({"at": 0, "pitch": 60, "length": 0}),
        json!({"at": 0, "pitch": 60}),
    ] {
        let args = json!({"clip": clip, "notes": [{"at": 5, "pitch": 60, "length": 10}, bad]});
        assert!(s.run_value("note.put", args, &h).is_err());
    }
    assert!(
        s.run_value("note.put", json!({"clip": clip, "notes": []}), &h)
            .is_err()
    );
    assert_eq!(s.steps().len(), steps);
    assert_eq!(notes_of(&s).len(), 2);
}

fn clip_length(s: &Session) -> i64 {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.length)
        .next()
        .expect("a clip")
}

#[test]
fn note_put_with_grow_places_a_note_past_the_clip_end_and_extends_to_the_bar_end() {
    let (mut s, clip, _) = song();
    let h = RunOptions::human();
    assert_eq!(clip_length(&s), 2 * 3840);
    // Without grow, a note past the end is still refused.
    let past = json!({"at": 2 * 3840 + 960, "pitch": 60, "length": 960});
    assert!(
        s.run_value("note.put", json!({"clip": clip, "notes": [past]}), &h)
            .is_err()
    );
    assert_eq!(clip_length(&s), 2 * 3840);
    // With grow, one step adds the note and ends the clip at the end of bar 3.
    let steps = s.steps().len();
    run(
        &mut s,
        "note.put",
        json!({"clip": clip, "grow": true, "notes": [past]}),
    );
    assert_eq!(s.steps().len(), steps + 1);
    assert_eq!(clip_length(&s), 3 * 3840);
    assert!(notes_of(&s).contains(&(2 * 3840 + 960, 960, 60, 100)));
    // A note inside the clip leaves the length alone.
    run(
        &mut s,
        "note.put",
        json!({"clip": clip, "grow": true, "notes": [{"at": 10, "pitch": 61, "length": 10}]}),
    );
    assert_eq!(clip_length(&s), 3 * 3840);
    // A note that ends exactly on a bar line stays in that bar.
    run(
        &mut s,
        "note.put",
        json!({"clip": clip, "grow": true, "notes": [{"at": 3 * 3840, "pitch": 62, "length": 3840}]}),
    );
    assert_eq!(clip_length(&s), 4 * 3840);
    s.undo(1).unwrap();
    assert_eq!(clip_length(&s), 3 * 3840);
    s.undo(2).unwrap();
    assert_eq!(clip_length(&s), 2 * 3840);
}

#[test]
fn note_split_cuts_a_note_in_two_and_leaves_the_others() {
    let (mut s, _, n) = song();
    run(
        &mut s,
        "note.split",
        json!({"ids": [n[0], n[1]], "at": 240}),
    );
    let mut all = notes_of(&s);
    all.sort();
    assert_eq!(
        all,
        [(0, 240, 60, 100), (240, 240, 60, 100), (960, 480, 64, 100)]
    );
    // A cut on a note's edge does not split it.
    assert!(
        s.run_value(
            "note.split",
            json!({"ids": [n[1]], "at": 960}),
            &RunOptions::human()
        )
        .is_err()
    );
}

fn points(s: &Session, clip: &str, kind: CtrlKind) -> Vec<(i64, i32)> {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| c.id.to_string() == clip)
        .flat_map(|c| c.controllers.iter())
        .filter(|l| l.kind == kind)
        .flat_map(|l| l.points.iter().map(|p: &CtrlPoint| (p.tick, p.value)))
        .collect()
}

#[test]
fn cc_replace_rewrites_a_stretch_and_keeps_the_rest_in_order() {
    let (mut s, clip, _) = song();
    let put = |s: &mut Session, from: i64, to: i64, pts: Value| {
        run(
            s,
            "cc.replace",
            json!({"clip": clip, "lane": "cc1", "from": from, "to": to, "points": pts}),
        );
    };
    put(
        &mut s,
        0,
        960,
        json!([{"tick": 0, "value": 10}, {"tick": 480, "value": 50}, {"tick": 960, "value": 90}]),
    );
    assert_eq!(
        points(&s, &clip, CtrlKind::Cc(1)),
        [(0, 10), (480, 50), (960, 90)]
    );
    // Replace the middle; the ends stay.
    put(&mut s, 100, 700, json!([{"tick": 300, "value": 77}]));
    assert_eq!(
        points(&s, &clip, CtrlKind::Cc(1)),
        [(0, 10), (300, 77), (960, 90)]
    );
    // An empty list clears the stretch; clearing all drops the lane.
    put(&mut s, 0, 2000, json!([]));
    assert!(points(&s, &clip, CtrlKind::Cc(1)).is_empty());
    assert!(
        s.project()
            .tracks
            .values()
            .flat_map(|t| t.clips.iter())
            .all(|c| c.controllers.is_empty())
    );
}

#[test]
fn cc_values_are_held_to_the_range_of_the_controller_and_each_edit_is_one_step() {
    let (mut s, clip, _) = song();
    let steps = s.steps().len();
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "bend", "from": 0, "to": 10,
               "points": [{"tick": 0, "value": 99999}, {"tick": 10, "value": -99999}]}),
    );
    assert_eq!(
        points(&s, &clip, CtrlKind::PitchBend),
        [(0, 8191), (10, -8192)]
    );
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc64", "from": 0, "to": 0,
               "points": [{"tick": 0, "value": 300}]}),
    );
    assert_eq!(points(&s, &clip, CtrlKind::Cc(64)), [(0, 127)]);
    assert_eq!(s.steps().len(), steps + 2);
    s.undo(2).unwrap();
    assert!(points(&s, &clip, CtrlKind::PitchBend).is_empty());
    s.redo(2).unwrap();
    assert_eq!(points(&s, &clip, CtrlKind::Cc(64)), [(0, 127)]);
}

#[test]
fn cc_replace_refuses_bad_lanes_ranges_and_points_outside_the_stretch() {
    let (mut s, clip, _) = song();
    let h = RunOptions::human();
    let steps = s.steps().len();
    for args in [
        json!({"clip": clip, "lane": "cc200", "from": 0, "to": 1}),
        json!({"clip": clip, "lane": "volume", "from": 0, "to": 1}),
        json!({"clip": clip, "lane": "cc1", "from": 5, "to": 1}),
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 10, "points": [{"tick": 11, "value": 1}]}),
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 10, "points": [{"tick": 1}]}),
    ] {
        assert!(
            s.run_value("cc.replace", args.clone(), &h).is_err(),
            "{args}"
        );
    }
    assert_eq!(s.steps().len(), steps);
}

#[test]
fn a_clip_split_carries_the_lanes_and_repeats_the_value_at_the_cut() {
    let (mut s, clip, _) = song();
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 1500,
               "points": [{"tick": 0, "value": 20}, {"tick": 1500, "value": 100}]}),
    );
    let out = run(&mut s, "clip.split", json!({"ids": [clip], "at": 960}));
    let right = out["ids"][0].as_str().unwrap().to_string();
    assert_eq!(points(&s, &clip, CtrlKind::Cc(1)), [(0, 20)]);
    // The right clip starts at 960: the value 20 still holds there, then 100 at 540.
    assert_eq!(points(&s, &right, CtrlKind::Cc(1)), [(0, 20), (540, 100)]);
}

#[test]
fn lanes_survive_saving_and_opening_the_song() {
    let (mut s, clip, _) = song();
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "at", "from": 0, "to": 480,
               "points": [{"tick": 0, "value": 5}, {"tick": 480, "value": 70}]}),
    );
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc11", "from": 0, "to": 0,
               "points": [{"tick": 0, "value": 64}]}),
    );
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-note-edit-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("note-edit");
    let _ = std::fs::remove_dir_all(&dir);
    s.save_as(&dir).unwrap();
    let before = s.project().content_hash();
    drop(s);
    let s = Session::open(&dir).unwrap();
    assert_eq!(s.project().content_hash(), before);
    assert_eq!(points(&s, &clip, CtrlKind::Aftertouch), [(0, 5), (480, 70)]);
    assert_eq!(points(&s, &clip, CtrlKind::Cc(11)), [(0, 64)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_song_without_lanes_writes_no_lane_field_and_reads_back() {
    let (s, _, _) = song();
    let text = serde_json::to_string(s.project()).unwrap();
    assert!(!text.contains("controllers"));
    let back: xirius_core::model::Project = serde_json::from_str(&text).unwrap();
    assert!(
        back.tracks
            .values()
            .flat_map(|t| t.clips.iter())
            .all(|c| c.controllers.is_empty())
    );
}

#[test]
fn controller_keys_read_and_write_the_same_text() {
    for key in ["cc1", "cc11", "cc64", "bend", "at"] {
        assert_eq!(CtrlKind::parse(key).unwrap().key(), key);
    }
    assert!(CtrlKind::parse("cc128").is_none() && CtrlKind::parse("x").is_none());
}

#[test]
fn cc_point_set_sets_the_value_of_the_named_points_in_one_step() {
    let (mut s, clip, _) = song();
    run(
        &mut s,
        "cc.replace",
        json!({"clip": clip, "lane": "cc1", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 10}, {"tick": 480, "value": 50}, {"tick": 960, "value": 90}]}),
    );
    let steps = s.steps().len();
    run(
        &mut s,
        "cc.point.set",
        json!({"clip": clip, "lane": "cc1", "ticks": "0, 960", "value": 200}),
    );
    assert_eq!(
        points(&s, &clip, CtrlKind::Cc(1)),
        [(0, 127), (480, 50), (960, 127)]
    );
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(
        points(&s, &clip, CtrlKind::Cc(1)),
        [(0, 10), (480, 50), (960, 90)]
    );

    // No point there, or no such lane: an error, and nothing changes.
    let none = s.run_value(
        "cc.point.set",
        json!({"clip": clip, "lane": "cc1", "ticks": "100", "value": 5}),
        &RunOptions::human(),
    );
    assert!(none.is_err());
    let lane = s.run_value(
        "cc.point.set",
        json!({"clip": clip, "lane": "cc11", "ticks": "0", "value": 5}),
        &RunOptions::human(),
    );
    assert!(lane.is_err());
    assert_eq!(s.steps().len(), steps + 1);
}

/// B-4: `note.put`'s notes take `at` and `length` as `bar.beat.tick` too. `at` counts from
/// the clip's start, as a tick count does.
#[test]
fn note_put_takes_a_position_as_bar_beat_tick() {
    let (mut s, clip, _) = song();
    let out = run(
        &mut s,
        "note.put",
        json!({"clip": clip, "notes": [
            {"at": "1.2.0", "pitch": 70, "length": "1.3.0"}
        ]}),
    );
    assert_eq!(out["ids"].as_array().unwrap().len(), 1);
    assert!(
        notes_of(&s).contains(&(960, 1920, 70, 100)),
        "{:?}",
        notes_of(&s)
    );
}

/// B-4: an `at` that cannot be read names the shape, rather than saying it is missing.
#[test]
fn note_put_says_the_shape_of_an_at_it_cannot_read() {
    let (mut s, clip, _) = song();
    let e = s
        .run_value(
            "note.put",
            json!({"clip": clip, "notes": [{"at": "soon", "pitch": 60, "length": 10}]}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert!(e.message.contains("tick の整数か"), "{e:?}");
    assert!(e.message.contains("小節.拍.tick"), "{e:?}");
    assert!(!e.message.contains("ありません"), "{e:?}");
}
