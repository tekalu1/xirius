//! EDT-135: ripple editing. With `--ripple`, deleting, cutting or taking a range out closes up
//! what follows on the same track, and resizing a clip's end pushes or pulls what follows. The
//! other tracks, the markers and the tempo stay; one edit is one history step.
use serde_json::{Value, json};
use xirius_core::id::TrackId;
use xirius_core::session::{RunOptions, Session};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args, &RunOptions::human()).unwrap().data
}

fn id_of(v: &Value) -> String {
    v["id"].as_str().unwrap().to_string()
}

/// Two instrument tracks. The first has clips at 0, 960 and 1920 (each 480 long); the second has one
/// clip at 1920. A marker sits at 1920 and the tempo changes there.
struct Song {
    s: Session,
    first: String,
    second: String,
    clips: Vec<String>,
}

fn song() -> Song {
    let mut s = Session::new("ripple");
    let first = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let second = id_of(&run(&mut s, "track.add.instrument", json!({})));
    let mut clips = Vec::new();
    for at in [0, 960, 1920] {
        let c = run(
            &mut s,
            "clip.add.midi",
            json!({"track": first, "at": at, "length": 480}),
        );
        clips.push(id_of(&c));
    }
    run(
        &mut s,
        "clip.add.midi",
        json!({"track": second, "at": 1920, "length": 480}),
    );
    run(&mut s, "marker.add", json!({"at": 1920, "name": "B"}));
    run(&mut s, "tempo.set", json!({"bpm": 90, "at": 1920}));
    Song {
        s,
        first,
        second,
        clips,
    }
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

/// The marker and the tempo point stay at 1920 whatever the clips do.
fn timeline_stays(s: &Session) {
    let p = s.project();
    assert_eq!(p.markers.iter().map(|m| m.tick).collect::<Vec<_>>(), [1920]);
    assert!(p.tempo.iter().any(|t| t.tick == 1920 && t.bpm == 90.0));
}

#[test]
fn deleting_a_clip_with_ripple_closes_up_its_track_only() {
    let mut g = song();
    let steps = g.s.steps().len();
    run(
        &mut g.s,
        "clip.delete",
        json!({"ids": [g.clips[1]], "ripple": true}),
    );
    assert_eq!(g.s.steps().len(), steps + 1);
    assert_eq!(spans(&g.s, &g.first), [(0, 480), (1440, 480)]);
    assert_eq!(spans(&g.s, &g.second), [(1920, 480)]);
    timeline_stays(&g.s);
    g.s.undo(1).unwrap();
    assert_eq!(spans(&g.s, &g.first), [(0, 480), (960, 480), (1920, 480)]);
}

#[test]
fn without_ripple_a_delete_leaves_the_gap() {
    let mut g = song();
    run(&mut g.s, "clip.delete", json!({"ids": [g.clips[1]]}));
    assert_eq!(spans(&g.s, &g.first), [(0, 480), (1920, 480)]);
}

#[test]
fn deleting_two_clips_closes_up_both_gaps() {
    let mut g = song();
    run(
        &mut g.s,
        "clip.delete",
        json!({"ids": [g.clips[0], g.clips[1]], "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(960, 480)]);
}

#[test]
fn cutting_and_deleting_clips_with_ripple_close_up() {
    let mut g = song();
    run(
        &mut g.s,
        "edit.cut",
        json!({"ids": [g.clips[0]], "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(480, 480), (1440, 480)]);
    let clip = run(&mut g.s, "edit.clipboard", json!({}));
    assert_eq!(clip["kind"], "clips");
    run(
        &mut g.s,
        "edit.delete",
        json!({"ids": [g.clips[1]], "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(960, 480)]);
    assert_eq!(spans(&g.s, &g.second), [(1920, 480)]);
    timeline_stays(&g.s);
}

#[test]
fn a_range_taken_out_with_ripple_closes_up_the_tracks_it_was_on() {
    let mut g = song();
    let steps = g.s.steps().len();
    // 240..1200 cuts the first clip at 240 and the second at 1200.
    run(
        &mut g.s,
        "range.delete",
        json!({"ids": [g.clips[0], g.clips[1]], "start": 240, "end": 1200, "ripple": true}),
    );
    assert_eq!(g.s.steps().len(), steps + 1);
    assert_eq!(spans(&g.s, &g.first), [(0, 240), (240, 240), (960, 480)]);
    assert_eq!(spans(&g.s, &g.second), [(1920, 480)]);
    timeline_stays(&g.s);

    let mut g = song();
    run(
        &mut g.s,
        "range.cut",
        json!({"ids": [g.clips[1]], "start": 960, "end": 1440, "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(0, 480), (1440, 480)]);
    assert_eq!(
        run(&mut g.s, "edit.clipboard", json!({}))["count"],
        json!(1)
    );
}

#[test]
fn shortening_a_clip_with_ripple_pulls_what_follows_and_lengthening_pushes_it() {
    let mut g = song();
    let steps = g.s.steps().len();
    run(
        &mut g.s,
        "clip.resize",
        json!({"ids": [g.clips[0]], "by": -240, "ripple": true}),
    );
    assert_eq!(g.s.steps().len(), steps + 1);
    assert_eq!(spans(&g.s, &g.first), [(0, 240), (720, 480), (1680, 480)]);
    run(
        &mut g.s,
        "clip.resize",
        json!({"ids": [g.clips[1]], "length": 960, "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(0, 240), (720, 960), (2160, 480)]);
    assert_eq!(spans(&g.s, &g.second), [(1920, 480)]);
    timeline_stays(&g.s);
}

#[test]
fn resizing_two_clips_of_a_track_adds_up_for_what_follows_both() {
    let mut g = song();
    run(
        &mut g.s,
        "clip.resize",
        json!({"ids": [g.clips[0], g.clips[1]], "by": 120, "ripple": true}),
    );
    assert_eq!(spans(&g.s, &g.first), [(0, 600), (1080, 600), (2160, 480)]);
}

#[test]
fn the_automation_under_a_rippled_clip_goes_with_it_unless_told_to_stay() {
    // The point at 2000 lies under the clip at 1920, which closes up to 1440.
    for (automation, kept, gone) in [("follow", 1520, 2000), ("stay", 2000, 1520)] {
        let mut g = song();
        let t = g.first.clone();
        run(
            &mut g.s,
            "auto.point.add",
            json!({"track": t, "target": "pan", "at": 2000, "value": 0.5}),
        );
        run(
            &mut g.s,
            "clip.delete",
            json!({"ids": [g.clips[1]], "ripple": true, "automation": automation}),
        );
        let id: TrackId = t.parse().unwrap();
        let ticks: Vec<i64> = g.s.project().tracks[&id].automation[0]
            .points
            .iter()
            .map(|x| x.tick)
            .collect();
        assert!(
            ticks.contains(&kept) && !ticks.contains(&gone),
            "{automation}: {ticks:?}"
        );
    }
}
