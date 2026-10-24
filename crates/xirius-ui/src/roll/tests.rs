use super::*;
use crate::roll::notes::WHEEL_NOTCH;
use crate::studio::{Geometry, Studio};
use crate::tools::{Area, Mods, SnapMode, Tool, effective_tool};
use xirius_core::model::CtrlKind;

fn setup() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    let g = s.geometry(1440.0, 820.0);
    let ctx = s.roll_ctx(&g).unwrap();
    s.song
        .run(
            "note.delete",
            json!({"ids":ids_json(ctx.notes().iter().map(|n| &n.id))}),
        )
        .unwrap();
    s.song
        .run(
            "note.put",
            json!({"clip":ctx.clip.id.to_string(),"notes":[
                {"at":960,"pitch":60,"length":960,"velocity":80},
                {"at":2880,"pitch":62,"length":1440,"velocity":90},
                {"at":4800,"pitch":64,"length":480,"velocity":100}
            ]}),
        )
        .unwrap();
    s.resized(&g);
    (s, g)
}
fn context(s: &Studio, g: &Geometry) -> Ctx {
    s.roll_ctx(g).unwrap()
}
fn at(s: &Studio, g: &Geometry, i: usize, zone: Zone) -> (f32, f32) {
    let c = context(s, g);
    let v = s.roll.view(&c);
    let r = super::notes::note_rect(&v, &NoteView::from(&c.notes()[i]));
    match zone {
        Zone::Body => (r.x + r.w / 2.0, r.y + r.h / 2.0),
        Zone::Velocity => (r.x + r.w / 2.0, r.y + 1.0),
        Zone::Split => (r.x + r.w / 2.0, r.bottom() - 1.0),
        Zone::EdgeL => (r.x + 1.0, r.y + r.h / 2.0),
        Zone::EdgeR => (r.right() - 1.0, r.y + r.h / 2.0),
    }
}
fn down(s: &mut Studio, g: &Geometry, p: (f32, f32), m: Mods) {
    let c = context(s, g);
    let out = s.roll.down(&c, p.0, p.1, m, 1);
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn moved(s: &mut Studio, g: &Geometry, p: (f32, f32), m: Mods) {
    let c = context(s, g);
    let out = s.roll.moved(&c, p.0, p.1, m);
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn up(s: &mut Studio, g: &Geometry, p: (f32, f32), m: Mods) {
    let c = context(s, g);
    let out = s.roll.up(&c, p.0, p.1, m);
    assert!(out.edits.len() <= 1);
    s.apply_roll(g, out);
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn key(s: &mut Studio, g: &Geometry, k: &str, m: Mods) {
    assert!(s.edit_key(g, k, m), "{k}");
    assert!(s.error.is_none(), "{:?}", s.error);
}
fn drag(s: &mut Studio, g: &Geometry, p: (f32, f32), d: (f32, f32), m: Mods) {
    down(s, g, p, m);
    let end = (p.0 + d.0, p.1 + d.1);
    moved(s, g, end, m);
    up(s, g, end, m);
}

#[test]
fn feel_01_number_keys_and_temporary_tools() {
    let (mut s, g) = setup();
    for t in Tool::ALL {
        key(&mut s, &g, &t.digit().to_string(), Mods::NONE);
        assert_eq!(s.tool, t);
    }
    key(&mut s, &g, "1", Mods::NONE);
    assert_eq!(effective_tool(Area::Roll, s.tool, Mods::ctrl()), Tool::Draw);
    assert_eq!(
        effective_tool(Area::Roll, s.tool, Mods::ctrl_shift()),
        Tool::Cut
    );
    assert_eq!(effective_tool(Area::Roll, s.tool, Mods::NONE), Tool::Select);
    assert_eq!(
        effective_tool(Area::Roll, Tool::Draw, Mods::ctrl()),
        Tool::Select
    );
}

#[test]
fn feel_03_copy_tip_and_undo_one_history_step() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let p = at(&s, &g, 0, Zone::Body);
    let steps = s.song.steps();
    down(&mut s, &g, p, Mods::alt());
    moved(&mut s, &g, (p.0 + 60.0, p.1), Mods::alt());
    assert_eq!(s.song.steps(), steps);
    assert!(s.roll.preview.balloon.as_ref().unwrap().text.contains('＋'));
    assert_eq!(context(&s, &g).notes(), c.notes());
    up(&mut s, &g, (p.0 + 60.0, p.1), Mods::alt());
    let after = context(&s, &g);
    assert_eq!(after.notes().len(), 4);
    assert_eq!(&after.notes()[0], &c.notes()[0]);
    assert_eq!(s.roll.selected.len(), 1);
    assert_eq!(s.song.steps(), steps + 1);
    key(&mut s, &g, "z", Mods::ctrl());
    assert_eq!(context(&s, &g).notes(), c.notes());
    key(&mut s, &g, "y", Mods::ctrl());
    assert_eq!(context(&s, &g).notes().len(), 4);
}

#[test]
fn feel_04_shift_snap_and_modifier_changes_without_pointer_motion() {
    let (mut s, g) = setup();
    let p = at(&s, &g, 0, Zone::Body);
    let id = context(&s, &g).notes()[0].id;
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 + 11.0, p.1), Mods::NONE);
    assert_eq!(s.roll.preview.changed[&id].start % 240, 0);
    let c = context(&s, &g);
    s.roll.mods_changed(&c, Mods::shift());
    assert_ne!(s.roll.preview.changed[&id].start % 240, 0);
    s.roll.mods_changed(&c, Mods::alt_shift());
    assert_eq!(s.roll.preview.ghosts.len(), 1);
    assert!(s.roll.preview.changed.is_empty());
    s.roll.mods_changed(&c, Mods::NONE);
    assert!(s.roll.preview.ghosts.is_empty());
    assert_eq!(s.roll.drag.as_ref().unwrap().mode, Mode::Move);
    s.roll.mods_changed(&c, Mods::ctrl());
    assert_eq!(s.roll.drag.as_ref().unwrap().mode, Mode::Move);
    s.roll.cancel();
    assert_eq!(context(&s, &g).notes()[0].start, 960);
}

#[test]
fn feel_05_hit_regions_cursors_tips_and_resize() {
    for (zone, cursor, delta, text) in [
        (Zone::Body, Cursor::Move, (24.0, -14.0), " · "),
        (Zone::EdgeL, Cursor::ResizeH, (12.0, 0.0), " · "),
        (Zone::EdgeR, Cursor::ResizeH, (24.0, 0.0), "長さ"),
        (Zone::Velocity, Cursor::ResizeV, (0.0, -20.0), "ベロシティ"),
    ] {
        let (mut s, g) = setup();
        s.roll.extended = true;
        let c = context(&s, &g);
        let old = c.notes()[0];
        let p = at(&s, &g, 0, zone);
        moved(&mut s, &g, p, Mods::NONE);
        assert_eq!(s.roll.cursor(Some(&c)), cursor);
        down(&mut s, &g, p, Mods::NONE);
        let end = (p.0 + delta.0, p.1 + delta.1);
        moved(&mut s, &g, end, Mods::NONE);
        assert!(s.roll.preview.balloon.as_ref().unwrap().text.contains(text));
        up(&mut s, &g, end, Mods::NONE);
        let now = context(&s, &g).notes()[0];
        match zone {
            Zone::Body => assert_eq!(now.pitch, old.pitch + 1),
            Zone::EdgeL => {
                assert!(now.start > old.start);
                assert_eq!(now.start + now.length, old.start + old.length);
            }
            Zone::EdgeR => assert!(now.length > old.length),
            _ => assert!(now.velocity > old.velocity),
        }
        key(&mut s, &g, "z", Mods::ctrl());
        assert_eq!(context(&s, &g).notes()[0], old);
    }
}

#[test]
fn feel_05b_lower_edge_and_ctrl_shift_cut() {
    for mods in [Mods::NONE, Mods::ctrl_shift()] {
        let (mut s, g) = setup();
        s.roll.extended = true;
        let p = at(&s, &g, 0, if mods.ctrl { Zone::Body } else { Zone::Split });
        down(&mut s, &g, p, mods);
        up(&mut s, &g, p, mods);
        assert_eq!(context(&s, &g).notes().len(), 4);
        key(&mut s, &g, "z", Mods::ctrl());
        assert_eq!(context(&s, &g).notes().len(), 3);
    }
}

#[test]
fn the_arrow_is_basic_by_default_and_the_extended_places_show_their_cursor_and_a_band() {
    let (mut s, g) = setup();
    assert!(!s.roll.extended, "the arrow starts basic (2026-10-21)");
    let top = at(&s, &g, 0, Zone::Velocity);
    moved(&mut s, &g, top, Mods::NONE);
    let c = context(&s, &g);
    assert_eq!(s.roll.cursor(Some(&c)), Cursor::Move);
    assert!(s.roll.zone_band(&c).is_none(), "no band in the basic mode");

    s.roll.extended = true;
    let r = super::notes::note_rect(&s.roll.view(&c), &NoteView::from(&c.notes()[0]));
    for (zone, cursor) in [
        (Zone::Velocity, Cursor::ResizeV),
        (Zone::Split, Cursor::Split),
    ] {
        let p = at(&s, &g, 0, zone);
        moved(&mut s, &g, p, Mods::NONE);
        let c = context(&s, &g);
        assert_eq!(s.roll.cursor(Some(&c)), cursor);
        let (on, band) = s.roll.zone_band(&c).expect("a band on the edge");
        assert_eq!(on, zone);
        assert_eq!(band.h, 4.0);
        assert!(
            band.x > r.x && band.right() < r.right(),
            "between the two edge places"
        );
        match zone {
            Zone::Velocity => assert_eq!(band.y, r.y + 1.0),
            _ => assert_eq!(band.bottom(), r.bottom() - 1.0),
        }
    }
    let middle = at(&s, &g, 0, Zone::Body);
    moved(&mut s, &g, middle, Mods::NONE);
    let c = context(&s, &g);
    assert_eq!(s.roll.cursor(Some(&c)), Cursor::Move);
    assert!(
        s.roll.zone_band(&c).is_none(),
        "the middle of the note has no band"
    );

    let top = at(&s, &g, 0, Zone::Velocity);
    moved(&mut s, &g, top, Mods::NONE);
    down(&mut s, &g, top, Mods::NONE);
    assert!(
        s.roll.zone_band(&context(&s, &g)).is_none(),
        "the band goes while dragging"
    );
}

#[test]
fn feel_06_draw_click_drag_erase_and_line() {
    let (mut s, g) = setup();
    key(&mut s, &g, "5", Mods::NONE);
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let p = (v.x(7000), v.y(61) + 7.0);
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.preview.ghosts[0].length, 240);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), 4);
    let p = (p.0 + 50.0, p.1);
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 + 100.0, p.1 - 10.0), Mods::NONE);
    let tip = &s.roll.preview.balloon.as_ref().unwrap().text;
    assert!(tip.contains("長さ") && tip.contains("ベロシティ"));
    up(&mut s, &g, (p.0 + 100.0, p.1 - 10.0), Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), 5);
    assert!(context(&s, &g).notes().last().unwrap().length > 240);
    let p = at(&s, &g, 0, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), 4);
    let p = (v.x(11000), v.y(61) + 7.0);
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 + 96.0, p.1 - 28.0), Mods::alt());
    assert!(s.roll.preview.balloon.as_ref().unwrap().text.contains('個'));
    let count = s.roll.preview.ghosts.len();
    assert!(count > 2);
    let steps = s.song.steps();
    up(&mut s, &g, (p.0 + 96.0, p.1 - 28.0), Mods::alt());
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(context(&s, &g).notes().len(), 4 + count);
}

#[test]
fn feel_07_keys_transpose_nudge_mute_duplicate_delete() {
    let (mut s, g) = setup();
    key(&mut s, &g, "a", Mods::ctrl());
    let old = context(&s, &g).notes().to_vec();
    key(&mut s, &g, "up", Mods::alt());
    assert_eq!(context(&s, &g).notes()[0].pitch, 61);
    key(&mut s, &g, "up", Mods::alt_shift());
    assert_eq!(context(&s, &g).notes()[0].pitch, 73);
    key(&mut s, &g, "down", Mods::alt_shift());
    key(&mut s, &g, "down", Mods::alt());
    assert_eq!(context(&s, &g).notes()[0].pitch, 60);
    key(&mut s, &g, "right", Mods::alt());
    assert_eq!(context(&s, &g).notes()[0].start, 1200);
    key(&mut s, &g, "left", Mods::alt());
    assert_eq!(context(&s, &g).notes()[0].start, 960);
    key(&mut s, &g, "m", Mods::shift());
    assert!(context(&s, &g).notes().iter().all(|n| n.muted));
    key(&mut s, &g, "m", Mods::shift());
    assert!(context(&s, &g).notes().iter().all(|n| !n.muted));
    key(&mut s, &g, "d", Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), 6);
    key(&mut s, &g, "z", Mods::ctrl());
    key(&mut s, &g, "a", Mods::ctrl());
    key(&mut s, &g, "delete", Mods::NONE);
    assert!(context(&s, &g).notes().is_empty());
    key(&mut s, &g, "z", Mods::ctrl());
    assert_eq!(context(&s, &g).notes(), old);
}

