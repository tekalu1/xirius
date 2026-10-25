use crate::{
    studio::{Geometry, Studio},
    tools::{Mods, Tool},
};
use serde_json::json;

fn roll() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    let clip = s.selected_clip.unwrap().1;
    s.song
        .run("note.delete", json!({"ids":[clip.to_string()]}))
        .unwrap();
    s.song
        .run(
            "note.put",
            json!({"clip":clip.to_string(),"notes":[
                {"at":960,"pitch":60,"length":960,"velocity":70},
                {"at":1920,"pitch":60,"length":960,"velocity":90}
            ]}),
        )
        .unwrap();
    s.resized(&g);
    s.roll.center(&s.roll_ctx(&g).unwrap());
    (s, g)
}

#[test]
fn edt_021_restoration_uses_the_binding_and_can_be_remapped() {
    let (mut s, g) = roll();
    let id = s.roll_ctx(&g).unwrap().notes()[0].id;
    s.song
        .run(
            "note.set",
            json!({"notes":[{"id":id.to_string(),"start":977}]}),
        )
        .unwrap();
    s.roll.selected.insert(id);
    assert!(s.edit_key(&g, "q", Mods::NONE));
    assert_eq!(s.roll_ctx(&g).unwrap().note(id).unwrap().start, 960);
    s.song
        .run(
            "keymap.set",
            json!({"key":"Shift+Q","context":"roll","command":"tool.draw","replace":true}),
        )
        .unwrap();
    s.song
        .run(
            "keymap.set",
            json!({"key":"F9","context":"roll","command":"note.quantize --restore"}),
        )
        .unwrap();
    assert!(s.edit_key(&g, "q", Mods::shift()));
    assert_eq!(s.tool, Tool::Draw);
    assert_eq!(s.roll_ctx(&g).unwrap().note(id).unwrap().start, 960);
    assert!(s.edit_key(&g, "f9", Mods::NONE));
    assert_eq!(s.roll_ctx(&g).unwrap().note(id).unwrap().start, 977);
}

#[test]
fn edt_001_052_g_joins_in_the_focused_editor_and_retains_selection() {
    let (mut s, g) = roll();
    s.roll.selected = s
        .roll_ctx(&g)
        .unwrap()
        .notes()
        .iter()
        .map(|n| n.id)
        .collect();
    assert!(s.edit_key(&g, "g", Mods::NONE));
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 1);
    assert_eq!(s.roll.selected.len(), 1);
    assert!(s.song.undo());
    let (t, c) = s.selected_clip.unwrap();
    let part = s.project().clip(c).unwrap().1.clone();
    s.song
        .run(
            "clip.split",
            json!({"ids":[c.to_string()],"at":part.start+part.length/2}),
        )
        .unwrap();
    s.arrange.focus = true;
    s.arrange.clips = s.project().tracks[&t]
        .clips
        .iter()
        .map(|c| (t, c.id))
        .collect();
    assert!(s.edit_key(&g, "g", Mods::NONE));
    assert_eq!(s.project().tracks[&t].clips.len(), 1);
    assert_eq!(s.arrange.clips.len(), 1);
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn edt_072_focus_changes_keep_the_snap_toggle_and_grid_modes() {
    let (mut s, g) = roll();
    assert!(s.edit_key(&g, "n", Mods::NONE));
    assert!(!s.song.snap_for("roll"));
    s.set_grid_mode(crate::tools::SnapMode::Frames);
    s.arrange.focus = true;
    s.apply_edit_prefs();
    assert!(s.song.snap());
    s.set_grid_mode(crate::tools::SnapMode::Bar);
    s.arrange.focus = false;
    s.apply_edit_prefs();
    assert!(!s.song.snap());
    assert_eq!(s.grid_mode(), crate::tools::SnapMode::Frames);
    assert_eq!(s.snap_mode, crate::tools::SnapMode::Bar);
    s.song
        .run(
            "prefs.set",
            json!({"key":"edit.snap.roll.mode","value":"quantize"}),
        )
        .unwrap();
    s.apply_edit_prefs();
    assert_eq!(s.grid_mode(), crate::tools::SnapMode::Quantize);
    assert_eq!(s.snap_mode, crate::tools::SnapMode::Bar);
    assert!(!s.song.snap());
}

