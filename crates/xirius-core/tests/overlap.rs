//! EDT-137 / EDT-138: no overlap when editing. With `--overlap trim`, an edit that lays a note
//! over another of the same pitch cuts the earlier one's end at the later one's start (a note
//! wholly covered goes); with `--no-overlap`, an edit that lays a clip over another of the same
//! track cuts away what lies under it (in the middle, two pieces stay). Without them, things
//! overlap as before. One edit is one history step.
use serde_json::{Value, json};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::ClipBody;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

fn ids_of(v: &Value) -> Vec<String> {
    v["ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect()
}

// ---- notes -----------------------------------------------------------------------

/// One instrument track with one long empty clip at 0.
fn roll() -> (Session, String) {
    let mut s = Session::new("overlap");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let c = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 0, "length": 15360}),
    ));
    (s, c)
}

/// Puts notes `(start, length, pitch)` into the clip, returning their ids.
fn put(s: &mut Session, clip: &str, notes: &[(i64, i64, u8)]) -> Vec<String> {
    let notes: Vec<Value> = notes
        .iter()
        .map(|&(at, length, pitch)| json!({"at": at, "length": length, "pitch": pitch}))
        .collect();
    ids_of(&run(s, "note.put", json!({"clip": clip, "notes": notes})))
}

/// The notes of the clip as (start, length, pitch), in order.
fn notes(s: &Session, clip: &str) -> Vec<(i64, i64, u8)> {
    let id: ClipId = clip.parse().unwrap();
    let (_, c) = s.project().clip(id).unwrap();
    let mut v: Vec<_> = c
        .notes()
        .iter()
        .map(|n| (n.start, n.length, n.pitch))
        .collect();
    v.sort_unstable();
    v
}

fn note_ids(s: &Session, clip: &str) -> Vec<String> {
    let id: ClipId = clip.parse().unwrap();
    let (_, c) = s.project().clip(id).unwrap();
    c.notes().iter().map(|n| n.id.to_string()).collect()
}

#[test]
fn moving_a_note_over_the_tail_of_another_cuts_that_ones_end() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (960, 480, 60)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[1]], "by": -720, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(0, 240, 60), (240, 480, 60)]);
    assert_eq!(
        s.steps().len(),
        steps + 1,
        "the trimming is in the same step"
    );
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c), [(0, 480, 60), (960, 480, 60)]);
}

#[test]
fn with_allow_notes_overlap_as_before() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (960, 480, 60)]);
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[1]], "by": -720, "overlap": "allow"}),
    );
    assert_eq!(notes(&s, &c), [(0, 480, 60), (240, 480, 60)]);
    // Nothing said is the same as allow.
    run(&mut s, "note.move", json!({"ids": [n[1]], "by": -120}));
    assert_eq!(notes(&s, &c), [(0, 480, 60), (120, 480, 60)]);
}

#[test]
fn moving_a_note_onto_the_head_of_another_cuts_the_moved_ones_end() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (960, 480, 60)]);
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[0]], "by": 720, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(720, 240, 60), (960, 480, 60)]);
}

#[test]
fn a_note_wholly_covered_goes() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (960, 480, 60), (960, 480, 64)]);
    run(
        &mut s,
        "note.resize",
        json!({"ids": [n[0]], "length": 2000, "overlap": "trim"}),
    );
    // The other pitch stays.
    assert_eq!(notes(&s, &c), [(0, 2000, 60), (960, 480, 64)]);
    assert!(!note_ids(&s, &c).contains(&n[1]));
    s.undo(1).unwrap();
    assert_eq!(notes(&s, &c).len(), 3);
}

#[test]
fn a_note_put_into_the_middle_of_another_cuts_it_there() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 1920, 60), (3840, 240, 60)]);
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[1]], "by": -2880, "overlap": "trim"}),
    );
    // The long one ends where the short one starts; its rest is not made again.
    assert_eq!(notes(&s, &c), [(0, 960, 60), (960, 240, 60)]);
}

#[test]
fn of_two_notes_that_start_together_the_moved_one_stays() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 960, 60), (960, 480, 60)]);
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[1]], "by": -960, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(0, 480, 60)]);
    assert_eq!(note_ids(&s, &c), [n[1].clone()]);
}

