//! The recording area under a track through the real press path (REC-002, REC-005, REC-006,
//! REC-014, REC-015): the takes of a passage in rows under their track, a bar pressed to play
//! that take there, and the input gain of an armed track. No window, no device, no sound.

use serde_json::json;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::Clip;
use xirius_core::time::Tick;

use crate::arrange::Spot;
use crate::rec_area::{COMP_H, INPUT_H, RecPart, TAKE_H};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const BAR: Tick = 3840;

fn studio() -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.drawer_open = false;
    let g = s.geometry(1440.0, 1000.0);
    s.resized(&g);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .expect("a track")
        .id
}

fn clips(s: &Studio, t: TrackId) -> Vec<Clip> {
    s.project().tracks[&t].clips.clone()
}

/// Three takes of two bars each, from bar 40 on (where the sample song has nothing), with their
/// rows open under the track.
fn record_three_takes(s: &mut Studio, t: TrackId) -> (Tick, Vec<ClipId>) {
    let taken = record_takes_folded(s, t);
    open_takes(s, t);
    taken
}

/// Opens the Comp and take rows of `t`, as the clip's count of takes does (UI-296).
fn open_takes(s: &mut Studio, t: TrackId) {
    if !s.takes_open.contains(&t) {
        s.rec_down(t, RecPart::Fold);
    }
}

/// The three takes, their rows folded as a recording leaves them (UI-296).
fn record_takes_folded(s: &mut Studio, t: TrackId) -> (Tick, Vec<ClipId>) {
    let start = s.project().time_map().bar_start(40);
    let mut ids = Vec::new();
    let ok = s.command(|song| {
        let d = song.run(
            "record.take.add",
            json!({"takes": [
                {"track": t.to_string(), "start": start, "length": 2 * BAR, "file": "Recordings/v-1.wav"},
                {"track": t.to_string(), "start": start, "length": 2 * BAR, "file": "Recordings/v-2.wav"},
                {"track": t.to_string(), "start": start, "length": 2 * BAR, "file": "Recordings/v-3.wav"},
            ]}),
        )?;
        for v in d["clips"].as_array().expect("clips") {
            ids.push(v.as_str().unwrap().parse().unwrap());
        }
        Ok(())
    });
    assert!(ok && s.error.is_none(), "{:?}", s.error);
    (start, ids)
}

/// Scrolls so that the takes at `start` and the track's rows are on screen.
fn show(s: &mut Studio, g: &Geometry, t: TrackId, start: Tick) {
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    s.scroll.1 = row.y;
    s.scroll.0 = (s.ticks_to_px(start) - 40.0).max(0.0);
    let _ = g;
}

/// The middle of bar `bar` (0-based from `start`) of take row `j`.
fn bar_mid(s: &Studio, g: &Geometry, t: TrackId, start: Tick, bar: Tick, j: usize) -> (f32, f32) {
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    let area = crate::rec_area::RecArea::of(&s.project().tracks[&t], s.takes_open.contains(&t));
    (
        s.tick_x(g, start + bar * BAR + BAR / 2),
        s.row_y(g, &row) + (row.rec_y() - row.y) + area.take_y(j) + TAKE_H / 2.0,
    )
}

fn click(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    assert!(
        g.lanes.contains(at.0, at.1) || g.column.contains(at.0, at.1),
        "{at:?}"
    );
    assert!(s.arrange_down(g, at.0, at.1, Mods::default()));
    s.arrange_up(g, at.0, at.1, Mods::default());
    assert_eq!(s.error, None);
}

/// What plays at `tick`: the file of every unmuted clip over it.
fn playing(s: &Studio, t: TrackId, tick: Tick) -> Vec<String> {
    clips(s, t)
        .iter()
        .filter(|c| !c.muted && c.start <= tick && tick < c.start + c.length)
        .filter_map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, .. } => Some(file.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn takes_hang_under_their_track_as_a_comp_row_and_a_row_each() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let before = s.tree().height;
    record_three_takes(&mut s, vocal);
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    assert_eq!(row.rec_h, COMP_H + 3.0 * TAKE_H);
    assert_eq!(s.tree().height, before + row.rec_h);
    // The rows under it moved down by the area, and the area belongs to its track.
    let (start, _) = (s.project().time_map().bar_start(40), 0);
    show(&mut s, &g, vocal, start);
    let at = bar_mid(&s, &g, vocal, start, 0, 2);
    assert!(matches!(
        s.arrange_spot(&g, at.0, at.1),
        Spot::Rec { track, part: RecPart::Bar { .. } } if track == vocal
    ));
}

