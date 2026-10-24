//! Gestures fed into the studio as they come from the window, with no window:
//! press, moves, release, keys and the wheel.

use super::probe::{clip_mid, down, folder, hold, point_xy, row_at, row_mid, track, two_lanes};
use super::*;
use crate::roll::Cursor;
use crate::tools::SnapMode;
use xirius_core::id::Node;

const BAR: Tick = 3840;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.snap_mode = SnapMode::Bar;
    s.song
        .run(
            "prefs.set",
            json!({"key":"edit.snap.arrange.mode","value":"bar"}),
        )
        .unwrap();
    // The arrangement alone, tall enough for every row.
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
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

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

/// Press at `from`, move to `to`, let go.
fn drag(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32), mods: Mods) {
    hold(s, g, from, to, mods);
    s.arrange_up(g, to.0, to.1, mods);
}

fn click(s: &mut Studio, g: &Geometry, at: (f32, f32), mods: Mods) {
    down(s, g, at, mods);
    s.arrange_up(g, at.0, at.1, mods);
}

fn bars(s: &Studio, n: f32) -> f32 {
    s.ticks_to_px(BAR) * n
}

fn clips(s: &Studio, name: &str) -> Vec<xirius_core::model::Clip> {
    let t = track(s, name);
    s.project().tracks[&t].clips.clone()
}

fn steps(s: &Studio) -> usize {
    s.song.steps()
}

fn parent_of(s: &Studio, node: Node) -> (Parent, usize) {
    s.project().find_node(node).expect("in the tree")
}

/// The gesture helpers, with the places worked out before `s` is lent out.
macro_rules! drag {
    ($s:ident, $g:ident, $from:expr, $to:expr, $mods:expr) => {{
        let (from, to) = ($from, $to);
        drag(&mut $s, &$g, from, to, $mods)
    }};
}

macro_rules! hold {
    ($s:ident, $g:ident, $from:expr, $to:expr, $mods:expr) => {{
        let (from, to) = ($from, $to);
        hold(&mut $s, &$g, from, to, $mods)
    }};
}

macro_rules! click {
    ($s:ident, $g:ident, $at:expr, $mods:expr) => {{
        let at = $at;
        click(&mut $s, &$g, at, $mods)
    }};
}

// ---------------------------------------------------------------- clips

#[test]
fn a_clip_dragged_later_lands_on_the_bar_grid_in_one_undoable_step() {
    let (mut s, g) = studio();
    let before = clips(&s, "Keys")[0].start;
    let n = steps(&s);
    let from = clip_mid(&s, &g, "Keys", 0);
    hold!(s, g, from, (from.0 + bars(&s, 2.3), from.1), none());
    // While held: a ghost at the new place, and its start in the balloon.
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let Preview::Clips(ghosts) = &plan.preview else {
        panic!("{:?}", plan.preview)
    };
    assert_eq!(ghosts[0].start, before + 2 * BAR);
    assert!(plan.tip.starts_with("7."), "{}", plan.tip);
    s.arrange_up(&g, from.0 + bars(&s, 2.3), from.1, none());
    assert_eq!(clips(&s, "Keys")[0].start, before + 2 * BAR);
    assert_eq!(steps(&s), n + 1);
    s.undo(&g);
    assert_eq!(clips(&s, "Keys")[0].start, before);
}

#[test]
fn shift_turns_snapping_off_for_the_drag() {
    let (mut s, g) = studio();
    let before = clips(&s, "Keys")[0].start;
    let from = clip_mid(&s, &g, "Keys", 0);
    drag!(s, g, from, (from.0 + bars(&s, 0.4), from.1), shift());
    let after = clips(&s, "Keys")[0].start;
    assert!(after > before && after < before + BAR, "{after}");
    assert_ne!(after % (BAR / 4), 0, "off the beat grid too");
}

#[test]
fn a_clip_moves_to_another_track_of_the_same_kind_only() {
    let (mut s, g) = studio();
    // Keys (MIDI) down onto Vocal (audio): stays on Keys.
    let from = clip_mid(&s, &g, "Keys", 0);
    let vocal = row_at(&s, &g, RowKind::Track(track(&s, "Vocal")), 0.5);
    drag!(s, g, from, (from.0 + bars(&s, 1.0), vocal), none());
    assert_eq!(clips(&s, "Keys").len(), 1);
    assert!(clips(&s, "Vocal").len() == 1);
    // Vocal (audio) down onto Guitar (audio): moves.
    let from = clip_mid(&s, &g, "Vocal", 0);
    let guitar = row_at(&s, &g, RowKind::Track(track(&s, "Guitar")), 0.5);
    drag!(s, g, from, (from.0, guitar), none());
    assert!(clips(&s, "Vocal").is_empty());
    assert_eq!(clips(&s, "Guitar").len(), 2);
}

#[test]
fn alt_drag_copies_and_d_duplicates_the_selection() {
    let (mut s, g) = studio();
    let n = clips(&s, "Drums").len();
    let first = clips(&s, "Drums")[0].clone();
    let from = clip_mid(&s, &g, "Drums", 0);
    drag!(s, g, from, (from.0 + bars(&s, 16.0), from.1), alt());
    let now = clips(&s, "Drums");
    assert_eq!(now.len(), n + 1);
    assert!(
        now.iter()
            .any(|c| c.id == first.id && c.start == first.start)
    );
    assert!(
        now.iter()
            .any(|c| c.id != first.id && c.start == first.start + 16 * BAR)
    );
    // The copy is what is selected now; D puts another right after it.
    assert_eq!(s.arrange.clips.len(), 1);
    let copy = *s.arrange.clips.iter().next().unwrap();
    assert_ne!(copy.1, first.id);
    assert!(s.arrange.focus);
    assert!(s.arrange_key(&g, "d", none()));
    assert_eq!(clips(&s, "Drums").len(), n + 2);
    assert!(
        clips(&s, "Drums")
            .iter()
            .any(|c| c.start == first.start + 20 * BAR)
    );
    // Delete takes the selection away.
    assert!(s.arrange_key(&g, "delete", none()));
    assert_eq!(clips(&s, "Drums").len(), n + 1);
}

#[test]
fn the_edges_change_the_length_and_the_start() {
    let (mut s, g) = studio();
    let t = track(&s, "Keys");
    let c = clips(&s, "Keys")[0].clone();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let r = s.clip_rect(&g, &row, &c);
    let y = r.y + r.h / 2.0;
    assert!(matches!(
        s.arrange_spot(&g, r.right() - 2.0, y),
        Spot::Clip {
            grip: Grip::Right,
            ..
        }
    ));
    s.arrange_move(&g, r.right() - 2.0, y, none());
    assert_eq!(paint::cursor(&s), Some(Cursor::ResizeH));
    drag!(
        s,
        g,
        (r.right() - 2.0, y),
        (r.right() + bars(&s, 1.1), y),
        none()
    );
    let k = clips(&s, "Keys")[0].clone();
    assert_eq!((k.start, k.length), (c.start, c.length + BAR));
    drag!(s, g, (r.x + 2.0, y), (r.x + 2.0 + bars(&s, 1.0), y), none());
    let k = clips(&s, "Keys")[0].clone();
    assert_eq!((k.start, k.length), (c.start + BAR, c.length));
    // Never shorter than one grid step.
    let r = s.clip_rect(&g, &row, &k);
    drag!(s, g, (r.right() - 2.0, y), (r.x - bars(&s, 6.0), y), none());
    assert_eq!(clips(&s, "Keys")[0].length, BAR);
}

#[test]
fn cut_splits_eraser_deletes_and_mute_mutes() {
    let (mut s, g) = studio();
    let n = steps(&s);
    s.tool = Tool::Cut;
    let c = clips(&s, "Keys")[0].clone();
    let x = s.tick_x(&g, c.start + 2 * BAR + 100);
    let y = clip_mid(&s, &g, "Keys", 0).1;
    click!(s, g, (x, y), none());
    let now = clips(&s, "Keys");
    assert_eq!(now.len(), 2);
    assert_eq!(now[0].length, 2 * BAR, "split on the grid");
    assert_eq!(steps(&s), n + 1);
    s.tool = Tool::Mute;
    click!(s, g, clip_mid(&s, &g, "Keys", 1), none());
    assert!(clips(&s, "Keys")[1].muted);
    s.tool = Tool::Eraser;
    click!(s, g, clip_mid(&s, &g, "Keys", 0), none());
    assert_eq!(clips(&s, "Keys").len(), 1);
    assert_eq!(steps(&s), n + 3);
}