#[test]
fn an_overlap_the_edit_did_not_touch_stays() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (240, 480, 60), (3840, 240, 62)]);
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[2]], "by": 240, "overlap": "trim"}),
    );
    assert_eq!(
        notes(&s, &c),
        [(0, 480, 60), (240, 480, 60), (4080, 240, 62)]
    );
}

#[test]
fn moving_several_notes_trims_each_and_not_one_another() {
    let (mut s, c) = roll();
    let n = put(
        &mut s,
        &c,
        &[
            (0, 480, 60),
            (480, 480, 60),
            (1440, 480, 60),
            (0, 960, 62),
            (1440, 480, 62),
        ],
    );
    let steps = s.steps().len();
    run(
        &mut s,
        "note.move",
        json!({"ids": [n[0], n[1], n[3]], "by": 720, "overlap": "trim"}),
    );
    // The two moved 60s keep touching end to start; the second ends at the third's start,
    // and the moved 62 at the other 62's start.
    assert_eq!(
        notes(&s, &c),
        [
            (720, 480, 60),
            (720, 720, 62),
            (1200, 240, 60),
            (1440, 480, 60),
            (1440, 480, 62),
        ]
    );
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(
        notes(&s, &c),
        [
            (0, 480, 60),
            (0, 960, 62),
            (480, 480, 60),
            (1440, 480, 60),
            (1440, 480, 62),
        ]
    );
}

#[test]
fn adding_a_note_trims_the_one_it_lands_on() {
    let (mut s, c) = roll();
    put(&mut s, &c, &[(0, 480, 60)]);
    run(
        &mut s,
        "note.add",
        json!({"clip": c, "at": 240, "pitch": 60, "length": 480, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(0, 240, 60), (240, 480, 60)]);
}

#[test]
fn pencil_notes_trim_what_they_land_on() {
    let (mut s, c) = roll();
    put(&mut s, &c, &[(0, 960, 60), (1920, 480, 60)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "note.put",
        json!({"clip": c, "overlap": "trim", "notes": [
            {"at": 480, "pitch": 60, "length": 240},
            {"at": 1920, "pitch": 60, "length": 960},
        ]}),
    );
    // The new one at 1920 starts with the old one and stays; the old one goes.
    assert_eq!(
        notes(&s, &c),
        [(0, 480, 60), (480, 240, 60), (1920, 960, 60)]
    );
    assert_eq!(s.steps().len(), steps + 1);
}

#[test]
fn a_drag_committed_with_note_set_trims() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60), (960, 480, 62)]);
    // The drag moves the 62 down onto the 60's pitch, over its tail.
    run(
        &mut s,
        "note.set",
        json!({"overlap": "trim", "notes": [{"id": n[1], "start": 240, "pitch": 60}]}),
    );
    assert_eq!(notes(&s, &c), [(0, 240, 60), (240, 480, 60)]);
}

#[test]
fn quantizing_a_note_into_another_trims() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(1440, 960, 60), (1930, 480, 60)]);
    run(
        &mut s,
        "note.quantize",
        json!({"ids": [n[1]], "grid": "1/16", "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(1440, 480, 60), (1920, 480, 60)]);
}

#[test]
fn pasting_and_duplicating_notes_trims() {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60)]);
    run(&mut s, "edit.copy", json!({"ids": [n[0]]}));
    run(
        &mut s,
        "edit.paste",
        json!({"clip": c, "at": 240, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(0, 240, 60), (240, 480, 60)]);

    let (mut s, c) = roll();
    let n = put(&mut s, &c, &[(0, 480, 60)]);
    run(
        &mut s,
        "note.duplicate",
        json!({"ids": [n[0]], "by": 120, "overlap": "trim"}),
    );
    assert_eq!(notes(&s, &c), [(0, 120, 60), (120, 480, 60)]);
}

// ---- clips -----------------------------------------------------------------------

