//! OPS-006 / CTX-001 / CTX-002: a row of a menu and the command typed with the same words do the
//! same thing (the positions are `--at`, the targets `--ids`, the ranges `--range`). The window
//! is pressed where it is pressed; no OS input, no sound device, and the OS's file window never
//! opens.

use std::sync::Arc;

use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;
use xirius_plugins::Catalog;

use super::browser::BrowseAct;
use super::tests::{BAR, studio};
use super::{Act, Item};
use crate::arrange::probe::{folder, track, two_lanes};
use crate::browser::BRow;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

fn row(s: &mut Studio, g: &Geometry, act: Act) -> bool {
    s.context_run(g, Item::act("row", "", act))
}

fn browsing() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.browser = crate::browser::test_library::example();
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

// ------------------------------------------------------------------ positions: --at

#[test]
fn seek_and_play_from_take_a_position_as_ticks_or_as_bar_beat_tick() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let (mut c, _) = studio();
    assert!(row(&mut a, &g, Act::Seek(BAR)));
    assert!(b.run_binding(&g, &format!("transport.seek --at {BAR}")));
    assert!(c.run_binding(&g, "transport.seek --at 2.1.0"));
    assert_eq!((a.playhead, b.playhead, c.playhead), (BAR, BAR, BAR));
    // The older `--to` still reads.
    assert!(b.run_binding(&g, "transport.seek --to 0"));
    assert_eq!(b.playhead, 0);
    // Play from: the playhead goes there and the song plays.
    let (mut a, _) = studio();
    let (mut b, _) = studio();
    assert!(row(&mut a, &g, Act::PlayFrom(2 * BAR)));
    assert!(b.run_binding(&g, "transport.play --at 3.1.0"));
    assert_eq!((a.playhead, b.playhead), (2 * BAR, 2 * BAR));
    assert_eq!(a.transport.playing, b.transport.playing);
    // A position that is no position says so and leaves the playhead.
    assert!(!b.run_binding(&g, "transport.play --at nowhere"));
    assert!(b.error.is_some());
    assert_eq!(b.playhead, 2 * BAR);
}

// ------------------------------------------------------------------ targets: --ids

fn folder_state(s: &Studio) -> (Vec<RowKind>, Option<RowKind>, Vec<bool>) {
    let p = s.project();
    (
        s.arrange.tracks.clone(),
        s.selected_row,
        p.folders.keys().map(|f| s.is_collapsed(*f)).collect(),
    )
}

#[test]
fn the_folder_rows_and_the_commands_with_ids_do_the_same() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let f = folder(&a, "Rhythm");
    assert!(row(&mut a, &g, Act::SelectIn(f)));
    assert!(b.run_binding(&g, &format!("select.in_folder --ids {f}")));
    assert_eq!(folder_state(&a), folder_state(&b));
    assert!(!a.arrange.tracks.is_empty());
    // Close all, open all.
    assert!(row(&mut a, &g, Act::Folders(false)));
    assert!(b.run_binding(&g, "view.folders --state closed"));
    assert_eq!(folder_state(&a), folder_state(&b));
    assert!(a.is_collapsed(f));
    assert!(row(&mut a, &g, Act::Folders(true)));
    assert!(b.run_binding(&g, "view.folders --state open"));
    assert_eq!(folder_state(&a), folder_state(&b));
    assert!(!a.is_collapsed(f));
    assert!(!b.run_binding(&g, "view.folders --state sideways"));
}

fn export_state(s: &Studio) -> String {
    format!(
        "{:?}",
        (
            s.popup.as_ref().map(|p| p.kind),
            s.export.stems,
            &s.export.range,
            s.export.fixed_span,
            &s.export.picked,
            &s.export.folders,
        )
    )
}

