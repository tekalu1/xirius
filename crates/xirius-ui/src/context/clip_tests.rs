//! The clip menus (S02 audio, S03 MIDI, S04 several), pressed as the window presses them.

use serde_json::json;

use super::tests::{choose, enabled, key, labels, steps, studio};
use crate::arrange::probe::{clip_mid, track};
use crate::popup::Look;
use crate::studio::{DrawerTab, Geometry, Hit, Studio};

/// Saves the song in a folder of its own and writes a tone where the Vocal clip's file is.
fn with_sound(s: &mut Studio, name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!("target/ctx-audio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let t = track(s, "Vocal");
    let p = s.project();
    let xirius_core::model::ClipBody::Audio { file, .. } = &p.tracks[&t].clips[0].body else {
        panic!("audio clip");
    };
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let wrote = crate::drop_audio::tests::tone(
        path.parent().unwrap(),
        path.file_name().unwrap().to_str().unwrap(),
        4.0,
    );
    assert_eq!(wrote, path);
}

fn press_on(s: &mut Studio, g: &Geometry, name: &str, i: usize) -> bool {
    // A menu still open could cover the clip, as the window's press would close it first.
    s.popup = None;
    let (x, y) = clip_mid(s, g, name, i);
    s.scene_context(g, x, y)
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

#[test]
fn a_midi_clip_has_its_own_rows_in_the_order_of_the_mock() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert_eq!(
        labels(&s),
        [
            "トランスポーズ",
            "ピアノロールで開く",
            "クリップ名を変更",
            "クリップを分割",
            "クリップを複製",
            "クリップをミュート",
            "ループ範囲を設定",
            "色",
            "クオンタイズ",
            "バウンス",
            "選択範囲を書き出す…",
            "MIDI ファイルとして書き出す…",
            "切り取り",
            "コピー",
            "クリップを削除",
        ]
    );
    assert_eq!(key(&s, "クリップを複製"), "D");
    assert_eq!(key(&s, "コピー"), "Ctrl+C");
    assert_eq!(key(&s, "クリップを削除"), "Del / Backspace");
    // The press made the clip the selection.
    assert_eq!(s.arrange.clips.len(), 1);
}

#[test]
fn an_audio_clip_has_the_fade_and_gain_rows_dim_until_there_is_something_to_take_back() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let rows = labels(&s);
    assert!(rows.contains(&"フェードを削除".to_string()), "{rows:?}");
    assert!(rows.contains(&"ゲインを戻す".to_string()));
    assert!(!rows.contains(&"ピアノロールで開く".to_string()));
    assert!(!rows.contains(&"クオンタイズ".to_string()));
    assert!(!enabled(&s, "フェードを削除"));
    assert!(!enabled(&s, "ゲインを戻す"));
    let (_, c) = s.selected_clip.unwrap();
    let id = c.to_string();
    s.arrange_run("clip.fade.set", json!({ "ids": [id], "in": 240 }));
    s.arrange_run("clip.gain.set", json!({ "ids": [id], "db": -6 }));
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(enabled(&s, "フェードを削除"));
    assert!(enabled(&s, "ゲインを戻す"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "フェードを削除"));
    assert_eq!(steps(&s), n + 1);
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(!enabled(&s, "フェードを削除"));
    assert!(choose(&mut s, &g, "ゲインを戻す"));
    assert_eq!(steps(&s), n + 2);
}

#[test]
fn mute_is_one_step_and_shows_a_check_once_the_clip_is_muted() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    assert!(press_on(&mut s, &g, "Keys", 0));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "クリップをミュート"));
    assert_eq!(steps(&s), n + 1);
    assert!(s.project().tracks[&keys].clips[0].muted);
    assert!(press_on(&mut s, &g, "Keys", 0));
    let row = s
        .context_rows()
        .into_iter()
        .find(|r| r.label == "クリップをミュート")
        .unwrap();
    assert!(row.on);
    s.undo(&g);
    assert!(!s.project().tracks[&keys].clips[0].muted);
}

