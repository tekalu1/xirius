//! One field of text for every screen: what the keys and the IME do in each place.

use crate::interaction::{ImeTarget, TextKey};
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

/// A letter as the window runs it: the key event, then the text the OS sent for
/// it — `WM_CHAR` comes through the `EntityInputHandler` (UI-338).
fn key_then_text(s: &mut Studio, g: &crate::studio::Geometry, k: &str) {
    s.song_key(
        g,
        crate::input_dispatch::KeyIn {
            key: k,
            mapped: k,
            key_char: Some(k),
            character: Some(k),
            code: None,
            mods: Mods::NONE,
            held: false,
        },
    );
    s.ime_replace(None, k, false);
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

/// One letter lands once: the key's own path puts nothing in a field the IME
/// owns, whichever of them is open — the letter the key brought is only the
/// field's, never a note or a shortcut (UI-338).
#[test]
fn a_letter_goes_into_a_field_once_whichever_field_owns_the_ime() {
    type Begin = fn(&mut Studio, &crate::studio::Geometry);
    let cases: &[(&str, Begin, ImeTarget)] = &[
        (
            "the menu's search",
            |s: &mut Studio, _| {
                s.open_context(
                    vec![crate::context::Item::new(
                        "a row",
                        crate::context::Run::Cmd("marker.add", serde_json::json!({})),
                    )],
                    0.0,
                    0.0,
                );
            },
            ImeTarget::Ask,
        ),
        (
            "a track's name",
            |s: &mut Studio, _| {
                s.input_map.name = Some(Name {
                    track: xirius_core::id::TrackId::MASTER,
                    field: Field::new(FieldId::Search, String::new()),
                });
            },
            ImeTarget::Name,
        ),
        (
            "a version's name",
            |s: &mut Studio, _| {
                let mut p = Popup::new(PopupKind::Versions, Rect::default());
                p.naming = Some(Field::new(FieldId::Search, String::new()));
                s.popup = Some(p);
            },
            ImeTarget::Naming,
        ),
        (
            "a list's search",
            |s: &mut Studio, _| {
                s.open_popup(
                    PopupKind::Insert(xirius_core::id::TrackId(1)),
                    Rect::default(),
                );
            },
            ImeTarget::Pick,
        ),
        (
            "the settings' search",
            |s: &mut Studio, _| {
                s.open_settings(crate::settings::Section::Audio);
                s.settings.as_mut().unwrap().field =
                    Some(Field::new(FieldId::Search, String::new()));
            },
            ImeTarget::Settings,
        ),
        (
            "a device's value",
            |s: &mut Studio, _| {
                s.instruments.entry = Some(crate::device::input::Entry {
                    plugin: xirius_core::id::PluginId(2),
                    k: 0,
                    field: Field::new(FieldId::Search, String::new()),
                    fresh: false,
                });
            },
            ImeTarget::Device,
        ),
        (
            "a fader's value",
            |s: &mut Studio, _| {
                let track = *s
                    .project()
                    .tracks
                    .keys()
                    .find(|t| s.tree().row_of(crate::tree::RowKind::Track(**t)).is_some())
                    .unwrap();
                s.head_fader.entry = Some(crate::head_fader::Entry {
                    track,
                    field: Field::new(FieldId::Search, String::new()),
                    fresh: false,
                });
            },
            ImeTarget::Head,
        ),
        (
            "a branch's value",
            |s: &mut Studio, _| {
                let t = s
                    .project()
                    .tracks
                    .values()
                    .find(|t| t.name == "Vocal")
                    .unwrap()
                    .id;
                assert!(s.run_command("rack.split.add", serde_json::json!({"track": t})));
                let node = match &s.project().tracks[&t].rack.as_ref().unwrap().fx[..] {
                    [.., xirius_core::rack::RackNode::Split(x)] => x.branches[0].id,
                    _ => unreachable!(),
                };
                s.open_channel(t);
                assert!(s.value_type(crate::branch_value::Focus {
                    track: t,
                    node,
                    which: crate::branch_value::Which::Gain,
                    place: crate::branch_value::Place::Editor,
                }));
            },
            ImeTarget::Branch,
        ),
        (
            "the palette",
            |s: &mut Studio, g: &crate::studio::Geometry| s.open_palette(g),
            ImeTarget::Palette,
        ),
        (
            "the browser's search",
            |s: &mut Studio, _| {
                s.browser.open = true;
                s.browser.typing = true;
            },
            ImeTarget::Browser,
        ),
    ];
    for (name, begin, target) in cases {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 860.0);
        s.resized(&g);
        begin(&mut s, &g);
        assert_eq!(s.ime_target(), Some(*target), "{name}");
        key_then_text(&mut s, &g, "k");
        assert_eq!(value(&s), "k", "{name}");
    }
}

