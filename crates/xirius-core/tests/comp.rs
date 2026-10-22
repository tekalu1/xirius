//! Takes and comping (REC-006, REC-014, REC-015, REC-016): clips that lie over one another are the
//! takes of a passage, each in a row of its own; `take.comp` picks, over a range, the take that
//! plays, in one step of the history, for audio and for MIDI.

use serde_json::{Value, json};
use xirius_core::error::ErrorCode;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::Track;
use xirius_core::session::{RunOptions, Session};
use xirius_core::takes;

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

fn track_id(d: &Value) -> String {
    d["id"]
        .as_str()
        .or_else(|| d["ids"][0].as_str())
        .unwrap()
        .to_string()
}

fn track(s: &Session, id: &str) -> Track {
    let id: TrackId = id.parse().unwrap();
    s.project().track(id).unwrap().clone()
}

/// Three takes of the same four bars, as a recording of three laps leaves them.
fn three_takes() -> (Session, String, Vec<String>) {
    let mut s = Session::new("comp");
    let t = track_id(&run(&mut s, "track.add.audio", json!({})));
    let d = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-1.wav"},
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-2.wav"},
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-3.wav"},
        ]}),
    );
    let ids = d["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    (s, t, ids)
}

/// What plays at `tick`: the file of every unmuted clip over it.
fn playing(t: &Track, tick: i64) -> Vec<String> {
    t.clips
        .iter()
        .filter(|c| !c.muted && c.start <= tick && tick < c.start + c.length)
        .map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, .. } => file.clone(),
            _ => c.name.clone(),
        })
        .collect()
}

#[test]
fn a_recording_of_three_laps_is_one_stack_of_three_rows() {
    let (s, t, ids) = three_takes();
    let tr = track(&s, &t);
    let st = takes::stacks(&tr);
    assert_eq!(st.len(), 1);
    assert_eq!((st[0].start, st[0].end, st[0].rows), (0, 7680, 3));
    // Take 1 is in row 0, take 3 in row 2, and only the last plays.
    let row_of = |id: &str| {
        let id: ClipId = id.parse().unwrap();
        st[0].slots.iter().find(|x| x.clip == id).unwrap().clone()
    };
    assert_eq!(row_of(&ids[0]).row, 0);
    assert_eq!(row_of(&ids[2]).row, 2);
    assert!(!row_of(&ids[0]).plays && row_of(&ids[2]).plays);
}

#[test]
fn a_clip_over_nothing_is_not_a_stack() {
    let mut s = Session::new("alone");
    let t = track_id(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 960, "file": "a.wav"}),
    );
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 960, "length": 960, "file": "b.wav"}),
    );
    assert!(takes::stacks(&track(&s, &t)).is_empty());
}

#[test]
fn clips_that_only_crossfade_are_not_takes() {
    let mut s = Session::new("fade");
    let t = track_id(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 3840, "file": "a.wav"}),
    );
    // A tenth of a bar over the end of the first.
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 3456, "length": 3840, "file": "b.wav"}),
    );
    assert!(takes::stacks(&track(&s, &t)).is_empty());
    // Half of it over the first is a take.
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 1920, "length": 3840, "file": "c.wav"}),
    );
    assert_eq!(takes::stacks(&track(&s, &t)).len(), 1);
}

