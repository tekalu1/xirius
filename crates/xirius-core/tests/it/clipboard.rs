//! Cut, copy, paste, duplicate and delete over notes, clips and lane points, and the selecting
//! commands (CTX-028, KEY-003, OPS-015). Everything goes through the registry, as the CLI and
//! MCP do, and one command is one undo.

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::command::Author;
use xirius_core::error::ErrorCode;
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{RunOptions, RunResult, Session};

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn fail(s: &mut Session, name: &str, args: Value) -> ErrorCode {
    s.run_value(name, args, &RunOptions::human())
        .expect_err("should fail")
        .code
}

fn id(r: &RunResult) -> String {
    r.data["id"].as_str().expect("an id").to_string()
}

fn ids(r: &RunResult) -> Vec<String> {
    r.data["ids"]
        .as_array()
        .expect("ids")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn hash(s: &Session) -> String {
    s.project().content_hash()
}

struct Song {
    s: Session,
    lead: String,
    clip: String,
    notes: Vec<String>,
}

/// One instrument track with a two-bar clip at bar 1 holding notes at 0, 480 and 960.
fn song() -> Song {
    let mut s = Session::new("clipboard");
    let lead = id(&run(
        &mut s,
        "track.add.instrument",
        json!({"name": "Lead"}),
    ));
    let clip = id(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": lead, "at": 0, "bars": 2}),
    ));
    let notes = [(0, 60), (480, 62), (960, 64)]
        .iter()
        .map(|(at, pitch)| {
            id(&run(
                &mut s,
                "note.add",
                json!({"clip": clip, "at": at, "pitch": pitch, "length": 240}),
            ))
        })
        .collect();
    Song {
        s,
        lead,
        clip,
        notes,
    }
}

fn note_starts(p: &Project, clip: &str) -> Vec<(i64, u8)> {
    let (_, c) = p.clip(clip.parse().unwrap()).unwrap();
    let mut v: Vec<(i64, u8)> = c.notes().iter().map(|n| (n.start, n.pitch)).collect();
    v.sort();
    v
}

fn cc_points(p: &Project, clip: &str, lane: &str) -> Vec<(i64, i32)> {
    let (_, c) = p.clip(clip.parse().unwrap()).unwrap();
    c.controllers
        .iter()
        .find(|l| l.kind.key() == lane)
        .map(|l| l.points.iter().map(|q| (q.tick, q.value)).collect())
        .unwrap_or_default()
}

#[test]
fn copy_and_paste_notes_puts_new_notes_at_the_place_and_one_undo_removes_them() {
    let mut g = song();
    let before = hash(&g.s);
    let steps = g.s.steps().len();
    let r = run(
        &mut g.s,
        "edit.copy",
        json!({"ids": [g.notes[0], g.notes[1]]}),
    );
    assert_eq!(r.data["count"], 2);
    // A copy is not an edit: no step, the song is as it was.
    assert_eq!(g.s.steps().len(), steps);
    assert_eq!(hash(&g.s), before);

    let r = run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 1920}));
    assert_eq!(ids(&r).len(), 2);
    assert!(g.notes.iter().all(|n| !ids(&r).contains(n)), "new note ids");
    assert_eq!(
        note_starts(g.s.project(), &g.clip),
        vec![(0, 60), (480, 62), (960, 64), (1920, 60), (2400, 62)],
        "the first copied note lands on --at, the next keeps its distance"
    );
    assert_eq!(g.s.steps().len(), steps + 1);
    g.s.undo(1).unwrap();
    assert_eq!(hash(&g.s), before);
    // The clipboard is not in the history: it is still there after the undo.
    run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 1920}));
    assert_eq!(note_starts(g.s.project(), &g.clip).len(), 5);
}

#[test]
fn pasting_without_a_place_puts_things_back_where_they_were() {
    let mut g = song();
    run(&mut g.s, "edit.cut", json!({"ids": [g.notes[1]]}));
    assert_eq!(note_starts(g.s.project(), &g.clip).len(), 2);
    run(&mut g.s, "edit.paste", json!({"at": "original"}));
    assert_eq!(
        note_starts(g.s.project(), &g.clip),
        vec![(0, 60), (480, 62), (960, 64)]
    );
}

