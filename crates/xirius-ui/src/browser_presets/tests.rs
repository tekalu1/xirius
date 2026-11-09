//! BRW-017: the presets under the built-in devices in the browser, pressed as the window
//! presses them (no OS input) on the sample song, its preset folder the tests' own.

use std::sync::Arc;

use serde_json::json;
use xirius_plugins::Catalog;

use super::*;
use crate::instrument::Pick;
use crate::instrument::menu::{Menu, SHOW};
use crate::studio::Hit;

#[test]
fn late_index_and_favourite_replies_never_replace_newer_preferences() {
    for reply_first in [false, true] {
        for command in ["browser.preset.list", "browser.preset.favourite"] {
            let (mut s, _) = studio();
            let root = tempfile::tempdir().unwrap();
            let file = root.path().join("browser.json");
            s.browser.use_prefs_fixture(file.clone()).unwrap();
            let reply = json!({"entries":[], "favourites":["device:sampler", "preset:old"]});
            if reply_first {
                s.track_browser_answer(command, &reply);
            }
            s.browser.toggle_favourite("Sampler");
            s.browser.toggle_favourite("preset:new");
            if !reply_first {
                s.track_browser_answer(command, &reply);
            }
            s.browser.settle_prefs_io();
            assert!(!s.browser.is_favourite("Sampler"));
            assert!(s.browser.is_favourite("preset:new"));
            assert!(!s.browser.is_favourite("preset:old"));
            let saved = xirius_session::places::load(&file).unwrap();
            assert!(saved.favourites.contains("preset:new"));
            assert!(!saved.favourites.contains("device:sampler"));
        }
    }
}

#[test]
fn indexed_rows_and_drag_use_device_and_subtree_icons() {
    use xirius_core::track_browser::{Category, Entry};
    let (mut s, g) = studio();
    for (category, device, device_name, icon) in [
        (Category::Instrument, uid("Sampler"), "Sampler", "keys"),
        (Category::Instrument, uid("Drum Kit"), "Drum Kit", "drum"),
        (Category::Fx, uid("EQ"), "EQ", "effects"),
        (Category::Fx, uid("Reverb"), "Reverb", "effects"),
        (
            Category::Instrument,
            "missing-plugin",
            "Absent Synth",
            "keys",
        ),
        (Category::MultiInstrument, "", "", "layers"),
        (Category::FxChain, "", "", "cable"),
        (Category::Track, "", "", "audio"),
    ] {
        let mut entry = Entry {
            id: "preset:icon-test".into(),
            name: "Unrelated preset name".into(),
            category,
            device: device.into(),
            device_name: device_name.into(),
            vendor: "Xirius".into(),
            source: "User".into(),
            folder: vec![],
            file: Some("icon-test.xpreset".into()),
            has_fx: false,
            fx_names: vec![],
        };
        assert_eq!(s.browser.entry_icon(&entry), icon);
        entry.has_fx = true;
        assert_eq!(
            s.browser.entry_icon(&entry),
            icon,
            "+FX does not change the device"
        );
        s.browser.tab = if category.tab() == "effects" {
            RailTab::Effects
        } else {
            RailTab::Instruments
        };
        s.browser.track_kind = category == Category::Track;
        s.browser.track_library = Some(vec![entry.clone()]);
        s.browser.favourites.insert(entry.id.clone());
        let layout = s.browser.layout(g.panel);
        let index = layout
            .rows
            .iter()
            .position(|r| matches!(r.row, BRow::Preset { .. }))
            .unwrap();
        assert!(s.preset_grab(&g, index, 0.0, 0.0, 1));
        assert_eq!(s.browser.drag.as_ref().unwrap().icon, icon);
        if matches!(category, Category::Instrument | Category::Fx) {
            entry.file = None;
            entry.name = device_name.into();
            s.browser.track_library = Some(vec![entry]);
            assert!(
                s.browser
                    .rows()
                    .iter()
                    .any(|r| matches!(r, BRow::Item { item, .. } if item.icon == icon))
            );
        }
    }
    assert_eq!(
        crate::browser::TRACK_BROWSER_ICONS,
        ["keys", "effects", "audio"]
    );
    assert_eq!(RailTab::Instruments.icon(), "arrange");
}