#[test]
fn comping_a_range_plays_the_chosen_take_there_and_keeps_the_rest() {
    let (mut s, t, ids) = three_takes();
    let steps = s.steps().len();
    run(
        &mut s,
        "take.comp",
        json!({"take": ids[0], "from": 1920, "to": 3840}),
    );
    assert_eq!(s.steps().len(), steps + 1, "one step");
    let tr = track(&s, &t);
    assert_eq!(playing(&tr, 100), vec!["Recordings/a-3.wav"]);
    assert_eq!(playing(&tr, 2000), vec!["Recordings/a-1.wav"]);
    assert_eq!(playing(&tr, 3839), vec!["Recordings/a-1.wav"]);
    assert_eq!(playing(&tr, 3840), vec!["Recordings/a-3.wav"]);
    // The cut pieces still read the same audio from the same places.
    let take1: Vec<_> = tr
        .clips
        .iter()
        .filter_map(|c| match &c.body {
            xirius_core::model::ClipBody::Audio { file, offset, .. }
                if file == "Recordings/a-1.wav" =>
            {
                Some((c.start, c.length, *offset))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        take1,
        vec![(0, 1920, 0), (1920, 1920, 1920), (3840, 3840, 3840)]
    );
    // Three takes still, each in the row it was in, the cut ones in one row.
    let st = takes::stacks(&tr);
    assert_eq!((st.len(), st[0].rows), (1, 3));
    s.undo(1).unwrap();
    let tr = track(&s, &t);
    assert_eq!(tr.clips.len(), 3);
    assert_eq!(playing(&tr, 2000), vec!["Recordings/a-3.wav"]);
}

/// A person pressed the take that plays in bar after bar, and every press cut every take at
/// the bar lines although nothing sounded different: eight pieces of one recording, each
/// named after it, and a history of steps that changed nothing to hear.
#[test]
fn comping_the_take_that_already_plays_cuts_nothing() {
    let (mut s, t, ids) = three_takes();
    let before = track(&s, &t);
    let steps = s.steps().len();
    for bar in 0..4 {
        let e = s
            .run_value(
                "take.comp",
                json!({"take": ids[2], "from": bar * 1920, "to": (bar + 1) * 1920}),
                &RunOptions::human(),
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict, "bar {bar}");
        assert!(e.message.contains("すでに"), "{}", e.message);
    }
    assert_eq!(track(&s, &t), before);
    assert_eq!(s.steps().len(), steps);
    assert!(takes::already_plays(
        &before,
        ids[2].parse().unwrap(),
        0,
        7680
    ));
    assert!(!takes::already_plays(
        &before,
        ids[0].parse().unwrap(),
        0,
        7680
    ));
    // The same press on a take that does not play is a change, and then pressing the take
    // that played before is a change again; only the second press of the same one is not.
    run(
        &mut s,
        "take.comp",
        json!({"take": ids[0], "from": 1920, "to": 3840}),
    );
    let cut = track(&s, &t);
    assert_eq!(cut.clips.len(), 3 + 2 * 3);
    let e = s
        .run_value(
            "take.comp",
            json!({"take": ids[0], "from": 1920, "to": 3840}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(track(&s, &t), cut);
    // Over part of the stretch that is already the take's, and part that is not, it runs.
    run(
        &mut s,
        "take.comp",
        json!({"take": ids[0], "from": 1920, "to": 5760}),
    );
    assert_ne!(track(&s, &t), cut);
}

#[test]
fn comping_one_range_after_another_builds_a_comp_from_the_takes() {
    let (mut s, t, ids) = three_takes();
    run(
        &mut s,
        "take.comp",
        json!({"take": ids[0], "from": 0, "to": 3840}),
    );
    // The second range is chosen from the take as it is now, a cut piece of take 2.
    run(
        &mut s,
        "take.comp",
        json!({"take": ids[1], "from": 1920, "to": 5760}),
    );
    let tr = track(&s, &t);
    for (tick, file) in [
        (0, "a-1"),
        (1919, "a-1"),
        (1920, "a-2"),
        (5759, "a-2"),
        (5760, "a-3"),
        (7679, "a-3"),
    ] {
        assert_eq!(
            playing(&tr, tick),
            vec![format!("Recordings/{file}.wav")],
            "at {tick}"
        );
    }
}

#[test]
fn comping_asks_for_a_range_the_take_covers() {
    let (mut s, _, ids) = three_takes();
    let e = s
        .run_value(
            "take.comp",
            json!({"take": ids[0], "from": 7000, "to": 9000}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::OutOfRange);
    let e = s
        .run_value(
            "take.comp",
            json!({"take": ids[0], "from": 960, "to": 960}),
            &RunOptions::human(),
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::OutOfRange);
}

#[test]
fn midi_takes_are_comped_by_cutting_their_notes() {
    let mut s = Session::new("midi");
    let t = track_id(&run(&mut s, "track.add.instrument", json!({})));
    let d = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": t, "start": 0, "length": 1920,
             "notes": [{"start": 0, "length": 240, "pitch": 60}, {"start": 960, "length": 240, "pitch": 62}]},
            {"track": t, "start": 0, "length": 1920,
             "notes": [{"start": 0, "length": 240, "pitch": 72}, {"start": 960, "length": 240, "pitch": 74}]},
        ]}),
    );
    let first = d["clips"][0].as_str().unwrap().to_string();
    run(
        &mut s,
        "take.comp",
        json!({"take": first, "from": 480, "to": 1920}),
    );
    let tr = track(&s, &t);
    let mut heard: Vec<(i64, u8)> = Vec::new();
    for c in tr.clips.iter().filter(|c| !c.muted) {
        for n in c.notes().iter() {
            heard.push((c.start + n.start, n.pitch));
        }
    }
    heard.sort();
    assert_eq!(heard, vec![(0, 72), (960, 62)]);
}

