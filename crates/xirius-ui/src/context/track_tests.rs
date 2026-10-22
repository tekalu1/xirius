//! The menus of the track column (S06 a track, S07 several rows, S08 a folder, S09 the
//! room under the last row), pressed as the window presses them.

use super::tests::{choose, enabled, key, labels, steps, studio};
use crate::arrange::probe::{folder, row_mid, track};
use crate::export::Group;
use crate::popup::{Look, PopupKind};
use crate::studio::{Geometry, Hit, Studio};
use crate::tree::RowKind;

fn press_row(s: &mut Studio, g: &Geometry, kind: RowKind) -> bool {
    let (x, y) = row_mid(s, g, kind);
    s.scene_context(g, x, y)
}

fn press_track(s: &mut Studio, g: &Geometry, name: &str) -> bool {
    let t = track(s, name);
    press_row(s, g, RowKind::Track(t))
}

fn press_folder(s: &mut Studio, g: &Geometry, name: &str) -> bool {
    let f = folder(s, name);
    press_row(s, g, RowKind::Folder(f))
}

/// Presses the `n`-th colour square of the open menu.
fn choose_swatch(s: &mut Studio, g: &Geometry, n: usize) -> bool {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .filter(|c| matches!(c.look, Look::Swatch(_)))
        .nth(n)
        .expect("a colour square");
    let hit = s.hit(
        g,
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    assert!(matches!(hit, Hit::Item(_)), "{hit:?}");
    s.press(g, hit)
}

/// The space just under the last row of the column.
fn under_the_rows(s: &Studio, g: &Geometry) -> (f32, f32) {
    let row = *s.tree().rows.last().expect("rows");
    let y = s.row_y(g, &row) + row.h + 12.0;
    assert!(y < g.column.bottom(), "no room under the rows");
    (g.column.x + 40.0, y)
}

/// Types a name into the box as the keyboard and the IME do.
fn type_name(s: &mut Studio, name: &str) {
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    s.ask_text(None, name, false);
}

fn name_of(s: &Studio, t: xirius_core::id::TrackId) -> String {
    s.project().tracks[&t].name.clone()
}

#[test]
fn a_track_has_its_rows_in_the_order_of_the_mock() {
    let (mut s, g) = studio();
    assert!(press_track(&mut s, &g, "Vocal"));
    assert_eq!(
        labels(&s),
        [
            "トラック名を変更",
            "色",
            "トラックを複製",
            "下にトラックを追加",
            "トラックのプリセットとして保存…",
            "トラックのプリセットを適用",
            "オートメーションを表示",
            "フォルダーにまとめる",
            "新しいトラックにバウンス",
            "トラックをフリーズ",
            "書き出す",
            "コピー",
            "トラックを削除",
        ]
    );
    // The press made the track the selection.
    assert_eq!(s.selected_row, Some(RowKind::Track(track(&s, "Vocal"))));
    // A row with rows under it shows no key.
    assert_eq!(key(&s, "書き出す"), "");
}

#[test]
fn renaming_asks_for_the_name_in_the_menu_and_one_undo_takes_it_back() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "トラック名を変更"));
    // The menu stays open as a box that starts with the name.
    assert!(s.context_open());
    assert_eq!(s.ask_field().map(|f| f.text()), Some("Vocal".to_string()));
    type_name(&mut s, "Lead");
    assert_eq!(s.popup.as_ref().unwrap().ask.as_deref(), Some("Lead"));
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    assert_eq!(name_of(&s, vocal), "Lead");
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(name_of(&s, vocal), "Vocal");
}

#[test]
fn escape_and_an_empty_name_leave_the_name_alone() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let n = steps(&s);
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "トラック名を変更"));
    type_name(&mut s, "Lead");
    assert_eq!(s.ask_key("escape"), Some(true));
    assert!(!s.context_open());
    assert!(s.ask_field().is_none());
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "トラック名を変更"));
    type_name(&mut s, "  ");
    s.ask_key("enter");
    assert!(!s.context_open());
    assert_eq!(name_of(&s, vocal), "Vocal");
    assert_eq!(steps(&s), n);
    // Without a box the keys are the screen's own.
    assert_eq!(s.ask_key("enter"), None);
}

#[test]
fn a_colour_is_one_step_for_the_track() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "色"));
    assert!(choose_swatch(&mut s, &g, 3));
    assert_eq!(steps(&s), n + 1);
    assert!(s.project().tracks[&vocal].color.is_some());
}

