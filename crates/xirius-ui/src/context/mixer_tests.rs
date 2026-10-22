//! The menus of the mixer (S27 a channel's head, S28 a fader, S29 a knob, S31 an effect,
//! S32 the ＋ of the inserts, S33 a send, S34 the ＋ of the sends, S35 a bus, S36 the master,
//! S37 the room right of the last strip), pressed as the window presses them.

use serde_json::json;
use xirius_core::id::{PluginId, SendId, TrackId};

use super::tests::{choose, enabled, labels, steps};
use crate::mixer::MixerHit;
use crate::studio::{DrawerTab, Geometry, Rect, Studio};
use crate::tree::RowKind;

const W: f32 = 1600.0;
const H: f32 = 1000.0;

#[test]
fn a_send_target_list_excludes_feedback_buses() {
    let (mut s, g) = mix();
    let a = add_bus(&mut s, "A");
    let b = add_bus(&mut s, "B");
    let c = add_bus(&mut s, "C");
    let id = add_send(&mut s, a, c);
    add_send(&mut s, b, a);
    assert!(press(&mut s, &g, MixerHit::SendPre(a, id)));
    assert!(choose(&mut s, &g, "送り先"));
    assert!(!labels(&s).contains(&"A".to_string()));
    assert!(!labels(&s).contains(&"B".to_string()));
    assert!(labels(&s).contains(&"C".to_string()));
}

/// The sample song with the mixer open.
fn mix() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
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

fn add_effect(s: &mut Studio, t: TrackId, name: &str) -> PluginId {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ));
    s.project().tracks[&t]
        .inserts
        .iter()
        .rev()
        .find(|p| p.name == name)
        .expect("added")
        .id
}

fn add_bus(s: &mut Studio, name: &str) -> TrackId {
    assert!(s.run_command("track.add.bus", json!({ "name": name, "into": "root" })));
    track(s, name)
}

fn add_send(s: &mut Studio, t: TrackId, to: TrackId) -> SendId {
    assert!(s.run_command(
        "send.add",
        json!({ "ids": [t.to_string()], "to": to.to_string() })
    ));
    s.project().tracks[&t].sends.last().expect("sent").id
}

fn insert_count(s: &Studio, t: TrackId) -> usize {
    s.project().tracks[&t].inserts.len()
}

/// Right-presses at the middle of `hit`.
fn press(s: &mut Studio, g: &Geometry, hit: MixerHit) -> bool {
    let (x, y) = s.mixer_center(g, hit).expect("on screen");
    s.scene_context(g, x, y)
}

/// Right-presses the name of a channel's strip.
fn press_head(s: &mut Studio, g: &Geometry, t: TrackId) -> bool {
    let r = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .name;
    s.scene_context(g, r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[test]
fn a_channel_head_has_its_rows_and_the_press_selects_the_track() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(press_head(&mut s, &g, vocal));
    assert_eq!(
        labels(&s),
        [
            "チャンネルの設定をコピー",
            "チャンネルの設定を貼り付け",
            "トラック名を変更",
            "色",
            "トラックを複製",
            "トラックの列で表示",
            "トラックのプリセットとして保存…",
            "トラックのプリセットを適用",
            "オートメーションを表示",
            "トラックをフリーズ",
            "ステムを書き出す…",
            "トラックを削除",
        ]
    );
    assert_eq!(s.selected_row, Some(RowKind::Track(vocal)));
}

#[test]
fn deleting_from_a_head_is_one_step_and_one_undo_brings_the_channel_back() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let n = steps(&s);
    assert!(press_head(&mut s, &g, vocal));
    assert!(choose(&mut s, &g, "トラックを削除"));
    assert!(!s.project().tracks.contains_key(&vocal));
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.project().tracks.contains_key(&vocal));
}

#[test]
fn showing_the_channel_in_the_track_column_makes_it_the_selection() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    assert!(press_head(&mut s, &g, keys));
    assert!(choose(&mut s, &g, "トラックの列で表示"));
    assert_eq!(s.selected_row, Some(RowKind::Track(keys)));
}

