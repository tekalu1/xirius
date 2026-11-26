//! EDT-137 / EDT-138: no overlap when editing. With `--overlap trim`, an edit that lays a note
//! over another of the same pitch cuts the earlier one's end at the later one's start (a note
//! wholly covered goes); with `--no-overlap`, an edit that lays a clip over another of the same
//! track cuts away what lies under it (in the middle, two pieces stay). Without them, things
//! overlap as before. Each table row is one edit on a fresh song: what is left, that it is one
//! history step, that one undo puts everything back, and (for clips) that another track is left
//! alone.
use serde_json::{Value, json};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::ClipBody;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<Value, String> {
    s.run_value(name, args, &RunOptions::human())
        .map(|o| o.data)
        .map_err(|e| format!("{name}: {e:?}"))
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

/// The arguments of a row's command, from the ids the row's song was made with (note ids and
/// the clip, or clip ids and the track).
type Args = fn(&[String], &str) -> Value;

/// A command run before the row's edit, as part of making its song.
fn pre(cmd: &'static str, args: Args) -> (&'static str, Args) {
    (cmd, args)
}

/// Runs the commands, then the edit; checks it is one step and that one undo goes back. `made`
/// is the ids and the clip or track the song was made with, as `Args` gets them.
fn one_step_that_undoes<T: PartialEq + std::fmt::Debug>(
    s: &mut Session,
    (ids, at): (&[String], &str),
    before: &[(&'static str, Args)],
    cmd: &str,
    args: Args,
    look: impl Fn(&Session) -> T,
    check: impl FnOnce(&Session) -> Result<(), String>,
) -> Result<(), String> {
    for (name, args) in before {
        try_run(s, name, args(ids, at))?;
    }
    let start = look(s);
    let steps = s.steps().len();
    try_run(s, cmd, args(ids, at))?;
    check(s)?;
    let made = s.steps().len() as i64 - steps as i64;
    if made != 1 {
        return Err(format!("{made} history steps, want 1"));
    }
    s.undo(1).map_err(|e| format!("undo: {e:?}"))?;
    let back = look(s);
    if back != start {
        return Err(format!("after undo {back:?}, want {start:?}"));
    }
    Ok(())
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

struct NoteRow {
    name: &'static str,
    /// The notes in the clip before the edit; their ids are what `args` gets.
    start: &'static [(i64, i64, u8)],
    before: Vec<(&'static str, Args)>,
    cmd: &'static str,
    args: Args,
    want: &'static [(i64, i64, u8)],
    /// Which of the first notes are still there, by place in `start`, where it matters.
    kept: Option<&'static [usize]>,
}

fn note_row(r: &NoteRow) -> Result<(), String> {
    let (mut s, c) = roll();
    let n = put(&mut s, &c, r.start);
    let look = |s: &Session| notes(s, &c);
    let check = |s: &Session| {
        let got = notes(s, &c);
        if got != r.want {
            return Err(format!("notes {got:?}, want {:?}", r.want));
        }
        if let Some(kept) = r.kept {
            let (_, clip) = s.project().clip(c.parse().unwrap()).unwrap();
            let mut got: Vec<String> = clip.notes().iter().map(|x| x.id.to_string()).collect();
            let mut want: Vec<String> = kept.iter().map(|&i| n[i].clone()).collect();
            got.sort();
            want.sort();
            if got != want {
                return Err(format!("note ids {got:?}, want {want:?}"));
            }
        }
        Ok(())
    };
    one_step_that_undoes(&mut s, (&n, &c), &r.before, r.cmd, r.args, look, check)
}

#[test]
fn every_note_edit_with_trim_cuts_only_what_it_lands_on_in_one_step() {
    let rows = [
        NoteRow {
            name: "moving a note over the tail of another cuts that one's end",
            start: &[(0, 480, 60), (960, 480, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[1]], "by": -720, "overlap": "trim"}),
            want: &[(0, 240, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "with allow notes overlap as before",
            start: &[(0, 480, 60), (960, 480, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[1]], "by": -720, "overlap": "allow"}),
            want: &[(0, 480, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "nothing said is the same as allow",
            start: &[(0, 480, 60), (960, 480, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[1]], "by": -720}),
            want: &[(0, 480, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "moving a note onto the head of another cuts the moved one's end",
            start: &[(0, 480, 60), (960, 480, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[0]], "by": 720, "overlap": "trim"}),
            want: &[(720, 240, 60), (960, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "a note wholly covered goes and the other pitch stays",
            start: &[(0, 480, 60), (960, 480, 60), (960, 480, 64)],
            before: vec![],
            cmd: "note.resize",
            args: |n, _| json!({"ids": [n[0]], "length": 2000, "overlap": "trim"}),
            want: &[(0, 2000, 60), (960, 480, 64)],
            kept: Some(&[0, 2]),
        },
        NoteRow {
            // The long one ends where the short one starts; its rest is not made again.
            name: "a note put into the middle of another cuts it there",
            start: &[(0, 1920, 60), (3840, 240, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[1]], "by": -2880, "overlap": "trim"}),
            want: &[(0, 960, 60), (960, 240, 60)],
            kept: None,
        },
        NoteRow {
            name: "of two notes that start together the moved one stays",
            start: &[(0, 960, 60), (960, 480, 60)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[1]], "by": -960, "overlap": "trim"}),
            want: &[(0, 480, 60)],
            kept: Some(&[1]),
        },
        NoteRow {
            name: "an overlap the edit did not touch stays",
            start: &[(0, 480, 60), (240, 480, 60), (3840, 240, 62)],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[2]], "by": 240, "overlap": "trim"}),
            want: &[(0, 480, 60), (240, 480, 60), (4080, 240, 62)],
            kept: None,
        },
        NoteRow {
            // The two moved 60s keep touching end to start; the second ends at the third's
            // start, and the moved 62 at the other 62's start.
            name: "moving several notes trims each and not one another",
            start: &[
                (0, 480, 60),
                (480, 480, 60),
                (1440, 480, 60),
                (0, 960, 62),
                (1440, 480, 62),
            ],
            before: vec![],
            cmd: "note.move",
            args: |n, _| json!({"ids": [n[0], n[1], n[3]], "by": 720, "overlap": "trim"}),
            want: &[
                (720, 480, 60),
                (720, 720, 62),
                (1200, 240, 60),
                (1440, 480, 60),
                (1440, 480, 62),
            ],
            kept: None,
        },
        NoteRow {
            name: "adding a note trims the one it lands on",
            start: &[(0, 480, 60)],
            before: vec![],
            cmd: "note.add",
            args: |_, c| json!({"clip": c, "at": 240, "pitch": 60, "length": 480, "overlap": "trim"}),
            want: &[(0, 240, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            // The new one at 1920 starts with the old one and stays; the old one goes.
            name: "pencil notes trim what they land on",
            start: &[(0, 960, 60), (1920, 480, 60)],
            before: vec![],
            cmd: "note.put",
            args: |_, c| {
                json!({"clip": c, "overlap": "trim", "notes": [
                    {"at": 480, "pitch": 60, "length": 240},
                    {"at": 1920, "pitch": 60, "length": 960},
                ]})
            },
            want: &[(0, 480, 60), (480, 240, 60), (1920, 960, 60)],
            kept: None,
        },
        NoteRow {
            // The drag moves the 62 down onto the 60's pitch, over its tail.
            name: "a drag committed with note.set trims",
            start: &[(0, 480, 60), (960, 480, 62)],
            before: vec![],
            cmd: "note.set",
            args: |n, _| json!({"overlap": "trim", "notes": [{"id": n[1], "start": 240, "pitch": 60}]}),
            want: &[(0, 240, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "quantizing a note into another trims",
            start: &[(1440, 960, 60), (1930, 480, 60)],
            before: vec![],
            cmd: "note.quantize",
            args: |n, _| json!({"ids": [n[1]], "grid": "1/16", "overlap": "trim"}),
            want: &[(1440, 480, 60), (1920, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "pasting notes trims",
            start: &[(0, 480, 60)],
            before: vec![pre("edit.copy", |n, _| json!({"ids": [n[0]]}))],
            cmd: "edit.paste",
            args: |_, c| json!({"clip": c, "at": 240, "overlap": "trim"}),
            want: &[(0, 240, 60), (240, 480, 60)],
            kept: None,
        },
        NoteRow {
            name: "duplicating notes trims",
            start: &[(0, 480, 60)],
            before: vec![],
            cmd: "note.duplicate",
            args: |n, _| json!({"ids": [n[0]], "by": 120, "overlap": "trim"}),
            want: &[(0, 120, 60), (120, 480, 60)],
            kept: None,
        },
    ];
    let mut failures = Vec::new();
    for r in &rows {
        if let Err(e) = note_row(r) {
            failures.push(format!("{}: {e}", r.name));
        }
    }
    assert!(failures.is_empty(), "note edits: {failures:#?}");
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

/// The clips of a track as (start, length, audio offset; 0 for a MIDI clip), in time order.
fn layout(s: &Session, track: &str) -> Vec<(i64, i64, i64)> {
    let id: TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .map(|c| match &c.body {
            ClipBody::Audio { offset, .. } => (c.start, c.length, *offset),
            _ => (c.start, c.length, 0),
        })
        .collect();
    v.sort_unstable();
    v
}

/// Where the notes of a track's clips sound, as (song position, pitch), in order; a note
/// before its clip's start is not heard.
fn heard(s: &Session, track: &str) -> Vec<(i64, u8)> {
    let id: TrackId = track.parse().unwrap();
    let mut v: Vec<_> = s.project().tracks[&id]
        .clips
        .iter()
        .flat_map(|c| {
            c.notes()
                .iter()
                .filter(|n| n.start >= 0)
                .map(|n| (c.start + n.start, n.pitch))
        })
        .collect();
    v.sort_unstable();
    v
}

/// The track a row's edit is made on.
#[derive(Clone, Copy)]
enum Lane {
    /// An instrument track with MIDI clips `(start, length)`.
    Midi(&'static [(i64, i64)]),
    /// An audio track with clips `(start, length)`, each of a file of its own.
    Audio(&'static [(i64, i64)]),
    /// An instrument track with three takes of 3840 at 1920, as a recording of three laps
    /// leaves them (the last one plays), and a clip of its own of 1920 at 6720; the ids are the
    /// three takes and then the clip.
    Takes,
}

fn make(lane_of: Lane) -> (Session, String, Vec<String>) {
    match lane_of {
        Lane::Midi(at) => lane(at),
        Lane::Audio(at) => {
            let mut s = Session::new("no-overlap-audio");
            let t = id_of(&run(&mut s, "track.add.audio", json!({})));
            let clips = at
                .iter()
                .enumerate()
                .map(|(i, &(at, length))| {
                    let file = format!("{i}.wav");
                    id_of(&run(
                        &mut s,
                        "clip.add.audio",
                        json!({"track": t, "at": at, "length": length, "file": file}),
                    ))
                })
                .collect();
            (s, t, clips)
        }
        Lane::Takes => {
            let mut s = Session::new("no-overlap-takes");
            let t = id_of(&run(&mut s, "track.add.instrument", json!({})));
            let take = json!({"track": t, "start": 1920, "length": 3840,
                "notes": [{"start": 0, "length": 480, "pitch": 60}]});
            let d = run(
                &mut s,
                "record.take.add",
                json!({"takes": [take, take, take]}),
            );
            let mut ids: Vec<String> = d["clips"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            ids.push(id_of(&run(
                &mut s,
                "clip.add.midi",
                json!({"track": t, "at": 6720, "length": 1920}),
            )));
            let id: TrackId = t.parse().unwrap();
            assert_eq!(
                xirius_core::takes::stacks(&s.project().tracks[&id])[0].rows,
                3
            );
            (s, t, ids)
        }
    }
}

struct ClipRow {
    name: &'static str,
    lane: Lane,
    before: Vec<(&'static str, Args)>,
    cmd: &'static str,
    args: Args,
    /// The clips of the track after the edit, as (start, length), in time order.
    want: &'static [(i64, i64)],
    /// Their audio offsets, where it matters.
    offsets: &'static [i64],
    /// Where their notes sound, where it matters.
    heard: &'static [(i64, u8)],
}

fn clip_row(r: &ClipRow) -> Result<(), String> {
    let (mut s, t, c) = make(r.lane);
    // A clip on another track under everything: no edit here cuts it.
    let other = id_of(&run(&mut s, "track.add.instrument", json!({})));
    run(
        &mut s,
        "clip.add.midi",
        json!({"track": other, "at": 0, "length": 15360}),
    );
    let look = |s: &Session| (layout(s, &t), layout(s, &other));
    let untouched = layout(&s, &other);
    let check = |s: &Session| {
        let got = layout(s, &t);
        let got_spans: Vec<(i64, i64)> = got.iter().map(|&(at, len, _)| (at, len)).collect();
        if got_spans != r.want {
            return Err(format!("clips {got_spans:?}, want {:?}", r.want));
        }
        let offsets: Vec<i64> = got.iter().map(|x| x.2).collect();
        if !r.offsets.is_empty() && offsets != r.offsets {
            return Err(format!("offsets {offsets:?}, want {:?}", r.offsets));
        }
        let h = heard(s, &t);
        if !r.heard.is_empty() && h != r.heard {
            return Err(format!("notes heard at {h:?}, want {:?}", r.heard));
        }
        if layout(s, &other) != untouched {
            return Err(format!("the other track changed: {:?}", layout(s, &other)));
        }
        Ok(())
    };
    one_step_that_undoes(&mut s, (&c, &t), &r.before, r.cmd, r.args, look, check)
}

fn clip_rows(rows: &[ClipRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| clip_row(r).err().map(|e| format!("{}: {e}", r.name)))
        .collect()
}

#[test]
fn every_clip_edit_with_no_overlap_cuts_only_what_it_lands_on_in_one_step() {
    let rows = [
        ClipRow {
            name: "moving a clip over the tail of another trims it",
            lane: Lane::Midi(&[(0, 960), (1920, 960)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 480, "no-overlap": true}),
            want: &[(0, 480), (480, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "with no-overlap off clips overlap as before",
            lane: Lane::Midi(&[(0, 960), (1920, 960)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 480, "no-overlap": false}),
            want: &[(0, 960), (480, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "nothing said is the same as no-overlap off",
            lane: Lane::Midi(&[(0, 960), (1920, 960)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 240}),
            want: &[(0, 960), (240, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            // The note under the moved clip goes with the trimmed head; the other stays where
            // it sounds.
            name: "moving a clip over the head of another trims its start and keeps its notes in place",
            lane: Lane::Midi(&[(0, 960), (1920, 960)]),
            before: vec![pre("note.put", |c, _| {
                json!({"clip": c[1], "notes": [
                    {"at": 0, "length": 240, "pitch": 60},
                    {"at": 720, "length": 240, "pitch": 62},
                ]})
            })],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[0]], "at": 1440, "no-overlap": true}),
            want: &[(1440, 960), (2400, 480)],
            offsets: &[],
            heard: &[(2640, 62)],
        },
        ClipRow {
            name: "a clip wholly under a resized one goes",
            lane: Lane::Midi(&[(0, 960), (1920, 960), (4800, 960)]),
            before: vec![],
            cmd: "clip.resize",
            args: |c, _| json!({"ids": [c[0]], "length": 3000, "no-overlap": true}),
            want: &[(0, 3000), (4800, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "a clip dropped into the middle of another splits it",
            lane: Lane::Midi(&[(0, 3840), (5760, 480)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 1920, "no-overlap": true}),
            want: &[(0, 1920), (1920, 480), (2400, 1440)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "trimming a clip's start over another trims it",
            lane: Lane::Midi(&[(0, 960), (1920, 960)]),
            before: vec![],
            cmd: "clip.trim",
            args: |c, _| json!({"ids": [c[1]], "by": -1440, "no-overlap": true}),
            want: &[(0, 480), (480, 2400)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "duplicating a clip trims what it lands on",
            lane: Lane::Midi(&[(0, 960), (1440, 960)]),
            before: vec![],
            cmd: "clip.duplicate",
            args: |c, _| json!({"ids": [c[0]], "no-overlap": true}),
            want: &[(0, 960), (960, 960), (1920, 480)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "pasting a clip trims what it lands on",
            lane: Lane::Midi(&[(0, 960)]),
            before: vec![pre("edit.copy", |c, _| json!({"ids": [c[0]]}))],
            cmd: "edit.paste",
            args: |_, t| json!({"at": 480, "track": t, "no-overlap": true}),
            want: &[(0, 480), (480, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            // The two moved clips still touch; the third loses its head under the second.
            name: "moving several clips trims what is under each and not one another",
            lane: Lane::Midi(&[(0, 960), (960, 960), (3840, 960)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[0], c[1]], "by": 2400, "no-overlap": true}),
            want: &[(2400, 960), (3360, 960), (4320, 480)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "an audio clip trimmed at its head plays on from the same place",
            lane: Lane::Audio(&[(0, 3840), (5760, 960)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 0, "no-overlap": true}),
            want: &[(0, 960), (960, 2880)],
            offsets: &[0, 960],
            heard: &[],
        },
        ClipRow {
            name: "an audio clip split by one dropped into it keeps its offset on both pieces",
            lane: Lane::Audio(&[(0, 3840), (5760, 480)]),
            before: vec![],
            cmd: "clip.move",
            args: |c, _| json!({"ids": [c[1]], "at": 960, "no-overlap": true}),
            want: &[(0, 960), (960, 480), (1440, 2400)],
            offsets: &[0, 0, 1440],
            heard: &[],
        },
        ClipRow {
            // The crossfade leaves (0, 1020) and (900, 1020): the right clip of the pair stays
            // as it was and the left grows by the 30.
            name: "a crossfading pair keeps its overlap",
            lane: Lane::Audio(&[(0, 960), (960, 960)]),
            before: vec![
                pre("clip.slip", |c, _| json!({"ids": [c[1]], "by": -480})),
                pre(
                    "clip.fade.cross",
                    |c, _| json!({"ids": [c[0], c[1]], "length": 120}),
                ),
            ],
            cmd: "clip.resize",
            args: |c, _| json!({"ids": [c[0]], "by": 30, "no-overlap": true}),
            want: &[(0, 1050), (900, 1020)],
            offsets: &[],
            heard: &[],
        },
        // EDT-139: the takes of one stack do not cut one another.
        ClipRow {
            name: "moving a take leaves the other takes of its stack whole",
            lane: Lane::Takes,
            before: vec![],
            cmd: "clip.move",
            args: |k, _| json!({"ids": [k[2]], "by": -960, "no-overlap": true}),
            want: &[(960, 3840), (1920, 3840), (1920, 3840), (6720, 1920)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "a take that was not playing does not cut the one that plays",
            lane: Lane::Takes,
            before: vec![pre(
                "clip.move",
                |k, _| json!({"ids": [k[2]], "by": -960, "no-overlap": true}),
            )],
            cmd: "clip.move",
            args: |k, _| json!({"ids": [k[0]], "by": 960, "no-overlap": true}),
            want: &[(960, 3840), (1920, 3840), (2880, 3840), (6720, 1920)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "resizing and trimming a take leaves the other takes whole",
            lane: Lane::Takes,
            before: vec![
                pre(
                    "clip.resize",
                    |k, _| json!({"ids": [k[1]], "length": 1920, "no-overlap": true}),
                ),
                pre(
                    "clip.trim",
                    |k, _| json!({"ids": [k[2]], "by": -960, "no-overlap": true}),
                ),
            ],
            cmd: "clip.resize",
            args: |k, _| json!({"ids": [k[1]], "length": 3840, "no-overlap": true}),
            want: &[(960, 4800), (1920, 3840), (1920, 3840), (6720, 1920)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "the take that plays, moved onto the clip after the stack, cuts its head away",
            lane: Lane::Takes,
            before: vec![],
            cmd: "clip.move",
            args: |k, _| json!({"ids": [k[2]], "by": 1920, "no-overlap": true}),
            want: &[(1920, 3840), (1920, 3840), (3840, 3840), (7680, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "a clip of its own dropped into the takes splits each take under it",
            lane: Lane::Takes,
            before: vec![],
            cmd: "clip.move",
            args: |k, _| json!({"ids": [k[3]], "at": 2880, "no-overlap": true}),
            want: &[
                (1920, 960),
                (1920, 960),
                (1920, 960),
                (2880, 1920),
                (4800, 960),
                (4800, 960),
                (4800, 960),
            ],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            // The first take plays over its second half, so the comp cut the takes at 3840;
            // the head of the third take, moved over the second halves of all three, cuts none.
            name: "the pieces of a comped take stay in its stack",
            lane: Lane::Takes,
            before: vec![pre(
                "take.comp",
                |k, _| json!({"take": k[0], "from": 3840, "to": 5760}),
            )],
            cmd: "clip.move",
            args: |k, _| json!({"ids": [k[2]], "by": 480, "no-overlap": true}),
            want: &[
                (1920, 1920),
                (1920, 1920),
                (2400, 1920),
                (3840, 1920),
                (3840, 1920),
                (3840, 1920),
                (6720, 1920),
            ],
            offsets: &[],
            heard: &[],
        },
    ];
    let failures = clip_rows(&rows);
    assert!(failures.is_empty(), "clip edits: {failures:#?}");
}

#[test]
fn with_ripple_the_clips_pushed_along_cut_nothing_in_one_step() {
    let rows = [
        ClipRow {
            // A, a gap, then B and C that overlap a little (as a crossfade would): A pushes B
            // and C by as much, and B and C keep their overlap.
            name: "the clips pushed along do not cut one another",
            lane: Lane::Midi(&[(0, 960), (1440, 960), (2160, 960)]),
            before: vec![],
            cmd: "clip.resize",
            args: |c, _| json!({"ids": [c[0]], "by": 960, "ripple": true, "no-overlap": true}),
            want: &[(0, 1920), (2400, 960), (3120, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "pulled back, nothing is cut either",
            lane: Lane::Midi(&[(0, 960), (1440, 960), (2160, 960)]),
            before: vec![],
            cmd: "clip.resize",
            args: |c, _| json!({"ids": [c[0]], "by": -480, "ripple": true, "no-overlap": true}),
            want: &[(0, 480), (960, 960), (1680, 960)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            // X starts under A's end and runs on under B, too little to be a take of B: it
            // starts before A ends, so it is not pushed. A grows to 1200 and cuts X's head
            // there; B, pushed to 1200, leaves the rest of X alone.
            name: "the edited clip cuts what lay over its end and the pushed ones cut nothing",
            lane: Lane::Midi(&[(0, 960), (960, 1920), (720, 720)]),
            before: vec![],
            cmd: "clip.resize",
            args: |c, _| json!({"ids": [c[0]], "by": 240, "ripple": true, "no-overlap": true}),
            want: &[(0, 1200), (1200, 240), (1200, 1920)],
            offsets: &[],
            heard: &[],
        },
        ClipRow {
            name: "a take grown over the others pushes what follows and cuts no take",
            lane: Lane::Takes,
            before: vec![],
            cmd: "clip.resize",
            args: |k, _| json!({"ids": [k[2]], "by": 960, "ripple": true, "no-overlap": true}),
            want: &[(1920, 3840), (1920, 3840), (1920, 4800), (7680, 1920)],
            offsets: &[],
            heard: &[],
        },
    ];
    let failures = clip_rows(&rows);
    assert!(failures.is_empty(), "ripple edits: {failures:#?}");
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
