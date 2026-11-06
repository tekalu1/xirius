//! The track head's width and its volume fader (UI-312, UI-324): the parts at the right follow
//! the column's right edge, the name takes the rest, and the fader shows from 64 wide and moves
//! the value the mixer's fader moves.

use serde_json::json;
use xirius_core::id::TrackId;
use xirius_core::model::{FADER_FLOOR_DB, MAX_DB};
use xirius_design::Space;

use crate::a11y;
use crate::head_fader::{FINE_DB, PAGE_DB, STEP_DB};
use crate::studio::{Geometry, Hit, RowParts, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap_or_else(|| panic!("no track {name}"))
}

fn volume(s: &Studio, t: TrackId) -> f64 {
    s.project().tracks[&t].volume_db
}

/// A song whose head is dragged to `width` (as wide as its rows need, at the least).
fn studio(width: f32) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.side_size.head = Some(width);
    let g = s.geometry(1600.0, 1000.0);
    s.rebuild(&g);
    (s, g)
}

fn set(s: &mut Studio, t: TrackId, db: f64) {
    assert!(s.run_command(
        "track.volume.set",
        json!({ "ids": [t.to_string()], "db": db })
    ));
}

/// The middle of the fader of `t`, which the pointer hits.
fn mid(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let r = s.head_fader_rect(g, t).expect("a fader on show");
    let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert_eq!(s.hit(g, at.0, at.1), Hit::HeadFader(t));
    at
}

fn none() -> Mods {
    Mods::default()
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

/// Presses on the fader, moves sideways by `dx` and lets go, as the window does.
fn drag(s: &mut Studio, g: &Geometry, t: TrackId, dx: f32, mods: Mods) {
    let (x, y) = mid(s, g, t);
    assert!(s.head_fader_down(g, Hit::HeadFader(t), x, 1, mods));
    assert_eq!(s.head_fader.dragging, Some(t));
    assert!(s.drag_move_at(x + dx / 2.0, y, mods.shift));
    assert!(s.drag_move_at(x + dx, y, mods.shift));
    assert!(s.drag_end());
    assert_eq!(s.head_fader.dragging, None);
}

fn key(s: &mut Studio, k: &str, mods: Mods) -> bool {
    s.head_fader_key(k, mods)
}

fn focus(s: &mut Studio, g: &Geometry, t: TrackId) {
    let (x, y) = mid(s, g, t);
    s.head_fader_down(g, Hit::HeadFader(t), x, 1, none());
    s.drag_end();
    assert_eq!(s.head_fader.focus, Some(t));
    let _ = y;
}

#[test]
fn the_column_s_width_moves_the_right_parts_and_the_name_takes_the_rest() {
    let (s, _) = studio(216.0);
    let keys = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(keys)).unwrap();
    let lo = row.head_w;
    assert!(lo < 340.0, "the content is {lo} wide");
    let base = RowParts::at(&row, lo);
    let edge = Space::X2.px();
    for w in [lo, 340.0, 420.0] {
        let p = RowParts::at(&row, w);
        // The meter sits on the column's right edge, the marks left of it by the same gaps.
        let (m, a, wd) = (p.meter.unwrap(), p.auto.unwrap(), p.widen.unwrap());
        assert!((m.right() - (w - edge)).abs() < 0.01, "{w}");
        assert!(
            (wd.x - base.widen.unwrap().x - (w - lo)).abs() < 0.01,
            "{w}"
        );
        assert!((a.x - base.auto.unwrap().x - (w - lo)).abs() < 0.01, "{w}");
        // The name keeps its left edge and takes what the width gives.
        assert_eq!(p.name_x, base.name_x);
        assert!(
            (p.name_right - base.name_right - (w - lo)).abs() < 0.01,
            "{w}"
        );
        // What sits at the left does not move.
        assert_eq!(p.mute, base.mute);
        assert_eq!(p.keys, base.keys);
    }
    // The geometry gives the dragged width to every part (the cause of the bug).
    let (s, g) = studio(340.0);
    assert!((g.column.w - 340.0).abs() < 0.01);
    let row = *s.tree().row_of(RowKind::Track(keys)).unwrap();
    assert_eq!(g.parts(&row), RowParts::at(&row, g.column.w));
    assert!((g.parts(&row).meter.unwrap().right() - (340.0 - edge)).abs() < 0.01);
}

