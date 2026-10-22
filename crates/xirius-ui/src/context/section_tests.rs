//! The section band as the window drives it: a press selects, the right press opens the menu
//! of a section (CTX-023) or of the empty band (CTX-024), the keys act on the selection.

use serde_json::json;

use super::tests::{BAR, choose, enabled, key, labels, steps, studio};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

const NONE: Mods = Mods {
    ctrl: false,
    shift: false,
    alt: false,
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const ALT: Mods = Mods { alt: true, ..NONE };
const CTRL_ALT: Mods = Mods {
    ctrl: true,
    alt: true,
    ..NONE
};

/// The middle of the band at bar `bar` (1-based) and a little in.
fn band_at(s: &Studio, g: &Geometry, bar: i64) -> (f32, f32) {
    let x = s.tick_x(g, (bar - 1) * BAR) + 24.0;
    assert!(x < g.section_band.right(), "bar {bar} is off screen");
    (x, g.section_band.y + g.section_band.h / 2.0)
}

fn sections(s: &Studio) -> Vec<(String, i64, i64)> {
    s.project()
        .sections
        .iter()
        .map(|x| (x.name.clone(), x.start / BAR + 1, x.length / BAR))
        .collect()
}

/// The sample song with its Chorus taken out: bars 13 and on are free, and on screen.
fn with_room() -> (Studio, Geometry) {
    let (mut s, g) = studio();
    let chorus = s.project().sections[2].id.to_string();
    s.song
        .run("section.delete", json!({ "id": chorus }))
        .unwrap();
    s.rebuild(&g);
    (s, g)
}

/// A press and a release on the band at `bar`.
fn click_band(s: &mut Studio, g: &Geometry, bar: i64) -> bool {
    let (x, y) = band_at(s, g, bar);
    let used = s.arrange_down(g, x, y, NONE);
    s.arrange_up(g, x, y, NONE);
    used
}

fn press_section(s: &mut Studio, g: &Geometry, bar: i64) -> bool {
    let (x, y) = band_at(s, g, bar);
    s.scene_context(g, x, y)
}

#[test]
fn a_press_on_a_section_selects_it_and_leaves_the_playhead_where_it_is() {
    let (mut s, g) = studio();
    s.seek(3 * BAR);
    let p = s.project();
    let (track, clip) = p
        .tracks
        .values()
        .find_map(|t| t.clips.first().map(|c| (t.id, c.id)))
        .expect("a clip");
    s.arrange.clips.insert((track, clip));
    let verse = s.project().sections[1].id;
    assert!(click_band(&mut s, &g, 7), "the press is used here");
    assert_eq!(s.arrange.section, Some(verse));
    assert_eq!(s.playhead, 3 * BAR, "no seek");
    assert!(s.arrange.clips.is_empty(), "the clips let go");
}

#[test]
fn a_press_on_the_empty_band_lets_the_section_go_and_does_not_seek() {
    let (mut s, g) = with_room();
    click_band(&mut s, &g, 7);
    assert!(s.arrange.section.is_some());
    s.seek(2 * BAR);
    assert!(click_band(&mut s, &g, 15));
    assert_eq!(s.arrange.section, None);
    assert_eq!(s.playhead, 2 * BAR);
}

#[test]
fn the_bar_numbers_of_the_ruler_still_seek() {
    let (mut s, g) = studio();
    let x = s.tick_x(&g, 9 * BAR) + 2.0;
    let y = g.ruler.y + g.ruler.h - 4.0;
    assert!(!s.arrange_down(&g, x, y, NONE));
    let hit = s.hit(&g, x, y);
    s.press(&g, hit);
    assert!((s.playhead - 9 * BAR).abs() < BAR / 4);
}

#[test]
fn the_right_press_on_a_section_opens_its_menu_in_the_order_of_the_mock() {
    let (mut s, g) = studio();
    assert!(press_section(&mut s, &g, 7));
    assert!(s.context_open());
    assert_eq!(s.arrange.section, Some(s.project().sections[1].id));
    assert_eq!(
        labels(&s),
        [
            "セクション名を変更",
            "色",
            "複製",
            "セクションをループ",
            "セクションのものを選択",
            "ここで分割",
            "セクションを書き出す…",
            "切り取り",
            "コピー",
            "貼り付け",
            "削除",
            "時間を削除",
        ]
    );
    assert_eq!(key(&s, "複製"), "Alt+D");
    assert_eq!(key(&s, "セクションをループ"), "P");
    assert_eq!(key(&s, "切り取り"), "Ctrl+X");
    assert_eq!(key(&s, "コピー"), "Ctrl+C");
    assert_eq!(key(&s, "貼り付け"), "Ctrl+V");
    assert_eq!(key(&s, "削除"), "Del / Backspace");
    assert_eq!(key(&s, "時間を削除"), "Ctrl+Alt+D");
    // Nothing was copied.
    assert!(!enabled(&s, "貼り付け"));
}

#[test]
fn the_name_rows_are_the_six_names_and_a_box() {
    let (mut s, g) = studio();
    assert!(press_section(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "セクション名を変更"));
    assert_eq!(
        labels(&s),
        [
            "戻る",
            "Intro",
            "Verse",
            "Pre-Chorus",
            "Chorus",
            "Bridge",
            "Outro",
            "名前を入力…"
        ]
    );
    let on: Vec<_> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(on, ["Verse"], "the current name has the check");
    let n = steps(&s);
    assert!(choose(&mut s, &g, "Bridge"));
    assert_eq!(s.project().sections[1].name, "Bridge");
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().sections[1].name, "Verse");
}

