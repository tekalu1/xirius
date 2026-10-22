//! The menus of the browser (S38 an instrument, S39 an effect, S40 the blank of a list,
//! S41 a file, S42 a folder, S43 a tab of the files), pressed where the window presses
//! them. Called directly: no OS input, and the OS's file window is never opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use xirius_core::id::TrackId;
use xirius_core::model::{ClipBody, Project, Track, TrackKind};
use xirius_plugins::Catalog;

use super::tests::{choose, enabled, labels, steps};
use crate::browser::{BRow, BrowserHit, RailTab};
use crate::import_shots::{file_row, make_place};
use crate::instrument::current;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// The sample song with the browser on its instruments and no scan.
fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.browser = crate::browser::test_library::example();
    s.mixer.catalog = Some(Arc::new(Catalog::default()));
    assert!(s.browser.open);
    assert_eq!(s.browser.tab, RailTab::Instruments);
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn first_of(s: &Studio, kind: TrackKind) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.kind == kind)
        .map(|t| t.id)
        .unwrap()
}

fn ids(s: &Studio) -> Vec<TrackId> {
    s.project().tracks.keys().copied().collect()
}

/// The track that is not in `before`.
fn added(p: &Project, before: &[TrackId]) -> Track {
    p.tracks
        .values()
        .find(|t| !before.contains(&t.id))
        .map(|t| (**t).clone())
        .expect("a new track")
}

fn instrument_name(p: &Project, t: TrackId) -> Option<String> {
    current(&p.tracks[&t]).map(|x| x.name.clone())
}

/// The middle of a hit of the browser.
fn centre(s: &Studio, g: &Geometry, hit: BrowserHit) -> (f32, f32) {
    let l = s.browser.layout(g.panel);
    s.browser.center_of(&l, hit).expect("on screen")
}

/// The right press on the list item `name`.
fn press_item(s: &mut Studio, g: &Geometry, name: &str) {
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::Item { item, .. } if item.name == name))
        .unwrap_or_else(|| panic!("{name} is in the list"));
    let (x, y) = centre(s, g, BrowserHit::Item(i));
    assert!(s.lib_context(g, x, y));
}

fn names(s: &Studio) -> Vec<&'static str> {
    s.browser.library().iter().map(|x| x.name).collect()
}

/// Which rows are on, by label.
fn on(s: &Studio, label: &str) -> bool {
    s.context_rows()
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
        .on
}

// ------------------------------------------------------------ S38 an instrument

#[test]
fn the_menu_of_an_instrument_follows_the_selected_track() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    s.selected_row = Some(RowKind::Track(keys));
    press_item(&mut s, &g, "Sampler");
    assert!(s.context_open());
    assert_eq!(s.browser.selected_item, Some("Sampler"));
    assert_eq!(
        labels(&s),
        [
            "新しいトラックで使う",
            "選んだトラックのインストゥルメントにする",
            "お気に入り",
            "隠す",
            "ファイルの場所を表示",
        ]
    );
    assert!(enabled(&s, "新しいトラックで使う"));
    assert!(enabled(&s, "選んだトラックのインストゥルメントにする"));
    assert!(on(&s, "お気に入り"), "Sampler is a favourite");
    assert!(!enabled(&s, "ファイルの場所を表示"), "1.0");
    let gaps = s.context_rows().iter().filter(|r| r.gap).count();
    assert_eq!(gaps, 1);
    // An audio track takes no instrument.
    let vocal = first_of(&s, TrackKind::Audio);
    s.selected_row = Some(RowKind::Track(vocal));
    press_item(&mut s, &g, "Drum Kit");
    assert!(!enabled(&s, "選んだトラックのインストゥルメントにする"));
    assert!(!on(&s, "お気に入り"), "Drum Kit is no favourite");
}