#[test]
fn quantize_full_and_half() {
    for (mods, want) in [(Mods::NONE, 960), (Mods::alt(), 1010)] {
        let (mut s, g) = setup();
        let c = context(&s, &g);
        let id = c.notes()[0].id;
        s.song
            .run(
                "note.set",
                json!({"notes":[{"id":id.to_string(),"start":1060}]}),
            )
            .unwrap();
        s.roll.selected.insert(id);
        key(&mut s, &g, "q", mods);
        assert_eq!(context(&s, &g).notes()[0].start, want);
    }
}

#[test]
fn mod_g_splits_the_selected_notes_on_the_grid_and_mod_alt_digits_pick_a_quantize_program() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let first = c.notes()[0];
    s.roll.selected.insert(first.id);
    key(&mut s, &g, "g", Mods::ctrl());
    let c = context(&s, &g);
    let pieces: Vec<_> = c
        .notes()
        .iter()
        .filter(|n| n.pitch == first.pitch)
        .collect();
    assert!(pieces.len() > 1, "the note was cut on the grid");
    assert_eq!(pieces.iter().map(|n| n.length).sum::<i64>(), first.length);
    assert_eq!(c.notes().len(), 2 + pieces.len(), "the others stay whole");

    let both = Mods {
        ctrl: true,
        alt: true,
        ..Mods::NONE
    };
    key(&mut s, &g, "7", both);
    let view = &s.song.session().state.view;
    assert_eq!(view.quantize_program.as_deref(), Some("G"));
    assert_eq!(s.quant.swing, 50, "the panel shows the program");
    key(&mut s, &g, "1", both);
    assert_eq!(
        s.song.session().state.view.quantize_program.as_deref(),
        Some("A")
    );
    assert_eq!(s.quant.swing, 0);
}

#[test]
fn rectangle_selects_intersecting_notes() {
    let (mut s, g) = setup();
    key(&mut s, &g, "2", Mods::NONE);
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    drag(
        &mut s,
        &g,
        (v.x(700), v.y(65)),
        (v.px(4900), v.row_h * 6.0),
        Mods::NONE,
    );
    assert_eq!(s.roll.selected.len(), 3);
}

#[test]
fn feel_09_grid_lengths_lines_and_modes() {
    let (mut s, g) = setup();
    // The grid value is the step in クオンタイズ mode (自動 follows the zoom).
    s.set_grid_mode(SnapMode::Quantize);
    for d in [4, 8, 16, 32, 64] {
        for triplet in [false, true] {
            let grid = format!("1/{d}{}", if triplet { "t" } else { "" });
            s.song.set_grid(&grid).unwrap();
            let c = context(&s, &g);
            let lines = paint::grid_lines(&s.roll, &c);
            let ticks = xirius_core::time::parse_grid(&grid, c.project.ppq).unwrap();
            let m = c.meter(s.roll.beat_px);
            let v = s.roll.view(&c);
            // No two lines closer than their tiers' threshold.
            for w in lines.windows(2) {
                let px = m.px((w[1].tick - w[0].tick) as f64);
                let min = w[0].tier.min_px().max(w[1].tier.min_px());
                assert!(px + 0.01 >= min, "{grid}: {w:?}");
            }
            // Every point the button says the lines show has one, from each bar's
            // start: the grid's own points, or those of the value it adds.
            let start = c.clip.start;
            let (first, last) = (v.tick(v.grid.x) + start, v.tick(v.grid.right()) + start);
            let every =
                crate::grid_lines::visible(&c.tools.snap, &m, first).map_or(ticks, |e| e as Tick);
            let have: Vec<_> = lines.iter().map(|l| l.tick + start).collect();
            let mut points = 0;
            for (bar, b0) in crate::ruler::bars_in(&m.time, first, last) {
                let b1 = m.time.bar_start(bar + 1);
                for t in (b0..b1)
                    .step_by(every as usize)
                    .filter(|t| (first..=last).contains(t))
                {
                    assert!(have.contains(&t), "{grid} shows {every}: {t}");
                    points += 1;
                }
            }
            assert!(points > 4, "{grid}: {points}");
            // The finest lines are the grid's own points.
            for l in lines
                .iter()
                .filter(|l| l.tier == xirius_design::GridTier::Fine)
            {
                let t = l.tick + start;
                assert_eq!(
                    (t - m.time.bar_start(m.time.to_bbt(t).0)) % ticks,
                    0,
                    "{grid}"
                );
            }
            s.tool = Tool::Draw;
            let p = (v.x(9000), v.y(61) + 7.0);
            down(&mut s, &g, p, Mods::NONE);
            assert_eq!(s.roll.preview.ghosts[0].length, c.step(s.roll.beat_px));
            s.roll.cancel();
        }
    }
    for mode in SnapMode::ALL {
        s.set_grid_mode(mode);
        let c = context(&s, &g);
        assert!(c.step(s.roll.beat_px) > 0);
    }
    s.song.set_snap(false).unwrap();
    assert!(context(&s, &g).snap(s.roll.beat_px, false).is_none());
}

#[test]
fn zoom_keeps_pointer_pitch_and_tick() {
    let (mut s, g) = setup();
    let mut c = context(&s, &g);
    c.body.w = 600.0;
    s.roll.scroll.0 = 200.0;
    let v = s.roll.view(&c);
    let x = v.grid.x + 150.0;
    let y = v.grid.y + 60.0;
    let pitch = (y - v.grid.y + s.roll.scroll.1) / s.roll.row_h;
    s.roll.wheel(&c, x, y, 0.0, 60.0, Mods::ctrl());
    assert!(((y - v.grid.y + s.roll.scroll.1) / s.roll.row_h - pitch).abs() < 0.001);
    let tick = s.roll.view(&c).tick_f(x);
    s.roll.wheel(&c, x, y, 0.0, 60.0, Mods::ctrl_shift());
    assert!((s.roll.view(&c).tick_f(x) - tick).abs() < 0.01);
}

#[test]
fn zoom_stops_at_the_limits_and_one_beat_can_fill_the_roll() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    let (x, y) = (f.grid.x + 100.0, f.grid.y + 60.0);
    for _ in 0..200 {
        s.roll.wheel(&c, x, y, 0.0, WHEEL_NOTCH, Mods::ctrl_shift());
    }
    assert_eq!(s.roll.beat_px, BEAT_PX_MAX);
    for _ in 0..200 {
        s.roll
            .wheel(&c, x, y, 0.0, -WHEEL_NOTCH, Mods::ctrl_shift());
    }
    assert_eq!(s.roll.beat_px, BEAT_PX_MIN);
    for _ in 0..200 {
        s.roll.wheel(&c, x, y, 0.0, WHEEL_NOTCH, Mods::ctrl());
    }
    assert_eq!(s.roll.row_h, ROW_H_MAX);
}

#[test]
fn feel_08_lane_add_select_tools_fold_and_restore() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    down(&mut s, &g, (f.plus.x + 5.0, f.plus.y + 5.0), Mods::NONE);
    let entries = s.roll.menu_entries();
    assert!(entries.len() > 2);
    let r = s
        .roll
        .menu_item_rect(s.roll.menu_rect(&f, entries.len()), 0);
    down(&mut s, &g, (r.x + 5.0, r.y + 5.0), Mods::NONE);
    assert_eq!(s.roll.lanes[1].kind, LaneKind::Ctrl(CtrlKind::PitchBend));
    let f = s.roll.frame(c.body);
    s.tool = Tool::Draw;
    assert_eq!(
        LaneTool::from_tools(s.tools(), Mods::shift()),
        LaneTool::Line
    );
    let lb = f.lanes[0];
    down(&mut s, &g, (lb.fold.x + 2.0, lb.fold.y + 2.0), Mods::NONE);
    assert!(s.roll.lanes[0].folded);
    assert!(!s.roll.lanes[1].folded);
    let f = s.roll.frame(c.body);
    let p = (f.fold.x + 5.0, f.fold.y + 5.0);
    down(&mut s, &g, p, Mods::NONE);
    assert!(s.roll.frame(c.body).lanes.is_empty());
    let f = s.roll.frame(c.body);
    down(&mut s, &g, (f.fold.x + 5.0, f.fold.y + 5.0), Mods::NONE);
    assert_eq!(s.roll.frame(c.body).lanes.len(), 2);
}

#[test]
fn the_roll_strip_switch_turns_following_on_and_off_beside_the_tools() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    assert!(s.follow.on, "the default follows");
    let tool = s.roll.lane_tool;
    let f = s.roll.frame(c.body);
    assert!(
        f.fold.right() < f.follow.x,
        "the switch sits after the fold control"
    );
    down(&mut s, &g, (f.follow.x + 5.0, f.follow.y + 5.0), Mods::NONE);
    assert!(!s.follow.on);
    assert_eq!(s.roll.lane_tool, tool, "the switch leaves the tool alone");
    let f = s.roll.frame(c.body);
    down(&mut s, &g, (f.follow.x + 5.0, f.follow.y + 5.0), Mods::NONE);
    assert!(s.follow.on);
    // Folded lanes hide the tools, but the switch stays where the eye looks for it.
    let f = s.roll.frame(c.body);
    down(&mut s, &g, (f.fold.x + 5.0, f.fold.y + 5.0), Mods::NONE);
    assert!(s.roll.lanes_folded);
    let f = s.roll.frame(c.body);
    assert!(f.follow.x > f.fold.x, "the switch sits after the chevron");
    down(&mut s, &g, (f.follow.x + 5.0, f.follow.y + 5.0), Mods::NONE);
    assert!(!s.follow.on);
    assert!(s.roll.lanes_folded);
}

#[test]
fn controller_pencil_line_select_and_undo() {
    for kind in [CtrlKind::PitchBend, CtrlKind::Cc(1), CtrlKind::Aftertouch] {
        for tool in [LaneTool::Pencil, LaneTool::Line] {
            let (mut s, g) = setup();
            s.roll.lanes.push(Lane::new(LaneKind::Ctrl(kind)));
            s.tool = Tool::Draw;
            let draw_mods = if tool == LaneTool::Line {
                Mods::shift()
            } else {
                Mods::NONE
            };
            let c = context(&s, &g);
            let lb = s.roll.frame(c.body).lanes[1];
            let p = (lb.plot.x + 60.0, lb.plot.y + 8.0);
            let steps = s.song.steps();
            down(&mut s, &g, p, draw_mods);
            moved(&mut s, &g, (p.0 + 100.0, p.1 + 15.0), draw_mods);
            assert_eq!(s.song.steps(), steps);
            up(&mut s, &g, (p.0 + 100.0, p.1 + 15.0), draw_mods);
            assert_eq!(s.song.steps(), steps + 1);
            let c = context(&s, &g);
            let pts = s.roll.song_points(&c, kind);
            assert!(pts.len() >= 2);
            s.tool = Tool::Select;
            let v = s.roll.view(&c);
            let p = (
                v.x(pts[0].tick),
                super::lanes::y_of(LaneKind::Ctrl(kind), &lb.plot, pts[0].value),
            );
            down(&mut s, &g, p, Mods::NONE);
            moved(&mut s, &g, (p.0, p.1 + 5.0), Mods::ctrl());
            up(&mut s, &g, (p.0, p.1 + 5.0), Mods::ctrl());
            let now = s.roll.song_points(&context(&s, &g), kind);
            assert_eq!(now[0].tick, pts[0].tick);
            assert_ne!(now[0].value, pts[0].value);
            key(&mut s, &g, "z", Mods::ctrl());
            assert_eq!(s.roll.song_points(&context(&s, &g), kind), pts);
            key(&mut s, &g, "z", Mods::ctrl());
            assert!(s.roll.song_points(&context(&s, &g), kind).is_empty());
        }
    }
}