#[test]
fn the_master_has_the_copy_rows_the_mixdown_and_the_lanes_only() {
    let (mut s, g) = mix();
    assert!(press_head(&mut s, &g, TrackId::MASTER));
    assert_eq!(
        labels(&s),
        [
            "チャンネルの設定をコピー",
            "チャンネルの設定を貼り付け",
            "ミックスダウンを書き出す…",
            "オートメーションを表示"
        ]
    );
}

#[test]
fn a_bus_head_adds_the_sources_row_and_it_selects_who_plays_into_it() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    let bus = add_bus(&mut s, "Fx");
    let g = {
        s.resized(&g);
        s.geometry(W, H)
    };
    assert!(press_head(&mut s, &g, bus));
    assert!(labels(&s).contains(&"送っているチャンネルを選択".to_string()));
    assert!(!enabled(&s, "送っているチャンネルを選択"));
    s.popup = None;
    s.ctx = None;
    add_send(&mut s, keys, bus);
    assert!(press_head(&mut s, &g, bus));
    assert!(enabled(&s, "送っているチャンネルを選択"));
    assert!(choose(&mut s, &g, "送っているチャンネルを選択"));
    assert_eq!(s.selected_row, Some(RowKind::Track(keys)));
}

#[test]
fn the_fader_resets_to_zero_in_one_step_and_a_typed_value_sets_it() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(s.run_command(
        "track.volume.set",
        json!({ "ids": [vocal.to_string()], "db": -6.0 })
    ));
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::Fader(vocal)));
    assert_eq!(
        labels(&s),
        [
            "0 dB に戻す",
            "値を入力…",
            "オートメーションを表示",
            "すべてのポイントを削除",
            "MIDI 学習",
            "MIDI 割り当てを解除"
        ]
    );
    assert!(!enabled(&s, "すべてのポイントを削除"));
    assert!(choose(&mut s, &g, "0 dB に戻す"));
    assert_eq!(s.project().tracks[&vocal].volume_db, 0.0);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tracks[&vocal].volume_db, -6.0);

    assert!(press(&mut s, &g, MixerHit::Fader(vocal)));
    assert!(choose(&mut s, &g, "値を入力…"));
    assert!(s.ask_field().is_some());
    for _ in 0..12 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "-3", false);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert_eq!(s.project().tracks[&vocal].volume_db, -3.0);
}

#[test]
fn the_fader_shows_its_lane() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(press(&mut s, &g, MixerHit::Fader(vocal)));
    assert!(choose(&mut s, &g, "オートメーションを表示"));
    assert!(
        s.project().tracks[&vocal]
            .automation
            .iter()
            .any(|l| l.target.to_string() == "volume" && !l.hidden)
    );
}

#[test]
fn the_knob_resets_to_the_centre() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(s.run_command(
        "track.pan.set",
        json!({ "ids": [vocal.to_string()], "pan": 0.5 })
    ));
    assert!(press(&mut s, &g, MixerHit::Pan(vocal)));
    assert_eq!(
        labels(&s),
        [
            "既定に戻す",
            "値を入力…",
            "オートメーションを表示",
            "すべてのポイントを削除",
            "MIDI 学習",
            "MIDI 割り当てを解除"
        ]
    );
    assert!(choose(&mut s, &g, "既定に戻す"));
    assert_eq!(s.project().tracks[&vocal].pan, 0.0);
    assert!(s.song.undo());
    assert_eq!(s.project().tracks[&vocal].pan, 0.5);
}

#[test]
fn the_plus_of_the_sends_lists_the_buses_and_adds_the_send() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let bus = add_bus(&mut s, "Fx");
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::AddSend(vocal)));
    let rows = labels(&s);
    assert!(rows.contains(&"Fx".to_string()), "{rows:?}");
    assert!(choose(&mut s, &g, "Fx"));
    let sent = &s.project().tracks[&vocal].sends;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, bus);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.project().tracks[&vocal].sends.is_empty());
}

