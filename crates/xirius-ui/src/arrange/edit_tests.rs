use super::*;
use crate::arrange::probe::{clip_mid, track};

fn setup() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 820.0);
    s.rebuild(&g);
    (s, g)
}
fn gesture(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32), mods: Mods) {
    s.arrange_down(g, from.0, from.1, mods);
    s.arrange_move(g, to.0, to.1, mods);
    s.arrange_up(g, to.0, to.1, mods);
    assert!(s.error.is_none(), "{:?}", s.error);
}
#[test]
fn link_uses_the_upper_half_only_on_normal_or_taller_tracks() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let cid = s.project().tracks[&t].clips[0].id;
    s.song.run("view.link", json!({"on":true})).unwrap();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let c = s.project().clip(cid).unwrap().1.clone();
    let r = s.clip_rect(&g, &row, &c);
    s.arrange_down(&g, r.x + r.w / 2.0, r.y + r.h / 4.0, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Range);
    s.arrange_down(&g, r.x + r.w / 2.0, r.y + r.h * 0.65, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
    s.arrange.track_h = Arrange::TRACK_H_MIN;
    s.rebuild(&g);
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, &row, &c);
    s.arrange_down(&g, r.x + r.w / 2.0, r.y + r.h / 4.0, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
}
#[test]
fn the_link_setting_turns_link_on_and_off() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let cid = s.project().tracks[&t].clips[0].id;
    s.song.run("view.link", json!({"on":true})).unwrap();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let c = s.project().clip(cid).unwrap().1.clone();
    let r = s.clip_rect(&g, &row, &c);
    let upper = (r.x + r.w / 2.0, r.y + r.h / 4.0);
    // Turned off in the settings, the upper half picks the clip like the lower half.
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrange.link", "value": "true"}),
    );
    s.apply_edit_prefs();
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrange.link", "value": "false"}),
    );
    s.arrange_down(&g, upper.0, upper.1, Mods::NONE);
    assert!(!s.song.session().state.view.link);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
    // Turned on again, the upper half selects a range.
    s.rec_set(
        "prefs.set",
        json!({"key": "edit.arrange.link", "value": "true"}),
    );
    s.arrange_down(&g, upper.0, upper.1, Mods::NONE);
    assert!(s.song.session().state.view.link);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Range);
    // The toolbar button still turns it off for the song; the setting does not undo that.
    s.song.run("view.link", json!({"off":true})).unwrap();
    s.arrange_down(&g, upper.0, upper.1, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
}
#[test]
fn shift_double_click_selects_every_clip_on_the_track() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    s.song
        .run("clip.duplicate", json!({"ids":[c.id.to_string()]}))
        .unwrap();
    let p = clip_mid(&s, &g, "Keys", 0);
    assert!(s.arrange_open(&g, p.0, p.1, Mods::shift()));
    assert_eq!(s.arrange.clips.len(), 2);
    assert!(!s.drawer_open);
}
#[test]
fn drawing_a_part_is_one_step_and_dragging_backwards_works() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let y = s.row_y(&g, &row) + row.h / 2.0;
    s.tool = Tool::Draw;
    let at = 3840;
    let from = (s.tick_x(&g, at + 960), y);
    let to = (s.tick_x(&g, at), y);
    let before = s.song.steps();
    gesture(&mut s, &g, from, to, Mods::NONE);
    assert_eq!(s.song.steps(), before + 1);
    let p = s.project();
    let c = p.clip(s.selected_clip.unwrap().1).unwrap().1;
    assert_eq!((c.start, c.length), (3840, 960));
}
#[test]
fn erasing_or_muting_a_sweep_is_one_history_step() {
    for tool in [Tool::Eraser, Tool::Mute] {
        let (mut s, g) = setup();
        let t = track(&s, "Keys");
        let c = s.project().tracks[&t].clips[0].clone();
        s.song
            .run(
                "clip.split",
                json!({"ids":[c.id.to_string()],"at":c.start+c.length/2}),
            )
            .unwrap();
        s.tool = tool;
        let from = clip_mid(&s, &g, "Keys", 0);
        let to = clip_mid(&s, &g, "Keys", 1);
        let before = s.song.steps();
        gesture(&mut s, &g, from, to, Mods::NONE);
        assert_eq!(s.song.steps(), before + 1);
        if tool == Tool::Eraser {
            assert!(s.project().tracks[&t].clips.is_empty());
        } else {
            assert!(s.project().tracks[&t].clips.iter().all(|c| c.muted));
        }
        assert!(s.song.undo());
        assert_eq!(s.project().tracks[&t].clips.len(), 2);
    }
}
#[test]
fn smart_lower_click_splits_and_boundary_click_joins_without_changing_tools() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, &row, &c);
    let at = c.start + c.length / 2;
    let p = (s.tick_x(&g, at), r.bottom() - 2.0);
    gesture(&mut s, &g, p, p, Mods::NONE);
    assert_eq!(s.project().tracks[&t].clips.len(), 2);
    let p = (s.tick_x(&g, at) - 1.0, r.bottom() - 2.0);
    assert!(matches!(
        s.arrange_spot(&g, p.0, p.1),
        Spot::Clip { .. } | Spot::Fx { .. }
    ));
    gesture(&mut s, &g, p, p, Mods::NONE);
    assert_eq!(s.project().tracks[&t].clips.len(), 1);
    assert_eq!(s.tool, Tool::Select);
}
#[test]
fn range_shift_adds_to_the_selection_and_double_click_splits_both_ends() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let p = clip_mid(&s, &g, "Keys", 0);
    s.tool = Tool::Range;
    let from = (s.tick_x(&g, 5 * 3840), p.1);
    let to = (s.tick_x(&g, 6 * 3840), p.1);
    gesture(&mut s, &g, from, to, Mods::NONE);
    assert_eq!(s.arrange.range, Some((5 * 3840, 6 * 3840)));
    assert!(s.arrange_open(&g, from.0 + 4.0, p.1, Mods::NONE));
    assert_eq!(s.project().tracks[&t].clips.len(), 3);
}

