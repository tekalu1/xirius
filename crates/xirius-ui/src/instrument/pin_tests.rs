//! UI-323: pinned windows stay; an unpinned one takes the next device in.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};

use super::window::{CASCADE, FLASH, Part};
use crate::studio::{Geometry, Studio};

/// The sample song with an EQ, a Compressor and a Delay on Keys.
fn three() -> (Studio, Geometry, TrackId, [PluginId; 3]) {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 1000.0);
    let t = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    let ids = ["EQ", "Compressor", "Delay"].map(|name| {
        assert!(s.run_command(
            "plugin.add",
            json!({ "track": t.to_string(), "name": name }),
        ));
        s.project().tracks[&t]
            .inserts
            .iter()
            .find(|x| x.name == name)
            .unwrap()
            .id
    });
    (s, g, t, ids)
}

fn shown(s: &Studio) -> Vec<PluginId> {
    s.instruments.open.iter().map(|w| w.plugin).collect()
}

fn press(s: &mut Studio, g: &Geometry, id: PluginId, part: Part) {
    let (x, y) = s.instrument_center(g, id, part).expect("in sight");
    assert!(s.instrument_down(g, x, y));
}

#[test]
fn ui_323_an_unpinned_window_takes_the_next_device_in_where_it_was() {
    let (mut s, g, t, [eq, comp, _]) = three();
    s.show_instrument(t, eq);
    let before = s.instruments.rect(0, g.size);
    s.show_instrument(t, comp);
    assert_eq!(shown(&s), [comp]);
    let after = s.instruments.rect(0, g.size);
    assert_eq!((after.x, after.y), (before.x, before.y));
    // The EQ's own state went with it.
    assert!(!s.instruments.chosen.contains_key(&eq));
}

#[test]
fn ui_323_a_pinned_window_stays_and_the_next_opens_a_step_on() {
    let (mut s, g, t, [eq, comp, delay]) = three();
    s.show_instrument(t, eq);
    press(&mut s, &g, eq, Part::Pin);
    assert!(s.instruments.open[0].pinned);
    s.show_instrument(t, comp);
    assert_eq!(shown(&s), [eq, comp]);
    let (a, b) = (s.instruments.rect(0, g.size), s.instruments.rect(1, g.size));
    assert_eq!((b.x - a.x, b.y - a.y), (CASCADE, CASCADE));
    // The Compressor's window is not pinned: the Delay takes its place.
    s.show_instrument(t, delay);
    assert_eq!(shown(&s), [eq, delay]);
    assert_eq!(s.instruments.rect(1, g.size).y, b.y);
}

#[test]
fn ui_323_an_open_device_comes_to_the_top_and_lights() {
    let (mut s, g, t, [eq, comp, _]) = three();
    s.show_instrument(t, eq);
    press(&mut s, &g, eq, Part::Pin);
    s.show_instrument(t, comp);
    assert_eq!(s.instruments.flash.map(|f| f.0), Some(comp));
    s.show_instrument(t, eq);
    assert_eq!(shown(&s), [comp, eq], "on top, none opened");
    let (id, since) = s.instruments.flash.unwrap();
    assert_eq!(id, eq);
    assert_eq!(s.instruments.lit(eq, since), 1.0);
    assert!(s.instruments.lit(eq, since + FLASH / 2) > 0.0);
    assert_eq!(s.instruments.lit(eq, since + FLASH), 0.0);
    assert_eq!(s.instruments.lit(comp, since), 0.0);
}

#[test]
fn ui_323_of_two_unpinned_windows_the_one_used_last_is_filled() {
    let (mut s, g, t, [eq, comp, delay]) = three();
    s.show_instrument(t, eq);
    press(&mut s, &g, eq, Part::Pin);
    s.show_instrument(t, comp);
    // Both unpinned, the EQ used last.
    press(&mut s, &g, eq, Part::Pin);
    assert!(s.instruments.open.iter().all(|w| !w.pinned));
    assert_eq!(shown(&s), [comp, eq]);
    s.show_instrument(t, delay);
    assert_eq!(shown(&s), [comp, delay]);
    // Then the Compressor, once a press in it makes it the last used.
    press(&mut s, &g, comp, Part::Body);
    s.show_instrument(t, eq);
    assert_eq!(shown(&s), [delay, eq]);
}

#[test]
fn ui_323_a_new_window_takes_the_first_free_place_in_the_cascade() {
    let (mut s, g, t, [eq, comp, delay]) = three();
    for id in [eq, comp] {
        s.show_instrument(t, id);
        press(&mut s, &g, id, Part::Pin);
    }
    let seats: Vec<usize> = s.instruments.open.iter().map(|w| w.seat).collect();
    assert_eq!(seats, [0, 1]);
    press(&mut s, &g, eq, Part::Close);
    s.show_instrument(t, delay);
    let at = |s: &Studio, id| {
        s.instruments
            .open
            .iter()
            .find(|w| w.plugin == id)
            .unwrap()
            .seat
    };
    assert_eq!(at(&s, delay), 0);
    assert_eq!(at(&s, comp), 1);
}
