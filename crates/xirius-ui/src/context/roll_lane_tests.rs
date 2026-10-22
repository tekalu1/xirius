//! The menus of the piano roll's keys and controller lanes (S22 … S26), pressed as the
//! window presses them.

use serde_json::json;
use xirius_core::model::{CtrlKind, CtrlPoint, CtrlShape, Note};

use super::roll_tests::{note_at, notes, roll};
use super::tests::{choose, enabled, key, labels, steps};
use crate::roll::{LaneKind, y_of};
use crate::studio::{Geometry, Studio};

/// The roll with a CC1 lane of three points: 0 → 10, 480 → 90, 960 → 40.
fn with_cc1() -> (Studio, Geometry, usize) {
    let (mut s, g) = roll();
    let c = s.roll_ctx(&g).unwrap();
    s.song
        .run(
            "cc.replace",
            json!({ "clip": c.clip.id.to_string(), "lane": "cc1", "from": 0, "to": 1920,
                    "points": [{ "tick": 0, "value": 10 }, { "tick": 480, "value": 90 },
                               { "tick": 960, "value": 40 }] }),
        )
        .unwrap();
    s.resized(&g);
    let index = s
        .roll
        .lanes
        .iter()
        .position(|l| l.kind == LaneKind::Ctrl(CtrlKind::Cc(1)))
        .unwrap();
    (s, g, index)
}

fn cc1(s: &Studio, g: &Geometry) -> Vec<(i64, i32)> {
    let c = s.roll_ctx(g).unwrap();
    s.project()
        .clip(c.clip.id)
        .unwrap()
        .1
        .controllers
        .iter()
        .find(|l| l.kind == CtrlKind::Cc(1))
        .map(|l| {
            l.points
                .iter()
                .map(|q: &CtrlPoint| (q.tick, q.value))
                .collect()
        })
        .unwrap_or_default()
}

/// The labels of the rows that are lit.
fn lit(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect()
}

/// The shapes of the CC1 points, in tick order.
fn cc1_shapes(s: &Studio, g: &Geometry) -> Vec<CtrlShape> {
    let c = s.roll_ctx(g).unwrap();
    s.project()
        .clip(c.clip.id)
        .unwrap()
        .1
        .controllers
        .iter()
        .find(|l| l.kind == CtrlKind::Cc(1))
        .map(|l| l.points.iter().map(|q| q.shape).collect())
        .unwrap_or_default()
}

/// The window point of a controller point of lane `index`.
fn point_at(s: &Studio, g: &Geometry, index: usize, tick: i64, value: i32) -> (f32, f32) {
    let c = s.roll_ctx(g).unwrap();
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    let plot = f.lanes[index].plot;
    (v.x(tick), y_of(s.roll.lanes[index].kind, &plot, value))
}

/// A point of lane `index` with no point near it.
fn lane_empty_at(s: &Studio, g: &Geometry, index: usize, tick: i64) -> (f32, f32) {
    let (x, _) = point_at(s, g, index, tick, 0);
    let c = s.roll_ctx(g).unwrap();
    let plot = s.roll.frame(c.body).lanes[index].plot;
    (x, plot.y + plot.h / 2.0)
}

fn tab_at(s: &Studio, g: &Geometry, index: usize) -> (f32, f32) {
    let c = s.roll_ctx(g).unwrap();
    let r = s.roll.frame(c.body).tabs[index];
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

#[test]
fn a_key_selects_the_notes_of_its_height_and_is_dim_where_there_are_none() {
    let (mut s, g) = roll();
    let c = s.roll_ctx(&g).unwrap();
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    let x = f.keys.x + f.keys.w / 2.0;

    assert!(s.scene_context(&g, x, v.y(61) + v.row_h / 2.0));
    assert_eq!(labels(&s), ["同じ高さのノートを選択", "鍵盤の表示"]);
    assert!(!enabled(&s, "同じ高さのノートを選択"));

    assert!(s.scene_context(&g, x, v.y(62) + v.row_h / 2.0));
    assert!(enabled(&s, "同じ高さのノートを選択"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "同じ高さのノートを選択"));
    assert_eq!(steps(&s), n);
    let picked: Vec<_> = s.roll.selected.iter().copied().collect();
    assert_eq!(picked, [notes(&s, &g)[1].id]);
}

#[test]
fn a_velocity_bar_takes_a_typed_value_and_evens_out_or_slopes_the_selected_bars() {
    let (mut s, g) = roll();
    let ids: Vec<_> = notes(&s, &g).iter().map(|n| n.id.to_string()).collect();
    s.song
        .run("note.velocity.set", json!({ "ids": [ids[0]], "value": 40 }))
        .unwrap();
    let c = s.roll_ctx(&g).unwrap();
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    let plot = f.lanes[0].plot;
    let at = |n: &Note| (v.x(n.start), plot.y + plot.h / 2.0);

    // Pressing a bar selects it alone; a typed value sets it.
    let (x, y) = at(&notes(&s, &g)[2]);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["値を入力…", "揃える"]);
    assert_eq!(s.roll.selected.len(), 1);
    assert!(choose(&mut s, &g, "値を入力…"));
    for _ in 0..4 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "64", false);
    let n = steps(&s);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(notes(&s, &g)[2].velocity, 64);

    // All three picked: a straight slope from the first to the last.
    s.roll.selected = notes(&s, &g).iter().map(|n| n.id).collect();
    let (x, y) = at(&notes(&s, &g)[1]);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(s.roll.selected.len(), 3);
    assert!(choose(&mut s, &g, "揃える"));
    assert_eq!(labels(&s), ["戻る", "同じ値に", "直線で傾ける"]);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "直線で傾ける"));
    assert_eq!(steps(&s), n + 1);
    let v: Vec<u8> = notes(&s, &g).iter().map(|n| n.velocity).collect();
    assert_eq!(v[0], 40);
    assert_eq!(v[2], 64);
    assert!(v[1] > 40 && v[1] < 64, "{v:?}");

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "揃える"));
    assert!(choose(&mut s, &g, "同じ値に"));
    let v: Vec<u8> = notes(&s, &g).iter().map(|n| n.velocity).collect();
    assert!(v.iter().all(|x| *x == v[0]), "{v:?}");

    // Even already: nothing to slope.
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "揃える"));
}

