//! The menus of the piano roll's grid (S20, S21), pressed as the window presses them.

use serde_json::json;
use xirius_core::model::Note;

use super::tests::{choose, enabled, key, labels, steps};
use crate::roll::ids_json;
use crate::studio::{Geometry, Studio};

/// The sample song with three notes in the Keys clip, the roll open on it:
/// 60 at 960 (length 960, velocity 80), 62 at 2880 (1440, 90), 64 at 4800 (480, 100).
pub(super) fn roll() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    let ctx = s.roll_ctx(&g).unwrap();
    s.song
        .run(
            "note.delete",
            json!({ "ids": ids_json(ctx.notes().iter().map(|n| &n.id)) }),
        )
        .unwrap();
    s.song
        .run(
            "note.put",
            json!({ "clip": ctx.clip.id.to_string(), "notes": [
                { "at": 960, "pitch": 60, "length": 960, "velocity": 80 },
                { "at": 2880, "pitch": 62, "length": 1440, "velocity": 90 },
                { "at": 4800, "pitch": 64, "length": 480, "velocity": 100 },
            ] }),
        )
        .unwrap();
    s.resized(&g);
    (s, g)
}

pub(super) fn notes(s: &Studio, g: &Geometry) -> Vec<Note> {
    s.roll_ctx(g).unwrap().notes().to_vec()
}

/// The middle of note `i` in the grid.
pub(super) fn note_at(s: &Studio, g: &Geometry, i: usize) -> (f32, f32) {
    let c = s.roll_ctx(g).unwrap();
    let v = s.roll.view(&c);
    let n = &c.notes()[i];
    (
        (v.x(n.start) + v.x(n.start + n.length)) / 2.0,
        v.y(n.pitch) + v.row_h / 2.0,
    )
}

/// A point of the grid with no note under it.
fn empty_at(s: &Studio, g: &Geometry) -> (f32, f32) {
    let c = s.roll_ctx(g).unwrap();
    let v = s.roll.view(&c);
    (v.x(3000), v.y(61) + v.row_h / 2.0)
}

#[test]
fn a_note_has_the_rows_of_the_mock_and_is_selected_alone() {
    let (mut s, g) = roll();
    let (x, y) = note_at(&s, &g, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        [
            "クオンタイズ",
            "トランスポーズ",
            "ベロシティ",
            "長さ",
            "ノートを複製",
            "ノートをミュート",
            "値を入力",
            "消しゴムツール",
            "切り取り",
            "コピー",
            "ノートを削除"
        ]
    );
    assert_eq!(key(&s, "ノートを複製"), "D");
    assert_eq!(key(&s, "ノートをミュート"), "Shift+M");
    assert_eq!(key(&s, "ノートを削除"), "Del / Backspace");
    let c = s.roll_ctx(&g).unwrap();
    let only = c.notes()[1].id;
    assert_eq!(s.roll.selected.len(), 1);
    assert!(s.roll.selected.contains(&only));
}

#[test]
fn transposing_by_the_rows_moves_every_selected_note_in_one_step() {
    let (mut s, g) = roll();
    let before = notes(&s, &g);
    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "トランスポーズ"));
    assert_eq!(labels(&s), ["戻る", "+1", "−1", "+12", "−12"]);
    let n = steps(&s);
    assert!(choose(&mut s, &g, "+12"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(notes(&s, &g)[0].pitch, before[0].pitch + 12);
    assert_eq!(notes(&s, &g)[1].pitch, before[1].pitch);
    assert!(s.song.undo());
    assert_eq!(notes(&s, &g)[0].pitch, before[0].pitch);

    // Every note picked, the press on one of them: they all go.
    let all: std::collections::BTreeSet<_> = before.iter().map(|n| n.id).collect();
    s.roll.selected = all.into_iter().collect();
    let (x, y) = note_at(&s, &g, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(s.roll.selected.len(), 3);
    assert!(choose(&mut s, &g, "トランスポーズ"));
    assert!(choose(&mut s, &g, "−1"));
    let after = notes(&s, &g);
    for (a, b) in after.iter().zip(&before) {
        assert_eq!(a.pitch, b.pitch - 1);
    }
}

#[test]
fn the_strength_rows_raise_lower_even_out_and_take_a_typed_value() {
    let (mut s, g) = roll();
    s.roll.selected = notes(&s, &g).iter().map(|n| n.id).collect();
    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "ベロシティ"));
    assert_eq!(labels(&s), ["戻る", "値を入力…", "強く", "弱く", "揃える"]);
    assert!(choose(&mut s, &g, "強く"));
    let v: Vec<u8> = notes(&s, &g).iter().map(|n| n.velocity).collect();
    assert_eq!(v, [90, 100, 110]);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "ベロシティ"));
    assert!(choose(&mut s, &g, "揃える"));
    let v: Vec<u8> = notes(&s, &g).iter().map(|n| n.velocity).collect();
    assert_eq!(v, [100, 100, 100]);

    // 値を入力…: the menu turns into a box; Enter sets the value on every note.
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "ベロシティ"));
    assert!(choose(&mut s, &g, "値を入力…"));
    for _ in 0..4 {
        s.ask_key("backspace");
    }
    s.ask_text(None, "64", false);
    let n = steps(&s);
    assert_eq!(s.ask_key("enter"), Some(true));
    assert!(!s.context_open());
    assert_eq!(steps(&s), n + 1);
    assert!(notes(&s, &g).iter().all(|n| n.velocity == 64));
    assert!(s.song.undo());
    assert!(notes(&s, &g).iter().all(|n| n.velocity == 100));
}

