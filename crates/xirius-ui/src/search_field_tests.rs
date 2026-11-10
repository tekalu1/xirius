//! The shared search field on every screen: the ✖, the two-step Esc, the keys between the
//! field and the list, Ctrl+F, the text across the tabs, the zero-match text and the names a
//! screen reader reads.

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::search_box::{self, CLEAR_NAME, NO_MATCH};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;
use xirius_design::Space;

fn browser() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.browser.open = true;
    s.browser.tab = RailTab::Effects;
    s.browser.typing = true;
    s.input_map.browser_focus = true;
    (s, g)
}

fn key(s: &mut Studio, g: &Geometry, k: &str) -> bool {
    s.browser_list_key(g, k, Mods::NONE)
}

fn stops(s: &Studio, g: &Geometry) -> Vec<usize> {
    let l = s.browser.layout(g.panel);
    l.rows
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            matches!(
                p.row,
                BRow::Group { .. } | BRow::Item { .. } | BRow::Preset { .. }
            )
        })
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn the_browser_clear_shows_only_with_text_and_keeps_the_text_room() {
    let (mut s, g) = browser();
    let l = s.browser.layout(g.panel);
    let pad = Space::X2.px();
    let c = search_box::clear_rect(l.search, pad);
    let (x, y) = (c.x + c.w / 2.0, c.y + c.h / 2.0);
    assert_eq!(s.browser.hit(&l, x, y), Some(BrowserHit::Search));
    let before = (l.search, crate::browser::search_text_rect(l.search));
    s.browser.search.set("a".into());
    let l = s.browser.layout(g.panel);
    assert_eq!(s.browser.hit(&l, x, y), Some(BrowserHit::SearchClear));
    assert_eq!(
        (l.search, crate::browser::search_text_rect(l.search)),
        before,
        "neither the field nor its text room moves when the ✖ shows"
    );
    assert_eq!(l.search.h, search_box::HEIGHT);
}

#[test]
fn pressing_the_browser_clear_empties_it_and_the_field_keeps_the_keys() {
    let (mut s, g) = browser();
    s.browser.search.set("rev".into());
    s.browser.typing = false;
    let l = s.browser.layout(g.panel);
    s.browser.press(&l, BrowserHit::SearchClear);
    assert!(s.browser.search.is_empty());
    assert!(s.browser.typing, "the field still has the keys");
    // Esc in two steps: the words, then the field.
    s.browser.search.set("rev".into());
    assert!(s.browser.typed("escape", None));
    assert!(s.browser.search.is_empty() && s.browser.typing);
    assert!(s.browser.typed("escape", None));
    assert!(!s.browser.typing);
}

#[test]
fn the_browser_down_goes_to_the_first_row_and_up_on_it_comes_back() {
    let (mut s, g) = browser();
    let rows = stops(&s, &g);
    assert!(rows.len() > 3);
    assert!(key(&mut s, &g, "down"));
    assert!(!s.browser.typing);
    assert_eq!(
        s.browser.key_row,
        Some(rows[0]),
        "the first row has the ring"
    );
    assert!(key(&mut s, &g, "down"));
    assert_eq!(s.browser.key_row, Some(rows[1]));
    assert!(key(&mut s, &g, "end"));
    assert_eq!(s.browser.key_row, rows.last().copied());
    assert!(key(&mut s, &g, "home"));
    assert_eq!(s.browser.key_row, Some(rows[0]));
    assert!(key(&mut s, &g, "up"));
    assert!(s.browser.typing, "up on the first row returns to the field");
    assert_eq!(s.browser.key_row, None);
}

#[test]
fn the_browser_enter_picks_the_first_item_and_the_ring_goes_with_the_mouse() {
    let (mut s, g) = browser();
    s.browser.search.set("a".into());
    assert!(key(&mut s, &g, "enter"));
    assert!(!s.browser.typing);
    let l = s.browser.layout(g.panel);
    let at = s.browser.key_row.expect("a row has the keys");
    assert!(matches!(l.rows[at].row, BRow::Item { selected: true, .. }));
    // A press with the mouse takes the ring away.
    s.browser.press(&l, BrowserHit::Group(0));
    assert_eq!(s.browser.key_row, None);
}

