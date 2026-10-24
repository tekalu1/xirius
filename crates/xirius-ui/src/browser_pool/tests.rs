//! The song-files tab (BRW-004) on a song made in the test's own folder: its rows, its menus,
//! its keys, the files it points elsewhere and copies in, and the song's settings of where the
//! files go. Pressed where the window presses; the OS's file window is answered in its place.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use xirius_core::model::ClipBody;

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::pool_shots::{KICK, MIX, SYNTH, VOCAL_1, VOCAL_2, make_song, pool_mid, pool_row};
use crate::studio::{FileRequest, Geometry, Hit, Studio};
use crate::tools::Mods;

mod cleanup;

struct Song {
    s: Studio,
    g: Geometry,
    root: tempfile::TempDir,
}

impl Song {
    fn dir(&self) -> PathBuf {
        self.root.path().join("曲")
    }
    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }
    fn path(&self, file: &str) -> String {
        let path: PathBuf = self.dir().join(file).components().collect();
        path.to_string_lossy().into_owned()
    }
    fn lost(&self, name: &str) -> String {
        self.root
            .path()
            .join("なくした")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }
    fn found(&self, name: &str) -> PathBuf {
        self.root.path().join("見つけた").join(name)
    }
    fn outside(&self) -> String {
        self.root
            .path()
            .join("外部/Pad.wav")
            .to_string_lossy()
            .into_owned()
    }
}

/// The song of [`make_song`], open on the song-files tab, whose recycle bin is `bin` beside it.
fn song(tag: &str) -> Song {
    let root = tempfile::Builder::new()
        .prefix(&format!("xirius-ui-pool-{tag}-"))
        .tempdir()
        .unwrap();
    let dir = make_song(root.path());
    let bin = root.path().join("bin");
    let trash: xirius_session::cleanup::Trash = Arc::new(move |p: &Path| {
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(p.file_name().unwrap())).map_err(|e| e.to_string())
    });
    let mut s = Studio::new(crate::song::Song::sample_with_trash(trash));
    assert!(s.command(|song| song.open(&dir)));
    s.browser.open = false;
    s.browser.press_rail(RailTab::SongFiles);
    s.follow_browser();
    let g = s.geometry(1440.0, 860.0);
    Song { s, g, root }
}

/// The list as text: `[heading count]`, and each file with ` ?` when missing, ` ext` when
/// outside the song and its count of clips.
fn rows(s: &Studio, g: &Geometry) -> Vec<String> {
    s.browser
        .layout(g.panel)
        .rows
        .iter()
        .map(|p| match &p.row {
            BRow::Group { name, count, .. } => format!("[{name} {count}]"),
            BRow::Pool {
                name,
                clips,
                missing,
                external,
                ..
            } => {
                let mut t = name.clone();
                if *missing {
                    t.push_str(" ?");
                }
                if *external {
                    t.push_str(" ext");
                }
                if let Some(n) = clips {
                    t.push_str(&format!(" {n}"));
                }
                t
            }
            BRow::Audition { .. } => "<audition>".into(),
            other => format!("{other:?}"),
        })
        .collect()
}

fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

fn row_of(s: &Studio, label: &str) -> crate::context::Row {
    s.context_rows()
        .into_iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
}

/// The right press on the row of `name`.
fn menu_of(t: &mut Song, name: &str) {
    let (x, y) = pool_mid(&t.s, &t.g, name);
    assert!(t.s.lib_context(&t.g, x, y));
    assert!(t.s.context_open());
}

/// The press on the tab's 「…」.
fn more(t: &mut Song) {
    let (x, y) =
        t.s.browser
            .center_of(&t.s.browser.layout(t.g.panel), BrowserHit::PoolMenu)
            .unwrap();
    let hit = t.s.hit(&t.g, x, y);
    assert_eq!(hit, Hit::Browser(BrowserHit::PoolMenu));
    assert!(t.s.press(&t.g, hit));
    assert!(t.s.context_open());
}