/// The points the pencil left in the CC1 lane after a drag with the shape `name`.
fn shaped_lane(name: &str, m: Mods, dy: f32) -> (usize, usize, Vec<i32>) {
    let (mut s, g) = setup();
    s.roll
        .lanes
        .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
    s.tool = Tool::Draw;
    s.song
        .set("tool.draw.shape", serde_json::json!({ "shape": name }))
        .unwrap();
    let c = context(&s, &g);
    let lb = s.roll.frame(c.body).lanes[1];
    let p = (lb.plot.x + 40.0, lb.plot.y + lb.plot.h * 0.5);
    let steps = s.song.steps();
    down(&mut s, &g, p, m);
    moved(&mut s, &g, (p.0 + 60.0, p.1 + dy * 0.5), m);
    moved(&mut s, &g, (p.0 + 180.0, p.1 + dy), m);
    up(&mut s, &g, (p.0 + 180.0, p.1 + dy), m);
    let pts = s.roll.song_points(&context(&s, &g), CtrlKind::Cc(1));
    let values = pts.iter().map(|p| p.value).collect();
    let span = pts.last().map_or(0, |p| p.tick) - pts.first().map_or(0, |p| p.tick);
    (
        s.song.steps() - steps,
        usize::try_from(span).unwrap_or(0),
        values,
    )
}

#[test]
fn the_controller_pencil_lays_the_shape_the_top_bar_chose_in_one_step() {
    let (steps, _, line) = shaped_lane("line", Mods::NONE, -30.0);
    assert_eq!(steps, 1);
    assert_eq!(line.len(), 2, "a line is its two ends: {line:?}");
    assert!(line[1] > line[0], "dragging up raises it: {line:?}");

    let (steps, _, sine) = shaped_lane("sine", Mods::NONE, -30.0);
    assert_eq!(steps, 1);
    assert!(sine.len() > 4, "a wave is sampled: {sine:?}");
    let (hi, lo) = (sine.iter().max().unwrap(), sine.iter().min().unwrap());
    assert!(hi > lo, "and it swings: {sine:?}");

    let (_, _, free) = shaped_lane("free", Mods::NONE, -30.0);
    assert!(free.len() > 2, "freehand keeps its path: {free:?}");
}

#[test]
fn alt_makes_a_controller_wave_one_period_and_the_other_shapes_a_line() {
    let (_, _, parabola) = shaped_lane("parabola", Mods::alt(), -30.0);
    assert_eq!(parabola.len(), 2, "{parabola:?}");
    let (steps, _, wave) = shaped_lane("sine", Mods::alt(), -30.0);
    assert_eq!(steps, 1);
    let top = wave
        .iter()
        .position(|v| v == wave.iter().max().unwrap())
        .unwrap();
    assert!(
        top > 0 && top < wave.len() - 1,
        "one period peaks inside the stroke: {wave:?}"
    );
}

#[test]
fn the_velocity_pencil_gives_the_bars_the_height_of_the_shape_the_top_bar_chose() {
    let heights = |name: &str, m: Mods| {
        let (mut s, g) = setup();
        s.tool = Tool::Draw;
        s.song
            .set("tool.draw.shape", serde_json::json!({ "shape": name }))
            .unwrap();
        let c = context(&s, &g);
        let lb = s.roll.frame(c.body).lanes[0];
        let v = s.roll.view(&c);
        let p = (v.x(480), lb.plot.y + lb.plot.h * 0.5);
        let end = (v.x(5280), lb.plot.y + lb.plot.h * 0.2);
        let steps = s.song.steps();
        down(&mut s, &g, p, m);
        moved(&mut s, &g, ((p.0 + end.0) / 2.0, (p.1 + end.1) / 2.0), m);
        moved(&mut s, &g, end, m);
        up(&mut s, &g, end, m);
        let mut v: Vec<_> = context(&s, &g)
            .notes()
            .iter()
            .map(|n| (n.start, n.velocity))
            .collect();
        v.sort();
        (
            s.song.steps() - steps,
            v.iter().map(|x| x.1).collect::<Vec<_>>(),
        )
    };
    let (steps, line) = heights("line", Mods::NONE);
    assert_eq!(steps, 1);
    assert!(
        line.windows(2).all(|w| w[0] < w[1]),
        "a line climbs: {line:?}"
    );
    let (steps, wave) = heights("sine", Mods::alt());
    assert_eq!(steps, 1);
    assert!(
        wave.iter().any(|v| *v != wave[0]),
        "one period of a sine swings the bars: {wave:?}"
    );
    let (_, parabola) = heights("parabola", Mods::NONE);
    assert!(parabola[0] < parabola[2], "a parabola climbs: {parabola:?}");
}

#[test]
fn alt_makes_the_velocity_pencil_draw_a_straight_line_between_the_press_and_the_release() {
    let (mut s, g) = setup();
    s.tool = Tool::Draw;
    s.song
        .set("tool.draw.shape", serde_json::json!({ "shape": "free" }))
        .unwrap();
    let c = context(&s, &g);
    let lb = s.roll.frame(c.body).lanes[0];
    let v = s.roll.view(&c);
    let p = (v.x(480), lb.plot.y + lb.plot.h * 0.8);
    let end = (v.x(5280), lb.plot.y + lb.plot.h * 0.2);
    let m = Mods::alt();
    down(&mut s, &g, p, m);
    // The pointer wanders on the way; the line ignores where.
    moved(&mut s, &g, ((p.0 + end.0) / 2.0, lb.plot.y + 1.0), m);
    moved(&mut s, &g, end, m);
    up(&mut s, &g, end, m);
    let mut bars: Vec<_> = context(&s, &g)
        .notes()
        .iter()
        .map(|n| (n.start, i32::from(n.velocity)))
        .collect();
    bars.sort();
    let (a, b, c) = (bars[0].1, bars[1].1, bars[2].1);
    assert!(a < b && b < c, "{bars:?}");
    assert!(
        (b - a - (c - b)).abs() <= 1,
        "evenly spaced bars climb evenly: {bars:?}"
    );
}

#[test]
fn velocity_lane_drag_is_one_command() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let lb = s.roll.frame(c.body).lanes[0];
    let v = s.roll.view(&c);
    let n = &c.notes()[0];
    let p = (
        v.x(n.start),
        super::lanes::y_of(LaneKind::Velocity, &lb.plot, i32::from(n.velocity)),
    );
    let steps = s.song.steps();
    drag(&mut s, &g, p, (0.0, -5.0), Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1);
    assert!(context(&s, &g).notes()[0].velocity > n.velocity);
}

#[test]
fn roll_starts_with_velocity() {
    assert_eq!(Roll::new().lanes[0].kind, LaneKind::Velocity);
}

#[test]
fn edt_053_modifiers_after_press_align_lengths_heads_and_ends() {
    for zone in [Zone::EdgeL, Zone::EdgeR] {
        let (mut s, g) = setup();
        key(&mut s, &g, "a", Mods::ctrl());
        let p = at(&s, &g, if zone == Zone::EdgeL { 0 } else { 2 }, zone);
        down(&mut s, &g, p, Mods::NONE);
        moved(&mut s, &g, (p.0 + 12.0, p.1), Mods::NONE);
        let c = context(&s, &g);
        s.roll.mods_changed(&c, Mods::ctrl());
        let length = s.roll.preview.changed.values().next().unwrap().length;
        assert!(s.roll.preview.changed.values().all(|n| n.length == length));
        s.roll.mods_changed(
            &c,
            Mods {
                ctrl: true,
                alt: true,
                ..Mods::NONE
            },
        );
        let values: Vec<_> = s
            .roll
            .preview
            .changed
            .values()
            .map(|n| {
                if zone == Zone::EdgeL {
                    n.start
                } else {
                    n.start + n.length
                }
            })
            .collect();
        assert!(values.iter().all(|n| *n == values[0]));
        let mode = s.roll.drag.as_ref().unwrap().mode;
        s.roll.mods_changed(&c, Mods::alt());
        assert_eq!(s.roll.drag.as_ref().unwrap().mode, mode);
        s.roll.mods_changed(&c, Mods::NONE);
        assert_eq!(s.roll.drag.as_ref().unwrap().mode, mode);
        let steps = s.song.steps();
        up(&mut s, &g, (p.0 + 12.0, p.1), Mods::ctrl());
        assert_eq!(s.song.steps(), steps + 1);
    }
}

#[test]
fn feel_10_menu_shortcuts_and_clipboard_keep_muting() {
    let (mut s, g) = setup();
    let p = at(&s, &g, 0, Zone::Body);
    assert!(s.scene_context(&g, p.0, p.1));
    let keys: Vec<_> = s.context_rows().into_iter().map(|r| r.key).collect();
    for key in ["D", "Shift+M", "Ctrl+X", "Ctrl+C", "Del / Backspace"] {
        assert!(keys.iter().any(|k| k == key), "{key}");
    }
    assert!(s.context_choose(&g, "ノートをミュート"));
    assert!(context(&s, &g).notes()[0].muted);
    key(&mut s, &g, "c", Mods::ctrl());
    let steps = s.song.steps();
    key(&mut s, &g, "v", Mods::ctrl());
    assert_eq!(s.song.steps(), steps + 1);
    let c = context(&s, &g);
    assert_eq!(c.notes().len(), 4);
    assert!(c.notes().last().unwrap().muted);
    key(&mut s, &g, "x", Mods::ctrl());
    assert_eq!(context(&s, &g).notes().len(), 3);
    key(&mut s, &g, "v", Mods::ctrl());
    assert_eq!(context(&s, &g).notes().len(), 4);
}

#[test]
fn keys_during_drag_do_not_overwrite_the_preview_or_add_history() {
    let (mut s, g) = setup();
    let p = at(&s, &g, 0, Zone::Body);
    let steps = s.song.steps();
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 + 24.0, p.1), Mods::NONE);
    key(&mut s, &g, "right", Mods::alt());
    assert_eq!(s.song.steps(), steps);
    key(&mut s, &g, "delete", Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), 3);
    up(&mut s, &g, (p.0 + 24.0, p.1), Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(context(&s, &g).notes()[0].start, 1440);
}

#[test]
fn controller_selection_cannot_cut_notes_or_replace_the_note_clipboard() {
    let (mut s, g) = setup();
    key(&mut s, &g, "a", Mods::ctrl());
    key(&mut s, &g, "c", Mods::ctrl());
    let steps = s.song.steps();
    s.roll.points.insert(960);
    key(&mut s, &g, "x", Mods::ctrl());
    assert_eq!(s.song.steps(), steps);
    assert_eq!(s.roll.clipboard.len(), 3);
    assert_eq!(context(&s, &g).notes().len(), 3);
    key(&mut s, &g, "v", Mods::ctrl());
    assert!(s.roll.points.is_empty());
    assert_eq!(context(&s, &g).notes().len(), 6);
}

#[test]
fn empty_menu_action_and_cursor_boundary_request_redraw() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    assert!(s.scene_context(&g, f.grid.x + 20.0, f.grid.y + 8.0));
    assert!(s.context_choose(&g, "グリッド"));
    assert!(s.context_choose(&g, "1/8"));
    assert!(!s.context_open());
    s.roll
        .moved(&c, f.grid.x + 20.0, f.grid.y + 8.0, Mods::NONE);
    let r = f.lanes[0].plot;
    let out = s.roll.moved(&c, r.x + 20.0, r.y + 8.0, Mods::NONE);
    assert!(out.redraw);
}

#[test]
fn changing_lane_by_head_folded_plot_or_tab_clears_point_selection() {
    for target in 0..3 {
        let (mut s, g) = setup();
        s.roll
            .lanes
            .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
        s.roll
            .lanes
            .push(Lane::new(LaneKind::Ctrl(CtrlKind::Aftertouch)));
        s.roll.active_lane = 1;
        s.roll.points.insert(960);
        if target == 1 {
            s.roll.lanes[2].folded = true;
        }
        let f = s.roll.frame(context(&s, &g).body);
        let r = match target {
            0 => f.lanes[2].head,
            1 => f.lanes[2].plot,
            _ => f.tabs[2],
        };
        down(&mut s, &g, (r.x + r.w / 2.0, r.y + r.h / 2.0), Mods::NONE);
        assert_eq!(s.roll.active_lane, 2);
        assert!(s.roll.points.is_empty());
    }
}

#[test]
fn velocity_line_edits_crossed_bars_and_select_moves_the_group() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    s.tool = Tool::Draw;
    drag(
        &mut s,
        &g,
        (v.x(960), f.lanes[0].plot.y + 8.0),
        (v.px(3840), 16.0),
        Mods::NONE,
    );
    let after = context(&s, &g);
    assert!(after.notes()[0].velocity > after.notes()[1].velocity);
    assert!(after.notes()[1].velocity > after.notes()[2].velocity);
    key(&mut s, &g, "a", Mods::ctrl());
    s.tool = Tool::Select;
    let p = (
        v.x(960),
        super::lanes::y_of(
            LaneKind::Velocity,
            &f.lanes[0].plot,
            i32::from(after.notes()[0].velocity),
        ),
    );
    drag(&mut s, &g, p, (0.0, 4.0), Mods::NONE);
    let now = context(&s, &g);
    assert!(
        now.notes()
            .iter()
            .zip(after.notes())
            .all(|(a, b)| a.velocity < b.velocity)
    );
}