#[test]
fn only_the_take_that_plays_carries_the_count_of_takes() {
    let (mut s, _g) = studio();
    let vocal = track(&s, "Vocal");
    let (_, ids) = record_three_takes(&mut s, vocal);
    let p = s.project();
    let stacks = xirius_core::takes::stacks(&p.tracks[&vocal]);
    let counted: Vec<_> = p.tracks[&vocal]
        .clips
        .iter()
        .filter(|c| ids.contains(&c.id))
        .map(|c| (c.id, c.muted, crate::draw::takes_under(&stacks, c)))
        .collect();
    assert_eq!(counted.len(), ids.len());
    for (_, muted, n) in &counted {
        assert_eq!(*n, if *muted { None } else { Some(3) });
    }
    assert_eq!(counted.iter().filter(|c| c.2.is_some()).count(), 1);
    // One take over nothing is not a stack and carries no count.
    let other = track(&s, "Keys");
    let p = s.project();
    assert!(p.tracks[&other].clips.iter().all(|c| {
        crate::draw::takes_under(&xirius_core::takes::stacks(&p.tracks[&other]), c).is_none()
    }));
}

/// Presses at `from` and drags to `to`, then lets go.
fn drag(s: &mut Studio, g: &Geometry, from: (f32, f32), to: (f32, f32)) {
    assert!(s.arrange_down(g, from.0, from.1, Mods::default()));
    s.arrange_move(g, to.0, to.1, Mods::default());
    s.arrange_up(g, to.0, to.1, Mods::default());
    assert_eq!(s.error, None);
}

/// `bar_mid`, but at `tick` of the row `j`.
fn row_at(s: &Studio, g: &Geometry, t: TrackId, tick: Tick, j: usize) -> (f32, f32) {
    let at = bar_mid(s, g, t, 0, 0, j);
    (s.tick_x(g, tick), at.1)
}

#[test]
fn pressing_a_take_plays_it_over_the_stretch_between_dividers_in_one_undoable_step() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let (start, ids) = record_three_takes(&mut s, vocal);
    show(&mut s, &g, vocal, start);
    // The last take plays, as a recording of laps leaves it.
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-3.wav"]);

    let steps = s.song.steps();
    let at = bar_mid(&s, &g, vocal, start, 1, 0);
    // No divider inside: the whole stretch of the stack, not the bar under the pointer.
    assert_eq!(
        s.arrange_spot(&g, at.0, at.1),
        Spot::Rec {
            track: vocal,
            part: RecPart::Bar {
                take: ids[0],
                from: start,
                to: start + 2 * BAR
            }
        }
    );
    click(&mut s, &g, at);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-1.wav"]);
    assert_eq!(
        playing(&s, vocal, start + BAR + 10),
        vec!["Recordings/v-1.wav"]
    );
    assert_eq!(
        s.comp_sel,
        Some(crate::rec_area::CompSel {
            track: vocal,
            from: start,
            to: start + 2 * BAR
        })
    );
    // The rows are still three takes.
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    assert_eq!(row.rec_h, COMP_H + 3.0 * TAKE_H);
    s.undo(&g);
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-3.wav"]);
}

