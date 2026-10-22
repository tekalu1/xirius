//! The menus that copy a channel's settings and keep track presets (MIX-007, MIX-030), pressed
//! as the window presses them: the right press opens the menu, a row is chosen by its cell, and
//! the notice at the foot of the lanes is pressed at its button.

use serde_json::{Value, json};
use xirius_core::id::TrackId;

use super::tests::{choose, enabled, labels, steps};
use crate::notice::{button_rect, place};
use crate::studio::{DrawerTab, Geometry, Studio};

const W: f32 = 1600.0;
const H: f32 = 1000.0;

/// The sample song with the mixer open.
fn mix() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    crate::arrange::probe::track(s, name)
}

fn press_head(s: &mut Studio, g: &Geometry, t: TrackId) -> bool {
    let r = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .name;
    s.scene_context(g, r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn add_effect(s: &mut Studio, t: TrackId, name: &str) {
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": name, "kind": "effect", "format": "builtin", "uid": name }),
    ));
}

fn inserts(s: &Studio, t: TrackId) -> Vec<String> {
    s.project().tracks[&t]
        .inserts
        .iter()
        .map(|p| p.name.clone())
        .collect()
}

fn type_name(s: &mut Studio, name: &str) {
    for _ in 0..40 {
        s.ask_key("backspace");
    }
    s.ask_text(None, name, false);
}

/// Presses the button of the notice that is shown.
fn press_notice_button(s: &mut Studio, g: &Geometry) -> bool {
    let text = s.notice_text().expect("a notice").to_string();
    let label = s.notice_button().expect("a button").to_string();
    let r = button_rect(place(s, g, &text, Some(&label)), &label);
    s.notice_press(g, r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// Saves `t` as the preset `name` through its head menu.
fn save_through_menu(s: &mut Studio, g: &Geometry, t: TrackId, name: &str) {
    assert!(press_head(s, g, t));
    assert!(choose(s, g, "トラックのプリセットとして保存…"));
    assert!(s.ask_field().is_some(), "the menu asks for the name");
    type_name(s, name);
    s.ask_key("enter");
}

fn presets(s: &mut Studio) -> Vec<Value> {
    s.song.run("track.preset.list", json!({})).unwrap()["presets"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn nothing_can_be_pasted_before_something_is_copied_and_never_onto_its_own_channel() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    assert!(press_head(&mut s, &g, vocal));
    assert!(enabled(&s, "チャンネルの設定をコピー"));
    assert!(!enabled(&s, "チャンネルの設定を貼り付け"));
    assert!(choose(&mut s, &g, "チャンネルの設定をコピー"));
    assert!(press_head(&mut s, &g, vocal));
    assert!(!enabled(&s, "チャンネルの設定を貼り付け"));
    let keys = track(&s, "Keys");
    assert!(press_head(&mut s, &g, keys));
    assert!(enabled(&s, "チャンネルの設定を貼り付け"));
}

#[test]
fn pasting_from_the_menu_is_one_step_the_notice_undoes_it() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let guitar = track(&s, "Guitar");
    add_effect(&mut s, vocal, "Compressor");
    assert!(press_head(&mut s, &g, vocal));
    assert!(choose(&mut s, &g, "チャンネルの設定をコピー"));
    let before = steps(&s);
    assert!(press_head(&mut s, &g, guitar));
    assert!(choose(&mut s, &g, "チャンネルの設定を貼り付け"));
    assert_eq!(steps(&s), before + 1);
    assert_eq!(inserts(&s, guitar), ["Compressor"]);
    assert_eq!(s.notice_text(), Some("チャンネルの設定を貼り付けました"));
    assert_eq!(s.notice_button(), Some("元に戻す"));
    assert!(press_notice_button(&mut s, &g));
    assert!(inserts(&s, guitar).is_empty());
    assert_eq!(s.notice_text(), None);
}

#[test]
fn the_master_copies_and_takes_settings_too() {
    let (mut s, g) = mix();
    assert!(press_head(&mut s, &g, TrackId::MASTER));
    assert!(enabled(&s, "チャンネルの設定をコピー"));
    assert!(choose(&mut s, &g, "チャンネルの設定をコピー"));
    let keys = track(&s, "Keys");
    assert!(press_head(&mut s, &g, keys));
    assert!(enabled(&s, "チャンネルの設定を貼り付け"));
}

#[test]
fn saving_a_track_as_a_preset_asks_for_the_name_and_marks_the_track() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    add_effect(&mut s, vocal, "Compressor");
    // Nothing to apply or update before the first preset.
    assert!(press_head(&mut s, &g, vocal));
    assert!(!enabled(&s, "トラックのプリセットを適用"));
    assert!(!labels(&s).contains(&"トラックのプリセットを更新".to_string()));
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    assert_eq!(
        s.notice_text(),
        Some("トラックのプリセット「ボーカル」を保存しました")
    );
    assert_eq!(
        s.project().tracks[&vocal].preset.as_deref(),
        Some("ボーカル")
    );
    let list = presets(&mut s);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "ボーカル");
    // Now the track offers the update, and the others the apply.
    assert!(press_head(&mut s, &g, vocal));
    assert!(labels(&s).contains(&"トラックのプリセットを更新".to_string()));
    let guitar = track(&s, "Guitar");
    assert!(press_head(&mut s, &g, guitar));
    assert!(enabled(&s, "トラックのプリセットを適用"));
    assert!(!labels(&s).contains(&"トラックのプリセットを更新".to_string()));
}

