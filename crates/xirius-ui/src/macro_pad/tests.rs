use serde_json::json;
use xirius_core::controls::ControlId;
use xirius_core::id::TrackId;

use super::*;
use crate::channel::{self, POPOUT_LABEL};
use crate::popup::{Action, PopupKind};
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.instruments.open.clear();
    let g = s.geometry(1600.0, 1000.0);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .unwrap()
        .id
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

fn knob(s: &Studio, t: TrackId, k: u8) -> f64 {
    s.project().tracks[&t]
        .controls
        .get(ControlId::Knob(k))
        .value
}

fn press(s: &mut Studio, (x, y): (f32, f32)) {
    assert!(s.macro_press(x, y));
    s.macro_up();
}

#[test]
fn the_channel_editor_shows_the_row_and_its_button_opens_the_window() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.open_channel(t);
    let v = s.channel_view(&g).unwrap();
    assert!(v.macros.is_none(), "off until Macro Controls is pressed");
    let tool = v
        .tools
        .iter()
        .find(|(x, _)| *x == channel::Tool::Macros)
        .unwrap()
        .1;
    let (x, y) = mid(tool);
    assert!(s.channel_press(&g, x, y, 1));
    s.channel_up(&g);
    let v = s.channel_view(&g).unwrap();
    let row = v.macros.clone().expect("the row");
    assert_eq!(row.band.y, v.toolbar.bottom());
    assert_eq!(v.body.y, row.band.bottom());
    assert!(row.cells.iter().all(|c| c.right() <= row.popout.x));
    // the row's knob turns its macro, one step on release
    let (x, y) = mid(channel::macro_row(row.band).cells[2]);
    assert_eq!(channel::part_at(&v, x, y), Some(channel::Part::Macro(2)));
    let undo = s.song().steps();
    let was = knob(&s, t, 2);
    assert!(s.channel_press(&g, x, y, 1));
    assert!(s.mixer.drag.is_some());
    s.drag_move(y - 60.0, false);
    assert!(s.drag_end());
    let want = (was + 0.4).min(1.0);
    assert!((knob(&s, t, 2) - want).abs() < 1e-6, "{}", knob(&s, t, 2));
    assert_eq!(s.song().steps(), undo + 1);
    // the row's button opens the window on the track
    let (x, y) = mid(row.popout);
    assert_eq!(channel::part_at(&v, x, y), Some(channel::Part::MacroOut));
    assert!(s.channel_press(&g, x, y, 1));
    assert_eq!(s.macro_pad.as_ref().unwrap().track, t);
    // and so does the toolbar's
    s.macro_pad = None;
    let out = v
        .tools
        .iter()
        .find(|(x, _)| *x == channel::Tool::Popout)
        .unwrap()
        .1;
    let (x, y) = mid(out);
    assert!(s.channel_press(&g, x, y, 1));
    assert!(s.macro_pad.is_some());
}

#[test]
fn the_window_lays_out_four_by_two_and_the_pad() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let v = s.macro_view().unwrap();
    // the whole of the window's own surface, from its corner
    assert_eq!((v.r.x, v.r.y, v.r.w, v.r.h), (0.0, 0.0, W, H));
    assert_eq!((v.pad.w, v.pad.h), (SIDE, SIDE));
    assert_eq!(v.knobs[1].x - v.knobs[0].x, CELL);
    assert_eq!(v.knobs[4].y - v.knobs[0].y, CELL);
    assert_eq!(v.knobs[4].x, v.knobs[0].x);
    assert!(v.knobs[3].right() <= v.pad.x);
    assert!(v.picks[0].right() < v.picks[1].x);
    assert_eq!(v.picks[1].right(), v.pad.right());
    assert!(
        v.bar.channel.is_none(),
        "the bar is the shared one: a title and the close"
    );
    assert!(v.r.bottom() >= v.picks[0].bottom());
}

#[test]
fn unpinned_it_follows_the_chosen_track_and_pinned_it_stays() {
    let (mut s, _) = studio();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    // opened on Vocal while Keys is chosen, it shows Vocal until another track is chosen
    s.selected_row = Some(RowKind::Track(keys));
    s.open_macros(vocal);
    assert_eq!(s.macro_track(), Some(vocal));
    s.selected_row = Some(RowKind::Track(vocal));
    s.macro_follow();
    assert_eq!(s.macro_track(), Some(vocal));
    s.selected_row = Some(RowKind::Track(keys));
    s.macro_follow();
    assert_eq!(s.macro_track(), Some(keys));
    // the pin keeps what it shows now
    let pin = mid(s.macro_view().unwrap().bar.pin);
    press(&mut s, pin);
    assert!(s.macro_pad.as_ref().unwrap().pinned);
    s.selected_row = Some(RowKind::Track(vocal));
    s.macro_follow();
    assert_eq!(s.macro_track(), Some(keys));
    // a pinned window is not taken by another track's button
    s.open_macros(vocal);
    assert_eq!(s.macro_track(), Some(keys));
    // the head moves the window (the system does, not the surface): a press on it does nothing
    let head = drag_rects(&s.macro_view().unwrap())[0];
    let (hx, hy) = mid(head);
    assert_eq!(part_at(&s.macro_view().unwrap(), hx, hy), Some(Part::Head));
    press(&mut s, (hx, hy));
    assert!(s.channel.is_none() && s.macro_pad.is_some());
    let close = mid(s.macro_view().unwrap().bar.close);
    press(&mut s, close);
    assert!(s.macro_pad.is_none());
}

