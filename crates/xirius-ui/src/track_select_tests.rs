use serde_json::{Map, Value, json};

use crate::arrange::probe::{folder, row_mid, track};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

fn studio() -> (Studio, Geometry) {
    let s = Studio::sample();
    // Tall enough for every row of the sample song to be on screen.
    let g = s.geometry(1440.0, 1200.0);
    (s, g)
}

fn row(s: &Studio, name: &str) -> RowKind {
    RowKind::Track(track(s, name))
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::NONE
    }
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::NONE
    }
}

fn ctrl_shift() -> Mods {
    Mods {
        ctrl: true,
        shift: true,
        alt: false,
    }
}

/// The selection in the column's order, and the focus.
fn picked(s: &Studio) -> (Vec<RowKind>, Option<RowKind>) {
    let rows = s.picked_rows();
    let order: Vec<RowKind> = s.tree().rows.iter().map(|r| r.kind).collect();
    let mut sorted = rows.clone();
    sorted.sort_by_key(|r| order.iter().position(|o| o == r));
    assert_eq!(
        rows.first().copied(),
        s.selected_row,
        "the focus comes first"
    );
    (sorted, s.selected_row)
}

/// A press and a release on a row's name as the window makes them: the arrangement's press,
/// then the studio's, then the release.
fn head_click(s: &mut Studio, g: &Geometry, kind: RowKind, mods: Mods) {
    let at = row_mid(s, g, kind);
    if !s.arrange_down(g, at.0, at.1, mods) {
        let hit = s.hit(g, at.0, at.1);
        s.press(g, hit);
    }
    s.arrange_up(g, at.0, at.1, mods);
}

#[test]
fn ctrl_adds_or_takes_away_one_row_and_the_last_one_stays() {
    let (mut s, _) = studio();
    let (keys, vocal, guitar) = (row(&s, "Keys"), row(&s, "Vocal"), row(&s, "Guitar"));
    s.pick_row(keys, Mods::NONE);
    assert_eq!(picked(&s), (vec![keys], Some(keys)));

    s.pick_row(guitar, ctrl());
    assert_eq!(picked(&s), (vec![keys, guitar], Some(guitar)));
    s.pick_row(vocal, ctrl());
    assert_eq!(picked(&s), (vec![keys, vocal, guitar], Some(vocal)));

    // Taking away a row that is not the focus leaves the focus where it is.
    s.pick_row(keys, ctrl());
    assert_eq!(picked(&s), (vec![vocal, guitar], Some(vocal)));
    // Taking away the focus moves it to the nearest row left.
    s.pick_row(vocal, ctrl());
    assert_eq!(picked(&s), (vec![guitar], Some(guitar)));
    // The last selected row stays.
    s.pick_row(guitar, ctrl());
    assert_eq!(picked(&s), (vec![guitar], Some(guitar)));
}

#[test]
fn shift_chooses_the_range_from_the_anchor_and_ctrl_shift_adds_it() {
    let (mut s, _) = studio();
    let (keys, vocal, guitar) = (row(&s, "Keys"), row(&s, "Vocal"), row(&s, "Guitar"));
    s.pick_row(vocal, Mods::NONE);
    s.pick_row(guitar, shift());
    assert_eq!(picked(&s), (vec![vocal, guitar], Some(guitar)));
    // The range is measured from the same anchor again: it turns around Vocal.
    s.pick_row(keys, shift());
    assert_eq!(picked(&s), (vec![keys, vocal], Some(keys)));

    // A range over a folder takes the folders' rows and the tracks in them.
    let bass = row(&s, "Bass");
    s.pick_row(bass, shift());
    let (rows, focus) = picked(&s);
    assert_eq!(focus, Some(bass));
    assert!(rows.contains(&RowKind::Folder(folder(&s, "Percussion"))));
    for name in ["Bass", "Shaker", "Conga", "Keys", "Vocal"] {
        assert!(rows.contains(&row(&s, name)), "{name}");
    }
    assert!(!rows.contains(&guitar) && !rows.contains(&row(&s, "Drums")));

    // Ctrl+Shift adds the range to what is selected.
    let drums = row(&s, "Drums");
    s.pick_row(drums, Mods::NONE);
    s.pick_row(keys, ctrl());
    s.pick_row(guitar, ctrl_shift());
    assert_eq!(picked(&s), (vec![drums, keys, vocal, guitar], Some(guitar)));
}