#[test]
fn audition_is_transient_and_escape_never_changes_the_song() {
    let (mut s, g) = setup();
    let steps = s.song.steps();
    key(&mut s, &g, "8", Mods::NONE);
    let p = at(&s, &g, 0, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.audition, Some(60));
    moved(&mut s, &g, (p.0, p.1 - 14.0), Mods::NONE);
    assert_eq!(s.roll.audition, Some(61));
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.audition, None);
    assert_eq!(s.song.steps(), steps);
}

// ------------------------------------------------- the top bar and the roll

fn choose(s: &mut Studio, g: &Geometry, action: crate::popup::Action) {
    let i = s
        .popup
        .as_ref()
        .unwrap()
        .layout(g.size)
        .cells
        .iter()
        .position(|c| c.live() && c.action.as_ref() == Some(&action))
        .unwrap();
    assert!(s.press(g, crate::studio::Hit::Item(i)));
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn the_bar_tool_and_the_number_keys_pick_the_roll_tool() {
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    assert!(s.press(&g, Hit::Bar(BarButton::Tool(Tool::Draw))));
    assert_eq!(s.bar_view().tool, Tool::Draw);
    assert_eq!(context(&s, &g).tools.tool, Tool::Draw);
    // A press in the roll now draws a note.
    let count = context(&s, &g).notes().len();
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let p = (v.x(9000), v.y(61) + 7.0);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(context(&s, &g).notes().len(), count + 1);
    // A number key in the roll lights the bar's button.
    key(&mut s, &g, "4", Mods::NONE);
    assert_eq!(s.bar_view().tool, Tool::Eraser);
    assert_eq!(context(&s, &g).tools.tool, Tool::Eraser);
}

#[test]
fn the_bar_grid_is_the_roll_snap_and_its_lines() {
    use crate::popup::Action;
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    let ppq = context(&s, &g).ppq();
    let step = |s: &Studio| {
        let c = context(s, &g);
        let lines = paint::grid_lines(&s.roll, &c);
        assert_eq!(
            lines[1].tick - lines[0].tick,
            c.step(s.roll.beat_px).min(c.ppq())
        );
        c.step(s.roll.beat_px)
    };
    assert!(s.press(&g, Hit::Bar(BarButton::Grid)));
    choose(&mut s, &g, Action::GridValue("1/8"));
    assert_eq!(step(&s), ppq / 2);
    assert_eq!(s.bar_view().grid_text, "1/8");
    choose(&mut s, &g, Action::Triplet);
    assert_eq!(step(&s), ppq / 3);
    assert_eq!(s.bar_view().grid_text, "1/8・3連");
    choose(&mut s, &g, Action::Mode(SnapMode::Bar));
    assert_eq!(step(&s), ppq * 4);
    assert_eq!(s.bar_view().grid_text, "小節");
    // A value picks クオンタイズ again; the triplet stays.
    choose(&mut s, &g, Action::GridValue("1/16"));
    assert_eq!(s.roll_grid_mode(), SnapMode::Quantize);
    assert_eq!(step(&s), ppq / 6);
    choose(&mut s, &g, Action::Snap(false));
    assert!(context(&s, &g).snap(s.roll.beat_px, false).is_none());
    assert!(!s.bar_view().snap);
}

#[test]
fn each_roll_edit_is_one_step_in_the_history_popover() {
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    let before = s.song.history().len();
    key(&mut s, &g, "a", Mods::ctrl());
    key(&mut s, &g, "up", Mods::alt());
    key(&mut s, &g, "d", Mods::NONE);
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let p = at(&s, &g, 0, Zone::Body);
    drag(&mut s, &g, p, (v.px(960), 0.0), Mods::NONE);
    assert!(s.press(&g, Hit::Bar(BarButton::History)));
    let history = &s.popup.as_ref().unwrap().history;
    assert_eq!(history.len(), before + 3, "{history:?}");
    for item in &history[before..] {
        assert!(item.command.starts_with("note."), "{item:?}");
        assert!(!item.label.is_empty() && item.label != "編集", "{item:?}");
    }
    assert!(history.last().unwrap().current);
}

#[test]
fn a_new_studio_snaps_in_auto_mode() {
    use crate::popup::Action;
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    assert_eq!(s.snap_mode, SnapMode::Auto);
    assert_eq!(context(&s, &g).tools.snap.mode, SnapMode::Auto);
    assert!(s.press(&g, Hit::Bar(BarButton::Grid)));
    let p = s.popup.as_ref().unwrap();
    assert_eq!(p.mode, SnapMode::Auto);
    let on: Vec<String> = p
        .layout(g.size)
        .cells
        .iter()
        .filter(|c| c.on && c.action == Some(Action::Mode(SnapMode::Auto)))
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(on, ["自動"]);
}

/// Puts an off-grid note at 1100 and selects only it.
fn off_grid(s: &mut Studio, g: &Geometry) {
    let c = context(s, g);
    s.song
        .run(
            "note.put",
            json!({"clip":c.clip.id.to_string(),"notes":[
                {"at":1100,"pitch":67,"length":240,"velocity":80}
            ]}),
        )
        .unwrap();
    s.resized(g);
    let c = context(s, g);
    let id = c.notes().iter().find(|n| n.pitch == 67).unwrap().id;
    s.roll.selected = [id].into_iter().collect();
}

fn starts(s: &Studio, g: &Geometry) -> Vec<(u8, i64)> {
    let mut v: Vec<(u8, i64)> = context(s, g)
        .notes()
        .iter()
        .map(|n| (n.pitch, n.start))
        .collect();
    v.sort();
    v
}

#[test]
fn with_aq_on_a_grid_value_requantizes_the_selected_notes_in_one_step() {
    use crate::popup::Action;
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    let ppq = context(&s, &g).ppq();
    off_grid(&mut s, &g);
    let before = starts(&s, &g);
    let steps = s.song.history().len();
    assert!(s.press(&g, Hit::Bar(BarButton::Grid)));
    choose(&mut s, &g, Action::AutoQuantize);
    assert!(s.auto_quantize);
    assert!(s.popup.as_ref().unwrap().aq);
    // Turning AQ on by itself does not move anything.
    assert_eq!(starts(&s, &g), before);
    choose(&mut s, &g, Action::GridValue("1/4"));
    let after = starts(&s, &g);
    assert!(after.contains(&(67, 960)), "{after:?}");
    // The other notes were not selected and stay where they were.
    assert_eq!(
        after.iter().filter(|n| n.0 != 67).collect::<Vec<_>>(),
        before.iter().filter(|n| n.0 != 67).collect::<Vec<_>>()
    );
    let history = s.song.history();
    assert_eq!(history.len(), steps + 1, "{history:?}");
    assert_eq!(history.last().unwrap().command, "note.quantize");
    // The triplet toggle requantizes on the new grid too.
    choose(&mut s, &g, Action::Triplet);
    let moved = starts(&s, &g).into_iter().find(|n| n.0 == 67).unwrap();
    assert_eq!(moved.1 % (ppq / 3), 0, "{moved:?}");
    assert_ne!(moved.1, 960);
    assert_eq!(s.song.history().len(), steps + 2);
}

#[test]
fn with_aq_off_or_nothing_selected_a_grid_value_moves_no_note() {
    use crate::popup::Action;
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    off_grid(&mut s, &g);
    let before = starts(&s, &g);
    let steps = s.song.history().len();
    assert!(s.press(&g, Hit::Bar(BarButton::Grid)));
    choose(&mut s, &g, Action::GridValue("1/4"));
    assert_eq!(starts(&s, &g), before);
    assert_eq!(s.song.history().len(), steps);
    // AQ on with nothing selected: still nothing moves.
    s.roll.selected.clear();
    choose(&mut s, &g, Action::AutoQuantize);
    choose(&mut s, &g, Action::GridValue("1/8"));
    assert_eq!(starts(&s, &g), before);
    assert_eq!(s.song.history().len(), steps);
}

/// A point on the roll's ruler `rel` ticks from the clip's head, `dy` px below
/// the ruler's top. The ruler reads song time, so the clip's start is added.
fn ruler_at(s: &Studio, g: &Geometry, rel: Tick, dy: f32) -> (f32, f32) {
    let c = context(s, g);
    let f = s.roll.frame(c.body);
    (s.roll.ruler_scale(&c).x(c.clip.start + rel), f.ruler.y + dy)
}

#[test]
fn a_press_below_the_roll_ruler_band_seeks_to_the_song_tick_under_it() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let ppq = c.ppq();
    let p = ruler_at(&s, &g, 3 * ppq, 20.0);
    let out = s.roll.down(&c, p.0, p.1, Mods::NONE, 1);
    assert_eq!(out.seek, Some(c.clip.start + 3 * ppq));
    s.apply_roll(&g, out);
    assert_eq!(s.playhead, c.clip.start + 3 * ppq);
    assert!(s.roll.drag.is_none(), "a seek starts no drag");
}

#[test]
fn the_roll_ruler_puts_song_time_where_the_arrangement_ruler_does() {
    let (mut s, g) = setup();
    // The same pixels per beat and no scroll on either side.
    s.beat_w = 60.0;
    s.scroll.0 = 0.0;
    s.roll.beat_px = 60.0;
    s.roll.scroll.0 = 0.0;
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    let roll = s.roll.ruler_scale(&c);
    let arr = s.ruler_scale(&g);
    // Each ruler measures song time from its own left edge: the roll's from
    // the clip's head, which sits `px(clip.start)` left of the grid's edge.
    let head = s.roll.view(&c).px(c.clip.start);
    for t in [0, 960, 3840, 9600] {
        let from_roll = roll.x(t) - f.grid.x + head;
        let from_arr = arr.x(t) - g.lanes.x;
        assert!(
            (from_roll - from_arr).abs() < 1e-3,
            "tick {t}: {from_roll} vs {from_arr}"
        );
    }
}

#[test]
fn a_drag_on_the_roll_loop_band_makes_then_moves_the_loop_and_commits_it() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    let ppq = c.ppq();
    let base = c.clip.start;
    // A new range from one beat to three: the press seeks and starts it.
    let a = ruler_at(&s, &g, ppq, 5.0);
    let b = ruler_at(&s, &g, 3 * ppq, 5.0);
    let out = s.roll.down(&c, a.0, a.1, Mods::NONE, 1);
    assert_eq!(out.seek, Some(base + ppq));
    s.apply_roll(&g, out);
    assert_eq!(s.playhead, base + ppq);
    moved(&mut s, &g, b, Mods::NONE);
    assert_eq!(s.roll.preview.loop_span, Some((base + ppq, base + 3 * ppq)));
    up(&mut s, &g, b, Mods::NONE);
    let loop_now = |s: &Studio| s.project().loop_range.map(|l| (l.start, l.end));
    assert_eq!(loop_now(&s), Some((base + ppq, base + 3 * ppq)));

    // The start grip moves the start alone.
    let grip = ruler_at(&s, &g, ppq, 5.0);
    let to = ruler_at(&s, &g, 2 * ppq, 5.0);
    drag(&mut s, &g, grip, (to.0 - grip.0, 0.0), Mods::NONE);
    assert_eq!(loop_now(&s), Some((base + 2 * ppq, base + 3 * ppq)));
}

fn cc1(s: &Studio, g: &Geometry) -> Vec<(i64, i32)> {
    let c = context(s, g);
    s.project()
        .clip(c.clip.id)
        .unwrap()
        .1
        .controllers
        .iter()
        .find(|l| l.kind == CtrlKind::Cc(1))
        .map(|l| l.points.iter().map(|q| (q.tick, q.value)).collect())
        .unwrap_or_default()
}

fn with_cc1(s: &mut Studio, g: &Geometry) {
    let c = context(s, g);
    s.song
        .run(
            "cc.replace",
            json!({"clip": c.clip.id.to_string(), "lane": "cc1", "from": 0, "to": 1920,
                   "points": [{"tick": 0, "value": 10}, {"tick": 480, "value": 90},
                              {"tick": 960, "value": 40}]}),
        )
        .unwrap();
    s.roll
        .lanes
        .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
    s.roll.active_lane = 1;
    s.arrange.focus = false;
}