#[test]
fn split_cuts_the_clip_where_the_menu_was_opened() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let before = s.project().tracks[&keys].clips.len();
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    let at = s.ctx_tick(&g, x);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "クリップを分割"));
    assert_eq!(steps(&s), n + 1);
    let clips = &s.project().tracks[&keys].clips;
    assert_eq!(clips.len(), before + 1);
    assert!(clips.iter().any(|c| c.start == at), "{at}");
}

#[test]
fn the_loop_row_takes_the_span_of_the_clips() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let (start, end) = {
        let c = &s.project().tracks[&keys].clips[0];
        (c.start, c.end())
    };
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "ループ範囲を設定"));
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (start, end));
}

#[test]
fn a_colour_square_colours_the_clip_and_the_first_row_gives_it_back() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "色"));
    assert_eq!(labels(&s)[..2], ["戻る", "トラックの色"]);
    let n = steps(&s);
    assert!(choose_swatch(&mut s, &g, 3));
    assert_eq!(steps(&s), n + 1);
    assert!(s.project().tracks[&keys].clips[0].color.is_some());
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "色"));
    assert!(choose(&mut s, &g, "トラックの色"));
    assert_eq!(s.project().tracks[&keys].clips[0].color, None);
}

#[test]
fn the_transpose_field_sets_the_clips_value_and_the_notes_stay_as_written() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let pitches = |s: &Studio| -> Vec<u8> {
        s.project().tracks[&keys].clips[0]
            .notes()
            .iter()
            .map(|n| n.pitch)
            .collect()
    };
    let semis = |s: &Studio| s.project().tracks[&keys].clips[0].transpose();
    let before = pitches(&s);
    assert!(!before.is_empty());
    assert_eq!(semis(&s), 0);
    assert!(press_on(&mut s, &g, "Keys", 0));
    // The field comes first, as on an audio clip, and the write row sits with the note
    // operations inside 「クオンタイズ」, where it cannot be mistaken for the field (UI-313).
    let rows = labels(&s);
    assert_eq!(rows[..2], ["トランスポーズ", "ピアノロールで開く"]);
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert_eq!(
        labels(&s),
        [
            "戻る",
            "今のグリッド",
            "50%",
            "元に戻す",
            "トランスポーズをノートに反映"
        ]
    );
    assert!(!enabled(&s, "トランスポーズをノートに反映"));
    assert!(choose(&mut s, &g, "戻る"));

    // 12 typed into the field is the clip's value: the notes do not move.
    let field = s
        .context_rows()
        .iter()
        .position(|r| r.label == "トランスポーズ")
        .expect("the field");
    assert!(s.context_pick(&g, field));
    s.ask_text(None, "12", false);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(semis(&s), 12);
    assert_eq!(pitches(&s), before);
    let said = s
        .context_rows()
        .into_iter()
        .find_map(|r| r.field.map(|f| f.text))
        .expect("the field shows the clip's value");
    assert_eq!(said, "+12");
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert!(enabled(&s, "トランスポーズをノートに反映"));

    // Writing it into the notes keeps the sound and clears the value; one undo brings both back.
    let n = steps(&s);
    assert!(choose(&mut s, &g, "トランスポーズをノートに反映"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(semis(&s), 0);
    assert!(before.iter().zip(pitches(&s)).all(|(a, b)| b == a + 12));
    s.undo(&g);
    assert_eq!(semis(&s), 12);
    assert_eq!(pitches(&s), before);
}

/// The steps of the screenshot of the feel (f07a..f07c): the field shows the digits as typed,
/// Enter leaves the menu open with the field showing the clip's value, as an audio clip's
/// fields do, and then one Escape closes it. The window's simulated Enter types 「\n」 after the
/// key, and that leaves the value shown.
#[test]
fn the_transpose_field_shows_what_was_typed_and_the_menu_stays_after_enter() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let said = |s: &Studio| {
        s.context_rows()
            .into_iter()
            .find_map(|r| r.field.map(|f| f.text))
    };
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert_eq!(said(&s).as_deref(), Some("0"));
    let field = s
        .context_rows()
        .iter()
        .position(|r| r.label == "トランスポーズ")
        .expect("the field");
    assert!(s.context_pick(&g, field));
    s.ask_text(None, "5", false);
    assert_eq!(said(&s).as_deref(), Some("5"), "as typed");
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    s.ask_text(None, "\n", false);
    assert!(s.context_open(), "the menu stays");
    assert_eq!(said(&s).as_deref(), Some("+5"));
    assert!(s.field_typing().is_some_and(|t| t.fresh));
    assert_eq!(s.project().tracks[&keys].clips[0].transpose(), 5);

    // Nothing is half typed now: one Escape closes the menu (P2-1 of review 15).
    assert_eq!(s.field_key(&g, "escape", None, false), Some(true));
    assert!(!s.context_open());
}

