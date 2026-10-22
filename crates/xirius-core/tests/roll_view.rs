//! EDT-006: how the roll shows a part (`roll.view`): the keyboard, or the rows of a drum map,
//! remembered for the part and the map for its track.

use serde_json::{Value, json};
use xirius_core::Session;
use xirius_core::session::RunOptions;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"].as_str().expect("an id").to_string()
}

fn song() -> (Session, String, String) {
    let mut s = Session::new("t");
    let track = id(&run(&mut s, "track.add.instrument", json!({})));
    let clip = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": track, "at": 0, "length": 3840}),
    ));
    (s, track, clip)
}

fn shown(s: &Session, clip: &str) -> Option<String> {
    xirius_core::drum::shown(
        s.project(),
        &s.state.view.roll,
        clip.parse().expect("a clip id"),
    )
}

#[test]
fn a_part_opens_as_a_keyboard_until_its_track_is_a_drum_track() {
    let (mut s, track, clip) = song();
    assert_eq!(shown(&s, &clip), None);
    run(
        &mut s,
        "track.icon.set",
        json!({"ids": [track], "icon": "drums"}),
    );
    assert_eq!(shown(&s, &clip).as_deref(), Some("gm"));
}

#[test]
fn the_choice_is_remembered_for_the_part_and_the_map_for_the_track_outside_the_history() {
    let (mut s, track, clip) = song();
    let steps = s.steps().len();
    let r = run(&mut s, "roll.view", json!({"clip": clip, "mode": "drums"}));
    assert_eq!(r["mode"], "drums");
    assert_eq!(shown(&s, &clip).as_deref(), Some("gm"));
    run(&mut s, "roll.view", json!({"clip": clip, "map": "kit"}));
    assert_eq!(shown(&s, &clip).as_deref(), Some("kit"));
    // Another part of the same track uses the same map.
    let other = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": track, "at": 3840, "length": 3840}),
    ));
    run(&mut s, "roll.view", json!({"clip": other, "mode": "drums"}));
    assert_eq!(shown(&s, &other).as_deref(), Some("kit"));
    // A drum track can still be seen as a keyboard.
    run(
        &mut s,
        "track.icon.set",
        json!({"ids": [track], "icon": "drums"}),
    );
    run(&mut s, "roll.view", json!({"clip": clip, "mode": "keys"}));
    assert_eq!(shown(&s, &clip), None);
    assert_eq!(
        s.steps().len(),
        steps + 2,
        "only the part and the icon are steps"
    );
}

#[test]
fn a_view_with_nothing_to_set_or_an_unknown_map_is_refused() {
    let (mut s, _, clip) = song();
    for args in [
        json!({"clip": clip}),
        json!({"clip": clip, "map": "nothing"}),
        json!({"clip": clip, "mode": "score"}),
    ] {
        assert!(
            s.run_value("roll.view", args.clone(), &RunOptions::human())
                .is_err(),
            "{args}"
        );
    }
}