#[test]
fn a_send_resets_its_level_shows_its_bus_and_comes_off_in_one_step() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let bus = add_bus(&mut s, "Fx");
    let send = add_send(&mut s, vocal, bus);
    assert!(s.run_command("send.set", json!({ "id": send.to_string(), "level": -9.0 })));
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::SendLevel(vocal, send)));
    assert!(labels(&s).contains(&"0 dB に戻す".to_string()));
    assert!(choose(&mut s, &g, "0 dB に戻す"));
    assert_eq!(s.project().tracks[&vocal].sends[0].level_db, 0.0);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tracks[&vocal].sends[0].level_db, -9.0);

    assert!(press(&mut s, &g, MixerHit::SendPre(vocal, send)));
    assert_eq!(
        labels(&s),
        [
            "送り先",
            "0 dB に戻す",
            "ミュート",
            "送り先のバスを表示",
            "オートメーションを表示",
            "センドを削除"
        ]
    );
    assert!(choose(&mut s, &g, "センドを削除"));
    assert!(s.project().tracks[&vocal].sends.is_empty());
    assert!(s.song.undo());
    assert_eq!(s.project().tracks[&vocal].sends.len(), 1);
}

#[test]
fn an_effect_has_its_rows_and_a_duplicate_is_one_step() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Comp");
    let before = insert_count(&s, vocal);
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert_eq!(
        labels(&s),
        [
            "画面を開く",
            "プリセット",
            "再起動",
            "無効にする",
            "プラグインを複製",
            "別のチャンネルへ移動",
            "別のチャンネルへ写す",
            "オートメーションを表示",
            "切り取り",
            "コピー",
            "プラグインを削除",
        ]
    );
    assert!(choose(&mut s, &g, "プラグインを複製"));
    assert_eq!(insert_count(&s, vocal), before + 1);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(insert_count(&s, vocal), before);
}

#[test]
fn the_restart_row_is_pressable_only_for_a_plugin_that_went_down() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Comp");
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(
        !enabled(&s, "再起動"),
        "a plugin that runs has nothing to restart"
    );
    let before = s.health.restarts;
    assert!(!choose(&mut s, &g, "再起動"));
    assert_eq!(s.health.restarts, before);
}

#[test]
fn an_effect_moves_to_another_channel_and_one_undo_takes_it_back() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    let comp = add_effect(&mut s, vocal, "Comp");
    let (a, b) = (insert_count(&s, vocal), insert_count(&s, keys));
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(choose(&mut s, &g, "別のチャンネルへ移動"));
    assert!(choose(&mut s, &g, "Keys"));
    assert_eq!(insert_count(&s, vocal), a - 1);
    assert_eq!(insert_count(&s, keys), b + 1);
    assert!(s.song.undo());
    assert_eq!(insert_count(&s, vocal), a);
    assert_eq!(insert_count(&s, keys), b);
}

#[test]
fn an_effect_is_copied_to_another_channel_and_removed() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    let comp = add_effect(&mut s, vocal, "Comp");
    let b = insert_count(&s, keys);
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(choose(&mut s, &g, "別のチャンネルへ写す"));
    assert!(choose(&mut s, &g, "Keys"));
    assert_eq!(insert_count(&s, keys), b + 1);
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(choose(&mut s, &g, "プラグインを削除"));
    assert!(
        !s.project().tracks[&vocal]
            .inserts
            .iter()
            .any(|p| p.id == comp)
    );
}

#[test]
fn an_effect_is_copied_and_pasted_from_the_plus_row_of_another_channel() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    let comp = add_effect(&mut s, vocal, "Comp");
    assert!(press(&mut s, &g, MixerHit::AddInsert(keys)));
    assert_eq!(labels(&s), ["エフェクトを追加…", "貼り付け"]);
    assert_eq!(super::tests::key(&s, "貼り付け"), "Ctrl+V");
    assert!(!enabled(&s, "貼り付け"));
    s.popup = None;
    s.ctx = None;
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert_eq!(super::tests::key(&s, "切り取り"), "Ctrl+X");
    assert_eq!(super::tests::key(&s, "コピー"), "Ctrl+C");
    assert!(choose(&mut s, &g, "コピー"));
    let b = insert_count(&s, keys);
    assert!(press(&mut s, &g, MixerHit::AddInsert(keys)));
    assert!(enabled(&s, "貼り付け"));
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(insert_count(&s, keys), b + 1);
    assert!(s.song.undo());
    assert_eq!(insert_count(&s, keys), b);
}