#[test]
fn track_browser_kind_buttons_keep_query_and_have_one_rail_entry() {
    let (mut s, g) = studio();
    assert_eq!(
        RailTab::ALL
            .iter()
            .filter(|&&t| matches!(t, RailTab::Instruments | RailTab::Effects))
            .count(),
        1
    );
    s.browser.search = "warm user".into();
    let l = s.browser.layout(g.panel);
    assert_eq!(l.tabs.len(), 3);
    assert!(l.view.is_none());
    for i in [0, 1, 2] {
        s.press(&g, Hit::Browser(BrowserHit::Loc(i)));
        assert_eq!(s.browser.search, "warm user");
        assert_eq!(s.browser.track_kind, i == 2);
    }
    assert!(!s.browser.typing, "the kind buttons leave text entry");
    assert!(s.browser.typed("escape", None));
    assert!(s.browser.search.is_empty());
}

#[test]
fn preset_save_dialog_starts_without_fx_and_requires_a_second_overwrite_press() {
    use xirius_core::track_browser::{Category, Entry};
    let (mut s, _) = studio();
    s.browser.track_library = Some(vec![Entry {
        id: "preset:test".into(),
        name: "Warm".into(),
        category: Category::Instrument,
        device: "sampler".into(),
        device_name: "Sampler".into(),
        vendor: "Xirius".into(),
        source: "User".into(),
        folder: Vec::new(),
        file: Some("test.xpreset".into()),
        has_fx: false,
        fx_names: Vec::new(),
    }]);
    s.open_track_preset_save(
        "browser.preset.save",
        json!({"kind":"instrument"}),
        "Warm",
        "Sampler",
        false,
        "EQ · Reverb".into(),
    );
    assert!(
        !s.ctx
            .as_ref()
            .unwrap()
            .ask
            .as_ref()
            .unwrap()
            .preset
            .as_ref()
            .unwrap()
            .include_fx
    );
    s.preset_save_pick(1);
    assert!(
        s.ctx
            .as_ref()
            .unwrap()
            .ask
            .as_ref()
            .unwrap()
            .preset
            .as_ref()
            .unwrap()
            .include_fx
    );
    s.preset_save_pick(3);
    assert!(
        s.ctx
            .as_ref()
            .unwrap()
            .ask
            .as_ref()
            .unwrap()
            .preset
            .as_ref()
            .unwrap()
            .overwrite
    );
    s.preset_save_pick(4);
    assert!(s.popup.is_none());
    assert!(crate::io_command::handles("browser.preset.save"));
    assert!(crate::io_command::handles("browser.preset.list"));
}

#[test]
fn reading_the_track_index_preserves_the_screens_error_and_history() {
    let (mut s, _) = studio();
    s.error = Some("音声の入力を確認してください".into());
    let before = s.song().steps();
    assert!(s.run_command("browser.preset.list", json!({})));
    assert_eq!(s.error.as_deref(), Some("音声の入力を確認してください"));
    assert_eq!(s.song().steps(), before);
    assert!(
        s.browser
            .track_library
            .as_ref()
            .is_some_and(|es| !es.is_empty())
    );
}

#[test]
fn the_owner_reads_the_index_without_closing_the_save_dialog_or_clearing_errors() {
    let (mut s, _) = studio();
    let keys = track(&s, "Keys");
    assert!(s.run_command(
        "browser.preset.save",
        json!({
            "track": keys.to_string(), "kind":"track", "name":"Folder test", "folder":"Keys/Soft"
        })
    ));
    s.open_track_preset_save(
        "browser.preset.save",
        json!({"kind":"instrument"}),
        "Warm",
        "Sampler",
        false,
        String::new(),
    );
    s.error = Some("入力を確認してください".into());
    let before = s.song().steps();
    s.bind_window_io();
    assert!(s.run_command("browser.preset.list", json!({})));
    assert!(s.context_open());
    assert_eq!(s.error.as_deref(), Some("入力を確認してください"));
    s.wait_for_save();
    assert!(s.context_open());
    assert_eq!(s.ask_field().map(|f| f.text()), Some("Warm".into()));
    assert_eq!(s.error.as_deref(), Some("入力を確認してください"));
    assert_eq!(s.song().steps(), before);
    assert_eq!(
        s.ctx
            .as_ref()
            .unwrap()
            .ask
            .as_ref()
            .unwrap()
            .preset
            .as_ref()
            .unwrap()
            .folders,
        ["Keys", "Keys/Soft"]
    );
    assert!(
        s.browser
            .track_library
            .as_ref()
            .is_some_and(|es| !es.is_empty())
    );
}