#[test]
fn the_rows_that_open_the_export_panel_are_view_export_with_ids_or_a_range() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let vocal = track(&a, "Vocal");
    let bass = track(&a, "Bass");
    // The tracks as stems.
    assert!(row(&mut a, &g, Act::Stems(vec![vocal, bass])));
    assert!(b.run_binding(&g, &format!("view.export --ids {vocal},{bass}")));
    assert_eq!(export_state(&a), export_state(&b));
    assert!(a.export.stems);
    assert_eq!(a.export.picked, [vocal, bass]);
    // The loop, a span, and the whole song.
    for (act, line) in [
        (Act::ExportLoop, "view.export --range loop".to_string()),
        (
            Act::ExportSpan(BAR, 3 * BAR),
            format!("view.export --range {BAR}..{}", 3 * BAR),
        ),
        (
            Act::ExportSpan(BAR, 3 * BAR),
            "view.export --range 2.1.0..4.1.0".to_string(),
        ),
    ] {
        let (mut a, g) = studio();
        let (mut b, _) = studio();
        assert!(row(&mut a, &g, act), "{line}");
        assert!(b.run_binding(&g, &line), "{line}");
        assert_eq!(export_state(&a), export_state(&b), "{line}");
        assert!(a.popup.is_some());
    }
    let (mut c, _) = studio();
    assert!(c.run_binding(&g, "view.export --range song"));
    assert!(!c.export.stems && c.popup.is_some());
    // A range that is no range is refused and the panel stays shut.
    let (mut d, _) = studio();
    assert!(!d.run_binding(&g, "view.export --range 4.1.0..2.1.0"));
    assert!(d.error.is_some() && d.popup.is_none());
}

// ------------------------------------------------------------------ lanes and points

#[test]
fn a_lane_is_shown_hidden_and_a_point_is_named_by_track_target_and_at() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let t: TrackId = track(&a, "Bass");
    let lanes = |s: &Studio| {
        s.project().tracks[&t]
            .automation
            .iter()
            .map(|l| (l.target.to_string(), l.hidden))
            .collect::<Vec<_>>()
    };
    assert!(row(&mut a, &g, Act::Lane(t, "pan".into())));
    assert!(b.run_binding(&g, &format!("auto.lane.show --track {t} --target pan")));
    assert_eq!(lanes(&a), lanes(&b));
    assert!(a.arrange.open.contains(&t) && b.arrange.open.contains(&t));
    assert!(row(&mut a, &g, Act::HideLane(t, AutoTarget::Pan)));
    assert!(b.run_binding(&g, &format!("auto.lane.hide --track {t} --target pan")));
    assert_eq!(lanes(&a), lanes(&b));
    assert!(lanes(&a).iter().any(|(n, hidden)| n == "pan" && *hidden));
}

#[test]
fn the_value_box_of_a_point_opens_by_the_row_and_by_the_command() {
    let (mut a, g) = studio();
    let (mut b, _) = studio();
    let t = two_lanes(&mut a, &g, "Bass");
    two_lanes(&mut b, &g, "Bass");
    let at = 4 * BAR;
    assert!(a.run_binding(
        &g,
        &format!("auto.point.value --track {t} --target volume --at {at}")
    ));
    let open = |s: &Studio| {
        s.popup
            .as_ref()
            .map(|p| (matches!(p.kind, PopupKind::AutoValue(_)), p.naming.clone()))
    };
    assert_eq!(open(&a).map(|o| o.0), Some(true), "the value box is open");
    let (x, y) = (120.0, 80.0);
    assert!(row(
        &mut b,
        &g,
        Act::PointValue(t, AutoTarget::Volume, at, x, y)
    ));
    assert_eq!(open(&a), open(&b));
    // There is no point there: nothing opens.
    let (mut c, _) = studio();
    let t = two_lanes(&mut c, &g, "Bass");
    assert!(!c.run_binding(
        &g,
        &format!(
            "auto.point.value --track {t} --target volume --at {}",
            5 * BAR + 7
        )
    ));
}

// ------------------------------------------------------------------ the browser

fn state_of(s: &Studio) -> String {
    format!(
        "{:?}",
        (
            &s.browser.favourites,
            &s.browser.hidden,
            s.browser.show_hidden,
            s.browser.locations().len(),
            s.browser
                .rows()
                .iter()
                .filter(|r| matches!(r, BRow::Item { .. }))
                .count(),
        )
    )
}