#[test]
fn cut_with_alt_on_audio_acts_as_select_and_select_with_ctrl_as_range() {
    let (mut s, g) = studio();
    s.tool = Tool::Cut;
    click!(s, g, clip_mid(&s, &g, "Vocal", 0), alt());
    assert_eq!(clips(&s, "Vocal").len(), 1, "not split");
    let keys = (track(&s, "Vocal"), clips(&s, "Vocal")[0].id);
    assert_eq!(s.arrange.clips, BTreeSet::from([keys]));

    s.tool = Tool::Select;
    // Ctrl + drag from empty space over Keys and Vocal: a stretch of time.
    let y0 = row_at(&s, &g, RowKind::Track(track(&s, "Keys")), 0.5);
    let y1 = row_at(&s, &g, RowKind::Track(track(&s, "Vocal")), 0.5);
    let x0 = s.tick_x(&g, 2 * BAR);
    drag!(s, g, (x0, y0), (s.tick_x(&g, 7 * BAR), y1), ctrl());
    assert_eq!(s.arrange.range, Some((2 * BAR, 7 * BAR)));
    assert!(s.arrange.clips.contains(&keys));
    assert_eq!(s.arrange.clips.len(), 2, "{:?}", s.arrange.clips);
}

/// The middle of a track's mark (the head's pill), where the pointer presses.
fn mark_mid(s: &Studio, g: &Geometry, t: xirius_core::id::TrackId) -> (f32, f32) {
    let row = *s
        .tree()
        .row_of(RowKind::Track(t))
        .expect("the row is in sight");
    let p = s.project();
    let track = &p.tracks[&t];
    let mark = s.row_mark(&row, track);
    assert!(mark.text.is_some(), "a mark");
    let r = crate::draw::track_mark_rect(&row, &mark);
    (
        g.column.x + r.x + r.w / 2.0,
        s.row_y(g, &row) + r.y + r.h / 2.0,
    )
}

/// The mark on a track's head takes the same head operations as the rest of the row: the
/// right press opens the row's menu, and a shift press chooses the range to the row (UI-312,
/// UI-340).
#[test]
fn the_mark_opens_the_rows_menu_and_joins_a_shift_selection() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let bass = track(&s, "Bass");
    for id in [keys, bass] {
        s.song
            .run(
                "track.transpose",
                json!({"ids": [id.to_string()], "semis": 2}),
            )
            .unwrap();
    }
    s.rebuild(&g);
    let at = mark_mid(&s, &g, keys);
    assert!(matches!(s.hit(&g, at.0, at.1), Hit::TrackMark(t) if t == keys));

    // The right press on the mark gives the row's menu, the one the head's plain part gives.
    assert!(s.timeline_context(&g, at.0, at.1));
    let on_mark: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    s.ctx = None;
    s.popup = None;
    let row = *s.tree().row_of(RowKind::Track(keys)).expect("the row");
    let parts = RowParts::of(&row);
    let plain = (
        g.column.x + parts.name_x + 2.0,
        s.row_y(&g, &row) + RowParts::LINE_1_Y + 4.0,
    );
    assert!(s.timeline_context(&g, plain.0, plain.1));
    let on_row: Vec<String> = s.context_rows().into_iter().map(|r| r.label).collect();
    assert_eq!(on_mark, on_row);
    s.ctx = None;
    s.popup = None;

    // Shift presses on two marks choose both rows and the ones between them.
    s.arrange.tracks.clear();
    let a = mark_mid(&s, &g, keys);
    click(&mut s, &g, a, shift());
    let b = mark_mid(&s, &g, bass);
    click(&mut s, &g, b, shift());
    assert!(s.arrange.tracks.contains(&RowKind::Track(keys)));
    assert!(s.arrange.tracks.contains(&RowKind::Track(bass)));
}

#[test]
fn a_marquee_selects_what_it_touches_and_shift_adds_or_takes_away() {
    let (mut s, g) = studio();
    let y0 = row_at(&s, &g, RowKind::Track(track(&s, "Drums")), 0.5);
    let y1 = row_at(&s, &g, RowKind::Track(track(&s, "Bass")), 0.5);
    // From the empty space after both, leftwards.
    let x = s.tick_x(&g, BAR * 27 / 2);
    drag!(s, g, (x, y0), (s.tick_x(&g, BAR * 6), y1), none());
    assert_eq!(s.arrange.clips.len(), 2, "Drums' second clip and Bass");
    // Shift + click adds Keys, Shift + click again takes it away.
    let keys = clip_mid(&s, &g, "Keys", 0);
    click!(s, g, keys, shift());
    assert_eq!(s.arrange.clips.len(), 3);
    click!(s, g, keys, shift());
    assert_eq!(s.arrange.clips.len(), 2);
    // Both move together.
    let drums = clips(&s, "Drums")[1].start;
    let bass = clips(&s, "Bass")[0].start;
    let from = clip_mid(&s, &g, "Bass", 0);
    drag!(s, g, from, (from.0 + bars(&s, 1.0), from.1), none());
    assert_eq!(clips(&s, "Drums")[1].start, drums + BAR);
    assert_eq!(clips(&s, "Bass")[0].start, bass + BAR);
    // A click on empty space clears it.
    click!(s, g, (s.tick_x(&g, 14 * BAR), y0), none());
    assert!(s.arrange.clips.is_empty());
}

#[test]
fn escape_lets_a_drag_go_without_doing_anything() {
    let (mut s, g) = studio();
    let n = steps(&s);
    let from = clip_mid(&s, &g, "Keys", 0);
    hold!(s, g, from, (from.0 + bars(&s, 2.0), from.1), none());
    assert!(s.arrange_key(&g, "escape", none()));
    s.arrange_up(&g, from.0 + bars(&s, 2.0), from.1, none());
    assert_eq!(steps(&s), n);
}

// ---------------------------------------------------------------- loop

#[test]
fn the_loop_band_makes_stretches_and_moves_the_range() {
    let (mut s, g) = studio();
    let y = g.ruler.y + Arrange::LOOP_H / 2.0;
    // A new range, dragged leftwards from bar 9 to bar 5.
    let (a, b) = (s.tick_x(&g, 8 * BAR + 300), s.tick_x(&g, 4 * BAR - 200));
    drag!(s, g, (a, y), (b, y), none());
    let l = s.project().loop_range.expect("a loop");
    assert_eq!((l.start, l.end), (4 * BAR, 8 * BAR));
    // Its end two bars later.
    let x = s.tick_x(&g, 8 * BAR);
    assert_eq!(s.arrange_spot(&g, x, y), Spot::Loop(LoopGrip::End));
    drag!(s, g, (x, y), (x + bars(&s, 2.0), y), none());
    assert_eq!(s.project().loop_range.unwrap().end, 10 * BAR);
    // Its start one bar earlier.
    let x = s.tick_x(&g, 4 * BAR);
    drag!(s, g, (x, y), (x - bars(&s, 1.0), y), none());
    assert_eq!(s.project().loop_range.unwrap().start, 3 * BAR);
    // The body moves both, keeping the length.
    let x = s.tick_x(&g, 6 * BAR);
    drag!(s, g, (x, y), (x + bars(&s, 2.0), y), none());
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (5 * BAR, 12 * BAR));
    // Shift: off the grid.
    let x = s.tick_x(&g, 12 * BAR);
    drag!(s, g, (x, y), (x + bars(&s, 0.3), y), shift());
    assert_ne!(s.project().loop_range.unwrap().end % BAR, 0);
}

#[test]
fn auto_punch_makes_the_loop_band_the_punch_range_with_the_rolls_beside_it() {
    let (mut s, g) = studio();
    let y = g.ruler.y + Arrange::LOOP_H / 2.0;
    let (a, b) = (s.tick_x(&g, 4 * BAR), s.tick_x(&g, 8 * BAR));
    drag!(s, g, (a, y), (b, y), none());
    assert!(
        paint::punch_bands(&s, &s.project()).is_none(),
        "off: just a loop"
    );
    s.song.set("record.punch", json!({"on": true})).unwrap();
    let bands = paint::punch_bands(&s, &s.project()).expect("on: the punch range");
    assert_eq!(bands.range, (4 * BAR, 8 * BAR));
    assert_eq!((bands.pre, bands.post), (None, None));
    s.song
        .set("record.preroll", json!({"on": true, "bars": 2}))
        .unwrap();
    s.song
        .set("record.postroll", json!({"on": true, "bars": 1}))
        .unwrap();
    let bands = paint::punch_bands(&s, &s.project()).unwrap();
    assert_eq!(bands.pre, Some((2 * BAR, 4 * BAR)));
    assert_eq!(bands.post, Some((8 * BAR, 9 * BAR)));
    // The post-roll only lives with auto-punch.
    s.song.set("record.punch", json!({"on": false})).unwrap();
    assert!(paint::punch_bands(&s, &s.project()).is_none());
}