#[test]
fn the_instrument_row_keeps_its_own_menu() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": keys.to_string(), "name": "Sampler", "kind": "instrument", "format": "builtin", "uid": "sampler" }),
    ));
    let id = s.project().tracks[&keys]
        .inserts
        .iter()
        .find(|p| p.kind == xirius_core::model::PluginKind::Instrument)
        .expect("an instrument")
        .id;
    let before = steps(&s);
    // The mixer's menu steps aside; the older rule opens the instrument's.
    let (x, y) = s
        .mixer_center(&g, MixerHit::Open(keys, id))
        .expect("on screen");
    assert!(!s.mixer_context(&g, x, y));
    assert_eq!(steps(&s), before);
}

#[test]
fn a_preset_of_a_built_in_effect_loads_in_one_step() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Compressor");
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(enabled(&s, "プリセット"));
    assert!(choose(&mut s, &g, "プリセット"));
    assert!(choose(&mut s, &g, "強め"));
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.song.can_redo());
}

/// The last channel strip; the master beside it is pinned to the edge and does not count.
fn last_channel(s: &Studio, g: &Geometry) -> Rect {
    s.mixer_strips(g)
        .into_iter()
        .rfind(|x| !x.track.is_master())
        .expect("a strip")
        .rect
}

#[test]
fn the_room_right_of_the_last_strip_adds_a_bus_or_a_track() {
    let (mut s, g) = mix();
    let last = last_channel(&s, &g);
    let (x, y) = (last.x + last.w + 40.0, last.y + 60.0);
    assert!(g.drawer_body.contains(x, y), "no room right of the strips");
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        ["バスを追加", "トラックを追加", "貼り付け", "チャンネルの幅"]
    );
    let n = s.project().tracks.len();
    assert!(choose(&mut s, &g, "バスを追加"));
    assert_eq!(s.project().tracks.len(), n + 1);
    assert!(s.song.undo());
    assert_eq!(s.project().tracks.len(), n);
}

#[test]
fn empty_mixer_pastes_tracks_and_channel_width_changes_the_hit_rectangles() {
    let (mut s, g) = mix();
    // Undo here must land exactly on the state the test kept; the
    // instrument-input arm that would follow the selection is not
    // what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let t = track(&s, "Keys");
    s.song
        .run("edit.copy", json!({"tracks":[t.to_string()]}))
        .unwrap();
    let last = last_channel(&s, &g);
    let (x, y) = (last.right() + 15.0, last.y + 60.0);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(super::tests::key(&s, "貼り付け"), "Ctrl+V");
    let before = s.project();
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(s.project().tracks.len(), before.tracks.len() + 1);
    s.undo(&g);
    let mut restored = s.project().as_ref().clone();
    restored.next_id = before.next_id;
    assert_eq!(&restored, before.as_ref());
    s.tab = DrawerTab::Mixer;
    s.drawer_open = true;
    let edge = last_channel(&s, &g);
    assert!(s.scene_context(&g, edge.right() + 15.0, edge.y + 60.0));
    assert!(
        labels(&s).contains(&"チャンネルの幅".to_string()),
        "after undo: {:?}",
        labels(&s)
    );
    assert!(choose(&mut s, &g, "チャンネルの幅"));
    assert!(choose(&mut s, &g, "広い"));
    assert!(s.mixer_strips(&g)[0].rect.w > crate::mixer::STRIP_W);
    s.mixer.scroll = s.mixer_max_scroll(&g);
    assert!(s.scene_context(&g, g.drawer_body.right() - 2.0, last.y + 60.0));
    assert!(
        labels(&s).contains(&"チャンネルの幅".to_string()),
        "wide: {:?}",
        labels(&s)
    );
    assert!(choose(&mut s, &g, "チャンネルの幅"));
    assert!(choose(&mut s, &g, "細い"));
    assert_eq!(s.mixer_strips(&g)[0].rect.w, crate::mixer::STRIP_W);
}