#[test]
fn a_controller_point_takes_a_value_and_is_copied_cut_and_deleted_with_the_selected_ones() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = point_at(&s, &g, i, 480, 90);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        [
            "値を入力…",
            "次のポイントまでの形",
            "切り取り",
            "コピー",
            "削除"
        ]
    );
    assert_eq!(key(&s, "切り取り"), "Ctrl+X");
    assert_eq!(key(&s, "コピー"), "Ctrl+C");
    assert_eq!(key(&s, "削除"), "Del / Backspace");
    assert_eq!(s.roll.points.iter().copied().collect::<Vec<_>>(), [480]);

    assert!(choose(&mut s, &g, "値を入力…"));
    for _ in 0..4 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "55", false);
    let n = steps(&s);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(cc1(&s, &g), [(0, 10), (480, 55), (960, 40)]);
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g)[1], (480, 90));

    // Copy keeps the song as it is and fills the clipboard.
    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "コピー"));
    assert_eq!(steps(&s), n);
    assert_eq!(
        s.song.session().state.clipboard.as_ref().map(|b| b.kind()),
        Some("controller")
    );

    // Two points picked, the press on one of them: both go in one step.
    s.roll.points = [0, 480].into_iter().collect();
    assert!(s.scene_context(&g, x, y));
    assert_eq!(s.roll.points.len(), 2);
    assert!(choose(&mut s, &g, "削除"));
    assert_eq!(cc1(&s, &g), [(960, 40)]);
    assert!(s.roll.points.is_empty());
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g).len(), 3);

    let (x, y) = point_at(&s, &g, i, 960, 40);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "切り取り"));
    assert_eq!(cc1(&s, &g), [(0, 10), (480, 90)]);
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g).len(), 3);
}

#[test]
fn a_controller_point_is_made_a_line_or_a_step_to_the_next_one_in_one_undo() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = point_at(&s, &g, i, 480, 90);
    assert!(s.scene_context(&g, x, y));
    assert!(enabled(&s, "次のポイントまでの形"));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    assert_eq!(labels(&s), ["戻る", "直線", "階段"]);
    // The row of the shape the point has already would change nothing, so it is dim.
    assert!(enabled(&s, "直線") && !enabled(&s, "階段"));
    // Every point starts as a step, and the lit row says so.
    assert_eq!(lit(&s), ["階段"]);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "直線"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(
        cc1_shapes(&s, &g),
        [CtrlShape::Step, CtrlShape::Linear, CtrlShape::Step]
    );
    // The value is kept by a value change, and the lit row follows the shape.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    assert_eq!(lit(&s), ["直線"]);
    assert!(choose(&mut s, &g, "階段"));
    assert_eq!(cc1_shapes(&s, &g)[1], CtrlShape::Step);
    assert!(s.song.undo());
    assert_eq!(cc1_shapes(&s, &g)[1], CtrlShape::Linear);
    assert!(s.song.undo());
    assert_eq!(cc1_shapes(&s, &g), [CtrlShape::Step; 3]);
    assert_eq!(cc1(&s, &g), [(0, 10), (480, 90), (960, 40)]);
}