#[test]
fn new_track_and_selected_track_put_the_instrument_in_one_step() {
    let (mut s, g) = studio();
    let before = ids(&s);
    let steps0 = steps(&s);
    press_item(&mut s, &g, "Drum Kit");
    assert!(choose(&mut s, &g, "新しいトラックで使う"));
    assert_eq!(s.error, None);
    let p = s.project();
    let new = added(&p, &before);
    assert_eq!(new.kind, TrackKind::Instrument);
    assert_eq!(instrument_name(&p, new.id).as_deref(), Some("Drum Kit"));
    assert_eq!(s.selected_row, Some(RowKind::Track(new.id)));
    assert_eq!(steps(&s), steps0 + 1);
    assert!(s.undo(&g));
    assert_eq!(ids(&s).len(), before.len());
    // The selected track's instrument.
    let keys = track(&s, "Keys");
    s.selected_row = Some(RowKind::Track(keys));
    press_item(&mut s, &g, "Sampler");
    assert!(choose(
        &mut s,
        &g,
        "選んだトラックのインストゥルメントにする"
    ));
    assert_eq!(s.error, None);
    assert_eq!(
        instrument_name(&s.project(), keys).as_deref(),
        Some("Sampler")
    );
    assert_eq!(ids(&s).len(), before.len());
    assert!(s.undo(&g));
    assert_eq!(instrument_name(&s.project(), keys), None);
}

#[test]
fn favourite_and_hide_change_the_list() {
    let (mut s, g) = studio();
    press_item(&mut s, &g, "Sampler");
    assert!(choose(&mut s, &g, "お気に入り"));
    assert!(!s.browser.favourites.contains("Sampler"));
    press_item(&mut s, &g, "Sampler");
    assert!(choose(&mut s, &g, "お気に入り"));
    assert!(s.browser.favourites.contains("Sampler"));
    // Hidden: out of the list, but a search finds it.
    let steps0 = steps(&s);
    press_item(&mut s, &g, "Dexed");
    assert!(choose(&mut s, &g, "隠す"));
    assert!(!names(&s).contains(&"Dexed"));
    assert_eq!(s.browser.selected_item, None);
    assert_eq!(s.notice_text(), Some("「Dexed」を隠しました"));
    assert_eq!(steps(&s), steps0, "the song does not change");
    s.browser.search = "dex".into();
    assert_eq!(names(&s), ["Dexed"]);
    press_item(&mut s, &g, "Dexed");
    assert_eq!(labels(&s)[3], "一覧に戻す");
    assert!(choose(&mut s, &g, "一覧に戻す"));
    s.browser.search.clear();
    assert!(names(&s).contains(&"Dexed"));
}

#[test]
fn an_instrument_this_pc_has_not_found_says_so() {
    let (mut s, g) = studio();
    let before = ids(&s).len();
    let steps0 = steps(&s);
    press_item(&mut s, &g, "Vital");
    assert!(!choose(&mut s, &g, "新しいトラックで使う"));
    assert_eq!(
        s.error.as_deref(),
        Some("「Vital」はまだ見つかっていません")
    );
    assert_eq!(ids(&s).len(), before);
    assert_eq!(steps(&s), steps0);
}

// ---------------------------------------------------------------- S39 an effect