#[test]
fn the_fader_shows_from_64_wide_and_goes_first_when_the_column_narrows() {
    let (s, _) = studio(216.0);
    let keys = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(keys)).unwrap();
    let mut first = None;
    let mut w = row.head_w;
    while w <= 420.0 {
        let p = RowParts::at(&row, w);
        match (p.fader, first) {
            (Some(f), None) => {
                assert!(f.w >= RowParts::FADER_MIN_W, "{w}: {f:?}");
                assert!(f.w < RowParts::FADER_MIN_W + 1.0, "{w}: {f:?}");
                first = Some(w);
            }
            (None, Some(from)) => panic!("gone again at {w}, shown from {from}"),
            _ => {}
        }
        if let Some(f) = p.fader {
            // It sits on line 2, right of the buttons and left of the freeze button or the meter.
            assert!(f.x >= p.keys.unwrap().right());
            assert!(f.right() <= p.freeze.map_or(p.meter.unwrap().x, |z| z.x));
            assert_eq!(f.y, RowParts::LINE_2_Y);
        }
        w += 1.0;
    }
    let from = first.expect("it shows by 420");
    assert!(from > row.head_w, "not at the narrowest");
    // A pixel narrower, it is not there; a track's mute, solo and the rest stay.
    let narrow = RowParts::at(&row, from - 1.0);
    assert!(narrow.fader.is_none());
    assert!(narrow.mute.is_some() && narrow.meter.is_some());
    // A folder has none.
    let folder = s
        .tree()
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Folder(_)))
        .unwrap();
    assert!(RowParts::at(folder, 420.0).fader.is_none());
}

#[test]
fn a_head_wide_enough_has_a_fader_a_head_that_is_not_has_none() {
    let keys = |w| {
        let (s, g) = studio(w);
        let t = track(&s, "Keys");
        s.head_fader_rect(&g, t)
    };
    assert!(keys(420.0).is_some());
    assert!(keys(216.0).is_none());
}

#[test]
fn a_drag_moves_the_track_s_volume_in_one_step_and_shift_moves_it_less() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -12.0);
    let steps = s.song.steps();
    drag(&mut s, &g, t, 30.0, none());
    let coarse = volume(&s, t);
    assert!(coarse > -12.0, "{coarse}");
    // One step of the history for the whole drag.
    assert_eq!(s.song.steps(), steps + 1);
    assert!(s.undo(&g));
    assert_eq!(volume(&s, t), -12.0);
    drag(&mut s, &g, t, 30.0, shift());
    let fine = volume(&s, t);
    assert!(fine > -12.0 && fine < coarse, "{fine} {coarse}");
    // The mixer's fader reads the same value.
    assert_eq!(s.mixer_value(crate::mixer::MixerHit::Fader(t)), Some(fine));
}

#[test]
fn the_value_tag_is_only_there_while_the_fader_moves() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    assert!(s.head_fader.dragging.is_none() && s.mixer.drag.is_none());
    let (x, y) = mid(&s, &g, t);
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 1, none()));
    assert!(s.drag_move_at(x + 10.0, y, false));
    assert_eq!(s.head_fader.dragging, Some(t));
    assert!(s.mixer.drag.is_some());
    assert!(s.drag_end());
    assert!(s.head_fader.dragging.is_none() && s.mixer.drag.is_none());
}

#[test]
fn alt_and_delete_set_zero_db() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -9.0);
    let (x, _) = mid(&s, &g, t);
    let steps = s.song.steps();
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 1, alt()));
    assert_eq!(volume(&s, t), 0.0);
    assert_eq!(s.song.steps(), steps + 1);
    assert!(s.undo(&g));
    assert_eq!(volume(&s, t), -9.0);
    focus(&mut s, &g, t);
    assert!(key(&mut s, "delete", none()));
    assert_eq!(volume(&s, t), 0.0);
}