#[test]
fn the_pad_dragged_sets_both_knobs_in_one_step() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let v = s.macro_view().unwrap();
    let undo = s.song().steps();
    let (x, y) = pad_point(v.pad, (0.2, 0.2));
    assert!(s.macro_press(x, y));
    assert!(s.macro_pad.as_ref().unwrap().focused);
    let (x, y) = pad_point(v.pad, (0.4, 0.7));
    assert!(s.macro_moved(x, y));
    assert_eq!(s.song().steps(), undo, "nothing is written while held");
    let track = s.project().tracks[&t].clone();
    assert_eq!(s.pad_now(&track), (0.4, 0.7));
    assert!(s.macro_up());
    assert_eq!(s.song().steps(), undo + 1);
    assert!((knob(&s, t, 0) - 0.4).abs() < 1e-9);
    assert!((knob(&s, t, 1) - 0.7).abs() < 1e-9);
    // off the pad, it holds to the edge
    let (x, y) = pad_point(v.pad, (0.5, 0.5));
    assert!(s.macro_press(x, y));
    assert!(s.macro_moved(v.pad.right() + 50.0, v.pad.y - 50.0));
    s.macro_up();
    assert_eq!((knob(&s, t, 0), knob(&s, t, 1)), (1.0, 1.0));
    // one undo takes back both
    assert!(s.undo(&g));
    assert_eq!((knob(&s, t, 0), knob(&s, t, 1)), (0.4, 0.7));
}

#[test]
fn a_release_a_dialog_takes_still_lets_the_pad_and_the_scroll_bar_go() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let v = s.macro_view().unwrap();
    let undo = s.song().steps();
    let (x, y) = pad_point(v.pad, (0.2, 0.2));
    assert!(s.macro_press(x, y));
    let (x, y) = pad_point(v.pad, (0.6, 0.3));
    assert!(s.macro_moved(x, y));
    s.mixer.bar_grab = Some(0.0);
    s.let_go_drags();
    assert!(s.macro_pad.as_ref().unwrap().grab.is_none());
    assert!(s.mixer.bar_grab.is_none());
    // the pad ends where it was, for the track it was pressed for
    assert_eq!(s.song().steps(), undo + 1);
    assert!((knob(&s, t, 0) - 0.6).abs() < 1e-9);
    assert!(!s.macro_moved(x + 20.0, y));
}

#[test]
fn the_arrow_keys_move_the_pad_while_it_has_them() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    assert!(s.run_command("control.xy", json!({"track":t,"x":0.4,"y":0.7})));
    let none = crate::tools::Mods::default();
    assert!(
        !s.macro_key("right", none),
        "not before the pad was pressed"
    );
    let v = s.macro_view().unwrap();
    let (x, y) = pad_point(v.pad, (0.4, 0.7));
    press(&mut s, (x, y));
    let undo = s.song().steps();
    assert!(s.macro_key("right", none));
    assert!(s.macro_key("up", none));
    assert!((knob(&s, t, 0) - 0.41).abs() < 1e-9);
    assert!((knob(&s, t, 1) - 0.71).abs() < 1e-9);
    assert_eq!(s.song().steps(), undo + 2, "each press its own step");
    let shift = crate::tools::Mods {
        shift: true,
        ..none
    };
    assert!(s.macro_key("left", shift));
    assert!(s.macro_key("down", shift));
    assert!((knob(&s, t, 0) - 0.31).abs() < 1e-9);
    assert!((knob(&s, t, 1) - 0.61).abs() < 1e-9);
    assert!(s.macro_key("escape", none));
    assert!(!s.macro_key("right", none));
}

#[test]
fn a_pick_lists_the_eight_and_chooses_the_axis_knob() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let pick = mid(s.macro_view().unwrap().picks[1]);
    press(&mut s, pick);
    let popup = s.popup.as_ref().expect("the pick's list");
    assert_eq!(popup.kind, PopupKind::MacroAxis(t, 1));
    assert!(crate::pick_list::is(popup.kind), "a PickList (UI-320)");
    let rows: Vec<_> = crate::pick_list::found(popup).cloned().collect();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[0].label, "Macro 1");
    assert!(rows[1].on, "Y has Macro 2");
    let act = rows[0].act.clone().unwrap();
    assert_eq!(act, Action::MacroAxis(t, 1, 0));
    assert_eq!(crate::operations::of_action(&act), "control.xy.assign");
    // choosing X's knob for Y swaps them
    assert!(s.popup_action(&g, act));
    assert!(s.popup.is_none());
    assert_eq!(s.project().tracks[&t].controls.xy, [1, 0]);
    let v = s.macro_view().unwrap();
    let (x, y) = pad_point(v.pad, (0.25, 0.75));
    press(&mut s, (x, y));
    assert!((knob(&s, t, 1) - 0.25).abs() < 1e-9, "X now moves Macro 2");
    assert!((knob(&s, t, 0) - 0.75).abs() < 1e-9);
}