#[test]
fn duplicating_a_track_copies_its_clips_or_only_its_settings() {
    let (mut s, g) = studio();
    let count = |s: &Studio| s.project().tracks.len();
    let before = count(&s);
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "トラックを複製"));
    assert!(choose(&mut s, &g, "設定だけ"));
    assert_eq!(count(&s), before + 1);
    assert_eq!(steps(&s), n + 1);
    let p = s.project();
    let empty = p
        .tracks
        .values()
        .find(|t| t.name == "Vocal コピー")
        .expect("the copy");
    assert!(empty.clips.is_empty());
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "トラックを複製"));
    assert!(choose(&mut s, &g, "中身ごと"));
    assert_eq!(count(&s), before + 2);
    assert_eq!(steps(&s), n + 2);
}

#[test]
fn a_row_added_from_a_track_goes_right_under_it() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "下にトラックを追加"));
    assert_eq!(
        labels(&s),
        [
            "戻る",
            "オーディオ",
            "インストゥルメント",
            "フォルダー",
            "バス"
        ]
    );
    assert!(choose(&mut s, &g, "インストゥルメント"));
    assert_eq!(steps(&s), n + 1);
    let p = s.project();
    let (parent, at) = p.find_node(xirius_core::id::Node::Track(vocal)).unwrap();
    let next = p.children(parent)[at + 1];
    let xirius_core::id::Node::Track(made) = next else {
        panic!("a track follows: {next:?}");
    };
    assert_eq!(
        p.tracks[&made].kind,
        xirius_core::model::TrackKind::Instrument
    );
}

#[test]
fn the_lanes_menu_shows_a_lane_and_makes_it_when_the_track_has_none() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "オートメーションを表示"));
    let rows = labels(&s);
    assert_eq!(&rows[1..4], ["ボリューム", "パン", "ミュート"], "{rows:?}");
    let n = steps(&s);
    assert!(choose(&mut s, &g, "パン"));
    assert_eq!(steps(&s), n + 1);
    assert!(s.arrange.open.contains(&vocal));
    let lanes = s.arrange.lanes_of(&s.project().tracks[&vocal]);
    assert!(lanes.iter().any(|l| l.target.to_string() == "pan"));
    // The row of a lane that is on says so.
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "オートメーションを表示"));
    let on = s
        .context_rows()
        .into_iter()
        .find(|r| r.label == "パン")
        .unwrap()
        .on;
    assert!(on);
    // Choosing it again changes nothing in the song.
    let n = steps(&s);
    assert!(choose(&mut s, &g, "パン"));
    assert_eq!(steps(&s), n);
}

#[test]
fn packing_and_deleting_a_track_are_one_step_each() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    let folders = s.project().folders.len();
    assert!(choose(&mut s, &g, "フォルダーにまとめる"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().folders.len(), folders + 1);
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(choose(&mut s, &g, "トラックを削除"));
    assert_eq!(steps(&s), n + 2);
    assert!(!s.project().tracks.contains_key(&vocal));
    assert!(s.song.undo());
    assert!(s.project().tracks.contains_key(&vocal));
}

#[test]
fn the_stems_row_opens_the_export_with_the_track_picked() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_track(&mut s, &g, "Vocal"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "書き出す"));
    assert!(choose(&mut s, &g, "このトラックのステム…"));
    assert_eq!(s.popup.as_ref().map(|p| p.kind), Some(PopupKind::Export));
    assert!(s.export.stems);
    assert_eq!(s.export.group, Group::Selected);
    assert_eq!(s.export.picked, [vocal]);
    assert_eq!(steps(&s), n);
}

#[test]
fn several_rows_have_the_rows_that_fit_all_of_them() {
    let (mut s, g) = studio();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(guitar)];
    assert!(press_row(&mut s, &g, RowKind::Track(guitar)));
    assert_eq!(
        labels(&s),
        [
            "新しいバスへまとめる",
            "出力先を設定",
            "フォルダーにまとめる",
            "色",
            "トラックを複製",
            "ステムを書き出す…",
            "トラックを削除",
        ]
    );
    // The press kept the selection.
    assert_eq!(s.arrange.tracks.len(), 2);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "新しいバスへまとめる"));
    assert_eq!(steps(&s), n + 1);
    let p = s.project();
    assert!(p.tracks[&vocal].output.is_some());
    assert_eq!(p.tracks[&vocal].output, p.tracks[&guitar].output);
}

#[test]
fn a_press_on_a_row_outside_the_selection_makes_it_the_only_one() {
    let (mut s, g) = studio();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(guitar)];
    assert!(press_track(&mut s, &g, "Keys"));
    assert_eq!(s.arrange.tracks, [RowKind::Track(track(&s, "Keys"))]);
    assert!(labels(&s).contains(&"トラック名を変更".to_string()));
}