/// The name of each clip of the first stack, by the row it is in (`T{row + 1}`).
fn names_by_row(t: &Track) -> Vec<(usize, String)> {
    let st = takes::stacks(t);
    let mut out: Vec<(usize, String)> = st[0]
        .slots
        .iter()
        .map(|x| {
            let c = t.clips.iter().find(|c| c.id == x.clip).unwrap();
            (x.row, c.name.clone())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_take_is_named_after_its_track() {
    // A recording over a clip that was already there: its three laps are rows 2 to 4 (T2..T4),
    // each named after the track and each a take of its own (UI-296).
    let mut s = Session::new("rows");
    let t = track_id(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": 0, "length": 7680, "file": "old.wav"}),
    );
    run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-1.wav"},
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-2.wav"},
            {"track": t, "start": 0, "length": 7680, "file": "Recordings/a-3.wav"},
        ]}),
    );
    let tr = track(&s, &t);
    let rows = names_by_row(&tr);
    assert_eq!(rows.len(), 4);
    assert!(rows[1..].iter().all(|(_, n)| *n == tr.name), "{rows:?}");
    assert_eq!(
        tr.clips[0].take, None,
        "the clip that was there was not recorded"
    );
    assert!(tr.clips[1..].iter().all(|c| c.take == Some(c.id)));
}

#[test]
fn midi_takes_named_alike_keep_their_rows_when_comped() {
    // Both takes are called after the track and have no file: each piece of a take cut by the
    // comp stays in that take's row, by the take it keeps (UI-296).
    let mut s = Session::new("midi rows");
    let t = track_id(&run(&mut s, "track.add.instrument", json!({})));
    let d = run(
        &mut s,
        "record.take.add",
        json!({"takes": [
            {"track": t, "start": 0, "length": 1920,
             "notes": [{"start": 0, "length": 240, "pitch": 60}]},
            {"track": t, "start": 0, "length": 1920,
             "notes": [{"start": 0, "length": 240, "pitch": 72}]},
        ]}),
    );
    let first: ClipId = d["clips"][0].as_str().unwrap().parse().unwrap();
    run(
        &mut s,
        "take.comp",
        json!({"take": first.to_string(), "from": 480, "to": 960}),
    );
    let tr = track(&s, &t);
    assert!(tr.clips.iter().all(|c| c.name == tr.name));
    let st = takes::stacks(&tr);
    assert_eq!(st.len(), 1);
    assert_eq!(st[0].rows, 2);
    for x in &st[0].slots {
        let c = tr.clips.iter().find(|c| c.id == x.clip).unwrap();
        assert_eq!(c.take == Some(first), x.row == 0, "{:?}", st[0].slots);
    }
}