fn effects() -> (Studio, Geometry) {
    let (mut s, _) = studio();
    s.browser.press_rail(RailTab::Effects);
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn inserts(s: &Studio, t: TrackId) -> Vec<String> {
    s.project().tracks[&t]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

#[test]
fn an_effect_goes_to_the_channel_a_new_bus_or_the_master_in_one_step() {
    let (mut s, g) = effects();
    let vocal = first_of(&s, TrackKind::Audio);
    // No channel selected: that row is dim, the others are not.
    s.selected_row = None;
    press_item(&mut s, &g, "EQ");
    assert_eq!(
        labels(&s),
        [
            "選んだチャンネルに追加",
            "新しいバスに追加",
            "マスターに追加",
            "お気に入り",
            "隠す",
            "ファイルの場所を表示",
        ]
    );
    assert!(!enabled(&s, "選んだチャンネルに追加"));
    assert!(enabled(&s, "新しいバスに追加"));
    assert!(enabled(&s, "マスターに追加"));
    s.popup = None;
    s.ctx = None;
    // The selected channel.
    s.selected_row = Some(RowKind::Track(vocal));
    let before = inserts(&s, vocal);
    let steps0 = steps(&s);
    press_item(&mut s, &g, "EQ");
    assert!(enabled(&s, "選んだチャンネルに追加"));
    assert!(choose(&mut s, &g, "選んだチャンネルに追加"));
    assert_eq!(s.error, None);
    assert_eq!(inserts(&s, vocal).len(), before.len() + 1);
    assert_eq!(steps(&s), steps0 + 1);
    assert!(s.undo(&g));
    assert_eq!(inserts(&s, vocal), before);
    // The master. (The undone step stays listed until the next one takes its place.)
    let master = TrackId::MASTER;
    let at = inserts(&s, master).len();
    let steps0 = steps(&s) - 1;
    press_item(&mut s, &g, "EQ");
    assert!(choose(&mut s, &g, "マスターに追加"));
    assert_eq!(s.error, None);
    assert_eq!(inserts(&s, master).len(), at + 1);
    assert_eq!(steps(&s), steps0 + 1);
    assert!(s.undo(&g));
    assert_eq!(inserts(&s, master).len(), at);
    // A new bus that has it: one step for both.
    let tracks = ids(&s);
    let steps0 = steps(&s) - 1;
    press_item(&mut s, &g, "EQ");
    assert!(choose(&mut s, &g, "新しいバスに追加"));
    assert_eq!(s.error, None);
    let p = s.project();
    let bus = added(&p, &tracks);
    assert_eq!(bus.kind, TrackKind::Bus);
    assert_eq!(bus.inserts.len(), 1);
    assert_eq!(s.selected_row, Some(RowKind::Track(bus.id)));
    assert_eq!(steps(&s), steps0 + 1);
    assert!(s.undo(&g));
    assert_eq!(ids(&s).len(), tracks.len());
}

#[test]
fn an_effect_put_in_from_the_menu_is_among_the_recent_ones() {
    let (mut s, g) = effects();
    let vocal = first_of(&s, TrackKind::Audio);
    s.selected_row = Some(RowKind::Track(vocal));
    press_item(&mut s, &g, "EQ");
    assert!(choose(&mut s, &g, "新しいバスに追加"));
    assert_eq!(s.recent_effects().len(), 1);
    assert_eq!(s.recent_effects()[0].label, "EQ");
}

#[test]
fn an_effect_has_favourite_and_hide_too() {
    let (mut s, g) = effects();
    press_item(&mut s, &g, "EQ");
    assert!(choose(&mut s, &g, "隠す"));
    assert!(!names(&s).contains(&"EQ"));
    s.browser.set_show_hidden(true);
    assert!(names(&s).contains(&"EQ"));
}

// ---------------------------------------------------------- S40 the blank of a list

#[test]
fn the_blank_of_a_list_opens_folds_and_shows_the_hidden() {
    let (mut s, g) = studio();
    // A group's header.
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(p.row, BRow::Group { .. }))
        .expect("grouped by kind");
    let (x, y) = centre(&s, &g, BrowserHit::Group(i));
    assert!(s.lib_context(&g, x, y));
    assert_eq!(
        labels(&s),
        [
            "すべて開く",
            "すべて畳む",
            "隠したものを表示",
            "プラグインを再スキャン",
        ]
    );
    let items = |s: &Studio| {
        s.browser
            .rows()
            .iter()
            .filter(|r| matches!(r, BRow::Item { .. }))
            .count()
    };
    let all = items(&s);
    assert!(all > 0);
    assert!(choose(&mut s, &g, "すべて畳む"));
    assert_eq!(items(&s), 0);
    assert!(s.lib_context(&g, x, y));
    assert!(choose(&mut s, &g, "すべて開く"));
    assert_eq!(items(&s), all);
    // The items left out.
    s.browser.set_hidden("Dexed", true);
    assert!(!names(&s).contains(&"Dexed"));
    assert!(s.lib_context(&g, x, y));
    assert!(!on(&s, "隠したものを表示"));
    assert!(choose(&mut s, &g, "隠したものを表示"));
    assert!(s.browser.show_hidden);
    assert!(names(&s).contains(&"Dexed"));
    assert!(s.lib_context(&g, x, y));
    assert!(on(&s, "隠したものを表示"));
}

#[test]
fn a_flat_list_has_nothing_to_open_and_a_scan_in_progress_dims_the_rescan() {
    let (mut s, g) = studio();
    s.browser.set_view(crate::browser::View::Name);
    let g2 = s.geometry(g.size.0, g.size.1);
    // Below the last row.
    let l = s.browser.layout(g2.panel);
    let (x, y) = (l.list.x + 20.0, l.list.bottom() - 2.0);
    assert!(s.lib_context(&g2, x, y));
    assert!(!enabled(&s, "すべて開く"));
    assert!(!enabled(&s, "すべて畳む"));
    assert!(enabled(&s, "プラグインを再スキャン"));
    s.browser.scanning = true;
    assert!(s.lib_context(&g2, x, y));
    assert!(!enabled(&s, "プラグインを再スキャン"));
}

// ------------------------------------------------------- S41–S43 the files tab

/// A folder of the test's own, removed when dropped (also when the test fails).
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("xirius-ui-ctxbrowser-{name}-"))
        .tempdir()
        .unwrap()
}

