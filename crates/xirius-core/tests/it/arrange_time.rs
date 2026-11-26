//! Time put into the song and taken out of it (ARR-005), sections rearranged and copied
//! (ARR-002, ARR-004) and kept from overlapping (ARR-007).

use serde_json::{Value, json};
use xirius_core::CmdError;
use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, RunResult, Session};

const BAR: i64 = 3840;

fn run(s: &mut Session, name: &str, args: Value) -> RunResult {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
}

fn try_run(s: &mut Session, name: &str, args: Value) -> Result<RunResult, CmdError> {
    s.run_value(name, args, &RunOptions::human())
}

fn id_of(r: &RunResult) -> String {
    r.data["id"]
        .as_str()
        .or_else(|| r.data["ids"][0].as_str())
        .unwrap_or_else(|| panic!("no id in {:?}", r.data))
        .to_string()
}

fn clips_of(p: &Project, t: &str) -> Vec<(i64, i64)> {
    let id: TrackId = t.parse().unwrap();
    let mut v: Vec<(i64, i64)> = p.tracks[&id]
        .clips
        .iter()
        .map(|c| (c.start, c.length))
        .collect();
    v.sort();
    v
}

fn lane(p: &Project, t: &str) -> Vec<(i64, f64)> {
    let id: TrackId = t.parse().unwrap();
    p.tracks[&id].automation[0]
        .points
        .iter()
        .map(|x| (x.tick, x.value))
        .collect()
}

fn sections(p: &Project) -> Vec<(String, i64, i64)> {
    p.sections
        .iter()
        .map(|s| (s.name.clone(), s.start, s.length))
        .collect()
}

fn tempo(p: &Project) -> Vec<(i64, f64)> {
    p.tempo.iter().map(|t| (t.tick, t.bpm)).collect()
}

struct Song {
    s: Session,
    midi: String,
    audio: String,
}

/// Verse bars 1–2, Chorus bars 3–4; a MIDI clip over all four bars with a note in each,
/// an audio clip over bars 3–4, a volume line, a tempo change at the Chorus, a marker in
/// the Chorus and a loop over bars 2–3.
fn song() -> Song {
    let mut s = Session::new("time");
    let midi = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let audio = id_of(&run(&mut s, "track.add.audio", json!({})));
    let clip = id_of(&run(
        &mut s,
        "clip.add.midi",
        json!({"track": midi, "at": 0, "length": 4 * BAR}),
    ));
    for bar in 0..4 {
        run(
            &mut s,
            "note.add",
            json!({"clip": clip, "at": bar * BAR + 100, "pitch": 60, "length": 200}),
        );
    }
    // the tempo first: an audio clip follows the tempo, so it is placed once the map is set
    run(&mut s, "tempo.set", json!({"bpm": 100}));
    run(&mut s, "tempo.set", json!({"bpm": 140, "at": 2 * BAR}));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": audio, "at": 2 * BAR, "length": 2 * BAR, "file": "a.wav"}),
    );
    run(
        &mut s,
        "auto.lane.add",
        json!({"track": audio, "target": "volume"}),
    );
    for (at, v) in [(0, -20.0), (4 * BAR, 0.0)] {
        run(
            &mut s,
            "auto.point.add",
            json!({"track": audio, "target": "volume", "at": at, "value": v}),
        );
    }
    run(
        &mut s,
        "section.add",
        json!({"name": "Verse", "at": 0, "bars": 2}),
    );
    run(
        &mut s,
        "section.add",
        json!({"name": "Chorus", "at": 2 * BAR, "bars": 2}),
    );
    run(&mut s, "marker.add", json!({"at": 3 * BAR, "name": "Hook"}));
    run(&mut s, "loop.set", json!({"start": BAR, "end": 3 * BAR}));
    Song { s, midi, audio }
}