#[test]
fn dragging_along_a_take_plays_it_over_the_snapped_stretch_and_a_press_then_takes_the_rest() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let (start, _) = record_three_takes(&mut s, vocal);
    show(&mut s, &g, vocal, start);
    let steps = s.song.steps();
    // One drag along take 2, from just inside the first bar to just inside the second.
    let from = row_at(&s, &g, vocal, start + 20, 1);
    let to = row_at(&s, &g, vocal, start + BAR + 20, 1);
    assert!(s.arrange_down(&g, from.0, from.1, Mods::default()));
    s.arrange_move(&g, to.0, to.1, Mods::default());
    // Nothing happens until the button comes up; the band shows the stretch.
    assert_eq!(s.song.steps(), steps);
    let plan = s
        .plan(&g, s.arrange.drag.as_ref().expect("a drag"))
        .expect("a plan");
    assert!(matches!(
        plan.preview,
        crate::arrange::Preview::Range { start: a, end: b, .. } if a == start && b == start + BAR
    ));
    s.arrange_up(&g, to.0, to.1, Mods::default());
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-2.wav"]);
    assert_eq!(
        playing(&s, vocal, start + BAR + 10),
        vec!["Recordings/v-3.wav"]
    );
    // A press on take 1 in the second bar now takes the stretch between the dividers: bar 2.
    let at = row_at(&s, &g, vocal, start + BAR + 500, 0);
    click(&mut s, &g, at);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-2.wav"]);
    assert_eq!(
        playing(&s, vocal, start + BAR + 10),
        vec!["Recordings/v-1.wav"]
    );
    // A drag that stays within one snap step does nothing at all (it moves up and down only).
    let steps = s.song.steps();
    let a = row_at(&s, &g, vocal, start + 20, 2);
    drag(&mut s, &g, a, (a.0, a.1 + 5.0));
    assert_eq!(s.song.steps(), steps);
}

#[test]
fn the_arrow_keys_switch_the_take_and_move_the_stretch() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let (start, _) = record_three_takes(&mut s, vocal);
    show(&mut s, &g, vocal, start);
    let none = Mods::default();
    // Without a stretch chosen the keys are not the comp's.
    assert!(!s.comp_key("up", none));
    // Take 2 over bar 1, then take 1 over the rest.
    let from = row_at(&s, &g, vocal, start + 20, 1);
    let to = row_at(&s, &g, vocal, start + BAR + 20, 1);
    drag(&mut s, &g, from, to);
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-2.wav"]);
    // Down: take 3 plays the stretch; up twice: take 1.
    let steps = s.song.steps();
    assert!(s.comp_key("down", none));
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-3.wav"]);
    assert!(s.comp_key("up", none));
    assert!(s.comp_key("up", none));
    assert_eq!(playing(&s, vocal, start + 10), vec!["Recordings/v-1.wav"]);
    // One key press is one step; the top row has no row above it.
    assert_eq!(s.song.steps(), steps + 3);
    assert!(s.comp_key("up", none));
    assert_eq!(s.song.steps(), steps + 3);
    // Right moves to the stretch after the dividers, the second bar; Shift widens the end.
    assert!(s.comp_key("right", none));
    assert_eq!(
        s.comp_sel.map(|c| (c.from, c.to)),
        Some((start + BAR, start + 2 * BAR))
    );
    assert!(s.comp_key("left", none));
    assert_eq!(
        s.comp_sel.map(|c| (c.from, c.to)),
        Some((start, start + BAR))
    );
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    assert!(s.comp_key("right", shift));
    assert_eq!(
        s.comp_sel.map(|c| (c.from, c.to)),
        Some((start, start + 2 * BAR))
    );
    // Moving the stretch changes nothing in the song.
    assert_eq!(s.song.steps(), steps + 3);
    // Ctrl and Alt are not the comp's; Escape lets the stretch go and the key goes on.
    let ctrl = Mods {
        ctrl: true,
        ..Mods::default()
    };
    assert!(!s.comp_key("up", ctrl));
    assert!(!s.comp_key("escape", none));
    assert_eq!(s.comp_sel, None);
    assert!(!s.comp_key("up", none));
}

/// A MIDI track with a clip of its own over bars 40-41 and a recording made over it: the two
/// lie over one another (the user's loop recording over an earlier clip).
fn midi_recorded_over_a_clip(s: &mut Studio) -> (TrackId, Tick) {
    let start = s.project().time_map().bar_start(40);
    let mut made = None;
    assert!(s.command(|song| {
        let t: TrackId = song.run("track.add.instrument", json!({"name": "Loop"}))?["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        song.run(
            "clip.add.midi",
            json!({"track": t.to_string(), "at": start, "bars": 2, "name": "Hook"}),
        )?;
        song.run(
            "record.take.add",
            json!({"takes": [
                {"track": t.to_string(), "start": start, "length": 2 * BAR,
                 "notes": [{"start": 0, "length": 240, "pitch": 60, "velocity": 99}]},
                {"track": t.to_string(), "start": start, "length": 2 * BAR,
                 "notes": [{"start": 0, "length": 240, "pitch": 64, "velocity": 99}]},
            ]}),
        )?;
        made = Some(t);
        Ok(())
    }));
    assert_eq!(s.error, None);
    let t = made.expect("a track");
    open_takes(s, t);
    (t, start)
}

