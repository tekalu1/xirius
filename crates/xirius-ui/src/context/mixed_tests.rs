//! A right press inside a selection of several kinds (CTX-007), pressed as the window presses.

use std::collections::BTreeSet;

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::TrackId;
use xirius_core::model::CtrlKind;

use super::roll_tests::{note_at, notes, roll};
use super::tests::{choose, enabled, labels, steps, studio};
use crate::arrange::probe::{clip_mid, point_xy, track, two_lanes};
use crate::roll::{LaneKind, y_of};
use crate::studio::{Geometry, Studio};

/// What the dim row says when the pointer rests on it.
fn reason(s: &Studio, label: &str) -> String {
    s.context_rows()
        .into_iter()
        .find(|r| r.label == label)
        .map(|r| r.reason)
        .expect("the row")
}

// ------------------------------------------------------------------ the arrangement

/// What `clip_and_point` selected: the track of the lane, the track of the clip and the point.
struct Picked {
    lane: TrackId,
    clip_track: TrackId,
    tick: i64,
}

/// "Bass" with its volume lane open (its clips are not to be taken hold of meanwhile), the
/// first clip of "Keys" and the volume point `i` selected together.
fn clip_and_point(s: &mut Studio, g: &Geometry, i: usize) -> Picked {
    let lane = two_lanes(s, g, "Bass");
    let clip_track = track(s, "Keys");
    let clip = s.project().tracks[&clip_track].clips[0].id;
    let vol = AutoTarget::Volume;
    let tick = s.project().tracks[&lane]
        .automation
        .iter()
        .find(|l| l.target == vol)
        .expect("the volume lane")
        .points[i]
        .tick;
    s.arrange.clips = BTreeSet::from([(clip_track, clip)]);
    s.selected_clip = Some((clip_track, clip));
    s.arrange.points.select(lane, &vol, tick, false);
    Picked {
        lane,
        clip_track,
        tick,
    }
}

fn volume_points(s: &Studio, t: TrackId) -> usize {
    s.project().tracks[&t]
        .automation
        .iter()
        .find(|l| l.target == AutoTarget::Volume)
        .expect("the volume lane")
        .points
        .len()
}

#[test]
fn a_point_pressed_inside_clips_and_points_shows_what_both_kinds_have() {
    let (mut s, g) = studio();
    let p = clip_and_point(&mut s, &g, 1);
    let (x, y) = point_xy(&s, &g, p.lane, 0, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["切り取り", "コピー", "削除"]);
    assert!(!enabled(&s, "切り取り"));
    assert!(!enabled(&s, "コピー"));
    assert!(enabled(&s, "削除"));
    assert_eq!(reason(&s, "コピー"), "選択の種類が混在しています");
    // Pressing a row that is dim changes nothing.
    let n = steps(&s);
    assert!(!choose(&mut s, &g, "コピー"));
    assert_eq!(steps(&s), n);
    assert!(s.arrange.points.has(p.lane, &AutoTarget::Volume, p.tick));
    assert_eq!(s.arrange.clips.len(), 1);
}

#[test]
fn a_clip_pressed_inside_clips_and_points_shows_the_same_rows() {
    let (mut s, g) = studio();
    let p = clip_and_point(&mut s, &g, 1);
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["切り取り", "コピー", "削除"]);
    assert!(!enabled(&s, "切り取り"));
    // The press kept both kinds.
    assert_eq!(s.arrange.clips.len(), 1);
    assert_eq!(s.arrange.points.ticks().len(), 1);
    assert!(s.arrange.points.lane().is_some_and(|(l, _)| *l == p.lane));
}

#[test]
fn delete_of_clips_and_points_takes_both_away() {
    let (mut s, g) = studio();
    let p = clip_and_point(&mut s, &g, 1);
    let clips = s.project().tracks[&p.clip_track].clips.len();
    let points = volume_points(&s, p.lane);
    let (x, y) = point_xy(&s, &g, p.lane, 0, 1);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "削除"));
    assert_eq!(s.project().tracks[&p.clip_track].clips.len(), clips - 1);
    assert_eq!(volume_points(&s, p.lane), points - 1);
    assert!(s.arrange.clips.is_empty());
    assert!(s.arrange.points.is_empty());
}

#[test]
fn a_point_that_is_not_selected_is_the_whole_selection_and_leaves_the_clips() {
    let (mut s, g) = studio();
    let p = clip_and_point(&mut s, &g, 1);
    let (x, y) = point_xy(&s, &g, p.lane, 0, 0);
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
    assert!(s.arrange.clips.is_empty());
    assert!(!s.arrange.points.has(p.lane, &AutoTarget::Volume, p.tick));
    assert_eq!(s.arrange.points.ticks().len(), 1);
}

#[test]
fn a_clip_that_is_not_selected_is_the_whole_selection_and_lets_the_points_go() {
    let (mut s, g) = studio();
    clip_and_point(&mut s, &g, 1);
    let (x, y) = clip_mid(&s, &g, "Drums", 0);
    assert!(s.scene_context(&g, x, y));
    assert!(labels(&s).contains(&"クリップを複製".to_string()));
    assert!(s.arrange.points.is_empty());
    assert_eq!(s.arrange.clips.len(), 1);
}