/// The sample song with the files tab on a place of WAV and MIDI files, its Loops folder
/// open. The place goes when the guard is dropped.
fn files(name: &str) -> (Studio, Geometry, PathBuf, tempfile::TempDir) {
    let top_tmp = folder(name);
    let top = top_tmp.path().join("素材");
    make_place(&top);
    let mut s = Studio::sample();
    s.browser.press_rail(RailTab::Files);
    s.browser.add_place(&top).unwrap();
    s.browser
        .open_dirs
        .insert(top.join("Loops").to_string_lossy().into_owned());
    let g = s.geometry(1440.0, 860.0);
    (s, g, top, top_tmp)
}

fn press_file(s: &mut Studio, g: &Geometry, name: &str) {
    let i = file_row(s, g, name).unwrap_or_else(|| panic!("no row {name}"));
    let (x, y) = centre(s, g, BrowserHit::File(i));
    assert!(s.lib_context(g, x, y));
}

fn press_dir(s: &mut Studio, g: &Geometry, name: &str) {
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::Dir { name: n, .. } if n == name))
        .unwrap_or_else(|| panic!("no folder {name}"));
    let (x, y) = centre(s, g, BrowserHit::Dir(i));
    assert!(s.lib_context(g, x, y));
}

fn press_tab(s: &mut Studio, g: &Geometry, i: usize) {
    let (x, y) = centre(s, g, BrowserHit::Loc(i));
    assert!(s.lib_context(g, x, y));
}

fn audio_clips(s: &Studio, file: &Path) -> Vec<(TrackId, i64)> {
    let mut out = Vec::new();
    for t in s.project().tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file: f, .. } = &c.body
                && Path::new(f) == file
            {
                out.push((t.id, c.start));
            }
        }
    }
    out
}

#[test]
fn a_file_goes_on_a_new_track_or_the_selected_audio_track_in_one_step() {
    let (mut s, g, top, _tmp) = files("file");
    let wav = top.join("Loops/loop_120bpm.wav");
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert_eq!(
        labels(&s),
        [
            "新しいトラックに置く",
            "選んだトラックに置く",
            "お気に入り",
            "エクスプローラーで表示",
            "名前を変更",
            "ごみ箱へ",
        ]
    );
    assert_eq!(
        s.browser.selected_file.as_deref().map(Path::new),
        Some(wav.as_path())
    );
    assert_eq!(s.browser.playing, None, "the right press does not play it");
    // No audio track selected.
    s.selected_row = None;
    s.popup = None;
    s.ctx = None;
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(!enabled(&s, "選んだトラックに置く"));
    let tracks = ids(&s).len();
    let steps0 = steps(&s);
    assert!(choose(&mut s, &g, "新しいトラックに置く"));
    assert_eq!(s.error, None);
    assert_eq!(ids(&s).len(), tracks + 1);
    assert_eq!(audio_clips(&s, &wav).len(), 1);
    assert_eq!(steps(&s), steps0 + 1);
    assert!(s.undo(&g));
    assert_eq!(ids(&s).len(), tracks);
    assert!(audio_clips(&s, &wav).is_empty());
    // On the selected audio track.
    let vocal = first_of(&s, TrackKind::Audio);
    s.selected_row = Some(RowKind::Track(vocal));
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(enabled(&s, "選んだトラックに置く"));
    assert!(choose(&mut s, &g, "選んだトラックに置く"));
    assert_eq!(s.error, None);
    assert_eq!(ids(&s).len(), tracks);
    let clips = audio_clips(&s, &wav);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].0, vocal);
    assert!(s.undo(&g));
    assert!(audio_clips(&s, &wav).is_empty());
}