#[test]
fn cut_removes_in_one_step_and_one_undo_brings_it_all_back() {
    let mut g = song();
    let before = hash(&g.s);
    let steps = g.s.steps().len();
    let r = run(
        &mut g.s,
        "edit.cut",
        json!({"ids": [g.notes[0], g.notes[2]]}),
    );
    assert_eq!(r.data["count"], 2);
    assert_eq!(note_starts(g.s.project(), &g.clip), vec![(480, 62)]);
    assert_eq!(g.s.steps().len(), steps + 1);
    g.s.undo(1).unwrap();
    assert_eq!(hash(&g.s), before);
    // What was cut is still on the clipboard.
    let r = run(&mut g.s, "edit.clipboard", json!({}));
    assert_eq!(r.data["kind"], "notes");
    assert_eq!(r.data["count"], 2);
}

#[test]
fn the_selection_stands_in_for_ids() {
    let mut g = song();
    run(&mut g.s, "select.set", json!({"ids": [g.notes[1]]}));
    run(&mut g.s, "edit.copy", json!({}));
    run(&mut g.s, "edit.cut", json!({}));
    assert_eq!(note_starts(g.s.project(), &g.clip).len(), 2);
    run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 1920}));
    assert!(note_starts(g.s.project(), &g.clip).contains(&(1920, 62)));
}

#[test]
fn a_note_that_would_fall_outside_the_clip_is_left_out_and_the_rest_goes_in() {
    let mut g = song();
    run(&mut g.s, "edit.copy", json!({"ids": g.notes}));
    // The clip is two bars (7680 ticks): only the first note, at 7200, is inside.
    let r = run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 7200}));
    assert!(r.message.contains("除き"), "{}", r.message);
    assert_eq!(ids(&r).len(), 1, "{r:?}");
    // Nothing fits at all: refused, no step.
    let steps = g.s.steps().len();
    assert_eq!(
        fail(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 90000})),
        ErrorCode::OutOfRange
    );
    assert_eq!(g.s.steps().len(), steps);
}

#[test]
fn an_empty_clipboard_is_refused() {
    let mut g = song();
    assert_eq!(
        fail(&mut g.s, "edit.paste", json!({"at": 0})),
        ErrorCode::NothingToDo
    );
}

#[test]
fn clips_are_copied_pasted_and_cut_with_new_ids_and_their_notes() {
    let mut g = song();
    let before = hash(&g.s);
    run(&mut g.s, "edit.copy", json!({"ids": [g.clip]}));
    let r = run(&mut g.s, "edit.paste", json!({"at": "3.1.0"}));
    let made = ids(&r);
    assert_eq!(made.len(), 1);
    assert_ne!(made[0], g.clip);
    let copy =
        g.s.project()
            .clip(made[0].parse().unwrap())
            .unwrap()
            .1
            .clone();
    assert_eq!(copy.start, 7680);
    assert_eq!(copy.notes().len(), 3);
    let old =
        g.s.project()
            .clip(g.clip.parse().unwrap())
            .unwrap()
            .1
            .clone();
    assert!(
        copy.notes()
            .iter()
            .all(|n| old.notes().iter().all(|o| o.id != n.id)),
        "the notes of a pasted clip have new ids"
    );
    g.s.undo(1).unwrap();
    assert_eq!(hash(&g.s), before);

    // Cut takes the clip out; paste puts it back where it was.
    run(&mut g.s, "edit.cut", json!({"ids": [g.clip]}));
    assert!(g.s.project().find_clip(g.clip.parse().unwrap()).is_none());
    run(&mut g.s, "edit.paste", json!({}));
    let t = g.s.project().track(g.lead.parse().unwrap()).unwrap();
    assert_eq!(t.clips.len(), 1);
    assert_eq!(t.clips[0].start, 0);
}