#[test]
fn the_view_survives_save_and_reopen_and_an_old_view_still_opens() {
    let (mut s, _, clip) = song();
    run(&mut s, "roll.view", json!({"clip": clip, "mode": "drums"}));
    run(&mut s, "roll.view", json!({"clip": clip, "map": "kit"}));
    let dir = std::env::temp_dir().join(format!("xirius-roll-view-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save_as(&dir).unwrap();
    drop(s);
    let reopened = Session::open(&dir).unwrap();
    assert_eq!(shown(&reopened, &clip).as_deref(), Some("kit"));
    drop(reopened);
    let path = dir.join("view.json");
    let mut view: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let roll = view["roll"].as_object_mut().unwrap();
    roll.remove("drum_clips").unwrap();
    roll.remove("drum_maps").unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(&view).unwrap()).unwrap();
    let old = Session::open(&dir).unwrap();
    assert_eq!(shown(&old, &clip), None);
    let _ = std::fs::remove_dir_all(&dir);
}

fn rows(v: &Value) -> Vec<(u64, String)> {
    v["rows"]
        .as_array()
        .expect("the rows")
        .iter()
        .map(|r| {
            (
                r["pitch"].as_u64().unwrap(),
                r["name"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pitches(v: &Value) -> Vec<u64> {
    rows(v).into_iter().map(|(p, _)| p).collect()
}

#[test]
fn rows_are_hidden_moved_and_renamed_for_the_track_outside_the_history() {
    let (mut s, track, clip) = song();
    run(&mut s, "roll.view", json!({"clip": clip, "mode": "drums"}));
    let steps = s.steps().len();
    // A hit on the kick keeps its row in sight even when it is hidden.
    run(
        &mut s,
        "note.add",
        json!({"clip": clip, "pitch": 36, "at": 0}),
    );
    let steps = steps + 1;
    let r = run(&mut s, "roll.drum.hide", json!({"clip": clip, "pitch": 42}));
    assert!(!pitches(&r).contains(&42));
    let r = run(&mut s, "roll.drum.hide", json!({"clip": clip, "pitch": 36}));
    assert!(pitches(&r).contains(&36), "the kick has a note");
    assert_eq!(r["hidden"], json!([36, 42]));
    let r = run(
        &mut s,
        "roll.drum.hide",
        json!({"clip": clip, "pitch": 42, "show": true}),
    );
    assert!(pitches(&r).contains(&42));

    // The kick to the top: past the first row it stops there.
    let r = run(
        &mut s,
        "roll.drum.move",
        json!({"clip": clip, "pitch": 36, "by": -100}),
    );
    assert_eq!(pitches(&r)[0], 36);
    let r = run(
        &mut s,
        "roll.drum.move",
        json!({"clip": clip, "pitch": 36, "by": 1}),
    );
    assert_eq!(pitches(&r)[1], 36);

    let r = run(
        &mut s,
        "roll.drum.rename",
        json!({"clip": clip, "pitch": 38, "name": "  スネア（リム）  "}),
    );
    assert!(rows(&r).contains(&(38, "スネア（リム）".to_string())));
    // Another part of the track shows the same rows.
    let other = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": track, "at": 3840, "length": 3840}),
    ));
    let steps = steps + 1;
    let e = xirius_core::drum::edits(&s.state.view.roll, &track).into_owned();
    assert_eq!(e.name(38), "スネア（リム）");
    let r = run(
        &mut s,
        "roll.drum.rename",
        json!({"clip": other, "pitch": 38, "name": ""}),
    );
    assert!(rows(&r).contains(&(38, "スネア".to_string())));
    assert_eq!(s.steps().len(), steps, "no row change is a step");

    let r = run(&mut s, "roll.drum.reset", json!({"clip": clip}));
    assert_eq!(
        pitches(&r),
        xirius_core::drum::all_rows("gm", [36])
            .into_iter()
            .map(u64::from)
            .collect::<Vec<_>>()
    );
    assert!(s.state.view.roll.drum_rows.is_empty());
}

#[test]
fn a_row_change_with_a_pitch_off_the_keyboard_or_a_row_not_shown_is_refused() {
    let (mut s, _, clip) = song();
    run(&mut s, "roll.drum.hide", json!({"clip": clip, "pitch": 42}));
    for (name, args) in [
        ("roll.drum.hide", json!({"clip": clip, "pitch": 128})),
        (
            "roll.drum.move",
            json!({"clip": clip, "pitch": 42, "by": 1}),
        ),
        (
            "roll.drum.rename",
            json!({"clip": clip, "pitch": -1, "name": "x"}),
        ),
    ] {
        assert!(
            s.run_value(name, args.clone(), &RunOptions::human())
                .is_err(),
            "{name} {args}"
        );
    }
}

#[test]
fn the_rows_survive_save_and_reopen_and_a_view_without_them_still_opens() {
    let (mut s, track, clip) = song();
    run(&mut s, "roll.drum.hide", json!({"clip": clip, "pitch": 42}));
    run(
        &mut s,
        "roll.drum.move",
        json!({"clip": clip, "pitch": 36, "by": -3}),
    );
    run(
        &mut s,
        "roll.drum.rename",
        json!({"clip": clip, "pitch": 38, "name": "Snare A"}),
    );
    let before = xirius_core::drum::edits(&s.state.view.roll, &track).into_owned();
    assert!(!before.is_empty());
    let dir = std::env::temp_dir().join(format!("xirius-drum-rows-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save_as(&dir).unwrap();
    drop(s);
    let reopened = Session::open(&dir).unwrap();
    assert_eq!(
        xirius_core::drum::edits(&reopened.state.view.roll, &track).into_owned(),
        before
    );
    drop(reopened);
    let path = dir.join("view.json");
    let mut view: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    view["roll"]
        .as_object_mut()
        .unwrap()
        .remove("drum_rows")
        .expect("the rows were saved");
    std::fs::write(&path, serde_json::to_vec_pretty(&view).unwrap()).unwrap();
    let old = Session::open(&dir).unwrap();
    assert!(old.state.view.roll.drum_rows.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
