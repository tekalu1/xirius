use super::*;
use crate::{
    studio::{Geometry, Studio},
    tools::{Mods, Tool},
};

fn fixture() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    let c = s.roll_ctx(&g).unwrap();
    s.song
        .run("note.delete", json!({"ids":[c.clip.id.to_string()]}))
        .unwrap();
    s.song.run("note.put",json!({"clip":c.clip.id.to_string(),"notes":[{"at":960,"pitch":60,"length":960,"velocity":70},{"at":1920,"pitch":60,"length":960,"velocity":90}]})).unwrap();
    s.resized(&g);
    s.roll.center(&s.roll_ctx(&g).unwrap());
    (s, g)
}

fn gesture(
    s: &mut Studio,
    g: &Geometry,
    from: (f32, f32),
    to: (f32, f32),
    mods: Mods,
    clicks: u32,
) {
    let c = s.roll_ctx(g).unwrap();
    let out = s.roll.down(&c, from.0, from.1, mods, clicks);
    s.apply_roll(g, out);
    let c = s.roll_ctx(g).unwrap();
    let out = s.roll.moved(&c, to.0, to.1, mods);
    s.apply_roll(g, out);
    let c = s.roll_ctx(g).unwrap();
    let out = s.roll.up(&c, to.0, to.1, mods);
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn low_notes_and_basic_arrow_disable_vertical_zones_but_keep_edges() {
    let (mut s, g) = fixture();
    for (height, extended) in [(10.0, true), (24.0, false)] {
        s.roll.row_h = height;
        s.roll.extended = extended;
        s.roll.center(&s.roll_ctx(&g).unwrap());
        let c = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&c);
        let n = c.notes()[0];
        for y in [v.y(n.pitch) + 1.0, v.y(n.pitch) + height - 1.0] {
            assert_eq!(
                s.roll
                    .hit_note(&c, &v, v.x(n.start) + v.px(n.length) / 2.0, y),
                Some((n.id, Zone::Body))
            );
        }
        assert_eq!(
            s.roll
                .hit_note(&c, &v, v.x(n.start) + 1.0, v.y(n.pitch) + height / 2.0),
            Some((n.id, Zone::EdgeL))
        );
    }
}

#[test]
fn velocities_move_relatively_and_become_the_next_drawn_velocity() {
    let (mut s, g) = fixture();
    s.roll.extended = true;
    let c = s.roll_ctx(&g).unwrap();
    s.roll.selected = c.notes().iter().map(|n| n.id).collect();
    let v = s.roll.view(&c);
    let from = (v.x(1440), v.y(60) + 1.0);
    gesture(&mut s, &g, from, (from.0, from.1 - 10.0), Mods::NONE, 1);
    let c = s.roll_ctx(&g).unwrap();
    assert_eq!(
        c.notes().iter().map(|n| n.velocity).collect::<Vec<_>>(),
        [81, 101]
    );
    assert_eq!(s.roll.next_vel, 81);
    let from = (v.x(1440), v.y(60) + 7.0);
    gesture(
        &mut s,
        &g,
        from,
        (from.0, from.1 + 10.0),
        Mods {
            ctrl: true,
            alt: true,
            ..Mods::NONE
        },
        1,
    );
    assert_eq!(
        s.roll_ctx(&g)
            .unwrap()
            .notes()
            .iter()
            .map(|n| n.velocity)
            .collect::<Vec<_>>(),
        [70, 90]
    );
}

#[test]
fn the_bottom_of_a_note_boundary_joins_and_one_undo_restores_both() {
    let (mut s, g) = fixture();
    s.roll.extended = true;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(1920), v.y(60) + v.row_h - 1.0);
    gesture(&mut s, &g, p, p, Mods::NONE, 1);
    let c = s.roll_ctx(&g).unwrap();
    assert_eq!(c.notes().len(), 1);
    assert_eq!(c.notes()[0].length, 1920);
    assert!(s.song.undo());
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 2);
}