#[test]
fn the_empty_mixers_paste_takes_a_copied_effect_too() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Comp");
    s.song
        .run("edit.copy", json!({"plugin": comp.to_string()}))
        .unwrap();
    let last = last_channel(&s, &g);
    assert!(s.scene_context(&g, last.right() + 15.0, last.y + 60.0));
    assert!(
        enabled(&s, "貼り付け"),
        "an effect on the board is what the command pastes"
    );
    let before = s.project();
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(
        s.project().tracks[&vocal].inserts.len(),
        before.tracks[&vocal].inserts.len() + 1,
        "{:?}",
        s.error
    );
}

#[test]
fn the_empty_mixers_paste_stays_dim_when_nothing_pasteable_was_copied() {
    let (mut s, g) = mix();
    let last = last_channel(&s, &g);
    assert!(s.scene_context(&g, last.right() + 15.0, last.y + 60.0));
    assert!(!enabled(&s, "貼り付け"));
}

#[test]
fn unavailable_send_targets_keep_their_place_in_the_list() {
    let (mut s, g) = mix();
    let t = track(&s, "Vocal");
    let first = add_bus(&mut s, "First");
    let second = add_bus(&mut s, "Second");
    let sid = add_send(&mut s, t, first);
    add_send(&mut s, t, second);
    assert!(press(&mut s, &g, MixerHit::SendPre(t, sid)));
    assert!(choose(&mut s, &g, "送り先"));
    assert!(labels(&s).contains(&"Second".to_string()));
    assert!(!enabled(&s, "Second"));
    let before = s.project();
    assert!(!choose(&mut s, &g, "Second"));
    assert_eq!(s.project(), before);
}

#[test]
fn with_the_mixer_shut_the_timeline_keeps_its_menus() {
    let (mut s, g) = mix();
    s.drawer_open = false;
    let g2 = s.geometry(W, H);
    s.resized(&g2);
    assert!(!s.mixer_context(&g2, g.drawer_body.x + 10.0, g.drawer_body.y + 10.0));
}

#[test]
fn a_bus_head_exports_the_bus_as_a_stem() {
    let (mut s, g) = mix();
    let bus = add_bus(&mut s, "Fx");
    s.resized(&g);
    let g = s.geometry(W, H);
    assert!(press_head(&mut s, &g, bus));
    assert!(choose(&mut s, &g, "このバスを書き出す…"));
    assert!(s.export.stems);
    assert_eq!(s.export.picked, vec![bus]);
}

#[test]
fn the_plus_row_lists_the_effects_used_lately_and_one_puts_it_in() {
    let (mut s, g) = mix();
    let keys = track(&s, "Keys");
    s.mixer.recent.push(crate::instrument::Pick {
        name: "Compressor".into(),
        format: "builtin".into(),
        uid: "compressor".into(),
    });
    let b = insert_count(&s, keys);
    let n = steps(&s);
    assert!(press(&mut s, &g, MixerHit::AddInsert(keys)));
    assert_eq!(
        labels(&s),
        ["エフェクトを追加…", "最近使ったもの", "貼り付け"]
    );
    assert!(choose(&mut s, &g, "最近使ったもの"));
    assert!(choose(&mut s, &g, "Compressor"));
    assert_eq!(insert_count(&s, keys), b + 1);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(insert_count(&s, keys), b);
}

#[test]
fn the_preset_rows_ask_for_a_file_for_any_effect() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let comp = add_effect(&mut s, vocal, "Comp");
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(choose(&mut s, &g, "プリセット"));
    assert!(choose(&mut s, &g, "プリセットを保存…"));
    assert_eq!(
        s.file_request,
        Some(crate::studio::FileRequest::PresetSave(comp))
    );
    s.file_request = None;
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    assert!(choose(&mut s, &g, "プリセット"));
    assert!(choose(&mut s, &g, "プリセットを読み込む…"));
    assert_eq!(
        s.file_request,
        Some(crate::studio::FileRequest::PresetOpen(comp))
    );
}