#[test]
fn a_midi_file_goes_on_a_new_track_only() {
    let (mut s, g, top, _tmp) = files("midi");
    let mid = std::fs::read_dir(&top)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| crate::file_drop::is_midi(p))
        .expect("a MIDI file in the place");
    let name = mid.file_name().unwrap().to_string_lossy().into_owned();
    let vocal = first_of(&s, TrackKind::Audio);
    s.selected_row = Some(RowKind::Track(vocal));
    press_file(&mut s, &g, &name);
    assert!(enabled(&s, "新しいトラックに置く"));
    assert!(!enabled(&s, "選んだトラックに置く"));
    let tracks = ids(&s).len();
    assert!(choose(&mut s, &g, "新しいトラックに置く"));
    assert_eq!(s.error, None);
    assert_eq!(ids(&s).len(), tracks + 1);
    assert!(s.undo(&g));
    assert_eq!(ids(&s).len(), tracks);
}

#[test]
fn the_explorer_row_names_the_path_and_opens_nothing_outside_the_real_window() {
    let (mut s, g, top, _tmp) = files("reveal");
    assert!(!s.os_files);
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(choose(&mut s, &g, "エクスプローラーで表示"));
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "エクスプローラーで表示"));
    assert_eq!(
        s.revealed,
        [top.join("Loops/loop_120bpm.wav"), top.join("Drums")]
    );
    assert_eq!(s.error, None);
}

#[test]
fn file_and_folder_favourites_are_kept_in_the_existing_preferences() {
    let (mut s, g, top, _tmp) = files("favorites");
    let pref = top.join("browser.json");
    s.browser.use_prefs(pref.clone()).unwrap();
    s.browser.add_place(&top).unwrap();
    s.browser
        .open_dirs
        .insert(top.join("Loops").to_string_lossy().into_owned());
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(choose(&mut s, &g, "お気に入り"));
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "お気に入り"));
    let got = s.browser.favourites.clone();
    assert_eq!(
        got.iter().filter(|name| name.starts_with("file:")).count(),
        2
    );
    let mut browser = crate::browser::Browser::new();
    browser.use_prefs(pref).unwrap();
    assert_eq!(browser.favourites, got);
    assert_eq!(steps(&s), 0);
}

#[test]
fn a_folder_becomes_a_tab_and_is_dim_once_it_is() {
    let (mut s, g, top, _tmp) = files("dir");
    let steps0 = steps(&s);
    press_dir(&mut s, &g, "Drums");
    assert_eq!(
        labels(&s),
        [
            "場所のタブに追加",
            "お気に入り",
            "エクスプローラーで表示",
            "新しいフォルダー",
            "名前を変更",
            "ごみ箱へ",
        ]
    );
    assert!(enabled(&s, "場所のタブに追加"));
    let places = s.browser.places.len();
    assert!(choose(&mut s, &g, "場所のタブに追加"));
    assert_eq!(s.error, None);
    assert_eq!(s.browser.places.len(), places + 1);
    assert_eq!(s.browser.places.last().unwrap().path, top.join("Drums"));
    assert_eq!(s.browser.loc, places + 1, "the new tab is shown");
    assert_eq!(steps(&s), steps0, "the song does not change");
    // Its own top shows no folder row of itself: press the place's Loops folder instead.
    s.browser.set_location(places);
    let g = s.geometry(1440.0, 860.0);
    press_dir(&mut s, &g, "Loops");
    assert!(enabled(&s, "場所のタブに追加"));
}

#[test]
fn a_tab_is_renamed_or_taken_away_but_the_songs_is_not() {
    let (mut s, g, top, _tmp) = files("tab");
    // The song's tab.
    press_tab(&mut s, &g, 0);
    assert_eq!(
        labels(&s),
        ["場所名を変更", "エクスプローラーで表示", "場所を外す"]
    );
    assert!(!enabled(&s, "場所名を変更"));
    assert!(!enabled(&s, "場所を外す"));
    s.popup = None;
    s.ctx = None;
    // A place.
    press_tab(&mut s, &g, 1);
    assert!(enabled(&s, "場所名を変更"));
    assert!(enabled(&s, "場所を外す"));
    assert!(choose(&mut s, &g, "場所名を変更"));
    assert!(s.ask_field().is_some(), "the menu asks for a name");
    for c in s.ask_field().unwrap().value.clone().chars() {
        let _ = c;
        s.ask_key("backspace");
    }
    s.ask_text(None, "素材集", false);
    s.ask_key("enter");
    assert_eq!(s.error, None);
    assert_eq!(s.browser.places[0].name, "素材集");
    assert_eq!(s.browser.places[0].path, top);
    assert!(!s.context_open());
    // Taken away: the folder stays.
    let g = s.geometry(1440.0, 860.0);
    press_tab(&mut s, &g, 1);
    assert!(choose(&mut s, &g, "場所を外す"));
    assert!(s.browser.places.is_empty());
    assert!(top.is_dir());
    assert_eq!(s.browser.loc, 0);
}