#[test]
fn pasted_clips_keep_their_rows_relative_to_the_first_and_must_fit_the_track() {
    let mut g = song();
    let lead2 = id(&run(
        &mut g.s,
        "track.add.instrument",
        json!({"name": "Pad"}),
    ));
    let audio = id(&run(&mut g.s, "track.add.audio", json!({"name": "Kick"})));
    run(&mut g.s, "edit.copy", json!({"ids": [g.clip]}));
    let r = run(&mut g.s, "edit.paste", json!({"track": lead2, "at": 0}));
    let (t, _) = g.s.project().clip(ids(&r)[0].parse().unwrap()).unwrap();
    assert_eq!(t.id.to_string(), lead2);
    // An audio track does not take a MIDI clip.
    let steps = g.s.steps().len();
    assert_eq!(
        fail(&mut g.s, "edit.paste", json!({"track": audio, "at": 0})),
        ErrorCode::Conflict
    );
    assert_eq!(g.s.steps().len(), steps);
}

#[test]
fn controller_points_copy_paste_and_cut_by_tick_inside_the_lane() {
    let mut g = song();
    run(
        &mut g.s,
        "cc.replace",
        json!({"clip": g.clip, "lane": "cc1", "from": 0, "to": 960,
               "points": [{"tick": 0, "value": 10}, {"tick": 480, "value": 90}, {"tick": 960, "value": 40}]}),
    );
    let before = hash(&g.s);
    run(
        &mut g.s,
        "edit.copy",
        json!({"clip": g.clip, "lane": "cc1", "ticks": "480,960"}),
    );
    let r = run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 1920}));
    assert_eq!(r.data["points"], 2);
    assert_eq!(
        cc_points(g.s.project(), &g.clip, "cc1"),
        vec![(0, 10), (480, 90), (960, 40), (1920, 90), (2400, 40)]
    );
    g.s.undo(1).unwrap();
    assert_eq!(hash(&g.s), before);

    // A point at the same place is replaced, not doubled.
    run(&mut g.s, "edit.paste", json!({"clip": g.clip, "at": 0}));
    assert_eq!(
        cc_points(g.s.project(), &g.clip, "cc1"),
        vec![(0, 90), (480, 40), (960, 40)]
    );

    // Cut leaves the rest, and cutting every point drops the lane.
    run(
        &mut g.s,
        "edit.cut",
        json!({"clip": g.clip, "lane": "cc1", "from": 480, "to": 960}),
    );
    assert_eq!(cc_points(g.s.project(), &g.clip, "cc1"), vec![(0, 90)]);
    run(
        &mut g.s,
        "edit.delete",
        json!({"clip": g.clip, "lane": "cc1"}),
    );
    let (_, c) = g.s.project().clip(g.clip.parse().unwrap()).unwrap();
    assert!(c.controllers.is_empty());
}

#[test]
fn controller_points_can_go_to_another_clip_and_another_lane_with_the_value_scaled() {
    let mut g = song();
    let other = id(&run(
        &mut g.s,
        "clip.add.midi",
        json!({"track": g.lead, "at": "5.1.0", "bars": 1}),
    ));
    run(
        &mut g.s,
        "cc.replace",
        json!({"clip": g.clip, "lane": "cc1", "from": 0, "to": 0,
               "points": [{"tick": 0, "value": 127}]}),
    );
    run(
        &mut g.s,
        "edit.copy",
        json!({"clip": g.clip, "lane": "cc1"}),
    );
    run(
        &mut g.s,
        "edit.paste",
        json!({"clip": other, "lane": "bend", "at": "5.1.0"}),
    );
    // 127 of 0..127 is the top of the bend range.
    assert_eq!(cc_points(g.s.project(), &other, "bend"), vec![(0, 8191)]);
}