#[test]
fn the_keys_i_o_and_shift_c_flip_auto_punch_pre_roll_and_pre_count() {
    let (mut s, g) = studio();
    let pref = |s: &Studio, k: &str| {
        let st = s.song.settings().lock().unwrap();
        (st.prefs.bool(k), st.prefs.number(k))
    };
    assert!(!pref(&s, "record.punch").0 && !pref(&s, "record.preroll").0);
    assert!(s.mapped_key(&g, "i", None, none()));
    assert!(pref(&s, "record.punch").0);
    assert!(s.mapped_key(&g, "o", None, none()));
    assert!(pref(&s, "record.preroll").0);
    let was = pref(&s, "audio.count-in").1;
    assert!(s.mapped_key(&g, "c", None, shift()));
    assert_ne!(pref(&s, "audio.count-in").1, was, "the pre-count flips");
    assert!(
        !s.transport.metronome,
        "C alone is still the metronome, not Shift+C"
    );
    assert!(s.error.is_none(), "{:?}", s.error);
    // Again: back to how they were.
    for (k, m) in [("i", none()), ("o", none()), ("c", shift())] {
        assert!(s.mapped_key(&g, k, None, m));
    }
    assert!(!pref(&s, "record.punch").0 && !pref(&s, "record.preroll").0);
    assert_eq!(pref(&s, "audio.count-in").1, was);
}

// ---------------------------------------------------------------- sections and tempo

#[test]
fn the_pencil_paints_a_section_on_the_empty_band_and_stops_at_the_next_one() {
    let (mut s, g) = studio();
    s.tool = Tool::Draw;
    // Shorten Verse (bars 4..12) to 2 bars: the band is empty from bar 6 to the Chorus.
    let verse = s.project().sections[1].clone();
    s.song
        .run(
            "section.resize",
            json!({"id": verse.id.to_string(), "length": 2 * BAR}),
        )
        .unwrap();
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, 6 * BAR) + bars(&s, 0.5);
    assert_eq!(s.arrange_spot(&g, x, y), Spot::Band);
    let n = s.project().sections.len();
    // A press alone: one bar, with a name not yet taken; the playhead stays.
    let at = s.playhead;
    click!(s, g, (x, y), none());
    assert_eq!(s.project().sections.len(), n + 1);
    let one = s
        .project()
        .sections
        .iter()
        .find(|x| x.start == 6 * BAR)
        .unwrap()
        .clone();
    assert_eq!(one.length, BAR);
    assert!(
        s.project()
            .sections
            .iter()
            .filter(|x| x.name == one.name)
            .count()
            == 1
    );
    assert_eq!(s.arrange.section, Some(one.id), "the new one is selected");
    assert_eq!(s.playhead, at);
    // Pulled across 3 bars: three bars, in one undoable step.
    let x = s.tick_x(&g, 8 * BAR) + bars(&s, 0.5);
    let before = steps(&s);
    drag!(s, g, (x, y), (x + bars(&s, 2.0), y), none());
    let three = s
        .project()
        .sections
        .iter()
        .find(|x| x.start == 8 * BAR)
        .expect("a section from bar 8")
        .clone();
    assert_eq!(three.length, 3 * BAR);
    assert_eq!(steps(&s), before + 1);
    // Painting on toward the Chorus (bar 12) stops at its head.
    let x = s.tick_x(&g, 11 * BAR) + bars(&s, 0.5);
    hold!(s, g, (x, y), (x + bars(&s, 3.0), y), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(
        matches!(plan.preview, Preview::NewSection { start, end } if start == 11 * BAR && end == 12 * BAR),
        "{:?}",
        plan.preview
    );
}

#[test]
fn a_section_dragged_with_alt_leaves_itself_and_puts_a_copy_at_the_slot() {
    let (mut s, g) = studio();
    let before: Vec<_> = s
        .project()
        .sections
        .iter()
        .map(|x| x.name.clone())
        .collect();
    let intro = s.project().sections[0].clone();
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, intro.start + intro.length / 2);
    hold!(s, g, (x, y), (x + bars(&s, 7.0), y), alt());
    let d = s.arrange.drag.clone().expect("dragging");
    let plan = s.plan(&g, &d).expect("a plan");
    assert_eq!(plan.command.as_ref().unwrap().0, "section.duplicate");
    assert_eq!(plan.tip, "3 番目に複製", "{}", plan.tip);
    s.arrange_up(&g, d.at.0, d.at.1, alt());
    let names: Vec<_> = s
        .project()
        .sections
        .iter()
        .map(|x| x.name.clone())
        .collect();
    assert_eq!(names.len(), before.len() + 1);
    assert_eq!(names[0], before[0], "the original stays first");
    assert_eq!(
        names[2], before[0],
        "and a copy sits at the slot: {names:?}"
    );
    // the same drag without Alt moves it and says so
    let (mut s, g) = studio();
    hold!(s, g, (x, y), (x + bars(&s, 7.0), y), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert_eq!(plan.command.as_ref().unwrap().0, "section.reorder");
    assert!(
        plan.tip.starts_with(&format!("{} → ", intro.name)),
        "{}",
        plan.tip
    );
}

#[test]
fn a_section_dragged_takes_the_clips_of_every_track_along() {
    let (mut s, g) = studio();
    let drums = clips(&s, "Drums")[0].clone();
    assert_eq!(drums.start, 0);
    let p = s.project();
    let intro = p.sections[0].clone();
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, intro.start + intro.length / 2);
    hold!(s, g, (x, y), (x + bars(&s, 7.0), y), none());
    assert_eq!(paint::cursor(&s), Some(Cursor::Move));
    s.arrange_up(&g, x + bars(&s, 7.0), y, none());
    let p = s.project();
    let moved = p.sections.iter().find(|x| x.id == intro.id).unwrap();
    assert_eq!(moved.start, 8 * BAR, "after Verse");
    let d = clips(&s, "Drums");
    let it = d.iter().find(|c| c.id == drums.id).unwrap();
    assert_eq!(it.start, 8 * BAR);
    assert_eq!(clips(&s, "Keys")[0].start, 0, "Keys was in Verse");
}

#[test]
fn a_sections_right_edge_changes_its_length_by_bars() {
    let (mut s, g) = studio();
    let verse = s.project().sections[1].clone();
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, verse.start + verse.length) - 2.0;
    drag!(s, g, (x, y), (x - bars(&s, 2.2), y), none());
    assert_eq!(s.project().sections[1].length, verse.length - 2 * BAR);
    // Shift: by beats.
    let x = s.tick_x(&g, verse.start + verse.length - 2 * BAR) - 2.0;
    drag!(s, g, (x, y), (x + bars(&s, 0.26), y), shift());
    assert_eq!(
        s.project().sections[1].length,
        verse.length - 2 * BAR + BAR / 4
    );
}

#[test]
fn a_sections_right_edge_lands_on_the_bar_lines_of_its_meter() {
    // UI-339: in 7/8 the edge lands on the 7/8 bar lines, and Shift on the
    // quarters counted from the start of the bar, as the grid draws them.
    let (mut s, g) = studio();
    let verse = s.project().sections[1].clone();
    let tm = s.project().time_map();
    let (bar, _, _) = tm.to_bbt(verse.start);
    assert_eq!(tm.bar_start(bar), verse.start);
    s.song
        .run("timesig.set", json!({"bar": bar, "num": 7, "den": 8}))
        .unwrap();
    let seven = 7 * 480;
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, verse.start + verse.length) - 2.0;
    let left = s.ticks_to_px(verse.length - 7 * seven) + s.ticks_to_px(seven) * 0.1;
    drag!(s, g, (x, y), (x - left, y), none());
    assert_eq!(s.project().sections[1].length, 7 * seven);
    let x = s.tick_x(&g, verse.start + 7 * seven) - 2.0;
    drag!(s, g, (x, y), (x + s.ticks_to_px(seven) * 0.3, y), shift());
    assert_eq!(s.project().sections[1].length, 7 * seven + 960);
}

#[test]
fn a_section_widened_into_the_next_stops_at_it_in_the_picture_too() {
    let (mut s, g) = studio();
    let verse = s.project().sections[1].clone();
    // Make room before the chorus, then pull the verse's edge far past it.
    let y = g.section_band.y + g.section_band.h / 2.0;
    let x = s.tick_x(&g, verse.start + verse.length) - 2.0;
    drag!(s, g, (x, y), (x - bars(&s, 2.0), y), none());
    let x = s.tick_x(&g, verse.start + verse.length - 2 * BAR) - 2.0;
    hold!(s, g, (x, y), (x + bars(&s, 6.0), y), none());
    let d = s.arrange.drag.clone().expect("dragging");
    let plan = s.plan(&g, &d).expect("a plan");
    assert!(
        matches!(plan.preview, Preview::SectionLength { length, .. } if length == verse.length),
        "{:?}",
        plan.preview
    );
    assert!(plan.tip.contains("次のセクションまで"), "{}", plan.tip);
    s.arrange_up(&g, d.at.0, d.at.1, none());
    assert_eq!(s.project().sections[1].length, verse.length);
}

