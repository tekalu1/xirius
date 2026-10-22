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
    assert!(s.lib_up(g, to.0, to.1));
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
    assert!(s.lib_up(&g, x, y));
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
    s.lib_up(&g, lane(&s, &g, keys).0, lane(&s, &g, keys).1);
    assert_eq!(s.song().steps(), steps);
    assert_eq!(instrument_name(&s.project(), keys), None);
    // Nothing is held: the pointer's moves are not taken.
    assert!(!s.lib_move(x, y));
    assert!(!s.lib_up(&g, x, y));
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
}
