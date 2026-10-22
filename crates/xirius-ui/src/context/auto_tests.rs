//! The menus of the automation lanes (S17, S18, S19), pressed as the window presses them.

use xirius_core::auto::{AutoLane, AutoTarget};
use xirius_core::id::TrackId;

use super::tests::{choose, enabled, labels, steps, studio};
use crate::arrange::probe::{point_xy, two_lanes};
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

fn lane(s: &Studio, t: TrackId, target: &AutoTarget) -> AutoLane {
    s.project().tracks[&t]
        .automation
        .iter()
        .find(|l| l.target == *target)
        .expect("the lane")
        .clone()
}

fn ticks(s: &Studio, t: TrackId, target: &AutoTarget) -> Vec<i64> {
    lane(s, t, target).points.iter().map(|p| p.tick).collect()
}

fn on(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect()
}

/// A press on the line between volume point 0 and 1 of "Bass", and on empty lane above it.
fn mid_line(s: &Studio, g: &Geometry, t: TrackId, lane: usize) -> (f32, f32) {
    let (xa, ya) = point_xy(s, g, t, lane, 0);
    let (xb, yb) = point_xy(s, g, t, lane, 1);
    ((xa + xb) / 2.0, (ya + yb) / 2.0)
}

#[test]
fn a_muted_send_curve_remains_editable_and_shape_changes_undo_once() {
    let (mut s, g) = studio();
    // Undo here must land exactly on the state the test kept; the
    // instrument-input arm that would follow the selection is not
    // what this test checks (REC-024).
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let t = crate::arrange::probe::track(&s, "Bass");
    let bus = s.song.run("track.add.bus", serde_json::json!({})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.song
        .run(
            "send.add",
            serde_json::json!({"ids":[t.to_string()],"to":bus}),
        )
        .unwrap();
    let sid = s.project().tracks[&t].sends[0].id;
    let target = AutoTarget::Send(sid);
    s.song
        .run("send.mute", serde_json::json!({"id":sid.to_string()}))
        .unwrap();
    for (at, value) in [(4 * super::tests::BAR, -18), (6 * super::tests::BAR, -6)] {
        s.song.run("auto.point.add",serde_json::json!({"track":t.to_string(),"target":target.to_string(),"at":at,"value":value})).unwrap();
    }
    s.arrange.open.insert(t);
    s.rebuild(&g);
    let idx = s
        .arrange
        .lanes_of(&s.project().tracks[&t])
        .iter()
        .position(|l| l.target == target)
        .unwrap();
    let (x, y) = mid_line(&s, &g, t, idx);
    let before = s.project();
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "この線の形"));
    assert!(choose(&mut s, &g, "階段"));
    assert!(lane(&s, t, &target).points[0].step);
    assert!(lane(&s, t, &target).suspended);
    s.undo(&g);
    assert_eq!(s.project(), before);
}

#[test]
fn a_point_has_the_rows_of_the_mock() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let (x, y) = point_xy(&s, &g, t, 0, 1);
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
    assert!(enabled(&s, "次のポイントまでの形"));
    // Pressing a point that was not selected selected it alone.
    let vol = AutoTarget::Volume;
    let tick = ticks(&s, t, &vol)[1];
    assert!(s.arrange.points.has(t, &vol, tick));
    assert_eq!(s.arrange.points.ticks().len(), 1);
}

#[test]
fn the_last_point_has_no_line_after_it() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let (x, y) = point_xy(&s, &g, t, 0, 2);
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "次のポイントまでの形"));
}

#[test]
fn the_shape_to_the_next_point_is_a_line_a_curve_or_a_stair_and_one_undo_takes_it_back() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let pan = AutoTarget::Pan;
    let before = s.project().content_hash();
    let (x, y) = point_xy(&s, &g, t, 1, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    assert_eq!(labels(&s), ["戻る", "直線", "曲線", "階段"]);
    assert_eq!(on(&s), ["直線"]);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "階段"));
    assert_eq!(steps(&s), n + 1);
    let l = lane(&s, t, &pan);
    assert!(l.points[0].step);
    // Halfway along, the stair holds the first value.
    let mid = (l.points[0].tick + l.points[1].tick) as f64 / 2.0;
    assert_eq!(l.value_at(mid), Some(l.points[0].value));

    // The stair is the lit row now; 曲線 takes it away.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "次のポイントまでの形"));
    assert_eq!(on(&s), ["階段"]);
    assert!(choose(&mut s, &g, "曲線"));
    let p = lane(&s, t, &pan).points[0];
    assert!(!p.step);
    assert_eq!(p.curve, crate::autovalue::CURVE);
    assert!(s.song.undo());
    assert!(lane(&s, t, &pan).points[0].step);
    assert!(s.song.undo());
    assert_eq!(s.project().content_hash(), before);
}

#[test]
fn delete_takes_every_selected_point_in_one_step() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let vol = AutoTarget::Volume;
    let all = ticks(&s, t, &vol);
    // Two points selected; the press is on one of them.
    s.arrange.points.select(t, &vol, all[0], false);
    s.arrange.points.select(t, &vol, all[1], true);
    let (x, y) = point_xy(&s, &g, t, 0, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(s.arrange.points.ticks().len(), 2);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "削除"));
    assert_eq!(ticks(&s, t, &vol), [all[2]]);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(ticks(&s, t, &vol), all);
}