#[test]
fn a_tempo_point_dragged_up_is_faster() {
    let (mut s, g) = studio();
    assert!(g.tempo.h > 0.0);
    let t = s.project().tempo[0].clone();
    let (x, y) = s.tempo_xy(&g, t.tick, t.bpm);
    assert_eq!(s.arrange_spot(&g, x, y), Spot::Tempo(0));
    drag!(s, g, (x, y), (x, y - 20.0), none());
    assert_eq!(s.project().tempo[0].bpm, t.bpm + 10.0);
    drag!(s, g, (x, y), (x, y + 5.0), shift());
    assert_eq!(s.project().tempo[0].bpm, t.bpm + 10.0 - 0.5);
}

#[test]
fn the_tempo_row_adds_and_deletes_points_with_a_double_click() {
    let (mut s, g) = studio();
    let n = s.project().tempo.len();
    let at = s.tick_x(&g, 4 * BAR);
    let y = g.tempo.y + 4.0;
    assert_eq!(s.arrange_spot(&g, at, y), Spot::TempoGap);
    click!(s, g, (at, y), none());
    assert_eq!(s.project().tempo.len(), n, "a single click adds nothing");
    s.arrange.clicks = 2;
    click!(s, g, (at, y), none());
    s.arrange.clicks = 1;
    assert_eq!(s.project().tempo.len(), n + 1);
    let added = s.project().tempo[n].clone();
    assert_eq!(added.tick, 4 * BAR);
    // The point under the pointer goes with a double-click; the first one stays.
    let (px, py) = s.tempo_xy(&g, added.tick, added.bpm);
    s.arrange.clicks = 2;
    click!(s, g, (px, py), none());
    assert_eq!(s.project().tempo.len(), n);
    let first = s.project().tempo[0].clone();
    let (fx, fy) = s.tempo_xy(&g, first.tick, first.bpm);
    click!(s, g, (fx, fy), none());
    s.arrange.clicks = 1;
    assert_eq!(s.project().tempo.len(), n, "the head point stays");
}

#[test]
fn split_and_mute_do_nothing_in_the_tempo_row() {
    let (mut s, g) = studio();
    let n = s.project().tempo.len();
    let at = s.tick_x(&g, 4 * BAR);
    let y = g.tempo.y + 4.0;
    for tool in [Tool::Cut, Tool::Mute] {
        s.tool = tool;
        s.arrange.clicks = 2;
        click!(s, g, (at, y), none());
        s.arrange.clicks = 1;
        assert_eq!(s.project().tempo.len(), n);
        s.arrange.hover = s.arrange_spot(&g, at, y);
        assert_eq!(super::paint::cursor(&s), Some(crate::roll::Cursor::Blocked));
    }
}

fn draw_shape(s: &mut Studio, name: &str) {
    s.song
        .set("tool.draw.shape", json!({ "shape": name }))
        .unwrap();
}

#[test]
fn the_pencil_adds_a_tempo_point_with_a_click_and_draws_the_shape_with_a_drag() {
    let (mut s, g) = studio();
    s.tool = Tool::Draw;
    let n = s.project().tempo.len();
    let at = s.tick_x(&g, 4 * BAR);
    let y = g.tempo.y + 4.0;
    click!(s, g, (at, y), none());
    assert_eq!(s.project().tempo.len(), n + 1, "a click lays a point");
    // A line over two bars, from fast to slow: ramps the tempo down, the head stays.
    draw_shape(&mut s, "line");
    let head = s.project().tempo[0].bpm;
    let (x0, x1) = (s.tick_x(&g, 8 * BAR), s.tick_x(&g, 10 * BAR));
    drag!(
        s,
        g,
        (x0, g.tempo.y + 10.0),
        (x1, g.tempo.bottom() - 10.0),
        none()
    );
    let t = &s.project().tempo;
    let a = t.iter().find(|p| p.tick == 8 * BAR).expect("start");
    let b = t.iter().find(|p| p.tick == 10 * BAR).expect("end");
    assert!(a.bpm > b.bpm, "{} > {}", a.bpm, b.bpm);
    assert_eq!(t[0].bpm, head, "the head does not move");
}

/// A lane of Bass with its points at 4, 6 and 8 bars, the pencil in hand.
fn pencil_lane() -> (Studio, Geometry, xirius_core::id::TrackId, f32) {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    s.tool = Tool::Draw;
    let row = *s.tree().row_of(RowKind::Track(bass)).unwrap();
    let top = s.lane_top(&g, &row, 0);
    (s, g, bass, top)
}

fn lane_points(s: &Studio, t: xirius_core::id::TrackId) -> Vec<(Tick, f64)> {
    s.project().tracks[&t].automation[0]
        .points
        .iter()
        .map(|p| (p.tick, p.value))
        .collect()
}

#[test]
fn the_pencil_in_a_lane_lays_a_point_with_a_click_and_a_line_with_a_drag() {
    let (mut s, g, bass, top) = pencil_lane();
    let n = steps(&s);
    let x = s.tick_x(&g, 12 * BAR);
    click!(s, g, (x, top + 30.0), none());
    assert_eq!(lane_points(&s, bass).len(), 4, "a click lays one point");
    assert_eq!(steps(&s), n + 1);
    draw_shape(&mut s, "line");
    let x1 = s.tick_x(&g, 14 * BAR);
    hold!(s, g, (x, top + 20.0), (x1, top + 60.0), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert_eq!(plan.command.as_ref().unwrap().0, "auto.draw");
    assert!(matches!(plan.preview, Preview::Stroke(ref l) if l.len() >= 2));
    s.arrange_up(&g, x1, top + 60.0, none());
    let pts = lane_points(&s, bass);
    let (a, b) = (
        pts.iter().find(|p| p.0 == 12 * BAR).expect("start"),
        pts.iter().find(|p| p.0 == 14 * BAR).expect("end"),
    );
    assert!(a.1 > b.1, "drawn from high to low: {a:?} {b:?}");
    assert_eq!(steps(&s), n + 2, "the stroke is one undoable step");
}

#[test]
fn a_freehand_stroke_keeps_its_path_and_alt_makes_a_wave_one_period_or_a_line_of_two_points() {
    let (mut s, g, bass, top) = pencil_lane();
    // Alt with a parabola: a straight line of two points.
    draw_shape(&mut s, "parabola");
    let (x0, x1) = (s.tick_x(&g, 9 * BAR), s.tick_x(&g, 10 * BAR));
    drag!(s, g, (x0, top + 20.0), (x1, top + 60.0), alt());
    let drawn = lane_points(&s, bass)
        .into_iter()
        .filter(|p| p.0 >= 9 * BAR && p.0 <= 10 * BAR)
        .count();
    assert_eq!(drawn, 2);
    // Alt with a wave: the whole stroke is one period.
    draw_shape(&mut s, "sine");
    let (x2, x3) = (s.tick_x(&g, 11 * BAR), s.tick_x(&g, 12 * BAR));
    hold!(s, g, (x2, top + 40.0), (x3, top + 40.0), alt());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let args = plan.command.unwrap().1;
    assert_eq!(args["period"], json!(BAR), "{args}");
    s.arrange_up(&g, x3, top + 40.0, alt());
    // Freehand keeps the path.
    draw_shape(&mut s, "free");
    let (x4, x5) = (s.tick_x(&g, 13 * BAR), s.tick_x(&g, 14 * BAR));
    s.arrange_down(&g, x4, top + 20.0, none());
    for i in 1..=8 {
        let f = i as f32 / 8.0;
        let y = top + 20.0 + 40.0 * (f * std::f32::consts::PI * 2.0).sin().abs();
        s.arrange_move(&g, x4 + (x5 - x4) * f, y, none());
    }
    s.arrange_up(&g, x5, top + 20.0, none());
    let free = lane_points(&s, bass)
        .into_iter()
        .filter(|p| p.0 >= 13 * BAR)
        .count();
    assert!(free >= 4, "{free}");
}

fn ctrl_alt() -> Mods {
    Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    }
}

/// The value the lane's stroke stands for at `tick`, worked out from the command's arguments.
fn wave_value(args: &Value, target: &AutoTarget, tick: Tick) -> f64 {
    let s = xirius_core::shape::Stroke {
        from: args["from"].as_i64().unwrap(),
        to: args["to"].as_i64().unwrap(),
        v0: args["value"].as_f64().unwrap(),
        v1: args["end-value"].as_f64().unwrap(),
        amp: args["amp"].as_f64().unwrap(),
        period: args["period"].as_f64().unwrap(),
        phase: args["phase"].as_f64().unwrap(),
    };
    let (lo, hi) = target.range();
    xirius_core::shape::value_at(xirius_core::shape::Shape::Sine, &s, tick as f64).clamp(lo, hi)
}