#[test]
fn the_last_point_has_no_next_one_and_the_pedal_has_steps_only() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = point_at(&s, &g, i, 960, 40);
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "次のポイントまでの形"));

    // CC64 is a step and nothing else, so the row has nothing to offer and is dim.
    let c = s.roll_ctx(&g).unwrap();
    s.song
        .run(
            "cc.replace",
            json!({ "clip": c.clip.id.to_string(), "lane": "cc64", "from": 0, "to": 1920,
                    "points": [{ "tick": 0, "value": 100 }, { "tick": 960, "value": 0 }] }),
        )
        .unwrap();
    s.roll.lanes[i].kind = LaneKind::Ctrl(CtrlKind::Cc(64));
    s.resized(&g);
    let (x, y) = point_at(&s, &g, i, 0, 100);
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "次のポイントまでの形"));
}

#[test]
fn the_empty_part_of_a_lane_adds_a_point_pastes_and_clears_the_lane() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = lane_empty_at(&s, &g, i, 1440);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        ["ここにポイントを追加", "貼り付け", "すべてのポイントを削除"]
    );
    assert!(!enabled(&s, "貼り付け"));
    assert_eq!(key(&s, "貼り付け"), "Ctrl+V");
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ここにポイントを追加"));
    assert_eq!(steps(&s), n + 1);
    let after = cc1(&s, &g);
    assert_eq!(after.len(), 4);
    assert!(
        after.iter().any(|(t, _)| (1300..1600).contains(t)),
        "{after:?}"
    );
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g).len(), 3);

    // Points on the clipboard: the row lights and the paste lands at the press.
    s.roll.points = [0, 480].into_iter().collect();
    let (px, py) = point_at(&s, &g, i, 480, 90);
    assert!(s.scene_context(&g, px, py));
    assert!(choose(&mut s, &g, "コピー"));
    assert!(s.scene_context(&g, x, y));
    assert!(enabled(&s, "貼り付け"));
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(cc1(&s, &g).len(), 5);
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g).len(), 3);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "すべてのポイントを削除"));
    assert!(cc1(&s, &g).is_empty());
    assert!(s.song.undo());
    assert_eq!(cc1(&s, &g).len(), 3);
}

#[test]
fn the_tab_changes_the_controller_hides_the_lane_and_clears_its_points() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = tab_at(&s, &g, i);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        ["種類を変更", "このレーンを隠す", "すべてのポイントを削除"]
    );
    assert!(choose(&mut s, &g, "種類を変更"));
    assert_eq!(
        labels(&s),
        [
            "戻る",
            "ピッチベンド",
            "CC1",
            "CC11",
            "CC64",
            "アフタータッチ",
            "番号で選ぶ · CC2",
            "番号で選ぶ · CC7",
            "番号で選ぶ · CC10",
            "番号で選ぶ · CC71",
            "番号で選ぶ · CC74",
            "番号で選ぶ · CC91",
            "番号で選ぶ · CC93"
        ]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "CC11"));
    assert_eq!(steps(&s), n);
    assert_eq!(s.roll.lanes[i].kind, LaneKind::Ctrl(CtrlKind::Cc(11)));

    // A lane that is shown already cannot be picked again.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "種類を変更"));
    assert!(!enabled(&s, "CC11"));
    assert!(enabled(&s, "CC1"));
    assert!(labels(&s).contains(&"番号で選ぶ · CC2".to_string()));
    assert!(choose(&mut s, &g, "番号で選ぶ · CC74"));
    assert_eq!(s.roll.lanes[i].kind, LaneKind::Ctrl(CtrlKind::Cc(74)));

    // Hide: gone, and gone for good while the song changes.
    let count = s.roll.lanes.len();
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "このレーンを隠す"));
    assert_eq!(s.roll.lanes.len(), count - 1);
    let c = s.roll_ctx(&g).unwrap();
    s.song
        .run("note.mute", json!({ "ids": [c.notes()[0].id.to_string()] }))
        .unwrap();
    s.resized(&g);
    assert_eq!(s.roll.lanes.len(), count - 1);
}

#[test]
fn the_velocity_tab_cannot_be_hidden_and_has_no_points_to_clear() {
    let (mut s, g, _) = with_cc1();
    let (x, y) = tab_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "このレーンを隠す"));
    assert!(!enabled(&s, "すべてのポイントを削除"));
    assert!(enabled(&s, "種類を変更"));
}

#[test]
fn a_press_on_the_clearing_row_of_a_lane_with_points_runs_one_command() {
    let (mut s, g, i) = with_cc1();
    let (x, y) = tab_at(&s, &g, i);
    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "すべてのポイントを削除"));
    assert_eq!(steps(&s), n + 1);
    assert!(cc1(&s, &g).is_empty());
    // The notes were not touched.
    let (nx, ny) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, nx, ny));
    assert_eq!(notes(&s, &g).len(), 3);
}