#[test]
fn edt_033_shift_double_click_on_empty_lane_selects_only_its_track() {
    let (mut s, _) = roll();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    let (t, c) = s.selected_clip.unwrap();
    s.song
        .run("clip.duplicate", json!({"ids":[c.to_string()]}))
        .unwrap();
    let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let p = (s.tick_x(&g, 3840), s.row_y(&g, row) + row.h / 2.0);
    let before = s.song.steps();
    assert!(s.arrange_open(&g, p.0, p.1, Mods::shift()));
    let clips = &s.project().tracks[&t].clips;
    assert_eq!(s.arrange.clips, clips.iter().map(|c| (t, c.id)).collect());
    assert_eq!(s.arrange.clips.len(), 2);
    assert_eq!(s.song.steps(), before);
    assert!(!s.drawer_open);
}

#[test]
fn edt_029_alternative_tool_is_temporary_and_uses_the_mousemap() {
    let (mut s, _) = roll();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    let (t, _) = s.selected_clip.unwrap();
    let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let p = (s.tick_x(&g, 3840), s.row_y(&g, row) + row.h / 2.0);
    let before = s.song.steps();
    s.arrange_down(&g, p.0, p.1, Mods::ctrl());
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Range);
    s.arrange.drag = None;
    s.song
        .run(
            "mousemap.set",
            json!({"context":"arrange.select.hold","modifier":"Mod","action":"tool:draw"}),
        )
        .unwrap();
    s.arrange_down(&g, p.0, p.1, Mods::ctrl());
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Draw);
    assert_eq!(s.tool, Tool::Select);
    s.arrange.drag = None;
    s.arrange_down(&g, p.0, p.1, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
    assert_eq!(s.song.steps(), before);
}