#[test]
fn mod_while_drawing_a_wave_shifts_its_phase_and_mod_alt_moves_it_in_time() {
    let (mut s, g, bass, top) = pencil_lane();
    let target = s.project().tracks[&bass].automation[0].target.clone();
    draw_shape(&mut s, "sine");
    let y = top + crate::tree::LANE_H / 2.0;
    // Out to the end of bar 12, then on a quarter note with Mod: the ends stay, the phase moves.
    let (x0, x1) = (s.tick_x(&g, 11 * BAR), s.tick_x(&g, 12 * BAR));
    let x2 = s.tick_x(&g, 12 * BAR + 240);
    s.arrange_down(&g, x0, y, none());
    s.arrange_move(&g, x1, y, none());
    s.arrange_move(&g, x2, y, ctrl());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let args = plan.command.clone().unwrap().1;
    assert_eq!(args["from"], json!(11 * BAR), "{args}");
    assert_eq!(args["to"], json!(12 * BAR), "{args}");
    let phase = args["phase"].as_i64().unwrap();
    assert!((phase - 240).abs() <= 2, "{args}");
    // The line shown while drawing is the one the command lays.
    let Preview::Stroke(line) = &plan.preview else {
        panic!("{:?}", plan.preview)
    };
    let head = top + lane_value_y(&target, wave_value(&args, &target, 11 * BAR));
    assert!((line[0].1 - head).abs() < 1.0, "{line:?} {head}");
    s.arrange_up(&g, x2, y, ctrl());
    let pts = lane_points(&s, bass);
    let at = |t: Tick| pts.iter().find(|p| p.0 == t).expect("drawn").1;
    for t in [11 * BAR, 12 * BAR] {
        assert!(
            (at(t) - wave_value(&args, &target, t)).abs() < 1e-6,
            "{pts:?}"
        );
    }
    let centre = args["value"].as_f64().unwrap();
    assert!(
        (at(11 * BAR) - centre).abs() > args["amp"].as_f64().unwrap() * 0.5,
        "a quarter of a period on, the wave starts away from its centre: {pts:?}"
    );
    // Mod+Alt on the way moves the whole stroke a bar later; the phase stays.
    let (x3, x4, x5) = (
        s.tick_x(&g, 14 * BAR),
        s.tick_x(&g, 15 * BAR),
        s.tick_x(&g, 16 * BAR),
    );
    s.arrange_down(&g, x3, y, none());
    s.arrange_move(&g, x4, y, none());
    s.arrange_move(&g, x5, y, ctrl_alt());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let args = plan.command.unwrap().1;
    assert_eq!(args["from"], json!(15 * BAR), "{args}");
    assert_eq!(args["to"], json!(16 * BAR), "{args}");
    assert_eq!(args["phase"], json!(0), "{args}");
    assert_eq!(
        args["period"],
        json!(960),
        "Mod+Alt is not Alt's one period: {args}"
    );
    s.arrange_up(&g, x5, y, ctrl_alt());
    let pts = lane_points(&s, bass);
    assert!(pts.iter().any(|p| p.0 == 15 * BAR) && pts.iter().any(|p| p.0 == 16 * BAR));
    assert!(
        !pts.iter().any(|p| p.0 >= 14 * BAR && p.0 < 15 * BAR - 1),
        "nothing is left where the stroke began: {pts:?}"
    );
}

#[test]
fn mod_at_the_press_still_puts_the_pencil_away_and_other_shapes_ignore_mod() {
    let (mut s, g, bass, top) = pencil_lane();
    draw_shape(&mut s, "sine");
    let y = top + crate::tree::LANE_H / 2.0;
    let (x0, x1) = (s.tick_x(&g, 11 * BAR), s.tick_x(&g, 12 * BAR));
    s.arrange_down(&g, x0, y, ctrl());
    // As before: Mod held at the press is the arrow, which draws a rectangle in the lane.
    assert_eq!(s.arrange.drag.as_ref().map(|d| d.tool), Some(Tool::Select));
    assert!(s.arrange.stroke.is_empty());
    s.arrange_up(&g, x0, y, ctrl());
    // A line does not read Mod: it runs from the press to the release.
    draw_shape(&mut s, "line");
    s.arrange_down(&g, x0, top + 20.0, none());
    s.arrange_move(&g, x1, top + 40.0, ctrl());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    let args = plan.command.unwrap().1;
    assert_eq!(
        (&args["from"], &args["to"]),
        (&json!(11 * BAR), &json!(12 * BAR))
    );
    assert!(args.get("phase").is_none(), "{args}");
    s.arrange_up(&g, x1, top + 40.0, ctrl());
    assert!(lane_points(&s, bass).iter().any(|p| p.0 == 12 * BAR));
}

// ---------------------------------------------------------------- tracks

