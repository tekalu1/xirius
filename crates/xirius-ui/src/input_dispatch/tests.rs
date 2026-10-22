use super::*;

fn setup() -> (Studio, Geometry) {
    let mut studio = Studio::sample();
    let geometry = studio.geometry(1440.0, 860.0);
    studio.resized(&geometry);
    // Song::sample attaches Settings::memory, never the person's config directory.
    assert!(studio.song.settings().lock().unwrap().dir.is_none());
    (studio, geometry)
}

#[test]
fn ops_014_loop_editor_and_dialog_keys_run_through_the_preset() {
    let (mut s, g) = setup();
    s.arrange.focus = true;
    let (t, c) = s.selected_clip.unwrap();
    s.arrange.clips.insert((t, c));
    assert!(s.edit_key(&g, "p", Mods::shift()));
    assert!(s.project().loop_range.is_some());
    s.drawer_open = false;
    assert!(s.edit_key(&g, "f2", Mods::NONE));
    assert!(s.drawer_open);
    assert_eq!(s.tab, DrawerTab::Roll);
    assert!(s.edit_key(&g, "t", Mods::NONE));
    assert!(s.popup.is_some());
    s.popup = None;
    assert!(s.edit_key(&g, "e", Mods::ctrl()));
    assert!(!s.export.stems);
    assert!(s.edit_key(&g, "e", Mods::ctrl_shift()));
    assert!(s.export.stems);
}

#[test]
fn key_007_keys_existing_note_menu_and_registry_call_the_same_command() {
    let (mut s, g) = setup();
    let ctx = s.roll_ctx(&g).unwrap();
    let n = &ctx.notes()[0];
    let id = n.id;
    s.roll.selected.insert(id);
    let from_key = s.roll.key(&ctx, "d", Mods::NONE);
    let view = s.roll.view(&ctx);
    let at = (
        view.x(n.start) + view.px(n.length) / 2.0,
        view.y(n.pitch) + view.row_h / 2.0,
    );
    assert!(s.scene_context(&g, at.0, at.1));
    let row = s
        .context_rows()
        .iter()
        .position(|r| r.label == "ノートを複製")
        .expect("the note menu has the row");
    assert!(s.context_pick(&g, row));
    assert_eq!(from_key.edits[0].command, "note.duplicate");
    assert_eq!(
        s.song
            .history()
            .into_iter()
            .find(|h| h.current)
            .unwrap()
            .command,
        "note.duplicate"
    );
}

#[test]
fn key_020_area_preferences_are_applied_when_focus_changes() {
    let (mut s, g) = setup();
    for (key, value) in [
        ("edit.snap.arrange.mode", json!("bar")),
        ("edit.snap.roll.mode", json!("quantize")),
        ("edit.snap.arrange.grid", json!("false")),
    ] {
        s.song
            .run("prefs.set", json!({"key":key,"value":value}))
            .unwrap();
    }
    s.arrange.focus = true;
    s.apply_edit_prefs();
    assert_eq!(s.snap_mode, crate::tools::SnapMode::Bar);
    assert!(!s.song.snap());
    s.arrange.focus = false;
    assert!(s.apply_edit_prefs());
    assert_eq!(s.grid_mode(), crate::tools::SnapMode::Quantize);
    assert_eq!(s.snap_mode, crate::tools::SnapMode::Bar);
    assert!(s.song.snap());
    assert_eq!(s.input_context(&g), "roll");
}