#[test]
fn inserting_time_moves_the_whole_song_and_splits_what_it_lands_in() {
    let Song { mut s, midi, audio } = song();
    let before = s.project().clone();
    // a bar of nothing a little way into the second Verse bar
    let at = BAR + 1000;
    run(
        &mut s,
        "range.time.insert",
        json!({"start": at, "length": BAR}),
    );
    let p = s.project();
    // the long MIDI clip is cut where the time goes in
    assert_eq!(clips_of(p, &midi), vec![(0, at), (at + BAR, 4 * BAR - at)]);
    // the notes go with their halves, keeping their place in them
    let id: TrackId = midi.parse().unwrap();
    let mut clips: Vec<_> = p.tracks[&id].clips.iter().collect();
    clips.sort_by_key(|c| c.start);
    let starts = |i: usize| -> Vec<i64> { clips[i].notes().iter().map(|n| n.start).collect() };
    assert_eq!(starts(0), vec![100, BAR + 100]);
    assert_eq!(starts(1), vec![2 * BAR + 100 - at, 3 * BAR + 100 - at]);
    assert_eq!(clips_of(p, &audio), vec![(3 * BAR, 2 * BAR)]);
    // the Verse holds the time and grows; the Chorus moves back
    assert_eq!(
        sections(p),
        vec![
            ("Verse".to_string(), 0, 3 * BAR),
            ("Chorus".to_string(), 3 * BAR, 2 * BAR)
        ]
    );
    // the marker rides along, the loop that held the place grows
    assert_eq!(p.markers[0].tick, 4 * BAR);
    let l = p.loop_range.unwrap();
    assert_eq!((l.start, l.end), (BAR, 4 * BAR));
    // the line holds across the gap and goes on after it
    let v = lane(p, &audio);
    assert_eq!(v.first(), Some(&(0, -20.0)));
    assert_eq!(v.last(), Some(&(5 * BAR, 0.0)));
    let held: Vec<f64> = v
        .iter()
        .filter(|x| x.0 == at || x.0 == at + BAR)
        .map(|x| x.1)
        .collect();
    assert_eq!(held.len(), 2, "{v:?}");
    assert!((held[0] - held[1]).abs() < 1e-9, "flat across the gap");
    // the 140 begins a bar later; what came before is as it was
    assert_eq!(tempo(p), vec![(0, 100.0), (3 * BAR, 140.0)]);
    // one undo takes all of it back
    s.undo(1).unwrap();
    assert_eq!(s.project(), &before);
}

#[test]
fn time_goes_in_by_range_loop_or_section() {
    let Song { mut s, midi, .. } = song();
    let before = s.project().clone();
    // the loop is bars 2–3: two bars go in at its start
    run(&mut s, "range.time.insert", json!({"range": "loop"}));
    assert_eq!(sections(s.project())[0], ("Verse".to_string(), 0, 4 * BAR));
    assert!(clips_of(s.project(), &midi).contains(&(3 * BAR, 3 * BAR)));
    s.undo(1).unwrap();
    run(
        &mut s,
        "range.time.insert",
        json!({"range": "section:Chorus"}),
    );
    assert_eq!(
        sections(s.project())[1],
        ("Chorus".to_string(), 4 * BAR, 2 * BAR)
    );
    s.undo(1).unwrap();
    run(&mut s, "range.time.insert", json!({"range": "0..960"}));
    // at the Verse's own start the Verse is pushed back, not stretched
    assert_eq!(
        sections(s.project())[0],
        ("Verse".to_string(), 960, 2 * BAR)
    );
    s.undo(1).unwrap();
    assert_eq!(s.project(), &before);
    // a range that ends before it starts is refused and changes nothing
    assert!(try_run(&mut s, "range.time.insert", json!({"range": "960..0"})).is_err());
    assert_eq!(s.project(), &before);
}