#[test]
fn the_favourite_and_hide_rows_say_the_state_they_ask_for() {
    let (mut a, g) = browsing();
    let (mut b, _) = browsing();
    let before = state_of(&a);
    for (act, line) in [
        (
            Act::Browse(BrowseAct::Favourite("Dexed", true)),
            "browser.favorite --item Dexed --on",
        ),
        (
            Act::Browse(BrowseAct::Hide("Dexed", true)),
            "browser.hide --item Dexed",
        ),
        (
            Act::Browse(BrowseAct::Hide("Dexed", false)),
            "browser.hide --item Dexed --off",
        ),
        (
            Act::Browse(BrowseAct::Favourite("Sampler", false)),
            "browser.favorite --item Sampler --off",
        ),
    ] {
        assert!(row(&mut a, &g, act), "{line}");
        assert!(b.run_binding(&g, line), "{line}");
        assert_eq!(state_of(&a), state_of(&b), "{line}");
    }
    assert_ne!(before, state_of(&a));
    assert!(a.browser.favourites.contains("Dexed"));
    assert!(
        !a.browser.favourites.contains("Sampler"),
        "Sampler was a favourite"
    );
    assert!(!a.browser.hidden.contains("Dexed"));
    // Asking for the state it is already in changes nothing, however often it is said.
    let once = state_of(&b);
    assert!(b.run_binding(&g, "browser.favorite --item Dexed --on"));
    assert_eq!(state_of(&b), once);
}

#[test]
fn the_blank_rows_of_a_list_are_commands_with_a_state() {
    let (mut a, g) = browsing();
    let (mut b, _) = browsing();
    assert!(row(&mut a, &g, Act::Browse(BrowseAct::Groups(false))));
    assert!(b.run_binding(&g, "browser.groups --state closed"));
    assert_eq!(state_of(&a), state_of(&b));
    assert!(row(&mut a, &g, Act::Browse(BrowseAct::Groups(true))));
    assert!(b.run_binding(&g, "browser.groups --state open"));
    assert_eq!(state_of(&a), state_of(&b));
    a.browser.set_hidden("Dexed", true);
    b.browser.set_hidden("Dexed", true);
    assert!(row(&mut a, &g, Act::Browse(BrowseAct::ShowHidden(true))));
    assert!(b.run_binding(&g, "view.browser.hidden --on"));
    assert_eq!(state_of(&a), state_of(&b));
    assert!(a.browser.show_hidden);
    assert!(row(&mut a, &g, Act::Browse(BrowseAct::ShowHidden(false))));
    assert!(b.run_binding(&g, "view.browser.hidden --off"));
    assert!(!b.browser.show_hidden);
    // With neither word it turns over.
    assert!(b.run_binding(&g, "view.browser.hidden"));
    assert!(b.browser.show_hidden);
}

#[test]
fn an_instrument_is_put_on_a_new_track_by_the_row_and_by_the_command() {
    let (mut a, g) = browsing();
    let (mut b, _) = browsing();
    let n = a.project().tracks.len();
    assert!(row(&mut a, &g, Act::Browse(BrowseAct::UseNew("Drum Kit"))));
    assert!(b.run_binding(&g, "browser.use --item \"Drum Kit\" --into new"));
    assert_eq!(a.project().tracks.len(), n + 1);
    assert_eq!(b.project().tracks.len(), n + 1);
    assert_eq!(a.song.steps(), b.song.steps());
    // An instrument this PC has not found says so.
    assert!(!b.run_binding(&g, "browser.use --item Vital --into new"));
    assert_eq!(
        b.error.as_deref(),
        Some("「Vital」はまだ見つかっていません")
    );
}

#[test]
fn a_place_is_added_and_removed_by_its_folder() {
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-ui-ops6-place-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("ui-ops6-place");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (mut a, g) = browsing();
    let (mut b, _) = browsing();
    let n = a.browser.locations().len();
    assert!(row(
        &mut a,
        &g,
        Act::Browse(BrowseAct::AddPlace(dir.clone()))
    ));
    assert!(b.run_binding(
        &g,
        &format!(
            "location.add --path \"{}\"",
            dir.display().to_string().replace('\\', "/")
        )
    ));
    assert_eq!(a.browser.locations().len(), n + 1);
    assert_eq!(state_of(&a), state_of(&b));
    let key = a
        .browser
        .locations()
        .last()
        .and_then(|l| l.path.clone())
        .expect("the folder")
        .display()
        .to_string();
    assert!(row(
        &mut a,
        &g,
        Act::Browse(BrowseAct::RemovePlace(key.clone()))
    ));
    assert!(b.run_binding(
        &g,
        &format!("location.remove --location \"{}\"", key.replace('\\', "/"))
    ));
    assert_eq!(a.browser.locations().len(), n);
    assert_eq!(state_of(&a), state_of(&b));
    let _ = std::fs::remove_dir_all(&dir);
}