#[test]
fn a_track_dragged_between_rows_goes_there_with_a_line_shown() {
    let (mut s, g) = studio();
    let keys = track(&s, "Keys");
    let rhythm = folder(&s, "Rhythm");
    let from = row_mid(&s, &g, RowKind::Track(keys));
    let y = row_at(&s, &g, RowKind::Track(track(&s, "Bass")), 0.2);
    hold!(s, g, from, (from.0, y), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(matches!(
        plan.preview,
        Preview::Drop {
            mark: DropMark::Line { .. },
            ..
        }
    ));
    assert_eq!(plan.command.as_ref().unwrap().0, "track.move");
    s.arrange_up(&g, from.0, y, none());
    assert_eq!(
        parent_of(&s, Node::Track(keys)),
        (Parent::Folder(rhythm), 1)
    );
}

#[test]
fn a_track_dropped_on_a_folder_goes_into_it_and_out_again() {
    let (mut s, g) = studio();
    let guitar = track(&s, "Guitar");
    let perc = folder(&s, "Percussion");
    let from = row_mid(&s, &g, RowKind::Track(guitar));
    let y = row_at(&s, &g, RowKind::Folder(perc), 0.5);
    hold!(s, g, from, (from.0, y), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(matches!(
        plan.preview,
        Preview::Drop {
            mark: DropMark::Into(f),
            ..
        } if f == perc
    ));
    s.arrange_up(&g, from.0, y, none());
    let (parent, i) = parent_of(&s, Node::Track(guitar));
    assert_eq!(parent, Parent::Folder(perc));
    assert_eq!(i, s.project().children(Parent::Folder(perc)).len() - 1);
    // Back out above the Keys row, at the root.
    let from = row_mid(&s, &g, RowKind::Track(guitar));
    let y = row_at(&s, &g, RowKind::Track(track(&s, "Keys")), 0.1);
    drag!(s, g, from, (from.0, y), none());
    assert_eq!(parent_of(&s, Node::Track(guitar)).0, Parent::Root);
}

#[test]
fn folders_nest_without_end_and_move_with_what_is_in_them() {
    let (mut s, g) = studio();
    let latin = folder(&s, "Latin");
    let conga = track(&s, "Conga");
    // Latin (inside Percussion inside Rhythm) out to the root, after Guitar.
    let from = row_mid(&s, &g, RowKind::Folder(latin));
    let guitar = *s
        .tree()
        .row_of(RowKind::Track(track(&s, "Guitar")))
        .unwrap();
    let y = s.row_y(&g, &guitar) + guitar.h - 2.0;
    drag!(s, g, from, (from.0, y), none());
    assert_eq!(parent_of(&s, Node::Folder(latin)).0, Parent::Root);
    assert_eq!(parent_of(&s, Node::Track(conga)).0, Parent::Folder(latin));
    // Into Latin goes Keys; Latin into Rhythm again: four levels deep.
    let from = row_mid(&s, &g, RowKind::Track(track(&s, "Keys")));
    let y = row_at(&s, &g, RowKind::Folder(latin), 0.5);
    drag!(s, g, from, (from.0, y), none());
    let perc = folder(&s, "Percussion");
    let from = row_mid(&s, &g, RowKind::Folder(latin));
    let y = row_at(&s, &g, RowKind::Folder(perc), 0.5);
    drag!(s, g, from, (from.0, y), none());
    let row = s.tree().row_of(RowKind::Track(track(&s, "Keys"))).copied();
    assert_eq!(row.unwrap().depth, 3);
}

#[test]
fn several_tracks_move_together_and_alt_copies_them() {
    let (mut s, g) = studio();
    let (vocal, guitar) = (track(&s, "Vocal"), track(&s, "Guitar"));
    let rhythm = folder(&s, "Rhythm");
    // Vocal, then Shift + Guitar.
    click!(s, g, row_mid(&s, &g, RowKind::Track(vocal)), none());
    let at = row_mid(&s, &g, RowKind::Track(guitar));
    down(&mut s, &g, at, shift());
    s.arrange_up(&g, 0.0, 0.0, shift());
    assert_eq!(s.arrange.tracks.len(), 2);
    let n = s.project().tracks.len();
    let steps0 = steps(&s);
    // Alt + drag both onto Rhythm: copies, the originals stay.
    let from = row_mid(&s, &g, RowKind::Track(guitar));
    let y = row_at(&s, &g, RowKind::Folder(rhythm), 0.5);
    drag!(s, g, from, (from.0, y), alt());
    assert_eq!(s.project().tracks.len(), n + 2);
    assert_eq!(parent_of(&s, Node::Track(vocal)).0, Parent::Root);
    assert_eq!(steps(&s), steps0 + 1);
    // Without Alt: both go to the top of the root, in their order.
    let from = row_mid(&s, &g, RowKind::Track(vocal));
    let rows = s.tree().rows.clone();
    let y = s.row_y(&g, &rows[0]) + 2.0;
    drag!(s, g, from, (from.0, y), none());
    let p = s.project();
    assert_eq!(p.root[0], Node::Track(vocal));
    assert_eq!(p.root[1], Node::Track(guitar));
}

// ---------------------------------------------------------------- automation

#[test]
fn the_lanes_button_opens_a_volume_lane_and_plus_adds_more() {
    let (mut s, g) = studio();
    let bass = track(&s, "Bass");
    let row = *s.tree().row_of(RowKind::Track(bass)).unwrap();
    let auto = RowParts::of(&row).auto.expect("an automation button");
    let at = (
        g.column.x + auto.x + auto.w / 2.0,
        s.row_y(&g, &row) + auto.y + auto.h / 2.0,
    );
    assert_eq!(s.arrange_spot(&g, at.0, at.1), Spot::AutoToggle(bass));
    click!(s, g, at, none());
    assert!(s.arrange.open.contains(&bass));
    let lanes = &s.project().tracks[&bass].automation;
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].target, AutoTarget::Volume);
    assert_eq!(s.tree().row_of(RowKind::Track(bass)).unwrap().lanes, 1);
    // + on the lane's head: a pan lane, both shown.
    let row = *s.tree().row_of(RowKind::Track(bass)).unwrap();
    let parts = LaneParts::of(&row, true);
    let top = s.lane_top(&g, &row, 0);
    let plus = (
        g.column.x + parts.add.x + parts.add.w / 2.0,
        top + parts.add.y + parts.add.h / 2.0,
    );
    click!(s, g, plus, none());
    assert!(s.popup.is_some());
    s.popup_action(
        &g,
        crate::popup::Action::AutomationTarget(bass, None, "pan".into()),
    );
    let p = s.project();
    let targets: Vec<_> = p.tracks[&bass]
        .automation
        .iter()
        .map(|l| l.target.clone())
        .collect();
    assert_eq!(targets, [AutoTarget::Volume, AutoTarget::Pan]);
    assert_eq!(s.tree().row_of(RowKind::Track(bass)).unwrap().lanes, 2);
    // The rows under Bass moved down by two lanes.
    let shaker = s
        .tree()
        .row_of(RowKind::Folder(folder(&s, "Percussion")))
        .unwrap()
        .y;
    assert_eq!(shaker, row.y + row.h + 2.0 * LANE_H);
    // The track head's icon cycles through the four modes, with the lanes shut too.
    let r = crate::automation_track_mode::rect(RowParts::of(&row).auto.unwrap());
    let at = (
        g.column.x + r.x + r.w / 2.0,
        s.row_y(&g, &row) + r.y + r.h / 2.0,
    );
    for want in [
        AutoMode::Touch,
        AutoMode::Latch,
        AutoMode::Write,
        AutoMode::Read,
    ] {
        click(&mut s, &g, at, none());
        assert_eq!(s.project().tracks[&bass].auto_mode, want);
    }
    // The name button gives the second lane something not yet shown.
    let parts2 = LaneParts::of(&row, false);
    let top2 = s.lane_top(&g, &row, 1);
    click(
        &mut s,
        &g,
        (
            g.column.x + parts2.param.x + 10.0,
            top2 + parts2.param.y + 4.0,
        ),
        none(),
    );
    assert!(s.popup.is_some());
    s.popup_action(
        &g,
        crate::popup::Action::AutomationTarget(bass, Some("pan".into()), "mute".into()),
    );
    let second = s.project().tracks[&bass].automation[1].target.clone();
    assert!(
        !matches!(second, AutoTarget::Volume | AutoTarget::Pan),
        "{second:?}"
    );
    // × folds one lane away and leaves the other.
    let close = (
        g.column.x + parts2.remove.x + parts2.remove.w / 2.0,
        top2 + parts2.remove.y + parts2.remove.h / 2.0,
    );
    click!(s, g, close, none());
    assert_eq!(s.tree().row_of(RowKind::Track(bass)).unwrap().lanes, 1);
    assert!(s.arrange.open.contains(&bass));
}

#[test]
fn every_kind_of_target_can_have_a_lane() {
    let (s, _) = studio();
    let p = s.project();
    let keys = &p.tracks[&track(&s, "Keys")];
    let all = targets_of(keys);
    assert!(all.contains(&AutoTarget::Volume));
    assert!(all.contains(&AutoTarget::Pan));
    assert!(all.contains(&AutoTarget::Mute));
}

#[test]
fn points_are_added_dragged_and_bent_in_single_steps() {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    let lane = |s: &Studio| s.project().tracks[&bass].automation[0].clone();
    assert_eq!(lane(&s).points.len(), 3);
    let n = steps(&s);
    // A double-click on the empty lane puts a point there (one click is a marquee).
    let row = *s.tree().row_of(RowKind::Track(bass)).unwrap();
    let top = s.lane_top(&g, &row, 0);
    let x = s.tick_x(&g, 10 * BAR + 50);
    assert!(matches!(
        s.arrange_spot(&g, x, top + 30.0),
        Spot::Lane { .. }
    ));
    click!(s, g, (x, top + 30.0), none());
    assert_eq!(lane(&s).points.len(), 3, "a single click only selects");
    s.arrange.clicks = 2;
    click!(s, g, (x, top + 30.0), none());
    s.arrange.clicks = 1;
    assert_eq!(lane(&s).points.len(), 4);
    assert_eq!(lane(&s).points[3].tick, 10 * BAR, "on the grid");
    assert_eq!(steps(&s), n + 1);
    // Dragging a point: later and higher, with its value in the balloon.
    let (px, py) = point_xy(&s, &g, bass, 0, 1);
    let v = lane(&s).points[1].value;
    hold!(s, g, (px, py), (px + bars(&s, 1.0), py - 10.0), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(plan.tip.contains("dB"), "{}", plan.tip);
    s.arrange_up(&g, px + bars(&s, 1.0), py - 10.0, none());
    let p = lane(&s).points[1];
    assert_eq!(p.tick, 6 * BAR + BAR);
    assert!(p.value > v);
    assert_eq!(steps(&s), n + 2);
    // Crossing neighbours pushes them while retaining their values.
    let (px, py) = point_xy(&s, &g, bass, 0, 1);
    drag!(s, g, (px, py), (px + bars(&s, 9.0), py), none());
    assert_eq!(lane(&s).points[1].tick, 16 * BAR);
    assert_eq!(lane(&s).points[2].tick, 16 * BAR + 1);
    assert_eq!(lane(&s).points[3].tick, 16 * BAR + 2);
    // The line between points 0 and 1, dragged down: bent.
    let (x0, _) = point_xy(&s, &g, bass, 0, 0);
    let (x1, _) = point_xy(&s, &g, bass, 0, 1);
    let x = (x0 + x1) / 2.0;
    let l = lane(&s);
    let y = top + lane_value_y(&l.target, l.value_at(s.tick_at(&g, x) as f64).unwrap());
    assert!(matches!(
        s.arrange_spot(&g, x, y),
        Spot::Line { index: 0, .. }
    ));
    drag!(s, g, (x, y), (x, y + 18.0), none());
    assert!(lane(&s).points[0].curve != 0.0);
    // A double-click on a point takes it away.
    let k = lane(&s).points.len();
    s.arrange.clicks = 2;
    click!(s, g, point_xy(&s, &g, bass, 0, 0), none());
    s.arrange.clicks = 1;
    assert_eq!(lane(&s).points.len(), k - 1);
}

#[test]
fn resting_on_a_volume_point_shows_its_value_in_db() {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    let (x, y) = super::probe::point_xy(&s, &g, bass, 0, 0);
    assert!(s.point_tip(&g).is_none());
    s.arrange_move(&g, x, y, none());
    let (at, tip) = s.point_tip(&g).expect("a balloon on the point");
    assert_eq!(at, (x, y));
    assert!(tip.ends_with(" dB"), "{tip}");
}

#[test]
fn clips_cannot_be_taken_while_their_lanes_show_unless_set_otherwise() {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    assert!(s.arrange.open.contains(&bass));
    let from = clip_mid(&s, &g, "Bass", 0);
    let before = clips(&s, "Bass")[0].start;
    let n = steps(&s);
    // Locked (the default): the press is not on the clip, nothing moves.
    assert!(!matches!(
        s.arrange_spot(&g, from.0, from.1),
        Spot::Clip { .. }
    ));
    drag!(s, g, from, (from.0 + bars(&s, 2.0), from.1), none());
    assert_eq!(clips(&s, "Bass")[0].start, before);
    assert_eq!(steps(&s), n);
    // The clip shows why: a lock before its name.
    assert_eq!(crate::draw::clip_badge(&s, bass), Some("lock"));

    // Unlocked: the clip moves, and the points under it move with it.
    s.arrange.locks_clips = false;
    assert_eq!(crate::draw::clip_badge(&s, bass), None);
    let points = |s: &Studio| s.project().tracks[&bass].automation[0].points.clone();
    let clip = clips(&s, "Bass")[0].clone();
    let under: Vec<Tick> = points(&s)
        .iter()
        .map(|p| p.tick)
        .filter(|t| (clip.start..clip.start + clip.length).contains(t))
        .collect();
    assert!(!under.is_empty(), "the sample has points under the clip");
    drag!(s, g, from, (from.0 + bars(&s, 2.0), from.1), none());
    assert_eq!(clips(&s, "Bass")[0].start, before + 2 * BAR);
    for t in &under {
        assert!(points(&s).iter().any(|p| p.tick == t + 2 * BAR), "{t}");
    }

    // Not following: the clip moves and the points stay.
    s.undo(&g);
    s.arrange.follows_clips = false;
    let kept = points(&s);
    drag!(s, g, from, (from.0 + bars(&s, 2.0), from.1), none());
    assert_eq!(clips(&s, "Bass")[0].start, before + 2 * BAR);
    assert_eq!(points(&s), kept);
}

#[test]
fn lane_values_play_and_the_lanes_survive_saving() {
    let (mut s, g) = studio();
    let bass = two_lanes(&mut s, &g, "Bass");
    let p = s.project();
    let json = serde_json::to_string(&*p).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.tracks[&bass].automation, p.tracks[&bass].automation);
    assert_eq!(back.tracks[&bass].auto_mode, AutoMode::Read);
    let vol = &p.tracks[&bass].automation[0];
    assert_eq!(vol.value_at((6 * BAR) as f64), Some(0.0));
}