#[test]
fn the_arrow_keys_move_one_db_shift_a_tenth_and_page_keys_six() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    // Without the keys the fader takes none.
    assert!(!key(&mut s, "right", none()));
    set(&mut s, t, -20.0);
    focus(&mut s, &g, t);
    let steps = s.song.steps();
    assert!(key(&mut s, "right", none()));
    assert!((volume(&s, t) - (-20.0 + STEP_DB)).abs() < 1e-6);
    assert!(key(&mut s, "left", none()));
    assert!(key(&mut s, "left", none()));
    assert!((volume(&s, t) - (-20.0 - STEP_DB)).abs() < 1e-6);
    assert!(key(&mut s, "right", shift()));
    assert!((volume(&s, t) - (-21.0 + FINE_DB)).abs() < 1e-6);
    assert!(key(&mut s, "pagedown", none()));
    assert!((volume(&s, t) - (-20.9 - PAGE_DB)).abs() < 1e-6);
    assert!(key(&mut s, "pageup", none()));
    assert!((volume(&s, t) - -20.9).abs() < 1e-6);
    // Each key is a step of the history.
    assert_eq!(s.song.steps(), steps + 6);
    // It stops at the top.
    set(&mut s, t, MAX_DB - 0.5);
    assert!(key(&mut s, "pageup", none()));
    assert_eq!(volume(&s, t), MAX_DB);
    // Keys with Ctrl or Alt are the app's.
    assert!(!key(&mut s, "right", alt()));
    assert!(!key(&mut s, "x", none()));
    // Esc lets go of the keys.
    assert!(key(&mut s, "escape", none()));
    assert!(!key(&mut s, "right", none()));
}

#[test]
fn a_value_is_typed_in_a_field_enter_puts_it_in_and_escape_keeps_the_old_one() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -6.0);
    // A double click opens the field with the value, which the first letter replaces.
    let (x, _) = mid(&s, &g, t);
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 2, none()));
    let e = s.head_fader.entry.as_ref().unwrap();
    assert_eq!(e.field.text(), "-6.0");
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Head));
    assert!(s.human_interaction_active());
    s.ime_replace(None, "-3", false);
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3");
    let steps = s.song.steps();
    assert!(key(&mut s, "enter", none()));
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), -3.0);
    assert_eq!(s.song.steps(), steps + 1);

    // Enter on the focused fader opens it; Esc leaves the value as it was.
    assert!(key(&mut s, "enter", none()));
    s.ime_replace(None, "-9", false);
    assert!(key(&mut s, "escape", none()));
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), -3.0);
    assert_eq!(s.song.steps(), steps + 1);

    // A unit is read, and so is minus infinity; what is no value changes nothing.
    for (typed, want) in [("-3 dB", -3.0), ("-5.5 dB", -5.5), ("-4", -4.0)] {
        assert!(key(&mut s, "enter", none()));
        s.ime_replace(None, typed, false);
        assert!(key(&mut s, "enter", none()));
        assert_eq!(volume(&s, t), want, "{typed}");
    }
    assert!(key(&mut s, "enter", none()));
    s.ime_replace(None, "oops", false);
    assert!(key(&mut s, "enter", none()));
    assert_eq!(volume(&s, t), -4.0);
    assert!(key(&mut s, "enter", none()));
    s.ime_replace(None, "-inf", false);
    assert!(key(&mut s, "enter", none()));
    assert!(volume(&s, t) <= FADER_FLOOR_DB);
    // Backspace takes the whole old value at once, then a letter at a time.
    set(&mut s, t, -6.0);
    assert!(key(&mut s, "enter", none()));
    assert!(key(&mut s, "backspace", none()));
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "");
    s.ime_replace(None, "-12", false);
    assert!(key(&mut s, "backspace", none()));
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-1");
}

#[test]
fn a_press_elsewhere_keeps_what_is_typed_and_lets_go_of_the_keys() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -6.0);
    focus(&mut s, &g, t);
    assert!(key(&mut s, "enter", none()));
    s.ime_replace(None, "-2", false);
    // A press on the fader itself leaves it be.
    let (x, y) = mid(&s, &g, t);
    s.head_fader_press_off(&g, x, y);
    assert!(s.head_fader.entry.is_some());
    // A press on the lanes puts the value in and gives the keys back.
    s.head_fader_press_off(&g, g.lanes.x + 40.0, g.lanes.y + 10.0);
    assert!(s.head_fader.entry.is_none());
    assert_eq!(s.head_fader.focus, None);
    assert_eq!(volume(&s, t), -2.0);
}