#[test]
fn key_016_017_mousemap_changes_actual_drag_during_hold_and_click() {
    let (mut s, g) = setup();
    let steps = s.song.steps();
    for (context, modifier, action) in [
        ("roll.select.note-body.drag", "Alt", "note.move"),
        ("roll.select.note-body.drag", "during.Mod", "snap.invert"),
        ("roll.select.hold", "Shift", "tool:erase"),
        ("arrange.select.clip-body.click", "Alt", "select.toggle"),
    ] {
        s.song
            .run(
                "mousemap.set",
                json!({"context":context,"modifier":modifier,"action":action}),
            )
            .unwrap();
    }
    assert_eq!(s.song.steps(), steps);
    let ctx = s.roll_ctx(&g).unwrap();
    let n = &ctx.notes()[0];
    let view = s.roll.view(&ctx);
    let at = (
        view.x(n.start) + view.px(n.length) / 2.0,
        view.y(n.pitch) + view.row_h / 2.0,
    );
    assert_eq!(
        crate::mouse_dispatch::roll_zone(crate::roll::Zone::Body),
        "note-body"
    );
    s.roll.down(&ctx, at.0, at.1, Mods::alt(), 1);
    s.roll.moved(&ctx, at.0 + 30.0, at.1, Mods::alt());
    assert!(
        s.roll.preview.ghosts.is_empty(),
        "mapped move must not duplicate"
    );
    s.roll.mods_changed(
        &ctx,
        Mods {
            ctrl: true,
            alt: true,
            shift: false,
        },
    );
    assert!(s.roll.mods.shift, "the configured during.Mod inverts snap");
    assert_eq!(
        s.holds()
            .tool(crate::tools::Area::Roll, Tool::Select, Mods::shift()),
        Tool::Eraser
    );
    s.roll.cancel();
    let at = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
    s.arrange_down(&g, at.0, at.1, Mods::NONE);
    s.arrange_up(&g, at.0, at.1, Mods::NONE);
    assert_eq!(s.arrange.clips.len(), 1);
    s.arrange_down(&g, at.0, at.1, Mods::alt());
    s.arrange_up(&g, at.0, at.1, Mods::alt());
    assert!(s.arrange.clips.is_empty());
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn key_017_fade_hit_uses_the_same_name_as_the_settings_preview() {
    let (mut s, g) = setup();
    let (track, clip) = s.selected_clip.unwrap();
    let spot = crate::arrange::Spot::Fx {
        track,
        clip,
        part: crate::arrange::clipfx::FxPart::Flag(crate::arrange::clipfx::Side::In),
    };
    assert_eq!(
        crate::mouse_dispatch::arrange_zone(&spot),
        Some("fade-handle")
    );
    assert!(xirius_prefs::mousemap::Ctx::parse("arrange.select.fade-handle.drag").is_ok());
    s.open_settings(crate::settings::Section::Keys);
    s.settings.as_mut().unwrap().sub = crate::settings::Sub::Mouse;
    s.settings.as_mut().unwrap().area = "arrange".into();
    assert!(
        s.settings_scene(&g)
            .items
            .iter()
            .any(|i| i.act == crate::settings::Do::Zone("fade-handle".into()))
    );
}

#[test]
fn edt_069_selection_toggle_and_palette_rectangle_work_in_both_editors() {
    for roll in [true, false] {
        let (mut s, g) = setup();
        s.arrange.focus = !roll;
        let (t, c) = s.selected_clip.unwrap();
        s.arrange.clips.insert((t, c));
        let ctx = s.roll_ctx(&g).unwrap();
        s.roll.selected.insert(ctx.notes()[0].id);
        let before = s.zoom_snapshot();
        assert!(s.edit_key(&g, "s", Mods::shift()));
        assert_ne!(s.zoom_snapshot(), before);
        assert!(s.edit_key(&g, "s", Mods::shift()));
        assert_eq!(s.zoom_snapshot(), before);
        assert!(s.run_binding(&g, "view.zoom.rect"));
        assert_eq!(s.zoom_snapshot(), before);
        let bounds = if roll {
            s.roll.frame(ctx.body).grid
        } else {
            g.lanes
        };
        assert!(s.zoom_box_down(&g, bounds.x + 30.0, bounds.y + 30.0, Mods::NONE));
        assert!(s.zoom_box_up(&g, bounds.x + 200.0, bounds.y + 120.0));
        assert_ne!(s.zoom_snapshot(), before);
    }
}

#[test]
fn selection_tool_and_snap_are_resolved_without_song_history() {
    let (mut studio, geometry) = setup();
    let steps = studio.song.steps();
    assert!(studio.edit_key(&geometry, "5", Mods::default()));
    assert_eq!(studio.tool, Tool::Draw);
    let snap = studio.song.snap();
    assert!(studio.edit_key(&geometry, "n", Mods::default()));
    assert_eq!(studio.song.snap(), !snap);
    assert_eq!(studio.song.steps(), steps);
}

#[test]
fn key_002_track_name_typing_owns_q_and_digits() {
    let (mut studio, g) = setup();
    // The row the test renames may be an instrument track; the instrument-input
    // arm that would follow the selection is not what this test checks (REC-024).
    studio.rec_set(
        "prefs.set",
        serde_json::json!({"key": "midi.follow-selection", "value": "false"}),
    );
    let RowKind::Track(t) = studio.selected_row.unwrap() else {
        panic!("track");
    };
    let row = studio.tree().row_of(RowKind::Track(t)).unwrap();
    let parts = crate::studio::RowParts::of(row);
    let (x, y) = (
        g.column.x + parts.name_x + 3.0,
        g.column.y + row.y + crate::studio::RowParts::LINE_1_Y + 3.0,
    );
    let before = (*studio.project()).clone();
    let steps = studio.song.steps();
    assert!(studio.name_press(&g, x, y, 2));
    assert_eq!(studio.input_context(&g), "text");
    for key in ["q", "1", "5", "m", "n"] {
        assert!(studio.edit_key(&g, key, Mods::NONE));
        studio
            .input_map
            .name
            .as_mut()
            .unwrap()
            .field
            .replace_text(None, key);
    }
    assert_eq!(*studio.project(), before);
    assert_eq!(studio.tool, Tool::Select);
    assert_eq!(studio.song.steps(), steps);
    studio
        .input_map
        .name
        .as_mut()
        .unwrap()
        .field
        .mark_text(None, "あ");
    assert!(studio.edit_key(&g, "enter", Mods::NONE)); // IME owns Enter during composition
    assert!(studio.input_map.name.is_some());
    studio.input_map.name.as_mut().unwrap().field.unmark();
    assert!(studio.edit_key(&g, "enter", Mods::NONE));
    assert!(studio.project().tracks[&t].name.ends_with("q15mnあ"));
    assert_eq!(studio.song.steps(), steps + 1);
}

#[test]
fn key_004_m_and_shift_m_have_distinct_track_clip_and_note_targets() {
    let (mut s, g) = setup();
    let (t, c) = s.selected_clip.unwrap();
    let ctx = s.roll_ctx(&g).unwrap();
    let n = ctx.notes()[0].id;
    s.roll.selected.insert(n);
    let original_clip = s.project().tracks[&t]
        .clips
        .iter()
        .find(|x| x.id == c)
        .unwrap()
        .muted;
    assert!(s.edit_key(&g, "m", Mods::NONE));
    assert!(s.project().tracks[&t].mute);
    assert!(s.edit_key(&g, "m", Mods::shift()));
    assert!(s.roll_ctx(&g).unwrap().note(n).unwrap().muted);
    assert_eq!(
        s.project().tracks[&t]
            .clips
            .iter()
            .find(|x| x.id == c)
            .unwrap()
            .muted,
        original_clip
    );
    s.arrange.focus = true;
    s.arrange.clips.insert((t, c));
    assert!(s.edit_key(&g, "m", Mods::shift()));
    assert_eq!(
        s.project().tracks[&t]
            .clips
            .iter()
            .find(|x| x.id == c)
            .unwrap()
            .muted,
        !original_clip
    );
    assert!(s.project().tracks[&t].mute);
}

#[test]
fn key_008_custom_keys_override_parents_in_all_contexts_and_show_in_palette() {
    let (mut s, g) = setup();
    s.song
        .run(
            "keymap.set",
            json!({"key":"F8","command":"tool.draw","context":"edit"}),
        )
        .unwrap();
    s.song
        .run(
            "keymap.set",
            json!({"key":"F8","command":"tool.erase","context":"roll","shadow":true}),
        )
        .unwrap();
    assert!(s.edit_key(&g, "f8", Mods::NONE));
    assert_eq!(s.tool, Tool::Eraser);
    s.arrange.focus = true;
    assert!(s.edit_key(&g, "f8", Mods::NONE));
    assert_eq!(s.tool, Tool::Draw);
    for context in [
        "global",
        "edit",
        "arrange",
        "roll",
        "lane",
        "automation",
        "mixer",
        "browser",
        "text",
    ] {
        assert!(xirius_prefs::catalog::context(context).is_some());
    }
    let rows = s.palette_rows(&g);
    assert!(
        rows.iter()
            .find(|r| r.name == "tool.draw")
            .unwrap()
            .key
            .as_deref()
            .unwrap()
            .contains("F8")
    );
    assert!(s.key_display("roll", "tool.erase").contains("F8")); // KEY-025 API
}

#[test]
fn aut_011_keys_open_lanes_change_mode_and_walk_parameters() {
    let (mut s, g) = setup();
    let RowKind::Track(t) = s.selected_row.unwrap() else {
        panic!()
    };
    s.arrange.focus = true;
    s.song
        .run("auto.mode", json!({"ids":[t.to_string()],"mode":"off"}))
        .unwrap();
    assert!(s.edit_key(&g, "a", Mods::NONE));
    assert!(s.arrange.open.contains(&t));
    assert!(s.edit_key(&g, "j", Mods::NONE));
    assert_eq!(
        s.project().tracks[&t].auto_mode,
        xirius_core::auto::AutoMode::Read
    );
    assert!(s.edit_key(&g, "j", Mods::NONE));
    assert_eq!(
        s.project().tracks[&t].auto_mode,
        xirius_core::auto::AutoMode::Off
    );
    assert!(s.edit_key(&g, "k", Mods::NONE));
    assert_eq!(
        s.project().tracks[&t].auto_mode,
        xirius_core::auto::AutoMode::Touch
    );
    assert!(s.edit_key(&g, "h", Mods::NONE));
    assert_eq!(s.input_map.parameter, 1);
    assert!(s.edit_key(&g, "h", Mods::shift()));
    assert_eq!(s.input_map.parameter, 0);
    assert_eq!(
        s.arrange.lanes_of(&s.project().tracks[&t])[0]
            .target
            .to_string(),
        "volume"
    );
}

#[test]
fn edt_069_zoom_keys_rectangle_and_history_are_independent_of_song() {
    let (mut s, g) = setup();
    s.arrange.focus = true;
    let steps = s.song.steps();
    let initial = s.zoom_snapshot();
    assert!(s.edit_key(&g, "z", Mods::shift()));
    for (key, mods) in [
        ("e", Mods::NONE),
        ("w", Mods::NONE),
        ("e", Mods::shift()),
        ("w", Mods::shift()),
        ("z", Mods::alt()),
        ("s", Mods::alt()),
        ("l", Mods::shift()),
    ] {
        assert!(s.edit_key(&g, key, mods), "{key}: {mods:?}");
    }
    assert!(s.edit_key(&g, "z", Mods::NONE));
    assert_eq!(s.zoom_snapshot(), initial);
    let before = s.zoom_snapshot();
    assert!(s.zoom_box_down(&g, g.lanes.x + 100.0, g.lanes.y + 30.0, Mods::alt_shift()));
    assert!(s.zoom_box_up(&g, g.lanes.x + 300.0, g.lanes.y + 150.0));
    assert!(s.beat_w > before.time);
    assert!(s.edit_key(&g, "w", Mods::alt()));
    assert_eq!(s.zoom_snapshot(), before);
    assert!(s.edit_key(&g, "e", Mods::alt()));
    assert!(s.beat_w > before.time);
    let zoomed = s.zoom_snapshot();
    assert!(s.zoom_box_down(&g, g.lanes.x + 50.0, g.lanes.y + 20.0, Mods::alt_shift()));
    assert!(s.zoom_box_up(&g, g.lanes.x + 50.0, g.lanes.y + 20.0));
    assert_eq!(s.zoom_snapshot(), before);
    assert_ne!(zoomed, before);
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn key_010_jis_plus_and_physical_slash_reach_their_bindings() {
    let (mut s, g) = setup();
    s.arrange.focus = true;
    let before = s.beat_w;
    assert!(s.mapped_key(&g, ";", Some("+"), Mods::ctrl_shift()));
    assert!(s.beat_w > before);
    s.song
        .run(
            "keymap.set",
            json!({"key":"code:Slash","command":"tool.erase","context":"edit"}),
        )
        .unwrap();
    assert!(s.mapped_key_code(&g, "?", Some("?"), Mods::NONE, Some("code:Slash")));
    assert_eq!(s.tool, Tool::Eraser);
    assert_ne!(Chord::from_event("0", 0), Chord::from_event("numpad0", 0));
}

#[test]
fn key_018_transient_markers_do_not_change_the_project_or_history() {
    let (mut s, _) = setup();
    let before = (*s.project()).clone();
    let steps = s.song.steps();
    for command in ["snap.invert", "tool.erase", "tool.draw"] {
        s.song.run(command, json!({})).unwrap();
    }
    assert_eq!(*s.project(), before);
    assert_eq!(s.song.steps(), steps);
}

fn map(s: &mut Studio, context: &str, modifier: &str, action: &str) {
    s.song
        .run(
            "mousemap.set",
            json!({"context":context,"modifier":modifier,"action":action}),
        )
        .unwrap();
}

fn points_of(s: &Studio, name: &str) -> usize {
    let t = crate::arrange::probe::track(s, name);
    s.project().tracks[&t].automation[0].points.len()
}

/// A place on the first automation lane that is neither a point nor the line: before the first point.
fn empty_lane_xy(s: &Studio, g: &Geometry, t: xirius_core::id::TrackId) -> (f32, f32) {
    let (x, y) = crate::arrange::probe::point_xy(s, g, t, 0, 0);
    (x - 30.0, y)
}

#[test]
fn key_017_automation_hit_areas_are_the_maps_contexts() {
    use crate::arrange::Spot;
    let (mut s, g) = setup();
    let t = crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
    let (px, py) = crate::arrange::probe::point_xy(&s, &g, t, 0, 1);
    let spot = s.arrange_spot(&g, px, py);
    assert!(matches!(spot, Spot::Point { .. }), "{spot:?}");
    assert_eq!(
        crate::mouse_dispatch::gesture_base(&spot, Tool::Select).as_deref(),
        Some("automation.select.point")
    );
    assert_eq!(
        crate::mouse_dispatch::gesture_base(&spot, Tool::Eraser).as_deref(),
        Some("automation.erase.point")
    );
    let (qx, qy) = crate::arrange::probe::point_xy(&s, &g, t, 0, 2);
    let mid = s.arrange_spot(&g, (px + qx) / 2.0, (py + qy) / 2.0);
    if matches!(mid, Spot::Line { .. }) {
        assert_eq!(
            crate::mouse_dispatch::gesture_base(&mid, Tool::Select).as_deref(),
            Some("automation.select.line")
        );
    }
    let (ex, ey) = empty_lane_xy(&s, &g, t);
    let empty = s.arrange_spot(&g, ex, ey);
    assert!(matches!(empty, Spot::Lane { .. }), "{empty:?}");
    assert_eq!(
        crate::mouse_dispatch::gesture_base(&empty, Tool::Draw).as_deref(),
        Some("automation.select.empty")
    );
    for spot in [&spot, &empty] {
        for gesture in ["click", "drag"] {
            let name = format!(
                "{}.{gesture}",
                crate::mouse_dispatch::gesture_base(spot, Tool::Select).unwrap()
            );
            assert!(xirius_prefs::mousemap::Ctx::parse(&name).is_ok(), "{name}");
        }
    }
}

#[test]
fn key_017_an_automation_click_does_what_the_map_says() {
    let (mut s, g) = setup();
    let t = crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
    let (px, py) = crate::arrange::probe::point_xy(&s, &g, t, 0, 1);
    let (ex, ey) = empty_lane_xy(&s, &g, t);
    let before = points_of(&s, "Bass");
    // As it is: Alt + click takes the point away; a double-click on the empty lane adds one.
    s.arrange_down(&g, px, py, Mods::alt());
    s.arrange_up(&g, px, py, Mods::alt());
    assert_eq!(points_of(&s, "Bass"), before - 1);
    assert!(s.song.undo());
    s.arrange_down(&g, ex, ey, Mods::NONE);
    s.arrange_up(&g, ex, ey, Mods::NONE);
    assert_eq!(points_of(&s, "Bass"), before, "one click is a marquee");
    s.arrange.clicks = 2;
    s.arrange_down(&g, ex, ey, Mods::NONE);
    s.arrange_up(&g, ex, ey, Mods::NONE);
    s.arrange.clicks = 1;
    assert_eq!(points_of(&s, "Bass"), before + 1);
    assert!(s.song.undo());
    assert_eq!(points_of(&s, "Bass"), before);
    // Alt + click only selects, when the map says so.
    map(&mut s, "automation.select.point.click", "Alt", "select.set");
    s.arrange_down(&g, px, py, Mods::alt());
    s.arrange_up(&g, px, py, Mods::alt());
    assert_eq!(points_of(&s, "Bass"), before);
    assert!(!s.arrange.points.is_empty());
    // Nothing for the empty lane's click and drag: no point is added.
    map(&mut s, "automation.select.empty.click", "none", "");
    map(&mut s, "automation.select.empty.drag", "none", "");
    let steps = s.song.steps();
    s.arrange_down(&g, ex, ey, Mods::NONE);
    s.arrange_up(&g, ex, ey, Mods::NONE);
    assert_eq!(s.song.steps(), steps);
    assert_eq!(points_of(&s, "Bass"), before);
}

#[test]
fn key_017_an_automation_drag_follows_the_map() {
    let (mut s, g) = setup();
    let t = crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
    let (px, py) = crate::arrange::probe::point_xy(&s, &g, t, 0, 1);
    let to = (px + 20.0, py - 12.0);
    let steps = s.song.steps();
    crate::arrange::probe::hold(&mut s, &g, (px, py), to, Mods::NONE);
    s.arrange_up(&g, to.0, to.1, Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1, "the point moves as it is");
    assert!(s.song.undo());
    map(&mut s, "automation.select.point.drag", "none", "");
    let steps = s.song.steps();
    crate::arrange::probe::hold(&mut s, &g, (px, py), to, Mods::NONE);
    s.arrange_up(&g, to.0, to.1, Mods::NONE);
    assert_eq!(s.song.steps(), steps, "nothing is bound to the drag");
    // The axis is held by the modifier the map names during the drag.
    map(
        &mut s,
        "automation.select.point.drag",
        "none",
        "auto.point.move",
    );
    map(
        &mut s,
        "automation.select.point.drag",
        "during.Alt",
        "auto.point.move --lock",
    );
    let mm = s.song.settings().lock().unwrap().mousemap.clone();
    let ctx = "automation.select.point.drag";
    assert!(crate::mouse_dispatch::during(&mm, ctx, Mods::alt(), Mods::alt()).ctrl);
    assert!(!crate::mouse_dispatch::during(&mm, ctx, Mods::NONE, Mods::NONE).ctrl);
}

#[test]
fn key_017_the_ruler_click_and_wheel_follow_the_map() {
    let (mut s, g) = setup();
    let tick = 3840 * 3;
    s.press(&g, crate::studio::Hit::Ruler(tick));
    assert_eq!(s.playhead, tick, "a click moves the play position");
    let (x, y) = (g.ruler.x + g.ruler.w / 2.0, g.ruler.y + g.ruler.h - 4.0);
    let before = s.beat_w;
    assert!(s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::NONE));
    assert_ne!(s.beat_w, before, "the wheel zooms in time");
    // With Ctrl held it is the lanes' (as the pref has it).
    assert!(!s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::ctrl()));
    map(&mut s, "ruler.any.empty.wheel", "Mod", "view.zoom.time");
    let before = s.beat_w;
    assert!(s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::ctrl()));
    assert_ne!(s.beat_w, before);
    // The plain wheel scrolls: it is left to the lanes.
    map(&mut s, "ruler.any.empty.wheel", "none", "view.scroll");
    let before = s.beat_w;
    assert!(!s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::NONE));
    // Nothing is bound: the wheel is spent and the view stays.
    map(&mut s, "ruler.any.empty.wheel", "none", "");
    assert!(s.ruler_wheel(&g, x, y, 0.0, 120.0, Mods::NONE));
    assert_eq!(s.beat_w, before);
    // The old arrangement name reaches the same cell.
    map(&mut s, "arrange.select.ruler.click", "none", "");
    s.press(&g, crate::studio::Hit::Ruler(0));
    assert_eq!(s.playhead, tick, "no click on the ruler moves it");
}