#[test]
fn indexed_device_disclosure_and_star_keep_their_rows_during_a_scan() {
    let (mut s, g) = studio();
    assert!(s.run_command("browser.preset.list", json!({})));
    for scanning in [false, true] {
        s.browser.scanning = scanning;
        let (i, open) = device(&s, &g, "Compressor");
        assert_eq!(open, Some(true));
        let layout = s.browser.layout(g.panel);
        s.browser.scroll = layout.rows[i].y;
        let layout = s.browser.layout(g.panel);
        let hit = BrowserHit::Disclose(i);
        let at = s.browser.center_of(&layout, hit).unwrap();
        assert_eq!(s.browser.hit(&layout, at.0, at.1), Some(hit));
        s.press(&g, Hit::Browser(hit));
        assert_eq!(device(&s, &g, "Compressor").1, Some(false));
        s.press(&g, Hit::Browser(hit));
        let fav = s.browser.favourites.contains("device:compressor");
        s.press(&g, Hit::Browser(BrowserHit::Fav(i)));
        assert_eq!(s.browser.favourites.contains("device:compressor"), !fav);
        let (i, _) = device(&s, &g, "Compressor");
        s.press(&g, Hit::Browser(BrowserHit::Fav(i)));
        assert_eq!(s.browser.favourites.contains("device:compressor"), fav);
    }
}

/// The sample song with the browser on the effects and the factory presets read.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    s.browser.press_rail(RailTab::Effects);
    assert_eq!(s.browser.tab, RailTab::Effects);
    s.refresh_presets();
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    crate::arrange::probe::track(s, name)
}

fn rows(s: &Studio, g: &Geometry) -> Vec<(usize, BRow)> {
    let l = s.browser.layout(g.panel);
    l.rows.into_iter().map(|p| p.row).enumerate().collect()
}

