use super::tests::{choose, labels, lane_at, studio};
use crate::popup::Action;
use crate::studio::{Geometry, Studio};

fn open_select(s: &mut Studio, g: &Geometry) {
    let (x, y) = lane_at(s, g, "Keys", 13);
    assert!(s.scene_context(g, x, y));
}

fn child_cells(
    s: &Studio,
    g: &Geometry,
    viewport: (f32, f32),
) -> (Vec<(Action, crate::studio::Rect)>, usize) {
    let layout = s.popup.as_ref().unwrap().layout(viewport);
    let _ = g;
    let cells = layout
        .cells
        .iter()
        .filter_map(|c| c.action.clone().map(|a| (a, c.rect)))
        .collect();
    (cells, layout.panels.len())
}

/// T3: a menu that fits one face keeps the caller's rows; no category group appears.
#[test]
fn a_small_menu_has_no_category_groups() {
    let (mut s, g) = studio();
    open_select(&mut s, &g);
    let rows = labels(&s);
    assert!(rows.len() <= 12, "{rows:?}");
    for word in [
        "編集",
        "クリップ",
        "オーディオ",
        "MIDI",
        "トラック",
        "機器",
        "時間",
        "表示",
        "書き出し",
        "ファイル",
    ] {
        assert!(
            !rows.contains(&word.to_string()),
            "{word} group in a small menu"
        );
    }
}

/// T11: gaps stay between rows of the panel and never lead, trail or repeat.
#[test]
fn gaps_never_lead_trail_or_repeat() {
    let (mut s, g) = studio();
    let (x, y) = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    let rows = s.context_rows();
    assert!(rows.iter().any(|r| r.gap), "the caller's gap is kept");
    assert!(!rows.first().unwrap().gap && !rows.last().unwrap().gap);
    assert!(rows.windows(2).all(|w| !(w[0].gap && w[1].gap)));
}

/// T12: a conditional row appearing or going away does not change the rows shown.
#[test]
fn the_shape_does_not_change_with_conditions() {
    let (mut s, g) = studio();
    open_select(&mut s, &g);
    let before = labels(&s);
    s.press(&g, crate::studio::Hit::None);
    open_select(&mut s, &g);
    assert_eq!(labels(&s), before);
}

/// T7: beside its parent a child panel has no "戻る" and its 1st row is level with the parent row.
#[test]
fn a_child_beside_its_parent_has_no_back_row_and_lines_up() {
    let (mut s, g) = studio();
    open_select(&mut s, &g);
    assert!(choose(&mut s, &g, "選択"));
    let (cells, panels) = child_cells(&s, &g, g.size);
    assert_eq!(panels, 2);
    assert!(
        !cells.iter().any(|(a, _)| *a == Action::Ctx(0)),
        "戻る is hidden"
    );
    let first = cells
        .iter()
        .find(|(a, _)| *a == Action::Ctx(2))
        .expect("1st row");
    let picked = s.popup.as_ref().unwrap().ctx_parents[0].1;
    let parent = cells
        .iter()
        .find(|(a, _)| *a == Action::CtxAncestor(0, Box::new(Action::Ctx(picked))))
        .expect("the parent row");
    assert_eq!(first.1.y, parent.1.y, "1st row level with the parent row");
}

/// T7: a child that covers more than half of its parent keeps its "戻る" row.
#[test]
fn a_child_over_its_parent_keeps_the_back_row() {
    let (mut s, g) = studio();
    open_select(&mut s, &g);
    assert!(choose(&mut s, &g, "選択"));
    let wide = s.popup.as_ref().unwrap().layout(g.size);
    let widest = wide.panels.iter().map(|(r, _)| r.w).fold(0.0, f32::max);
    let (cells, _) = child_cells(&s, &g, (widest + 8.0, g.size.1));
    assert!(
        cells.iter().any(|(a, _)| *a == Action::Ctx(0)),
        "戻る is shown"
    );
}