/// A press on the middle of bar `bar` of take row `j`.
fn click_bar(s: &mut Studio, g: &Geometry, t: TrackId, start: Tick, bar: Tick, j: usize) {
    let at = bar_mid(s, g, t, start, bar, j);
    click(s, g, at);
}

/// The clips that play at `tick`, by name.
/// Which take (or clip) plays at `tick`: takes are all named after the track (UI-296), so they
/// are told apart by the take each piece is of.
fn playing_names(s: &Studio, t: TrackId, tick: Tick) -> Vec<String> {
    clips(s, t)
        .iter()
        .filter(|c| !c.muted && c.start <= tick && tick < c.start + c.length)
        .map(|c| c.take.map_or_else(|| c.name.clone(), |k| k.to_string()))
        .collect()
}

#[test]
fn pressing_takes_again_and_again_does_not_cut_the_recording_into_pieces() {
    let (mut s, g) = studio();
    let (t, start) = midi_recorded_over_a_clip(&mut s);
    show(&mut s, &g, t, start);
    let stack = xirius_core::takes::stacks(&s.project().tracks[&t]);
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].rows, 3);
    let before = clips(&s, t);
    assert_eq!(before.len(), 3);
    // The clip the recording was made over keeps playing (recording changes no existing clip),
    // so the last take plays beside it.
    assert_eq!(playing_names(&s, t, start + 10).len(), 2);
    let last = 2;

    // Pressing the last take takes the whole stretch: the older clip is muted, nothing is cut.
    let steps = s.song.steps();
    click_bar(&mut s, &g, t, start, 1, last);
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(clips(&s, t).len(), 3);
    let top = playing_names(&s, t, start + 10);
    assert_eq!(top.len(), 1);
    assert_eq!(playing_names(&s, t, start + BAR + 10), top);
    // Pressing it again, bar after bar, is nothing: no cut, no step, no error.
    let after = clips(&s, t);
    for _ in 0..2 {
        for bar in 0..2 {
            click_bar(&mut s, &g, t, start, bar, last);
        }
    }
    assert_eq!(s.song.steps(), steps + 1);
    assert_eq!(clips(&s, t), after);
    assert!(
        s.comp_sel.is_some(),
        "the stretch is still chosen for the keys"
    );

    // Another row takes the whole stretch once: three clips still, one step. Pressing the row
    // it was just taken from takes it back, again with no cut.
    click_bar(&mut s, &g, t, start, 1, 1);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(clips(&s, t).len(), 3);
    assert_ne!(playing_names(&s, t, start + 10), top);
    click_bar(&mut s, &g, t, start, 0, 1);
    click_bar(&mut s, &g, t, start, 1, 1);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(clips(&s, t).len(), 3);

    // A drag over a stretch the take already plays is nothing either.
    let a = row_at(&s, &g, t, start + 20, 1);
    let b = row_at(&s, &g, t, start + BAR + 20, 1);
    drag(&mut s, &g, a, b);
    assert_eq!(s.song.steps(), steps + 2);
    assert_eq!(clips(&s, t).len(), 3);
    assert!(s.comp_sel.is_some());
}

#[test]
fn many_takes_shrink_their_rows_so_that_all_of_them_show() {
    use crate::rec_area::{MIN_TAKE_H, take_h};
    assert_eq!(take_h(1), TAKE_H);
    assert_eq!(take_h(4), TAKE_H);
    assert!(take_h(5) < TAKE_H);
    // Up to the point where the rows reach the smallest height, they share 96 px.
    assert_eq!(take_h(6) * 6.0, 96.0);
    assert_eq!(take_h(8), MIN_TAKE_H);
    assert_eq!(take_h(20), MIN_TAKE_H);
    // The rows take that height in the track's area.
    let (mut s, _g) = studio();
    let vocal = track(&s, "Vocal");
    let start = s.project().time_map().bar_start(40);
    let takes: Vec<_> = (1..=8)
        .map(|i| {
            json!({"track": vocal.to_string(), "start": start, "length": 2 * BAR,
                        "file": format!("Recordings/v-{i}.wav")})
        })
        .collect();
    assert!(s.command(|song| {
        song.run("record.take.add", json!({ "takes": takes }))
            .map(drop)
    }));
    open_takes(&mut s, vocal);
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    assert_eq!(row.rec_h, COMP_H + 8.0 * MIN_TAKE_H);
}