#[test]
fn double_click_deletes_and_creates_at_the_quantize_length() {
    let (mut s, g) = fixture();
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(1440), v.y(60) + 7.0);
    gesture(&mut s, &g, p, p, Mods::NONE, 2);
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 1);
    let p = (v.x(3360), v.y(62) + 7.0);
    gesture(&mut s, &g, p, p, Mods::NONE, 2);
    let c = s.roll_ctx(&g).unwrap();
    assert!(
        c.notes()
            .iter()
            .any(|n| n.pitch == 62 && n.length == c.tools.snap.grid)
    );
}

#[test]
fn keyboard_shift_selects_pitch_without_audition_and_drag_tracks_pitch() {
    let (mut s, g) = fixture();
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let f = s.roll.frame(c.body);
    let p = (f.keys.x + 12.0, v.y(60) + 7.0);
    gesture(&mut s, &g, p, p, Mods::shift(), 1);
    assert_eq!(s.roll.selected.len(), 2);
    assert!(s.roll.audition.is_none());
    let out = s.roll.down(&c, p.0, p.1, Mods::NONE, 1);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, Some(60));
    let out = s.roll.moved(&c, p.0, p.1 - v.row_h * 2.0, Mods::NONE);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, Some(62));
    let out = s.roll.up(&c, p.0, p.1, Mods::NONE);
    s.apply_roll(&g, out);
    assert!(s.roll.audition.is_none());
}

#[test]
fn dragging_outside_a_part_creates_a_new_part_and_selects_it() {
    let (mut s, g) = fixture();
    let old = s.selected_clip.unwrap();
    s.roll.beat_px = 24.0;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(c.clip.length + 960), v.y(61) + 7.0);
    gesture(&mut s, &g, p, (p.0 + v.px(960), p.1), Mods::NONE, 1);
    assert_ne!(s.selected_clip.unwrap().1, old.1);
    assert!(s.roll_ctx(&g).unwrap().notes().is_empty());
    assert_eq!(s.roll_ctx(&g).unwrap().clip.length, 960);
}

