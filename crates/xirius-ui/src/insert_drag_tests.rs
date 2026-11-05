//! The insert heading dragged and copied (MIX-045), pressed as the window presses it.

use serde_json::json;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::TrackId;

use super::*;
use crate::notice::Press;
use crate::studio::DrawerTab;
use crate::tools::Mods;

const CTRL: Mods = Mods {
    ctrl: true,
    ..Mods::NONE
};

fn mix() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(1600.0, 1000.0);
    s.resized(&g);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    crate::arrange::probe::track(s, name)
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

/// The middle of a strip's insert heading (left of its ＋).
fn heading(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let r = s
        .mixer_rect(g, MixerHit::Heading(t, Area::Insert))
        .expect("the heading is on screen");
    (r.x + 12.0, r.y + r.h / 2.0)
}

/// The middle of a strip's insert area.
fn area(s: &Studio, g: &Geometry, t: TrackId) -> (f32, f32) {
    let r = s
        .mixer_strips(g)
        .into_iter()
        .find(|x| x.track == t)
        .expect("a strip")
        .inserts;
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The left press on a heading, as the window does it.
fn press_heading(s: &mut Studio, g: &Geometry, t: TrackId) {
    let (x, y) = heading(s, g, t);
    s.insert_drag_down(g, x, y);
    let hit = s.mixer_hit(g, x, y).expect("the heading is in a strip");
    assert_eq!(hit, MixerHit::Heading(t, Area::Insert));
    s.mixer_press(g, hit);
}

fn drag(s: &mut Studio, g: &Geometry, from: TrackId, (x, y): (f32, f32), shift: bool) -> bool {
    press_heading(s, g, from);
    assert!(s.insert_drag_move(x, y, shift), "the drag has begun");
    s.insert_drag_up(g, x, y, shift)
}

/// Vocal: Compressor, EQ. Guitar: Delay.
fn song() -> (Studio, Geometry, TrackId, TrackId) {
    let (mut s, g) = mix();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    add_effect(&mut s, vocal, "Compressor");
    add_effect(&mut s, vocal, "EQ");
    add_effect(&mut s, guitar, "Delay");
    (s, g, vocal, guitar)
}

#[test]
fn a_heading_dropped_on_another_strip_replaces_its_inserts_in_one_step_with_a_toast() {
    let (mut s, g, vocal, guitar) = song();
    let before = s.song.steps();
    let at = area(&s, &g, guitar);
    assert!(drag(&mut s, &g, vocal, at, false));
    assert_eq!(inserts(&s, guitar), ["Compressor", "EQ"]);
    assert_eq!(inserts(&s, vocal), ["Compressor", "EQ"]);
    assert_eq!(s.song.steps(), before + 1);
    assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
    let button = s.notice.as_ref().unwrap().button.clone().unwrap();
    assert_eq!(button.label, "元に戻す");
    assert!(matches!(button.press, Press::Undo));
    assert!(s.mixer.insert_drag.is_none());
    // Ctrl+Z takes it back, and the toast with it.
    assert!(s.undo(&g));
    assert_eq!(inserts(&s, guitar), ["Delay"]);
    assert!(s.notice.is_none());
}

#[test]
fn with_shift_the_inserts_go_after_the_ones_it_has() {
    let (mut s, g, vocal, guitar) = song();
    let at = area(&s, &g, guitar);
    press_heading(&mut s, &g, vocal);
    assert!(s.insert_drag_move(at.0, at.1, true));
    assert!(s.mixer.insert_drag.unwrap().append);
    assert!(s.insert_drag_up(&g, at.0, at.1, true));
    assert_eq!(inserts(&s, guitar), ["Delay", "Compressor", "EQ"]);
    assert_eq!(s.notice_text(), Some("インサートを後ろに貼り付けました"));
}

#[test]
fn the_source_and_a_frozen_track_take_nothing_and_the_pointer_says_no() {
    let (mut s, g, vocal, guitar) = song();
    s.song
        .run(
            "track.freeze.apply",
            json!({ "tracks": [{ "id": guitar.to_string(), "file": "frozen.wav", "start": 0 }] }),
        )
        .unwrap();
    let before = s.song.steps();
    for to in [vocal, guitar] {
        let at = area(&s, &g, to);
        press_heading(&mut s, &g, vocal);
        assert!(s.insert_drag_move(at.0, at.1, false));
        assert_eq!(
            s.insert_drop(&g, vocal, at.0, at.1),
            Some(Drop {
                to,
                strip: true,
                takes: false
            })
        );
        assert!(s.insert_drag_refused(&g));
        assert!(s.insert_drag_up(&g, at.0, at.1, false));
        assert!(s.notice.is_none());
    }
    assert_eq!(s.song.steps(), before);
    assert_eq!(inserts(&s, guitar), ["Delay"]);
}

#[test]
fn a_new_drag_and_a_drop_nowhere_take_the_last_toast_away() {
    let (mut s, g, vocal, guitar) = song();
    let at = area(&s, &g, guitar);
    assert!(drag(&mut s, &g, vocal, at, false));
    assert!(s.notice.is_some());
    press_heading(&mut s, &g, vocal);
    let (x, y) = heading(&s, &g, vocal);
    assert!(s.insert_drag_move(x + 40.0, y - 300.0, false));
    assert!(s.notice.is_none(), "a new drag clears the toast");
    assert!(s.insert_drag_up(&g, x + 40.0, y - 300.0, false));
    assert!(drag(&mut s, &g, vocal, at, false));
    assert!(s.notice.is_some());
    // off every strip and every track
    press_heading(&mut s, &g, vocal);
    assert!(s.insert_drag_move(x + 40.0, y - 300.0, false));
    assert!(s.insert_drag_up(&g, x + 40.0, y - 300.0, false));
    assert!(s.notice.is_none());
}

#[test]
fn a_drag_leaves_what_another_action_said() {
    let (mut s, g, vocal, guitar) = song();
    s.tell_with("インサートを開きました".into(), "元に戻す", Press::Undo);
    press_heading(&mut s, &g, vocal);
    let (x, y) = heading(&s, &g, vocal);
    assert!(s.insert_drag_move(x + 40.0, y - 300.0, false));
    assert!(s.insert_drag_up(&g, x + 40.0, y - 300.0, false));
    assert_eq!(s.notice_text(), Some("インサートを開きました"));
    // nor does an undo take it, though it begins alike
    assert!(s.undo(&g));
    assert_eq!(inserts(&s, guitar), Vec::<String>::new());
    assert_eq!(s.notice_text(), Some("インサートを開きました"));
}

#[test]
fn a_press_without_a_move_is_not_a_drag() {
    let (mut s, g, vocal, guitar) = song();
    let before = s.song.steps();
    let (x, y) = heading(&s, &g, vocal);
    press_heading(&mut s, &g, vocal);
    assert!(!s.insert_drag_move(x + 2.0, y + 2.0, false));
    assert!(!s.insert_drag_up(&g, x + 2.0, y + 2.0, false));
    assert_eq!(s.song.steps(), before);
    assert_eq!(inserts(&s, guitar), ["Delay"]);
}

#[test]
fn a_track_head_in_the_arrangement_takes_the_inserts_too() {
    let (mut s, g, vocal, guitar) = song();
    let row = *s
        .tree()
        .row_of(RowKind::Track(guitar))
        .expect("Guitar's row");
    // the row at the top of the column, above the drawer
    s.scroll.1 = row.y;
    let y = s.row_y(&g, &row) + row.h / 2.0;
    let at = (g.column.x + g.column.w / 2.0, y);
    assert!(g.column.contains(at.0, at.1), "the row is on screen");
    assert_eq!(
        s.insert_drop(&g, vocal, at.0, at.1),
        Some(Drop {
            to: guitar,
            strip: false,
            takes: true
        })
    );
    assert!(drag(&mut s, &g, vocal, at, true));
    assert_eq!(inserts(&s, guitar), ["Delay", "Compressor", "EQ"]);
}

#[test]
fn the_master_takes_the_inserts() {
    let (mut s, g, vocal, _) = song();
    let at = area(&s, &g, TrackId::MASTER);
    assert!(drag(&mut s, &g, vocal, at, false));
    assert_eq!(inserts(&s, TrackId::MASTER), ["Compressor", "EQ"]);
}

#[test]
fn ctrl_c_and_ctrl_v_on_a_heading_copy_and_paste_the_inserts() {
    let (mut s, g, vocal, guitar) = song();
    press_heading(&mut s, &g, vocal);
    assert_eq!(s.mixer.heading, Some(vocal));
    assert!(s.mapped_key(&g, "c", None, CTRL));
    assert!(matches!(
        s.song.session().state.clipboard,
        Some(Clipboard::Inserts(ref c)) if c.source == vocal
    ));
    press_heading(&mut s, &g, guitar);
    let before = s.song.steps();
    assert!(s.mapped_key(&g, "v", None, CTRL));
    assert_eq!(inserts(&s, guitar), ["Compressor", "EQ"]);
    assert_eq!(s.song.steps(), before + 1);
    assert_eq!(s.notice_text(), Some("インサートを貼り付けました"));
}

#[test]
fn the_name_drag_stays_a_settings_paste() {
    let (mut s, g, vocal, guitar) = song();
    // the press on the name begins no insert drag
    let r = s
        .mixer_strips(&g)
        .into_iter()
        .find(|x| x.track == vocal)
        .unwrap()
        .name;
    s.insert_drag_down(&g, r.x + r.w / 2.0, r.y + r.h / 2.0);
    assert!(s.mixer.insert_drag.is_none());
    // and the heading's press begins no name drag
    press_heading(&mut s, &g, vocal);
    assert!(s.mixer.name_drag.is_none());
    assert!(s.mixer.insert_drag.is_some());
    let _ = guitar;
}

#[test]
fn a_pen_or_a_finger_moves_further_before_it_drags() {
    let (mut s, g, vocal, guitar) = song();
    let (x, y) = heading(&s, &g, vocal);
    // a mouse: 4 px
    press_heading(&mut s, &g, vocal);
    assert!(s.insert_drag_move(x + 4.0, y, false));
    s.mixer.insert_drag = None;
    // a pen or a finger trembles as it lifts: 10 px, as the ruler and the EQ (UI-326)
    s.coarse_press = true;
    press_heading(&mut s, &g, vocal);
    assert!(!s.insert_drag_move(x + 6.0, y + 6.0, false));
    assert!(!s.insert_drag_up(&g, x + 6.0, y + 6.0, false), "a press");
    press_heading(&mut s, &g, vocal);
    assert!(s.insert_drag_move(x + 10.0, y, false));
    let at = area(&s, &g, guitar);
    assert!(s.insert_drag_move(at.0, at.1, false));
    assert!(s.insert_drag_up(&g, at.0, at.1, false));
    assert_eq!(inserts(&s, guitar), ["Compressor", "EQ"]);
}
