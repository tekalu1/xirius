//! The menus of the tempo row (S15, S16), pressed as the window presses them.

use serde_json::json;

use super::tests::{BAR, choose, enabled, labels, steps, studio};
use crate::studio::{Geometry, Studio};

/// The sample song with a second tempo point at bar 9, slowing to 90.
fn two_points() -> (Studio, Geometry) {
    let (mut s, g) = studio();
    s.song
        .run("tempo.set", json!({ "at": 8 * BAR, "bpm": 90 }))
        .unwrap();
    (s, g)
}

/// A press on tempo point `i`.
fn press_point(s: &mut Studio, g: &Geometry, i: usize) -> bool {
    let t = s.project().tempo[i].clone();
    let (x, y) = s.tempo_xy(g, t.tick, t.bpm);
    s.scene_context(g, x, y)
}

/// A press in the tempo row at bar `bar`, off the points.
fn press_row(s: &mut Studio, g: &Geometry, bar: i64) -> bool {
    let x = s.tick_x(g, (bar - 1) * BAR) + 2.0;
    s.scene_context(g, x, g.tempo.y + 3.0)
}

#[test]
fn a_tempo_point_has_the_rows_of_the_mock() {
    let (mut s, g) = two_points();
    assert!(press_point(&mut s, &g, 1));
    assert_eq!(
        labels(&s),
        [
            "BPM を入力…",
            "次のポイントまでの形",
            "テンポポイントを削除"
        ]
    );
    assert!(enabled(&s, "テンポポイントを削除"));
}

#[test]
fn the_first_point_cannot_be_deleted_and_the_last_has_no_shape_to_a_next_point() {
    let (mut s, g) = two_points();
    assert!(press_point(&mut s, &g, 0));
    assert!(!enabled(&s, "テンポポイントを削除"));
    assert!(enabled(&s, "次のポイントまでの形"));
    assert!(press_point(&mut s, &g, 1));
    assert!(!enabled(&s, "次のポイントまでの形"));
}

#[test]
fn the_bpm_of_a_point_is_typed_and_one_undo_takes_it_back() {
    let (mut s, g) = two_points();
    assert!(press_point(&mut s, &g, 1));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "BPM を入力…"));
    assert_eq!(s.ask_field().map(|f| f.text()), Some("90".to_string()));
    for _ in 0..4 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "140", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    let p = s.project();
    assert_eq!(p.tempo.len(), 2);
    assert_eq!(p.tempo[1].bpm, 140.0);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tempo[1].bpm, 90.0);
}

#[test]
fn the_shape_to_the_next_point_is_a_stair_or_a_line() {
    let (mut s, g) = two_points();
    assert!(press_point(&mut s, &g, 0));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    assert_eq!(labels(&s), ["戻る", "階段", "直線"]);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "直線"));
    assert!(s.project().tempo[0].ramp);
    assert_eq!(steps(&s), n + 1);
    // The row that is on now is marked.
    assert!(press_point(&mut s, &g, 0));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    let on: Vec<_> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(on, ["直線"]);
    assert!(s.song.undo());
    assert!(!s.project().tempo[0].ramp);
}

#[test]
fn a_point_is_deleted_by_its_menu_in_one_step() {
    let (mut s, g) = two_points();
    assert!(press_point(&mut s, &g, 1));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "テンポポイントを削除"));
    assert_eq!(s.project().tempo.len(), 1);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tempo.len(), 2);
}

#[test]
fn the_row_between_the_points_adds_a_point_with_the_tempo_that_stands_there() {
    let (mut s, g) = two_points();
    assert!(press_row(&mut s, &g, 4));
    assert_eq!(
        labels(&s),
        [
            "ここにポイントを追加",
            "貼り付け",
            "テンポの変化をすべて削除"
        ]
    );
    assert!(!enabled(&s, "貼り付け"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ここにポイントを追加"));
    let p = s.project();
    let t = p.tempo.iter().find(|t| t.tick == 3 * BAR).expect("a point");
    assert_eq!(t.bpm, 120.0);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tempo.len(), 2);
}

#[test]
fn the_changes_are_cleared_but_the_first_point_and_a_song_without_changes_is_dim() {
    let (mut s, g) = studio();
    assert!(press_row(&mut s, &g, 4));
    assert!(!enabled(&s, "テンポの変化をすべて削除"));
    let (mut s, g) = two_points();
    assert!(press_row(&mut s, &g, 4));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "テンポの変化をすべて削除"));
    assert_eq!(s.project().tempo.len(), 1);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tempo.len(), 2);
}