#[test]
fn controller_points_copy_cut_and_paste_at_the_pointer_in_one_step_each() {
    let (mut s, g) = setup();
    with_cc1(&mut s, &g);
    let first = cc1(&s, &g);
    assert_eq!(first.len(), 3);
    s.roll.points = [0, 480].into_iter().collect();
    let steps = s.song.steps();
    key(&mut s, &g, "c", Mods::ctrl());
    assert_eq!(s.song.steps(), steps);
    assert_eq!(
        s.song.session().state.clipboard.as_ref().map(|b| b.kind()),
        Some("controller")
    );
    // Paste where the pointer is: the first point lands there, the next keeps its distance.
    let c = context(&s, &g);
    let x = s.roll.view(&c).x(2880);
    s.roll.pointer = Some((x, s.roll.frame(c.body).grid.y + 10.0));
    key(&mut s, &g, "v", Mods::ctrl());
    assert_eq!(s.song.steps(), steps + 1);
    let after = cc1(&s, &g);
    assert_eq!(after.len(), 5);
    assert!(after.iter().any(|p| p.0 == 2880) && after.iter().any(|p| p.0 == 3360));
    // The pasted points are the selection.
    assert_eq!(
        s.roll.points.iter().copied().collect::<Vec<_>>(),
        [2880, 3360]
    );
    key(&mut s, &g, "z", Mods::ctrl());
    assert_eq!(cc1(&s, &g), first);

    // Cut takes them out in one step; one undo brings them back.
    s.roll.points = [0, 480].into_iter().collect();
    key(&mut s, &g, "x", Mods::ctrl());
    assert_eq!(cc1(&s, &g).len(), 1);
    assert!(s.roll.points.is_empty());
    key(&mut s, &g, "z", Mods::ctrl());
    assert_eq!(cc1(&s, &g), first);
}

#[test]
fn copying_notes_in_the_roll_fills_the_sessions_clipboard_and_paste_still_works() {
    let (mut s, g) = setup();
    key(&mut s, &g, "a", Mods::ctrl());
    let steps = s.song.steps();
    key(&mut s, &g, "c", Mods::ctrl());
    assert_eq!(s.song.steps(), steps);
    assert_eq!(
        s.song.session().state.clipboard.as_ref().map(|b| b.len()),
        Some(3)
    );
    // The CLI reaches the same clipboard: a paste by command puts the notes in as well.
    let c = context(&s, &g);
    s.song
        .run(
            "edit.paste",
            json!({"clip": c.clip.id.to_string(), "at": c.clip.start}),
        )
        .unwrap();
    assert_eq!(context(&s, &g).notes().len(), 6);
    key(&mut s, &g, "z", Mods::ctrl());
    assert_eq!(context(&s, &g).notes().len(), 3);
}

#[test]
fn invert_none_and_in_loop_work_on_the_notes_of_the_open_clip() {
    let (mut s, g) = setup();
    s.arrange.focus = false;
    key(&mut s, &g, "a", Mods::ctrl());
    assert_eq!(s.roll.selected.len(), 3);
    key(&mut s, &g, "i", Mods::ctrl());
    assert!(s.roll.selected.is_empty());
    key(&mut s, &g, "i", Mods::ctrl());
    assert_eq!(s.roll.selected.len(), 3);
    key(&mut s, &g, "d", Mods::ctrl());
    assert!(s.roll.selected.is_empty());
    // Of three notes, the loop covers the first two (at 960 and 2880).
    let c = context(&s, &g);
    let start = c.clip.start;
    s.song
        .run(
            "loop.set",
            json!({"start": start + 900, "end": start + 3000}),
        )
        .unwrap();
    key(&mut s, &g, "l", Mods::alt());
    assert_eq!(s.roll.selected.len(), 2);
}

fn sus() -> LaneKind {
    LaneKind::Ctrl(CtrlKind::Cc(64))
}

#[test]
fn the_pedal_is_down_from_a_point_at_64_or_more_to_the_next_below_it() {
    use super::lanemark::pedal_spans;
    let p = |tick, value| CtrlPoint::new(tick, value);
    assert!(pedal_spans(&[], 3840).is_empty());
    assert_eq!(
        pedal_spans(&[p(0, 127), p(960, 0), p(1920, 127)], 3840),
        vec![(0, 960), (1920, 3840)]
    );
    // A second "on" inside a stretch does not cut it; a lone "off" starts nothing.
    assert_eq!(
        pedal_spans(&[p(0, 0), p(480, 127), p(960, 127), p(1440, 0)], 3840),
        vec![(480, 1440)]
    );
}

#[test]
fn pressing_the_sus_tab_chooses_the_lane_and_opens_it_when_it_was_folded() {
    let (mut s, g) = setup();
    s.roll.lanes.push(Lane::new(sus()));
    s.roll.lanes[1].folded = true;
    let f = s.roll.frame(context(&s, &g).body);
    assert!(f.lanes[1].folded);
    let tab = f.tabs[1];
    assert_eq!(s.roll.active_lane, 0);
    down(
        &mut s,
        &g,
        (tab.x + tab.w / 2.0, tab.y + tab.h / 2.0),
        Mods::NONE,
    );
    assert_eq!(s.roll.active_lane, 1);
    assert!(!s.roll.lanes[1].folded);
    let f = s.roll.frame(context(&s, &g).body);
    assert!(!f.lanes[1].folded);
    assert!(f.lanes[1].plot.h > super::frame::LANE_FOLDED_H);
    // The song is untouched by choosing a lane.
    assert!(context(&s, &g).clip.controllers.is_empty());
}

#[test]
fn the_playhead_line_is_where_the_ruler_puts_its_time_in_both_views() {
    let (mut s, g) = setup();
    let ctx = context(&s, &g);
    let tm = ctx.project.time_map();
    let ppq = ctx.ppq();
    // A bar's start, a point inside a bar, and a bar's start further on.
    for at in [
        tm.bar_start(6),
        tm.bar_start(6) + ppq + 240,
        tm.bar_start(8) + 2 * ppq,
    ] {
        s.playhead = at;
        let ctx = context(&s, &g);
        // The roll counts from the clip's head; the grid's left edge less the scroll.
        let roll_x = s.roll.view(&ctx).x(at - ctx.clip.start);
        let roll_line = super::ruler::playhead_x(&s, &s.roll, &ctx);
        assert!(
            (roll_line - roll_x).abs() < 0.01,
            "roll line {roll_line}, ruler {roll_x} at {at}"
        );
        // The arrangement: its lanes' left edge less the scroll, plus the time in px.
        let arrange_x = g.lanes.x + s.ticks_to_px(at) - s.scroll.0;
        let arrange_line = crate::draw::playhead_x(&s, &g);
        assert!(
            (arrange_line - arrange_x).abs() < 0.01,
            "arrange line {arrange_line}, ruler {arrange_x} at {at}"
        );
    }
}

#[test]
fn key_017_a_velocity_bar_with_nothing_bound_is_left_alone() {
    let (mut s, g) = setup();
    for gesture in ["click", "drag"] {
        s.song
            .run(
                "mousemap.set",
                serde_json::json!({
                    "context": format!("lane.select.velocity-bar.{gesture}"),
                    "modifier": "none",
                    "action": "",
                }),
            )
            .unwrap();
    }
    let c = context(&s, &g);
    let lb = s.roll.frame(c.body).lanes[0];
    let v = s.roll.view(&c);
    let n = &c.notes()[0];
    let p = (
        v.x(n.start),
        super::lanes::y_of(LaneKind::Velocity, &lb.plot, i32::from(n.velocity)),
    );
    let steps = s.song.steps();
    drag(&mut s, &g, p, (0.0, -5.0), Mods::NONE);
    assert_eq!(s.song.steps(), steps);
    assert_eq!(context(&s, &g).notes()[0].velocity, n.velocity);
}

fn band(s: &Studio, g: &Geometry, lane: usize, from: Tick, to: Tick) -> ((f32, f32), (f32, f32)) {
    let c = context(s, g);
    let v = s.roll.view(&c);
    let plot = s.roll.frame(c.body).lanes[lane].plot;
    ((v.x(from), plot.y + 1.0), (v.x(to), plot.bottom() - 1.0))
}

#[test]
fn key_016_a_rectangle_in_a_controller_lane_selects_the_points_inside() {
    let (mut s, g) = setup();
    with_cc1(&mut s, &g);
    let steps = s.song.steps();
    let (a, b) = band(&s, &g, 1, 240, 700);
    down(&mut s, &g, a, Mods::NONE);
    moved(&mut s, &g, b, Mods::NONE);
    assert!(s.roll.preview.band.is_some());
    up(&mut s, &g, b, Mods::NONE);
    assert!(s.roll.preview.band.is_none());
    assert_eq!(s.roll.points.iter().copied().collect::<Vec<_>>(), [480]);
    // Shift adds the points of the next rectangle; no key starts the selection over.
    let (a, b) = band(&s, &g, 1, 800, 1100);
    down(&mut s, &g, a, Mods::shift());
    moved(&mut s, &g, b, Mods::shift());
    up(&mut s, &g, b, Mods::shift());
    assert_eq!(
        s.roll.points.iter().copied().collect::<Vec<_>>(),
        [480, 960]
    );
    let (a, b) = band(&s, &g, 1, 100, 500);
    drag(&mut s, &g, a, (b.0 - a.0, b.1 - a.1), Mods::NONE);
    assert_eq!(s.roll.points.iter().copied().collect::<Vec<_>>(), [480]);
    assert_eq!(s.song.steps(), steps, "selecting writes nothing");
}