#[test]
fn empty_click_switches_to_the_part_under_the_pointer() {
    let (mut s, g) = fixture();
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let track = s.selected_clip.unwrap().0;
    let id = s
        .song
        .run(
            "clip.add.midi",
            json!({"track":track.to_string(),"at":c.clip.start+2400,"length":960}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let p = (v.x(2500), v.y(63) + 7.0);
    assert!(s.roll_press(&g, p.0, p.1, Mods::NONE, 1));
    // Overlapping parts prefer the active one; an adjacent part is unambiguous.
    s.song
        .run(
            "clip.move",
            json!({"ids":[id],"to":c.clip.start+c.clip.length}),
        )
        .unwrap();
    s.roll.beat_px = 24.0;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(c.clip.length + 240), v.y(63) + 7.0);
    assert!(s.roll_press(&g, p.0, p.1, Mods::NONE, 1));
    assert_eq!(s.selected_clip.unwrap().1.to_string(), id);
    assert!(s.roll.selected.is_empty());
}

#[test]
fn shift_q_restores_timing_after_q_and_alt_q() {
    let (mut s, g) = fixture();
    let c = s.roll_ctx(&g).unwrap();
    let id = c.notes()[0].id;
    s.song
        .run(
            "note.set",
            json!({"notes":[{"id":id.to_string(),"start":977}]}),
        )
        .unwrap();
    s.roll.selected = std::collections::BTreeSet::from([id]);
    for m in [Mods::alt(), Mods::NONE, Mods::shift()] {
        assert!(s.edit_key(&g, "q", m));
    }
    assert_eq!(s.roll_ctx(&g).unwrap().note(id).unwrap().start, 977);
}

#[test]
fn repeated_arrow_button_and_key_toggle_basic_and_extended() {
    let (mut s, g) = fixture();
    let mode = |s: &Studio| {
        s.song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .text("edit.arrow.mode")
    };
    // The arrow starts basic (2026-10-21).
    assert!(!s.roll.extended);
    assert_eq!(mode(&s), "basic");
    assert!(s.edit_key(&g, "1", Mods::NONE));
    assert!(s.roll.extended);
    assert_eq!(mode(&s), "extended");
    assert!(s.edit_key(&g, "1", Mods::NONE));
    assert!(!s.roll.extended);
    assert_eq!(mode(&s), "basic");
    assert!(s.edit_key(&g, "1", Mods::NONE));
    assert!(s.roll.extended);
    s.tool = Tool::Draw;
    s.edit_key(&g, "1", Mods::NONE);
    assert!(s.roll.extended);
}

#[test]
fn mousemap_file_changes_the_temporary_eraser_without_changing_the_picked_tool() {
    let (mut s, g) = fixture();
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-edit3-mousemap-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("edit3-mousemap");
    std::fs::create_dir_all(&dir).unwrap();
    let mut map = xirius_prefs::Mousemap::default();
    map.set("roll.select.hold", "Alt", "tool:erase").unwrap();
    std::fs::write(dir.join("mousemap.toml"), map.to_toml()).unwrap();
    s.song.load_settings(&dir);
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(1440), v.y(60) + 7.0);
    gesture(&mut s, &g, p, p, Mods::alt(), 1);
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 1);
    assert_eq!(s.tool, Tool::Select);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn both_editor_settings_and_quantize_panels_remain_independent() {
    use crate::{
        popup::{Action, PopupKind},
        quantize_panel::{Field, QAct},
        studio::Rect,
        tools::SnapMode,
    };
    let (mut s, g) = fixture();
    s.open_popup(PopupKind::Grid, Rect::default());
    s.popup_action(&g, Action::GridValue("1/8"));
    s.popup_action(&g, Action::Snap(false));
    s.popup = None;
    assert_eq!(s.song.grid_for("arrange"), "1/16");
    assert!(s.song.snap_for("arrange"));
    s.quant_action(&g, QAct::Step(Field::Start, false));
    assert_eq!(s.quant.start, 95);
    s.save_quant_settings();
    s.arrange.focus = true;
    s.open_popup(PopupKind::Grid, Rect::default());
    s.popup_action(&g, Action::GridValue("1/4"));
    s.quant_action(&g, QAct::Step(Field::End, true));
    assert_eq!(s.quant.start, 100);
    assert_eq!(s.quant.end, 5);
    s.popup = None;
    s.arrange.focus = false;
    s.choose_editor();
    assert_eq!(s.quant.start, 95);
    assert_eq!(s.quant.end, 0);
    assert_eq!(s.song.grid_for("roll"), "1/8");
    assert_eq!(s.roll_grid_mode(), SnapMode::Quantize);
}

#[test]
fn scale_marks_follow_the_selected_root_and_mode() {
    let (mut s, _) = fixture();
    s.run_command("view.scale", json!({"root":2,"scale":"minor"}));
    assert!(s.roll.in_scale(62));
    assert!(s.roll.in_scale(65));
    assert!(!s.roll.in_scale(66));
    s.run_command("view.scale", json!({"scale":"off"}));
    assert!(!s.roll.in_scale(62));
}

#[test]
fn dragging_the_second_of_multiple_selected_notes_remembers_its_velocity() {
    let (mut s, g) = fixture();
    s.roll.extended = true;
    let c = s.roll_ctx(&g).unwrap();
    s.roll.selected = c.notes().iter().map(|n| n.id).collect();
    let v = s.roll.view(&c);
    let p = (v.x(2400), v.y(60) + 1.0);
    gesture(&mut s, &g, p, (p.0, p.1 - 10.0), Mods::NONE, 1);
    assert_eq!(s.roll.next_vel, 101);
}