#[test]
fn the_output_of_several_rows_is_chosen_for_all_of_them_in_one_step() {
    let (mut s, g) = studio();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    s.arrange.tracks = vec![RowKind::Track(vocal), RowKind::Track(guitar)];
    assert!(press_row(&mut s, &g, RowKind::Track(vocal)));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "出力先を設定"));
    assert!(labels(&s).contains(&"マスター".to_string()));
    assert!(choose(&mut s, &g, "マスター"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(
        s.project().tracks[&guitar].output,
        Some(xirius_core::model::Output::Master)
    );
}

#[test]
fn a_folder_has_its_rows_in_the_order_of_the_mock() {
    let (mut s, g) = studio();
    assert!(press_folder(&mut s, &g, "Rhythm"));
    assert_eq!(
        labels(&s),
        [
            "フォルダー名を変更",
            "色",
            "中のトラックにも塗る",
            "中のトラックを選択",
            "フォルダーを複製",
            "ステムを書き出す",
            "まとめを解く",
            "フォルダーを削除",
        ]
    );
}

#[test]
fn a_folder_is_renamed_in_the_menu() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    assert!(press_folder(&mut s, &g, "Rhythm"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "フォルダー名を変更"));
    assert_eq!(s.ask_field().map(|f| f.text()), Some("Rhythm".to_string()));
    type_name(&mut s, "Beats");
    s.ask_key("enter");
    assert_eq!(s.project().folders[&f].name, "Beats");
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn choosing_the_tracks_inside_a_folder_selects_them() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    let nested = s
        .song
        .run(
            "folder.add",
            serde_json::json!({"name":"Inner","into":f.to_string()}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.song
        .run(
            "track.add.audio",
            serde_json::json!({"name":"Extra","into":nested}),
        )
        .unwrap();
    s.rebuild(&g);
    assert!(press_folder(&mut s, &g, "Rhythm"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "中のトラックを選択"));
    assert_eq!(steps(&s), n);
    let inner = s.project().tracks_in(f);
    assert_eq!(
        s.arrange.tracks,
        inner.iter().map(|t| RowKind::Track(*t)).collect::<Vec<_>>()
    );
    assert_eq!(
        s.song.session().state.selection,
        inner
            .iter()
            .map(|t| xirius_core::id::AnyId::Track(*t))
            .collect::<Vec<_>>()
    );
    for t in inner {
        assert!(s.arrange.tracks.contains(&RowKind::Track(t)));
    }
    // With the rows lit, the next press offers the menu of several rows.
    let drums = track(&s, "Drums");
    assert!(press_row(&mut s, &g, RowKind::Track(drums)));
    assert!(labels(&s).contains(&"新しいバスへまとめる".to_string()));
}

#[test]
fn a_folders_colour_can_go_to_the_tracks_inside_too() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    assert!(press_folder(&mut s, &g, "Rhythm"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "中のトラックにも塗る"));
    assert!(choose_swatch(&mut s, &g, 2));
    assert_eq!(steps(&s), n + 1);
    let p = s.project();
    assert!(p.folders[&f].color.is_some());
    for t in p.tracks_in(f) {
        assert_eq!(p.tracks[&t].color, p.folders[&f].color);
    }
}

#[test]
fn a_folder_is_duplicated_unpacked_and_deleted() {
    let (mut s, g) = studio();
    let tracks = s.project().tracks.len();
    let folders = s.project().folders.len();
    assert!(press_folder(&mut s, &g, "Latin"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "フォルダーを複製"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().folders.len(), folders + 1);
    assert_eq!(s.project().tracks.len(), tracks + 1);

    assert!(press_folder(&mut s, &g, "Latin"));
    assert!(choose(&mut s, &g, "まとめを解く"));
    assert_eq!(steps(&s), n + 2);
    assert_eq!(s.project().folders.len(), folders);
    assert_eq!(s.project().tracks.len(), tracks + 1);

    assert!(press_folder(&mut s, &g, "Percussion"));
    assert!(choose(&mut s, &g, "フォルダーを削除"));
    assert_eq!(labels(&s), ["戻る", "フォルダーだけ", "中のトラックごと"]);
    assert!(choose(&mut s, &g, "中のトラックごと"));
    assert_eq!(steps(&s), n + 3);
    assert!(s.project().tracks.len() < tracks + 1);
}

#[test]
fn the_stems_of_a_folder_are_the_tracks_inside() {
    let (mut s, g) = studio();
    let f = folder(&s, "Rhythm");
    assert!(press_folder(&mut s, &g, "Rhythm"));
    assert!(choose(&mut s, &g, "ステムを書き出す"));
    assert!(choose(&mut s, &g, "中のトラックごと…"));
    assert_eq!(s.export.picked, s.project().tracks_in(f));
    assert!(s.export.stems);
}

#[test]
fn the_room_under_the_last_row_adds_pastes_and_folds() {
    let (mut s, g) = studio();
    let (x, y) = under_the_rows(&s, &g);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["トラックを追加", "貼り付け", "フォルダー"]);
    assert_eq!(key(&s, "貼り付け"), "Ctrl+V");
    // Nothing was copied.
    assert!(!enabled(&s, "貼り付け"));
    let n = steps(&s);
    let tracks = s.project().tracks.len();
    assert!(choose(&mut s, &g, "トラックを追加"));
    assert!(choose(&mut s, &g, "バス"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().tracks.len(), tracks + 1);
}

#[test]
fn the_folders_row_folds_every_folder_and_opens_them_again() {
    let (mut s, g) = studio();
    let (x, y) = under_the_rows(&s, &g);
    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "フォルダー"));
    assert!(choose(&mut s, &g, "すべて畳む"));
    assert_eq!(steps(&s), n);
    let all: Vec<_> = s.project().folders.keys().copied().collect();
    assert!(all.iter().all(|f| s.is_collapsed(*f)));
    // The rows shrank; press the room under them now.
    let (x, y) = under_the_rows(&s, &g);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "フォルダー"));
    assert!(choose(&mut s, &g, "すべて開く"));
    assert!(all.iter().all(|f| !s.is_collapsed(*f)));
}