#[test]
fn ui_296_takes_start_folded_and_the_clip_s_count_of_takes_opens_them() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    let (start, ids) = record_takes_folded(&mut s, vocal);
    // Every take is named after the track; folded, nothing of them shows under it.
    let taken: Vec<Clip> = clips(&s, vocal)
        .into_iter()
        .filter(|c| ids.contains(&c.id))
        .collect();
    assert!(taken.iter().all(|c| c.name == "Vocal"), "{taken:?}");
    assert_eq!(s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h, 0.0);
    show(&mut s, &g, vocal, start);
    // The clip that plays carries the count at its lower left: pressing it opens the rows.
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    let playing = taken.iter().find(|c| !c.muted).unwrap();
    let badge = crate::draw::takes_badge(s.clip_rect(&g, &row, playing)).expect("the count");
    let at = (badge.x + 4.0, badge.y + badge.h / 2.0);
    assert!(matches!(
        s.arrange_spot(&g, at.0, at.1),
        Spot::Rec { track, part: RecPart::Fold } if track == vocal
    ));
    let steps = s.song.steps();
    click(&mut s, &g, at);
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    assert_eq!(row.rec_h, COMP_H + 3.0 * TAKE_H);
    assert_eq!(s.song.steps(), steps, "opening the rows is not an edit");
    // The Comp row's head folds them away again, all of them.
    let head = (
        g.column.x + row.content_x() + 6.0,
        s.row_y(&g, &row) + (row.rec_y() - row.y) + COMP_H / 2.0,
    );
    assert!(matches!(
        s.arrange_spot(&g, head.0, head.1),
        Spot::Rec {
            part: RecPart::Fold,
            ..
        }
    ));
    click(&mut s, &g, head);
    assert_eq!(s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h, 0.0);
    click(&mut s, &g, at);
    assert_eq!(
        s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h,
        COMP_H + 3.0 * TAKE_H
    );
}

#[test]
fn ui_296_midi_takes_are_named_after_their_track_and_counted_the_same() {
    let (mut s, _g) = studio();
    let keys = s
        .project()
        .tracks
        .values()
        .find(|t| t.kind == xirius_core::model::TrackKind::Instrument)
        .map(|t| (t.id, t.name.clone()))
        .expect("an instrument track");
    let start = s.project().time_map().bar_start(40);
    let take = |pitch: u8| {
        json!({"track": keys.0.to_string(), "start": start, "length": BAR,
               "notes": [{"start": 0, "length": 240, "pitch": pitch}]})
    };
    assert!(s.command(|song| {
        song.run("record.take.add", json!({ "takes": [take(60), take(64)] }))
            .map(drop)
    }));
    let p = s.project();
    let tr = &p.tracks[&keys.0];
    let stacks = xirius_core::takes::stacks(tr);
    let taken: Vec<_> = tr.clips.iter().filter(|c| c.start == start).collect();
    assert_eq!(taken.len(), 2);
    assert!(taken.iter().all(|c| c.name == keys.1));
    let counts: Vec<_> = taken
        .iter()
        .map(|c| crate::draw::takes_under(&stacks, c))
        .collect();
    assert!(
        counts.contains(&Some(2)) && counts.contains(&None),
        "{counts:?}"
    );
    assert_eq!(s.tree().row_of(RowKind::Track(keys.0)).unwrap().rec_h, 0.0);
}

