//! The colour square at the left of a track's head, and the colour choice it opens (the feel
//! pass, item 09), pressed the way the window presses them.

use crate::arrange::probe::track;
use crate::draw::track_color;
use crate::popup::{Action, Look, PopupKind, SWATCH, SWATCH_COLUMNS, SWATCH_GAP};
use crate::studio::{Geometry, Hit, Rect, RowParts, Studio};
use crate::tree::RowKind;
use xirius_design::{Layout, Space};

fn studio() -> (Studio, Geometry) {
    let s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    (s, g)
}

/// Where a part of a track's head is on the window.
fn on_screen(s: &Studio, g: &Geometry, name: &str, part: impl Fn(&RowParts) -> Rect) -> Rect {
    let row = *s
        .tree()
        .row_of(RowKind::Track(track(s, name)))
        .expect("a row");
    let r = part(&RowParts::of(&row));
    Rect::new(g.column.x + r.x, s.row_y(g, &row) + r.y, r.w, r.h)
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[test]
fn the_head_is_16_px_wider_and_the_square_sits_left_of_the_kind_icon() {
    assert_eq!(Layout::TRACK_HEAD_W, 216.0);
    let (s, _) = studio();
    let row = *s.tree().row_of(RowKind::Track(track(&s, "Vocal"))).unwrap();
    let p = RowParts::of(&row);
    let sw = p.swatch.expect("a track has the square");
    assert_eq!(sw.x, row.content_x());
    assert_eq!((sw.w, sw.h), (RowParts::SWATCH_CELL, RowParts::SWATCH_CELL));
    assert_eq!(sw.right(), p.icon.x, "the icon follows the square");
    let mid = |r: Rect| r.y + r.h / 2.0;
    assert_eq!(mid(sw), mid(p.icon), "on the icon's line");
    assert_eq!(p.name_x, p.icon.right() + Space::X2.px());
    assert_eq!(
        p.mute.unwrap().x,
        p.name_x,
        "the buttons start under the name"
    );
    // The name has as much room as before: the square took the 16 px the head gained.
    let before = row.content_x() + Layout::ROW_ICON + Space::X2.px();
    assert_eq!(p.name_right - p.name_x, (p.name_right - 16.0) - before);
}

#[test]
fn a_folder_has_no_colour_square() {
    let (s, _) = studio();
    let f = *s
        .tree()
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Folder(_)))
        .expect("a folder");
    assert!(RowParts::of(&f).swatch.is_none());
}

#[test]
fn the_square_and_the_icon_are_two_targets() {
    let (s, g) = studio();
    let t = track(&s, "Vocal");
    let (x, y) = centre(on_screen(&s, &g, "Vocal", |p| p.swatch.unwrap()));
    assert_eq!(s.hit(&g, x, y), Hit::TrackColor(t));
    let (x, y) = centre(on_screen(&s, &g, "Vocal", |p| p.icon));
    assert_eq!(s.hit(&g, x, y), Hit::TrackIcon(t));
}

#[test]
fn pressing_the_square_opens_the_colour_choice_under_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let sq = on_screen(&s, &g, "Vocal", |p| p.swatch.unwrap());
    let (x, y) = centre(sq);
    let hit = s.hit(&g, x, y);
    assert!(s.press(&g, hit));
    let p = s.popup.as_ref().expect("the choice is open");
    assert_eq!(p.kind, PopupKind::Color(t));
    assert_eq!(p.anchor, sq);
}

#[test]
fn the_icon_does_not_open_the_colour_choice() {
    let (mut s, g) = studio();
    let (x, y) = centre(on_screen(&s, &g, "Vocal", |p| p.icon));
    let hit = s.hit(&g, x, y);
    assert!(!s.press(&g, hit));
    assert!(s.popup.is_none());
}

#[test]
fn the_choice_is_two_rows_of_eight_with_a_check_on_the_current_colour() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let now = track_color(&s.project().tracks[&t]);
    assert!(s.press(&g, Hit::TrackColor(t)));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert_eq!(layout.cells.len(), 16, "no square for 'none'");
    assert!(
        layout
            .cells
            .iter()
            .all(|c| matches!(c.look, Look::Swatch(Some(_))) && c.rect.w == SWATCH)
    );
    let col = |i: usize| layout.cells[i].rect.x;
    let row = |i: usize| layout.cells[i].rect.y;
    for i in 0..16 {
        assert_eq!(col(i), col(i % SWATCH_COLUMNS));
        assert_eq!(row(i), row(i - i % SWATCH_COLUMNS));
    }
    assert_eq!(col(1) - col(0), SWATCH + SWATCH_GAP);
    assert_eq!(row(8) - row(0), SWATCH + SWATCH_GAP);
    assert_eq!(layout.rect.w, Layout::MENU_W, "as wide as a menu");
    let checked: Vec<_> = layout.cells.iter().filter(|c| c.on).collect();
    assert_eq!(checked.len(), 1);
    assert_eq!(checked[0].action, Some(Action::Color(Some(now))));
}

#[test]
fn choosing_a_square_paints_the_track_in_one_step_and_the_check_follows() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let now = track_color(&s.project().tracks[&t]);
    let other = (now + 7) % 16;
    assert!(s.press(&g, Hit::TrackColor(t)));
    let cell = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action == Some(Action::Color(Some(other))))
        .unwrap();
    assert!(s.press(&g, Hit::Item(cell)));
    assert!(s.popup.is_none());
    assert_eq!(track_color(&s.project().tracks[&t]), other);
    assert!(s.press(&g, Hit::TrackColor(t)));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert_eq!(
        layout.cells.iter().position(|c| c.on),
        Some(other),
        "the check moved"
    );
}