#[test]
fn edt_030_link_toggle_routes_the_upper_and_lower_clip_halves() {
    let (mut s, _) = roll();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    let (t, c) = s.selected_clip.unwrap();
    let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, row, s.project().clip(c).unwrap().1);
    let p = (r.x + r.w / 2.0, r.y + r.h / 4.0);
    s.run_command("view.link", json!({"on":true}));
    s.arrange_down(&g, p.0, p.1, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Range);
    s.arrange.drag = None;
    s.arrange_down(&g, p.0, r.y + r.h * 0.7, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
    s.arrange.drag = None;
    s.run_command("view.link", json!({"off":true}));
    s.arrange_down(&g, p.0, p.1, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
}

#[test]
fn edt_043_shift_adds_a_second_range_and_click_keeps_the_playhead() {
    let (mut s, _) = roll();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    let (t, _) = s.selected_clip.unwrap();
    let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
    let y = s.row_y(&g, row) + row.h / 2.0;
    s.tool = Tool::Range;
    for (a, b, m) in [(1920, 2880, Mods::NONE), (4800, 5760, Mods::shift())] {
        let (x, z) = (s.tick_x(&g, a), s.tick_x(&g, b));
        s.arrange_down(&g, x, y, m);
        s.arrange_move(&g, z, y, m);
        s.arrange_up(&g, z, y, m);
    }
    assert_eq!(s.arrange.ranges, [(1920, 2880), (4800, 5760)]);
    let x = s.tick_x(&g, 2400);
    s.arrange_down(&g, x, y, Mods::NONE);
    s.arrange_up(&g, x, y, Mods::NONE);
    assert_eq!(s.playhead, 2400);
    assert_eq!(s.arrange.ranges.len(), 2);
}

#[test]
fn edt_044_draw_click_and_forward_drag_create_one_undoable_part() {
    for length in [0, 1920] {
        let (mut s, _) = roll();
        s.drawer_open = false;
        let g = s.geometry(1440.0, 820.0);
        s.rebuild(&g);
        let (t, _) = s.selected_clip.unwrap();
        let row = s.tree().row_of(crate::tree::RowKind::Track(t)).unwrap();
        let y = s.row_y(&g, row) + row.h / 2.0;
        s.tool = Tool::Draw;
        let (a, b) = (s.tick_x(&g, 1920), s.tick_x(&g, 1920 + length));
        let before = s.song.steps();
        s.arrange_down(&g, a, y, Mods::NONE);
        s.arrange_move(&g, b, y, Mods::NONE);
        s.arrange_up(&g, b, y, Mods::NONE);
        let part = s
            .project()
            .clip(s.selected_clip.unwrap().1)
            .unwrap()
            .1
            .clone();
        assert_eq!(part.start, 1920);
        assert_eq!(
            part.length,
            if length == 0 {
                s.tools().snap.step(&s.meter())
            } else {
                length
            }
        );
        assert!(part.notes().is_empty());
        assert_eq!(s.song.steps(), before + 1);
        assert!(s.song.undo());
        assert!(s.project().clip(part.id).is_err());
    }
}

#[test]
fn edt_047_cursor_zones_follow_height_and_keep_the_edges() {
    use crate::roll::Cursor;
    let (mut s, g) = roll();
    s.roll.extended = true;
    for height in [24.0, 10.0] {
        s.roll.row_h = height;
        let ctx = s.roll_ctx(&g).unwrap();
        s.roll.center(&ctx);
        let v = s.roll.view(&ctx);
        let n = ctx.notes()[0];
        let x = v.x(n.start) + v.px(n.length) / 2.0;
        for (y, expected) in [
            (
                v.y(n.pitch) + 1.0,
                if height >= 12.0 {
                    Cursor::ResizeV
                } else {
                    Cursor::Move
                },
            ),
            (
                v.y(n.pitch) + height - 1.0,
                if height >= 12.0 {
                    Cursor::Split
                } else {
                    Cursor::Move
                },
            ),
            (v.y(n.pitch) + height / 2.0, Cursor::Move),
        ] {
            s.roll.moved(&ctx, x, y, Mods::NONE);
            assert_eq!(s.roll.cursor(Some(&ctx)), expected);
        }
        s.roll.moved(
            &ctx,
            v.x(n.start) + 1.0,
            v.y(n.pitch) + height / 2.0,
            Mods::NONE,
        );
        assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::ResizeH);
    }
}

#[test]
fn edt_055_double_click_creation_and_deletion_each_have_one_undo() {
    let (mut s, g) = roll();
    s.song.set_grid("1/8").unwrap();
    s.set_grid_mode(crate::tools::SnapMode::Quantize);
    let ctx = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&ctx);
    let p = (v.x(3360), v.y(62) + 7.0);
    let before = s.song.steps();
    let out = s.roll.down(&ctx, p.0, p.1, Mods::NONE, 2);
    s.apply_roll(&g, out);
    let ctx = s.roll_ctx(&g).unwrap();
    let made = ctx.notes().iter().find(|n| n.pitch == 62).unwrap();
    assert_eq!(made.length, 480);
    assert_eq!(s.song.steps(), before + 1);
    let v = s.roll.view(&ctx);
    let p = (v.x(made.start) + v.px(made.length) / 2.0, v.y(62) + 7.0);
    let out = s.roll.down(&ctx, p.0, p.1, Mods::NONE, 2);
    s.apply_roll(&g, out);
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 2);
    assert!(s.song.undo());
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 3);
    assert!(s.song.undo());
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 2);
}