#[test]
fn key_016_a_rectangle_over_the_bar_tips_selects_those_bars() {
    let (mut s, g) = setup();
    let ids: Vec<_> = context(&s, &g).notes().iter().map(|n| n.id).collect();
    let steps = s.song.steps();
    let (a, b) = band(&s, &g, 0, 500, 3000);
    drag(&mut s, &g, a, (b.0 - a.0, b.1 - a.1), Mods::NONE);
    assert_eq!(
        s.roll.selected.iter().copied().collect::<Vec<_>>(),
        ids[..2].to_vec()
    );
    // A rectangle that holds no tip empties the selection.
    let (a, b) = band(&s, &g, 0, 3300, 4000);
    drag(&mut s, &g, a, (b.0 - a.0, b.1 - a.1), Mods::NONE);
    assert!(s.roll.selected.is_empty());
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn key_016_a_lane_rectangle_with_nothing_bound_does_nothing() {
    let (mut s, g) = setup();
    s.song
        .run(
            "mousemap.set",
            serde_json::json!({
                "context": "lane.select.empty.drag", "modifier": "none", "action": "",
            }),
        )
        .unwrap();
    let first = context(&s, &g).notes()[0].id;
    s.roll.selected.insert(first);
    let (a, b) = band(&s, &g, 0, 3300, 4000);
    drag(&mut s, &g, a, (b.0 - a.0, b.1 - a.1), Mods::NONE);
    assert_eq!(s.roll.selected.iter().copied().collect::<Vec<_>>(), [first]);
}

#[test]
fn key_016_alt_click_on_the_empty_grid_selects_every_note_from_there_on() {
    let (mut s, g) = setup();
    key(&mut s, &g, "1", Mods::NONE);
    let c = context(&s, &g);
    let ids: Vec<_> = c.notes().iter().map(|n| n.id).collect();
    let v = s.roll.view(&c);
    let p = (v.x(2000), v.y(62) + 3.0);
    let steps = s.song.steps();
    down(&mut s, &g, p, Mods::alt());
    up(&mut s, &g, p, Mods::alt());
    assert_eq!(
        s.roll.selected.iter().copied().collect::<Vec<_>>(),
        ids[1..].to_vec()
    );
    // A plain click on the same spot empties the selection again.
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert!(s.roll.selected.is_empty());
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn a_selected_note_is_the_strong_ink_and_reads_against_every_track_colour() {
    use xirius_design::{Surface, Theme, TrackColor, contrast_ratio};
    // The least contrast a selected note keeps against its track's colour: dark 1.85, light 2.75 (the mock measured 1.89 and 2.78).
    for (theme, least) in [(Theme::Dark, 1.85), (Theme::Light, 2.75)] {
        let c = crate::paint::Colors::new(theme);
        let strong = Surface::S1.strong_text().ink_color(theme);
        for i in 0..TrackColor::COUNT {
            let track = c.tracks[i];
            assert_eq!(super::paint::note_face(true, track, &c), c.ink_strong);
            assert_eq!(super::paint::note_face(false, track, &c), track);
            let ratio = contrast_ratio(strong, TrackColor::new(i).content(theme));
            assert!(ratio >= least, "{theme:?} track {i}: {ratio:.2}");
        }
    }
}

fn clip_len_now(s: &Studio, g: &Geometry) -> Tick {
    context(s, g).clip_len()
}

#[test]
fn feel_05c_the_pencil_past_the_clip_end_grows_the_clip_in_one_step() {
    let (mut s, g) = setup();
    s.roll.beat_px = 20.0;
    s.roll.scroll.0 = 0.0;
    s.tool = Tool::Draw;
    let c = context(&s, &g);
    let (len, ppq) = (c.clip_len(), c.ppq());
    let before = c.notes().len();
    let p = ruler_at(&s, &g, len + ppq, 0.0);
    let y = at(&s, &g, 0, Zone::Body).1;
    let steps = s.song.steps();
    down(&mut s, &g, (p.0, y), Mods::NONE);
    assert_eq!(
        s.roll.drag.as_ref().map(|d| d.mode),
        Some(Mode::Draw),
        "the pencil keeps drawing past the end"
    );
    up(&mut s, &g, (p.0, y), Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1, "one history step");
    let after = context(&s, &g);
    assert_eq!(after.notes().len(), before + 1);
    let last = after
        .notes()
        .iter()
        .map(|n| n.start + n.length)
        .max()
        .unwrap();
    assert!(last > len, "the note lies past the old end");
    let tm = after.project.time_map();
    let bar = tm.to_bbt(after.clip.start + last - 1).0;
    assert_eq!(
        after.clip_len(),
        tm.bar_start(bar + 1) - after.clip.start,
        "the clip ends with the bar that holds the note"
    );
    assert!(s.song.undo());
    assert_eq!(clip_len_now(&s, &g), len);
    assert_eq!(context(&s, &g).notes().len(), before);
}

#[test]
fn feel_05c_the_select_tool_past_the_clip_end_still_makes_a_new_clip() {
    let (mut s, g) = setup();
    s.roll.beat_px = 20.0;
    s.roll.scroll.0 = 0.0;
    s.tool = Tool::Select;
    let c = context(&s, &g);
    let p = ruler_at(&s, &g, c.clip_len() + c.ppq(), 0.0);
    let y = at(&s, &g, 0, Zone::Body).1;
    down(&mut s, &g, (p.0, y), Mods::NONE);
    assert_eq!(s.roll.drag.as_ref().map(|d| d.mode), Some(Mode::Part));
    s.roll.drag = None;
}

#[test]
fn feel_05c_the_end_handle_on_the_ruler_resizes_the_clip() {
    let (mut s, g) = setup();
    s.roll.beat_px = 20.0;
    s.roll.scroll.0 = 0.0;
    let c = context(&s, &g);
    let (len, ppq) = (c.clip_len(), c.ppq());
    let handle = ruler_at(&s, &g, len, 20.0);
    // On the band (the top) the same x is not the handle.
    let on_band = ruler_at(&s, &g, len, 5.0);
    moved(&mut s, &g, on_band, Mods::NONE);
    assert!(!s.roll.end_hover);
    moved(&mut s, &g, handle, Mods::NONE);
    assert!(s.roll.end_hover);
    assert_eq!(s.roll.cursor(Some(&context(&s, &g))), Cursor::ResizeH);
    // The ruler's drag-to-zoom leaves the handle alone, and keeps the rest of the ruler.
    s.ruler_drag_down(&g, handle.0, handle.1, Mods::NONE);
    assert!(s.ruler_grab.is_none(), "the handle takes the drag");
    let elsewhere = ruler_at(&s, &g, len / 2, 20.0);
    s.ruler_drag_down(&g, elsewhere.0, elsewhere.1, Mods::NONE);
    assert!(s.ruler_grab.is_some(), "the rest of the ruler still zooms");
    s.ruler_grab = None;
    let steps = s.song.steps();
    let to = (handle.0 - 2.0 * 20.0, handle.1);
    down(&mut s, &g, handle, Mods::NONE);
    assert_eq!(s.roll.drag.as_ref().map(|d| d.mode), Some(Mode::ClipEnd));
    moved(&mut s, &g, to, Mods::NONE);
    let want = s.roll.preview.clip_end.expect("a preview length");
    assert!(want < len && want >= ppq, "the end moved left: {want}");
    assert_eq!(
        clip_len_now(&s, &g),
        len,
        "nothing is committed until release"
    );
    up(&mut s, &g, to, Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(clip_len_now(&s, &g), want);
    // A release where it started changes nothing.
    let handle = ruler_at(&s, &g, want, 20.0);
    let steps = s.song.steps();
    drag(&mut s, &g, handle, (0.0, 0.0), Mods::NONE);
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn feel_05c_the_shown_end_eases_to_a_new_length_and_the_veil_covers_what_is_outside() {
    let (mut s, g) = setup();
    s.roll.beat_px = 20.0;
    s.roll.scroll.0 = 0.0;
    s.motion.enabled.set(true);
    let c = context(&s, &g);
    let len = c.clip_len();
    assert_eq!(s.roll.shown_len(&s, &c), len as f32);
    let id = c.clip.id.to_string();
    s.song
        .run("clip.resize", json!({"ids": [id], "length": len + 3840}))
        .unwrap();
    let c = context(&s, &g);
    let now = s.roll.shown_len(&s, &c);
    assert!(
        now < (len + 3840) as f32,
        "it starts from the old end: {now}"
    );
    s.motion.enabled.set(false);
    assert_eq!(s.roll.shown_len(&s, &c), (len + 3840) as f32);
}

fn centre_tick(s: &Studio, g: &Geometry) -> f64 {
    let c = context(s, g);
    let f = s.roll.frame(c.body);
    s.roll.view(&c).tick_f(f.grid.x + f.grid.w / 2.0)
}

#[test]
fn feel_05d_one_drag_on_the_slider_reaches_both_ends_of_the_zoom() {
    let (mut s, g) = setup();
    s.roll.beat_px = 48.0;
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    let y = f.zoom_track.y + f.zoom_track.h / 2.0;
    let thumb = zoom::thumb_of(f.zoom_track, 48.0);
    let (steps, before) = (s.song.steps(), centre_tick(&s, &g));
    down(&mut s, &g, (thumb.x + 6.0, y), Mods::NONE);
    assert_eq!(s.roll.drag.as_ref().map(|d| d.mode), Some(Mode::Zoom));
    assert!(
        (s.roll.beat_px - 48.0).abs() < 4.0,
        "a press on the thumb does not jump: {}",
        s.roll.beat_px
    );
    moved(&mut s, &g, (f.zoom_track.right() + 30.0, y), Mods::NONE);
    assert_eq!(
        s.roll.beat_px, BEAT_PX_MAX,
        "past the right end is the widest"
    );
    moved(&mut s, &g, (f.zoom_track.x - 30.0, y), Mods::NONE);
    assert_eq!(
        s.roll.beat_px, BEAT_PX_MIN,
        "past the left end is the narrowest"
    );
    moved(
        &mut s,
        &g,
        (f.zoom_track.x + f.zoom_track.w / 2.0, y),
        Mods::NONE,
    );
    let mid = (BEAT_PX_MIN * BEAT_PX_MAX).sqrt();
    assert!(
        (s.roll.beat_px - mid).abs() / mid < 0.02,
        "{}",
        s.roll.beat_px
    );
    up(
        &mut s,
        &g,
        (f.zoom_track.x + f.zoom_track.w / 2.0, y),
        Mods::NONE,
    );
    assert!(s.roll.drag.is_none());
    assert_eq!(s.song.steps(), steps, "zooming is not an edit");
    // The tick in the middle of the grid stayed there (up to the scroll's clamp).
    let after = centre_tick(&s, &g);
    assert!((after - before).abs() < 2.0 * 960.0, "{before} -> {after}");
}

#[test]
fn feel_05d_the_buttons_zoom_by_four_steps_around_the_middle() {
    let (mut s, g) = setup();
    s.roll.beat_px = 96.0;
    s.roll.scroll.0 = 800.0;
    let c = context(&s, &g);
    let f = s.roll.frame(c.body);
    let before = centre_tick(&s, &g);
    let p = (f.zoom_in.x + 4.0, f.zoom_in.y + 4.0);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    let want = 96.0 * 1.18f32.powf(4.0);
    assert!((s.roll.beat_px - want).abs() < 0.01, "{}", s.roll.beat_px);
    assert!((centre_tick(&s, &g) - before).abs() < 2.0);
    let p = (f.zoom_out.x + 4.0, f.zoom_out.y + 4.0);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert!((s.roll.beat_px - 96.0).abs() < 0.01, "{}", s.roll.beat_px);
    assert!((centre_tick(&s, &g) - before).abs() < 2.0);
    // The ends hold.
    s.roll.beat_px = BEAT_PX_MAX;
    down(
        &mut s,
        &g,
        (f.zoom_in.x + 4.0, f.zoom_in.y + 4.0),
        Mods::NONE,
    );
    assert_eq!(s.roll.beat_px, BEAT_PX_MAX);
}

#[test]
fn feel_05d_the_zoom_command_takes_a_width_and_the_strip_parts_leave_the_tabs_room() {
    let (mut s, g) = setup();
    s.roll.beat_px = 48.0;
    assert!(s.run_binding(&g, "view.zoom.time --to 600"));
    assert_eq!(s.roll.beat_px, 600.0);
    assert!(s.run_binding(&g, "view.zoom.time --to 99999"));
    assert_eq!(s.roll.beat_px, BEAT_PX_MAX);
    assert!(s.run_binding(&g, "view.zoom.time --by -4"));
    assert!((s.roll.beat_px - BEAT_PX_MAX / 1.18f32.powf(4.0)).abs() < 0.5);
    // The parts do not overlap the name and the tabs, nor each other.
    let f = s.roll.frame(context(&s, &g).body);
    assert!(f.name.right() <= f.zoom_out.x);
    assert!(f.zoom_out.right() <= f.zoom_track.x);
    assert!(f.zoom_track.right() <= f.zoom_in.x);
    assert!(f.zoom_in.right() <= f.strip.right());
}

#[test]
fn feel_05d_the_ruler_numbers_follow_the_width_of_a_beat() {
    let (mut s, g) = setup();
    // The deepest number in the roll's ruler, as a beat widens: "1", "1.2", "1.2.3".
    for (px, want) in [
        (20.0, 0),
        (51.9, 0),
        (52.0, 1),
        (150.0, 1),
        (208.0, 2),
        (4096.0, 2),
    ] {
        s.roll.beat_px = px;
        let c = context(&s, &g);
        let marks = crate::ruler::labels(&c.project.time_map(), c.project.ppq, px, false, 0, 3840);
        let depth = marks
            .iter()
            .map(|m| m.label.matches('.').count())
            .max()
            .unwrap_or(0);
        assert_eq!(depth, want, "{px}");
    }
}

#[test]
fn feel_06_the_cut_tool_shows_a_line_on_the_grid_and_the_press_splits_there() {
    let (mut s, g) = setup();
    s.tool = Tool::Cut;
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let body = at(&s, &g, 1, Zone::Body);
    let x = v.x(2880 + 500);
    assert!(
        s.roll.cut_line(&c).is_none(),
        "nothing before the pointer is there"
    );
    moved(&mut s, &g, (x, body.1), Mods::NONE);
    let c = context(&s, &g);
    let (id, rel) = s.roll.cut_line(&c).expect("a line over the note");
    assert_eq!(id, c.notes()[1].id);
    assert!(rel > 2880 && rel < 2880 + 1440);
    assert_ne!(rel, 2880 + 500, "it sits on the grid");
    assert_eq!(s.roll.cursor(Some(&c)), Cursor::Split);
    // Another stretch of the grid moves the line, which is a redraw; the same one does not.
    let out = s.roll.moved(&c, x + 0.5, body.1, Mods::NONE);
    assert!(!out.redraw);
    let out = s.roll.moved(&c, v.x(2880 + 1100), body.1, Mods::NONE);
    assert!(out.redraw);
    let (_, later) = s.roll.cut_line(&c).unwrap();
    assert!(later > rel);
    moved(&mut s, &g, (x, body.1), Mods::NONE);
    down(&mut s, &g, (x, body.1), Mods::NONE);
    up(&mut s, &g, (x, body.1), Mods::NONE);
    let c = context(&s, &g);
    assert_eq!(c.notes().len(), 4);
    assert!(c.notes().iter().any(|n| n.start == rel));
    // Over the empty grid, and with another tool, there is no line.
    moved(&mut s, &g, (v.x(100), body.1 + 200.0), Mods::NONE);
    assert!(s.roll.cut_line(&c).is_none());
    s.tool = Tool::Select;
    moved(&mut s, &g, (x, body.1), Mods::NONE);
    assert!(s.roll.cut_line(&context(&s, &g)).is_none());
}

#[test]
fn feel_06_the_mute_tool_marks_the_note_under_the_pointer_before_the_press() {
    let (mut s, g) = setup();
    s.tool = Tool::Mute;
    let p = at(&s, &g, 0, Zone::Body);
    let c = context(&s, &g);
    assert_eq!(s.roll.mute_target(&c), None);
    moved(&mut s, &g, p, Mods::NONE);
    let c = context(&s, &g);
    assert_eq!(s.roll.mute_target(&c), Some(c.notes()[0].id));
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    let c = context(&s, &g);
    assert!(c.notes()[0].muted);
    // On the muted note the target is still it: the look it shows is the way back.
    assert_eq!(s.roll.mute_target(&c), Some(c.notes()[0].id));
    // Off the note, or with another tool, nothing is marked.
    moved(&mut s, &g, (p.0, p.1 + 200.0), Mods::NONE);
    assert_eq!(s.roll.mute_target(&context(&s, &g)), None);
    s.tool = Tool::Select;
    moved(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.mute_target(&context(&s, &g)), None);
}

#[test]
fn split_and_mute_do_nothing_in_a_controller_lane_and_the_pointer_says_so() {
    for tool in [Tool::Cut, Tool::Mute] {
        let (mut s, g) = setup();
        s.roll
            .lanes
            .push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
        s.tool = tool;
        let c = context(&s, &g);
        let lb = s.roll.frame(c.body).lanes[1];
        let p = (lb.plot.x + 40.0, lb.plot.y + lb.plot.h * 0.5);
        let steps = s.song.steps();
        down(&mut s, &g, p, Mods::NONE);
        moved(&mut s, &g, (p.0 + 90.0, p.1 - 20.0), Mods::NONE);
        up(&mut s, &g, (p.0 + 90.0, p.1 - 20.0), Mods::NONE);
        assert_eq!(
            s.song.steps(),
            steps,
            "{tool:?} wrote something into the lane"
        );
        let c = context(&s, &g);
        assert_eq!(s.roll.cursor(Some(&c)), Cursor::Blocked, "{tool:?}");
    }
}

#[test]
fn the_pencil_keeps_the_last_length_only_when_the_setting_says_so() {
    for (setting, same) in [("inspector", false), ("last", true)] {
        let (mut s, g) = setup();
        s.song
            .run(
                "prefs.set",
                json!({"key":"edit.draw.length","value":setting}),
            )
            .unwrap();
        s.tool = Tool::Draw;
        let y = at(&s, &g, 0, Zone::Body).1;
        let v = s.roll.view(&context(&s, &g));
        // A long note, dragged out; then a bare click further on.
        drag(&mut s, &g, (v.x(5760), y), (v.px(1920), 0.0), Mods::NONE);
        down(&mut s, &g, (v.x(9600), y), Mods::NONE);
        up(&mut s, &g, (v.x(9600), y), Mods::NONE);
        let mut drawn: Vec<_> = context(&s, &g)
            .notes()
            .iter()
            .filter(|n| n.start >= 5760)
            .map(|n| (n.start, n.length))
            .collect();
        drawn.sort();
        assert_eq!(drawn.len(), 2, "{setting}: {drawn:?}");
        assert_eq!(drawn[0].1, 1920, "{setting}: {drawn:?}");
        assert_eq!(drawn[1].1 == drawn[0].1, same, "{setting}: {drawn:?}");
    }
}

#[test]
fn dragging_the_top_note_sideways_with_mod_alt_strums_the_chord_in_one_step() {
    let (mut s, g) = setup();
    let clip = context(&s, &g).clip.id.to_string();
    s.song
        .run(
            "note.put",
            json!({"clip":clip,"notes":[
                {"at":7680,"pitch":60,"length":960,"velocity":80},
                {"at":7680,"pitch":64,"length":960,"velocity":80},
                {"at":7680,"pitch":67,"length":960,"velocity":80}
            ]}),
        )
        .unwrap();
    let chord = |s: &Studio| {
        let mut v: Vec<_> = context(s, &g)
            .notes()
            .iter()
            .filter(|n| n.start >= 7680)
            .map(|n| (n.pitch, n.start, n.length))
            .collect();
        v.sort();
        v
    };
    let c = context(&s, &g);
    s.roll.selected = c
        .notes()
        .iter()
        .filter(|n| n.start >= 7680)
        .map(|n| n.id)
        .collect();
    let top = c.notes().iter().position(|n| n.pitch == 67).unwrap();
    let p = at(&s, &g, top, Zone::Body);
    let mod_alt = Mods {
        ctrl: true,
        alt: true,
        ..Mods::NONE
    };
    let steps = s.song.steps();
    down(&mut s, &g, p, Mods::NONE);
    let v = s.roll.view(&context(&s, &g));
    moved(&mut s, &g, (p.0 + v.px(480), p.1), mod_alt);
    // The picture shows the spread before anything is written.
    assert_eq!(s.song.steps(), steps);
    assert_eq!(s.roll.preview.strum, Some(480));
    up(&mut s, &g, (p.0 + v.px(480), p.1), mod_alt);
    assert_eq!(s.song.steps() - steps, 1);
    assert_eq!(
        chord(&s),
        [(60, 7680, 960), (64, 7920, 720), (67, 8160, 480)]
    );
    // Dragged the other way, the high notes go first.
    assert!(s.song.undo());
    let c = context(&s, &g);
    let top = c.notes().iter().position(|n| n.pitch == 67).unwrap();
    let p = at(&s, &g, top, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 - v.px(480), p.1), mod_alt);
    assert_eq!(s.roll.preview.strum, Some(-480));
    up(&mut s, &g, (p.0 - v.px(480), p.1), mod_alt);
    assert_eq!(
        chord(&s),
        [(60, 8160, 480), (64, 7920, 720), (67, 7680, 960)]
    );
}

#[test]
fn the_handles_round_the_selected_velocity_bars_scale_and_tilt_them_in_one_step_each() {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    s.roll.selected = c.notes().iter().map(|n| n.id).collect();
    let plot = s.roll.frame(c.body).lanes[0].plot;
    assert_eq!(s.roll.lanes[0].kind, LaneKind::Velocity);
    // Four pixels up (past the slop), in values.
    let up_by = 4.0;
    let dv = (up_by * 126.0 / (plot.h - 2.0 * crate::roll::frame::LANE_PAD)).round() as i32;
    assert!((10..40).contains(&dv), "{dv}");
    let velocities = |s: &Studio| {
        let mut v: Vec<_> = context(s, &g)
            .notes()
            .iter()
            .map(|n| (n.start, n.velocity))
            .collect();
        v.sort();
        v
    };
    let handles = s
        .roll
        .velocity_handles(&c, &s.roll.view(&c), &plot)
        .unwrap();
    let centre = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);

    // The top handle carries the loudest tip up with it: the spread grows about the mean.
    let (x, y) = centre(handles.scale);
    let steps = s.song.steps();
    down(&mut s, &g, (x, y), Mods::NONE);
    moved(&mut s, &g, (x, y - up_by), Mods::NONE);
    assert_eq!(s.roll.preview.changed.len(), 3);
    up(&mut s, &g, (x, y - up_by), Mods::NONE);
    assert_eq!(s.song.steps() - steps, 1);
    // The loudest tip (100) moves by dv; the spread about the mean (90) grows by the same ratio.
    let scale = f64::from(10 + dv) * 10.0;
    let at = |v: f64| (90.0 + (v - 90.0) * scale / 100.0).round() as u8;
    assert_eq!(
        velocities(&s),
        [(960, at(80.0)), (2880, 90), (4800, at(100.0))]
    );
    let after_scale = velocities(&s);

    // The corner handle lifts the last bar and the ones before it in proportion.
    let c = context(&s, &g);
    let handles = s
        .roll
        .velocity_handles(&c, &s.roll.view(&c), &plot)
        .unwrap();
    let (x, y) = centre(handles.tilt.unwrap());
    let steps = s.song.steps();
    down(&mut s, &g, (x, y), Mods::NONE);
    moved(&mut s, &g, (x, y - up_by), Mods::NONE);
    up(&mut s, &g, (x, y - up_by), Mods::NONE);
    assert_eq!(s.song.steps() - steps, 1);
    assert_eq!(
        velocities(&s),
        [
            (960, after_scale[0].1),
            (2880, after_scale[1].1 + (f64::from(dv) / 2.0).round() as u8),
            (4800, (i32::from(after_scale[2].1) + dv).min(127) as u8)
        ]
    );

    // A press on a handle with no drag writes nothing.
    let c = context(&s, &g);
    let handles = s
        .roll
        .velocity_handles(&c, &s.roll.view(&c), &plot)
        .unwrap();
    let (x, y) = centre(handles.scale);
    let steps = s.song.steps();
    down(&mut s, &g, (x, y), Mods::NONE);
    up(&mut s, &g, (x, y), Mods::NONE);
    assert_eq!(s.song.steps(), steps);
    // One bar alone has no handles.
    s.roll.selected = BTreeSet::from([c.notes()[0].id]);
    assert!(
        s.roll
            .velocity_handles(&c, &s.roll.view(&c), &plot)
            .is_none()
    );
}

#[test]
fn the_audition_tool_asks_to_play_from_the_press_while_held() {
    let (mut s, g) = setup();
    s.tool = Tool::Audition;
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let p = at(&s, &g, 1, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.listen, Some(c.clip.start + v.tick(p.0)));
    assert!(s.roll.audition.is_some());
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.listen, None);
    assert_eq!(s.roll.audition, None);
}

/// The roll with a kick, a snare and a hat (36, 38, 42) as sixteenth-long hits, shown as drums.
fn drums() -> (Studio, Geometry) {
    let (mut s, g) = setup();
    let c = context(&s, &g);
    s.song
        .run(
            "note.delete",
            json!({"ids":ids_json(c.notes().iter().map(|n| &n.id))}),
        )
        .unwrap();
    s.song
        .run(
            "note.put",
            json!({"clip":c.clip.id.to_string(),"notes":[
                {"at":960,"pitch":36,"length":240,"velocity":100},
                {"at":1920,"pitch":38,"length":240,"velocity":90},
                {"at":2880,"pitch":42,"length":240,"velocity":80}
            ]}),
        )
        .unwrap();
    s.song
        .run(
            "roll.view",
            json!({"clip":c.clip.id.to_string(),"mode":"drums"}),
        )
        .unwrap();
    s.resized(&g);
    (s, g)
}

#[test]
fn drum_view_has_a_row_for_each_sound_of_the_map_and_none_for_the_rest() {
    let (s, g) = drums();
    let c = context(&s, &g);
    assert_eq!(c.drum.as_deref(), Some("gm"));
    let v = s.roll.view(&c);
    assert!(v.drums());
    assert_eq!(v.rows.len(), 14);
    assert_eq!(v.rows.pitches().next(), Some(51));
    assert_eq!(v.rows.pitches().last(), Some(36));
    // The rows sit one under another with no gap, however far apart the pitches are.
    assert_eq!(v.y(36) - v.y(37), v.row_h);
    assert_eq!(v.y(42) - v.y(44), v.row_h);
    assert_eq!(v.pitch_at(v.y(42) + v.row_h / 2.0), 42);
}

#[test]
fn a_hit_is_a_square_centered_on_its_start_whatever_its_length() {
    let (s, g) = drums();
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    for n in c.notes() {
        let r = super::notes::note_rect(&v, &NoteView::from(n));
        assert_eq!((r.w, r.h), (v.row_h, v.row_h));
        assert_eq!(r.x + r.w / 2.0, v.x(n.start));
    }
}

#[test]
fn a_clip_note_outside_the_map_gets_a_row_of_its_own() {
    let (mut s, g) = drums();
    let c = context(&s, &g);
    s.song
        .run(
            "note.put",
            json!({"clip":c.clip.id.to_string(),"notes":[
                {"at":3840,"pitch":90,"length":240,"velocity":70}
            ]}),
        )
        .unwrap();
    s.resized(&g);
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    assert_eq!(v.rows.len(), 15);
    assert_eq!(v.rows.pitches().next(), Some(90));
    assert_eq!(c.drum_name(90), "ノート 90");
}

#[test]
fn clicking_a_hit_selects_it_and_dragging_it_moves_it_by_rows_not_semitones() {
    let (mut s, g) = drums();
    let p = at(&s, &g, 0, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.selected.len(), 1);
    let row_h = s.roll.view(&context(&s, &g)).row_h;
    let steps = s.song.steps();
    drag(&mut s, &g, p, (0.0, -2.0 * row_h), Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1);
    // Kick 36 → 37 → 38: two rows up in the map, as the keys of the map are.
    let pitches: Vec<u8> = context(&s, &g).notes().iter().map(|n| n.pitch).collect();
    assert!(pitches.contains(&38), "{pitches:?}");
    assert!(!pitches.contains(&36), "{pitches:?}");
    assert_eq!(pitches.len(), 3);
}

#[test]
fn a_hit_cannot_be_dragged_past_the_last_row() {
    let (mut s, g) = drums();
    let p = at(&s, &g, 0, Zone::Body);
    let row_h = s.roll.view(&context(&s, &g)).row_h;
    drag(&mut s, &g, p, (0.0, 40.0 * row_h), Mods::NONE);
    let pitches: Vec<u8> = context(&s, &g).notes().iter().map(|n| n.pitch).collect();
    assert!(pitches.contains(&36), "{pitches:?}");
}

#[test]
fn the_pencil_lays_a_hit_at_every_step_it_is_dragged_across() {
    let (mut s, g) = drums();
    s.tool = Tool::Draw;
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let step = c.step(s.roll.beat_px);
    let before = c.notes().len();
    let y = v.y(42) + v.row_h / 2.0;
    let p = (v.x(4800), y);
    let end = (v.x(4800 + 3 * step), y);
    let steps = s.song.steps();
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, end, Mods::NONE);
    assert!(s.roll.preview.ghosts.len() >= 4);
    up(&mut s, &g, end, Mods::NONE);
    assert_eq!(s.song.steps(), steps + 1);
    let c = context(&s, &g);
    let laid: Vec<_> = c.notes().iter().filter(|n| n.start >= 4800).collect();
    assert_eq!(c.notes().len(), before + 4);
    assert!(laid.iter().all(|n| n.pitch == 42 && n.length == step));
}

