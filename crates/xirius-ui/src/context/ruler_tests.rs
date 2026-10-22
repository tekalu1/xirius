//! The menus of the ruler (S10) and of the loop band (S11), pressed as the window presses
//! them.

use serde_json::json;

use super::tests::{BAR, choose, enabled, labels, steps, studio};
use crate::export::Range;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Studio};

/// A press on the bar numbers of the ruler, at bar `bar` (1-based).
fn press_ruler(s: &mut Studio, g: &Geometry, bar: i64) -> bool {
    let x = s.tick_x(g, (bar - 1) * BAR) + 2.0;
    s.scene_context(g, x, g.ruler.y + g.ruler.h - 4.0)
}

/// A press on the loop band, at bar `bar`.
fn press_band(s: &mut Studio, g: &Geometry, bar: i64) -> bool {
    let x = s.tick_x(g, (bar - 1) * BAR) + 2.0;
    s.scene_context(g, x, g.ruler.y + 4.0)
}

#[test]
fn the_ruler_has_its_rows_in_the_order_of_the_mock() {
    let (mut s, g) = studio();
    assert!(press_ruler(&mut s, &g, 3));
    assert_eq!(
        labels(&s),
        [
            "ここから再生",
            "マーカーを追加",
            "ループの始まりをここに",
            "ループの終わりをここに",
            "テンポを設定…",
            "拍子を設定",
            "目盛り",
        ]
    );
}

#[test]
fn the_loop_rows_put_the_ends_where_the_press_was_in_one_step() {
    let (mut s, g) = studio();
    s.song
        .run("loop.set", json!({ "start": 4 * BAR, "end": 8 * BAR }))
        .unwrap();
    assert!(press_ruler(&mut s, &g, 6));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ループの終わりをここに"));
    let l = s.project().loop_range.expect("a loop");
    assert_eq!((l.start, l.end), (4 * BAR, 5 * BAR));
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().loop_range.unwrap().end, 8 * BAR);
}

#[test]
fn play_from_here_moves_the_playhead_to_the_press() {
    let (mut s, g) = studio();
    assert!(press_ruler(&mut s, &g, 5));
    assert!(choose(&mut s, &g, "ここから再生"));
    assert_eq!(s.playhead, 4 * BAR);
}

#[test]
fn the_tempo_row_asks_for_the_number_and_one_undo_takes_it_back() {
    let (mut s, g) = studio();
    assert!(press_ruler(&mut s, &g, 9));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "テンポを設定…"));
    assert!(s.context_open());
    // It starts with the tempo that stands at the press.
    assert_eq!(s.ask_field().map(|f| f.text()), Some("120".to_string()));
    // What is no number leaves the box open.
    for _ in 0..8 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "fast", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(s.context_open());
    assert_eq!(steps(&s), n);
    for _ in 0..8 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "90", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    let p = s.project();
    let at = p.tempo.iter().find(|t| t.tick == 8 * BAR).expect("a point");
    assert_eq!(at.bpm, 90.0);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.project().tempo.iter().all(|t| t.tick != 8 * BAR));
}

#[test]
fn the_meter_row_offers_the_meters_and_sets_the_bar_of_the_press() {
    let (mut s, g) = studio();
    assert!(press_ruler(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "拍子を設定"));
    assert_eq!(
        labels(&s),
        [
            "戻る", "2/4", "3/4", "4/4", "5/4", "6/8", "7/8", "9/8", "12/8"
        ]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "6/8"));
    let p = s.project();
    let m = p.timesig.iter().find(|m| m.bar == 7).expect("a change");
    assert_eq!((m.num, m.den), (6, 8));
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.project().timesig.iter().all(|m| m.bar != 7));
}

#[test]
fn the_loop_band_has_the_loop_rows_and_the_rest_of_the_ruler_does_not() {
    let (mut s, g) = studio();
    s.song
        .run("loop.set", json!({ "start": 4 * BAR, "end": 8 * BAR }))
        .unwrap();
    assert!(press_band(&mut s, &g, 6));
    assert_eq!(
        labels(&s),
        [
            "ループ",
            "ループ範囲のものを選択",
            "ループの中を書き出す…",
            "複製",
            "セクションにする",
            "時間を挿入",
            "時間を削除",
        ]
    );
    assert!(enabled(&s, "ループ"));
    // Outside the loop the band is the ruler's.
    assert!(press_band(&mut s, &g, 12));
    assert_eq!(labels(&s)[0], "ここから再生");
}

#[test]
fn the_loop_row_that_exports_opens_the_mixdown_of_the_loop() {
    let (mut s, g) = studio();
    s.song
        .run("loop.set", json!({ "start": 4 * BAR, "end": 8 * BAR }))
        .unwrap();
    assert!(press_band(&mut s, &g, 6));
    assert!(choose(&mut s, &g, "ループの中を書き出す…"));
    assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
    assert!(!s.export.stems);
    assert_eq!(s.export.range, Range::Loop);
}