#[test]
fn deleting_time_closes_up_the_whole_song() {
    let Song { mut s, midi, audio } = song();
    let before = s.project().clone();
    // bar 2: the second Verse bar goes
    run(
        &mut s,
        "range.time.delete",
        json!({"start": BAR, "end": 2 * BAR}),
    );
    let p = s.project();
    // the MIDI clip is cut at both edges of the range and the right piece closes up
    assert_eq!(clips_of(p, &midi), vec![(0, BAR), (BAR, 2 * BAR)]);
    assert_eq!(clips_of(p, &audio), vec![(BAR, 2 * BAR)]);
    assert_eq!(
        sections(p),
        vec![
            ("Verse".to_string(), 0, BAR),
            ("Chorus".to_string(), BAR, 2 * BAR)
        ]
    );
    assert_eq!(p.markers[0].tick, 2 * BAR);
    // the loop was bars 2–3: the bar that went was half of it
    let l = p.loop_range.unwrap();
    assert_eq!((l.start, l.end), (BAR, 2 * BAR));
    assert_eq!(tempo(p), vec![(0, 100.0), (BAR, 140.0)]);
    s.undo(1).unwrap();
    assert_eq!(s.project(), &before);
}

#[test]
fn deleting_a_section_or_the_loop_takes_out_what_it_held() {
    let Song { mut s, midi, .. } = song();
    run(
        &mut s,
        "range.time.delete",
        json!({"range": "section:Verse"}),
    );
    let p = s.project();
    assert_eq!(sections(p), vec![("Chorus".to_string(), 0, 2 * BAR)]);
    assert_eq!(clips_of(p, &midi), vec![(0, 2 * BAR)]);
    assert_eq!(p.markers[0].tick, BAR);
    s.undo(1).unwrap();
    // the loop goes with the time it was
    run(&mut s, "range.time.delete", json!({"range": "loop"}));
    let p = s.project();
    assert!(p.loop_range.is_none(), "a loop with no time left is gone");
    assert_eq!(
        sections(p),
        vec![
            ("Verse".to_string(), 0, BAR),
            ("Chorus".to_string(), BAR, BAR)
        ]
    );
}

#[test]
fn deleted_time_leaves_the_line_at_what_it_was_after_it() {
    let Song { mut s, audio, .. } = song();
    let id: TrackId = audio.parse().unwrap();
    let was = s.project().tracks[&id].automation[0].clone();
    run(
        &mut s,
        "range.time.delete",
        json!({"start": BAR, "end": 2 * BAR}),
    );
    let now = &s.project().tracks[&id].automation[0];
    // the line runs as it did up to the cut, and from the cut as it did after the time
    let at = |l: &xirius_core::auto::AutoLane, t: i64| l.curve_at(t as f64).unwrap();
    assert!((at(now, BAR - 1) - at(&was, BAR - 1)).abs() < 1e-6);
    assert!((at(now, BAR) - at(&was, 2 * BAR)).abs() < 1e-6);
    assert!((at(now, 2 * BAR) - at(&was, 3 * BAR)).abs() < 1e-6);
    assert!(
        at(&was, 2 * BAR) > at(&was, BAR),
        "the cut skipped part of a rising line"
    );
    assert_eq!(lane(s.project(), &audio).last().map(|x| x.0), Some(3 * BAR));
}

#[test]
fn a_section_moved_across_a_clip_cuts_the_clip_at_the_edge() {
    let mut s = Session::new("edge");
    let t = id_of(&run(&mut s, "track.add.audio", json!({})));
    run(
        &mut s,
        "clip.add.audio",
        json!({"track": t, "at": BAR, "length": 2 * BAR, "file": "x.wav"}),
    );
    run(
        &mut s,
        "section.add",
        json!({"name": "A", "at": 0, "bars": 2}),
    );
    let b = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "B", "at": 2 * BAR, "bars": 2}),
    ));
    run(&mut s, "section.reorder", json!({"id": b, "index": 0}));
    // the clip's first bar was in A and went back with it, its second went to the front
    assert_eq!(clips_of(s.project(), &t), vec![(0, BAR), (3 * BAR, BAR)]);
    s.undo(1).unwrap();
    assert_eq!(clips_of(s.project(), &t), vec![(BAR, 2 * BAR)]);
}