fn pencil_length(s: &mut Studio, g: &Geometry, tick: Tick) -> Tick {
    s.tool = Tool::Draw;
    let y = at(s, g, 0, Zone::Body).1;
    let v = s.roll.view(&context(s, g));
    down(s, g, (v.x(tick), y), Mods::NONE);
    up(s, g, (v.x(tick), y), Mods::NONE);
    context(s, g)
        .notes()
        .iter()
        .filter(|n| n.start == tick)
        .map(|n| n.length)
        .next()
        .unwrap_or_else(|| panic!("no note at {tick}: {:?}", context(s, g).notes()))
}

#[test]
fn the_note_before_counts_the_selected_and_the_resized_note_as_well_as_the_drawn_one() {
    use crate::studio::Hit;
    let (mut s, g) = setup();
    s.choose_editor();
    assert!(!s.draw_length_last());
    assert!(s.press(&g, Hit::RollLength(true)));
    assert!(s.draw_length_last());
    assert_eq!(
        s.draw_length_text().as_deref(),
        Some("1/16"),
        "nothing before yet: the grid's value"
    );
    // Taking the 3/8 note in hand: the next note is as long.
    s.tool = Tool::Select;
    let p = at(&s, &g, 1, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.draw_length_text().as_deref(), Some("3/8"));
    assert_eq!(pencil_length(&mut s, &g, 7680), 1440);
    // Shortening the first note to a 1/8: that is the one before now.
    s.tool = Tool::Select;
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let edge = at(&s, &g, 0, Zone::EdgeR);
    drag(&mut s, &g, edge, (-v.px(480), 0.0), Mods::NONE);
    assert_eq!(s.draw_length_text().as_deref(), Some("1/8"));
    assert_eq!(pencil_length(&mut s, &g, 5760), 480);
    // A note drawn longer by dragging is the one before as well.
    s.tool = Tool::Draw;
    let y = at(&s, &g, 0, Zone::Body).1;
    let v = s.roll.view(&context(&s, &g));
    drag(&mut s, &g, (v.x(9600), y), (v.px(1920), 0.0), Mods::NONE);
    assert_eq!(s.draw_length_text().as_deref(), Some("1/2"));
}