/// The left press on the row of `name`.
fn press(t: &mut Song, name: &str) {
    let (x, y) = pool_mid(&t.s, &t.g, name);
    let hit = t.s.hit(&t.g, x, y);
    assert!(matches!(hit, Hit::Browser(BrowserHit::Pool(_))), "{hit:?}");
    assert!(t.s.press(&t.g, hit));
}

fn using(s: &Studio, file: &str) -> usize {
    s.project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| matches!(&c.body, ClipBody::Audio { file: f, .. } if f == file))
        .count()
}

fn key(t: &mut Song, k: &str, mods: Mods) -> bool {
    t.s.mapped_key(&t.g, k, None, mods)
}

/// The OS's file window gives `paths` for what the studio asked.
fn answer(t: &mut Song, want: FileRequest, paths: &[PathBuf]) {
    let asked = t.s.file_request.take();
    assert_eq!(asked, Some(want.clone()));
    t.s.finish_picked(want, paths);
    t.s.follow_browser();
}

#[test]
fn the_files_are_grouped_by_what_made_them_with_their_counts_and_states() {
    let t = song("rows");
    assert!(t.s.pool_shown());
    assert_eq!(
        rows(&t.s, &t.g),
        [
            "[録音 2]",
            "Vocal_01.wav 2",
            "Vocal_02.wav 0",
            "[インポート 4]",
            "Bass.wav ? ext 1",
            "Gtr.wav ? ext 1",
            "Kick.wav 1",
            "Pad.wav ext 1",
            "[バウンス 1]",
            "Mix.wav 1",
            "[フリーズ 1]",
            // Only a frozen track uses it: no count.
            "Synth.wav",
        ]
    );
    assert_eq!(t.s.browser.pool_missing(), 2, "the rail has its dot");
}

#[test]
fn a_heading_folds_and_a_search_keeps_the_files_it_names() {
    let mut t = song("fold");
    t.s.browser.toggle_group(&super::heading_key("import"));
    let r = rows(&t.s, &t.g);
    assert!(r.contains(&"[インポート 4]".to_string()));
    assert!(!r.iter().any(|x| x.starts_with("Kick")), "{r:?}");
    t.s.browser.search = "vocal".into();
    assert_eq!(
        rows(&t.s, &t.g),
        ["[録音 2]", "Vocal_01.wav 2", "Vocal_02.wav 0"]
    );
    // The search opens a folded heading with a hit.
    t.s.browser.search = "kick".into();
    assert_eq!(rows(&t.s, &t.g), ["[インポート 1]", "Kick.wav 1"]);
    t.s.browser.search = "nothing-has-this".into();
    assert!(rows(&t.s, &t.g).is_empty());
}

/// The rows' icons, by their names.
fn icons(s: &Studio, g: &Geometry) -> Vec<(String, &'static str)> {
    s.browser
        .layout(g.panel)
        .rows
        .iter()
        .filter_map(|p| match &p.row {
            BRow::Pool { name, icon, .. } => Some((name.clone(), *icon)),
            _ => None,
        })
        .collect()
}

/// The press on the order's button and on 「`label`」 in the list it opens.
fn order_by(t: &mut Song, label: &str) {
    let (x, y) =
        t.s.browser
            .center_of(&t.s.browser.layout(t.g.panel), BrowserHit::ViewButton)
            .unwrap();
    let hit = t.s.hit(&t.g, x, y);
    assert_eq!(hit, Hit::Browser(BrowserHit::ViewButton));
    assert!(t.s.press(&t.g, hit));
    let popup = t.s.popup.as_ref().expect("the list of orders");
    let l = popup.layout(t.g.size);
    let texts: Vec<&str> = l.cells.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["", "種類別", "名前順"]);
    let i = l.cells.iter().position(|c| c.text == label).unwrap();
    let (x, y) = t.s.center_of(&t.g, Hit::Item(i)).unwrap();
    let hit = t.s.hit(&t.g, x, y);
    assert_eq!(hit, Hit::Item(i));
    assert!(t.s.press(&t.g, hit));
    assert!(t.s.popup.is_none());
}