/// A popup that takes no text covers the field behind it: opened from the
/// app's menu while the browser's box was typing, its own keys still type, and
/// the hidden field gets nothing (UI-338).
#[test]
fn a_popup_over_the_search_keeps_the_hidden_field_out_of_the_ime() {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.browser.open = true;
    s.browser.typing = true;
    assert_eq!(s.ime_target(), Some(ImeTarget::Browser));
    // The app's menu opens the About box: no click blurred the search.
    assert!(s.about_from_menu(&g));
    assert_eq!(s.ime_target(), None);
    // Its own search (the Rust list's) still types from the key.
    assert!(s.popup_action(
        &g,
        crate::popup::Action::About(crate::about::Act::Toggle(crate::about::Item::Rust))
    ));
    key_then_text(&mut s, &g, "s");
    assert_eq!(
        s.popup
            .as_ref()
            .and_then(|p| p.about.as_deref())
            .map(|v| v.query.as_str()),
        Some("s")
    );
    assert_eq!(s.browser.query(), "");
}

/// A field that is not the IME's still types at the key: the tempo's digits,
/// the inspector's, the record panel's, the meter's, a point's value box and
/// the About box's search (UI-338).
#[test]
fn a_value_field_still_takes_a_letter_or_digit_from_the_key() {
    // The tempo's typing (the top bar's).
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.bar_edit.focus_tempo();
    s.field_keys(&g, "enter", None, Mods::NONE);
    key_then_text(&mut s, &g, "5");
    assert_eq!(s.tempo_readout().0, "5");

    // The inspector's field (an audio clip's speed).
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    let (at, ac) = {
        let p = s.project();
        p.tracks
            .values()
            .find(|t| t.kind == xirius_core::model::TrackKind::Audio && !t.clips.is_empty())
            .map(|t| (t.id, t.clips[0].id))
            .expect("an audio clip")
    };
    s.selected_row = Some(crate::tree::RowKind::Track(at));
    s.selected_clip = Some((at, ac));
    s.arrange.clips = std::collections::BTreeSet::from([(at, ac)]);
    assert!(s.press(
        &g,
        crate::studio::Hit::Rail(crate::browser::RailTab::Inspector)
    ));
    let i = s
        .inspector_layout(&g)
        .rows
        .iter()
        .position(|r| r.kind == crate::context::fields::Kind::Speed)
        .expect("the speed's row");
    assert!(s.press(
        &g,
        crate::studio::Hit::Inspector(crate::inspector::Part::Field(i))
    ));
    key_then_text(&mut s, &g, "5");
    assert_eq!(s.inspector.field.as_ref().unwrap().text, "5");

    // The record panel's count-in.
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.rec_panel.open = true;
    s.rec_panel.focus = Some(crate::rec_panel::RecPart::CountBars);
    key_then_text(&mut s, &g, "4");
    assert_eq!(crate::rec_panel::RecView::of(&s).count_bars, 4);

    // The meter box's numerator.
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.open_popup(PopupKind::Meter, Rect::default());
    key_then_text(&mut s, &g, "5");
    assert_eq!(
        s.popup
            .as_ref()
            .and_then(|p| p.meter.as_ref())
            .map(|m| m.num.as_str()),
        Some("5")
    );

    // An automation point's value box.
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    s.popup = Some(Popup::new(
        PopupKind::AutoValue(crate::autovalue::AutoRef {
            track: xirius_core::id::TrackId(1),
            lane: 0,
            index: 0,
        }),
        Rect::default(),
    ));
    key_then_text(&mut s, &g, "5");
    assert_eq!(
        s.popup
            .as_ref()
            .and_then(|p| p.naming.as_ref())
            .map(|f| f.value.as_str()),
        Some("5")
    );

    // The About box's search.
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 860.0);
    s.resized(&g);
    assert!(s.press(&g, crate::studio::Hit::Bar(crate::topbar::BarButton::Logo)));
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
    assert!(s.press(&g, crate::studio::Hit::Item(head)));
    key_then_text(&mut s, &g, "s");
    assert_eq!(
        s.popup
            .as_ref()
            .and_then(|p| p.about.as_deref())
            .map(|v| v.query.as_str()),
        Some("s")
    );
}