#[test]
fn the_open_row_opens_the_piano_roll_with_the_clip() {
    let (mut s, g) = studio();
    s.drawer_open = false;
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "ピアノロールで開く"));
    assert!(s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Roll);
}

#[test]
fn several_clips_get_the_rows_that_fit_all_of_them() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    let (kt, kc) = s.selected_clip.unwrap();
    let vt = track(&s, "Vocal");
    let vc = s.project().tracks[&vt].clips[0].id;
    s.arrange.clips.insert((vt, vc));
    // A press on one of the selected clips keeps the selection.
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert_eq!(s.arrange.clips.len(), 2);
    let rows = labels(&s);
    // Audio and MIDI together: no row that fits only one kind, none for a single clip. The
    // transpose field fits both, so it is there; writing it into the notes is for MIDI only.
    for gone in [
        "クリップを分割",
        "ピアノロールで開く",
        "クオンタイズ",
        "フェードを削除",
        "トランスポーズをノートに反映",
    ] {
        assert!(!rows.contains(&gone.to_string()), "{gone}");
    }
    assert_eq!(rows[0], "トランスポーズ");
    assert!(rows.contains(&"クリップを複製".to_string()));

    // Values that differ show 「—」; a value typed there goes to every clip of the selection.
    s.command(|song| {
        song.run(
            "clip.transpose",
            json!({ "ids": [vc.to_string()], "semis": 2 }),
        )
        .unwrap();
        song.run(
            "clip.transpose",
            json!({ "ids": [kc.to_string()], "semis": 3 }),
        )
        .map(|_| ())
    });
    assert!(press_on(&mut s, &g, "Keys", 0));
    let said = |s: &Studio| {
        s.context_rows()
            .into_iter()
            .find_map(|r| r.field.map(|f| f.text))
            .expect("the transpose field")
    };
    assert_eq!(said(&s), "—");
    let field = s
        .context_rows()
        .iter()
        .position(|r| r.label == "トランスポーズ")
        .expect("the field");
    assert!(s.context_pick(&g, field));
    s.ask_text(None, "5", false);
    assert_eq!(s.field_key(&g, "enter", None, false), Some(true));
    assert_eq!(said(&s), "+5");
    let p = s.project();
    let semis = |t: xirius_core::id::TrackId, id: xirius_core::id::ClipId| {
        let c = p.tracks[&t].clips.iter().find(|c| c.id == id).unwrap();
        match &c.body {
            xirius_core::model::ClipBody::Midi { transpose, .. } => i32::from(*transpose),
            xirius_core::model::ClipBody::Audio { process, .. } => process.semis,
        }
    };
    assert_eq!((semis(kt, kc), semis(vt, vc)), (5, 5));

    let n = steps(&s);
    assert!(choose(&mut s, &g, "クリップをミュート"));
    assert_eq!(steps(&s), n + 1);
    let p = s.project();
    assert!(
        p.tracks[&kt]
            .clips
            .iter()
            .find(|c| c.id == kc)
            .unwrap()
            .muted
    );
    assert!(
        p.tracks[&vt]
            .clips
            .iter()
            .find(|c| c.id == vc)
            .unwrap()
            .muted
    );
}

#[test]
fn a_press_on_a_clip_that_is_not_selected_makes_it_the_only_one() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Keys", 0));
    s.press(&g, Hit::None);
    assert!(!s.context_open());
    let vt = track(&s, "Vocal");
    let vc = s.project().tracks[&vt].clips[0].id;
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert_eq!(s.arrange.clips.len(), 1);
    assert!(s.arrange.clips.contains(&(vt, vc)));
    assert_eq!(s.selected_clip, Some((vt, vc)));
}