#[test]
fn markers_follow_a_moved_section_and_the_loop_stays() {
    let Song { mut s, .. } = song();
    let chorus = s.project().sections[1].id.to_string();
    run(&mut s, "section.reorder", json!({"id": chorus, "index": 0}));
    let p = s.project();
    assert_eq!(
        sections(p),
        vec![
            ("Chorus".to_string(), 0, 2 * BAR),
            ("Verse".to_string(), 2 * BAR, 2 * BAR)
        ]
    );
    // the Hook was in the Chorus's second bar
    assert_eq!(p.markers[0].tick, BAR);
    let l = p.loop_range.unwrap();
    assert_eq!((l.start, l.end), (BAR, 3 * BAR));
}

#[test]
fn a_duplicated_section_goes_in_right_after_the_original() {
    let Song { mut s, midi, audio } = song();
    let before = s.project().clone();
    let verse = s.project().sections[0].id.to_string();
    let made = id_of(&run(&mut s, "section.duplicate", json!({"id": verse})));
    let p = s.project();
    assert_eq!(
        sections(p),
        vec![
            ("Verse".to_string(), 0, 2 * BAR),
            ("Verse".to_string(), 2 * BAR, 2 * BAR),
            ("Chorus".to_string(), 4 * BAR, 2 * BAR)
        ]
    );
    assert_eq!(p.sections[1].id.to_string(), made);
    // the copy has the MIDI the Verse had; the Chorus's audio moved back two bars
    let m = clips_of(p, &midi);
    assert!(
        m.contains(&(0, 2 * BAR)) && m.contains(&(2 * BAR, 2 * BAR)),
        "{m:?}"
    );
    assert_eq!(clips_of(p, &audio), vec![(4 * BAR, 2 * BAR)]);
    assert_eq!(p.markers[0].tick, 5 * BAR);
    assert_eq!(tempo(p), vec![(0, 100.0), (4 * BAR, 140.0)]);
    // the copy's notes are notes of their own
    let mut ids: Vec<_> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .flat_map(|c| c.notes().iter().map(|n| n.id))
        .collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n);
    s.undo(1).unwrap();
    assert_eq!(s.project(), &before);
}

#[test]
fn a_section_can_be_copied_to_the_end() {
    let Song { mut s, .. } = song();
    let verse = s.project().sections[0].id.to_string();
    run(
        &mut s,
        "section.duplicate",
        json!({"id": verse, "at": "end"}),
    );
    assert_eq!(
        sections(s.project()),
        vec![
            ("Verse".to_string(), 0, 2 * BAR),
            ("Chorus".to_string(), 2 * BAR, 2 * BAR),
            ("Verse".to_string(), 4 * BAR, 2 * BAR)
        ]
    );
}

#[test]
fn the_loop_is_copied_to_just_after_itself() {
    let Song { mut s, midi, .. } = song();
    run(&mut s, "loop.duplicate", json!({}));
    let l = s.project().loop_range.unwrap();
    assert_eq!((l.start, l.end), (3 * BAR, 5 * BAR));
    s.undo(1).unwrap();
    assert_eq!(s.project().loop_range.unwrap().start, BAR);
    assert_eq!(clips_of(s.project(), &midi), vec![(0, 4 * BAR)]);
}