#[test]
fn a_typed_name_renames_in_one_step() {
    let (mut s, g) = studio();
    assert!(press_section(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "セクション名を変更"));
    assert!(choose(&mut s, &g, "名前を入力…"));
    assert_eq!(s.ask_field().map(|f| f.text()), Some("Verse".to_string()));
    for _ in 0..5 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "A メロ", false);
    let n = steps(&s);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert_eq!(s.project().sections[1].name, "A メロ");
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn a_double_click_on_a_section_opens_the_name_box() {
    let (mut s, g) = studio();
    let (x, y) = band_at(&s, &g, 7);
    assert!(s.arrange_open(&g, x, y, NONE));
    assert!(s.context_open());
    assert_eq!(s.ask_field().map(|f| f.text()), Some("Verse".to_string()));
    for _ in 0..5 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "Bridge", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert_eq!(s.project().sections[1].name, "Bridge");
}

#[test]
fn the_colour_row_sets_the_sections_colour() {
    let (mut s, g) = studio();
    assert!(press_section(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "色"));
    assert!(choose(&mut s, &g, "なし"));
    assert_eq!(s.project().sections[1].color, None);
}

#[test]
fn copy_then_paste_on_the_empty_band_puts_a_section_there() {
    let (mut s, g) = with_room();
    assert!(press_section(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "コピー"));
    // The empty band after the last section.
    assert!(press_section(&mut s, &g, 15));
    assert_eq!(labels(&s), ["セクションを追加", "貼り付け"]);
    assert!(enabled(&s, "貼り付け"));
    assert_eq!(key(&s, "貼り付け"), "Ctrl+V");
    let n = steps(&s);
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(steps(&s), n + 1);
    let all = sections(&s);
    assert_eq!(all.len(), 3);
    assert_eq!(all[2], ("Verse".to_string(), 15, 8));
    assert_eq!(s.arrange.section, Some(s.project().sections[2].id));
}