/// The row of the device `name`.
fn device(s: &Studio, g: &Geometry, name: &str) -> (usize, Option<bool>) {
    rows(s, g)
        .into_iter()
        .find_map(|(i, r)| match r {
            BRow::Item { item, presets, .. } if item.name == name => Some((i, presets)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{name} is in the list"))
}

/// The preset rows under the device `uid`, in order.
fn presets_under(s: &Studio, g: &Geometry, uid: &str) -> Vec<String> {
    rows(s, g)
        .into_iter()
        .filter_map(|(_, r)| match r {
            BRow::Preset { device, name, .. } if device == uid => Some(name),
            _ => None,
        })
        .collect()
}

/// The row of a preset and its middle on screen.
fn preset(s: &Studio, g: &Geometry, name: &str) -> (Hit, f32, f32) {
    let i = rows(s, g)
        .into_iter()
        .find_map(|(i, r)| matches!(&r, BRow::Preset { name: n, .. } if n == name).then_some(i))
        .unwrap_or_else(|| panic!("{name} is in the list"));
    let l = s.browser.layout(g.panel);
    let (x, y) = s
        .browser
        .center_of(&l, BrowserHit::Preset(i))
        .expect("on screen");
    assert_eq!(s.browser.hit(&l, x, y), Some(BrowserHit::Preset(i)));
    (Hit::Browser(BrowserHit::Preset(i)), x, y)
}

/// A press on the device's arrow, where it is drawn.
fn disclose(s: &mut Studio, g: &Geometry, name: &str) {
    let (i, _) = device(s, g, name);
    let l = s.browser.layout(g.panel);
    let (x, y) = s
        .browser
        .center_of(&l, BrowserHit::Disclose(i))
        .expect("on screen");
    let hit = s.hit(g, x, y);
    assert_eq!(hit, Hit::Browser(BrowserHit::Disclose(i)));
    s.press(g, hit);
}

/// A press and release on a preset row (`clicks`: 2 for the double click's second press).
fn click(s: &mut Studio, g: &Geometry, name: &str, clicks: usize) {
    let (hit, x, y) = preset(s, g, name);
    s.mixer.browser_fx_focus = false;
    s.press(g, hit);
    assert!(s.lib_grab(g, hit, x, y, clicks));
    s.lib_up(g, x, y);
}

/// The middle of a track's row in the lanes.
fn lane(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let row = s.tree().row_of(RowKind::Track(t)).expect("a row");
    let y = g.column.y + row.y - s.scroll.1 + row.h / 2.0;
    (g.lanes.x + g.lanes.w / 2.0, y)
}

/// The built-in effects `uid` on the track, with the preset each keeps.
fn devices(s: &Studio, t: TrackId, uid: &str) -> Vec<Option<String>> {
    s.project().tracks[&t]
        .inserts
        .iter()
        .filter(|x| x.builtin().is_some_and(|d| d.uid == uid))
        .map(|x| x.preset.clone())
        .collect()
}

fn uid(name: &str) -> &'static str {
    xirius_params::find(name).unwrap().uid
}

/// Presses the cell of the open menu's row `label`.
fn choose(s: &mut Studio, g: &Geometry, label: &str) -> bool {
    crate::context::tests::choose(s, g, label)
}

fn menu_labels(s: &Studio) -> Vec<String> {
    crate::context::tests::all_labels(s)
}

#[test]
fn a_device_shows_its_presets_under_it_when_its_arrow_is_pressed() {
    let (mut s, g) = studio();
    let reverb = uid("Reverb");
    assert_eq!(device(&s, &g, "Reverb").1, Some(false), "closed at first");
    assert!(presets_under(&s, &g, reverb).is_empty());
    disclose(&mut s, &g, "Reverb");
    assert_eq!(device(&s, &g, "Reverb").1, Some(true));
    let mut factory = vec!["ホール", "ボーカルのプレート", "小さい部屋"];
    factory.sort_unstable();
    let mut shown = presets_under(&s, &g, reverb);
    shown.sort_unstable();
    assert_eq!(shown, factory, "the files, not Default");
    // The rows follow the device's row.
    let (i, _) = device(&s, &g, "Reverb");
    assert!(matches!(rows(&s, &g)[i + 1].1, BRow::Preset { .. }));
    // A press on the row selects the device, as before; it does not open or close it.
    let l = s.browser.layout(g.panel);
    let (x, y) = s.browser.center_of(&l, BrowserHit::Item(i)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Browser(BrowserHit::Item(i)));
    disclose(&mut s, &g, "Reverb");
    assert!(presets_under(&s, &g, reverb).is_empty());
}

#[test]
fn a_device_with_only_its_default_has_no_arrow() {
    let (mut s, g) = studio();
    s.press(&g, Hit::Browser(BrowserHit::Loc(0)));
    assert_eq!(device(&s, &g, "Sampler").1, None);
    assert_eq!(device(&s, &g, "Drum Kit").1, None);
}

#[test]
fn a_search_shows_the_presets_it_finds_under_their_devices() {
    let (mut s, g) = studio();
    s.browser.search = "ホール".into();
    let shown: Vec<String> = rows(&s, &g)
        .into_iter()
        .filter_map(|(_, r)| match r {
            BRow::Item { item, .. } => Some(item.name.to_string()),
            BRow::Preset { name, .. } => Some(format!("  {name}")),
            _ => None,
        })
        .collect();
    assert_eq!(shown, ["Reverb", "  ホール"]);
    // The case of the letters does not count, as in the list menus.
    s.browser.search = "REV".into();
    assert_eq!(device(&s, &g, "Reverb").1, Some(false), "found by its name");
    s.browser.search.clear();
    assert_eq!(
        device(&s, &g, "Reverb").1,
        Some(false),
        "not opened by the search"
    );
}

#[test]
fn a_preset_is_a_favourite_of_its_own() {
    let (mut s, g) = studio();
    disclose(&mut s, &g, "Delay");
    let (Hit::Browser(BrowserHit::Preset(i)), _, _) = preset(&s, &g, "スラップバック")
    else {
        unreachable!()
    };
    let l = s.browser.layout(g.panel);
    let r = s.browser.row_rect(&l, &l.rows[i]);
    let star = s.browser.fav_rect(r);
    let hit = s.hit(&g, star.x + star.w / 2.0, star.y + star.h / 2.0);
    assert_eq!(hit, Hit::Browser(BrowserHit::Fav(i)));
    s.press(&g, hit);
    let key = crate::browser::preset_key(uid("Delay"), "スラップバック");
    assert!(s.browser.favourites.contains(&key));
    assert!(!s.browser.favourites.contains("Delay"));
    // お気に入り lists the device with it.
    s.browser.set_view(crate::browser::View::Favourites);
    assert!(rows(&s, &g).iter().any(
        |(_, r)| matches!(r, BRow::Preset { name, fav: true, .. } if name == "スラップバック")
    ));
}

#[test]
fn enter_loads_the_chosen_effect_preset_on_the_chosen_track() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    s.select_row(Some(RowKind::Track(keys)));
    disclose(&mut s, &g, "Reverb");
    click(&mut s, &g, "ホール", 1);
    assert_eq!(
        s.browser.selected_preset,
        Some((uid("Reverb"), "ホール".to_string()))
    );
    assert_eq!(s.browser.selected_item, None);
    assert!(
        devices(&s, keys, uid("Reverb")).is_empty(),
        "a click only chooses"
    );
    let steps = s.song().steps();
    assert!(s.pick_typed(&g, "enter", None));
    assert_eq!(
        devices(&s, keys, uid("Reverb")),
        [Some("ホール".to_string())]
    );
    // The Reverb and its preset went in as one step (M-pre-4).
    assert_eq!(s.song().steps(), steps + 1);
    // Again: the track's Reverb takes it, no second one.
    click(&mut s, &g, "小さい部屋", 1);
    assert!(s.pick_typed(&g, "enter", None));
    assert_eq!(
        devices(&s, keys, uid("Reverb")),
        [Some("小さい部屋".to_string())]
    );
    assert!(s.song().steps() > steps);
}

#[test]
fn with_no_track_chosen_an_effect_preset_goes_on_the_master() {
    let (mut s, g) = studio();
    s.select_row(None);
    disclose(&mut s, &g, "Limiter");
    click(&mut s, &g, "マスターの仕上げ", 2);
    assert_eq!(
        devices(&s, TrackId::MASTER, uid("Limiter")),
        [Some("マスターの仕上げ".to_string())]
    );
}

#[test]
fn a_preset_dropped_on_a_track_loads_there() {
    let (mut s, g) = studio();
    let bass = track(&s, "Bass");
    disclose(&mut s, &g, "Compressor");
    let (hit, x, y) = preset(&s, &g, "ドラムをまとめる");
    let at = lane(&s, &g, bass);
    assert_eq!(s.preset_drop_at(&g, at.0, at.1, true), Drop::Replace(bass));
    s.press(&g, hit);
    assert!(s.lib_grab(&g, hit, x, y, 1));
    assert!(s.lib_move(at.0, at.1));
    assert!(s.lib_up(&g, at.0, at.1));
    assert_eq!(
        devices(&s, bass, uid("Compressor")),
        [Some("ドラムをまとめる".to_string())]
    );
    // One undo takes the Compressor away with its preset.
    s.undo(&g);
    assert!(devices(&s, bass, uid("Compressor")).is_empty());
    s.redo(&g);
    assert_eq!(s.selected_row, Some(RowKind::Track(bass)));
    assert!(s.browser.drag.is_none());
    // Off the tracks it does nothing.
    let (hit, x, y) = preset(&s, &g, "並列で太く");
    s.press(&g, hit);
    assert!(s.lib_grab(&g, hit, x, y, 1));
    let off = (g.panel.x + 10.0, g.panel.bottom() - 4.0);
    assert!(s.lib_move(off.0, off.1));
    assert_eq!(s.preset_drop_at(&g, off.0, off.1, true), Drop::Nothing);
    assert!(s.lib_up(&g, off.0, off.1));
    assert_eq!(
        devices(&s, bass, uid("Compressor")),
        [Some("ドラムをまとめる".to_string())]
    );
}

#[test]
fn an_instrument_preset_loads_on_the_chosen_instrument_track_or_a_new_one() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    assert!(s.set_instrument(keys, &Pick::built_in("Sampler")));
    let id = crate::instrument::current(&s.project().tracks[&keys])
        .unwrap()
        .id;
    // Its samples go into the preset with the knobs (H-pre-1).
    let files = json!([{ "file": "Piano C3.wav" }, { "file": "Piano C5.wav" }]);
    assert!(s.run_command(
        "plugin.samples.add",
        json!({ "id": id.to_string(), "files": files }),
    ));
    assert!(s.run_command(
        "plugin.param.set",
        json!({ "id": id.to_string(), "param": "file2.start", "value": 0.25 }),
    ));
    assert!(s.save_preset(id, "柔らかい鍵盤", false));
    // The track's Sampler moves on: other samples.
    assert!(s.run_command(
        "plugin.samples.add",
        json!({ "id": id.to_string(), "files": [{ "file": "Organ.wav" }] }),
    ));
    let samples = |s: &Studio, t: TrackId| {
        let x = crate::instrument::current(&s.project().tracks[&t])
            .cloned()
            .unwrap();
        (x.samples.clone(), x.params.get("file2.start").copied())
    };
    let kept = (
        vec!["Piano C3.wav".to_string(), "Piano C5.wav".to_string()],
        Some(0.25),
    );
    assert_ne!(samples(&s, keys), kept);
    s.refresh_presets();
    s.press(&g, Hit::Browser(BrowserHit::Loc(0)));
    assert_eq!(device(&s, &g, "Sampler").1, Some(false));
    disclose(&mut s, &g, "Sampler");
    assert_eq!(presets_under(&s, &g, uid("Sampler")), ["柔らかい鍵盤"]);
    // On the chosen instrument track.
    s.select_row(Some(RowKind::Track(keys)));
    let steps = s.song().steps();
    click(&mut s, &g, "柔らかい鍵盤", 2);
    let plugin = crate::instrument::current(&s.project().tracks[&keys])
        .cloned()
        .unwrap();
    assert_eq!(plugin.preset.as_deref(), Some("柔らかい鍵盤"));
    assert_eq!(samples(&s, keys), kept);
    assert_eq!(s.song().steps(), steps + 1);
    // Default is the Sampler as it comes, with no sample; one undo brings them back.
    assert!(s.load_preset(plugin.id, DEFAULT));
    assert!(samples(&s, keys).0.is_empty());
    s.undo(&g);
    assert_eq!(samples(&s, keys), kept);
    // With none chosen, on a new track.
    s.select_row(None);
    let before = s.project().tracks.len();
    click(&mut s, &g, "柔らかい鍵盤", 2);
    assert_eq!(s.project().tracks.len(), before + 1);
    let Some(RowKind::Track(t)) = s.selected_row else {
        panic!("the new track is chosen")
    };
    let plugin = crate::instrument::current(&s.project().tracks[&t])
        .cloned()
        .unwrap();
    assert_eq!(plugin.name, "Sampler");
    assert_eq!(plugin.preset.as_deref(), Some("柔らかい鍵盤"));
    assert_eq!(samples(&s, t), kept);
}