#[test]
fn the_order_button_lists_the_files_by_name_with_their_kinds_icons() {
    let mut t = song("by-name");
    t.s.browser.toggle_group(&super::heading_key("import"));
    order_by(&mut t, "名前順");
    assert_eq!(t.s.browser.pool_view, crate::browser::View::Name);
    // No headings, and a folded heading hides nothing.
    assert_eq!(
        rows(&t.s, &t.g),
        [
            "Bass.wav ? ext 1",
            "Gtr.wav ? ext 1",
            "Kick.wav 1",
            "Mix.wav 1",
            "Pad.wav ext 1",
            "Synth.wav",
            "Vocal_01.wav 2",
            "Vocal_02.wav 0",
        ]
    );
    let icons = icons(&t.s, &t.g);
    let icon = |n: &str| icons.iter().find(|(m, _)| m == n).unwrap().1;
    assert_eq!(icon("Vocal_01.wav"), "mic");
    assert_eq!(icon("Kick.wav"), "file");
    assert_eq!(icon("Mix.wav"), "wave");
    assert_eq!(icon("Synth.wav"), "freeze");
    assert_eq!(icon("Bass.wav"), "alert", "a missing file still says so");
    // A search keeps the order.
    t.s.browser.search = "wav".into();
    assert_eq!(rows(&t.s, &t.g).len(), 8);
    t.s.browser.search = "vocal".into();
    assert_eq!(rows(&t.s, &t.g), ["Vocal_01.wav 2", "Vocal_02.wav 0"]);
    t.s.browser.search.clear();
    // ↑↓ go from file to file; ← has no heading to go up to.
    t.s.input_map.browser_focus = true;
    assert!(key(&mut t, "down", Mods::NONE));
    assert!(key(&mut t, "down", Mods::NONE));
    let gtr = t.s.browser.selected_file.clone();
    assert!(
        gtr.as_deref().is_some_and(|k| k.ends_with("Gtr.wav")),
        "{gtr:?}"
    );
    assert!(key(&mut t, "left", Mods::NONE));
    assert_eq!(t.s.browser.selected_file, gtr);
    assert_eq!(t.s.browser.pool_heading, None);
    // The instruments keep their own order.
    t.s.browser.press_rail(RailTab::Instruments);
    assert_eq!(t.s.browser.view(), crate::browser::View::Kind);
    t.s.browser.press_rail(RailTab::SongFiles);
    order_by(&mut t, "種類別");
    assert_eq!(rows(&t.s, &t.g)[0], "[録音 2]");
    // The heading folded before is folded again.
    assert!(rows(&t.s, &t.g).contains(&"[インポート 4]".to_string()));
    assert!(!rows(&t.s, &t.g).iter().any(|r| r.starts_with("Kick")));
}

#[test]
fn the_order_changes_from_the_command_line_and_over_the_session() {
    let mut t = song("sort-command");
    assert!(t.s.run_binding(&t.g, "view.browser.sort --order name"));
    assert_eq!(rows(&t.s, &t.g)[0], "Bass.wav ? ext 1");
    assert!(t.s.run_binding(&t.g, "view.browser.sort --order kind"));
    assert_eq!(rows(&t.s, &t.g)[0], "[録音 2]");
    // An order the tab does not have says so and changes nothing.
    assert!(!t.s.run_binding(&t.g, "view.browser.sort --order vendor"));
    assert_eq!(
        t.s.error.take().as_deref(),
        Some("プールの一覧はベンダー別に並べ替えできません")
    );
    assert_eq!(t.s.browser.pool_view, crate::browser::View::Kind);
    // Naming the tab shows it.
    assert!(t.s.run_binding(&t.g, "view.browser.sort --order vendor --tab effects"));
    assert_eq!(t.s.browser.tab, RailTab::Effects);
    assert_eq!(t.s.browser.view(), crate::browser::View::Vendor);
    // MCP and the CLI come through the session: the window runs it on its next frame.
    t.s.song
        .run(
            "view.browser.sort",
            json!({"order": "name", "tab": "song_files"}),
        )
        .unwrap();
    assert!(t.s.run_asked(&t.g));
    assert!(t.s.pool_shown());
    assert_eq!(t.s.browser.pool_view, crate::browser::View::Name);
    assert_eq!(t.s.browser.effect_view, crate::browser::View::Vendor);
}

