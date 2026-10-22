//! Controller lanes drawn in a MIDI clip (EDT-004) reach the instrument as controller events on
//! the track's timeline, in the same frames a note would have. Nothing opens a device.

use serde_json::{Map, Value, json};
use xirius_core::model::Project;
use xirius_core::session::{RunOptions, Session};
use xirius_engine::{CTRL_AFTERTOUCH, CTRL_PITCH_BEND, NoteKind, TimedNote, Timeline};
use xirius_session::build::Builder;
use xirius_session::commands::{self, shared_registry};
use xirius_session::ext::ExtConfig;
use xirius_session::tempo::TempoMap;

const RATE: u32 = 48_000;

fn song() -> Session {
    let mut s = Session::with_registry(Project::new("lanes"), shared_registry());
    commands::attach(&mut s, ExtConfig::default());
    s
}

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    let args: Map<String, Value> = match args {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    commands::run(s, None, name, &args, &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"))
        .data
}

/// One instrument track with a two-bar MIDI clip at bar 1 holding one note.
fn voice(s: &mut Session) -> String {
    let t = run(s, "track.add.instrument", json!({ "name": "lead" }))["id"]
        .as_str()
        .unwrap()
        .to_string();
    let c = run(
        s,
        "clip.add.midi",
        json!({ "track": t, "at": 0, "bars": 2.0 }),
    )["id"]
        .as_str()
        .unwrap()
        .to_string();
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": 0, "pitch": 60, "length": 3840, "velocity": 100 }),
    );
    c
}

fn put(s: &mut Session, clip: &str, lane: &str, points: Value) {
    run(
        s,
        "cc.replace",
        json!({ "clip": clip, "lane": lane, "from": 0, "to": 7000, "points": points }),
    );
}

fn timeline_of(s: &Session) -> Timeline {
    let p = s.project();
    let t = p.tracks.values().find(|t| !t.clips.is_empty()).unwrap();
    let tmap = TempoMap::from_project(p);
    Builder::new(RATE, None).timeline(p, t, &tmap, &mut Vec::new())
}

fn numbers(t: &Timeline) -> Vec<(u64, u8, i32)> {
    t.controls()
        .iter()
        .map(|c: &TimedNote| match c.kind {
            NoteKind::Control { number, value } => (c.frame, number, value),
            _ => unreachable!(),
        })
        .collect()
}

#[test]
fn drawn_points_become_controller_events_on_their_frames() {
    let mut s = song();
    let clip = voice(&mut s);
    put(
        &mut s,
        &clip,
        "cc64",
        json!([{"tick": 0, "value": 127}, {"tick": 1920, "value": 0}]),
    );
    let got = numbers(&timeline_of(&s));
    // 120 bpm, 4/4, ppq 960: a beat is 24000 frames, so tick 1920 is frame 48000.
    assert!(got.contains(&(0, 64, 127)), "{got:?}");
    assert!(got.contains(&(48_000, 64, 0)), "{got:?}");
    // The notes are untouched.
    assert_eq!(timeline_of(&s).notes().len(), 2);
}

#[test]
fn a_point_made_a_line_sends_the_values_between_it_and_the_next_one() {
    let mut s = song();
    let clip = voice(&mut s);
    // A step holds 0 for a beat; a line from 0 to 96 over one beat (960 ticks, 24000 frames).
    put(
        &mut s,
        &clip,
        "cc1",
        json!([{"tick": 0, "value": 0, "shape": "linear"}, {"tick": 960, "value": 96}]),
    );
    let mut got: Vec<_> = numbers(&timeline_of(&s))
        .into_iter()
        .filter(|g| g.1 == 1)
        .collect();
    // The lane ends off rest, so the clip's end sends the way back; the rest is the stretch.
    assert_eq!(got.pop(), Some((192_000, 1, 0)), "{got:?}");
    assert_eq!(got.first(), Some(&(0, 1, 0)), "{got:?}");
    assert!(got.contains(&(24_000, 1, 96)), "{got:?}");
    // One value every 30 ticks, each one step up (96 / 32 = 3), on its own frame.
    assert!(got.contains(&(750, 1, 3)), "{got:?}");
    assert!(got.contains(&(12_000, 1, 48)), "{got:?}");
    assert!(
        got.windows(2).all(|w| w[0].0 < w[1].0 && w[0].2 < w[1].2),
        "{got:?}"
    );
    assert_eq!(got.len(), 33, "{got:?}");
    // The same two points as a step send only themselves.
    put(
        &mut s,
        &clip,
        "cc1",
        json!([{"tick": 0, "value": 0}, {"tick": 960, "value": 96}]),
    );
    let step: Vec<_> = numbers(&timeline_of(&s))
        .into_iter()
        .filter(|g| g.1 == 1)
        .collect();
    assert_eq!(step[..2], [(0, 1, 0), (24_000, 1, 96)], "{step:?}");
}

#[test]
fn bend_and_aftertouch_have_their_own_numbers_and_values_stay_in_range() {
    let mut s = song();
    let clip = voice(&mut s);
    put(
        &mut s,
        &clip,
        "bend",
        json!([{"tick": 480, "value": -8192}]),
    );
    put(&mut s, &clip, "at", json!([{"tick": 480, "value": 90}]));
    let got = numbers(&timeline_of(&s));
    assert!(got.iter().any(|g| g.1 == CTRL_PITCH_BEND && g.2 == -8192));
    assert!(got.iter().any(|g| g.1 == CTRL_AFTERTOUCH && g.2 == 90));
}

#[test]
fn a_lane_left_off_rest_goes_back_to_it_where_the_clip_ends() {
    let mut s = song();
    let clip = voice(&mut s);
    // The pedal goes down and is never lifted: the clip is two bars (7680 ticks = 192000 frames).
    put(&mut s, &clip, "cc64", json!([{"tick": 100, "value": 127}]));
    let got = numbers(&timeline_of(&s));
    let last = got.last().unwrap();
    assert_eq!((last.1, last.2), (64, 0), "{got:?}");
    assert_eq!(last.0, 192_000, "{got:?}");
}

#[test]
fn a_controller_edit_changes_what_the_host_compares_so_the_timeline_is_rebuilt() {
    let mut s = song();
    let clip = voice(&mut s);
    let before = s
        .project()
        .tracks
        .values()
        .find(|t| !t.clips.is_empty())
        .unwrap()
        .clone();
    put(&mut s, &clip, "cc1", json!([{"tick": 0, "value": 64}]));
    let after = s
        .project()
        .tracks
        .values()
        .find(|t| !t.clips.is_empty())
        .unwrap();
    assert!(before.clips != after.clips);
    // Undo puts the lane back out of the timeline.
    s.undo(1).unwrap();
    assert!(timeline_of(&s).controls().is_empty());
}

#[test]
fn a_clip_without_lanes_has_no_controller_events() {
    let mut s = song();
    voice(&mut s);
    assert!(timeline_of(&s).controls().is_empty());
}