#[test]
fn a_name_a_tab_already_has_is_refused_and_the_old_name_stays() {
    let (mut s, g, _, _tmp) = files("same");
    let other_tmp = folder("same2");
    let other = other_tmp.path().join("音");
    std::fs::create_dir_all(&other).unwrap();
    s.browser.add_place(&other).unwrap();
    let first = s.browser.places[0].name.clone();
    let g = s.geometry(g.size.0, g.size.1);
    press_tab(&mut s, &g, 2);
    assert!(choose(&mut s, &g, "場所名を変更"));
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    s.ask_text(None, &first, false);
    s.ask_key("enter");
    assert!(s.error.is_some());
    assert_eq!(s.browser.places[1].name, "音");
}

#[test]
fn a_right_press_off_the_rows_opens_no_menu() {
    let (mut s, g, _, _tmp) = files("none");
    let l = s.browser.layout(g.panel);
    // The blank under the last row of the files, and the search box.
    assert!(!s.lib_context(&g, l.list.x + 4.0, l.list.bottom() - 1.0));
    assert!(!s.lib_context(&g, l.search.x + 4.0, l.search.y + 4.0));
    assert!(!s.context_open());
}

#[test]
fn a_row_that_cannot_be_used_is_dim_and_the_rows_keep_their_places() {
    let (mut s, g) = studio();
    // An instrument menu with and without an instrument track selected.
    let keys = track(&s, "Keys");
    s.selected_row = Some(RowKind::Track(keys));
    press_item(&mut s, &g, "Sampler");
    let with = labels(&s);
    assert!(enabled(&s, "選んだトラックのインストゥルメントにする"));
    s.popup = None;
    s.ctx = None;
    let vocal = first_of(&s, TrackKind::Audio);
    s.selected_row = Some(RowKind::Track(vocal));
    press_item(&mut s, &g, "Sampler");
    assert_eq!(labels(&s), with, "nothing moves or goes");
    assert!(!enabled(&s, "選んだトラックのインストゥルメントにする"));
    assert!(enabled(&s, "新しいトラックで使う"));
}

// ------------------------------------------ CTX-059 new name, new folder, recycle bin

/// The recycle bin of these tests: `bin` in the test's own folder, never the PC's.
fn bin_in(dir: &Path) -> xirius_session::cleanup::Trash {
    let bin = dir.join("bin");
    Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    })
}

/// [`files`] with a song whose recycle bin is `bin` in the test's own folder.
fn binned(name: &str) -> (Studio, Geometry, PathBuf, tempfile::TempDir) {
    let tmp = folder(name);
    let top = tmp.path().join("素材");
    make_place(&top);
    let mut s = Studio::new(crate::song::Song::sample_with_trash(bin_in(tmp.path())));
    s.browser.press_rail(RailTab::Files);
    s.browser.add_place(&top).unwrap();
    s.browser
        .open_dirs
        .insert(top.join("Loops").to_string_lossy().into_owned());
    let g = s.geometry(1440.0, 860.0);
    (s, g, top, tmp)
}

/// Types `text` into the name box the menu turned into, in place of what it had, and
/// enters it.
fn type_name(s: &mut Studio, text: &str) {
    let had = s
        .ask_field()
        .expect("the menu asks for a name")
        .value
        .chars()
        .count();
    for _ in 0..had {
        s.ask_key("backspace");
    }
    s.ask_text(None, text, false);
    s.ask_key("enter");
}

/// What a dim row says it is dim for.
fn reason(s: &Studio, label: &str) -> String {
    s.context_rows()
        .into_iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}"))
        .reason
}

fn close(s: &mut Studio) {
    s.popup = None;
    s.ctx = None;
}