#[test]
fn the_browser_right_opens_and_left_closes_or_goes_to_the_parent() {
    let (mut s, g) = browser();
    s.browser.search.clear();
    assert!(key(&mut s, &g, "down"));
    let l = s.browser.layout(g.panel);
    let first = s.browser.key_row.unwrap();
    let BRow::Group { open, .. } = l.rows[first].row else {
        // The list starts on an item: nothing to fold.
        return;
    };
    assert!(open);
    assert!(key(&mut s, &g, "left"));
    let l = s.browser.layout(g.panel);
    assert!(matches!(l.rows[first].row, BRow::Group { open: false, .. }));
    assert!(key(&mut s, &g, "right"));
    let l = s.browser.layout(g.panel);
    assert!(matches!(l.rows[first].row, BRow::Group { open: true, .. }));
    assert!(key(&mut s, &g, "down"));
    assert!(key(&mut s, &g, "left"));
    assert_eq!(s.browser.key_row, Some(first), "← goes to the parent");
}

#[test]
fn ctrl_f_takes_the_keys_into_the_browser_search() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.browser.open = false;
    let ctrl = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    assert!(s.mapped_key(&g, "f", Some("f"), ctrl));
    assert!(s.browser.open && s.browser.typing);
    assert!(s.input_map.browser_focus);
}

#[test]
fn the_search_text_stays_across_the_browser_tabs() {
    let (mut s, g) = browser();
    s.browser.search.set("kick".into());
    s.browser.press_rail(RailTab::Files);
    assert_eq!(s.browser.search.value, "kick");
    s.browser.press_rail(RailTab::Effects);
    assert_eq!(s.browser.search.value, "kick");
    let l = s.browser.layout(g.panel);
    s.browser.press(&l, BrowserHit::Loc(0));
    assert_eq!(s.browser.search.value, "kick", "the place tabs keep it too");
}

#[test]
fn nothing_found_says_one_phrase_and_the_match_is_found_without_case() {
    let (mut s, g) = browser();
    s.browser.search.set("qqqqqq".into());
    let l = s.browser.layout(g.panel);
    assert!(
        l.rows
            .iter()
            .any(|p| matches!(p.row, BRow::Note(t) if t == NO_MATCH))
    );
    assert_eq!(NO_MATCH, "見つかりません");
    assert_eq!(search_box::count_info(0), NO_MATCH);
    assert_eq!(search_box::matched("Reverb", "ver"), Some((2, 5)));
    assert_eq!(search_box::matched("Reverb", "zz"), None);
}

#[test]
fn nothing_found_is_said_in_the_track_browser_list_too() {
    let (mut s, g) = browser();
    s.browser.track_library = Some(Vec::new());
    s.browser.search.set("qqqqqq".into());
    let l = s.browser.layout(g.panel);
    assert!(
        l.rows
            .iter()
            .any(|p| matches!(p.row, BRow::Note(t) if t == NO_MATCH))
    );
}

#[test]
fn a_screen_reader_reads_the_browser_search_its_count_and_the_clear() {
    let (mut s, g) = browser();
    let marks = crate::a11y::marks(&s, &g);
    assert!(marks.iter().any(|m| m.key == "browser-search"));
    assert!(
        !marks.iter().any(|m| m.key == "browser-search-clear"),
        "no ✖ without text"
    );
    s.browser.search.set("a".into());
    let marks = crate::a11y::marks(&s, &g);
    let clear = marks
        .iter()
        .find(|m| m.key == "browser-search-clear")
        .expect("the ✖");
    assert_eq!(clear.node.label(), Some(CLEAR_NAME));
    let field = marks.iter().find(|m| m.key == "browser-search").unwrap();
    assert!(field.node.description().is_some_and(|d| d.contains('件')));
    s.browser.search.set("qqqqqq".into());
    let marks = crate::a11y::marks(&s, &g);
    let field = marks.iter().find(|m| m.key == "browser-search").unwrap();
    assert_eq!(field.node.description(), Some(NO_MATCH));
}