#[test]
fn a_song_without_files_shows_nothing_and_no_dot() {
    let mut s = Studio::sample();
    s.browser.press_rail(RailTab::SongFiles);
    s.browser.pool.clear();
    s.browser.pool_read = Some((0, std::time::Instant::now()));
    let g = s.geometry(1440.0, 860.0);
    assert!(s.browser.layout(g.panel).rows.is_empty());
    assert_eq!(s.browser.pool_missing(), 0);
}

#[test]
fn a_press_selects_the_file_and_plays_it_and_a_missing_one_only_selects() {
    let mut t = song("press");
    press(&mut t, "Kick.wav");
    let kick = t.s.browser.selected_file.clone().unwrap();
    assert_eq!(kick, t.path(KICK));
    assert_eq!(t.s.browser.playing.as_deref(), Some(kick.as_str()));
    let r = rows(&t.s, &t.g);
    let at = r.iter().position(|x| x == "Kick.wav 1").unwrap();
    assert_eq!(r[at + 1], "<audition>", "the band under the row");
    press(&mut t, "Bass.wav");
    assert_eq!(t.s.browser.selected_file, Some(t.lost("Bass.wav")));
    assert_eq!(t.s.browser.playing, None);
    assert!(!rows(&t.s, &t.g).contains(&"<audition>".to_string()));
}

#[test]
fn the_tips_say_where_the_file_is_what_the_count_counts_and_what_the_dots_hold() {
    let t = song("tips");
    let l = t.s.browser.layout(t.g.panel);
    let i = pool_row(&t.s, &t.g, "Vocal_01.wav").unwrap();
    assert_eq!(
        t.s.pool_tip(&t.g, Hit::Browser(BrowserHit::Pool(i))),
        Some(t.path(VOCAL_1))
    );
    let (x, y) = t.s.browser.center_of(&l, BrowserHit::PoolCount(i)).unwrap();
    let hit = t.s.hit(&t.g, x, y);
    assert_eq!(hit, Hit::Browser(BrowserHit::PoolCount(i)));
    assert_eq!(
        t.s.pool_tip(&t.g, hit).as_deref(),
        Some("使っているクリップ 2")
    );
    assert_eq!(
        t.s.pool_tip(&t.g, Hit::Browser(BrowserHit::PoolMenu))
            .as_deref(),
        Some("メニュー")
    );
    let synth = pool_row(&t.s, &t.g, "Synth.wav").unwrap();
    assert_eq!(
        t.s.pool_tip(&t.g, Hit::Browser(BrowserHit::Pool(synth))),
        Some(t.path(SYNTH))
    );
}

#[test]
fn a_used_files_menu_puts_selects_reveals_renames_and_keeps_it_from_the_bin() {
    let mut t = song("used");
    menu_of(&mut t, "Vocal_01.wav");
    assert_eq!(
        labels(&t.s),
        [
            "新しいトラックに置く",
            "選んだトラックに置く",
            "使っているクリップを選択",
            "エクスプローラーで表示",
            "名前を変更",
            "ごみ箱へ",
        ]
    );
    let bin = row_of(&t.s, "ごみ箱へ");
    assert!(
        bin.reason.contains("Vocal_01.wav") && bin.reason.contains("使用中"),
        "{}",
        bin.reason
    );
    assert!(t.s.context_choose(&t.g, "使っているクリップを選択"));
    assert_eq!(t.s.arrange.clips.len(), 2);
    assert!(t.s.arrange.clips.iter().all(|(track, _)| track.0 == 100));
}