// ---------------------------------------------------------------- zoom

#[test]
fn ctrl_wheel_makes_tracks_taller_around_the_pointer() {
    let (mut s, g) = studio();
    let keys = RowKind::Track(track(&s, "Keys"));
    let y = row_at(&s, &g, keys, 0.5);
    let x = g.lanes.x + 300.0;
    let h = s.arrange.track_h;
    assert!(s.arrange_wheel(&g, x, y, 3.0, ctrl()));
    assert_eq!(s.arrange.track_h, h + 3.0 * Arrange::TRACK_H_STEP);
    let row = *s.tree().row_of(keys).unwrap();
    assert_eq!(row.h, s.arrange.track_h);
    let after = s.row_y(&g, &row) + row.h * 0.5;
    assert!((after - y).abs() < 1.0 || s.scroll.1 == 0.0, "{after} {y}");
    // Without Ctrl the wheel is not ours.
    assert!(!s.arrange_wheel(&g, x, y, 1.0, none()));
    // Never past the limits.
    s.arrange_wheel(&g, x, y, -100.0, ctrl());
    assert_eq!(s.arrange.track_h, Arrange::TRACK_H_MIN);
}

#[test]
fn ctrl_shift_wheel_zooms_time_around_the_pointer() {
    let (mut s, g) = studio();
    let x = g.lanes.x + 400.0;
    let y = g.lanes.y + 40.0;
    let tick = s.tick_at(&g, x);
    let w = s.beat_w;
    let both = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
    assert!(s.arrange_wheel(&g, x, y, 2.0, both));
    assert!(s.beat_w > w);
    assert!(
        (s.tick_at(&g, x) - tick).abs() <= 40,
        "{} {tick}",
        s.tick_at(&g, x)
    );
}

#[test]
fn zoom_stops_at_the_limits_and_one_beat_can_fill_the_view() {
    let (mut s, g) = studio();
    let x = g.lanes.x + 400.0;
    let y = g.lanes.y + 40.0;
    let both = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };
    s.arrange_wheel(&g, x, y, 1000.0, both);
    assert_eq!(s.beat_w, s.beat_w_max());
    // One beat is wider than a 4K window's lanes.
    assert!(s.beat_w >= 3840.0);
    s.arrange_wheel(&g, x, y, -1000.0, both);
    assert_eq!(s.beat_w, Studio::BEAT_W_MIN);
    let keys = RowKind::Track(track(&s, "Keys"));
    let row_y = row_at(&s, &g, keys, 0.5);
    s.arrange_wheel(&g, x, row_y, 1000.0, ctrl());
    assert_eq!(s.arrange.track_h, Arrange::TRACK_H_MAX);
}

// ---------------------------------------------------------------- what is shown

#[test]
fn every_change_the_picture_shows_bumps_the_generation() {
    let (mut s, g) = studio();
    let from = clip_mid(&s, &g, "Keys", 0);
    let a = s.arrange.generation;
    s.arrange_move(&g, from.0, from.1, none());
    let b = s.arrange.generation;
    assert_ne!(a, b, "hover");
    hold!(s, g, from, (from.0 + 40.0, from.1), none());
    let c = s.arrange.generation;
    s.arrange_move(&g, from.0 + 60.0, from.1, none());
    assert_ne!(c, s.arrange.generation, "drag");
}

#[test]
fn the_pointer_shape_says_what_a_press_would_do() {
    let (mut s, g) = studio();
    let at = clip_mid(&s, &g, "Keys", 0);
    s.tool = Tool::Cut;
    s.arrange_move(&g, at.0, at.1, none());
    assert_eq!(paint::cursor(&s), Some(Cursor::Split));
    s.tool = Tool::Select;
    let y = g.ruler.y + 4.0;
    s.arrange_move(&g, g.lanes.x + 100.0, y, none());
    assert_eq!(paint::cursor(&s), None, "the ruler's own");
    let t = s.project().tempo[0].clone();
    let (x, y) = s.tempo_xy(&g, t.tick, t.bpm);
    s.arrange_move(&g, x, y, none());
    assert_eq!(paint::cursor(&s), Some(Cursor::ResizeV));
}

/// A place on the lanes where neither Keys nor Vocal has a part.
fn free_x(s: &Studio, g: &Geometry) -> f32 {
    (0..60)
        .map(|bar| bar * BAR + BAR / 2)
        .find(|t| {
            let x = s.tick_x(g, *t);
            g.lanes.contains(x, g.lanes.y + 1.0)
                && ["Keys", "Vocal"].iter().all(|name| {
                    clips(s, name)
                        .iter()
                        .all(|c| *t < c.start || *t >= c.start + c.length)
                })
        })
        .map(|t| s.tick_x(g, t))
        .expect("a free place in view")
}

/// The pointer over `at` with `tool` held, as the window asks the system for it.
fn pointer_at(s: &mut Studio, g: &Geometry, tool: Tool, at: (f32, f32)) -> crate::pointer::Pointer {
    s.tool = tool;
    s.arrange_move(g, at.0, at.1, none());
    crate::pointer::of_studio(s, g)
}

#[test]
fn each_tool_shows_its_own_picture_on_a_part_and_not_allowed_where_there_is_none() {
    use crate::pointer::Shape;
    use gpui::CursorStyle;
    let (mut s, g) = studio();
    let on_clip = clip_mid(&s, &g, "Keys", 0);
    let x_free = free_x(&s, &g);
    let on_keys = (
        x_free,
        row_at(&s, &g, RowKind::Track(track(&s, "Keys")), 0.5),
    );
    let on_vocal = (
        x_free,
        row_at(&s, &g, RowKind::Track(track(&s, "Vocal")), 0.5),
    );
    assert!(
        g.lanes.contains(on_keys.0, on_keys.1),
        "the empty place is in view"
    );
    for (tool, shape) in [
        (Tool::Cut, Shape::Scissors),
        (Tool::Eraser, Shape::Eraser),
        (Tool::Mute, Shape::Mute),
        (Tool::Audition, Shape::Audition),
        (Tool::Draw, Shape::Pencil),
        (Tool::Range, Shape::Range),
    ] {
        let p = pointer_at(&mut s, &g, tool, on_clip);
        assert_eq!(p.shape, Some(shape), "{tool:?} on a part");
        assert_ne!(
            p.style,
            CursorStyle::OperationNotAllowed,
            "{tool:?} on a part"
        );
    }
    // Nothing to cut, erase, mute or hear: the system's "not allowed", and no picture.
    for tool in [Tool::Cut, Tool::Eraser, Tool::Mute, Tool::Audition] {
        for at in [on_keys, on_vocal] {
            let p = pointer_at(&mut s, &g, tool, at);
            assert_eq!(
                p.style,
                CursorStyle::OperationNotAllowed,
                "{tool:?} on empty"
            );
            assert_eq!(p.shape, None, "{tool:?} on empty");
        }
    }
    // The pencil makes a part on an instrument track only.
    let p = pointer_at(&mut s, &g, Tool::Draw, on_keys);
    assert_eq!(p.shape, Some(Shape::Pencil));
    let p = pointer_at(&mut s, &g, Tool::Draw, on_vocal);
    assert_eq!(
        p.style,
        CursorStyle::OperationNotAllowed,
        "the pencil on an audio track"
    );
    assert_eq!(p.shape, None);
    // The range tool and the arrow tool need nothing under them.
    assert_eq!(
        pointer_at(&mut s, &g, Tool::Range, on_vocal).shape,
        Some(Shape::Range)
    );
    let p = pointer_at(&mut s, &g, Tool::Select, on_vocal);
    assert_eq!((p.style, p.shape), (CursorStyle::Arrow, None));
    // Ctrl with the arrow tool is the range tool.
    s.arrange_move(&g, on_vocal.0, on_vocal.1, ctrl());
    assert_eq!(crate::pointer::of_studio(&s, &g).shape, Some(Shape::Range));
}