#[test]
fn edt_064_drag_balloon_and_next_note_take_the_grabbed_velocity() {
    let (mut s, g) = roll();
    s.roll.extended = true;
    let ctx = s.roll_ctx(&g).unwrap();
    s.roll.selected = ctx.notes().iter().map(|n| n.id).collect();
    let v = s.roll.view(&ctx);
    let p = (v.x(2400), v.y(60) + 1.0);
    let out = s.roll.down(&ctx, p.0, p.1, Mods::NONE, 1);
    s.apply_roll(&g, out);
    let out = s.roll.moved(&ctx, p.0, p.1 - 10.0, Mods::NONE);
    s.apply_roll(&g, out);
    assert!(s.roll.preview.balloon.is_some());
    let out = s.roll.up(&ctx, p.0, p.1 - 10.0, Mods::NONE);
    s.apply_roll(&g, out);
    assert!(s.roll.preview.balloon.is_none());
    assert_eq!(s.roll.next_vel, 101);
    s.tool = Tool::Draw;
    let ctx = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&ctx);
    let p = (v.x(3360), v.y(62) + 7.0);
    let out = s.roll.down(&ctx, p.0, p.1, Mods::NONE, 1);
    s.apply_roll(&g, out);
    let out = s.roll.up(&ctx, p.0, p.1, Mods::NONE);
    s.apply_roll(&g, out);
    assert_eq!(
        s.roll_ctx(&g)
            .unwrap()
            .notes()
            .iter()
            .find(|n| n.pitch == 62)
            .unwrap()
            .velocity,
        101
    );
}

#[test]
fn edt_082_lane_fold_click_preserves_other_lanes_and_controller_values() {
    use crate::roll::{Lane, LaneKind};
    use xirius_core::model::CtrlKind;
    let (mut s, g) = roll();
    s.roll
        .lanes
        .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
    s.roll
        .lanes
        .push(Lane::new(LaneKind::Ctrl(CtrlKind::PitchBend)));
    let ctx = s.roll_ctx(&g).unwrap();
    let before = s.roll.frame(ctx.body);
    let song = (*s.project()).clone();
    let r = before.lanes[1].fold;
    let out = s
        .roll
        .down(&ctx, r.x + r.w / 2.0, r.y + r.h / 2.0, Mods::NONE, 1);
    s.apply_roll(&g, out);
    assert!(s.roll.lanes[1].folded);
    let after = s.roll.frame(ctx.body);
    for i in [0, 2] {
        assert_eq!(before.lanes[i].plot.h, after.lanes[i].plot.h);
    }
    assert!(!s.roll.lanes[0].folded && !s.roll.lanes[2].folded);
    assert_eq!(*s.project(), song);
}

#[test]
fn edt_083_eraser_key_and_remapped_modifier_delete_without_replacing_the_tool() {
    let (mut s, g) = roll();
    assert!(s.edit_key(&g, "4", Mods::NONE));
    assert_eq!(s.tool, Tool::Eraser);
    assert!(s.edit_key(&g, "1", Mods::NONE));
    s.song
        .run(
            "mousemap.set",
            json!({"context":"roll.select.hold","modifier":"Alt","action":"tool:erase"}),
        )
        .unwrap();
    let ctx = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&ctx);
    let p = (v.x(1440), v.y(60) + 7.0);
    let before = s.song.steps();
    let out = s.roll.down(&ctx, p.0, p.1, Mods::alt(), 1);
    s.apply_roll(&g, out);
    let out = s.roll.up(&ctx, p.0, p.1, Mods::alt());
    s.apply_roll(&g, out);
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 1);
    assert_eq!(s.tool, Tool::Select);
    assert_eq!(s.song.steps(), before + 1);
    assert!(s.song.undo());
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 2);
}