#[test]
fn audio_and_midi_names_are_entered_from_the_menu_and_undo_once() {
    let (mut s, g) = studio();
    // Choosing a clip selects its track; the instrument-input arm that would
    // follow is not what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    for name in ["Vocal", "Keys"] {
        let before = s.project();
        assert!(press_on(&mut s, &g, name, 0));
        assert!(choose(&mut s, &g, "クリップ名を変更"));
        for _ in 0..60 {
            s.ask_key("backspace");
        }
        s.ask_text(None, "Lead", false);
        s.ask_key("enter");
        assert_eq!(s.project().tracks[&track(&s, name)].clips[0].name, "Lead");
        s.undo(&g);
        assert_eq!(s.project(), before);
    }
}

#[test]
fn quantize_restore_returns_to_the_original_timing_and_undo_once() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].id;
    let note = s.project().clip(c).unwrap().1.notes()[0].id;
    s.song
        .run("note.move", json!({"ids":[note.to_string()],"by":137}))
        .unwrap();
    let original = s.project().clip(c).unwrap().1.notes()[0].start;
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert!(choose(&mut s, &g, "今のグリッド"));
    let before = s.project();
    assert_ne!(before.clip(c).unwrap().1.notes()[0].start, original);
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert!(choose(&mut s, &g, "元に戻す"));
    assert_eq!(s.project().clip(c).unwrap().1.notes()[0].start, original);
    s.undo(&g);
    assert_eq!(s.project(), before);
}

#[test]
fn joining_adjacent_midi_clips_from_the_menu_is_one_undo() {
    let (mut s, g) = studio();
    // Choosing a clip selects its track; the instrument-input arm that would
    // follow is not what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let t = track(&s, "Keys");
    let mut clips = Vec::new();
    for at in [12 * super::tests::BAR, 13 * super::tests::BAR] {
        let result = s
            .song
            .run(
                "clip.add.midi",
                json!({"track":t.to_string(),"at":at,"bars":1}),
            )
            .unwrap();
        clips.push(result["id"].as_str().unwrap().parse().unwrap());
    }
    s.rebuild(&g);
    s.arrange.clips = clips.iter().map(|id| (t, *id)).collect();
    let before = s.project();
    let i = before.tracks[&t]
        .clips
        .iter()
        .position(|c| c.id == clips[0])
        .unwrap();
    assert!(press_on(&mut s, &g, "Keys", i));
    assert!(enabled(&s, "クリップを結合"));
    assert!(choose(&mut s, &g, "クリップを結合"));
    assert_eq!(
        s.project().tracks[&t].clips.len(),
        before.tracks[&t].clips.len() - 1
    );
    s.undo(&g);
    assert_eq!(s.project(), before);
}