#[test]
fn sections_do_not_overlap_when_added_or_moved() {
    let mut s = Session::new("overlap");
    run(
        &mut s,
        "section.add",
        json!({"name": "A", "at": 0, "bars": 2}),
    );
    let b = id_of(&run(
        &mut s,
        "section.add",
        json!({"name": "B", "at": 4 * BAR, "bars": 2}),
    ));
    // starting inside A, or running into B, is refused
    let before = s.project().clone();
    assert!(
        try_run(
            &mut s,
            "section.add",
            json!({"name": "C", "at": BAR, "bars": 1})
        )
        .is_err()
    );
    assert!(
        try_run(
            &mut s,
            "section.add",
            json!({"name": "C", "at": 2 * BAR, "bars": 4})
        )
        .is_err()
    );
    assert_eq!(s.project(), &before);
    // fitting in the room between them is fine
    run(
        &mut s,
        "section.add",
        json!({"name": "C", "at": 2 * BAR, "bars": 2}),
    );
    assert_eq!(s.project().sections.len(), 3);
    // moving B onto C is refused and nothing changes; moving it clear of C is fine
    let before = s.project().clone();
    assert!(try_run(&mut s, "section.move", json!({"id": b, "by": -BAR})).is_err());
    assert_eq!(s.project(), &before);
    run(&mut s, "section.move", json!({"id": b, "by": BAR}));
    for pair in s.project().sections.windows(2) {
        assert!(pair[0].start + pair[0].length <= pair[1].start);
    }
}

#[test]
fn a_section_is_made_from_the_loop() {
    let Song { mut s, .. } = song();
    let chorus = s.project().sections[1].id.to_string();
    run(&mut s, "section.delete", json!({"id": chorus}));
    // the loop is bars 2–3 and the Verse holds bar 2: refused
    assert!(
        try_run(
            &mut s,
            "section.add",
            json!({"name": "Bridge", "range": "loop"})
        )
        .is_err()
    );
    run(
        &mut s,
        "loop.set",
        json!({"start": 2 * BAR, "end": 3 * BAR}),
    );
    run(
        &mut s,
        "section.add",
        json!({"name": "Bridge", "range": "loop"}),
    );
    assert_eq!(
        sections(s.project())[1],
        ("Bridge".to_string(), 2 * BAR, BAR)
    );
}

#[test]
fn a_section_is_split_coloured_and_selected_with_its_clips() {
    let Song { mut s, .. } = song();
    let verse = s.project().sections[0].id.to_string();
    let chorus = s.project().sections[1].id.to_string();
    run(
        &mut s,
        "section.split",
        json!({"id": chorus, "at": 3 * BAR}),
    );
    assert_eq!(
        sections(s.project()),
        vec![
            ("Verse".to_string(), 0, 2 * BAR),
            ("Chorus".to_string(), 2 * BAR, BAR),
            ("Chorus".to_string(), 3 * BAR, BAR)
        ]
    );
    // a cut at the section's own edge makes nothing
    assert!(try_run(&mut s, "section.split", json!({"id": verse, "at": 0})).is_err());
    run(
        &mut s,
        "section.color.set",
        json!({"id": verse, "color": "#ff8800"}),
    );
    assert_eq!(s.project().sections[0].color.as_deref(), Some("#ff8800"));
    run(
        &mut s,
        "section.color.set",
        json!({"id": verse, "color": "none"}),
    );
    assert_eq!(s.project().sections[0].color, None);
}

#[test]
fn a_section_selects_the_clips_that_touch_it() {
    use xirius_core::id::AnyId;
    let Song { mut s, .. } = song();
    let verse = s.project().sections[0].clone();
    run(
        &mut s,
        "select.in-section",
        json!({"id": verse.id.to_string()}),
    );
    let (from, to) = (verse.start, verse.start + verse.length);
    let mut want: Vec<AnyId> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| c.start < to && c.end() > from)
        .map(|c| AnyId::Clip(c.id))
        .collect();
    assert!(!want.is_empty(), "the song has clips under the verse");
    let mut got = s.state.selection.clone();
    want.sort_by_key(|i| format!("{i:?}"));
    got.sort_by_key(|i| format!("{i:?}"));
    assert_eq!(got, want);
    // a clip in the chorus that does not reach the verse stays out
    let all: usize = s.project().tracks.values().map(|t| t.clips.len()).sum();
    assert!(got.len() < all, "not every clip lies under the verse");
}