#[test]
fn the_grids_value_keeps_the_pencil_at_the_grid_whatever_is_selected() {
    use crate::studio::Hit;
    let (mut s, g) = setup();
    s.choose_editor();
    assert!(s.press(&g, Hit::RollLength(true)));
    assert!(s.press(&g, Hit::RollLength(false)));
    assert!(!s.draw_length_last());
    assert_eq!(s.draw_length_text(), None);
    s.tool = Tool::Select;
    let p = at(&s, &g, 1, Zone::Body);
    down(&mut s, &g, p, Mods::NONE);
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(pencil_length(&mut s, &g, 7680), 240);
}

#[test]
fn the_length_shows_right_of_the_grid_and_a_press_on_it_goes_back_to_the_grids_value() {
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    s.choose_editor();
    let bar = |s: &Studio| s.bar_layout(g.size.0);
    assert_eq!(bar(&s).length.w, 0.0, "the grid's value: nothing to show");
    assert!(s.press(&g, Hit::RollLength(true)));
    let layout = bar(&s);
    assert!(layout.length.w > 0.0);
    assert!(layout.length.x >= layout.grid.right(), "right of the grid");
    assert!(layout.length.right() <= layout.cpu.x.max(layout.rec_panel.x));
    assert_eq!(s.bar_view().draw_length.as_deref(), Some("1/16"));
    let (x, y) = s.center_of(&g, Hit::Bar(BarButton::DrawLength)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Bar(BarButton::DrawLength));
    assert!(s.press(&g, Hit::Bar(BarButton::DrawLength)));
    assert!(!s.draw_length_last());
    assert_eq!(bar(&s).length.w, 0.0);
    assert_eq!(s.bar_view().draw_length, None);
}

#[test]
fn the_drum_view_has_no_length_to_pick_and_shows_none() {
    use crate::studio::Hit;
    let (mut s, g) = setup();
    s.choose_editor();
    assert!(s.press(&g, Hit::RollLength(true)));
    let switch = g.roll_length(true);
    assert_eq!(
        s.hit(&g, switch.x + 4.0, switch.y + 4.0),
        Hit::RollLength(true)
    );
    assert!(s.press(&g, Hit::RollView(true)));
    assert_eq!(s.draw_length_text(), None);
    assert_eq!(s.bar_layout(g.size.0).length.w, 0.0);
    assert!(!matches!(
        s.hit(&g, switch.x + 4.0, switch.y + 4.0),
        Hit::RollLength(_)
    ));
}

#[test]
fn a_hit_on_the_first_tick_shows_whole_in_the_drum_view() {
    let (mut s, g) = drums();
    let c = context(&s, &g);
    s.song
        .run(
            "note.put",
            json!({"clip":c.clip.id.to_string(),"notes":[
                {"at":0,"pitch":36,"length":240,"velocity":100}
            ]}),
        )
        .unwrap();
    s.resized(&g);
    let c = context(&s, &g);
    s.roll.sync(&c);
    s.roll.center(&c);
    let v = s.roll.view(&c);
    let first = c
        .notes()
        .iter()
        .find(|n| n.start == 0)
        .expect("the hit at 0");
    let r = super::notes::note_rect(&v, &NoteView::from(first));
    assert!(r.x >= v.grid.x, "{} < {}", r.x, v.grid.x);
    // Scrolled as far left as it goes, the hit still shows whole; the keyboard keeps its edge.
    s.roll.scroll.0 = -10_000.0;
    s.roll.clamp_scroll(&c);
    let v = s.roll.view(&c);
    assert!(super::notes::note_rect(&v, &NoteView::from(first)).x >= v.grid.x);
    s.song
        .run(
            "roll.view",
            json!({"clip":c.clip.id.to_string(),"mode":"keys"}),
        )
        .unwrap();
    s.resized(&g);
    let c = context(&s, &g);
    s.roll.sync(&c);
    s.roll.scroll.0 = -10_000.0;
    s.roll.clamp_scroll(&c);
    let keys = s.roll.view(&c);
    assert_eq!(keys.x(-c.clip.start), keys.grid.x);
}

#[test]
fn the_aq_switch_is_the_setting_and_stays_on_when_the_focus_moves() {
    use crate::popup::Action;
    use crate::studio::Hit;
    use crate::topbar::BarButton;
    let (mut s, g) = setup();
    let pref = |s: &Studio| {
        s.song
            .settings()
            .lock()
            .unwrap()
            .prefs
            .bool("edit.quantize.auto")
    };
    assert!(!pref(&s));
    assert!(s.press(&g, Hit::Bar(BarButton::Grid)));
    choose(&mut s, &g, Action::AutoQuantize);
    assert!(s.auto_quantize && pref(&s));
    // Moving to the arrangement and back reads the settings again: AQ is still on.
    for focus in [true, false] {
        s.arrange.focus = focus;
        s.apply_edit_prefs();
        assert!(s.auto_quantize, "focus on the arrangement: {focus}");
    }
    choose(&mut s, &g, Action::AutoQuantize);
    assert!(!s.auto_quantize && !pref(&s));
}

#[test]
fn the_value_tip_setting_hides_the_rolls_balloon() {
    let (mut s, g) = setup();
    key(&mut s, &g, "5", Mods::NONE);
    let c = context(&s, &g);
    let v = s.roll.view(&c);
    let p = (v.x(7000) + 50.0, v.y(61) + 7.0);
    down(&mut s, &g, p, Mods::NONE);
    moved(&mut s, &g, (p.0 + 100.0, p.1 - 10.0), Mods::NONE);
    assert!(s.roll_balloon().unwrap().text.contains("ベロシティ"));
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "edit.drag.value-tip", "value": "false"}),
    );
    s.apply_edit_prefs();
    assert!(s.roll.preview.balloon.is_some());
    assert!(s.roll_balloon().is_none());
}

#[test]
fn a_note_picked_sounds_only_with_the_audition_setting_and_the_tool_always() {
    let (mut s, g) = setup();
    let p = at(&s, &g, 0, Zone::Body);
    // The setting is on at first (UI-285): a note picked sounds.
    s.apply_edit_prefs();
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.sounding(), Some(60));
    up(&mut s, &g, p, Mods::NONE);
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "edit.audition", "value": "false"}),
    );
    s.apply_edit_prefs();
    // Turned off, the key lights, but nothing sounds.
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.audition, Some(60));
    assert_eq!(s.roll.sounding(), None);
    up(&mut s, &g, p, Mods::NONE);
    // The audition tool is for listening: it sounds whatever the setting.
    key(&mut s, &g, "8", Mods::NONE);
    down(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.sounding(), Some(60));
    up(&mut s, &g, p, Mods::NONE);
    assert_eq!(s.roll.sounding(), None);
}

#[test]
fn a_parts_edge_is_one_bar_line_where_the_grid_has_a_line_and_where_two_parts_meet() {
    use xirius_design::GridTier;
    let (mut s, g) = setup();
    s.roll.beat_px = 24.0;
    s.roll.scroll.0 = 0.0;
    let c = context(&s, &g);
    let (start, beat) = (c.clip.start, c.ppq());
    let tm = c.project.time_map();
    let at = |bar: u32| tm.bar_start(tm.to_bbt(start).0 + bar);
    assert_eq!(at(10) - at(0), 40 * beat, "4/4 from the roll's clip");
    let made = |v: serde_json::Value| v["id"].as_str().unwrap().to_string();
    let t = made(
        s.song
            .run("track.add.instrument", json!({"name": "Parts"}))
            .unwrap(),
    );
    // A over 8 bars, B from where A ends for a bar and a beat: both in the roll with its clip.
    let a = made(
        s.song
            .run(
                "clip.add.midi",
                json!({"track": t, "at": at(0), "length": at(8) - at(0)}),
            )
            .unwrap(),
    );
    let b = made(
        s.song
            .run(
                "clip.add.midi",
                json!({"track": t, "at": at(8), "length": at(9) - at(8) + beat}),
            )
            .unwrap(),
    );
    s.rebuild(&g);
    let t: xirius_core::id::TrackId = t.parse().unwrap();
    let target = s.selected_clip.unwrap();
    s.arrange.clips = [target, (t, a.parse().unwrap()), (t, b.parse().unwrap())]
        .into_iter()
        .collect();
    let c = context(&s, &g);
    assert_eq!(c.others.len(), 2);
    let grid = paint::grid_lines(&s.roll, &c);
    let tier = |lines: &[crate::grid_lines::Line], t: Tick| {
        let on: Vec<_> = lines.iter().filter(|l| l.tick == t - start).collect();
        assert_eq!(on.len(), 1, "one line at {t}: {on:?}");
        (on[0].tier, on[0].fade)
    };
    // A's start and A's end (B's start) are on bar lines; B's end is on a beat line.
    assert_eq!(tier(&grid, at(0)).0, GridTier::Bar);
    assert_eq!(tier(&grid, at(8)).0, GridTier::Bar);
    assert_eq!(tier(&grid, at(9) + beat).0, GridTier::Beat);
    let lines = parts::edged(grid.clone(), &c);
    // No time is painted twice over the translucent color: each edge is the one bar line there.
    for w in lines.windows(2) {
        assert!(w[0].tick < w[1].tick, "{w:?}");
    }
    for edge in [at(0), at(8), at(9) + beat] {
        assert_eq!(tier(&lines, edge), (GridTier::Bar, 1.0), "{edge}");
    }
    // The other lines are the grid's.
    let rest = |l: &[crate::grid_lines::Line]| -> Vec<_> {
        l.iter()
            .filter(|l| l.tick != at(9) + beat - start)
            .copied()
            .collect()
    };
    assert_eq!(rest(&lines), rest(&grid));
}