#[test]
fn the_fader_with_a_lane_in_read_does_not_move_as_the_mixer_s_does_not() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    let id = t.to_string();
    let bar = i64::from(s.project().ppq) * 4;
    for at in [0, 4 * bar] {
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": id, "target": "volume", "at": at, "value": -12.0 })
        ));
    }
    let g = {
        s.rebuild(&g);
        s.geometry(1600.0, 1000.0)
    };
    let before = volume(&s, t);
    let steps = s.song.steps();
    let (x, y) = mid(&s, &g, t);
    // The pointer says no, with the reason the mixer's fader gives.
    s.input_map.pointer = (x, y);
    s.hover = s.hit(&g, x, y);
    assert_eq!(
        crate::hover_tip::pointer_tip(&s, &g).map(|(tip, _)| tip),
        Some(crate::auto_lock::REASON)
    );
    assert_eq!(
        crate::pointer::of_studio(&s, &g).style,
        gpui::CursorStyle::OperationNotAllowed
    );
    // A drag, a double click, Alt and the keys move nothing.
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 1, none()));
    assert!(s.mixer.drag.is_none());
    assert!(!s.drag_move_at(x + 40.0, y, false));
    assert!(!s.drag_end());
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 2, none()));
    assert!(s.head_fader.entry.is_none());
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 1, alt()));
    assert!(key(&mut s, "right", none()));
    assert!(key(&mut s, "delete", none()));
    assert!(key(&mut s, "enter", none()));
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), before);
    assert_eq!(s.song.steps(), steps);
    // The menu keeps its rows, greyed out with the reason.
    assert!(s.scene_context(&g, x, y));
    for label in ["0 dB に戻す", "値を入力…"] {
        let row = s
            .context_rows()
            .into_iter()
            .find(|r| r.label == label)
            .unwrap_or_else(|| panic!("no {label}"));
        assert!(!row.enabled, "{label}");
        assert_eq!(row.reason, crate::auto_lock::REASON);
    }
}

#[test]
fn the_right_click_menu_has_the_value_field_and_zero_db() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    let (x, y) = mid(&s, &g, t);
    assert!(s.scene_context(&g, x, y));
    let rows = s.context_rows();
    for label in ["0 dB に戻す", "値を入力…"] {
        let row = rows
            .iter()
            .find(|r| r.label == label)
            .unwrap_or_else(|| panic!("no {label}"));
        assert!(row.enabled, "{label}");
    }
    // The hover tip and the operation name are the mixer fader's own.
    s.popup = None;
    s.hover = Hit::HeadFader(t);
    assert_eq!(
        crate::operations::of_hit(Hit::HeadFader(t)).map(|(_, command)| command),
        crate::operations::of_hit(Hit::Mixer(crate::mixer::MixerHit::Fader(t)))
            .map(|(_, command)| command)
    );
}

#[test]
fn a_tall_row_s_output_field_reaches_the_right_edge_at_any_width() {
    for w in [340.0, 420.0] {
        let (mut s, g) = studio(w);
        let t = track(&s, "Keys");
        assert!(s.press(&g, Hit::Widen(t)));
        let g = s.geometry(1600.0, 1000.0);
        let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
        assert!(row.tall);
        let p = g.parts(&row);
        let out = p.output.expect("the output field");
        assert!(
            (out.right() - (g.column.w - Space::X2.px())).abs() < 0.01,
            "{w}: {out:?}"
        );
        assert_eq!(out.x, p.name_x);
        let at = (
            g.column.x + out.x + out.w - 2.0,
            s.row_y(&g, &row) + out.y + 2.0,
        );
        assert_eq!(s.hit(&g, at.0, at.1), Hit::Output(t), "{w}");
        // The fader stays on line 2 above it.
        let f = p.fader.expect("the fader");
        assert!(f.bottom() <= out.y);
    }
}

