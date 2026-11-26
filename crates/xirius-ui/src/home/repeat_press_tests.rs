//! UI-350: a row pressed again. A person who is used to a double press (or who presses again a
//! second later because nothing seemed to happen) must not cancel the open, start it over, or
//! change the song that comes up.

use super::tests::{centre, row_spot};
use super::*;
use crate::song::Song;
use std::time::Duration;

/// Which kind of row is pressed.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Saved,
    Draft,
    Sample,
}

/// What the window did between the two presses. Nothing on the start screen or in the open
/// goes by the clock, so the gap is counted in frames (at 60 a second) and in how far the open
/// has got, not waited out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gap {
    /// At once: no frame between.
    Now,
    /// A fifth of a second of frames, the open still on its way as a rule.
    Short,
    /// A second on: the open has finished and the song is up.
    Second,
}

/// The frames of a fifth of a second, of a third of one (the rest after the song came up), and
/// of a second.
const SHORT: usize = 12;
const REST: usize = 18;
const SECOND: usize = 60;

impl Gap {
    fn run(self, s: &mut Studio) {
        match self {
            Gap::Now => {}
            Gap::Short => frames(s, SHORT),
            Gap::Second => {
                s.wait_for_open_for_test();
                frames(s, SECOND);
            }
        }
    }
}

