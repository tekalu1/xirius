//! The instruments of the browser: the double click and the drag onto the tracks. Called directly: no OS input.

use std::sync::Arc;

use xirius_core::model::{Project, Track};
use xirius_plugins::Catalog;

use super::*;
use crate::browser::RailTab;
use crate::instrument::current;

/// The sample song with the browser on its instruments and no scan.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Instruments);
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap()
}

fn tracks(s: &Studio) -> usize {
    s.project().tracks.len()
}

/// The middle of a browser item.
fn item(s: &Studio, g: &Geometry, name: &str) -> (Hit, f32, f32) {
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::Item { item, .. } if item.name == name))
        .unwrap_or_else(|| panic!("{name} is in the list"));
    let (x, y) = s
        .browser
        .center_of(&l, BrowserHit::Item(i))
        .expect("on screen");
    (Hit::Browser(BrowserHit::Item(i)), x, y)
}

/// The middle of a track's row in the lanes.
fn lane(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let row = s.tree().row_of(RowKind::Track(t)).expect("a row");
    let y = g.column.y + row.y - s.scroll.1 + row.h / 2.0;
    (g.lanes.x + g.lanes.w / 2.0, y)
}

/// A press on an item, a move to `to` and the release there.
fn drag(s: &mut Studio, g: &Geometry, name: &str, to: (f32, f32)) {
    let (hit, x, y) = item(s, g, name);
    s.press(g, hit);
    assert!(s.lib_grab(g, hit, x, y, 1));
    assert!(s.lib_move(to.0, to.1));
    assert!(s.lib_up(g, to.0, to.1, false));
}

fn instrument_name(p: &Project, t: TrackId) -> Option<String> {
    current(&p.tracks[&t]).map(|x| x.name.clone())
}

/// The track that is not in `before`.
fn added<'a>(p: &'a Project, before: &[TrackId]) -> &'a Track {
    p.tracks
        .values()
        .find(|t| !before.contains(&t.id))
        .expect("a new track")
}

#[test]
fn a_double_click_makes_a_new_track() {
    let (mut s, g) = studio();
    let before: Vec<TrackId> = s.project().tracks.keys().copied().collect();
    let (hit, x, y) = item(&s, &g, "Sampler");
    s.press(&g, hit);
    assert!(s.lib_grab(&g, hit, x, y, 1));
    assert!(s.lib_up(&g, x, y, false));
    assert_eq!(tracks(&s), before.len(), "a single click only selects");
    assert_eq!(s.browser.selected_item, Some("Sampler"));
    s.press(&g, hit);
    assert!(s.lib_grab(&g, hit, x, y, 2));
    assert!(s.browser.drag.is_none());
    let p = s.project();
    assert_eq!(
        instrument_name(&p, added(&p, &before).id).as_deref(),
        Some("Sampler")
    );
}

#[test]
fn dropped_on_an_instrument_track_it_takes_that_tracks_place() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let before = tracks(&s);
    let steps = s.song().steps();
    let at = lane(&s, &g, keys);
    assert_eq!(s.lib_drop_at(&g, at.0, at.1), Drop::Replace(keys));
    drag(&mut s, &g, "Drum Kit", at);
    assert_eq!(
        instrument_name(&s.project(), keys).as_deref(),
        Some("Drum Kit")
    );
    assert_eq!(tracks(&s), before);
    assert_eq!(s.song().steps(), steps + 1);
    assert_eq!(s.selected_row, Some(RowKind::Track(keys)));
    // The track column counts too.
    let (_, y) = lane(&s, &g, keys);
    drag(&mut s, &g, "Sampler", (g.column.x + 20.0, y));
    assert_eq!(
        instrument_name(&s.project(), keys).as_deref(),
        Some("Sampler")
    );
    assert!(s.browser.drag.is_none());
}

#[test]
fn dropped_anywhere_else_on_the_tracks_it_starts_a_new_one() {
    let (mut s, g) = studio();
    let audio = s
        .project()
        .tracks
        .values()
        .find(|t| t.kind == TrackKind::Audio)
        .map(|t| t.id)
        .unwrap();
    let before: Vec<TrackId> = s.project().tracks.keys().copied().collect();
    let at = lane(&s, &g, audio);
    assert_eq!(s.lib_drop_at(&g, at.0, at.1), Drop::New);
    drag(&mut s, &g, "Sampler", at);
    let p = s.project();
    let new = added(&p, &before);
    assert_eq!(instrument_name(&p, new.id).as_deref(), Some("Sampler"));
    assert_eq!(
        instrument_name(&p, audio),
        None,
        "the audio track is as it was"
    );
}

#[test]
fn let_go_off_the_tracks_or_unmoved_nothing_changes() {
    let (mut s, g) = studio();
    let steps = s.song().steps();
    let (_, x, y) = item(&s, &g, "Drum Kit");
    assert_eq!(s.lib_drop_at(&g, x, y), Drop::Nothing);
    drag(&mut s, &g, "Drum Kit", (x, y + 60.0));
    // A small wobble is not a drag.
    let keys = track(&s, "Keys");
    let (hit, x, y) = item(&s, &g, "Sampler");
    s.press(&g, hit);
    s.lib_grab(&g, hit, x, y, 1);
    s.lib_move(x + 2.0, y + 2.0);
    assert!(!s.browser.drag.as_ref().unwrap().moved);
    s.lib_up(&g, lane(&s, &g, keys).0, lane(&s, &g, keys).1, false);
    assert_eq!(s.song().steps(), steps);
    assert_eq!(instrument_name(&s.project(), keys), None);
    // Nothing is held: the pointer's moves are not taken.
    assert!(!s.lib_move(x, y));
    assert!(!s.lib_up(&g, x, y, false));
}