/// One instrument track with MIDI clips `(start, length)`.
fn lane(spans: &[(i64, i64)]) -> (Session, String, Vec<String>) {
    let mut s = Session::new("no-overlap");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let clips = spans
        .iter()
        .map(|&(at, length)| {
            id_of(&run(
                &mut s,
                "clip.add.midi",
                json!({"track": t, "at": at, "length": length}),
            ))
        })
        .collect();
    (s, t, clips)
}

/// The clips of a track as (start, length), in time order.
fn spans(s: &Session, track: &str) -> Vec<(i64, i64)> {
    let id: TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .map(|c| (c.start, c.length))
        .collect();
    v.sort_unstable();
    v
}

/// The clips of a track as (start, length, audio offset), in time order.
fn audio_spans(s: &Session, track: &str) -> Vec<(i64, i64, i64)> {
    let id: TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .map(|c| {
            let ClipBody::Audio { offset, .. } = &c.body else {
                panic!("not audio");
            };
            (c.start, c.length, *offset)
        })
        .collect();
    v.sort_unstable();
    v
}

#[test]
fn moving_a_clip_over_the_tail_of_another_trims_it() {
    let (mut s, t, c) = lane(&[(0, 960), (1920, 960)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[1]], "at": 480, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 480), (480, 960)]);
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(spans(&s, &t), [(0, 960), (1920, 960)]);
}

#[test]
fn with_no_overlap_off_clips_overlap_as_before() {
    let (mut s, t, c) = lane(&[(0, 960), (1920, 960)]);
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[1]], "at": 480, "no-overlap": false}),
    );
    assert_eq!(spans(&s, &t), [(0, 960), (480, 960)]);
    run(&mut s, "clip.move", json!({"ids": [c[1]], "at": 240}));
    assert_eq!(spans(&s, &t), [(0, 960), (240, 960)]);
}

#[test]
fn moving_a_clip_over_the_head_of_another_trims_its_start_and_keeps_its_notes_in_place() {
    let (mut s, t, c) = lane(&[(0, 960), (1920, 960)]);
    put(&mut s, &c[1], &[(0, 240, 60), (720, 240, 62)]);
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[0]], "at": 1440, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(1440, 960), (2400, 480)]);
    // The note under the moved clip goes with the trimmed head; the other stays where it sounds.
    let id: ClipId = c[1].parse().unwrap();
    let (_, under) = s.project().clip(id).unwrap();
    let heard: Vec<_> = under
        .notes()
        .iter()
        .filter(|n| n.start >= 0)
        .map(|n| (under.start + n.start, n.pitch))
        .collect();
    assert_eq!(heard, [(2640, 62)]);
}

#[test]
fn a_clip_wholly_under_a_resized_one_goes() {
    let (mut s, t, c) = lane(&[(0, 960), (1920, 960), (4800, 960)]);
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [c[0]], "length": 3000, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 3000), (4800, 960)]);
    s.undo(1).unwrap();
    assert_eq!(spans(&s, &t), [(0, 960), (1920, 960), (4800, 960)]);
}

#[test]
fn a_clip_dropped_into_the_middle_of_another_splits_it() {
    let (mut s, t, c) = lane(&[(0, 3840), (5760, 480)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[1]], "at": 1920, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 1920), (1920, 480), (2400, 1440)]);
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(spans(&s, &t), [(0, 3840), (5760, 480)]);
}

#[test]
fn trimming_a_clips_start_over_another_trims_it() {
    let (mut s, t, c) = lane(&[(0, 960), (1920, 960)]);
    run(
        &mut s,
        "clip.trim",
        json!({"ids": [c[1]], "by": -1440, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 480), (480, 2400)]);
}