// ---------------------------------------------------------------- palette

#[test]
fn the_palette_clear_empties_the_field_and_the_text_room_does_not_move() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.open_palette(&g);
    let search = s.palette.as_ref().unwrap().layout(g.size).search;
    assert_eq!(search.h, 48.0);
    let room = crate::palette::field_of(search);
    s.ime_replace(None, "mute", false);
    assert_eq!(crate::palette::field_of(search), room);
    let c = search_box::clear_rect(search, Space::X4.px());
    assert!(!s.palette_press(&g, c.x + c.w / 2.0, c.y + c.h / 2.0));
    assert!(s.palette.is_some());
    assert_eq!(s.ime_field().map(|f| f.value.clone()), Some(String::new()));
}

// ---------------------------------------------------------------- a pick list

fn insert_list() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = crate::studio::DrawerTab::Mixer;
    s.drawer_size.mixer = Some(600.0);
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    let vocal = crate::arrange::probe::track(&s, "Vocal");
    assert!(s.mixer_press(&g, crate::mixer::MixerHit::AddInsert(vocal)));
    (s, g)
}

#[test]
fn the_list_clear_shows_with_text_empties_it_and_the_list_stays() {
    let (mut s, g) = insert_list();
    let cell = |s: &Studio| {
        s.popup
            .as_ref()
            .unwrap()
            .layout(g.size)
            .cells
            .into_iter()
            .find(|c| c.is_search())
            .expect("the search cell")
    };
    let before = cell(&s);
    assert!(before.clear_rect().is_none());
    assert!(s.fx_typed("r", Some("r")));
    let after = cell(&s);
    assert_eq!(after.rect, before.rect);
    assert_eq!(after.text_room(), before.text_room(), "no movement");
    let c = after.clear_rect().expect("the ✖");
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert!(layout.clear_at(c.x + c.w / 2.0, c.y + c.h / 2.0));
    assert!(s.press(&g, Hit::SearchClear));
    assert!(s.popup.is_some(), "the list stays");
    assert!(
        s.popup
            .as_ref()
            .unwrap()
            .pick
            .as_ref()
            .unwrap()
            .field
            .is_empty()
    );
    assert!(cell(&s).clear_rect().is_none());
    // The matched part of a row is bold: the cell keeps the words it was built for.
    assert!(s.fx_typed("s", Some("s")));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    assert!(layout.cells.iter().any(|c| c.query == "s"));
}

#[test]
fn the_list_down_up_and_zero_matches() {
    let (mut s, _g) = insert_list();
    assert!(s.fx_typed("down", None));
    assert!(s.fx_typed("up", None));
    assert!(s.fx_typed("q", Some("qqqqqq")));
    let list = s.popup.as_ref().unwrap().pick.as_ref().unwrap();
    assert!(list.visible().is_empty());
    assert_eq!(list.empty, NO_MATCH);
}

// ---------------------------------------------------------------- About

#[test]
fn the_about_search_clears_before_it_closes() {
    let mut s = Studio::sample();
    let g = s.geometry(1600.0, 900.0);
    assert!(s.press(&g, Hit::Bar(crate::topbar::BarButton::Logo)));
    let head = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .iter()
        .position(|c| {
            c.action
                == Some(crate::popup::Action::About(crate::about::Act::Toggle(
                    crate::about::Item::Rust,
                )))
        })
        .unwrap();
    assert!(s.press(&g, Hit::Item(head)));
    for ch in ["s", "e"] {
        assert!(s.typed(&g, ch, Some(ch)));
    }
    let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
    assert!(
        cells
            .iter()
            .any(|c| c.is_search() && c.clear_rect().is_some()),
        "the ✖ shows with text"
    );
    assert!(s.typed(&g, "escape", None));
    assert!(s.popup.is_some(), "the first Esc empties the words");
    let cells = s.popup.as_ref().unwrap().layout(g.size).cells;
    assert!(cells.iter().all(|c| c.clear_rect().is_none()));
    let _ = s.mapped_key(&g, "escape", None, Mods::NONE);
    assert!(s.popup.is_none(), "the second closes it");
}