/// Runs `count` frames of the window as it runs while nothing is pressed, each with a moment
/// for the open's own thread.
fn frames(s: &mut Studio, count: usize) {
    for _ in 0..count {
        s.idle();
        s.tick();
        s.home_tick();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A song with a track and a few steps, saved: what the start screen lists.
fn seeded(path: &std::path::Path) {
    let mut seed = Song::sample();
    seed.save_as(path).unwrap();
}

fn listing() -> (crate::shot::TestFolder, Studio, Geometry) {
    let now = xirius_core::store::now_ms();
    let tmp = crate::shot::TestFolder::new("xirius-ui-home-");
    let keep = crate::keep::Keep::at(tmp.path().join("keep"));
    let saved = tmp.path().join("saved");
    let draft = tmp.path().join("keep").join("drafts").join("d1");
    for (path, name, is_draft, at) in [
        (&saved, "saved", false, now - 60_000),
        (&draft, "d1", true, now),
    ] {
        seeded(path);
        keep.remember(path, name, is_draft, at);
    }
    let mut s = Studio::new(Song::empty());
    super::tests::with_device(&mut s, true);
    s.keep = keep;
    s.keep.sync(&s.song.project());
    s.keep.settle_recent();
    s.keep.poll_recent();
    s.home = Some(Home::default());
    s.home_tick();
    let g = s.geometry(1280.0, 720.0);
    (tmp, s, g)
}

fn row_of(s: &Studio, g: &Geometry, kind: Kind) -> (f32, f32) {
    let index = match kind {
        Kind::Draft => 0,
        Kind::Saved => 1,
        Kind::Sample => 2,
    };
    centre(
        s.home_layout(g)
            .unwrap()
            .rect(row_spot(s, g, index))
            .unwrap(),
    )
}

fn press(s: &mut Studio, g: &Geometry, at: (f32, f32), clicks: u32) -> bool {
    let before = s.opening.is_some();
    s.window_press(g, at.0, at.1, clicks, Mods::NONE);
    before
}

/// Both presses, `gap` apart, on the same place of one row; returns whether the open was still
/// running at the second press, and the song's state after it came up and rested.
fn twice(
    kind: Kind,
    gap: Gap,
    second_clicks: u32,
) -> (crate::shot::TestFolder, bool, String, Studio) {
    let (tmp, mut s, g) = listing();
    let at = row_of(&s, &g, kind);
    press(&mut s, &g, at, 1);
    assert!(s.opening.is_some(), "{kind:?}: the first press opens");
    gap.run(&mut s);
    let running = press(&mut s, &g, at, second_clicks);
    s.wait_for_open_for_test();
    frames(&mut s, REST);
    let state = song_of(&s);
    (tmp, running, state, s)
}

/// What a press must not change: the song itself (its tracks, tempo and notes), what undo
/// holds, and whether it plays. What is selected is the view's, not the song's.
fn song_of(s: &Studio) -> String {
    format!(
        "{} {} {}",
        serde_json::to_string(&*s.project()).unwrap(),
        s.song.steps(),
        s.transport.playing
    )
}

/// The same row opened by one press, to compare with.
fn once(kind: Kind) -> (crate::shot::TestFolder, String, Studio) {
    let (tmp, mut s, g) = listing();
    let at = row_of(&s, &g, kind);
    press(&mut s, &g, at, 1);
    s.wait_for_open_for_test();
    frames(&mut s, REST);
    let state = song_of(&s);
    (tmp, state, s)
}

fn opened(s: &Studio, tmp: &crate::shot::TestFolder, kind: Kind) {
    assert!(!s.home_shown(), "{kind:?}: the song is up");
    assert!(s.opening.is_none());
    match kind {
        Kind::Saved => assert_eq!(s.song.dir(), Some(tmp.path().join("saved"))),
        Kind::Draft => assert!(s.song.is_draft()),
        Kind::Sample => assert!(s.song.is_sample()),
    }
}

#[test]
fn a_row_pressed_again_opens_its_song_once_and_changes_nothing() {
    let mut bad = Vec::new();
    for kind in [Kind::Saved, Kind::Draft, Kind::Sample] {
        let (_t0, want, s0) = once(kind);
        let steps = s0.song.steps();
        for gap in [Gap::Now, Gap::Short, Gap::Second] {
            // A double-click's second press comes at once or a moment after, not a second on.
            for clicks in [1, 2] {
                if clicks == 2 && gap == Gap::Second {
                    continue;
                }
                let (tmp, running, state, s) = twice(kind, gap, clicks);
                opened(&s, &tmp, kind);
                let tag =
                    format!("{kind:?} {gap:?} clicks={clicks} (open still running: {running})");
                if state != want {
                    let at = state
                        .chars()
                        .zip(want.chars())
                        .position(|(a, b)| a != b)
                        .unwrap_or(0);
                    bad.push(format!("{tag}: the song differs at {at}"));
                }
                if s.song.steps() != steps {
                    bad.push(format!("{tag}: steps {} -> {}", steps, s.song.steps()));
                }
                if s.error.is_some() {
                    bad.push(format!("{tag}: error {:?}", s.error));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{}",
        bad.join(
            "
"
        )
    );
}

/// The second press, one second on, lands on the song that came up: whatever is under the
/// row's place must not change the song (no clip, no tempo point, no step).
#[test]
fn a_press_on_the_song_that_came_up_at_the_rows_place_adds_no_step() {
    for kind in [Kind::Saved, Kind::Draft, Kind::Sample] {
        let (_tmp, mut s, g) = listing();
        let at = row_of(&s, &g, kind);
        press(&mut s, &g, at, 1);
        Gap::Second.run(&mut s);
        let g = s.geometry(1280.0, 720.0);
        let (steps, tempo) = (s.song.steps(), s.project().tempo.len());
        press(&mut s, &g, at, 1);
        s.window_released(&g, at.0, at.1, Mods::NONE);
        assert_eq!(s.song.steps(), steps, "{kind:?}: a step was added");
        assert_eq!(s.project().tempo.len(), tempo, "{kind:?}: a tempo point");
    }
}

/// The song that came up under the top row, rested, with the row's place and a fresh geometry.
fn up_under_the_top_row() -> (crate::shot::TestFolder, Studio, Geometry, (f32, f32)) {
    let (tmp, mut s, g) = listing();
    let at = row_of(&s, &g, Kind::Draft);
    press(&mut s, &g, at, 1);
    s.wait_for_open_for_test();
    frames(&mut s, REST);
    let g = s.geometry(1280.0, 720.0);
    (tmp, s, g, at)
}

/// Pressing on and on at the row's place counts 2, 3, 4, 5 …: none of them is the song's.
#[test]
fn a_run_of_presses_that_began_on_a_row_never_reaches_the_song() {
    let (_t0, want, _s0) = once(Kind::Draft);
    let (_tmp, mut s, g, at) = up_under_the_top_row();
    for clicks in 2..=5 {
        press(&mut s, &g, at, clicks);
        s.window_released(&g, at.0, at.1, Mods::NONE);
        assert_eq!(song_of(&s), want, "clicks={clicks}");
    }
}

/// A press of its own (count 1) ends the run: the next double press there is an ordinary one.
#[test]
fn a_press_of_its_own_after_the_run_makes_the_place_ordinary_again() {
    let (_tmp, mut s, g, at) = up_under_the_top_row();
    for clicks in 2..=5 {
        press(&mut s, &g, at, clicks);
        s.window_released(&g, at.0, at.1, Mods::NONE);
    }
    press(&mut s, &g, at, 1);
    s.window_released(&g, at.0, at.1, Mods::NONE);
    let tempo = s.project().tempo.len();
    press(&mut s, &g, at, 2);
    s.window_released(&g, at.0, at.1, Mods::NONE);
    assert_eq!(s.project().tempo.len(), tempo + 1, "the tempo band took it");
}

/// What is remembered is a place: a press further than 8 px away is the song's.
#[test]
fn a_double_press_far_from_the_row_is_the_songs_even_with_a_place_remembered() {
    let (_tmp, mut s, g, at) = up_under_the_top_row();
    let tempo = s.project().tempo.len();
    let far = (at.0 + 40.0, at.1);
    press(&mut s, &g, far, 2);
    s.window_released(&g, far.0, far.1, Mods::NONE);
    assert_eq!(s.project().tempo.len(), tempo + 1, "the tempo band took it");
}