#[test]
fn key_017_every_action_the_map_binds_has_a_place_it_is_read() {
    use xirius_prefs::mousemap::ACTIONS;
    let mut routed: Vec<&str> = crate::mouse_dispatch::ROUTES
        .iter()
        .map(|(a, _)| *a)
        .collect();
    let mut bindable: Vec<&str> = ACTIONS.iter().map(|(a, _)| *a).collect();
    routed.sort_unstable();
    bindable.sort_unstable();
    assert_eq!(routed, bindable);
}

#[test]
fn key_017_cc_draw_rows_set_the_pencils_modifiers() {
    let mut s = Studio::sample();
    s.song
        .load_settings(&std::env::temp_dir().join("xirius-key017-none"));
    let mut mm = xirius_prefs::Mousemap::default();
    mm.set("lane.draw.empty.drag", "Shift", "cc.draw").unwrap();
    mm.set("lane.draw.empty.drag", "Alt", "cc.draw --line")
        .unwrap();
    let c = "lane.draw.empty.drag";
    let shift = crate::mouse_dispatch::modifiers(&mm, c, Mods::shift(), false);
    assert!(!shift.shift && !shift.alt);
    let alt = crate::mouse_dispatch::modifiers(&mm, c, Mods::alt(), false);
    assert!(alt.alt && !alt.shift);
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("xirius-key017-none"));
}