#[test]
fn the_length_rows_reach_the_next_note_double_halve_and_take_a_grid() {
    let (mut s, g) = roll();
    let before = notes(&s, &g);
    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "長さ"));
    assert_eq!(
        labels(&s),
        ["戻る", "次のノートまで", "グリッド 1 つ分", "2 倍", "半分"]
    );
    let n = steps(&s);
    assert!(choose(&mut s, &g, "次のノートまで"));
    assert_eq!(steps(&s), n + 1);
    // The first note ran to the second one's start.
    assert_eq!(notes(&s, &g)[0].length, before[1].start - before[0].start);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "長さ"));
    assert!(choose(&mut s, &g, "半分"));
    assert_eq!(
        notes(&s, &g)[0].length,
        (before[1].start - before[0].start) / 2
    );

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "長さ"));
    assert!(choose(&mut s, &g, "2 倍"));
    assert_eq!(notes(&s, &g)[0].length, before[1].start - before[0].start);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "長さ"));
    assert!(choose(&mut s, &g, "グリッド 1 つ分"));
    let step = xirius_core::time::parse_grid(s.song.grid(), s.project().ppq).unwrap();
    assert_eq!(notes(&s, &g)[0].length, step);
}

#[test]
fn duplicate_mute_cut_copy_and_delete_run_the_keys_of_the_roll() {
    let (mut s, g) = roll();
    let (x, y) = note_at(&s, &g, 2);

    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ノートをミュート"));
    assert_eq!(steps(&s), n + 1);
    assert!(notes(&s, &g)[2].muted);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "ノートを複製"));
    assert_eq!(notes(&s, &g).len(), 4);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "コピー"));
    assert!(!s.roll.clipboard.is_empty());

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "切り取り"));
    assert_eq!(notes(&s, &g).len(), 3);

    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "ノートを削除"));
    assert_eq!(notes(&s, &g).len(), 2);
    assert_eq!(steps(&s), n + 1);
    assert!(s.song.undo());
    assert_eq!(notes(&s, &g).len(), 3);
}

#[test]
fn quantize_runs_to_the_grid_and_at_half_strength() {
    let (mut s, g) = roll();
    // Put the first note off the grid.
    let id = notes(&s, &g)[0].id;
    s.song
        .run(
            "note.set",
            json!({ "notes": [{ "id": id.to_string(), "start": 1000 }] }),
        )
        .unwrap();
    let (x, y) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert_eq!(labels(&s), ["戻る", "今のグリッド", "50%"]);
    assert_eq!(key(&s, "今のグリッド"), "Q");
    assert!(choose(&mut s, &g, "50%"));
    let half = notes(&s, &g)[0].start;
    assert!(half < 1000 && half > 960, "{half}");

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "クオンタイズ"));
    assert!(choose(&mut s, &g, "今のグリッド"));
    assert_eq!(notes(&s, &g)[0].start % 240, 0);
}