#[test]
fn alternative_tool_follows_the_existing_custom_hold_setting() {
    let (mut s, g) = setup();
    s.song
        .settings()
        .lock()
        .unwrap()
        .mousemap
        .set("arrange.select.hold", "Mod", "tool:draw")
        .unwrap();
    let t = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let y = s.row_y(&g, &row) + row.h / 2.0;
    s.arrange_down(&g, s.tick_x(&g, 3840), y, Mods::ctrl());
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Draw);
    assert_eq!(s.tool, Tool::Select);
    s.arrange_down(&g, s.tick_x(&g, 3840), y, Mods::NONE);
    assert_eq!(s.arrange.drag.as_ref().unwrap().tool, Tool::Select);
}

#[test]
fn alt_drop_replaces_the_material_in_the_same_slot_and_undo_restores_it() {
    let (mut s, g) = setup();
    let t = track(&s, "Vocal");
    let old = s.project().tracks[&t].clips[0].clone();
    let p = clip_mid(&s, &g, "Vocal", 0);
    s.browser.file_drag = Some(crate::browser_pick::FileDrag {
        path: "replacement.wav".into(),
        name: "replacement".into(),
        from: (0.0, 0.0),
        at: p,
        moved: true,
    });
    let before = s.song.steps();
    assert!(s.replace_file_up(&g, p.0, p.1));
    assert_eq!(s.song.steps(), before + 1);
    let now = s.project().clip(old.id).unwrap().1.clone();
    assert_eq!(
        (now.id, now.start, now.length),
        (old.id, old.start, old.length)
    );
    let ClipBody::Audio { file, offset, .. } = &now.body else {
        panic!()
    };
    assert_eq!(file, "replacement.wav");
    assert_eq!(*offset, 0);
    assert!(s.song.undo());
    assert_eq!(s.project().clip(old.id).unwrap().1, &old);
}