#[test]
fn shift_arrows_on_the_heads_widen_and_narrow_the_range() {
    let (mut s, g) = studio();
    let (conga, keys, vocal, guitar) = (
        row(&s, "Conga"),
        row(&s, "Keys"),
        row(&s, "Vocal"),
        row(&s, "Guitar"),
    );
    head_click(&mut s, &g, keys, Mods::NONE);
    assert!(s.heads_focused());
    let step = |s: &mut Studio, dir: &str, extend: bool| {
        let mut args = Map::new();
        args.insert("dir".into(), json!(dir));
        args.insert("extend".into(), Value::Bool(extend));
        assert!(s.run_parsed(&g, "select.step".into(), args));
    };
    step(&mut s, "down", true);
    assert_eq!(picked(&s), (vec![keys, vocal], Some(vocal)));
    step(&mut s, "down", true);
    assert_eq!(picked(&s), (vec![keys, vocal, guitar], Some(guitar)));
    step(&mut s, "up", true);
    assert_eq!(picked(&s), (vec![keys, vocal], Some(vocal)));
    step(&mut s, "up", true);
    assert_eq!(picked(&s), (vec![keys], Some(keys)));
    step(&mut s, "up", true);
    assert_eq!(picked(&s), (vec![conga, keys], Some(conga)));
    // Without Shift the next row is chosen alone.
    step(&mut s, "down", false);
    assert_eq!(picked(&s), (vec![keys], Some(keys)));
}

#[test]
fn ctrl_a_chooses_every_track_only_with_the_heads_in_focus() {
    let (mut s, g) = studio();
    let keys = row(&s, "Keys");
    head_click(&mut s, &g, keys, Mods::NONE);
    assert!(s.heads_focused());
    assert!(s.run_parsed(&g, "select.all".into(), Map::new()));
    let every: Vec<_> = s
        .project()
        .tracks
        .values()
        .map(|t| t.id)
        .filter(|t| !t.is_master())
        .collect();
    let mut tracks = s.picked_tracks();
    tracks.sort();
    let mut every = every;
    every.sort();
    assert_eq!(tracks, every);
    assert_eq!(s.selected_row, Some(keys), "the focus stays");

    // With the lanes in focus Ctrl+A takes the clips, and the tracks stay as they were.
    head_click(&mut s, &g, keys, Mods::NONE);
    s.arrange.heads = false;
    assert!(s.run_parsed(&g, "select.all".into(), Map::new()));
    assert_eq!(picked(&s), (vec![keys], Some(keys)));
    assert!(!s.arrange.clips.is_empty());
}

#[test]
fn a_press_on_a_selected_row_keeps_the_others_until_the_click_ends() {
    let (mut s, g) = studio();
    let (keys, vocal, guitar) = (row(&s, "Keys"), row(&s, "Vocal"), row(&s, "Guitar"));
    head_click(&mut s, &g, keys, Mods::NONE);
    head_click(&mut s, &g, guitar, shift());
    assert_eq!(picked(&s), (vec![keys, vocal, guitar], Some(guitar)));

    // The press keeps the three (a drag would take them all) and moves the focus.
    let at = row_mid(&s, &g, vocal);
    s.arrange_down(&g, at.0, at.1, Mods::NONE);
    let hit = s.hit(&g, at.0, at.1);
    s.press(&g, hit);
    assert_eq!(picked(&s), (vec![keys, vocal, guitar], Some(vocal)));
    // The release without a drag chooses the row alone.
    s.arrange_up(&g, at.0, at.1, Mods::NONE);
    assert_eq!(picked(&s), (vec![vocal], Some(vocal)));

    // Ctrl presses add and take away through the window's own path as well.
    head_click(&mut s, &g, guitar, ctrl());
    assert_eq!(picked(&s), (vec![vocal, guitar], Some(guitar)));
    head_click(&mut s, &g, vocal, ctrl());
    assert_eq!(picked(&s), (vec![guitar], Some(guitar)));
}

#[test]
fn choosing_a_row_outside_the_selection_by_other_means_chooses_it_alone() {
    let (mut s, _) = studio();
    let (keys, vocal, guitar) = (row(&s, "Keys"), row(&s, "Vocal"), row(&s, "Guitar"));
    s.pick_row(keys, Mods::NONE);
    s.pick_row(vocal, ctrl());
    // Inside the selection only the focus moves.
    s.select_row(Some(keys));
    assert_eq!(picked(&s), (vec![keys, vocal], Some(keys)));
    // Outside it the row is chosen alone.
    s.select_row(Some(guitar));
    assert_eq!(picked(&s), (vec![guitar], Some(guitar)));
}

#[test]
fn a_command_on_the_selected_tracks_takes_the_tracks_in_a_closed_folder() {
    let (mut s, g) = studio();
    let percussion = folder(&s, "Percussion");
    assert!(s.press(&g, crate::studio::Hit::Chevron(percussion)));
    let keys = row(&s, "Keys");
    head_click(&mut s, &g, keys, Mods::NONE);
    assert!(s.run_parsed(&g, "select.all".into(), Map::new()));
    assert!(s.picked_rows().contains(&RowKind::Folder(percussion)));
    let inside = s.project().tracks_in(percussion);
    assert!(!inside.is_empty());
    // M mutes every track the selection takes, the ones in the folder too.
    assert!(s.run_parsed(&g, "track.mute".into(), Map::new()));
    let p = s.project();
    let muted: Vec<_> = p.tracks.values().filter(|t| t.mute).map(|t| t.id).collect();
    for t in s.picked_tracks() {
        assert!(muted.contains(&t), "{t} is not muted: {muted:?}");
    }
    for t in inside {
        assert!(muted.contains(&t), "{t} in the folder is not muted");
    }
}