#[test]
fn a_name_that_is_taken_is_not_saved_again() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let guitar = track(&s, "Guitar");
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    save_through_menu(&mut s, &g, guitar, "ボーカル");
    assert_eq!(presets(&mut s).len(), 1);
    assert_eq!(s.project().tracks[&guitar].preset, None);
}

#[test]
fn applying_a_preset_is_one_step_and_keeps_the_tracks_name() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let guitar = track(&s, "Guitar");
    add_effect(&mut s, vocal, "Compressor");
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    let before = steps(&s);
    assert!(press_head(&mut s, &g, guitar));
    assert!(choose(&mut s, &g, "トラックのプリセットを適用"));
    assert!(choose(&mut s, &g, "ボーカル"));
    assert_eq!(steps(&s), before + 1);
    assert_eq!(inserts(&s, guitar), ["Compressor"]);
    assert_eq!(s.project().tracks[&guitar].name, "Guitar");
    assert_eq!(
        s.notice_text(),
        Some("トラックのプリセット「ボーカル」を適用しました")
    );
    assert_eq!(s.notice_button(), Some("元に戻す"));
    assert!(s.song.undo());
    assert!(inserts(&s, guitar).is_empty());
}

#[test]
fn a_preset_that_needed_adjusting_says_how_many_and_opens_the_list() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let guitar = track(&s, "Guitar");
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    // The file as another version, or a hand, left it: a volume out of range and a key that
    // this version does not know.
    let file = presets(&mut s)[0]["file"].as_str().unwrap().to_string();
    let mut v: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    v["volume_db"] = json!(60.0);
    v["from_the_future"] = json!(1);
    std::fs::write(&file, serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(press_head(&mut s, &g, guitar));
    assert!(choose(&mut s, &g, "トラックのプリセットを適用"));
    assert!(choose(&mut s, &g, "ボーカル"));
    let text = s.notice_text().expect("a notice").to_string();
    assert!(text.contains("件を調整"), "{text}");
    assert_eq!(s.notice_button(), Some("詳しく"));
    assert!(press_notice_button(&mut s, &g));
    assert_eq!(
        s.popup.as_ref().map(|p| p.kind),
        Some(crate::popup::PopupKind::Report)
    );
    let layout = s.popup.as_ref().unwrap().layout(g.size);
    let rows: Vec<&str> = layout
        .cells
        .iter()
        .filter(|c| matches!(c.look, crate::popup::Look::Note))
        .map(|c| c.text.as_str())
        .collect();
    assert!(rows.len() >= 2, "{rows:?}");
    assert!(rows.iter().any(|r| r.contains("範囲")), "{rows:?}");
    assert!(
        rows.iter().any(|r| r.contains("from_the_future")),
        "{rows:?}"
    );
    // The button closes it.
    let close = layout
        .cells
        .iter()
        .find(|c| c.text == "閉じる")
        .expect("the close button");
    let hit = s.hit(
        &g,
        close.rect.x + close.rect.w / 2.0,
        close.rect.y + close.rect.h / 2.0,
    );
    assert!(s.press(&g, hit));
    assert!(s.popup.is_none());
}