#[test]
fn duplicating_and_pasting_clips_trims_what_they_land_on() {
    let (mut s, t, c) = lane(&[(0, 960), (1440, 960)]);
    run(
        &mut s,
        "clip.duplicate",
        json!({"ids": [c[0]], "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 960), (960, 960), (1920, 480)]);

    let (mut s, t, c) = lane(&[(0, 960)]);
    run(&mut s, "edit.copy", json!({"ids": [c[0]]}));
    run(
        &mut s,
        "edit.paste",
        json!({"at": 480, "track": t, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 480), (480, 960)]);
}

#[test]
fn moving_several_clips_trims_what_is_under_each_and_not_one_another() {
    let (mut s, t, c) = lane(&[(0, 960), (960, 960), (3840, 960)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[0], c[1]], "by": 2400, "no-overlap": true}),
    );
    // The two moved clips still touch; the third loses its head under the second.
    assert_eq!(spans(&s, &t), [(2400, 960), (3360, 960), (4320, 480)]);
    assert_eq!(s.steps().len(), steps + 1);
}

#[test]
fn other_tracks_are_left_alone() {
    let (mut s, t, c) = lane(&[(0, 960)]);
    let other = id_of(&run(&mut s, "track.add.instrument", json!({})));
    run(
        &mut s,
        "clip.add.midi",
        json!({"track": other, "at": 0, "length": 1920}),
    );
    run(
        &mut s,
        "clip.move",
        json!({"ids": [c[0]], "at": 480, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(480, 960)]);
    assert_eq!(spans(&s, &other), [(0, 1920)]);
}

/// An audio track with one clip of `b.wav` at 0, 3840 long.
fn audio() -> (Session, String, String) {
    let mut s = Session::new("no-overlap-audio");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let c = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 3840, "file": "b.wav"}),
    ));
    (s, t, c)
}

#[test]
fn an_audio_clip_trimmed_at_its_head_plays_on_from_the_same_place() {
    let (mut s, t, _) = audio();
    let short = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 5760, "length": 960, "file": "a.wav"}),
    ));
    run(
        &mut s,
        "clip.move",
        json!({"ids": [short], "at": 0, "no-overlap": true}),
    );
    assert_eq!(audio_spans(&s, &t), [(0, 960, 0), (960, 2880, 960)]);
}

#[test]
fn an_audio_clip_split_by_one_dropped_into_it_keeps_its_offset_on_both_pieces() {
    let (mut s, t, _) = audio();
    let short = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 5760, "length": 480, "file": "a.wav"}),
    ));
    run(
        &mut s,
        "clip.move",
        json!({"ids": [short], "at": 960, "no-overlap": true}),
    );
    assert_eq!(
        audio_spans(&s, &t),
        [(0, 960, 0), (960, 480, 0), (1440, 2400, 1440)]
    );
}

#[test]
fn a_crossfading_pair_keeps_its_overlap() {
    let mut s = Session::new("no-overlap-cross");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    let a = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "a.wav"}),
    ));
    let b = id_of(&run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 960, "file": "b.wav"}),
    ));
    run(&mut s, "clip.slip", json!({"ids": [b], "by": -480}));
    run(
        &mut s,
        "clip.fade.cross",
        json!({"ids": [a, b], "length": 120}),
    );
    let before = audio_spans(&s, &t);
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [a], "by": 30, "no-overlap": true}),
    );
    let after = audio_spans(&s, &t);
    assert_eq!(after.len(), 2);
    assert_eq!(after[1], before[1], "the right clip of the pair stays");
    assert_eq!(after[0].1, before[0].1 + 30);
}

// ---- takes (EDT-139) ---------------------------------------------------------------

/// An instrument track with three takes of 3840 at 1920, as a recording of three laps leaves
/// them (the last one plays), and a clip of its own of 1920 at 6720; gives the session, the
/// track, the takes and the clip.
fn takes() -> (Session, String, Vec<String>, String) {
    let mut s = Session::new("no-overlap-takes");
    let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let take = json!({"track": t, "start": 1920, "length": 3840,
        "notes": [{"start": 0, "length": 480, "pitch": 60}]});
    let d = run(
        &mut s,
        "record.take.add",
        json!({"takes": [take, take, take]}),
    );
    let ids: Vec<String> = d["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let own = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": t, "at": 6720, "length": 1920}),
    ));
    let id: TrackId = t.parse().unwrap();
    assert_eq!(
        xirius_core::takes::stacks(&s.project().tracks[&id])[0].rows,
        3
    );
    (s, t, ids, own)
}