#[test]
fn the_folder_the_bus_the_lanes_and_the_record_area_follow_the_same_width() {
    let (s, _) = studio(216.0);
    let edge = Space::X2.px();
    let folder = *s
        .tree()
        .rows
        .iter()
        .find(|r| matches!(r.kind, RowKind::Folder(_)))
        .unwrap();
    let keys = *s.tree().row_of(RowKind::Track(track(&s, "Keys"))).unwrap();
    for w in [folder.head_w, 340.0, 420.0] {
        // A folder's bus button sits on the right edge, named or not.
        for name in [None, Some("Drum bus")] {
            let r = crate::folder_bus::rect(&folder, w, name);
            assert!((r.right() - (w - edge)).abs() < 0.01, "{w}: {r:?}");
        }
        // A folder's name stops where the width says.
        let f = RowParts::at(&folder, w);
        assert!((f.name_right - (w - edge)).abs() < 0.01, "{w}");
        // A track's lane buttons follow it too.
        let lanes = crate::arrange::LaneParts::at(&keys, w, true);
        assert!((lanes.remove.right() - (w - edge)).abs() < 0.01, "{w}");
    }
}

#[test]
fn a_screen_reader_finds_the_fader_as_a_slider_and_a_press_opens_the_field() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -6.0);
    let marks = a11y::marks(&s, &g);
    let m = marks
        .iter()
        .find(|m| m.key == format!("head-fader-{t}"))
        .expect("a mark for the fader");
    assert_eq!(m.node.role(), gpui::accesskit::Role::Slider);
    assert_eq!(m.node.label(), Some("Keys の音量"));
    assert_eq!(m.node.numeric_value(), Some(-6.0));
    assert_eq!(m.press, Some(Hit::HeadFader(t)));
    assert!(a11y::press(&mut s, &g, Hit::HeadFader(t)));
    assert!(s.head_fader.entry.is_some());
    // A column too narrow for the fader has no mark.
    let (s, g) = studio(216.0);
    assert!(
        !a11y::marks(&s, &g)
            .iter()
            .any(|m| m.key.starts_with("head-fader-"))
    );
}

/// A key as the window sends it: a field takes it first, then the menu's box, else the screen's
/// commands.
fn press_key(s: &mut Studio, g: &Geometry, k: &str) {
    if !s.field_keys(g, k, None, none()) && !s.typed(g, k, None) {
        s.key(g, k, false, false);
    }
}

#[test]
fn a_song_opened_over_keeps_nothing_of_the_old_faders_keys() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    let before = s.song.steps();
    // The keys were on the fader, then another song with the same track ids replaces it.
    focus(&mut s, &g, t);
    s.song_replaced(Ok(None));
    assert_eq!(s.head_fader, crate::head_fader::HeadFader::default());
    for k in ["right", "left", "pageup", "delete", "enter"] {
        assert!(!key(&mut s, k, none()), "{k}");
    }
    assert_eq!((volume(&s, t), s.song.steps()), (-7.0, before));
    // A drag still going is let go, with its value not written.
    let (x, y) = mid(&s, &g, t);
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 1, none()));
    assert!(s.drag_move_at(x + 40.0, y, false));
    s.song_replaced(Ok(None));
    assert!(s.mixer.drag.is_none() && s.head_fader.dragging.is_none());
    assert!(!key(&mut s, "right", none()));
    assert_eq!((volume(&s, t), s.song.steps()), (-7.0, before));
    // What was typed in the field does not go into the new song.
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 2, none()));
    s.head_fader
        .entry
        .as_mut()
        .unwrap()
        .type_text(None, "-3", false);
    s.song_replaced(Ok(None));
    assert!(s.head_fader.entry.is_none());
    assert!(!key(&mut s, "enter", none()));
    assert_eq!((volume(&s, t), s.song.steps()), (-7.0, before));
}