#[test]
fn right_pressing_a_clip_releases_old_automation_point_selection() {
    let (mut s, g) = studio();
    let t = crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
    let (x, y) = crate::arrange::probe::point_xy(&s, &g, t, 0, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(!s.arrange.points.is_empty());
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(s.arrange.points.is_empty());
    assert!(choose(&mut s, &g, "コピー"));
    assert!(matches!(
        s.song.session().state.clipboard,
        Some(xirius_core::clipboard::Clipboard::Clips(_))
    ));
    let before = s.project();
    assert!(s.mapped_key(&g, "delete", None, crate::tools::Mods::NONE));
    assert_eq!(
        s.project().tracks[&t].automation,
        before.tracks[&t].automation
    );
    s.undo(&g);
    assert_eq!(s.project(), before);
}

#[test]
fn an_audio_clips_reveal_row_resolves_the_file_from_the_song_folder() {
    let (mut s, g) = studio();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = root.join(format!(
        "target/ctx2-reveal-{}",
        crate::shot::fixture_run_id()
    ));
    s.command(|song| song.save_as(&dir));
    assert!(s.error.is_none(), "{:?}", s.error);
    let t = track(&s, "Vocal");
    let p = s.project();
    let xirius_core::model::ClipBody::Audio { file, .. } = &p.tracks[&t].clips[0].body else {
        panic!("audio clip");
    };
    let path = std::path::PathBuf::from(file);
    let expected = if path.is_absolute() {
        path
    } else {
        dir.join(path)
    };
    let n = steps(&s);
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(choose(&mut s, &g, "エクスプローラーで表示"));
    assert_eq!(s.revealed, vec![expected]);
    assert_eq!(steps(&s), n);
}

/// Shift+F10 and the menu key open the selected clip's menu, as a press on the clip would.
#[test]
fn shift_f10_and_the_menu_key_open_the_selected_clips_menu() {
    let (mut s, g) = studio();
    s.selected_clip = None;
    s.arrange.clips.clear();
    assert!(!s.mapped_key(&g, "f10", None, crate::tools::Mods::shift()));
    assert!(s.popup.is_none());
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let by_mouse = labels(&s);
    assert!(s.mapped_key(&g, "escape", None, crate::tools::Mods::NONE));
    assert!(s.popup.is_none());
    for (key, mods) in [
        ("f10", crate::tools::Mods::shift()),
        ("menu", crate::tools::Mods::NONE),
    ] {
        assert!(s.mapped_key(&g, key, None, mods), "{key}");
        assert_eq!(labels(&s), by_mouse, "{key}");
        assert!(choose(&mut s, &g, "曲のテンポに合わせる"), "{key}");
        s.undo(&g);
    }
}

#[test]
fn an_audio_clip_starts_with_following_the_tempo_and_groups_processing_and_bouncing() {
    let (mut s, g) = studio();
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let rows = labels(&s);
    assert_eq!(rows[4], "曲のテンポに合わせる", "{rows:?}");
    assert!(rows.contains(&"処理".to_string()), "{rows:?}");
    assert!(rows.contains(&"バウンス".to_string()), "{rows:?}");
    assert!(choose(&mut s, &g, "処理"));
    assert_eq!(
        labels(&s),
        ["戻る", "ノーマライズ", "逆再生", "テンポを検出"]
    );
    assert_eq!(key(&s, "逆再生"), "Ctrl+R");
    assert_eq!(key(&s, "ノーマライズ"), "Alt+N");
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(choose(&mut s, &g, "バウンス"));
    assert_eq!(
        labels(&s),
        ["戻る", "クリップをバウンス", "新しいトラックにバウンス"]
    );
    assert_eq!(key(&s, "クリップをバウンス"), "Ctrl+B");
    assert_eq!(key(&s, "新しいトラックにバウンス"), "Ctrl+Alt+B");
}

#[test]
fn following_the_tempo_is_one_step_and_shows_a_check() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "曲のテンポに合わせる"));
    assert_eq!(steps(&s), n + 1);
    let follows = |s: &Studio| match &s.project().tracks[&vocal].clips[0].body {
        xirius_core::model::ClipBody::Audio { process, .. } => process.follow,
        _ => unreachable!(),
    };
    assert!(follows(&s));
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let row = s
        .context_rows()
        .into_iter()
        .find(|r| r.label == "曲のテンポに合わせる")
        .unwrap();
    assert!(row.on);
    // A second choice lets the clip go of the tempo.
    assert!(choose(&mut s, &g, "曲のテンポに合わせる"));
    assert!(!follows(&s));
    s.undo(&g);
    assert!(follows(&s));
}