#[test]
fn the_crosshair_setting_swaps_every_pictures_for_the_systems_crosshair() {
    use crate::pointer::Shape;
    use gpui::CursorStyle;
    let (mut s, g) = studio();
    let on_clip = clip_mid(&s, &g, "Keys", 0);
    assert!(!s.crosshair, "the tool's own picture is the default");
    assert_eq!(
        pointer_at(&mut s, &g, Tool::Cut, on_clip).shape,
        Some(Shape::Scissors)
    );
    s.song
        .run(
            "prefs.set",
            json!({"key":"edit.cursor.crosshair","value":"true"}),
        )
        .unwrap();
    s.apply_edit_prefs();
    assert!(s.crosshair);
    for tool in [
        Tool::Cut,
        Tool::Eraser,
        Tool::Mute,
        Tool::Audition,
        Tool::Draw,
        Tool::Range,
    ] {
        let p = pointer_at(&mut s, &g, tool, on_clip);
        assert_eq!(
            (p.style, p.shape),
            (CursorStyle::Crosshair, None),
            "{tool:?}"
        );
    }
    // "Not allowed" is not a tool's picture, so it stays.
    let x_free = free_x(&s, &g);
    let on_vocal = (
        x_free,
        row_at(&s, &g, RowKind::Track(track(&s, "Vocal")), 0.5),
    );
    assert_eq!(
        pointer_at(&mut s, &g, Tool::Cut, on_vocal).style,
        CursorStyle::OperationNotAllowed
    );
}

// ---------------------------------------------------------------- ripple editing (EDT-135)

#[test]
fn edt_135_the_ripple_switch_and_its_key_make_a_delete_close_up_and_a_longer_clip_push() {
    use crate::topbar::BarButton;
    let (mut s, g) = studio();
    // The arrangement has the keys, as when it is clicked.
    s.choose_editor();
    let t = track(&s, "Keys");
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    // Two more clips after the first on the same track, a bar apart.
    let first = clips(&s, "Keys")[0].clone();
    let end = first.start + first.length;
    for at in [end + BAR, end + 3 * BAR] {
        s.song
            .run(
                "clip.add.midi",
                json!({"track": t.to_string(), "at": at, "length": BAR}),
            )
            .unwrap();
    }
    let starts = |s: &Studio| {
        let mut v: Vec<_> = clips(s, "Keys").iter().map(|c| c.start).collect();
        v.sort_unstable();
        v
    };
    let others = |s: &Studio| {
        s.project()
            .tracks
            .values()
            .filter(|x| x.id != t)
            .map(|x| x.clips.iter().map(|c| c.start).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    let was = starts(&s);
    let theirs = others(&s);

    // The switch is in the arrangement's bar, left of the grid; a press turns the setting on.
    let (x, y) = s.center_of(&g, Hit::Bar(BarButton::Ripple)).unwrap();
    assert_eq!(s.hit(&g, x, y), Hit::Bar(BarButton::Ripple));
    let l = s.bar_layout(g.size.0);
    assert!(l.ripple.right() < l.grid.x);
    assert!(!s.bar_view().ripple);
    assert!(s.press(&g, Hit::Bar(BarButton::Ripple)));
    assert!(s.bar_view().ripple);
    assert!(s.song.settings().lock().unwrap().prefs.bool("edit.ripple"));

    // A longer first clip pushes the two after it by as much; one step of the history.
    let r = s.clip_rect(&g, &row, &first);
    let y = r.y + r.h / 2.0;
    let n = steps(&s);
    drag!(
        s,
        g,
        (r.right() - 2.0, y),
        (r.right() + bars(&s, 1.1), y),
        none()
    );
    assert_eq!(steps(&s), n + 1);
    let pushed: Vec<_> = was
        .iter()
        .map(|&x| if x >= end { x + BAR } else { x })
        .collect();
    assert_eq!(starts(&s), pushed);

    // Delete on the selected first clip closes up the track; the other tracks stay.
    let at_clip = clip_mid(&s, &g, "Keys", 0);
    click(&mut s, &g, at_clip, none());
    let gone = clips(&s, "Keys")
        .into_iter()
        .find(|c| c.id == first.id)
        .unwrap();
    assert!(s.arrange_key(&g, "delete", none()));
    let closed: Vec<_> = pushed
        .iter()
        .filter(|&&x| x != gone.start)
        .map(|&x| {
            if x >= gone.start + gone.length {
                x - gone.length
            } else {
                x
            }
        })
        .collect();
    assert_eq!(starts(&s), closed);
    assert_eq!(others(&s), theirs);

    // Ctrl+Alt+R turns it off: the next delete leaves the gap.
    let ctrl_alt = Mods {
        ctrl: true,
        alt: true,
        ..Mods::default()
    };
    assert!(s.mapped_key(&g, "r", None, ctrl_alt));
    assert!(!s.bar_view().ripple);
    let at_clip = clip_mid(&s, &g, "Keys", 0);
    click(&mut s, &g, at_clip, none());
    let left = starts(&s);
    let picked = s.arrange.clips.iter().next().unwrap().1;
    let at = clips(&s, "Keys")
        .iter()
        .find(|c| c.id == picked)
        .unwrap()
        .start;
    assert!(s.arrange_key(&g, "delete", none()));
    let kept: Vec<_> = left.iter().copied().filter(|&x| x != at).collect();
    assert_eq!(starts(&s), kept);
    assert!(s.error.is_none(), "{:?}", s.error);
}

#[test]
fn edt_135_the_rolls_bar_has_no_ripple_switch() {
    let l = crate::topbar::BarLayout::of(1440.0, "曲", false);
    assert_eq!(l.ripple, Rect::default());
    let a = crate::topbar::BarLayout::of(1440.0, "曲", true);
    assert!(a.ripple.w > 0.0 && a.ripple.right() < a.grid.x);
    // With the transport in the bottom bar (UI-300), it stands at every width.
    for w in [
        960.0, 1200.0, 1240.0, 1280.0, 1400.0, 1440.0, 1600.0, 1920.0, 2200.0,
    ] {
        let l = crate::topbar::BarLayout::of(w, "夜の散歩", true);
        assert!(l.ripple.w > 0.0 && l.ripple.right() < l.grid.x, "at {w}");
    }
}

#[test]
fn the_value_tip_setting_hides_the_drag_balloon() {
    let (mut s, g) = studio();
    let from = clip_mid(&s, &g, "Keys", 0);
    hold!(s, g, from, (from.0 + bars(&s, 2.3), from.1), none());
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(s.drag_tip(&plan).starts_with("7."), "{}", plan.tip);
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "edit.drag.value-tip", "value": "false"}),
    );
    s.apply_edit_prefs();
    let plan = s.plan(&g, s.arrange.drag.as_ref().unwrap()).unwrap();
    assert!(plan.tip.starts_with("7."), "{}", plan.tip);
    assert_eq!(s.drag_tip(&plan), "");
}

#[test]
fn a_click_on_empty_space_moves_the_playhead_only_with_the_setting() {
    let (mut s, g) = studio();
    let y = row_at(&s, &g, RowKind::Track(track(&s, "Keys")), 0.5);
    let at = s.playhead;
    // Keys has a clip from bar 4 to 8; past it the lane is empty. The setting is on at first.
    assert_ne!(at, 10 * BAR);
    s.apply_edit_prefs();
    click!(s, g, (s.tick_x(&g, 12 * BAR), y), none());
    assert_eq!(s.playhead, 12 * BAR);
    s.rec_set(
        "prefs.set",
        serde_json::json!({"key": "edit.click-empty.locate", "value": "false"}),
    );
    s.apply_edit_prefs();
    click!(s, g, (s.tick_x(&g, 10 * BAR), y), none());
    assert_eq!(s.playhead, 12 * BAR);
}