#[test]
fn the_menu_has_the_keys_and_its_value_field_takes_the_number() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    // The fader has the keys, then the right-click opens the menu: the fader lets them go.
    focus(&mut s, &g, t);
    let (x, y) = mid(&s, &g, t);
    assert!(s.scene_context(&g, x, y));
    assert_eq!(s.head_fader.focus, None);
    let before = s.song.steps();
    // Arrows and Delete go to the menu, not to the volume behind it.
    for k in ["right", "left", "pageup", "delete"] {
        press_key(&mut s, &g, k);
    }
    assert_eq!((volume(&s, t), s.song.steps()), (-7.0, before));
    // 「値を入力…」, the number, Enter: the volume is the typed one, in one step, and the head's
    // own field did not open on the way.
    let row = s
        .context_rows()
        .iter()
        .position(|r| r.label == "値を入力…")
        .expect("the value field");
    assert!(s.context_pick(&g, row));
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Ask));
    // The box opens with the value it has; it is cleared, as a person would, before the number.
    for _ in 0..8 {
        press_key(&mut s, &g, "backspace");
    }
    s.ask_text(None, "-3", false);
    press_key(&mut s, &g, "enter");
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), -3.0);
    assert_eq!(s.song.steps(), before + 1);
    // Esc in the menu's field leaves the value.
    assert!(s.scene_context(&g, x, y));
    let row = s
        .context_rows()
        .iter()
        .position(|r| r.label == "値を入力…")
        .unwrap();
    assert!(s.context_pick(&g, row));
    for _ in 0..8 {
        press_key(&mut s, &g, "backspace");
    }
    s.ask_text(None, "-20", false);
    press_key(&mut s, &g, "escape");
    assert_eq!((volume(&s, t), s.song.steps()), (-3.0, before + 1));
    assert!(s.head_fader.entry.is_none());
}

#[test]
fn a_field_open_in_the_head_goes_with_the_menu_that_opens_over_it() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    let (x, y) = mid(&s, &g, t);
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 2, none()));
    s.head_fader
        .entry
        .as_mut()
        .unwrap()
        .type_text(None, "-5", false);
    // The menu keeps what was typed (as a press elsewhere does), and the field is not left open
    // behind it to take Enter, Esc and Backspace.
    assert!(s.scene_context(&g, x, y));
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), -5.0);
    // The menu's value field opens with that -5, not the -7 the song had when the press came;
    // Enter without a change puts back what it has, adding no step of history.
    let steps = s.song.steps();
    let row = s
        .context_rows()
        .iter()
        .position(|r| r.label == "値を入力…")
        .expect("the value field");
    assert!(s.context_pick(&g, row));
    let opened = s.ask_field().map(|f| f.value.clone()).unwrap_or_default();
    assert_eq!(opened.parse::<f64>().ok(), Some(-5.0), "{opened}");
    press_key(&mut s, &g, "enter");
    assert_eq!((volume(&s, t), s.song.steps()), (-5.0, steps));
}

#[test]
fn the_menus_value_field_opens_with_the_value_the_fader_shows_over_a_lane() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    let bar = i64::from(s.project().ppq) * 4;
    for (at, v) in [(4 * bar, -24.0), (6 * bar, 0.0)] {
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": t.to_string(), "target": "volume", "at": at, "value": v })
        ));
    }
    // Touch keeps the field usable while the lane plays: the fader shows the lane's value.
    assert!(s.run_command(
        "auto.mode",
        json!({ "ids": [t.to_string()], "mode": "touch" })
    ));
    s.seek(4 * bar);
    assert_eq!(s.head_fader_value(t), Some(-24.0));
    let (x, y) = mid(&s, &g, t);
    assert!(s.scene_context(&g, x, y));
    let row = s
        .context_rows()
        .iter()
        .position(|r| r.label == "値を入力…")
        .expect("the value field");
    assert!(s.context_rows()[row].enabled);
    assert!(s.context_pick(&g, row));
    let opened = s.ask_field().map(|f| f.value.clone()).unwrap_or_default();
    assert_eq!(opened.parse::<f64>().ok(), Some(-24.0), "{opened}");
}

#[test]
fn the_menu_key_too_puts_in_what_the_head_field_has_before_it_builds_the_menu() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    set(&mut s, t, -7.0);
    focus(&mut s, &g, t);
    s.select_row(Some(RowKind::Track(t)));
    let (x, _) = mid(&s, &g, t);
    assert!(s.head_fader_down(&g, Hit::HeadFader(t), x, 2, none()));
    s.head_fader
        .entry
        .as_mut()
        .unwrap()
        .type_text(None, "-4", false);
    s.key_context(&g);
    assert!(s.head_fader.entry.is_none());
    assert_eq!(volume(&s, t), -4.0);
}

