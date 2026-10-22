//! The roll's view switch and drum map button in the drawer's bar (EDT-006, EDT-058), pressed
//! as the window presses them.

use super::roll_tests::roll;
use super::tests::{choose, enabled, labels, steps};
use crate::studio::{DrawerTab, Hit, Studio};

fn shown(s: &Studio, g: &crate::studio::Geometry) -> Option<String> {
    s.roll_ctx(g).unwrap().drum
}

fn press_at(s: &mut Studio, g: &crate::studio::Geometry, hit: Hit) {
    let (x, y) = s.center_of(g, hit).unwrap();
    assert_eq!(s.hit(g, x, y), hit);
    assert!(s.press(g, hit));
}

#[test]
fn the_switch_shows_the_part_as_drums_and_back_as_keys_outside_the_history() {
    let (mut s, g) = roll();
    assert_eq!(s.tab, DrawerTab::Roll);
    assert_eq!(shown(&s, &g), None);
    let before = steps(&s);
    press_at(&mut s, &g, Hit::RollView(true));
    assert_eq!(shown(&s, &g).as_deref(), Some("gm"));
    assert_eq!(steps(&s), before);
    press_at(&mut s, &g, Hit::RollView(false));
    assert_eq!(shown(&s, &g), None);
}

#[test]
fn the_map_button_is_there_only_in_the_drum_view_and_lists_the_maps() {
    let (mut s, g) = roll();
    let map = g.roll_map();
    assert_ne!(s.hit(&g, map.x + 4.0, map.y + 4.0), Hit::RollMap);
    press_at(&mut s, &g, Hit::RollView(true));
    press_at(&mut s, &g, Hit::RollMap);
    assert_eq!(
        labels(&s),
        [
            "GM ドラム",
            "ドラムキット",
            "隠す",
            "行名を変更",
            "既定に戻す"
        ]
    );
    assert!(!enabled(&s, "既定に戻す"));
    let lit: Vec<String> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(lit, ["GM ドラム"]);
    assert!(choose(&mut s, &g, "ドラムキット"));
    assert_eq!(shown(&s, &g).as_deref(), Some("kit"));
}

#[test]
fn the_switch_is_hidden_with_the_drawer_shut_or_on_the_mixer() {
    let (mut s, g) = roll();
    s.tab = DrawerTab::Mixer;
    assert!(!s.roll_switch_shown());
    s.tab = DrawerTab::Roll;
    s.drawer_open = false;
    assert!(!s.roll_switch_shown());
    let _ = g;
}

/// The roll in the drum view (GM): the clip's notes 64, 62 and 60 on top, キック (36) last.
fn drums() -> (Studio, crate::studio::Geometry) {
    let (mut s, g) = roll();
    press_at(&mut s, &g, Hit::RollView(true));
    (s, g)
}

fn rows(s: &Studio, g: &crate::studio::Geometry) -> Vec<u8> {
    s.roll_ctx(g).unwrap().rows().pitches().collect()
}

fn name(s: &Studio, g: &crate::studio::Geometry, pitch: u8) -> String {
    s.roll_ctx(g).unwrap().drum_name(pitch)
}

/// Opens the map button's menu and the rows under `sub` in it.
fn map_sub(s: &mut Studio, g: &crate::studio::Geometry, sub: &str) {
    press_at(s, g, Hit::RollMap);
    assert!(choose(s, g, sub), "{sub}: {:?}", labels(s));
}

/// Right-clicks the name of the row of `pitch`, scrolled into the grid.
fn name_menu(s: &mut Studio, g: &crate::studio::Geometry, pitch: u8) {
    let c = s.roll_ctx(g).unwrap();
    s.roll.scroll_to_pitch(&c, pitch);
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    assert!(s.scene_context(g, f.keys.x + f.keys.w / 2.0, v.y(pitch) + v.row_h / 2.0));
}

#[test]
fn a_row_without_notes_is_hidden_from_the_map_menu_and_shown_back_outside_the_history() {
    let (mut s, g) = drums();
    let before = steps(&s);
    let all = rows(&s, &g);
    assert_eq!(all[..3], [64, 62, 60]);
    assert_eq!(all.last(), Some(&36));

    map_sub(&mut s, &g, "隠す");
    // A row with notes shows anyway, so it cannot be hidden.
    assert!(!enabled(&s, &name(&s, &g, 60)));
    assert!(enabled(&s, "キック"));
    assert!(choose(&mut s, &g, "キック"));
    assert!(!rows(&s, &g).contains(&36));
    assert_eq!(rows(&s, &g).len(), all.len() - 1);
    assert_eq!(steps(&s), before);

    map_sub(&mut s, &g, "隠す");
    let on: Vec<String> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(on, ["キック"]);
    assert!(choose(&mut s, &g, "キック"));
    assert_eq!(rows(&s, &g), all);
}

