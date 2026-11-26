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

/// Where an instrument row is let go decides what it does: an instrument track's lane or
/// column takes it in that track's place, an audio track's lane starts a new track, and
/// off the tracks nothing changes.
#[test]
fn an_instrument_dropped_does_what_its_spot_says() {
    enum At {
        KeysLane,
        KeysColumn,
        AudioLane,
        UnderTheItem,
    }
    enum Gets {
        KeysTrack,
        NewTrack,
        Nothing,
    }
    let rows = [
        (
            "on an instrument track's lane: in its place",
            "Drum Kit",
            At::KeysLane,
            Gets::KeysTrack,
        ),
        (
            "on an instrument track's column: in its place",
            "Sampler",
            At::KeysColumn,
            Gets::KeysTrack,
        ),
        (
            "on an audio track's lane: a new track",
            "Sampler",
            At::AudioLane,
            Gets::NewTrack,
        ),
        (
            "off the tracks: nothing",
            "Drum Kit",
            At::UnderTheItem,
            Gets::Nothing,
        ),
    ];
    let mut failures = Vec::new();
    for (row, name, at, gets) in rows {
        let (mut s, g) = studio();
        let keys = track(&s, "Keys");
        let audio = s
            .project()
            .tracks
            .values()
            .find(|t| t.kind == TrackKind::Audio)
            .map(|t| t.id)
            .unwrap();
        let before: Vec<TrackId> = s.project().tracks.keys().copied().collect();
        let steps = s.song().steps();
        let to = match at {
            At::KeysLane => lane(&s, &g, keys),
            At::KeysColumn => (g.column.x + 20.0, lane(&s, &g, keys).1),
            At::AudioLane => lane(&s, &g, audio),
            At::UnderTheItem => {
                let (_, x, y) = item(&s, &g, name);
                (x, y + 60.0)
            }
        };
        let aim = s.lib_drop_at(&g, to.0, to.1);
        drag(&mut s, &g, name, to);
        let p = s.project();
        let mut wrong = Vec::new();
        let (want_aim, more, step) = match gets {
            Gets::KeysTrack => (Drop::Replace(keys), 0, 1),
            Gets::NewTrack => (Drop::New, 1, 1),
            Gets::Nothing => (Drop::Nothing, 0, 0),
        };
        if aim != want_aim {
            wrong.push(format!("aimed {aim:?}, not {want_aim:?}"));
        }
        if p.tracks.len() != before.len() + more {
            wrong.push(format!(
                "{} tracks, not {}",
                p.tracks.len(),
                before.len() + more
            ));
        }
        if s.song().steps() != steps + step {
            wrong.push(format!("{} steps, not {}", s.song().steps(), steps + step));
        }
        let holder = match gets {
            Gets::KeysTrack => Some(keys),
            Gets::NewTrack => (p.tracks.len() > before.len()).then(|| added(&p, &before).id),
            Gets::Nothing => None,
        };
        if let Some(t) = holder.filter(|&t| instrument_name(&p, t).as_deref() != Some(name)) {
            wrong.push(format!("the track plays {:?}", instrument_name(&p, t)));
        }
        if matches!(gets, Gets::KeysTrack) && s.selected_row != Some(RowKind::Track(keys)) {
            wrong.push(format!("selected {:?}", s.selected_row));
        }
        if !matches!(gets, Gets::KeysTrack) && instrument_name(&p, keys).is_some() {
            wrong.push("Keys took it".into());
        }
        if instrument_name(&p, audio).is_some() {
            wrong.push("the audio track took it".into());
        }
        if s.browser.drag.is_some() {
            wrong.push("the drag stays".into());
        }
        if !wrong.is_empty() {
            failures.push(format!("{row}: {wrong:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn a_small_wobble_or_nothing_held_changes_nothing() {
    let (mut s, g) = studio();
    let steps = s.song().steps();
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
    assert!(s.mixer.open_later.is_empty());
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
fn insertion_is_at_the_boundary_and_replace_is_in_the_middle_of_the_row() {
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
    for (edge, inward, row, track) in [
        (first.bottom(), -1.0, first, first_id),
        (second.y, 1.0, second, second_id),
    ] {
        let y = g.column.y + edge - s.scroll.1 + inward;
        let Some(insertion) = s.browser_track_insertion(&g, x, y) else {
            panic!("just inside the boundary of {track:?}");
        };
        assert_eq!((insertion.parent, insertion.index), (Parent::Root, 1));
        let (parent, index, _) = s.drop_at(&g, y, &[]).unwrap();
        assert_eq!((parent, index), (Parent::Root, 1));
        let middle = g.column.y + (row.y + row.bottom()) / 2.0 - s.scroll.1;
        assert_eq!(s.instrument_drop_at(&g, x, middle), Drop::Replace(track));
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
    let (hit, x, y) = item(&s, &g, "Sampler");
    s.press(&g, hit);
    assert!(s.lib_grab(&g, hit, x, y, 1));
    assert!(s.lib_move(at.0, at.1));
    // The line comes in through the seam an audio file uses (UI-381): at once, as a row's.
    let now = std::time::Instant::now();
    assert!(s.lib_seam_moved(&g, now));
    let live = s
        .arrange
        .clip_drop
        .live
        .live()
        .expect("the line is on show");
    assert_eq!(live.spot, insertion.key());
    let grow = |s: &Studio, ms: u64| {
        s.arrange
            .clip_drop
            .live
            .look(now + std::time::Duration::from_millis(ms), false)
            .unwrap()
            .grow
    };
    assert!(grow(&s, 0) < 0.01 && grow(&s, 200) > 0.99);
    assert!(s.lib_up(&g, at.0, at.1, false));
    assert!(s.arrange.clip_drop.live.live().is_none(), "the line goes");
    let Some(RowKind::Track(new)) = s.selected_row else {
        panic!("select the new track")
    };
    assert_eq!(
        s.project().find_node(Node::Track(new)),
        Some((parent, index))
    );
}