/// The span of one clip.
fn span(s: &Session, clip: &str) -> (i64, i64) {
    let id: ClipId = clip.parse().unwrap();
    let (_, c) = s.project().clip(id).unwrap();
    (c.start, c.length)
}

#[test]
fn edt_139_moving_a_take_leaves_the_other_takes_of_its_stack_whole() {
    let (mut s, _, k, _) = takes();
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.move",
        json!({"ids": [k[2]], "by": -960, "no-overlap": true}),
    );
    assert_eq!(span(&s, &k[0]), (1920, 3840));
    assert_eq!(span(&s, &k[1]), (1920, 3840));
    assert_eq!(span(&s, &k[2]), (960, 3840));
    assert_eq!(s.steps().len(), steps + 1);
    // Nor does a take that was not playing cut the one that plays.
    run(
        &mut s,
        "clip.move",
        json!({"ids": [k[0]], "by": 960, "no-overlap": true}),
    );
    assert_eq!(span(&s, &k[0]), (2880, 3840));
    assert_eq!(span(&s, &k[1]), (1920, 3840));
    assert_eq!(span(&s, &k[2]), (960, 3840));
}

#[test]
fn edt_139_resizing_and_trimming_a_take_leaves_the_other_takes_whole() {
    let (mut s, t, k, own) = takes();
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [k[1]], "length": 1920, "no-overlap": true}),
    );
    run(
        &mut s,
        "clip.trim",
        json!({"ids": [k[2]], "by": -960, "no-overlap": true}),
    );
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [k[1]], "length": 3840, "no-overlap": true}),
    );
    assert_eq!(span(&s, &k[0]), (1920, 3840));
    assert_eq!(span(&s, &k[1]), (1920, 3840));
    assert_eq!(span(&s, &k[2]), (960, 4800));
    assert_eq!(span(&s, &own), (6720, 1920));
    assert_eq!(spans(&s, &t).len(), 4);
}

#[test]
fn edt_139_a_take_and_a_clip_outside_its_stack_are_cut_as_before() {
    let (mut s, t, k, own) = takes();
    // The take that plays, moved onto the clip after the stack, cuts its head away.
    run(
        &mut s,
        "clip.move",
        json!({"ids": [k[2]], "by": 1920, "no-overlap": true}),
    );
    assert_eq!(span(&s, &k[0]), (1920, 3840));
    assert_eq!(span(&s, &k[1]), (1920, 3840));
    assert_eq!(span(&s, &k[2]), (3840, 3840));
    assert_eq!(span(&s, &own), (7680, 960));
    s.undo(1).unwrap();
    assert_eq!(span(&s, &own), (6720, 1920));

    // A clip of its own dropped into the takes splits each take under it.
    run(
        &mut s,
        "clip.move",
        json!({"ids": [own], "at": 2880, "no-overlap": true}),
    );
    let mut want = vec![(1920, 960); 3];
    want.push((2880, 1920));
    want.extend([(4800, 960); 3]);
    assert_eq!(spans(&s, &t), want);
}

#[test]
fn edt_139_the_pieces_of_a_comped_take_stay_in_its_stack() {
    let (mut s, t, k, _) = takes();
    // The first take plays over its second half: the takes are cut at 3840.
    run(
        &mut s,
        "take.comp",
        json!({"take": k[0], "from": 3840, "to": 5760}),
    );
    let before = take_spans(&s, &t);
    assert_eq!(before.len(), 6, "the comp cut the takes: {before:?}");
    // The head of the third take, moved over the second halves of all three, cuts none.
    run(
        &mut s,
        "clip.move",
        json!({"ids": [k[2]], "by": 480, "no-overlap": true}),
    );
    let mut want: Vec<(i64, i64)> = before.clone();
    let i = want.iter().position(|x| *x == (1920, 1920)).unwrap();
    want[i] = (2400, 1920);
    want.sort_unstable();
    assert_eq!(take_spans(&s, &t), want);
}

/// The spans of the takes of the track (the recorded clips, in whatever pieces), in time order.
fn take_spans(s: &Session, track: &str) -> Vec<(i64, i64)> {
    let id: TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .filter(|c| c.take.is_some())
        .map(|c| (c.start, c.length))
        .collect();
    v.sort_unstable();
    v
}