#[test]
fn an_unused_take_has_no_select_row_and_goes_to_the_bin() {
    let mut t = song("unused");
    menu_of(&mut t, "Vocal_02.wav");
    assert_eq!(
        labels(&t.s),
        [
            "新しいトラックに置く",
            "選んだトラックに置く",
            "エクスプローラーで表示",
            "名前を変更",
            "ごみ箱へ",
        ]
    );
    assert!(row_of(&t.s, "ごみ箱へ").enabled);
    assert!(t.s.context_choose(&t.g, "ごみ箱へ"));
    assert!(t.s.context_choose(&t.g, "ファイルをごみ箱へ"));
    assert_eq!(t.s.error, None);
    assert!(!t.dir().join(VOCAL_2).exists());
    assert!(t.bin().join("Vocal_02.wav").is_file());
    t.s.follow_browser();
    assert!(pool_row(&t.s, &t.g, "Vocal_02.wav").is_none());
}

#[test]
fn an_outside_file_is_copied_in_from_its_row_and_the_window_says_so() {
    let mut t = song("collect");
    menu_of(&mut t, "Pad.wav");
    assert!(labels(&t.s).contains(&"曲のフォルダーにコピー".to_string()));
    let steps = t.s.song.steps();
    assert!(t.s.context_choose(&t.g, "曲のフォルダーにコピー"));
    assert_eq!(t.s.error, None);
    assert_eq!(
        t.s.notice_text(),
        Some("Pad.wav を曲のフォルダーにコピーしました")
    );
    assert_eq!(t.s.song.steps(), steps + 1);
    assert_eq!(using(&t.s, &t.outside()), 0);
    assert!(t.dir().join("Media/Imports/Pad.wav").is_file());
    t.s.follow_browser();
    assert!(rows(&t.s, &t.g).contains(&"Pad.wav 1".to_string()));
    // A file inside the song has no copy row.
    menu_of(&mut t, "Kick.wav");
    assert!(!labels(&t.s).contains(&"曲のフォルダーにコピー".to_string()));
}

#[test]
fn copying_many_files_counts_them() {
    let mut s = Studio::sample();
    s.say_command(
        "file.collect",
        &json!({}),
        &json!({"files": [{"from": "C:/a/One.wav", "to": "x"}, {"from": "C:/a/Two.wav", "to": "y"}]}),
    );
    assert_eq!(
        s.notice_text(),
        Some("ファイル 2 個を曲のフォルダーにコピーしました")
    );
    s.say_command("file.collect", &json!({}), &json!({"files": []}));
    assert_eq!(
        s.notice_text(),
        Some("ファイル 2 個を曲のフォルダーにコピーしました"),
        "nothing copied says nothing new"
    );
}

#[test]
fn a_missing_files_menu_only_points_it_and_selects_its_clips() {
    let mut t = song("missing");
    menu_of(&mut t, "Bass.wav");
    assert_eq!(labels(&t.s), ["場所を指定…", "使っているクリップを選択"]);
}

#[test]
fn pointing_a_missing_file_points_the_others_of_its_folder_in_one_undo() {
    let mut t = song("locate");
    menu_of(&mut t, "Bass.wav");
    let steps = t.s.song.steps();
    assert!(t.s.context_choose(&t.g, "場所を指定…"));
    let to = t.found("Bass.wav");
    let lost = t.lost("Bass.wav");
    answer(&mut t, FileRequest::Locate(lost), &[to]);
    assert_eq!(t.s.error, None);
    assert_eq!(
        t.s.notice_text(),
        Some("Bass.wav ほか 1 個の場所を指定しました")
    );
    assert_eq!(t.s.song.steps(), steps + 1);
    assert_eq!(t.s.browser.pool_missing(), 0, "the rail's dot is gone");
    let found = |n: &str| t.found(n).to_string_lossy().into_owned();
    assert_eq!(using(&t.s, &found("Bass.wav")), 1);
    assert_eq!(using(&t.s, &found("Gtr.wav")), 1);
    assert!(t.s.undo(&t.g));
    t.s.follow_browser();
    assert_eq!(t.s.browser.pool_missing(), 2);
}