#[test]
fn a_clip_over_nothing_has_no_area_and_an_armed_audio_track_has_its_input() {
    let (mut s, g) = studio();
    let vocal = track(&s, "Vocal");
    assert_eq!(s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h, 0.0);
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    s.scroll.1 = row.y;
    let (x, y) = s.center_of(&g, Hit::Arm(vocal)).expect("visible");
    assert!(s.press(&g, Hit::Arm(vocal)), "{x} {y}");
    let row = *s.tree().row_of(RowKind::Track(vocal)).unwrap();
    assert_eq!(row.rec_h, INPUT_H);
    // The gain slider's right end sets +24 dB; one step, one undo.
    let steps = s.song.steps();
    let at = (
        g.column.x + row.head_w - 8.0,
        s.row_y(&g, &row) + (row.rec_y() - row.y) + 29.0,
    );
    assert!(matches!(
        s.arrange_spot(&g, at.0, at.1),
        Spot::Rec {
            part: RecPart::Gain(_),
            ..
        }
    ));
    click(&mut s, &g, at);
    assert_eq!(s.project().tracks[&vocal].input_gain_db, 24.0);
    assert_eq!(s.song.steps(), steps + 1);
    s.undo(&g);
    assert_eq!(s.project().tracks[&vocal].input_gain_db, 0.0);
    // Disarmed again: the area goes.
    assert!(s.press(&g, Hit::Arm(vocal)));
    assert_eq!(s.tree().row_of(RowKind::Track(vocal)).unwrap().rec_h, 0.0);
}

#[test]
fn record_pressed_with_nothing_armed_says_so_and_sends_the_eye_to_the_arm_button() {
    use crate::topbar::BarButton;
    let (mut s, g) = studio();
    assert!(!s.project().tracks.values().any(|t| t.armed));
    // The selected track records: its button is the one.
    let vocal = track(&s, "Vocal");
    s.selected_row = Some(RowKind::Track(vocal));
    assert!(s.press(&g, Hit::Bar(BarButton::Record)));
    assert!(s.error.is_none(), "no words: the button shakes instead");
    assert!(s.record_shake.is_some());
    assert!(!s.transport.recording);
    assert_eq!(s.arm_cue, Some(vocal));
    // Pressing the button arms the track and lets the cue go.
    assert!(s.press(&g, Hit::Arm(vocal)));
    assert_eq!(s.arm_cue, None);
    assert!(s.project().tracks[&vocal].armed);
    // Nothing selected that records: the first track that does.
    assert!(s.press(&g, Hit::Arm(vocal)));
    s.selected_row = None;
    assert!(s.press(&g, Hit::Bar(BarButton::Record)));
    let first = s
        .tree()
        .rows
        .iter()
        .find_map(|r| match r.kind {
            RowKind::Track(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(s.arm_cue, Some(first));
}

#[test]
fn only_the_takes_of_a_stack_are_left_out_of_the_main_lane_when_they_do_not_play() {
    let (mut s, _g) = studio();
    let t = track(&s, "Vocal");
    let (_, ids) = record_three_takes(&mut s, t);
    let project = s.project();
    let tr = &project.tracks[&t];
    let stacks = xirius_core::takes::stacks(tr);
    assert_eq!(stacks.len(), 1);
    for c in &tr.clips {
        assert_eq!(
            crate::draw::in_stack(&stacks, c),
            ids.contains(&c.id),
            "{}",
            c.name
        );
    }
}

#[test]
fn a_press_on_a_stack_takes_hold_of_the_take_that_plays_not_one_under_it() {
    // EDT-139: the takes that do not play are not drawn in the lane, so a press there must not
    // take hold of them (it took the first take of the stack, which a drag then moved).
    let (mut s, g) = studio();
    let t = track(&s, "Vocal");
    let (start, ids) = record_three_takes(&mut s, t);
    show(&mut s, &g, t, start);
    let plays = *ids
        .iter()
        .find(|id| clips(&s, t).iter().any(|c| c.id == **id && !c.muted))
        .expect("a take plays");
    assert_ne!(plays, ids[0]);
    let at = s
        .center_of(&g, Hit::Clip(t, plays))
        .expect("the take that plays is on screen");
    assert_eq!(s.hit(&g, at.0, at.1), Hit::Clip(t, plays));
    assert!(matches!(
        s.arrange_spot(&g, at.0, at.1),
        Spot::Clip { track, clip, .. } if track == t && clip == plays
    ));
    for hidden in ids.iter().filter(|id| **id != plays) {
        assert!(s.center_of(&g, Hit::Clip(t, *hidden)).is_none());
    }
}