#[test]
fn reversing_from_the_menu_is_one_step_and_a_frozen_track_offers_to_unfreeze() {
    let (mut s, g) = studio();
    with_sound(&mut s, "reverse");
    let vocal = track(&s, "Vocal");
    let reversed = |s: &Studio| match &s.project().tracks[&vocal].clips[0].body {
        xirius_core::model::ClipBody::Audio { process, .. } => process.reverse,
        _ => unreachable!(),
    };
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "処理"));
    assert!(choose(&mut s, &g, "逆再生"), "{:?}", s.error);
    assert_eq!(steps(&s), n + 1);
    assert!(reversed(&s));
    // Frozen: the processing and the bounce in place are dim, and say why.
    s.song
        .run(
            "track.freeze.apply",
            json!({ "tracks": [{ "id": vocal.to_string(), "file": "frozen.wav", "start": 0 }] }),
        )
        .unwrap();
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(!enabled(&s, "処理"));
    let why = |s: &Studio, label: &str| {
        s.context_rows()
            .into_iter()
            .find(|r| r.label == label)
            .unwrap()
            .reason
    };
    assert_eq!(why(&s, "処理"), crate::toast::FROZEN);
    assert!(choose(&mut s, &g, "バウンス"));
    assert!(!enabled(&s, "クリップをバウンス"));
    assert_eq!(why(&s, "クリップをバウンス"), crate::toast::FROZEN);
    assert!(enabled(&s, "新しいトラックにバウンス"));
    s.popup = None;
    // The key still reaches the command: the refusal offers to unfreeze.
    let id = s.project().tracks[&vocal].clips[0].id.to_string();
    s.arrange_run("clip.reverse", json!({ "ids": [id] }));
    let toast = s.toast_shown().expect("a toast");
    assert_eq!(toast.text, crate::toast::FROZEN);
    assert_eq!(toast.button.as_ref().unwrap().0, "フリーズを解除");
    s.toast_answer();
    assert!(s.project().tracks[&vocal].frozen.is_none());
}

#[test]
fn a_clip_bounce_from_the_menu_runs_on_a_thread_and_lands_as_one_step() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let before = s.project().tracks[&vocal].clips.len();
    let tracks = s.project().tracks.len();
    assert!(press_on(&mut s, &g, "Vocal", 0));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "バウンス"));
    assert!(choose(&mut s, &g, "新しいトラックにバウンス"));
    assert!(s.job.is_some() || s.error.is_some(), "{:?}", s.error);
    s.job_wait();
    assert!(s.job.is_none());
    assert_eq!(s.error, None);
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().tracks.len(), tracks + 1);
    assert_eq!(s.project().tracks[&vocal].clips.len(), before);
    s.undo(&g);
    assert_eq!(s.project().tracks.len(), tracks);
}

#[test]
fn a_range_bounces_from_its_own_menu_and_from_the_key() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let c = s.project().tracks[&keys].clips[0].clone();
    s.arrange.range = Some((c.start, c.start + c.length));
    s.arrange.clips.insert((keys, c.id));
    // On a clip in the range, its バウンス is the range's.
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "バウンス"));
    assert_eq!(
        labels(&s),
        ["戻る", "選択範囲をバウンス", "新しいトラックにバウンス"]
    );
    s.popup = None;
    // Ctrl+B with a range bounces the range, not the clip.
    let (name, args) = s.bounce_instead(false).unwrap();
    assert_eq!(name, "range.bounce");
    assert_eq!(args["tracks"], json!([keys.to_string()]));
    let (_, each) = s.bounce_instead(true).unwrap();
    assert_eq!(each["per-track"], json!(true));
    // No range and no clip: the selected track.
    s.arrange.range = None;
    s.arrange.clips.clear();
    s.selected_row = Some(crate::tree::RowKind::Track(keys));
    assert_eq!(s.bounce_instead(false).unwrap().0, "track.bounce");
}