#[test]
fn pointing_one_file_names_it_alone() {
    let mut t = song("locate1");
    std::fs::remove_file(t.found("Gtr.wav")).unwrap();
    menu_of(&mut t, "Bass.wav");
    assert!(t.s.context_choose(&t.g, "場所を指定…"));
    let to = t.found("Bass.wav");
    let lost = t.lost("Bass.wav");
    answer(&mut t, FileRequest::Locate(lost), &[to]);
    assert_eq!(t.s.notice_text(), Some("Bass.wav の場所を指定しました"));
    assert_eq!(t.s.browser.pool_missing(), 1);
}

#[test]
fn the_dots_menu_looks_for_the_missing_in_a_folder_and_dims_what_has_nothing_to_do() {
    let mut t = song("more");
    more(&mut t);
    assert_eq!(
        labels(&t.s),
        [
            "見つからないファイルを探す…",
            "外部のファイルを曲のフォルダーにコピー",
            "曲のフォルダーをエクスプローラーで表示",
            "使われていない録音を整理…",
        ]
    );
    assert!(t.s.context_choose(&t.g, "見つからないファイルを探す…"));
    let folder = t.root.path().join("見つけた");
    answer(&mut t, FileRequest::LocateFolder, &[folder]);
    assert_eq!(t.s.error, None);
    assert_eq!(t.s.browser.pool_missing(), 0);
    more(&mut t);
    assert!(t.s.context_choose(&t.g, "外部のファイルを曲のフォルダーにコピー"));
    assert_eq!(t.s.error, None);
    t.s.follow_browser();
    more(&mut t);
    let find = row_of(&t.s, "見つからないファイルを探す…");
    assert!(!find.enabled);
    assert_eq!(find.reason, "見つからないファイルはありません");
    let collect = row_of(&t.s, "外部のファイルを曲のフォルダーにコピー");
    assert!(!collect.enabled);
    assert_eq!(collect.reason, "外部のファイルはありません");
    assert!(row_of(&t.s, "曲のフォルダーをエクスプローラーで表示").enabled);
    assert!(t.s.context_choose(&t.g, "曲のフォルダーをエクスプローラーで表示"));
    assert_eq!(t.s.revealed.last(), Some(&t.dir()));
}

#[test]
fn pressing_a_clip_selects_the_row_of_its_file_without_playing_it() {
    let mut t = song("clip");
    let (x, y) = crate::arrange::probe::clip_mid(&t.s, &t.g, "ドラム", 2);
    t.s.arrange_down(&t.g, x, y, Mods::NONE);
    t.s.arrange_up(&t.g, x, y, Mods::NONE);
    t.s.follow_browser();
    assert_eq!(t.s.browser.selected_file, Some(t.path(MIX)));
    assert_eq!(t.s.browser.playing, None);
    // On another tab the press leaves the files tab's selection alone.
    t.s.browser.press_rail(RailTab::Instruments);
    let (x, y) = crate::arrange::probe::clip_mid(&t.s, &t.g, "ドラム", 0);
    t.s.arrange_down(&t.g, x, y, Mods::NONE);
    t.s.arrange_up(&t.g, x, y, Mods::NONE);
    t.s.follow_browser();
    assert_eq!(t.s.browser.selected_file, Some(t.path(MIX)));
}