#[test]
fn an_empty_song_takes_a_dropped_instrument_as_its_first_track() {
    let mut s = Studio::new(crate::song::Song::empty());
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    let g = s.geometry(1600.0, 1000.0);
    let z = crate::start::zone(&g);
    let middle = (z.x + z.w / 2.0, z.y + z.h / 2.0);
    assert_eq!(s.lib_drop_at(&g, middle.0, middle.1), Drop::New);
    drag(&mut s, &g, "Sampler", middle);
    assert!(!s.start_shown());
    assert_eq!(s.project().root.len(), 1);
    // Below the last row: one more.
    let row = s.tree().rows.last().unwrap().bottom() + g.column.y;
    assert!(middle.1 > row);
    assert_eq!(s.lib_drop_at(&g, middle.0, middle.1), Drop::New);
    drag(&mut s, &g, "Drum Kit", middle);
    assert_eq!(s.project().root.len(), 2);
    let new = *s.project().tracks.keys().max().unwrap();
    assert_eq!(s.selected_row, Some(RowKind::Track(new)));
    assert!(s.mixer.open_later.is_none());
    assert!(s.mixer.editors.is_empty());
    assert!(
        s.instruments
            .is_open(current(&s.project().tracks[&new]).unwrap().id)
    );
}

#[test]
fn dropping_on_the_first_middle_and_last_seams_inserts_in_one_step() {
    use xirius_core::id::Node;
    for edge in [0, 1, 2, 3] {
        let mut s = Studio::new(crate::song::Song::empty());
        s.mixer.catalog = Some(Arc::new(Catalog::default()));
        for name in ["One", "Two", "Three"] {
            assert!(s.run_command("track.add.audio", json!({"name":name})));
        }
        let g = s.geometry(1600.0, 1000.0);
        let before = s.project().root.clone();
        let tree = s.tree();
        let y = tree.rows.get(edge).map_or(tree.height, |row| row.y);
        let point = (g.column.x + 20.0, g.column.y + y - s.scroll.1);
        let steps = s.song().steps();
        drag(&mut s, &g, "Sampler", point);
        let Some(RowKind::Track(new)) = s.selected_row else {
            panic!("select the new track")
        };
        let mut expected = before.clone();
        expected.insert(edge, Node::Track(new));
        assert_eq!(s.project().root, expected);
        assert!(
            s.instruments
                .is_open(current(&s.project().tracks[&new]).unwrap().id)
        );
        assert_eq!(s.song().steps(), steps + 1);
        s.song.run("history.undo", json!({})).unwrap();
        assert_eq!(s.project().root, before);
    }
}

#[test]
fn insertion_takes_eight_logical_pixels_on_each_side_and_leaves_replace_in_the_middle() {
    let mut s = Studio::new(crate::song::Song::empty());
    for name in ["One", "Two"] {
        assert!(s.run_command(
            "track.add.with-instrument",
            json!({"name":name, "instrument":"Sampler"})
        ));
    }
    let g = s.geometry(1600.0, 1000.0);
    let tree = s.tree();
    let [first, second] = tree.rows.as_slice() else {
        panic!("two track rows");
    };
    let RowKind::Track(first_id) = first.kind else {
        panic!("instrument row");
    };
    let RowKind::Track(second_id) = second.kind else {
        panic!("instrument row");
    };
    let x = g.column.x + 30.0;
    for (edge, delta, track) in [(first.bottom(), -8.0, first_id), (second.y, 8.0, second_id)] {
        let y = g.column.y + edge - s.scroll.1;
        let Some(insertion) = s.browser_track_insertion(&g, x, y + delta) else {
            panic!("eight pixels from the boundary");
        };
        assert_eq!((insertion.parent, insertion.index), (Parent::Root, 1));
        let (parent, index, _) = s.drop_at(&g, y + delta, &[]).unwrap();
        assert_eq!((parent, index), (Parent::Root, 1));
        assert_eq!(
            s.instrument_drop_at(&g, x, y + delta * 1.125),
            Drop::Replace(track)
        );
    }
}

#[test]
fn a_drop_between_folder_children_uses_the_reorder_boundary() {
    use xirius_core::id::Node;
    let (mut s, g) = studio();
    let folder: xirius_core::id::FolderId =
        s.song.run("folder.add", json!({"name":"Group"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
    let parent = Parent::Folder(folder);
    for name in ["One", "Two"] {
        s.song
            .run("track.add.audio", json!({"name":name,"into":folder}))
            .unwrap();
    }
    s.fold_all(&g, false);
    let child = track(&s, "Two");
    let row = *s.tree().row_of(RowKind::Track(child)).unwrap();
    s.scroll.1 = (row.y - g.column.h / 2.0).max(0.0);
    let at = (g.column.x + 30.0, g.column.y + row.y - s.scroll.1 + 1.0);
    let Drop::Insert(insertion) = s.instrument_drop_at(&g, at.0, at.1) else {
        panic!("a folder seam is highlighted");
    };
    let (expected_parent, index, mark) = s.drop_at(&g, at.1, &[]).unwrap();
    assert_eq!(expected_parent, parent);
    assert_eq!(
        insertion,
        Insertion {
            parent,
            index,
            mark
        }
    );
    drag(&mut s, &g, "Sampler", at);
    let Some(RowKind::Track(new)) = s.selected_row else {
        panic!("select the new track")
    };
    assert_eq!(
        s.project().find_node(Node::Track(new)),
        Some((parent, index))
    );
}