#[test]
fn updating_a_track_made_from_a_preset_rewrites_it_and_keeps_its_place() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let guitar = track(&s, "Guitar");
    save_through_menu(&mut s, &g, guitar, "ギター");
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    add_effect(&mut s, vocal, "Compressor");
    assert!(press_head(&mut s, &g, vocal));
    assert!(choose(&mut s, &g, "トラックのプリセットを更新"));
    assert_eq!(
        s.notice_text(),
        Some("トラックのプリセット「ボーカル」を更新しました")
    );
    let list = presets(&mut s);
    assert_eq!(
        list.iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["ギター", "ボーカル"]
    );
    assert_eq!(list[1]["inserts"], 1);
}

#[test]
fn a_preset_of_another_kind_is_dim_in_the_apply_list() {
    let (mut s, g) = mix();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    save_through_menu(&mut s, &g, vocal, "ボーカル");
    assert!(press_head(&mut s, &g, keys));
    assert!(choose(&mut s, &g, "トラックのプリセットを適用"));
    assert!(!enabled(&s, "ボーカル"));
}

#[test]
fn a_bus_and_the_master_have_no_preset_rows() {
    let (mut s, g) = mix();
    assert!(s.run_command("track.add.bus", json!({ "name": "Fx", "into": "root" })));
    let bus = track(&s, "Fx");
    assert!(press_head(&mut s, &g, bus));
    assert!(!labels(&s).iter().any(|l| l.contains("プリセット")));
    assert!(press_head(&mut s, &g, TrackId::MASTER));
    assert!(!labels(&s).iter().any(|l| l.contains("プリセット")));
}

#[test]
fn the_column_adds_a_track_from_a_preset_named_after_it() {
    let (mut a, ga) = super::tests::studio();
    let v = track(&a, "Vocal");
    add_effect(&mut a, v, "Compressor");
    assert!(
        a.song
            .run(
                "track.preset.save",
                json!({ "track": v.to_string(), "name": "ボーカル" })
            )
            .is_ok()
    );
    let n = a.project().tracks.len();
    let before = steps(&a);
    let row = *a.tree().rows.last().expect("rows");
    let y = a.row_y(&ga, &row) + row.h + 12.0;
    assert!(a.scene_context(&ga, ga.column.x + 40.0, y));
    assert!(choose(&mut a, &ga, "トラックを追加"));
    assert!(choose(&mut a, &ga, "ボーカル"));
    assert_eq!(a.project().tracks.len(), n + 1);
    assert_eq!(steps(&a), before + 1);
    let made = a
        .project()
        .tracks
        .values()
        .find(|t| t.preset.as_deref() == Some("ボーカル") && t.id != v)
        .expect("the new track")
        .clone();
    assert_eq!(made.name, "ボーカル");
    assert_eq!(made.inserts.len(), 1);
    assert_eq!(a.notice_text(), Some("トラック「ボーカル」を追加しました"));
}

#[test]
fn the_empty_mixer_adds_a_track_from_a_preset() {
    let (mut s, g) = mix();
    let v = track(&s, "Vocal");
    add_effect(&mut s, v, "Compressor");
    assert!(
        s.song
            .run(
                "track.preset.save",
                json!({ "track": v.to_string(), "name": "ボーカル" })
            )
            .is_ok()
    );
    let n = s.project().tracks.len();
    let before = steps(&s);
    let last = s
        .mixer_strips(&g)
        .into_iter()
        .rfind(|x| !x.track.is_master())
        .expect("a strip")
        .rect;
    assert!(s.scene_context(&g, last.right() + 15.0, last.y + 60.0));
    assert!(choose(&mut s, &g, "トラックを追加"));
    assert!(choose(&mut s, &g, "ボーカル"));
    assert_eq!(s.project().tracks.len(), n + 1);
    assert_eq!(steps(&s), before + 1);
}