#[test]
fn the_keys_go_from_row_to_row_fold_open_rename_and_open_the_menu() {
    let mut t = song("keys");
    t.s.input_map.browser_focus = true;
    assert!(key(&mut t, "down", Mods::NONE));
    assert_eq!(
        t.s.browser.pool_heading.as_deref(),
        Some("SongFiles/record")
    );
    assert!(key(&mut t, "down", Mods::NONE));
    assert_eq!(t.s.browser.selected_file, Some(t.path(VOCAL_1)));
    assert_eq!(
        t.s.browser.playing,
        Some(t.path(VOCAL_1)),
        "the keys play it"
    );
    // ← on a file goes up to its heading; ← again folds it; → opens it.
    assert!(key(&mut t, "left", Mods::NONE));
    assert_eq!(
        t.s.browser.pool_heading.as_deref(),
        Some("SongFiles/record")
    );
    assert!(key(&mut t, "left", Mods::NONE));
    assert!(pool_row(&t.s, &t.g, "Vocal_01.wav").is_none());
    assert!(key(&mut t, "down", Mods::NONE));
    assert_eq!(
        t.s.browser.pool_heading.as_deref(),
        Some("SongFiles/import"),
        "a folded heading is passed over to the next one"
    );
    assert!(key(&mut t, "up", Mods::NONE));
    assert!(key(&mut t, "right", Mods::NONE));
    assert!(pool_row(&t.s, &t.g, "Vocal_01.wav").is_some());
    assert!(key(&mut t, "down", Mods::NONE));
    // F2 asks for the new name in the row's menu.
    assert!(key(&mut t, "f2", Mods::NONE));
    assert_eq!(t.s.ask_field().map(|f| f.value.as_str()), Some("Vocal_01"));
    assert!(t.s.mapped_key(&t.g, "escape", None, Mods::NONE));
    t.s.popup = None;
    t.s.ctx = None;
    // Shift+F10 opens the selected file's menu.
    assert!(key(&mut t, "f10", Mods::shift()));
    assert!(t.s.context_open());
    assert_eq!(labels(&t.s)[0], "新しいトラックに置く");
    assert!(t.s.mapped_key(&t.g, "escape", None, Mods::NONE));
    assert!(!t.s.context_open());
    assert!(key(&mut t, "menu", Mods::NONE));
    assert!(t.s.context_open());
}

#[test]
fn the_tab_opens_from_the_view_command_the_browser_command_and_the_palette() {
    let mut t = song("open");
    t.s.browser.press_rail(RailTab::Instruments);
    assert!(t.s.run_binding(&t.g, "view.browser --tab song_files"));
    assert!(t.s.pool_shown());
    // The tab already open stays open.
    assert!(t.s.run_binding(&t.g, "view.browser --tab song_files"));
    assert!(t.s.pool_shown());
    t.s.browser.press_rail(RailTab::Effects);
    assert!(t.s.run_binding(&t.g, "browser.tab --tab song_files"));
    assert!(t.s.pool_shown());
    t.s.browser.press_rail(RailTab::Instruments);
    t.s.open_palette(&t.g);
    t.s.palette.as_mut().unwrap().replace_text(None, "プール");
    let p = t.s.palette.as_ref().unwrap();
    let hit = p
        .hits()
        .iter()
        .position(|&k| p.rows[k].label == "プール" && p.rows[k].icon == "pool")
        .expect("the palette's row");
    t.s.palette.as_mut().unwrap().select(hit);
    assert!(t.s.palette_key(&t.g, "enter", Mods::NONE));
    assert!(t.s.pool_shown());
}

#[test]
fn a_row_drags_onto_the_lanes_and_a_missing_one_does_not() {
    let mut t = song("drag");
    let tracks = t.s.project().tracks.len();
    let (x, y) = pool_mid(&t.s, &t.g, "Kick.wav");
    let hit = t.s.hit(&t.g, x, y);
    assert!(t.s.lib_grab(&t.g, hit, x, y, 1));
    let to = (t.g.lanes.x + t.g.lanes.w * 0.6, t.g.lanes.bottom() - 30.0);
    assert!(t.s.lib_move(to.0, to.1));
    assert!(t.s.lib_up(&t.g, to.0, to.1));
    assert_eq!(t.s.error, None);
    assert_eq!(t.s.project().tracks.len(), tracks + 1);
    assert_eq!(using(&t.s, KICK), 2);
    let (x, y) = pool_mid(&t.s, &t.g, "Bass.wav");
    let hit = t.s.hit(&t.g, x, y);
    t.s.lib_grab(&t.g, hit, x, y, 1);
    assert!(t.s.browser.file_drag.is_none());
}

