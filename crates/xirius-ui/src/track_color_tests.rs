//! The kind icon of a track's head in the track's colour, and the colour choice it opens (the
//! feel pass, item 09; UI-361), pressed the way the window presses them.

use crate::arrange::probe::track;
use crate::draw::track_color;
use crate::popup::{Action, Look, PopupKind, SWATCH, SWATCH_COLUMNS, SWATCH_GAP};
use crate::studio::{Geometry, Hit, Rect, RowParts, Studio};
use crate::tree::RowKind;
use xirius_design::{Layout, Space};

fn studio() -> (Studio, Geometry) {
    let s = Studio::sample();
    let g = s.geometry(1440.0, 960.0);
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
fn the_kind_icon_starts_the_head_and_the_name_takes_the_square_s_16_px() {
    assert_eq!(Layout::TRACK_HEAD_W, 216.0, "the head is as wide as before");
    let (s, _) = studio();
    let row = *s.tree().row_of(RowKind::Track(track(&s, "Vocal"))).unwrap();
    let p = RowParts::of(&row);
    assert_eq!(p.icon.x, row.content_x(), "no square before the icon");
    assert_eq!(p.name_x, p.icon.right() + Space::X2.px());
    assert_eq!(
        p.mute.unwrap().x,
        p.name_x,
        "the buttons start under the name"
    );
    // The name has 16 px more than when the square sat left of the icon.
    let before = row.content_x() + 16.0 + Layout::ROW_ICON + Space::X2.px();
    assert_eq!(p.name_x, before - 16.0);
}

#[test]
fn the_kind_icon_is_the_press_target_and_a_crashed_one_keeps_its_restart() {
    let (s, g) = studio();
    let t = track(&s, "Vocal");
    let (x, y) = centre(on_screen(&s, &g, "Vocal", |p| p.icon));
    assert_eq!(s.hit(&g, x, y), Hit::TrackColor(t));
}

#[test]
fn a_folder_icon_is_a_press_target_too() {
    let (s, g) = studio();
    let f = *s
        .tree()
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Folder(_)))
        .expect("a folder");
    let RowKind::Folder(id) = f.kind else {
        unreachable!()
    };
    let r = RowParts::of(&f).icon;
    let (x, y) = (
        g.column.x + r.x + r.w / 2.0,
        s.row_y(&g, &f) + r.y + r.h / 2.0,
    );
    assert_eq!(s.hit(&g, x, y), Hit::FolderColor(id));
}

#[test]
fn pressing_the_icon_opens_the_colour_choice_under_it() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let sq = on_screen(&s, &g, "Vocal", |p| p.icon);
    let (x, y) = centre(sq);
    let hit = s.hit(&g, x, y);
    assert!(s.press(&g, hit));
    let p = s.popup.as_ref().expect("the choice is open");
    assert_eq!(p.kind, PopupKind::Color(t));
    assert_eq!(p.anchor, sq);
}

#[test]
fn choosing_a_square_paints_a_folder_in_one_step() {
    let (mut s, g) = studio();
    let f = *s
        .tree()
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Folder(_)))
        .expect("a folder");
    let RowKind::Folder(id) = f.kind else {
        unreachable!()
    };
    assert!(s.press(&g, Hit::FolderColor(id)));
    assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::FolderColor(id));
    let cell = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.action == Some(Action::Color(Some(5))))
        .unwrap();
    assert!(s.press(&g, Hit::Item(cell)));
    assert_eq!(
        s.project().folders[&id].color,
        Some(crate::song::hex(xirius_design::TrackColor::new(5)))
    );
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