#[test]
fn a_rename_of_a_used_file_points_its_clips_at_the_new_name_in_one_step() {
    let (mut s, g, top, _tmp) = files("rename");
    let old = top.join("Loops/loop_120bpm.wav");
    let new = top.join("Loops/groove.wav");
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(choose(&mut s, &g, "新しいトラックに置く"));
    assert_eq!(audio_clips(&s, &old).len(), 1);
    let steps0 = steps(&s);
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(enabled(&s, "名前を変更"));
    assert!(choose(&mut s, &g, "名前を変更"));
    assert_eq!(
        s.ask_field().unwrap().value,
        "loop_120bpm",
        "the box has the name without its extension"
    );
    assert!(old.is_file(), "nothing changes before the name is entered");
    type_name(&mut s, "groove");
    assert_eq!(s.error, None);
    assert!(!s.context_open());
    assert!(new.is_file() && !old.exists());
    assert!(audio_clips(&s, &old).is_empty());
    assert_eq!(audio_clips(&s, &new).len(), 1);
    assert_eq!(steps(&s), steps0 + 1, "one step");
    assert_eq!(
        s.browser.selected_file.as_deref().map(Path::new),
        Some(new.as_path()),
        "the row stays selected under its new name"
    );
    assert!(
        file_row(&s, &g, "groove.wav").is_some(),
        "the list shows it at once"
    );
    assert!(file_row(&s, &g, "loop_120bpm.wav").is_none());
    // The undo takes the clip and the name on the disk back, and the list and the selected
    // row follow; the redo renames it again (BRW-013).
    assert!(s.undo(&g));
    assert_eq!(s.error, None);
    assert_eq!(audio_clips(&s, &old).len(), 1);
    assert!(old.is_file() && !new.exists());
    assert!(file_row(&s, &g, "loop_120bpm.wav").is_some());
    assert!(file_row(&s, &g, "groove.wav").is_none());
    assert_eq!(
        s.browser.selected_file.as_deref().map(Path::new),
        Some(old.as_path())
    );
    assert!(s.redo(&g));
    assert_eq!(audio_clips(&s, &new).len(), 1);
    assert!(new.is_file() && !old.exists());
    assert!(file_row(&s, &g, "groove.wav").is_some());
}

#[test]
fn an_undo_the_disk_cannot_follow_keeps_the_clips_on_the_new_name_and_says_why() {
    let (mut s, g, top, _tmp) = files("rename-blocked");
    let old = top.join("Loops/loop_120bpm.wav");
    let new = top.join("Loops/groove.wav");
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(choose(&mut s, &g, "新しいトラックに置く"));
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(choose(&mut s, &g, "名前を変更"));
    type_name(&mut s, "groove");
    std::fs::write(&old, b"another").unwrap();

    assert!(s.undo(&g), "the key is used");
    let e = s.error.clone().expect("the error is shown");
    assert!(
        e.contains("既にある") && e.contains("取り消していません"),
        "{e}"
    );
    assert_eq!(
        audio_clips(&s, &new).len(),
        1,
        "the clip stays on the new name"
    );
    assert!(audio_clips(&s, &old).is_empty());
    assert_eq!(std::fs::read(&old).unwrap(), b"another");
    assert!(new.is_file());
    assert!(!s.song.can_redo(), "the history did not move");

    // With the name free again the undo goes through and the error goes away.
    std::fs::remove_file(&old).unwrap();
    assert!(s.undo(&g));
    assert_eq!(s.error, None);
    assert_eq!(audio_clips(&s, &old).len(), 1);
}

#[test]
fn a_folder_is_renamed_and_a_name_in_use_is_refused() {
    let (mut s, g, top, _tmp) = files("rename-dir");
    let steps0 = steps(&s);
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "名前を変更"));
    assert_eq!(s.ask_field().unwrap().value, "Drums");
    type_name(&mut s, "Loops");
    assert!(
        s.error
            .as_deref()
            .is_some_and(|e| e.contains("既にあります")),
        "{:?}",
        s.error
    );
    assert!(top.join("Drums").is_dir());
    s.error = None;
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "名前を変更"));
    type_name(&mut s, "Kit");
    assert_eq!(s.error, None);
    assert!(top.join("Kit/kick_01.wav").is_file());
    assert_eq!(
        steps(&s),
        steps0,
        "no file of the song: the history is left alone"
    );
}