#[test]
fn a_row_moves_up_and_down_from_its_own_menu_and_the_grid_follows() {
    let (mut s, g) = drums();
    let all = rows(&s, &g);
    let last = all.len() - 1;

    name_menu(&mut s, &g, 36);
    let c = s.roll_ctx(&g).unwrap();
    let low = s.roll.view(&c).y(36);
    // The last row cannot go further down.
    assert!(!enabled(&s, "下へ移動"));
    assert!(choose(&mut s, &g, "上へ移動"));
    let moved = rows(&s, &g);
    assert_eq!(moved[last - 1], 36);
    assert_eq!(moved[last], all[last - 1]);
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    assert!(v.y(36) < low);
    assert_eq!(v.y(all[last - 1]), low);

    // The top row cannot go further up.
    name_menu(&mut s, &g, 64);
    assert!(!enabled(&s, "上へ移動"));
    assert!(enabled(&s, "下へ移動"));
    s.popup = None;

    name_menu(&mut s, &g, 36);
    assert!(enabled(&s, "上へ移動"));
    assert!(choose(&mut s, &g, "下へ移動"));
    assert_eq!(rows(&s, &g), all);
}

#[test]
fn a_row_is_renamed_from_the_map_menu_and_the_reset_brings_the_map_back() {
    let (mut s, g) = drums();
    let all = rows(&s, &g);
    map_sub(&mut s, &g, "行名を変更");
    assert!(choose(&mut s, &g, "キック"));
    for _ in 0..10 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "808", false);
    s.ask_key("enter");
    assert_eq!(name(&s, &g, 36), "808");

    name_menu(&mut s, &g, 36);
    assert!(choose(&mut s, &g, "上へ移動"));
    assert_ne!(rows(&s, &g), all);

    press_at(&mut s, &g, Hit::RollMap);
    assert!(enabled(&s, "既定に戻す"));
    assert!(choose(&mut s, &g, "既定に戻す"));
    assert_eq!(rows(&s, &g), all);
    assert_eq!(name(&s, &g, 36), "キック");
}

#[test]
fn the_name_of_a_drum_row_has_its_name_hiding_and_place_in_its_menu() {
    let (mut s, g) = drums();
    let all = rows(&s, &g);
    name_menu(&mut s, &g, 36);
    assert_eq!(
        labels(&s),
        [
            "同じ高さのノートを選択",
            "鍵盤の表示",
            "名前を変更",
            "隠す",
            "上へ移動",
            "下へ移動"
        ]
    );
    assert!(!enabled(&s, "同じ高さのノートを選択"));
    assert!(!enabled(&s, "下へ移動"));
    assert!(choose(&mut s, &g, "上へ移動"));
    assert_eq!(rows(&s, &g)[all.len() - 2], 36);

    name_menu(&mut s, &g, 36);
    assert!(choose(&mut s, &g, "名前を変更"));
    for _ in 0..10 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "Kick", false);
    s.ask_key("enter");
    assert_eq!(name(&s, &g, 36), "Kick");

    name_menu(&mut s, &g, 36);
    assert!(choose(&mut s, &g, "隠す"));
    assert!(!rows(&s, &g).contains(&36));

    // A row with notes keeps its place in the grid.
    name_menu(&mut s, &g, 60);
    assert!(enabled(&s, "同じ高さのノートを選択"));
    assert!(!enabled(&s, "隠す"));
}

#[test]
fn the_keyboard_keys_have_no_drum_rows_in_their_menu() {
    let (mut s, g) = roll();
    name_menu(&mut s, &g, 60);
    assert_eq!(labels(&s), ["同じ高さのノートを選択", "鍵盤の表示"]);
}

#[test]
fn the_keyboard_menu_switches_the_view_between_note_names_and_drum_names() {
    let (mut s, g) = roll();
    let before = steps(&s);
    name_menu(&mut s, &g, 60);
    assert!(choose(&mut s, &g, "鍵盤の表示"));
    assert_eq!(labels(&s), ["戻る", "音名", "ドラムの名前"]);
    let lit: Vec<String> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(lit, ["音名"]);
    assert!(choose(&mut s, &g, "ドラムの名前"));
    assert_eq!(shown(&s, &g).as_deref(), Some("gm"));

    name_menu(&mut s, &g, 36);
    assert!(choose(&mut s, &g, "鍵盤の表示"));
    let lit: Vec<String> = s
        .context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect();
    assert_eq!(lit, ["ドラムの名前"]);
    assert!(choose(&mut s, &g, "音名"));
    assert_eq!(shown(&s, &g), None);
    assert_eq!(steps(&s), before, "a view, outside the history");
}