#[test]
fn the_empty_band_adds_a_named_section_on_the_bar_pressed() {
    let (mut s, g) = with_room();
    assert!(press_section(&mut s, &g, 15));
    assert_eq!(labels(&s)[0], "セクションを追加");
    // Nothing to paste.
    assert!(!enabled(&s, "貼り付け"));
    assert!(choose(&mut s, &g, "セクションを追加"));
    assert_eq!(
        labels(&s),
        [
            "戻る",
            "Intro",
            "Verse",
            "Pre-Chorus",
            "Chorus",
            "Bridge",
            "Outro"
        ]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "Bridge"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(sections(&s)[2], ("Bridge".to_string(), 15, 1));
    assert_eq!(s.arrange.section, Some(s.project().sections[2].id));
}

#[test]
fn cut_takes_the_time_out_and_paste_puts_it_back_after_the_selection() {
    let (mut s, g) = studio();
    assert!(press_section(&mut s, &g, 7));
    assert!(choose(&mut s, &g, "切り取り"));
    assert_eq!(
        sections(&s),
        [("Intro".to_string(), 1, 4), ("Chorus".to_string(), 5, 8)]
    );
    assert_eq!(s.arrange.section, None);
    // Select Intro and paste with the key: it goes right after it.
    click_band(&mut s, &g, 2);
    assert!(s.mapped_key(&g, "v", None, CTRL));
    assert_eq!(
        sections(&s)
            .iter()
            .map(|x| x.0.as_str())
            .collect::<Vec<_>>(),
        ["Intro", "Verse", "Chorus"]
    );
    assert_eq!(s.arrange.section, Some(s.project().sections[1].id));
}

#[test]
fn the_keys_act_on_the_selected_section() {
    let (mut s, g) = studio();
    click_band(&mut s, &g, 7);
    // Alt+D copies it right after itself and selects the copy.
    assert!(s.mapped_key(&g, "d", None, ALT));
    assert_eq!(sections(&s).len(), 4);
    assert_eq!(sections(&s)[2], ("Verse".to_string(), 13, 8));
    assert_eq!(s.arrange.section, Some(s.project().sections[2].id));
    // Delete takes the selected one away.
    assert!(s.mapped_key(&g, "delete", None, NONE));
    assert_eq!(sections(&s).len(), 3);
    assert_eq!(s.arrange.section, None);
}

#[test]
fn p_loops_the_selected_section_exactly() {
    let (mut s, g) = studio();
    click_band(&mut s, &g, 7);
    assert!(s.mapped_key(&g, "p", None, NONE));
    let l = s.project().loop_range.expect("a loop");
    assert_eq!((l.start, l.end), (4 * BAR, 12 * BAR));
}

#[test]
fn the_time_keys_take_the_range_then_the_section_then_the_loop() {
    let (mut s, g) = studio();
    s.song
        .run("loop.set", json!({ "start": 0, "end": BAR }))
        .unwrap();
    // The loop alone.
    let song_end = |s: &Studio| {
        s.project()
            .sections
            .iter()
            .map(|x| x.start + x.length)
            .max()
    };
    let end = song_end(&s).unwrap();
    assert!(s.mapped_key(&g, "i", None, CTRL_ALT));
    assert_eq!(song_end(&s), Some(end + BAR), "one bar of the loop");
    // The selected section beats the loop.
    click_band(&mut s, &g, 7);
    let len = s.project().sections[1].length;
    let end = song_end(&s).unwrap();
    assert!(s.mapped_key(&g, "i", None, CTRL_ALT));
    assert_eq!(song_end(&s), Some(end + len));
    // The range beats the section.
    s.arrange.range = Some((0, 2 * BAR));
    let end = song_end(&s).unwrap();
    assert!(s.mapped_key(&g, "d", None, CTRL_ALT));
    assert_eq!(song_end(&s), Some(end - 2 * BAR));
}

#[test]
fn the_loop_menu_has_the_loop_rows_and_the_time_rows() {
    let (mut s, g) = studio();
    s.song
        .run("loop.set", json!({ "start": 4 * BAR, "end": 8 * BAR }))
        .unwrap();
    let x = s.tick_x(&g, 5 * BAR) + 24.0;
    assert!(s.scene_context(&g, x, g.ruler.y + 4.0));
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
    // The loop's key is the pad's slash alone, not "NumPad/ / /".
    assert_eq!(key(&s, "ループ"), "NumPad/");
    assert_eq!(key(&s, "時間を挿入"), "Ctrl+Alt+I");
    assert_eq!(key(&s, "時間を削除"), "Ctrl+Alt+D");
    // The loop is over the Verse's bars only in part: the new section needs free bars.
    assert!(!enabled(&s, "セクションにする"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "時間を挿入"));
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn a_loop_over_free_bars_becomes_a_section_with_the_first_name_not_taken() {
    let (mut s, g) = with_room();
    s.song
        .run("loop.set", json!({ "start": 12 * BAR, "end": 14 * BAR }))
        .unwrap();
    let x = s.tick_x(&g, 12 * BAR) + 24.0;
    assert!(s.scene_context(&g, x, g.ruler.y + 4.0));
    assert!(enabled(&s, "セクションにする"));
    assert!(choose(&mut s, &g, "セクションにする"));
    assert_eq!(sections(&s)[2], ("Pre-Chorus".to_string(), 13, 2));
    assert_eq!(s.arrange.section, Some(s.project().sections[2].id));
}