#[test]
fn the_empty_grid_pastes_selects_inverts_and_sets_the_grid() {
    let (mut s, g) = roll();
    let (x, y) = empty_at(&s, &g);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(
        labels(&s),
        [
            "貼り付け",
            "すべて選択",
            "選択を反転",
            "グリッド",
            "消しゴムツール"
        ]
    );
    // Nothing was copied yet.
    assert!(!enabled(&s, "貼り付け"));
    assert!(choose(&mut s, &g, "すべて選択"));
    assert_eq!(s.roll.selected.len(), 3);

    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "選択を反転"));
    assert!(s.roll.selected.is_empty());

    // Copy one note, then paste where the press was.
    let (nx, ny) = note_at(&s, &g, 0);
    assert!(s.scene_context(&g, nx, ny));
    assert!(choose(&mut s, &g, "コピー"));
    assert!(s.scene_context(&g, x, y));
    assert!(enabled(&s, "貼り付け"));
    let n = steps(&s);
    assert!(choose(&mut s, &g, "貼り付け"));
    assert_eq!(steps(&s), n + 1);
    assert_eq!(notes(&s, &g).len(), 4);

    // The pasted note sits where the press was; the grid rows are asked from empty grid.
    let (x, y) = (x, y + 4.0 * s.roll.view(&s.roll_ctx(&g).unwrap()).row_h);
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "グリッド"));
    assert_eq!(labels(&s), ["戻る", "1/4", "1/8", "1/16", "1/32", "3 連"]);
    assert!(choose(&mut s, &g, "1/8"));
    assert_eq!(s.song.grid_for("roll"), "1/8");
    assert!(s.scene_context(&g, x, y));
    assert!(choose(&mut s, &g, "グリッド"));
    assert!(choose(&mut s, &g, "3 連"));
    assert_eq!(s.song.grid_for("roll"), "1/8t");
}

#[test]
fn a_press_outside_the_grid_opens_no_roll_menu() {
    let (mut s, g) = roll();
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    // The ruler above the grid is none of the roll's menus.
    assert!(!s.roll_context(&g, v.grid.x + 20.0, v.grid.y - 6.0));
    assert!(!s.context_open());
}

/// KEY-025: the key on a row is the keymap's now, so a remap shows and an unbound row has none.
#[test]
fn the_key_on_a_row_follows_the_keymap() {
    let (mut s, g) = roll();
    let (x, y) = note_at(&s, &g, 1);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(key(&s, "ノートを複製"), "D");
    s.popup = None;
    s.song
        .run(
            "keymap.set",
            json!({"key":"F9","command":"note.duplicate","context":"roll","from":"D"}),
        )
        .unwrap();
    assert!(s.scene_context(&g, x, y));
    assert_eq!(key(&s, "ノートを複製"), "F9");
    s.popup = None;
    s.song
        .run("keymap.reset", json!({"command":"note.duplicate"}))
        .unwrap();
    s.song
        .run(
            "keymap.unset",
            json!({"key":"D","command":"note.duplicate"}),
        )
        .unwrap();
    assert!(s.scene_context(&g, x, y));
    assert_eq!(key(&s, "ノートを複製"), "");
}

/// Shift+F10 and the menu key in the roll open the selected note's menu, where a press on the
/// note would open it.
#[test]
fn shift_f10_and_the_menu_key_open_the_selected_notes_menu() {
    use crate::tools::Mods;
    let (mut s, g) = roll();
    s.arrange.focus = false;
    assert_eq!(s.input_context(&g), "roll");
    s.roll.selected.clear();
    assert!(!s.mapped_key(&g, "f10", None, Mods::shift()));
    assert!(s.popup.is_none());
    let at = note_at(&s, &g, 1);
    assert!(s.scene_context(&g, at.0, at.1));
    let by_mouse = labels(&s);
    assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    assert!(s.popup.is_none());
    for (key, mods) in [("f10", Mods::shift()), ("menu", Mods::NONE)] {
        assert!(s.mapped_key(&g, key, None, mods), "{key}");
        assert_eq!(labels(&s), by_mouse, "{key}");
        let anchor = s.popup.as_ref().unwrap().anchor;
        assert_eq!((anchor.x, anchor.y), at, "{key}");
        assert!(s.mapped_key(&g, "escape", None, Mods::NONE));
    }
    // The menu's rows run on that note.
    let n = steps(&s);
    assert!(s.mapped_key(&g, "f10", None, Mods::shift()));
    assert!(choose(&mut s, &g, "ノートを削除"), "{:?}", labels(&s));
    assert_eq!(steps(&s), n + 1);
    let left: Vec<u8> = notes(&s, &g).iter().map(|n| n.pitch).collect();
    assert_eq!(left, [60, 64]);
}