#[test]
fn keyboard_hover_requests_redraw_when_the_pitch_changes() {
    let (mut s, g) = fixture();
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let x = s.roll.frame(c.body).keys.x + 10.0;
    assert!(s.roll.moved(&c, x, v.y(60) + 7.0, Mods::NONE).redraw);
    assert!(s.roll.moved(&c, x, v.y(61) + 7.0, Mods::NONE).redraw);
}

#[test]
fn imported_midi_groove_and_panel_settings_survive_save_and_open() {
    use crate::quantize_panel::{Field, QAct};
    let (mut s, g) = fixture();
    let root_tmp = tempfile::Builder::new()
        .prefix("xirius-ui-groove-")
        .tempdir()
        .unwrap();
    let root = root_tmp.path().join("ui-groove");
    std::fs::create_dir_all(&root).unwrap();
    let mut bytes = b"MThd".to_vec();
    bytes.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
    bytes.extend(b"MTrk");
    bytes.extend([
        0, 0, 0, 12, 0, 0x90, 60, 80, 96, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
    ]);
    let file = root.join("groove.mid");
    std::fs::write(&file, bytes).unwrap();
    s.import_groove(&file).unwrap();
    s.quant_action(&g, QAct::Step(Field::End, true));
    let expected = s.quant;
    s.song.save_as(&root.join("song")).unwrap();
    s.song.open(&root.join("song")).unwrap();
    s.choose_editor();
    assert_eq!(s.quant, expected);
    assert_eq!(
        s.song
            .session()
            .state
            .view
            .roll
            .external_groove
            .as_ref()
            .unwrap()
            .hits,
        [(0, 80)]
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_velocity_setting_is_what_the_pencil_draws_and_last_keeps_the_edited_one() {
    let (mut s, g) = fixture();
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.draw.velocity-value", "value": "64"}),
    );
    s.apply_edit_prefs();
    assert_eq!(s.roll.next_vel, 64);
    s.tool = Tool::Draw;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let p = (v.x(3360), v.y(62) + 7.0);
    gesture(&mut s, &g, p, p, Mods::NONE, 1);
    let c = s.roll_ctx(&g).unwrap();
    let drawn: Vec<u8> = c
        .notes()
        .iter()
        .filter(|n| n.pitch == 62)
        .map(|n| n.velocity)
        .collect();
    assert_eq!(drawn, [64]);
    // 「前回の値」: a new value in the settings leaves the velocity used last.
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.draw.velocity", "value": "last"}),
    );
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.draw.velocity-value", "value": "30"}),
    );
    s.apply_edit_prefs();
    assert_eq!(s.roll.next_vel, 64);
}

#[test]
fn the_arrow_setting_basic_takes_the_velocity_strip_off_the_notes() {
    let (mut s, g) = fixture();
    s.roll.row_h = 24.0;
    s.roll.center(&s.roll_ctx(&g).unwrap());
    let upper = |s: &Studio| {
        let c = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&c);
        let n = c.notes()[0];
        s.roll
            .hit_note(
                &c,
                &v,
                v.x(n.start) + v.px(n.length) / 2.0,
                v.y(n.pitch) + 1.0,
            )
            .map(|(_, z)| z)
    };
    // Without the setting the arrow is basic (2026-10-21).
    s.apply_edit_prefs();
    assert!(!s.roll.extended);
    assert_eq!(upper(&s), Some(Zone::Body));
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrow.mode", "value": "extended"}),
    );
    s.apply_edit_prefs();
    assert_eq!(upper(&s), Some(Zone::Velocity));
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrow.mode", "value": "basic"}),
    );
    s.apply_edit_prefs();
    assert!(!s.roll.extended);
    assert_eq!(upper(&s), Some(Zone::Body));
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrow.mode", "value": "extended"}),
    );
    s.apply_edit_prefs();
    assert_eq!(upper(&s), Some(Zone::Velocity));
}
