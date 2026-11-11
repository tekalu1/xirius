//! A long name in the start list is cut before the time, never over it (UI-351).

use super::delete_tests::listed;
use super::*;
use crate::popup::row_name;

const LONG: &str = "夜明け前のうた（仮）をギターとピアノで録り直したバージョン その3 最終";

fn rows(s: &Studio, g: &Geometry) -> Vec<Cell> {
    let l = s.home_layout(g).unwrap();
    l.cells
        .into_iter()
        .filter(|c| c.look == Look::Row)
        .collect()
}

#[test]
fn a_draft_row_keeps_its_mark_whole_and_the_sample_row_its_tag() {
    let now = xirius_core::store::now_ms();
    let (s, g, _tmp) = listed(&[
        ("saved", false, now - 86_400_000),
        (LONG, true, now - 60_000),
    ]);
    let rows = rows(&s, &g);
    assert_eq!(rows[0].text, format!("{LONG}{DRAFT_MARK}"));
    assert_eq!(rows[0].tail, DRAFT_MARK);
    assert!(!rows[0].hint.is_empty());
    assert_eq!(rows[1].tail, "");
    assert_eq!(rows[1].tag, "");
    assert_eq!(rows.last().unwrap().tag, SAMPLE_TAG);
    assert_eq!(rows.last().unwrap().tail, "");
}

#[test]
fn the_name_drawn_ends_before_the_time_and_the_mark_remains() {
    let now = xirius_core::store::now_ms();
    let (s, g, _tmp) = listed(&[
        ("saved", false, now - 86_400_000),
        (LONG, true, now - 60_000),
    ]);
    let cell = rows(&s, &g).remove(0);
    // The pen's measure, as the estimate the balloon uses: 1 em for a wide letter.
    let wide = |t: &str| crate::browser::text_width(t, FontSize::Small);
    let pad = xirius_design::Space::X2.px();
    let hint = crate::browser::text_width(&cell.hint, FontSize::Caption);
    let end = cell.rect.right() - pad - hint - pad;
    let start = cell.rect.x + pad;
    let body = cell.text.strip_suffix(cell.tail.as_str()).unwrap();
    let name = row_name(body, &cell.tail, end - start, 0.0, wide);
    assert!(name.ends_with("…（保存前）"), "{name}");
    assert!(start + wide(&name) <= end, "{name}");
}

#[test]
fn a_long_saved_name_has_the_same_room_and_no_mark() {
    let now = xirius_core::store::now_ms();
    let (s, g, _tmp) = listed(&[(LONG, false, now - 4 * 86_400_000)]);
    let cell = rows(&s, &g).remove(0);
    assert_eq!(cell.text, LONG);
    assert_eq!(cell.tail, "");
    assert!(!cell.hint.is_empty());
}

#[test]
fn the_screen_reader_is_told_the_whole_name() {
    let now = xirius_core::store::now_ms();
    let (s, g, _tmp) = listed(&[(LONG, true, now - 60_000)]);
    let marks = crate::a11y::marks(&s, &g);
    let list = marks.last().unwrap();
    let said: Vec<String> = list
        .children
        .iter()
        .map(|m| m.node.label().unwrap_or_default().to_string())
        .collect();
    assert!(said[0].contains(LONG), "{said:?}");
}