#[test]
fn show_in_browser_chooses_the_presets_row_and_brings_it_into_view() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": keys.to_string(), "name": "Reverb" }),
    ));
    let id = s.project().tracks[&keys]
        .inserts
        .iter()
        .find(|x| x.name == "Reverb")
        .unwrap()
        .id;
    assert!(s.load_preset(id, "ボーカルのプレート"));
    // The browser was elsewhere, searching, scrolled, the group folded.
    s.browser.press_rail(RailTab::Files);
    s.browser.search = "xyz".into();
    s.browser.scroll = 400.0;
    s.inst_menu_pick(Menu::Preset(keys, id), SHOW);
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Effects);
    assert!(s.browser.search.is_empty());
    assert_eq!(
        s.browser.selected_preset,
        Some((uid("Reverb"), "ボーカルのプレート".to_string()))
    );
    assert!(s.mixer.browser_fx_focus, "Enter loads it again");
    s.resized(&g);
    assert!(!s.browser.reveal);
    let l = s.browser.layout(g.panel);
    let p = l
        .rows
        .iter()
        .find(|p| matches!(&p.row, BRow::Preset { selected: true, .. }))
        .expect("the row");
    let r = s.browser.row_rect(&l, p);
    assert!(r.y >= l.list.y && r.bottom() <= l.list.bottom(), "{r:?}");
    // Default chooses the device's row.
    assert!(s.load_preset(id, crate::instrument::window::DEFAULT));
    s.inst_menu_pick(Menu::Preset(keys, id), SHOW);
    assert_eq!(s.browser.selected_item, Some("Reverb"));
    assert_eq!(s.browser.selected_preset, None);
}