#[test]
fn another_fader_takes_the_press_and_the_field_moves_to_it() {
    let (mut s, g) = studio(420.0);
    let (a, b) = (track(&s, "Keys"), track(&s, "Bass"));
    set(&mut s, a, -7.0);
    set(&mut s, b, -9.0);
    let before = s.song.steps();
    // A field typed in on A, then a drag on B: A keeps what was typed, B is dragged.
    let (ax, ay) = mid(&s, &g, a);
    assert!(s.head_fader_down(&g, Hit::HeadFader(a), ax, 2, none()));
    s.head_fader
        .entry
        .as_mut()
        .unwrap()
        .type_text(None, "-4", false);
    let (bx, by) = mid(&s, &g, b);
    s.press_keys(&g, bx, by);
    assert!(s.head_fader_down(&g, Hit::HeadFader(b), bx, 1, none()));
    assert_eq!(volume(&s, a), -4.0);
    assert!(s.head_fader.entry.is_none());
    assert_eq!(s.head_fader.dragging, Some(b));
    assert!(s.drag_move_at(bx + 30.0, by, false));
    assert!(s.drag_end());
    assert!(volume(&s, b) > -9.0);
    assert_eq!(s.head_fader.focus, Some(b));
    assert_eq!(s.song.steps(), before + 2);
    // Straight from B's focus to A's number field: a double click opens it on A.
    s.press_keys(&g, ax, ay);
    assert_eq!(s.head_fader.focus, None);
    assert!(s.head_fader_down(&g, Hit::HeadFader(a), ax, 2, none()));
    assert_eq!(s.head_fader.entry.as_ref().map(|e| e.track), Some(a));
    // A press on the field's own fader leaves it open; one on B commits it and moves on.
    s.press_keys(&g, ax, ay);
    assert!(s.head_fader.entry.is_some());
    s.head_fader
        .entry
        .as_mut()
        .unwrap()
        .type_text(None, "-2", false);
    s.press_keys(&g, bx, by);
    assert!(s.head_fader_down(&g, Hit::HeadFader(b), bx, 2, none()));
    assert_eq!(volume(&s, a), -2.0);
    assert_eq!(s.head_fader.entry.as_ref().map(|e| e.track), Some(b));
}

#[test]
fn a_track_that_goes_takes_no_key_of_the_fader() {
    let (mut s, g) = studio(420.0);
    let t = track(&s, "Keys");
    focus(&mut s, &g, t);
    s.head_fader.entry = Some(crate::head_fader::Entry {
        track: TrackId::MASTER,
        field: crate::settings::field::Field::new(crate::settings::FieldId::Value, "1".into()),
        fresh: true,
    });
    // The field of a track that is not in the tree is dropped, the focus stays.
    assert!(key(&mut s, "left", none()));
    assert!(s.head_fader.entry.is_none());
    assert_eq!(s.head_fader.focus, Some(t));
}

/// A song of `n` tracks that each play a volume lane, and the studio on it.
fn big_song(n: usize, width: f32) -> (Studio, Geometry) {
    let (mut s, g) = studio(width);
    let keys = track(&s, "Keys");
    let bar = i64::from(s.project().ppq) * 4;
    for i in 0..16 {
        assert!(s.run_command(
            "auto.point.add",
            json!({ "track": keys.to_string(), "target": "volume", "at": i * bar, "value": -12.0 + i as f64 })
        ));
    }
    while s.project().tracks.len() < n {
        assert!(s.run_command("track.duplicate", json!({ "ids": [keys.to_string()] })));
    }
    s.rebuild(&g);
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

/// What a still screen asks of the head faders each frame: the screen reader's tree and rows.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_the_still_screen_with_many_tracks() {
    use std::time::Instant;
    for (n, width) in [(24, 420.0), (96, 420.0), (96, 232.0), (192, 420.0)] {
        let (s, g) = big_song(n, width);
        let rows = s.tree().rows.len();
        let (mut best, mut marks_best) = (f64::MAX, f64::MAX);
        let mut total = 0.0;
        let frames = 20;
        for _ in 0..frames {
            let t = Instant::now();
            let marks = a11y::marks(&s, &g);
            let mid = t.elapsed().as_secs_f64() * 1000.0;
            let nodes = a11y::track_rows(&s, &g);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box((marks.len(), nodes.len()));
            best = best.min(ms);
            total += ms;
            marks_best = marks_best.min(mid);
        }
        println!(
            "MEASURE tracks={} rows={rows} width={width} best={best:.2}ms (marks {marks_best:.2}ms) mean={:.2}ms",
            s.project().tracks.len(),
            total / frames as f64
        );
    }
}