/// The song's settings in the palette, and the row of `name` with its place.
fn settings_row(s: &Studio, g: &Geometry, name: &str) -> (usize, crate::studio::Rect) {
    let p = s.palette.as_ref().expect("the settings are open");
    let i = p
        .form
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.name == name)
        .unwrap();
    let r = p
        .layout(g.size)
        .rows
        .iter()
        .find(|(j, _)| *j == i)
        .unwrap()
        .1;
    (i, r)
}

fn value(s: &Studio, i: usize) -> String {
    s.palette.as_ref().unwrap().form.as_ref().unwrap().fields[i]
        .value
        .clone()
}

#[test]
fn the_settings_flip_the_copy_switch_pick_the_folder_and_apply_in_one_step() {
    let mut t = song("settings");
    assert!(t.s.open_palette_form("project.settings", json!({})));
    let (copy, r) = settings_row(&t.s, &t.g, "copy_imports");
    let was = value(&t.s, copy);
    assert!(t.s.palette_press(&t.g, r.x + 40.0, r.y + r.h / 2.0));
    assert_ne!(value(&t.s, copy), was, "the first press flips it");
    assert!(t.s.palette_press(&t.g, r.x + 40.0, r.y + r.h / 2.0));
    assert_eq!(value(&t.s, copy), was, "and the next flips it back");
    assert!(t.s.palette_press(&t.g, r.x + 40.0, r.y + r.h / 2.0));

    let (folder, r) = settings_row(&t.s, &t.g, "media_folder");
    let button = crate::palette_args::folder_button(r);
    let (bx, by) = (button.x + button.w / 2.0, button.y + button.h / 2.0);
    assert!(t.s.palette_hover(&t.g, bx, by) || t.s.palette.as_ref().unwrap().folder_hover);
    assert!(t.s.palette.as_ref().unwrap().folder_hover);
    assert!(t.s.palette_press(&t.g, bx, by));
    // Inside the song: the folder by its place in the song.
    let inside = t.dir().join("素材置き場");
    let asked = t.s.file_request.take();
    assert_eq!(asked, Some(FileRequest::MediaFolder));
    t.s.finish_picked(FileRequest::MediaFolder, std::slice::from_ref(&inside));
    assert_eq!(value(&t.s, folder), "素材置き場");
    // Outside it: the whole path.
    let outside = t.root.path().join("別の場所");
    t.s.finish_picked(FileRequest::MediaFolder, std::slice::from_ref(&outside));
    assert_eq!(value(&t.s, folder), outside.to_string_lossy());
    t.s.finish_picked(FileRequest::MediaFolder, &[inside]);

    let steps = t.s.song.steps();
    let last = t.s.palette.as_ref().unwrap().rows.len() - 1;
    let apply =
        t.s.palette
            .as_ref()
            .unwrap()
            .layout(t.g.size)
            .rows
            .iter()
            .find(|(j, _)| *j == last)
            .unwrap()
            .1;
    assert!(t.s.palette_press(&t.g, apply.x + 20.0, apply.y + apply.h / 2.0));
    assert_eq!(t.s.error, None);
    assert!(t.s.palette.is_none());
    assert_eq!(t.s.song.steps(), steps + 1);
    let media = t.s.project().media.clone();
    assert_eq!(media.folder, "素材置き場");
    assert_ne!(media.copy_imports.to_string(), was);
    // The files already made stay where they are.
    assert!(t.dir().join(VOCAL_1).is_file());
}