#[test]
fn edt_045_browser_press_drag_replaces_and_middle_drag_moves_only_the_view() {
    use crate::browser::{BrowserHit, RailTab};
    use crate::studio::Hit;
    let (mut s, _) = roll();
    s.drawer_open = false;
    let dir_tmp = tempfile::Builder::new()
        .prefix("xirius-edit3-replace-ui-")
        .tempdir()
        .unwrap();
    let dir = dir_tmp.path().join("edit3-replace-ui");
    crate::import_shots::make_place(&dir);
    s.browser.press_rail(RailTab::Files);
    s.browser.open = true;
    s.browser.add_place_fixture(&dir).unwrap();
    s.browser.toggle_dir(&dir.join("Loops").to_string_lossy());
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    let i = crate::import_shots::file_row(&s, &g, "loop_120bpm.wav").unwrap();
    let from = s
        .browser
        .center_of(&s.browser.layout(g.panel), BrowserHit::File(i))
        .unwrap();
    assert!(s.lib_grab(&g, Hit::Browser(BrowserHit::File(i)), from.0, from.1, 1));
    let t = crate::arrange::probe::track(&s, "Vocal");
    let old = s.project().tracks[&t].clips[0].clone();
    let p = crate::arrange::probe::clip_mid(&s, &g, "Vocal", 0);
    assert!(s.lib_move(p.0, p.1));
    assert!(s.replace_file_up(&g, p.0, p.1));
    let now = s.project().clip(old.id).unwrap().1.clone();
    assert_eq!(
        (now.id, now.start, now.length),
        (old.id, old.start, old.length)
    );
    assert!(s.song.undo());
    assert_eq!(s.project().clip(old.id).unwrap().1, &old);
    let before = (*s.project()).clone();
    let steps = s.song.steps();
    let p = (g.lanes.x + 400.0, g.lanes.y + 20.0);
    assert!(s.pan_begin(&g, p.0, p.1));
    assert!(s.pan_move(&g, p.0 - 120.0, p.1));
    assert!(s.pan_end());
    assert!(s.scroll.0 > 0.0);
    assert_eq!(*s.project(), before);
    assert_eq!(s.song.steps(), steps);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn edt_184_the_roll_tools_show_their_picture_on_a_note_and_not_allowed_between_notes() {
    use crate::pointer::{Shape, pointer};
    use crate::roll::Cursor;
    use gpui::CursorStyle;
    let (mut s, g) = roll();
    s.roll.center(&s.roll_ctx(&g).unwrap());
    let ctx = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&ctx);
    let n = ctx.notes()[0];
    let on_note = (
        v.x(n.start) + v.px(n.length) / 2.0,
        v.y(n.pitch) + s.roll.row_h / 2.0,
    );
    let between = (on_note.0, v.y(n.pitch + 3) + s.roll.row_h / 2.0);
    for (tool, shape, cursor) in [
        (Tool::Cut, Shape::Scissors, Cursor::Split),
        (Tool::Eraser, Shape::Eraser, Cursor::Erase),
        (Tool::Mute, Shape::Mute, Cursor::Mute),
        (Tool::Audition, Shape::Audition, Cursor::Audition),
    ] {
        s.tool = tool;
        let ctx = s.roll_ctx(&g).unwrap();
        s.roll.moved(&ctx, on_note.0, on_note.1, Mods::NONE);
        assert_eq!(s.roll.cursor(Some(&ctx)), cursor, "{tool:?} on a note");
        assert_eq!(pointer(cursor, false).shape, Some(shape));
        s.roll.moved(&ctx, between.0, between.1, Mods::NONE);
        let blocked = pointer(s.roll.cursor(Some(&ctx)), false);
        assert_eq!(
            blocked.style,
            CursorStyle::OperationNotAllowed,
            "{tool:?} between"
        );
        assert_eq!(blocked.shape, None);
    }
    // The pencil and the range tool need no note under them.
    s.tool = Tool::Draw;
    let ctx = s.roll_ctx(&g).unwrap();
    s.roll.moved(&ctx, between.0, between.1, Mods::NONE);
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Pencil);
    s.tool = Tool::Range;
    let ctx = s.roll_ctx(&g).unwrap();
    s.roll.moved(&ctx, between.0, between.1, Mods::NONE);
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Range);
    // Ctrl held with the arrow tool is the pencil by default; Ctrl+Shift is the cut tool.
    s.tool = Tool::Select;
    let ctx = s.roll_ctx(&g).unwrap();
    s.roll.moved(&ctx, on_note.0, on_note.1, Mods::ctrl());
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Pencil);
    s.roll.moved(&ctx, between.0, between.1, Mods::ctrl());
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Pencil);
    s.roll.moved(&ctx, on_note.0, on_note.1, Mods::ctrl_shift());
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Split);
    s.roll.moved(&ctx, between.0, between.1, Mods::ctrl_shift());
    assert_eq!(s.roll.cursor(Some(&ctx)), Cursor::Blocked);
}