#[test]
fn a_press_on_a_row_does_not_open_the_menu_of_the_room_under_the_rows() {
    let (mut s, g) = studio();
    let (x, y) = row_mid(&s, &g, RowKind::Track(track(&s, "Vocal")));
    assert!(s.scene_context(&g, x, y));
    assert!(!labels(&s).contains(&"貼り付け".to_string()));
}

#[test]
fn the_snowflake_freezes_the_track_on_a_thread_and_lets_it_go() {
    let (mut s, g) = studio();
    let vocal = crate::arrange::probe::track(&s, "Vocal");
    let hit = Hit::Freeze(vocal);
    let (x, y) = s.center_of(&g, hit).expect("on screen");
    assert_eq!(s.hit(&g, x, y), hit);
    let n = steps(&s);
    assert!(s.press(&g, hit));
    assert!(s.job.is_some() || s.error.is_some(), "{:?}", s.error);
    s.job_wait();
    assert_eq!(s.error, None);
    assert!(s.project().tracks[&vocal].frozen.is_some());
    assert_eq!(steps(&s), n + 1);
    // The menu now lets it go.
    assert!(press_track(&mut s, &g, "Vocal"));
    assert!(labels(&s).contains(&"フリーズを解除".to_string()));
    s.popup = None;
    assert!(s.press(&g, hit));
    assert!(s.project().tracks[&vocal].frozen.is_none());
}

fn mark_of(s: &Studio, g: &Geometry, label: &str) -> Option<usize> {
    s.popup
        .as_ref()
        .expect("a menu is open")
        .layout(g.size)
        .cells
        .iter()
        .find(|c| c.text == label)
        .unwrap_or_else(|| panic!("no row {label}"))
        .mark
}

#[test]
fn the_colour_row_shows_the_tracks_colour_and_its_squares_check_the_current_one() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let now = crate::draw::track_color(&s.project().tracks[&vocal]);
    assert!(press_track(&mut s, &g, "Vocal"));
    assert_eq!(mark_of(&s, &g, "色"), Some(now));
    assert_eq!(mark_of(&s, &g, "トラック名を変更"), None);
    assert!(choose(&mut s, &g, "色"));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let squares: Vec<_> = layout
        .cells
        .iter()
        .filter(|c| matches!(c.look, Look::Swatch(_)))
        .collect();
    assert_eq!(squares.len(), 16);
    let checked: Vec<_> = squares.iter().filter(|c| c.on).collect();
    assert_eq!(checked.len(), 1);
    assert_eq!(checked[0].look, Look::Swatch(Some(now)));
    // Two rows of eight, all inside the menu.
    assert_eq!(squares[0].rect.y, squares[7].rect.y);
    assert!(squares[8].rect.y > squares[7].rect.y);
    assert_eq!(squares[8].rect.y, squares[15].rect.y);
    assert!(
        squares
            .iter()
            .all(|c| c.rect.right() <= layout.rect.right())
    );
    // Choosing another colour moves the mark when the menu is opened again.
    assert!(choose_swatch(&mut s, &g, (now + 5) % 16));
    assert!(press_track(&mut s, &g, "Vocal"));
    assert_eq!(mark_of(&s, &g, "色"), Some((now + 5) % 16));
}
