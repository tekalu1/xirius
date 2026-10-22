//! A track made now takes the next identification colour (UI-077).

use std::collections::HashSet;

use serde_json::{Value, json};
use xirius_core::{Session, session::RunOptions};

fn run(s: &mut Session, n: &str, a: Value) {
    s.run_value(n, a, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{n}: {e:?}"));
}

#[test]
fn new_tracks_take_different_colours_in_turn() {
    let mut s = Session::new("colours");
    for name in ["A", "B", "C", "D", "E"] {
        run(&mut s, "track.add.audio", json!({ "name": name }));
    }
    run(&mut s, "track.add.instrument", json!({ "name": "Keys" }));
    run(&mut s, "track.add.bus", json!({ "name": "Drums" }));
    let p = s.project();
    let colours: Vec<String> = ["A", "B", "C", "D", "E", "Keys", "Drums"]
        .iter()
        .map(|n| {
            let t = p.tracks.values().find(|t| t.name == *n).unwrap();
            t.color
                .clone()
                .unwrap_or_else(|| panic!("{n} has no colour"))
        })
        .collect();
    // The list's colours, in the list's order.
    let list = |i: usize| {
        let c = xirius_design::TrackColor::new(i).content(xirius_design::Theme::Dark);
        format!("#{:06x}", c.to_rgb_u32())
    };
    assert_eq!(
        colours[..5],
        (0..5).map(list).collect::<Vec<_>>(),
        "{colours:?}"
    );
    let distinct: HashSet<_> = colours.iter().collect();
    assert_eq!(distinct.len(), colours.len(), "{colours:?}");
    assert!(
        colours.iter().all(|c| c.len() == 7 && c.starts_with('#')),
        "{colours:?}"
    );
}

#[test]
fn the_colour_survives_undo_and_redo_of_other_tracks() {
    let mut s = Session::new("colours");
    run(&mut s, "track.add.audio", json!({ "name": "A" }));
    let first = |s: &Session| {
        let p = s.project();
        p.tracks
            .values()
            .find(|t| t.name == "A")
            .unwrap()
            .color
            .clone()
    };
    let before = first(&s);
    run(&mut s, "track.add.audio", json!({ "name": "B" }));
    run(&mut s, "history.undo", json!({}));
    run(&mut s, "history.redo", json!({}));
    assert_eq!(first(&s), before);
    assert!(before.is_some());
}