#[test]
fn automation_points_copy_paste_cut_and_delete() {
    let mut g = song();
    for (at, v) in [(0, -6.0), (960, -12.0), (1920, 0.0)] {
        run(
            &mut g.s,
            "auto.point.add",
            json!({"track": g.lead, "target": "volume", "at": at, "value": v}),
        );
    }
    let before = hash(&g.s);
    run(
        &mut g.s,
        "edit.copy",
        json!({"track": g.lead, "target": "volume", "ticks": "0,960"}),
    );
    run(&mut g.s, "edit.paste", json!({"at": 3840}));
    let pts = |g: &Song| -> Vec<(i64, f64)> {
        g.s.project()
            .track(g.lead.parse().unwrap())
            .unwrap()
            .automation
            .iter()
            .find(|l| l.target.to_string() == "volume")
            .unwrap()
            .points
            .iter()
            .map(|q| (q.tick, q.value))
            .collect()
    };
    assert_eq!(
        pts(&g),
        vec![
            (0, -6.0),
            (960, -12.0),
            (1920, 0.0),
            (3840, -6.0),
            (4800, -12.0)
        ]
    );
    g.s.undo(1).unwrap();
    assert_eq!(hash(&g.s), before);

    // Del on points: many at once, in one step.
    run(
        &mut g.s,
        "edit.delete",
        json!({"track": g.lead, "target": "volume", "ticks": "0,1920"}),
    );
    assert_eq!(pts(&g), vec![(960, -12.0)]);
    g.s.undo(1).unwrap();
    assert_eq!(pts(&g).len(), 3);

    // Pasted into another target, the value keeps its place in the lane's range.
    run(
        &mut g.s,
        "edit.copy",
        json!({"track": g.lead, "target": "volume", "ticks": "1920"}),
    );
    run(&mut g.s, "edit.paste", json!({"target": "pan", "at": 0}));
    let t = g.s.project().track(g.lead.parse().unwrap()).unwrap();
    let pan = t
        .automation
        .iter()
        .find(|l| l.target.to_string() == "pan")
        .expect("a lane is made");
    assert_eq!(pan.points.len(), 1);
    let want = AutoTarget::Pan.from_norm(AutoTarget::Volume.to_norm(0.0));
    assert!(
        (pan.points[0].value - want).abs() < 1e-9,
        "{}",
        pan.points[0].value
    );
}

#[test]
fn duplicate_copies_to_the_right_by_the_span_or_by_the_given_amount() {
    let mut g = song();
    let r = run(
        &mut g.s,
        "edit.duplicate",
        json!({"ids": [g.notes[0], g.notes[1]]}),
    );
    assert_eq!(ids(&r).len(), 2);
    // Notes at 0 and 480 (length 240) span 720: the copies start at 720 and 1200.
    assert!(note_starts(g.s.project(), &g.clip).contains(&(720, 60)));
    assert!(note_starts(g.s.project(), &g.clip).contains(&(1200, 62)));
    let r = run(
        &mut g.s,
        "edit.duplicate",
        json!({"ids": [g.clip], "by": 7680}),
    );
    let copy =
        g.s.project()
            .clip(ids(&r)[0].parse().unwrap())
            .unwrap()
            .1
            .start;
    assert_eq!(copy, 7680);
    // The clipboard was not touched by a duplicate.
    let r = run(&mut g.s, "edit.clipboard", json!({}));
    assert_eq!(r.data["count"], 0);
}

#[test]
fn the_ai_pastes_notes_marked_as_its_own() {
    let mut g = song();
    run(&mut g.s, "edit.copy", json!({"ids": [g.notes[0]]}));
    g.s.run_value(
        "edit.paste",
        json!({"clip": g.clip, "at": 1920}),
        &RunOptions {
            author: Author::Ai("test".into()),
            dry_run: false,
        },
    )
    .unwrap();
    let (_, c) = g.s.project().clip(g.clip.parse().unwrap()).unwrap();
    assert_eq!(c.notes().iter().filter(|n| n.ai).count(), 1);
}

#[test]
fn a_dry_run_of_cut_leaves_the_song_and_the_clipboard_alone() {
    let mut g = song();
    run(&mut g.s, "edit.copy", json!({"ids": [g.notes[0]]}));
    let before = hash(&g.s);
    g.s.run_value(
        "edit.cut",
        json!({"ids": [g.notes[1], g.notes[2]]}),
        &RunOptions::human().dry(),
    )
    .unwrap();
    assert_eq!(hash(&g.s), before);
    let r = run(&mut g.s, "edit.clipboard", json!({}));
    assert_eq!(r.data["count"], 1);
}

#[test]
fn a_clip_cut_takes_the_audio_kind_too() {
    let mut g = song();
    let audio = id(&run(&mut g.s, "track.add.audio", json!({"name": "Kick"})));
    let a = id(&run(
        &mut g.s,
        "clip.add.audio",
        json!({"track": audio, "at": 3840, "length": 1920, "file": "kick.wav"}),
    ));
    run(&mut g.s, "edit.cut", json!({"ids": [a]}));
    run(&mut g.s, "edit.paste", json!({"at": 0}));
    let t = g.s.project().track(audio.parse().unwrap()).unwrap();
    assert_eq!(t.clips.len(), 1);
    assert!(matches!(t.clips[0].body, ClipBody::Audio { .. }));
    assert_eq!(t.clips[0].start, 0);
}