#[test]
fn the_square_on_a_bouncing_clip_stops_the_bounce_and_leaves_the_song_alone() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let c = s.project().tracks[&vocal].clips[0].clone();
    let n = steps(&s);
    let tracks = s.project().tracks.len();
    let next = |s: &Studio| {
        s.tree()
            .rows
            .iter()
            .position(|r| r.kind == crate::tree::RowKind::Track(vocal))
            .unwrap()
            + 1
    };
    let below = s.tree().rows[next(&s)].y;
    s.arrange_run(
        "clip.bounce",
        json!({ "ids": [c.id.to_string()], "new-track": true }),
    );
    assert!(s.job.is_some(), "{:?}", s.error);
    // The new track has its place below Vocal, a track high, and the rows under it moved down.
    let row = *s.tree().row_of(crate::tree::RowKind::Track(vocal)).unwrap();
    assert_eq!(row.job_h, s.arrange.track_h);
    assert_eq!(s.tree().rows[next(&s)].y, below + s.arrange.track_h);
    let places = crate::bounce_row::places(&s, &row);
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].planned.name, "Vocal バウンス");
    // Its clip is where Vocal's is, on the new row; the progress is there, not on Vocal's clip.
    let made = crate::bounce_row::clip_rects(&s, &g, &places[0])[0];
    let from = s.clip_rect(&g, &row, &c);
    assert_eq!((made.x, made.w), (from.x, from.w));
    assert!(made.y >= from.bottom());
    assert!(s.job_on_clip(c.id).is_none());
    let old = crate::clip_marks::stop_rect(from).expect("wide enough");
    assert!(!s.job_stop_press(&g, old.x + old.w / 2.0, old.y + old.h / 2.0));
    let stop = crate::bounce_row::stop_rects(&s, &g);
    assert_eq!(
        stop,
        vec![crate::clip_marks::stop_rect(made).expect("wide enough")]
    );
    let stop = stop[0];
    // Off the square, the press is not the job's.
    assert!(!s.job_stop_press(&g, stop.x - 30.0, stop.y + 2.0));
    assert!(s.job_stop_press(&g, stop.x + stop.w / 2.0, stop.y + stop.h / 2.0));
    s.job_wait();
    assert!(s.job.is_none());
    assert_eq!(steps(&s), n);
    assert_eq!(s.project().tracks.len(), tracks);
    assert!(s.toast_shown().is_none());
    // The place closed.
    assert_eq!(
        s.tree()
            .row_of(crate::tree::RowKind::Track(vocal))
            .unwrap()
            .job_h,
        0.0
    );
    assert_eq!(s.tree().rows[next(&s)].y, below);
}

#[test]
fn a_bounce_in_place_shows_on_its_clip_and_makes_no_row() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let c = s.project().tracks[&vocal].clips[0].clone();
    s.arrange_run("clip.bounce", json!({ "ids": [c.id.to_string()] }));
    assert!(s.job_on_clip(c.id).is_some(), "{:?}", s.error);
    let row = *s.tree().row_of(crate::tree::RowKind::Track(vocal)).unwrap();
    assert_eq!(row.job_h, 0.0);
    assert!(crate::bounce_row::stop_rects(&s, &g).is_empty());
    let stop = crate::clip_marks::stop_rect(s.clip_rect(&g, &row, &c)).expect("wide enough");
    assert!(s.job_stop_press(&g, stop.x + stop.w / 2.0, stop.y + stop.h / 2.0));
    s.job_wait();
    assert!(s.job.is_none());
}

#[test]
fn a_normalize_raises_the_gain_and_says_by_how_much_over_the_clip_for_a_moment() {
    let (mut s, g) = studio();
    with_sound(&mut s, "normalize");
    let vocal = track(&s, "Vocal");
    let gain = |s: &Studio| match &s.project().tracks[&vocal].clips[0].body {
        xirius_core::model::ClipBody::Audio { gain_db, .. } => *gain_db,
        _ => unreachable!(),
    };
    assert!(press_on(&mut s, &g, "Vocal", 0));
    assert!(choose(&mut s, &g, "処理"));
    assert!(choose(&mut s, &g, "ノーマライズ"), "{:?}", s.error);
    let raised = gain(&s);
    assert!(raised > 0.0, "{raised}");
    let tips = s.gain_tip_rects(&g);
    assert_eq!(tips.len(), 1);
    let (r, said) = &tips[0];
    assert_eq!(*said, crate::gain_tip::text(raised));
    assert!(said.starts_with('+') && said.ends_with(" dB"), "{said}");
    // Over the middle of the clip, above it.
    let tree = s.tree();
    let row = tree.row_of(crate::tree::RowKind::Track(vocal)).unwrap();
    let clip = s.clip_rect(&g, row, &s.project().tracks[&vocal].clips[0]);
    assert!(r.bottom() <= clip.y);
    assert!((r.x + r.w / 2.0 - (clip.x + clip.w / 2.0)).abs() < 1.0);
    // It goes after a moment.
    s.gain_tip.as_mut().unwrap().since -= crate::gain_tip::SHOWN;
    assert!(s.gain_tip_rects(&g).is_empty());
    assert!(s.expire_gain_tip());
    assert!(s.gain_tip.is_none());
}

