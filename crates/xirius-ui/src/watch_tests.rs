//! Watching the input of the armed tracks from the screen (REC-002, REC-011, REC-012): arming
//! a track makes its input level move before anything records, and the input monitor button is
//! what makes the input heard. The output is silent and keeps time; no device is opened and
//! nothing reaches the speakers.

use std::time::{Duration, Instant};

use xirius_core::id::TrackId;
use xirius_session::host::Strip;
use xirius_session::output::OutputMode;

use crate::studio::{Geometry, Hit, Studio};

fn studio() -> Studio {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    s
}

fn geometry(s: &Studio) -> Geometry {
    s.geometry(1440.0, 860.0)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .expect("a track")
        .id
}

/// Scrolls the rows so that the track's is the first on screen.
fn show(s: &mut Studio, t: TrackId) {
    let y = s
        .tree()
        .row_of(crate::tree::RowKind::Track(t))
        .map(|r| r.y)
        .expect("a row");
    s.scroll.1 = y;
}

/// Presses a part of the screen the way a click does: aims at it, checks that the pointer would
/// hit it, then presses.
fn click(s: &mut Studio, hit: Hit) {
    let g = geometry(s);
    let (x, y) = s
        .center_of(&g, hit)
        .unwrap_or_else(|| panic!("not visible: {hit:?}"));
    assert_eq!(s.hit(&g, x, y), hit);
    assert!(s.press(&g, hit));
    assert_eq!(s.error, None);
}

/// Ticks once a frame until `done` or three seconds pass.
fn until(s: &mut Studio, done: impl Fn(&Studio) -> bool) -> bool {
    let since = Instant::now();
    while since.elapsed() < Duration::from_secs(3) {
        s.tick();
        if done(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    false
}

fn master(s: &Studio) -> f32 {
    s.mixer.meters.get(Strip::Master).level[0]
}

/// Ticks until the input monitor has put something on the output, or says it has not.
fn heard(s: &mut Studio) -> f32 {
    let since = Instant::now();
    let mut peak = 0.0f32;
    while since.elapsed() < Duration::from_millis(600) {
        s.tick();
        let [l, r] = s.transport.take_monitor_peak();
        peak = peak.max(l).max(r);
        std::thread::sleep(Duration::from_millis(16));
    }
    peak
}

#[test]
fn arming_an_audio_track_makes_its_input_level_move_and_disarming_stops_it() {
    let mut s = studio();
    let vocal = track(&s, "Vocal");
    show(&mut s, vocal);
    // The roll is shut, so nothing else starts the player (UI-285).
    s.drawer_open = false;
    s.tick();
    assert!(!s.transport.is_live(), "nothing is armed: no input is open");
    click(&mut s, Hit::Arm(vocal));
    assert!(
        until(&mut s, |s| {
            let p = s.project();
            s.inputs.of(&p.tracks[&vocal], 2).level[0] > 0.0
        }),
        "the level moved"
    );
    let p = s.project();
    let shown = s.inputs.of(&p.tracks[&vocal], 2);
    assert!(shown.level[0] > 0.5 && shown.level[1] > 0.5, "{shown:?}");
    // Watching is silent: the level moves, the output does not.
    assert_eq!(master(&s), 0.0);

    click(&mut s, Hit::Arm(vocal));
    assert!(
        until(&mut s, |s| {
            let p = s.project();
            s.inputs.of(&p.tracks[&vocal], 2).level[0] == 0.0
        }),
        "the level falls away"
    );
}

#[test]
fn the_monitor_button_makes_the_input_heard_and_is_one_undo() {
    let mut s = studio();
    let vocal = track(&s, "Vocal");
    show(&mut s, vocal);
    click(&mut s, Hit::Arm(vocal));
    assert!(until(&mut s, |s| {
        let p = s.project();
        s.inputs.of(&p.tracks[&vocal], 2).level[0] > 0.0
    }));
    assert_eq!(master(&s), 0.0, "armed alone is silent");
    assert_eq!(heard(&mut s), 0.0);

    let steps = s.song.steps();
    click(&mut s, Hit::Monitor(vocal));
    assert!(s.project().tracks[&vocal].monitor);
    assert_eq!(s.song.steps(), steps + 1);
    let on = heard(&mut s);
    assert!(on > 0.2, "the input is on the output now: {on}");

    let g = geometry(&s);
    assert!(s.undo(&g));
    assert!(!s.project().tracks[&vocal].monitor);
    heard(&mut s);
    assert_eq!(heard(&mut s), 0.0, "off again");
}

#[test]
fn only_an_audio_track_has_a_monitor_button() {
    let s = studio();
    let g = geometry(&s);
    let drums = track(&s, "Drums");
    if let Some((x, y)) = s.center_of(&g, Hit::Monitor(drums)) {
        assert_ne!(s.hit(&g, x, y), Hit::Monitor(drums));
    }
}

#[test]
fn recording_with_the_input_outside_the_frame_says_so_beside_the_meter() {
    let mut s = studio();
    let vocal = track(&s, "Vocal");
    show(&mut s, vocal);
    click(&mut s, Hit::Arm(vocal));
    assert!(until(&mut s, |s| {
        let p = s.project();
        s.inputs.of(&p.tracks[&vocal], 2).level[0] > 0.0
    }));
    // Not recording: nothing is said, whatever the level.
    assert!(!s.input_outside_frame(vocal));
    // The test tone sits inside the frame: nothing is said while recording either.
    s.transport.recording = true;
    for _ in 0..30 {
        s.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(s.rec_peaks[&vocal] > 0.0);
    assert!(!s.input_outside_frame(vocal), "{:?}", s.rec_peaks);
    s.transport.recording = false;
    s.tick();
    assert!(
        s.rec_peaks.is_empty(),
        "the peaks are forgotten once it is over"
    );

    // Turned up by 24 dB the same tone is far over the frame.
    assert!(s.command(|song| {
        song.run(
            "track.input.gain",
            serde_json::json!({"ids": [vocal.to_string()], "db": 24.0}),
        )
        .map(|_| ())
    }));
    s.tick();
    s.transport.recording = true;
    assert!(until(&mut s, |s| s.input_outside_frame(vocal)));
    s.transport.recording = false;
    s.tick();
    assert!(!s.input_outside_frame(vocal));
}
