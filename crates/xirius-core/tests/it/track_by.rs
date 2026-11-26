//! `track.volume.set --by` and `track.pan.set --by` (CLI-003): each track moves from where it is,
//! a track at the end of the range stops there alone, and one run is one undo.

use serde_json::{Value, json};
use xirius_core::error::ErrorCode;
use xirius_core::id::TrackId;
use xirius_core::model::MIN_DB;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

fn fail(s: &mut Session, name: &str, args: Value) -> ErrorCode {
    s.run_value(name, args, &RunOptions::human())
        .expect_err("should fail")
        .code
}

fn add(s: &mut Session, name: &str) -> String {
    let out = s
        .run_value(
            "track.add.audio",
            json!({ "name": name }),
            &RunOptions::human(),
        )
        .expect("a track");
    out.data["id"].as_str().expect("an id").to_string()
}

fn volumes(s: &Session, ids: &[String]) -> Vec<f64> {
    ids.iter()
        .map(|t| s.project().tracks[&t.parse::<TrackId>().unwrap()].volume_db)
        .collect()
}

fn pans(s: &Session, ids: &[String]) -> Vec<f64> {
    ids.iter()
        .map(|t| s.project().tracks[&t.parse::<TrackId>().unwrap()].pan)
        .collect()
}

/// Three tracks at -12, -6 and +6 dB, panned L50, centre and R80.
fn song() -> (Session, Vec<String>) {
    let mut s = Session::new("by");
    let ids: Vec<String> = ["A", "B", "C"].iter().map(|n| add(&mut s, n)).collect();
    for (t, db, pan) in [
        (&ids[0], -12.0, -0.5),
        (&ids[1], -6.0, 0.0),
        (&ids[2], 6.0, 0.8),
    ] {
        run(&mut s, "track.volume.set", json!({ "ids": [t], "db": db }));
        run(&mut s, "track.pan.set", json!({ "ids": [t], "pan": pan }));
    }
    (s, ids)
}

#[test]
fn by_moves_every_track_from_where_it_is_and_keeps_the_differences() {
    let (mut s, ids) = song();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": ids, "by": -3.0 }),
    );
    assert_eq!(volumes(&s, &ids), [-15.0, -9.0, 3.0]);
    run(&mut s, "track.pan.set", json!({ "ids": ids, "by": 0.1 }));
    let p = pans(&s, &ids);
    for (got, want) in p.iter().zip([-0.4, 0.1, 0.9]) {
        assert!((got - want).abs() < 1e-9, "{p:?}");
    }
}

#[test]
fn a_track_at_the_end_stops_there_alone() {
    let (mut s, ids) = song();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": ids, "by": 10.0 }),
    );
    assert_eq!(volumes(&s, &ids), [-2.0, 4.0, 12.0]);
    run(&mut s, "track.pan.set", json!({ "ids": ids, "by": 0.5 }));
    let p = pans(&s, &ids);
    for (got, want) in p.iter().zip([0.0, 0.5, 1.0]) {
        assert!((got - want).abs() < 1e-9, "{p:?}");
    }
}

#[test]
fn one_by_is_one_undo() {
    let (mut s, ids) = song();
    let before = volumes(&s, &ids);
    run(&mut s, "track.volume.set", json!({ "ids": ids, "by": 4.0 }));
    assert_ne!(volumes(&s, &ids), before);
    s.undo(1).expect("undo");
    assert_eq!(volumes(&s, &ids), before);
}

#[test]
fn by_takes_the_selected_tracks_when_ids_are_left_out() {
    let (mut s, ids) = song();
    run(&mut s, "select.set", json!({ "ids": [ids[0], ids[2]] }));
    run(&mut s, "track.volume.set", json!({ "by": -1.0 }));
    assert_eq!(volumes(&s, &ids), [-13.0, -6.0, 5.0]);
}

#[test]
fn a_value_and_by_cannot_go_together_and_one_of_them_is_needed() {
    let (mut s, ids) = song();
    let both = json!({ "ids": ids, "db": -3.0, "by": 1.0 });
    assert_eq!(
        fail(&mut s, "track.volume.set", both),
        ErrorCode::InvalidArgs
    );
    let neither = json!({ "ids": ids });
    assert_eq!(
        fail(&mut s, "track.pan.set", neither),
        ErrorCode::InvalidArgs
    );
    // The absolute form is as before.
    run(&mut s, "track.volume.set", json!({ "ids": ids, "db": 0.0 }));
    assert_eq!(volumes(&s, &ids), [0.0, 0.0, 0.0]);
}

#[test]
fn a_track_taken_below_the_fader_floor_is_silent() {
    let (mut s, ids) = song();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": ids, "by": -55.0 }),
    );
    // -67 and -61 dB would show -∞ on the fader and still sound.
    assert_eq!(volumes(&s, &ids), [MIN_DB, MIN_DB, -49.0]);
}

#[test]
fn a_track_at_or_below_the_fader_floor_stays_where_it_is() {
    let (mut s, ids) = song();
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [ids[0]], "db": MIN_DB }),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": [ids[1]], "db": -70.0 }),
    );
    run(
        &mut s,
        "track.volume.set",
        json!({ "ids": ids, "by": 12.0 }),
    );
    assert_eq!(volumes(&s, &ids), [MIN_DB, -70.0, 12.0]);
}

#[test]
fn a_pan_moved_by_leaves_no_rounding_noise() {
    let (mut s, ids) = song();
    run(
        &mut s,
        "track.pan.set",
        json!({ "ids": [ids[1]], "pan": 0.1 }),
    );
    run(&mut s, "track.pan.set", json!({ "ids": ids, "by": 0.2 }));
    assert_eq!(pans(&s, &ids), [-0.3, 0.3, 1.0]);
}