/// A child panel that scrolls, beside its parent (no "戻る"): ↓ walks to the last row and the
/// panel's top and bottom edges land where the rows end, with no row hidden and no empty line.
#[test]
fn a_scrolling_child_without_back_reaches_both_ends() {
    use super::Item;
    use serde_json::json;
    let (mut s, _) = studio();
    let g = s.geometry(1440.0, 360.0);
    s.resized(&g);
    let many: Vec<Item> = (0..30)
        .map(|n| s.ctx_as(&format!("行 {n}"), "marker.add", json!({"at": n * 3840})))
        .collect();
    let items = vec![
        s.ctx_as("先頭", "marker.add", json!({"at": 0})),
        Item::sub("多い", many),
        s.ctx_as("末尾", "marker.add", json!({"at": 1})),
    ];
    s.open_context(items, 100.0, 40.0);
    assert!(choose(&mut s, &g, "多い"));
    let pad = xirius_design::Layout::POPUP_PAD;
    let row_h = xirius_design::Layout::MENU_ROW_H;
    let visible = |s: &Studio| {
        let p = s.popup.as_ref().unwrap();
        let room = g.size.1
            - 8.0
            - if p.ctx_query.is_some() {
                crate::search_box::MENU_ROOM
            } else {
                0.0
            };
        let layout = p.layout(g.size);
        let cells: Vec<_> = layout
            .cells
            .iter()
            .filter_map(|c| match &c.action {
                Some(Action::Ctx(i)) => Some((*i, c.rect)),
                _ => None,
            })
            .collect();
        (cells, room, p.offset, p.max_offset(g.size))
    };
    // Entered: the "戻る" row is hidden and the panel is at offset 0 (a panel as tall as the
    // window cannot line up with its parent row; that is the unscrolled tests' business).
    let (cells, _, offset, max) = visible(&s);
    assert_eq!(offset, 0);
    assert!(max > 0, "the panel scrolls");
    assert!(!cells.iter().any(|(i, _)| *i == 0), "戻る is hidden");
    let first = cells.iter().find(|(i, _)| *i == 2).expect("1st row");
    // ↓ through every row: the cursor row is always drawn.
    for _ in 0..30 {
        assert!(s.context_navigation(&g, "down"));
        let cursor = s.ctx.as_ref().unwrap().cursor.expect("cursor");
        let (cells, room, _, _) = visible(&s);
        let cell = cells.iter().find(|(i, _)| *i == cursor);
        let cell = cell.unwrap_or_else(|| panic!("cursor row {cursor} is drawn"));
        assert!(
            cell.1.bottom() <= room,
            "cursor {cursor} bottom {} room {room}",
            cell.1.bottom()
        );
    }
    // The last row is the cursor: the list is at its end, flush with the bottom edge.
    let (cells, room, offset, max) = visible(&s);
    assert_eq!(offset, max, "scrolled to the end");
    let last = cells.iter().map(|(_, r)| r.bottom()).fold(0.0, f32::max);
    assert!(last <= room);
    assert!(
        last + row_h > room - pad - 2.0,
        "no empty line below the last row"
    );
    // ↑ back to the top: the first row is drawn at the top edge, offset 0.
    for _ in 0..29 {
        assert!(s.context_navigation(&g, "up"));
    }
    let (cells, _, offset, _) = visible(&s);
    assert_eq!(offset, 0);
    let top = cells.iter().map(|(_, r)| r.y).fold(f32::MAX, f32::min);
    assert_eq!(top, first.1.y, "the first row is where it was");
}

#[test]
fn the_menu_search_row_is_as_tall_as_every_search_field() {
    use super::Item;
    use serde_json::json;
    let (mut s, _) = studio();
    let g = s.geometry(1440.0, 900.0);
    s.resized(&g);
    let items: Vec<Item> = (0..30)
        .map(|n| s.ctx_as(&format!("行 {n}"), "marker.add", json!({"at": n * 3840})))
        .collect();
    s.open_context(items, 100.0, 40.0);
    let p = s.popup.as_ref().unwrap();
    assert!(p.ctx_query.is_some(), "a long menu has the search row");
    let search = p
        .layout(g.size)
        .cells
        .iter()
        .find(|c| c.action == Some(Action::CtxSearch))
        .map(|c| c.rect.h);
    assert_eq!(search, Some(crate::search_box::HEIGHT));
}