#[test]
fn key_016_alt_click_selects_the_clips_after_the_pointer_and_alt_drag_still_copies() {
    let (mut s, g) = setup();
    let steps = s.song.steps();
    let at = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
    let tick = s.px_to_ticks(at.0 - g.lanes.x + s.scroll.0);
    let after: usize = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| c.start >= tick)
        .count();
    // As it is: the default row of the map.
    s.arrange.clips.clear();
    s.arrange_down(&g, at.0, at.1, Mods::alt());
    s.arrange_up(&g, at.0, at.1, Mods::alt());
    let (t, c) = s.selected_clip.unwrap();
    assert_eq!(t, crate::arrange::probe::track(&s, "Keys"));
    assert!(s.arrange.clips.contains(&(t, c)));
    // The clip under the pointer starts before the pointer: it comes along with what follows.
    let under = s.project().tracks[&t]
        .clips
        .iter()
        .find(|x| x.id == c)
        .unwrap()
        .start;
    assert!(under < tick);
    assert_eq!(s.arrange.clips.len(), after + 1);
    assert!(s.arrange.clips.iter().all(|(tr, cl)| {
        let start = s.project().tracks[tr]
            .clips
            .iter()
            .find(|x| x.id == *cl)
            .unwrap()
            .start;
        start >= tick || (*tr, *cl) == (t, c)
    }));
    assert_eq!(s.song.steps(), steps, "selecting never edits");
}