#[test]
fn a_range_has_time_rows_that_run_the_song_commands() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let (_, y) = clip_mid(&s, &g, "Keys", 0);
    let c = s.project().tracks[&keys].clips[0].clone();
    // A range off every clip still has its own menu, with the time rows.
    let gap = c.start + c.length + 480;
    s.arrange.range = Some((gap, gap + 7680));
    let x = s.tick_x(&g, gap + 3840);
    assert!(s.scene_context(&g, x, y));
    let rows = labels(&s);
    assert!(rows.contains(&"時間を挿入".to_string()), "{rows:?}");
    assert!(rows.contains(&"時間を削除".to_string()));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "時間を挿入"));
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn a_range_splits_the_clips_in_it_at_both_ends_from_its_own_menu() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let (_, y) = clip_mid(&s, &g, "Keys", 0);
    let c = s.project().tracks[&keys].clips[0].clone();
    // The range starts inside the clip and runs on past its end; the press is in the empty part.
    let (from, to) = (c.start + c.length / 2, c.start + c.length + 7680);
    s.arrange.range = Some((from, to));
    let x = s.tick_x(&g, c.start + c.length + 480);
    assert!(s.scene_context(&g, x, y));
    assert!(labels(&s).contains(&"範囲の両端で分割".to_string()));
    s.popup = None;
    s.arrange.clips.insert((keys, c.id));
    assert!(s.scene_context(&g, x, y));
    let (n, parts) = (steps(&s), s.project().tracks[&keys].clips.len());
    assert!(choose(&mut s, &g, "範囲の両端で分割"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().tracks[&keys].clips.len(), parts + 1);
}

#[test]
fn a_range_deletes_or_duplicates_what_lies_in_it_from_its_own_menu() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let (_, y) = clip_mid(&s, &g, "Keys", 0);
    let c = s.project().tracks[&keys].clips[0].clone();
    // The press is in the empty part after the clip, the range starts inside it.
    let (from, to) = (c.start + c.length / 4, c.start + c.length + 7680);
    s.arrange.range = Some((from, to));
    let x = s.tick_x(&g, c.start + c.length + 480);
    assert!(s.scene_context(&g, x, y));
    s.popup = None;
    s.arrange.clips.insert((keys, c.id));
    assert!(s.scene_context(&g, x, y));
    let rows = labels(&s);
    assert!(rows.contains(&"範囲の中身を複製".to_string()), "{rows:?}");
    assert!(rows.contains(&"範囲の中身を削除".to_string()));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "範囲の中身を削除"));
    assert_eq!(steps(&s), n + 1);
    // What is left of the clip ends where the range began.
    let left = &s.project().tracks[&keys].clips[0];
    assert_eq!((left.start, left.length), (c.start, from - c.start));
}

#[test]
fn a_midi_clip_and_several_clips_bounce_from_their_menus_as_one_step() {
    let (mut s, g) = studio();
    let tracks = s.project().tracks.len();
    // A MIDI clip.
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert!(choose(&mut s, &g, "バウンス"));
    assert_eq!(
        labels(&s),
        ["戻る", "クリップをバウンス", "新しいトラックにバウンス"]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "新しいトラックにバウンス"));
    s.job_wait();
    assert_eq!(s.error, None);
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().tracks.len(), tracks + 1);
    // A MIDI clip and an audio clip together: a new track for each.
    assert!(press_on(&mut s, &g, "Keys", 0));
    let vt = track(&s, "Vocal");
    let vc = s.project().tracks[&vt].clips[0].id;
    s.arrange.clips.insert((vt, vc));
    assert!(press_on(&mut s, &g, "Keys", 0));
    assert_eq!(s.arrange.clips.len(), 2);
    assert!(choose(&mut s, &g, "バウンス"));
    assert_eq!(
        labels(&s),
        ["戻る", "クリップをバウンス", "新しいトラックにバウンス"]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "新しいトラックにバウンス"));
    s.job_wait();
    assert_eq!(s.error, None);
    assert_eq!(steps(&s), n + 1);
    assert_eq!(s.project().tracks.len(), tracks + 3);
}