#[test]
fn copy_and_paste_carry_the_selected_points_to_where_the_press_was() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let pan = AutoTarget::Pan;
    let (x, y) = point_xy(&s, &g, t, 1, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "コピー"));
    assert_eq!(ticks(&s, t, &pan).len(), 2);
    // An empty part of the lane takes them.
    let (xl, yl) = point_xy(&s, &g, t, 1, 1);
    assert!(s.scene_context(&g, xl + 120.0, yl));
    assert!(enabled(&s, "貼り付け"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(ticks(&s, t, &pan).len(), 3);
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn a_press_on_a_line_adds_a_point_on_it_and_shapes_it() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let vol = AutoTarget::Volume;
    let (x, y) = mid_line(&s, &g, t, 0);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        [
            "ここにポイントを追加",
            "この線の形",
            "貼り付け",
            "ループの中のポイントを削除",
            "すべてのポイントを削除"
        ]
    );
    // Nothing was copied.
    assert!(!enabled(&s, "貼り付け"));
    assert!(enabled(&s, "この線の形"));
    let before = lane(&s, t, &vol);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ここにポイントを追加"));
    let l = lane(&s, t, &vol);
    assert_eq!(l.points.len(), before.points.len() + 1);
    assert_eq!(steps(&s), n + 1);
    // The new point sits on the line it was added to.
    let at = l.points[1].tick;
    assert_eq!(before.value_at(at as f64), Some(l.points[1].value));

    // The shape of that line: the first segment now.
    let (x, y) = mid_line(&s, &g, t, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "この線の形"));
    assert_eq!(labels(&s), ["戻る", "直線", "曲線", "階段"]);
    assert_eq!(on(&s), ["直線"]);
    assert!(choose(&mut s, &g, "階段"));
    assert!(lane(&s, t, &vol).points[0].step);
}

#[test]
fn an_empty_part_of_a_lane_adds_a_point_where_it_was_pressed_and_clears() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let pan = AutoTarget::Pan;
    let (xb, yb) = point_xy(&s, &g, t, 1, 1);
    // Right of the last point: no line there.
    assert!(s.scene_context(&g, xb + 200.0, yb));
    assert!(!enabled(&s, "この線の形"));
    let before = s.project().content_hash();
    assert!(choose(&mut s, &g, "ここにポイントを追加"));
    assert_eq!(ticks(&s, t, &pan).len(), 3);

    // すべてのポイントを削除 empties the lane in one step.
    assert!(s.scene_context(&g, xb + 200.0, yb - 10.0));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "すべてのポイントを削除"));
    assert!(ticks(&s, t, &pan).is_empty());
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert!(s.song.undo());
    assert_eq!(s.project().content_hash(), before);
}

#[test]
fn the_head_of_a_lane_adds_a_lane_hides_it_and_clears_it() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let vol = AutoTarget::Volume;
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let (x, y) = (g.column.x + 20.0, s.lane_top(&g, &row, 0) + 24.0);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        ["レーンを追加", "このレーンを隠す", "すべてのポイントを削除"]
    );
    // The lanes this track can still take are the rows below.
    assert!(choose(&mut s, &g, "レーンを追加"));
    assert!(labels(&s).len() > 1);
    assert_eq!(labels(&s)[0], "戻る");

    // 隠す hides the lane the press was on, not the next one.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "このレーンを隠す"));
    assert!(lane(&s, t, &vol).hidden);

    // The points stay when only the lane is hidden; すべてのポイントを削除 empties the pan lane.
    let pan = AutoTarget::Pan;
    let (x, y) = (g.column.x + 20.0, s.lane_top(&g, &row, 0) + 24.0);
    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "すべてのポイントを削除"));
    assert!(ticks(&s, t, &pan).is_empty());
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn a_row_off_the_lane_head_is_still_the_track_menu() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let (x, y) = crate::arrange::probe::row_mid(&s, &g, RowKind::Track(t));
    assert!(s.scene_context(&g, x, y));
    assert!(labels(&s).contains(&"トラック名を変更".to_string()));
}

#[test]
fn a_range_over_a_lane_makes_that_lane_a_line_and_leaves_the_others() {
    let (mut s, g) = studio();
    let t = two_lanes(&mut s, &g, "Bass");
    let vol = AutoTarget::Volume;
    let tk = ticks(&s, t, &vol);
    s.arrange.range = Some((tk[0], tk[2]));
    let (x, y) = mid_line(&s, &g, t, 0);
    assert!(s.scene_context(&g, x, y));
    let rows = labels(&s);
    for want in [
        "コピー",
        "切り取り",
        "範囲を直線にする",
        "範囲を平らにする",
        "時間を挿入",
    ] {
        assert!(rows.contains(&want.to_string()), "{want}: {rows:?}");
    }
    let pan = lane(&s, t, &AutoTarget::Pan);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "範囲を直線にする"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(
        ticks(&s, t, &vol)
            .iter()
            .filter(|k| **k > tk[0] && **k < tk[2])
            .count(),
        0
    );
    assert_eq!(lane(&s, t, &AutoTarget::Pan), pan);
    // The transform rows are a menu of their own, and a trim runs in one step.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "範囲をトランスフォーム"));
    assert!(choose(&mut s, &g, "縮小 50%"));
    assert_eq!(steps(&s), n + 2);
}

#[test]
fn a_range_over_the_tempo_row_has_the_shape_rows_but_no_curve() {
    let (mut s, g) = studio();
    let bar = super::tests::BAR;
    s.song
        .run("tempo.set", serde_json::json!({"at": 2 * bar, "bpm": 150}))
        .unwrap();
    s.arrange.range = Some((bar, 3 * bar));
    let x = s.tick_x(&g, bar + bar / 2);
    assert!(s.scene_context(&g, x, g.tempo.y + 4.0));
    let rows = labels(&s);
    assert!(rows.contains(&"範囲を直線にする".to_string()), "{rows:?}");
    assert!(!rows.contains(&"範囲をカーブにする".to_string()));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "範囲を平らにする"));
    assert_eq!(steps(&s), n + 1);
}