#[test]
fn a_section_copied_to_a_place_goes_there_in_one_step() {
    let Song { mut s, .. } = song();
    let before = sections(s.project());
    let verse = s.project().sections[0].id.to_string();
    // the copy of the first section becomes the last (an Alt drag onto the end)
    let n = before.len();
    run(
        &mut s,
        "section.duplicate",
        json!({"id": verse, "index": n}),
    );
    let after = sections(s.project());
    assert_eq!(after.len(), n + 1);
    assert_eq!(after.last().map(|x| x.0.as_str()), Some("Verse"));
    let names: Vec<_> = after.iter().map(|x| x.0.clone()).collect();
    let mut want: Vec<_> = before.iter().map(|x| x.0.clone()).collect();
    want.push("Verse".to_string());
    assert_eq!(names, want);
    s.undo(1).unwrap();
    assert_eq!(sections(s.project()), before, "one step undoes it");
}

#[test]
fn a_copied_section_is_pasted_with_what_it_held() {
    let Song { mut s, midi, audio } = song();
    let chorus = s.project().sections[1].id.to_string();
    run(&mut s, "section.copy", json!({"id": chorus}));
    // nothing changed by copying
    assert_eq!(s.project().sections.len(), 2);
    // pasted at the very start: time opens there and the Chorus comes in
    run(&mut s, "section.paste", json!({"at": 0}));
    let p = s.project();
    assert_eq!(
        sections(p),
        vec![
            ("Chorus".to_string(), 0, 2 * BAR),
            ("Verse".to_string(), 2 * BAR, 2 * BAR),
            ("Chorus".to_string(), 4 * BAR, 2 * BAR)
        ]
    );
    assert_eq!(clips_of(p, &audio), vec![(0, 2 * BAR), (4 * BAR, 2 * BAR)]);
    assert!(clips_of(p, &midi).contains(&(0, 2 * BAR)));
    assert_eq!(
        tempo(p),
        vec![(0, 140.0), (2 * BAR, 100.0), (4 * BAR, 140.0)]
    );
    s.undo(1).unwrap();
    assert_eq!(s.project().sections.len(), 2, "one step undoes it");
}

#[test]
fn a_cut_section_leaves_the_song_closed_up_and_can_be_pasted_back() {
    let Song { mut s, audio, .. } = song();
    let verse = s.project().sections[0].id.to_string();
    run(&mut s, "section.cut", json!({"id": verse}));
    assert_eq!(
        sections(s.project()),
        vec![("Chorus".to_string(), 0, 2 * BAR)]
    );
    assert_eq!(clips_of(s.project(), &audio), vec![(0, 2 * BAR)]);
    // by default it goes after the last section
    run(&mut s, "section.paste", json!({}));
    assert_eq!(
        sections(s.project()),
        vec![
            ("Chorus".to_string(), 0, 2 * BAR),
            ("Verse".to_string(), 2 * BAR, 2 * BAR)
        ]
    );
    // the clipboard still holds it: a second paste is a second copy
    run(&mut s, "section.paste", json!({"at": 4 * BAR}));
    assert_eq!(s.project().sections.len(), 3);
}

#[test]
fn pasting_with_nothing_copied_says_so() {
    let Song { mut s, .. } = song();
    let e = try_run(&mut s, "section.paste", json!({"at": 0})).unwrap_err();
    assert!(format!("{e:?}").contains("クリップボード"), "{e:?}");
}

#[test]
fn edit_paste_puts_a_copied_section_where_asked() {
    let Song { mut s, .. } = song();
    let verse = s.project().sections[0].id.to_string();
    run(&mut s, "section.copy", json!({"id": verse}));
    run(&mut s, "edit.paste", json!({"at": 4 * BAR}));
    assert_eq!(s.project().sections.len(), 3);
    assert_eq!(s.project().sections[2].start, 4 * BAR);
}