#[test]
fn alt_cut_splits_a_part_and_the_note_crossing_the_boundary() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    let at = c.start + c.length / 2;
    s.song
        .run(
            "note.add",
            json!({"clip":c.id.to_string(),"at":at-120,"pitch":85,"length":240}),
        )
        .unwrap();
    s.tool = Tool::Cut;
    let p = clip_mid(&s, &g, "Keys", 0);
    gesture(&mut s, &g, p, p, Mods::alt());
    let p = s.project();
    let clips = &p.tracks[&t].clips;
    assert_eq!(clips.len(), 2);
    assert!(
        clips[0]
            .notes()
            .iter()
            .any(|n| n.pitch == 85 && n.length == 120)
    );
    assert!(
        clips[1]
            .notes()
            .iter()
            .any(|n| n.pitch == 85 && n.start == 0 && n.length == 120)
    );
}
#[test]
fn feel_06_the_cut_tool_shows_where_a_press_would_split_and_the_press_splits_there() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    let y = clip_mid(&s, &g, "Keys", 0).1;
    let x = s.tick_x(&g, c.start + c.length / 2 + 37);
    s.tool = Tool::Select;
    s.arrange_move(&g, x, y, Mods::NONE);
    assert_eq!(s.cut_preview(&g), None, "only the cut tool shows a line");
    s.tool = Tool::Cut;
    s.arrange_move(&g, s.tick_x(&g, c.start + c.length) + 40.0, y, Mods::NONE);
    assert!(s.arrange_move(&g, x + 1.0, y, Mods::NONE));
    let (pt, pc, at) = s.cut_preview(&g).expect("a line over the clip");
    assert_eq!((pt, pc), (t, c.id));
    assert!(at > c.start && at < c.start + c.length);
    assert_ne!(at, c.start + c.length / 2 + 37, "it sits on the grid");
    // The same stretch of the grid keeps the line where it is: nothing to redraw.
    assert!(!s.arrange_move(&g, x + 1.5, y, Mods::NONE));
    s.arrange_down(&g, x + 1.5, y, Mods::NONE);
    s.arrange_up(&g, x + 1.5, y, Mods::NONE);
    let clips = &s.project().tracks[&t].clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips.iter().map(|c| c.start).max(), Some(at));
}
#[test]
fn feel_06_the_cut_line_follows_shift_and_is_gone_off_the_clip_and_on_its_edge() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    let y = clip_mid(&s, &g, "Keys", 0).1;
    s.tool = Tool::Cut;
    let x = s.tick_x(&g, c.start + c.length / 2 + 37);
    s.arrange_move(&g, x, y, Mods::NONE);
    let snapped = s.cut_preview(&g).unwrap().2;
    // Shift takes the snap off, as it does for the press.
    s.arrange_move(&g, x, y, Mods::shift());
    let free = s.cut_preview(&g).unwrap().2;
    assert_ne!(snapped, free);
    assert!((free - s.tick_at(&g, x)).abs() <= 1);
    s.arrange_move(&g, s.tick_x(&g, c.start) + 0.5, y, Mods::NONE);
    assert_eq!(s.cut_preview(&g), None, "a cut at the head splits nothing");
    s.arrange_move(&g, s.tick_x(&g, c.start + c.length) + 40.0, y, Mods::NONE);
    assert_eq!(s.cut_preview(&g), None);
}
#[test]
fn feel_06_the_mute_tool_shows_the_look_a_press_leaves_and_a_muted_clip_shows_the_way_back() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    let (x, y) = clip_mid(&s, &g, "Keys", 0);
    s.tool = Tool::Mute;
    assert!(!s.clip_muted_shown(c.id, false));
    assert!(s.arrange_move(&g, x, y, Mods::NONE));
    assert!(s.clip_muted_shown(c.id, false), "about to be muted");
    s.arrange_down(&g, x, y, Mods::NONE);
    s.arrange_up(&g, x, y, Mods::NONE);
    assert!(s.project().tracks[&t].clips[0].muted);
    assert!(
        !s.clip_muted_shown(c.id, true),
        "over a muted clip the unmuted look shows"
    );
    s.tool = Tool::Select;
    assert!(
        s.clip_muted_shown(c.id, true),
        "other tools show the song's"
    );
}
#[test]
fn feel_06_a_sweep_shows_every_clip_it_passes_as_the_press_will_leave_them() {
    let (mut s, g) = setup();
    let t = track(&s, "Keys");
    let c = s.project().tracks[&t].clips[0].clone();
    s.song
        .run(
            "clip.split",
            json!({"ids":[c.id.to_string()],"at":c.start+c.length/2}),
        )
        .unwrap();
    let ids: Vec<_> = s.project().tracks[&t].clips.iter().map(|c| c.id).collect();
    // The first clip is muted already, so a sweep over both leaves both unmuted.
    s.song
        .run("clip.mute", json!({"ids":[ids[0].to_string()]}))
        .unwrap();
    s.tool = Tool::Mute;
    let from = clip_mid(&s, &g, "Keys", 0);
    let to = clip_mid(&s, &g, "Keys", 1);
    s.arrange_down(&g, from.0, from.1, Mods::NONE);
    s.arrange_move(&g, to.0, to.1, Mods::NONE);
    let shown: Vec<_> = ids.iter().map(|id| s.clip_muted_shown(*id, true)).collect();
    s.arrange_up(&g, to.0, to.1, Mods::NONE);
    let left: Vec<_> = s.project().tracks[&t]
        .clips
        .iter()
        .map(|c| c.muted)
        .collect();
    assert_eq!(shown, left, "what it showed is what it left");
    assert_eq!(left, [false, false]);
}

#[test]
fn the_range_tool_also_takes_a_stretch_on_a_lane_and_in_the_tempo_row() {
    let (mut s, g) = setup();
    let t = crate::arrange::probe::two_lanes(&mut s, &g, "Bass");
    s.tool = Tool::Range;
    let bar = 3840;
    // Past the last point, the lane is a lane of its own, not a line.
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let y = s.lane_top(&g, &row, 0) + 6.0;
    let from = (s.tick_x(&g, 10 * bar), y);
    assert!(
        matches!(s.arrange_spot(&g, from.0, from.1), Spot::Lane { .. }),
        "{:?}",
        s.arrange_spot(&g, from.0, from.1)
    );
    let to = (s.tick_x(&g, 11 * bar), y);
    gesture(&mut s, &g, from, to, Mods::NONE);
    assert_eq!(s.arrange.range, Some((10 * bar, 11 * bar)));
    // In the tempo row.
    let y = g.tempo.y + 4.0;
    let a = (s.tick_x(&g, 2 * bar), y);
    assert_eq!(s.arrange_spot(&g, a.0, y), Spot::TempoGap);
    let n = s.project().tempo.len();
    let to = (s.tick_x(&g, 3 * bar), y);
    gesture(&mut s, &g, a, to, Mods::NONE);
    assert_eq!(s.arrange.range, Some((2 * bar, 3 * bar)));
    assert_eq!(s.project().tempo.len(), n, "a range adds no tempo point");
}