// ---- selecting -------------------------------------------------------------------------------------

fn selected(s: &Session) -> Vec<String> {
    s.state.selection.iter().map(ToString::to_string).collect()
}

#[test]
fn select_all_takes_every_clip_or_the_notes_of_the_clips_that_hold_the_selection() {
    let mut g = song();
    let second = id(&run(
        &mut g.s,
        "clip.add.midi",
        json!({"track": g.lead, "at": "5.1.0", "bars": 1}),
    ));
    run(&mut g.s, "select.all", json!({}));
    let mut got = selected(&g.s);
    got.sort();
    let mut want = vec![g.clip.clone(), second.clone()];
    want.sort();
    assert_eq!(got, want);

    run(&mut g.s, "select.set", json!({"ids": [g.notes[1]]}));
    run(&mut g.s, "select.all", json!({}));
    let mut got = selected(&g.s);
    got.sort();
    let mut want = g.notes.clone();
    want.sort();
    assert_eq!(got, want);

    run(&mut g.s, "select.clear", json!({}));
    assert!(selected(&g.s).is_empty());
}

#[test]
fn select_invert_flips_within_the_same_universe() {
    let mut g = song();
    run(&mut g.s, "select.set", json!({"ids": [g.notes[0]]}));
    run(&mut g.s, "select.invert", json!({}));
    let mut got = selected(&g.s);
    got.sort();
    let mut want = vec![g.notes[1].clone(), g.notes[2].clone()];
    want.sort();
    assert_eq!(got, want);

    // With clips: select one of two, the other is left.
    let second = id(&run(
        &mut g.s,
        "clip.add.midi",
        json!({"track": g.lead, "at": "5.1.0", "bars": 1}),
    ));
    run(&mut g.s, "select.set", json!({"ids": [g.clip]}));
    run(&mut g.s, "select.invert", json!({}));
    assert_eq!(selected(&g.s), vec![second]);
    // Inverting twice is the identity.
    run(&mut g.s, "select.invert", json!({}));
    assert_eq!(selected(&g.s), vec![g.clip.clone()]);
}

#[test]
fn select_on_tracks_and_in_loop_pick_clips() {
    let mut g = song();
    let pad = id(&run(
        &mut g.s,
        "track.add.instrument",
        json!({"name": "Pad"}),
    ));
    let far = id(&run(
        &mut g.s,
        "clip.add.midi",
        json!({"track": g.lead, "at": "9.1.0", "bars": 1}),
    ));
    let other = id(&run(
        &mut g.s,
        "clip.add.midi",
        json!({"track": pad, "at": 0, "bars": 1}),
    ));
    // The clips on the track of the selected clip.
    run(&mut g.s, "select.set", json!({"ids": [g.clip]}));
    run(&mut g.s, "select.on-tracks", json!({}));
    let mut got = selected(&g.s);
    got.sort();
    let mut want = vec![g.clip.clone(), far.clone()];
    want.sort();
    assert_eq!(got, want);

    // No loop yet: refused. Then the clips that touch the loop.
    assert_eq!(
        fail(&mut g.s, "select.in-loop", json!({})),
        ErrorCode::Conflict
    );
    run(
        &mut g.s,
        "loop.set",
        json!({"start": "1.1.0", "end": "2.1.0"}),
    );
    run(&mut g.s, "select.in-loop", json!({}));
    let mut got = selected(&g.s);
    got.sort();
    let mut want = vec![g.clip.clone(), other];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn the_selecting_commands_stay_out_of_the_history() {
    let mut g = song();
    let steps = g.s.steps().len();
    for name in [
        "select.all",
        "select.invert",
        "select.clear",
        "select.on-tracks",
    ] {
        let _ = g.s.run_value(name, json!({}), &RunOptions::human());
    }
    assert_eq!(g.s.steps().len(), steps);
}