#[test]
fn key_016_alt_click_on_the_empty_lane_selects_what_follows_and_a_changed_row_is_kept() {
    let (mut s, g) = setup();
    map(&mut s, "arrange.select.empty.click", "Alt", "select.after");
    let steps = s.song.steps();
    let last = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start)
        .max()
        .unwrap();
    // Before every clip: everything is picked.
    let first = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .map(|c| c.start)
        .min()
        .unwrap();
    let total: usize = s.project().tracks.values().map(|t| t.clips.len()).sum();
    let at = crate::arrange::probe::clip_mid(&s, &g, "Keys", 0);
    let space = (s.tick_x(&g, first) - 1.0, at.1);
    if first > 0 && space.0 > g.lanes.x + 1.0 {
        s.arrange_down(&g, space.0, space.1, Mods::alt());
        s.arrange_up(&g, space.0, space.1, Mods::alt());
        assert_eq!(s.arrange.clips.len(), total);
    }
    assert!(last >= first);
    assert_eq!(s.song.steps(), steps);
    // Taken away on purpose: the click does what the plain click does.
    map(&mut s, "arrange.select.clip-body.click", "Alt", "");
    s.arrange.clips.clear();
    s.arrange_down(&g, at.0, at.1, Mods::alt());
    s.arrange_up(&g, at.0, at.1, Mods::alt());
    assert!(s.arrange.clips.len() <= 1);
    // Alt + drag on a clip is the copy still.
    map(
        &mut s,
        "arrange.select.clip-body.click",
        "Alt",
        "select.after",
    );
    let before: usize = s.project().tracks.values().map(|t| t.clips.len()).sum();
    crate::arrange::probe::hold(&mut s, &g, at, (at.0 + 80.0, at.1), Mods::alt());
    s.arrange_up(&g, at.0 + 80.0, at.1, Mods::alt());
    let now: usize = s.project().tracks.values().map(|t| t.clips.len()).sum();
    assert_eq!(now, before + 1, "the Alt drag duplicates");
}

#[test]
fn key_020_the_roll_grid_and_the_automation_settings_reach_the_editors() {
    let (mut s, _g) = setup();
    assert!(s.song.session().state.view.roll.snap);
    assert!(s.arrange.locks_clips && s.arrange.follows_clips);
    for key in [
        "edit.snap.roll.grid",
        "edit.automation.locks-clips",
        "edit.automation.follows-clips",
    ] {
        s.song
            .run("prefs.set", json!({"key": key, "value": "false"}))
            .unwrap();
    }
    s.arrange.focus = false;
    assert!(s.apply_edit_prefs());
    assert!(!s.song.session().state.view.roll.snap);
    assert!(!s.arrange.locks_clips);
    assert!(!s.arrange.follows_clips);
}