#[test]
fn the_window_menus_show_in_browser_row_is_live() {
    let (mut s, _) = studio();
    let keys = track(&s, "Keys");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": keys.to_string(), "name": "EQ" }),
    ));
    let id = s.project().tracks[&keys]
        .inserts
        .iter()
        .find(|x| x.name == "EQ")
        .unwrap()
        .id;
    let (rows, _) = s.inst_menu_view(Menu::Preset(keys, id));
    let row = rows.iter().find(|r| r.label == "ブラウザーに表示").unwrap();
    assert_eq!(row.to, SHOW);
}

#[test]
fn the_factory_presets_come_back_from_the_browsers_menus() {
    let (mut s, g) = studio();
    let delay = uid("Delay");
    disclose(&mut s, &g, "Delay");
    let before = presets_under(&s, &g, delay);
    assert!(s.run_command(
        "preset.remove",
        json!({ "device": delay, "preset": "スラップバック" }),
    ));
    s.refresh_presets();
    assert!(!presets_under(&s, &g, delay).contains(&"スラップバック".to_string()));
    // A preset row's menu has it, for its device.
    let (_, x, y) = preset(&s, &g, "4 分のピンポン");
    assert!(s.lib_context(&g, x, y));
    let labels = menu_labels(&s);
    assert!(labels.contains(&"読み込む".to_string()));
    assert_eq!(
        s.context_rows()
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        ["機器", "表示"]
    );
    assert!(labels.contains(&"お気に入り".to_string()));
    assert!(choose(&mut s, &g, "工場出荷のプリセットを読み込み直す"));
    s.refresh_presets();
    assert_eq!(presets_under(&s, &g, delay), before);
    // The blank of the list has it for every device.
    let l = s.browser.layout(g.panel);
    assert!(s.lib_context(&g, l.list.x + 20.0, l.list.bottom() - 6.0));
    assert!(menu_labels(&s).contains(&"工場出荷のプリセットを読み込み直す".to_string()));
}