// ---- ripple ------------------------------------------------------------------------

#[test]
fn with_ripple_the_clips_pushed_along_do_not_cut_one_another_and_it_is_one_step() {
    // A, a gap, then B and C that overlap a little (as a crossfade would).
    let (mut s, t, c) = lane(&[(0, 960), (1440, 960), (2160, 960)]);
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [c[0]], "by": 960, "ripple": true, "no-overlap": true}),
    );
    // A pushes B and C by as much; B and C keep their overlap.
    assert_eq!(spans(&s, &t), [(0, 1920), (2400, 960), (3120, 960)]);
    assert_eq!(s.steps().len(), steps + 1);
    s.undo(1).unwrap();
    assert_eq!(spans(&s, &t), [(0, 960), (1440, 960), (2160, 960)]);

    // Pulled back, nothing is cut either.
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [c[0]], "by": -480, "ripple": true, "no-overlap": true}),
    );
    assert_eq!(spans(&s, &t), [(0, 480), (960, 960), (1680, 960)]);
}

#[test]
fn with_ripple_the_edited_clip_cuts_what_lay_over_its_end_and_the_pushed_ones_cut_nothing() {
    // X starts under A's end and runs on under B, too little to be a take of B: it starts
    // before A ends, so it is not pushed.
    let (mut s, t, c) = lane(&[(0, 960), (960, 1920), (720, 720)]);
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [c[0]], "by": 240, "ripple": true, "no-overlap": true}),
    );
    // A grows to 1200 and cuts X's head there; B, pushed to 1200, leaves the rest of X alone.
    assert_eq!(span(&s, &c[0]), (0, 1200));
    assert_eq!(span(&s, &c[1]), (1200, 1920));
    assert_eq!(span(&s, &c[2]), (1200, 240));
    assert_eq!(spans(&s, &t).len(), 3);
}

#[test]
fn with_ripple_a_take_grown_over_the_others_pushes_what_follows_and_cuts_no_take() {
    let (mut s, t, k, own) = takes();
    let steps = s.steps().len();
    run(
        &mut s,
        "clip.resize",
        json!({"ids": [k[2]], "by": 960, "ripple": true, "no-overlap": true}),
    );
    assert_eq!(span(&s, &k[0]), (1920, 3840));
    assert_eq!(span(&s, &k[1]), (1920, 3840));
    assert_eq!(span(&s, &k[2]), (1920, 4800));
    assert_eq!(span(&s, &own), (7680, 1920));
    assert_eq!(spans(&s, &t).len(), 4);
    assert_eq!(s.steps().len(), steps + 1);
}

// ---- recording ---------------------------------------------------------------------

#[test]
fn recording_has_no_overlap_argument_and_its_takes_cut_nothing() {
    let reg = xirius_core::command::Registry::standard();
    let def = reg.get("record.take.add").unwrap();
    assert!(
        def.args
            .iter()
            .all(|a| a.name != "overlap" && a.name != "no-overlap"),
        "recording is outside the no-overlap settings"
    );
    let (mut s, t, c) = lane(&[(0, 3840)]);
    put(&mut s, &c[0], &[(0, 960, 60)]);
    // A take recorded over a clip leaves it whole, and its own notes as they came.
    run(
        &mut s,
        "record.take.add",
        json!({"takes": [{"track": t, "start": 960, "length": 960, "notes": [
            {"start": 0, "length": 480, "pitch": 60},
            {"start": 240, "length": 480, "pitch": 60},
        ]}]}),
    );
    assert_eq!(spans(&s, &t), [(0, 3840), (960, 960)]);
    assert_eq!(notes(&s, &c[0]), [(0, 960, 60)]);
    let id: TrackId = t.parse().unwrap();
    let take = s.project().tracks[&id]
        .clips
        .iter()
        .find(|x| x.start == 960)
        .unwrap();
    let mut got: Vec<_> = take.notes().iter().map(|n| (n.start, n.length)).collect();
    got.sort_unstable();
    assert_eq!(got, [(0, 480), (240, 480)]);
}
