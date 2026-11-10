//! One field of text for every screen: what the keys and the IME do in each place.

use crate::interaction::TextKey;
use crate::popup::{Popup, PopupKind};
use crate::settings::FieldId;
use crate::settings::field::Field;
use crate::studio::{Rect, Studio};
use crate::tools::Mods;
use crate::track_name::Name;

const CTRL: Mods = Mods {
    ctrl: true,
    shift: false,
    alt: false,
};
const SHIFT: Mods = Mods {
    ctrl: false,
    shift: true,
    alt: false,
};

fn palette() -> (Studio, crate::studio::Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.open_palette(&g);
    (s, g)
}

fn value(s: &Studio) -> String {
    s.ime_field().map(|f| f.value.clone()).unwrap_or_default()
}

#[test]
fn the_palette_takes_select_all_copy_undo_and_paste() {
    let (mut s, _g) = palette();
    s.ime_replace(None, "mute", false);
    assert_eq!(s.text_key("a", CTRL), TextKey::Used);
    assert_eq!(s.text_key("c", CTRL), TextKey::Copy("mute".into()));
    s.ime_replace(None, "x", false);
    assert_eq!(value(&s), "x", "typing over a selection replaces it");
    assert_eq!(s.text_key("z", CTRL), TextKey::Used);
    assert_eq!(value(&s), "mute");
    assert_eq!(s.text_key("y", CTRL), TextKey::Used);
    assert_eq!(value(&s), "x");
    assert_eq!(s.text_key("v", CTRL), TextKey::Paste);
    s.text_paste("a\r\nb ");
    assert_eq!(value(&s), "xa b");
    assert_eq!(s.text_key("x", CTRL), TextKey::Used, "nothing selected");
    assert_eq!(s.text_key("left", SHIFT), TextKey::Used);
    assert_eq!(s.text_key("x", CTRL), TextKey::Copy("b".into()));
    assert_eq!(value(&s), "xa ");
}

#[test]
fn the_palette_escape_clears_before_it_closes() {
    let (mut s, g) = palette();
    s.ime_replace(None, "mute", false);
    assert!(s.palette_key(&g, "escape", Mods::NONE));
    assert!(s.palette.is_some());
    assert_eq!(value(&s), "");
    assert!(s.palette_key(&g, "escape", Mods::NONE));
    assert!(s.palette.is_none());
}

#[test]
fn the_palette_caret_moves_by_cluster_and_word() {
    let (mut s, g) = palette();
    s.ime_replace(None, "ab 🇯🇵cd", false);
    assert!(s.palette_key(&g, "left", Mods::NONE));
    assert!(s.palette_key(&g, "left", Mods::NONE));
    assert!(s.palette_key(&g, "left", Mods::NONE));
    assert_eq!(s.ime_field().unwrap().caret(), "ab ".len());
    assert_eq!(s.text_key("left", CTRL), TextKey::Used);
    assert_eq!(s.ime_field().unwrap().caret(), 0);
}

#[test]
fn the_ime_ranges_are_utf16_and_never_split_a_cluster() {
    let (mut s, _g) = palette();
    s.ime_replace(None, "a𝄞é", false);
    let f = s.ime_field().unwrap();
    assert_eq!(f.selected_range(), 4..4);
    assert_eq!(f.text_for_range(1..3), ("𝄞".to_string(), 1..3));
    // A range that ends inside the pair is widened to the whole of it.
    assert_eq!(s.ime_field().unwrap().text_for_range(1..2).0, "𝄞");
    s.ime_replace(Some(1..3), "ー", false);
    assert_eq!(value(&s), "aーé");
    s.ime_replace(None, "ぜんかく", true);
    let f = s.ime_field().unwrap();
    assert_eq!(f.marked_range(), Some(2..6));
}

fn browser() -> (Studio, crate::studio::Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.browser.open = true;
    s.browser.typing = true;
    (s, g)
}

#[test]
fn the_browser_search_takes_the_ime_the_editing_keys_and_two_escapes() {
    let (mut s, g) = browser();
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Browser));
    s.ime_replace(None, "ドラム", true);
    assert_eq!(s.ime_field().unwrap().marked_range(), Some(0..3));
    s.ime_replace(None, "ドラム", false);
    assert_eq!(s.browser.search.value, "ドラム");
    assert!(s.typed(&g, "left", None));
    assert_eq!(s.browser.search.caret(), "ドラ".len());
    assert!(s.typed(&g, "backspace", None));
    assert_eq!(s.browser.search.value, "ドム");
    assert_eq!(s.text_key("a", CTRL), TextKey::Used);
    s.ime_replace(None, "q", false);
    assert_eq!(s.browser.search.value, "q");
    assert!(s.typed(&g, "escape", None));
    assert!(s.browser.search.is_empty() && s.browser.typing);
    assert!(s.typed(&g, "escape", None));
    assert!(!s.browser.typing);
    assert_eq!(
        s.text_key("a", CTRL),
        TextKey::Pass,
        "no field has the keys"
    );
}

#[test]
fn a_rename_starts_with_everything_selected_and_typing_replaces_it() {
    let mut s = Studio::sample();
    let track = *s.project().tracks.keys().next().unwrap();
    let name = s.project().tracks[&track].name.clone();
    s.input_map.name = Some(Name {
        track,
        field: Field::selected(FieldId::Search, name.clone()),
    });
    assert_eq!(s.ime_field().unwrap().selected_text(), name);
    s.ime_replace(None, "新しい名前", false);
    assert_eq!(s.input_map.name.as_ref().unwrap().field.value, "新しい名前");
    assert_eq!(s.text_key("z", CTRL), TextKey::Used);
    assert_eq!(s.input_map.name.as_ref().unwrap().field.value, name);
}

#[test]
fn a_version_name_takes_the_ime() {
    let mut s = Studio::sample();
    let mut p = Popup::new(PopupKind::Versions, Rect::new(0.0, 0.0, 10.0, 10.0));
    p.naming = Some(Field::new(FieldId::Search, String::new()));
    s.popup = Some(p);
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Naming));
    s.ime_replace(None, "初稿", true);
    assert_eq!(s.ime_field().unwrap().marked_range(), Some(0..2));
    s.ime_replace(None, "初稿", false);
    assert_eq!(
        s.popup.as_ref().unwrap().naming.as_ref().unwrap().value,
        "初稿"
    );
}