#[test]
fn the_delete_key_takes_clips_and_points_together() {
    let (mut s, g) = studio();
    let p = clip_and_point(&mut s, &g, 1);
    let clips = s.project().tracks[&p.clip_track].clips.len();
    let points = volume_points(&s, p.lane);
    s.arrange.focus = true;
    assert_eq!(s.clip_command(&g, "clip.delete", false), Some(true));
    assert_eq!(s.project().tracks[&p.clip_track].clips.len(), clips - 1);
    assert_eq!(volume_points(&s, p.lane), points - 1);
    assert!(s.arrange.clips.is_empty());
    assert!(s.arrange.points.is_empty());
}

// ------------------------------------------------------------------ the roll

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

/// The window point of the CC1 point at `tick` with `value`.
fn point_at(s: &Studio, g: &Geometry, index: usize, tick: i64, value: i32) -> (f32, f32) {
    let c = s.roll_ctx(g).unwrap();
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    let plot = f.lanes[index].plot;
    (v.x(tick), y_of(s.roll.lanes[index].kind, &plot, value))
}

fn cc1_ticks(s: &Studio, g: &Geometry) -> Vec<i64> {
    let c = s.roll_ctx(g).unwrap();
    s.project()
        .clip(c.clip.id)
        .unwrap()
        .1
        .controllers
        .iter()
        .find(|l| l.kind == CtrlKind::Cc(1))
        .map(|l| l.points.iter().map(|q| q.tick).collect())
        .unwrap_or_default()
}

/// Two notes and the point at 480 selected together, the CC1 lane active.
fn notes_and_point(s: &mut Studio, g: &Geometry, index: usize) {
    let ids: Vec<_> = notes(s, g).iter().take(2).map(|n| n.id).collect();
    s.roll.selected = ids.into_iter().collect();
    s.roll.active_lane = index;
    s.roll.points = [480].into();
}

#[test]
fn a_point_pressed_inside_notes_and_points_shows_what_both_kinds_have() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let (x, y) = point_at(&s, &g, index, 480, 90);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["切り取り", "コピー", "ノートを削除"]);
    assert!(!enabled(&s, "切り取り"));
    assert!(!enabled(&s, "コピー"));
    assert!(enabled(&s, "ノートを削除"));
    assert_eq!(reason(&s, "切り取り"), "選択の種類が混在しています");
    assert_eq!(s.roll.selected.len(), 2);
    assert_eq!(s.roll.points.len(), 1);
}

#[test]
fn a_note_pressed_inside_notes_and_points_shows_the_same_rows() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let (x, y) = note_at(&s, &g, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(labels(&s), ["切り取り", "コピー", "ノートを削除"]);
    assert_eq!(s.roll.selected.len(), 2);
    assert_eq!(s.roll.points.len(), 1);
}

#[test]
fn delete_of_notes_and_points_takes_both_away() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let (x, y) = point_at(&s, &g, index, 480, 90);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "ノートを削除"));
    assert_eq!(cc1_ticks(&s, &g), [0, 960]);
    let left: Vec<_> = notes(&s, &g).iter().map(|n| n.pitch).collect();
    assert_eq!(left, [64]);
    assert!(s.roll.selected.is_empty());
    assert!(s.roll.points.is_empty());
}

#[test]
fn the_delete_key_takes_notes_and_points_together() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let ctx = s.roll_ctx(&g).unwrap();
    let out = s.roll.command(&ctx, "note.delete", &serde_json::Map::new());
    s.apply_roll(&g, out);
    assert_eq!(cc1_ticks(&s, &g), [0, 960]);
    assert_eq!(notes(&s, &g).len(), 1);
}

#[test]
fn the_delete_key_with_only_points_leaves_the_notes() {
    let (mut s, g, index) = with_cc1();
    s.roll.active_lane = index;
    s.roll.points = [480].into();
    let ctx = s.roll_ctx(&g).unwrap();
    let out = s.roll.command(&ctx, "note.delete", &serde_json::Map::new());
    s.apply_roll(&g, out);
    assert_eq!(cc1_ticks(&s, &g), [0, 960]);
    assert_eq!(notes(&s, &g).len(), 3);
}

#[test]
fn a_point_that_is_not_selected_is_the_whole_selection_and_leaves_the_notes() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let (x, y) = point_at(&s, &g, index, 960, 40);
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
    assert!(s.roll.selected.is_empty());
    assert_eq!(s.roll.points.iter().copied().collect::<Vec<_>>(), [960]);
}

#[test]
fn a_note_that_is_not_selected_is_the_whole_selection_and_lets_the_points_go() {
    let (mut s, g, index) = with_cc1();
    notes_and_point(&mut s, &g, index);
    let (x, y) = note_at(&s, &g, 2);
    assert!(s.scene_context(&g, x, y));
    assert!(labels(&s).contains(&"ノートを複製".to_string()));
    assert!(s.roll.points.is_empty());
    assert_eq!(s.roll.selected.len(), 1);
}