#[test]
fn a_preset_rows_menu_loads_it() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    s.select_row(Some(RowKind::Track(keys)));
    disclose(&mut s, &g, "EQ");
    let (_, x, y) = preset(&s, &g, "電話の声");
    assert!(s.lib_context(&g, x, y));
    assert!(choose(&mut s, &g, "読み込む"));
    assert_eq!(devices(&s, keys, uid("EQ")), [Some("電話の声".to_string())]);
}

#[test]
fn a_screen_reader_hears_the_arrow_and_the_presets() {
    let (mut s, g) = studio();
    let find = |s: &Studio, label: &str| {
        crate::a11y::marks(s, &g)
            .into_iter()
            .find(|m| m.node.label() == Some(label))
    };
    let arrow = find(&s, "Reverb のプリセット").expect("the arrow");
    assert_eq!(arrow.node.role(), gpui::accesskit::Role::Button);
    assert_eq!(arrow.node.is_expanded(), Some(false));
    disclose(&mut s, &g, "Reverb");
    let arrow = find(&s, "Reverb のプリセット").unwrap();
    assert_eq!(arrow.node.is_expanded(), Some(true));
    let row = find(&s, "Reverb のプリセット ホール").expect("the row");
    assert_eq!(row.node.role(), gpui::accesskit::Role::ListItem);
    assert_eq!(row.node.is_selected(), Some(false));
    // Its press chooses it, as the pointer's does.
    assert!(crate::a11y::press(&mut s, &g, row.press.unwrap()));
    let row = find(&s, "Reverb のプリセット ホール").unwrap();
    assert_eq!(row.node.is_selected(), Some(true));
}