#[test]
fn a_reader_hears_the_knobs_the_pad_and_the_picks() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    assert!(s.run_command("control.xy", json!({"track":t,"x":0.4,"y":0.7})));
    s.open_macros(t);
    let marks = marks(&s);
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].node.label(), Some("Vocal › Macro Controls"));
    let find = |label: &str| {
        marks[0]
            .children
            .iter()
            .find(|m| m.node.label() == Some(label))
            .unwrap_or_else(|| panic!("{label}"))
            .node
            .clone()
    };
    assert_eq!(find("Macro 1").value(), Some("40 %"));
    assert_eq!(
        find("X/Y パッド").value(),
        Some("X Macro 1 40 %, Y Macro 2 70 %")
    );
    assert_eq!(find("Y").value(), Some("Macro 2"));
    assert!(find("ピン留め").toggled().is_some());
    find("閉じる");
    // the channel editor's row too
    s.open_channel(t);
    s.channel.as_mut().unwrap().macros = true;
    let marks = channel::marks(&s, &g);
    let labels: Vec<String> = marks[0]
        .children
        .iter()
        .map(|m| m.node.label().unwrap_or_default().to_string())
        .collect();
    for want in ["Macro 1", "Macro 8", POPOUT_LABEL] {
        assert!(labels.iter().any(|l| l == want), "{want} in {labels:?}");
    }
}

#[test]
fn the_window_is_laid_out_on_its_own_surface_whatever_the_channel_editor_is() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    s.open_channel(t);
    s.open_macros(t);
    let v = s.macro_view().unwrap();
    let (x, y) = mid(v.pad);
    assert!(s.macro_press(x, y));
    s.macro_up();
    // a press off the window takes the keys from the pad
    assert!(!s.macro_press(2.0, H + 20.0));
    assert!(!s.macro_pad.as_ref().unwrap().focused);
}

#[test]
fn a_pinned_pad_whose_track_goes_closes_and_others_open_again() {
    let (mut s, _) = studio();
    let vocal = track(&s, "Vocal");
    let keys = track(&s, "Keys");
    s.open_macros(vocal);
    s.macro_pad.as_mut().unwrap().pinned = true;
    assert!(s.run_command("track.delete", json!({"ids":[vocal]})));
    assert!(s.macro_pad.is_none(), "its track went");
    s.open_macros(keys);
    assert_eq!(s.macro_pad.as_ref().map(|w| w.track), Some(keys));
    s.new_song_sync();
    assert!(s.macro_pad.is_none(), "another song closes it");
}

#[test]
fn a_press_on_the_window_stays_in_it_and_one_elsewhere_takes_the_keys() {
    let (mut s, _) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let v = s.macro_view().unwrap();
    // its ground, between the parts, is the window's own
    let (x, y) = (v.r.x + 3.0, v.r.bottom() - 3.0);
    assert_eq!(part_at(&v, x, y), Some(Part::Body));
    assert!(s.macro_press(x, y));
    press(&mut s, mid(v.pad));
    assert!(s.macro_pad.as_ref().unwrap().focused);
    // a press off the window (the main window's is not this surface's) lets the keys go
    assert!(!s.macro_press(-4.0, -4.0));
    assert!(!s.macro_pad.as_ref().unwrap().focused);
    assert!(!s.macro_key("right", crate::tools::Mods::default()));
}

#[test]
fn the_main_windows_tree_has_no_macro_marks_and_the_macro_windows_has_them_locally() {
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    s.open_macros(t);
    let main = crate::a11y::marks(&s, &g);
    fn keys(m: &crate::a11y::Mark, out: &mut Vec<String>) {
        out.push(m.key.clone());
        for c in &m.children {
            keys(c, out);
        }
    }
    let mut seen = Vec::new();
    for m in &main {
        keys(m, &mut seen);
    }
    assert!(
        seen.iter().all(|k| !k.starts_with("macros")),
        "the main window's tree holds no Macro Controls mark: {seen:?}"
    );
    let own = marks(&s);
    assert_eq!(own.len(), 1);
    let root = own[0].node.bounds().unwrap();
    assert_eq!(
        (root.x0, root.y0),
        (0.0, 0.0),
        "the window's own coordinates"
    );
    assert_eq!(
        (root.x1 as f32, root.y1 as f32),
        (W, H),
        "the root is the whole window"
    );
    for c in &own[0].children {
        let b = c.node.bounds().unwrap();
        assert!(
            b.x0 >= 0.0 && b.y0 >= 0.0 && b.x1 <= f64::from(W) && b.y1 <= f64::from(H),
            "{} lies inside the window's own surface: {b:?}",
            c.key
        );
    }
    let pad = own[0]
        .children
        .iter()
        .find(|c| c.key == "macros-pad")
        .unwrap();
    let v = s.macro_view().unwrap();
    let b = pad.node.bounds().unwrap();
    assert_eq!((b.x0 as f32, b.y0 as f32), (v.pad.x, v.pad.y));
}