#[test]
fn a_new_folder_takes_a_name_the_folder_does_not_have() {
    let (mut s, g, top, _tmp) = files("new-dir");
    let steps0 = steps(&s);
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "新しいフォルダー"));
    assert_eq!(s.ask_field().unwrap().value, "新しいフォルダー");
    s.ask_key("enter");
    assert_eq!(s.error, None);
    assert!(top.join("Drums/新しいフォルダー").is_dir());
    press_dir(&mut s, &g, "Drums");
    assert!(choose(&mut s, &g, "新しいフォルダー"));
    assert_eq!(s.ask_field().unwrap().value, "新しいフォルダー (2)");
    type_name(&mut s, "Toms");
    assert_eq!(s.error, None);
    assert!(top.join("Drums/Toms").is_dir());
    s.browser
        .open_dirs
        .insert(top.join("Drums").to_string_lossy().into_owned());
    let l = s.browser.layout(g.panel);
    assert!(
        l.rows
            .iter()
            .any(|p| matches!(&p.row, BRow::Dir { name, .. } if name == "Toms")),
        "the list shows it at once"
    );
    assert_eq!(steps(&s), steps0);
}

#[test]
fn the_recycle_bin_asks_once_and_what_the_song_uses_is_dim() {
    let (mut s, g, top, tmp) = binned("trash");
    let bin = tmp.path().join("bin");
    // A file the song does not use: one question, then the bin.
    press_file(&mut s, &g, "loop_120bpm.wav");
    assert!(enabled(&s, "ごみ箱へ"));
    assert!(choose(&mut s, &g, "ごみ箱へ"));
    assert!(s.context_open(), "the row opens the one question");
    assert_eq!(labels(&s), ["戻る", "ファイルをごみ箱へ"]);
    assert!(
        top.join("Loops/loop_120bpm.wav").is_file(),
        "nothing moves before it"
    );
    assert!(choose(&mut s, &g, "ファイルをごみ箱へ"));
    assert_eq!(s.error, None);
    assert!(!top.join("Loops/loop_120bpm.wav").exists());
    assert!(bin.join("loop_120bpm.wav").is_file());
    assert_eq!(s.browser.selected_file, None);
    // A file the song uses, and the folder it is in, are dim and say why.
    s.browser
        .open_dirs
        .insert(top.join("Drums").to_string_lossy().into_owned());
    press_file(&mut s, &g, "kick_01.wav");
    assert!(choose(&mut s, &g, "新しいトラックに置く"));
    press_file(&mut s, &g, "kick_01.wav");
    assert!(!enabled(&s, "ごみ箱へ"));
    assert!(
        reason(&s, "ごみ箱へ").contains("使用中"),
        "{}",
        reason(&s, "ごみ箱へ")
    );
    assert!(!choose(&mut s, &g, "ごみ箱へ"));
    close(&mut s);
    press_dir(&mut s, &g, "Drums");
    assert!(!enabled(&s, "ごみ箱へ"));
    assert!(
        reason(&s, "ごみ箱へ").contains("中のファイル"),
        "{}",
        reason(&s, "ごみ箱へ")
    );
    close(&mut s);
    assert!(top.join("Drums/kick_01.wav").is_file());
    // A folder of nothing the song uses goes as a whole.
    press_dir(&mut s, &g, "Loops");
    assert!(choose(&mut s, &g, "ごみ箱へ"));
    assert_eq!(labels(&s), ["戻る", "フォルダーをごみ箱へ"]);
    assert!(choose(&mut s, &g, "フォルダーをごみ箱へ"));
    assert_eq!(s.error, None);
    assert!(!top.join("Loops").exists());
    assert!(bin.join("Loops").is_dir());
}

#[test]
fn the_folder_of_the_open_song_is_not_renamed_or_binned() {
    let (mut s, g, top, _tmp) = binned("own");
    s.song.save_as(&top.join("曲")).unwrap();
    crate::browser_files::forget();
    press_dir(&mut s, &g, "曲");
    assert!(!enabled(&s, "名前を変更"));
    assert!(
        reason(&s, "名前を変更").contains("開いている曲"),
        "{}",
        reason(&s, "名前を変更")
    );
    assert!(!enabled(&s, "ごみ箱へ"));
    close(&mut s);
    press_dir(&mut s, &g, "Drums");
    assert!(enabled(&s, "名前を変更"), "a folder beside it is free");
}
