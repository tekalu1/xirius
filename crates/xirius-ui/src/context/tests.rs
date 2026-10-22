//! The right-click menus, pressed as the window presses them: the point of the press
//! opens the menu, and a row is chosen by the press on its cell.

use xirius_core::time::Tick;

use crate::arrange::probe::{clip_mid, row_at, track};
use crate::studio::{Geometry, Hit, Studio};
use crate::tree::RowKind;

pub(super) const BAR: Tick = 3840;

pub(super) fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    // The arrangement alone, tall enough for every row.
    s.drawer_open = false;
    s.snap_mode = crate::tools::SnapMode::Bar;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

pub(super) fn steps(s: &Studio) -> usize {
    s.song.steps()
}

/// The labels of the rows, gaps left out.
pub(super) fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

/// Whether the row named `label` can be chosen.
pub(super) fn enabled(s: &Studio, label: &str) -> bool {
    s.context_rows()
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
        .enabled
}

/// The key shown on the row named `label`.
pub(super) fn key(s: &Studio, label: &str) -> String {
    s.context_rows()
        .iter()
        .find(|r| r.label == label)
        .unwrap_or_else(|| panic!("no row {label}: {:?}", labels(s)))
        .key
        .clone()
}

/// Presses the cell of the row named `label`, where the pop-over draws it.
pub(super) fn choose(s: &mut Studio, g: &Geometry, label: &str) -> bool {
    let layout = s.popup.as_ref().expect("a menu is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
        .unwrap_or_else(|| panic!("no cell {label}"));
    let (x, y) = (
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    let hit = s.hit(g, x, y);
    assert!(
        matches!(hit, Hit::Item(_)) || !cell.live(),
        "{label}: {hit:?}"
    );
    s.press(g, hit)
}

/// The middle of the empty lane of `name` at bar `bar`.
pub(super) fn lane_at(s: &Studio, g: &Geometry, name: &str, bar: i64) -> (f32, f32) {
    let t = track(s, name);
    let x = s.tick_x(g, (bar - 1) * BAR) + 4.0;
    assert!(x < g.lanes.right(), "bar {bar} is off screen");
    (x, row_at(s, g, RowKind::Track(t), 0.5))
}

#[test]
fn a_press_on_an_empty_lane_opens_its_menu_and_a_row_runs_by_the_command() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    assert!(s.context_open());
    assert_eq!(
        labels(&s),
        [
            "貼り付け",
            "MIDI クリップを作成",
            "再生位置を移動",
            "マーカーを追加",
            "選択"
        ]
    );
    // Nothing was copied: paste is dim, and the row keeps its place.
    assert!(!enabled(&s, "貼り付け"));
    assert_eq!(key(&s, "貼り付け"), "Ctrl+V");
    assert!(enabled(&s, "MIDI クリップを作成"));
    let keys = track(&s, "Keys");
    let before = s.project().tracks[&keys].clips.len();
    let n = steps(&s);
    assert!(choose(&mut s, &g, "MIDI クリップを作成"));
    assert!(!s.context_open());
    let clips = &s.project().tracks[&keys].clips;
    assert_eq!(clips.len(), before + 1);
    assert_eq!(clips.last().map(|c| c.start), Some(12 * BAR));
    assert_eq!(steps(&s), n + 1);
    s.undo(&g);
    assert_eq!(s.project().tracks[&keys].clips.len(), before);
}

#[test]
fn an_audio_track_cannot_take_a_midi_clip_and_a_dim_row_does_nothing() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Vocal", 14);
    assert!(s.scene_context(&g, x, y));
    assert!(!enabled(&s, "MIDI クリップを作成"));
    let n = steps(&s);
    assert!(!choose(&mut s, &g, "MIDI クリップを作成"));
    assert_eq!(steps(&s), n);
}

#[test]
fn paste_puts_the_copied_clip_where_the_menu_was_opened() {
    let (mut s, g) = studio();
    let (mx, my) = clip_mid(&s, &g, "Keys", 0);
    s.arrange_down(&g, mx, my, Default::default());
    s.arrange_up(&g, mx, my, Default::default());
    assert!(s.edit_key(&g, "c", crate::tools::Mods::ctrl()));
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    assert!(enabled(&s, "貼り付け"));
    // Where the menu was opened, on the grid.
    let at = s.ctx_tick(&g, x);
    assert!((at - 12 * BAR).abs() < BAR / 4);
    let keys = track(&s, "Keys");
    let before = s.project().tracks[&keys].clips.len();
    let n = steps(&s);
    assert!(choose(&mut s, &g, "貼り付け"));
    let p = s.project();
    assert_eq!(p.tracks[&keys].clips.len(), before + 1);
    assert!(p.tracks[&keys].clips.iter().any(|c| c.start == at));
    // The pasted clip is the selection.
    assert_eq!(s.arrange.clips.len(), 1);
    assert_eq!(steps(&s), n + 1);
}

#[test]
fn the_playhead_moves_to_the_press() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "再生位置を移動"));
    assert_eq!(s.playhead, 12 * BAR);
}

#[test]
fn a_row_with_rows_below_opens_them_in_its_place_and_goes_back() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "選択"));
    assert!(s.context_open());
    let rows = labels(&s);
    assert_eq!(rows[0], "戻る");
    assert!(rows.contains(&"すべて選択".to_string()));
    assert_eq!(key(&s, "すべて選択"), "Ctrl+A");
    assert!(choose(&mut s, &g, "戻る"));
    assert_eq!(labels(&s)[0], "貼り付け");
    // Every row of the second level has a command behind it.
    assert!(choose(&mut s, &g, "選択"));
    assert!(choose(&mut s, &g, "すべて選択"));
    assert!(!s.context_open());
    assert!(!s.arrange.clips.is_empty());
}

#[test]
fn a_second_press_replaces_the_menu_and_a_press_outside_closes_it() {
    let (mut s, g) = studio();
    let (x, y) = lane_at(&s, &g, "Keys", 13);
    assert!(s.scene_context(&g, x, y));
    let (x2, y2) = lane_at(&s, &g, "Vocal", 14);
    assert!(s.scene_context(&g, x2, y2));
    assert!(!enabled(&s, "MIDI クリップを作成"));
    s.press(&g, Hit::None);
    assert!(!s.context_open());
}
