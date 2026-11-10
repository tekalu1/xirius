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