#[test]
fn the_plugins_copy_paste_and_cut_keys_use_the_menu_target_and_undo_once() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    let comp = add_effect(&mut s, vocal, "Comp");
    let ctrl = crate::tools::Mods {
        ctrl: true,
        ..crate::tools::Mods::NONE
    };
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    s.popup = None;
    assert!(s.mapped_key(&g, "c", None, ctrl));
    assert!(
        matches!(
            s.song.session().state.clipboard,
            Some(xirius_core::clipboard::Clipboard::Plugins(_))
        ),
        "{:?}",
        s.error
    );
    assert!(press(&mut s, &g, MixerHit::AddInsert(keys)));
    s.popup = None;
    let before = s.project();
    let n = steps(&s);
    assert_eq!(s.input_context(&g), "mixer");
    assert_eq!(s.selected_row, Some(RowKind::Track(keys)));
    assert!(s.mapped_key(&g, "v", None, ctrl));
    assert_eq!(
        s.project().tracks[&keys].inserts.len(),
        before.tracks[&keys].inserts.len() + 1,
        "{:?}",
        s.error
    );
    assert_eq!(steps(&s), n + 1);
    assert!(s.undo(&g));
    let mut restored = s.project().as_ref().clone();
    restored.next_id = before.next_id;
    assert_eq!(restored, *before);
    assert!(press(&mut s, &g, MixerHit::Open(vocal, comp)));
    s.popup = None;
    let before = s.project();
    assert!(s.mapped_key(&g, "x", None, ctrl));
    assert_eq!(
        s.project().tracks[&vocal].inserts.len(),
        before.tracks[&vocal].inserts.len() - 1
    );
    assert!(s.undo(&g));
    assert_eq!(s.project(), before);
}

#[test]
fn showing_the_master_strip_scrolls_nothing_and_a_channel_scrolls_into_the_view() {
    let (mut s, _) = mix();
    let g = s.geometry(700.0, 1000.0);
    assert!(s.mixer_max_scroll(&g) > 0.0);
    assert!(s.show_strip(&g, TrackId::MASTER));
    assert_eq!(s.mixer.scroll, 0.0, "the master is always on screen");
    let last = crate::mixer::strip_order(&s.project())
        .into_iter()
        .rfind(|t| !t.is_master())
        .unwrap();
    assert!(s.show_strip(&g, last));
    let view = s.mixer_view(&g);
    let at = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == last)
        .expect("the channel is on screen")
        .rect;
    assert!(
        at.x >= view.x && at.right() <= view.right() + 0.01,
        "{at:?} in {view:?}"
    );
}

#[test]
fn a_channel_head_shows_its_colour_on_the_colour_row() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let now = crate::draw::track_color(&s.project().tracks[&vocal]);
    assert!(press_head(&mut s, &g, vocal));
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let row = layout
        .cells
        .iter()
        .find(|c| c.text == "色")
        .expect("a colour row");
    assert_eq!(row.mark, Some(now));
    assert!(
        layout
            .cells
            .iter()
            .filter(|c| c.text != "色")
            .all(|c| c.mark.is_none())
    );
}

/// Shift+F10 and the menu key in the mixer open the selected track's channel menu, where a
/// press on its name would open it.
#[test]
fn shift_f10_and_the_menu_key_open_the_selected_channels_menu() {
    use crate::tools::Mods;
    let (mut s, g) = mix();
    s.arrange.focus = false;
    assert_eq!(s.input_context(&g), "mixer");
    s.selected_row = None;
    assert!(!s.mapped_key(&g, "f10", None, Mods::shift()));
    assert!(s.popup.is_none());
    let vocal = track(&s, "Vocal");
    assert!(press_head(&mut s, &g, vocal));
    let by_mouse = labels(&s);
    let at = s.popup.as_ref().unwrap().anchor;
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(s.popup.is_none());
    assert_eq!(s.selected_row, Some(RowKind::Track(vocal)));
    for (key, mods) in [("f10", Mods::shift()), ("menu", Mods::NONE)] {
        assert!(s.mapped_key(&g, key, None, mods), "{key}");
        assert_eq!(labels(&s), by_mouse, "{key}");
        assert_eq!(s.popup.as_ref().unwrap().anchor, at, "{key}");
        assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    }
}
