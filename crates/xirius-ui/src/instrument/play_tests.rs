//! A Sampler's keys and a Drum Kit's pads play their note through the real engine, silent
//! (PLG-066): the note ends when let go, when the pointer leaves it and when the window
//! closes, and nothing is recorded.

use std::time::{Duration, Instant};

use gpui::accesskit;

use xirius_engine::NoteKind;
use xirius_session::output::OutputMode;

use super::window::Part;
use super::*;
use crate::song::Song;
use crate::studio::Geometry;

/// A new song after 「MIDI を演奏」 with the armed track's instrument window open.
fn played(device: &str) -> (Studio, Geometry, TrackId, PluginId) {
    let mut s = Studio::new(Song::empty());
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.start_midi());
    let t = s.pc_target().expect("the armed instrument track");
    if device != DEFAULT {
        assert!(s.set_instrument(t, &Pick::built_in(device)));
    }
    let id = current(&s.project().tracks[&t]).unwrap().id;
    s.show_instrument(t, id);
    assert!(s.instruments.is_open(id));
    let g = s.geometry(1600.0, 1000.0);
    (s, g, t, id)
}

/// How many voices of `id` sound, once the engine has played.
fn sounding(s: &Studio, id: PluginId) -> Option<u32> {
    let probe = s.transport.host()?.plugins().stock_probe(id)?;
    Some(probe.sounding())
}

/// Waits until the voices of `id` meet `predicate`, with a guard for an engine that stops.
fn wait_sounding(s: &mut Studio, id: PluginId, predicate: impl Fn(u32) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut last = None;
    while Instant::now() < deadline {
        s.transport.poll();
        last = sounding(s, id);
        if last.is_some_and(&predicate) {
            return;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    panic!("the device's voices did not reach the expected count; last: {last:?}");
}

fn last_sent(s: &Studio) -> Option<(TrackId, NoteKind)> {
    s.live_midi.sent.last().copied()
}

fn clips_in_song(s: &Studio) -> usize {
    let p = s.project();
    p.tracks.values().map(|t| t.clips.len()).sum()
}

#[test]
fn plg_066_a_key_sounds_while_held_and_stops_when_let_go_unrecorded() {
    let (mut s, g, t, id) = played(DEFAULT);
    let before = clips_in_song(&s);
    let (x, y) = s.instrument_center(&g, id, Part::Key(60)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(matches!(
        last_sent(&s),
        Some((tt, NoteKind::On { pitch: 60, .. })) if tt == t
    ));
    assert_eq!(s.instruments.note.map(|n| n.pitch), Some(60));
    wait_sounding(&mut s, id, |n| n > 0);

    assert!(s.instrument_up());
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 60 })));
    assert_eq!(s.instruments.note, None);
    wait_sounding(&mut s, id, |n| n == 0);
    assert_eq!(clips_in_song(&s), before);
}

#[test]
fn plg_066_a_black_key_wins_over_the_white_one_under_it() {
    let (mut s, g, t, id) = played(DEFAULT);
    let (x, y) = s.instrument_center(&g, id, Part::Key(61)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(matches!(
        last_sent(&s),
        Some((tt, NoteKind::On { pitch: 61, .. })) if tt == t
    ));
    assert!(s.instrument_up());
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 61 })));
}

#[test]
fn plg_066_leaving_the_key_ends_the_note_and_coming_back_does_not_play_it() {
    let (mut s, g, t, id) = played(DEFAULT);
    let (x, y) = s.instrument_center(&g, id, Part::Key(60)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    let sent = s.live_midi.sent.len();

    // Onto the next key: the note held ends, the next does not start.
    let (nx, ny) = s.instrument_center(&g, id, Part::Key(62)).unwrap();
    assert!(s.instrument_moved(&g, nx, ny, false));
    assert_eq!(s.live_midi.sent.len(), sent + 1);
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 60 })));
    // Back onto it, and out of the window: nothing more.
    assert!(s.instrument_moved(&g, x, y, false));
    assert!(s.instrument_moved(&g, 2.0, 2.0, false));
    assert!(s.instrument_up());
    assert_eq!(s.live_midi.sent.len(), sent + 1);
    wait_sounding(&mut s, id, |n| n == 0);
}

#[test]
fn plg_066_closing_the_window_ends_the_note() {
    let (mut s, g, t, id) = played(DEFAULT);
    let (x, y) = s.instrument_center(&g, id, Part::Key(64)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    let (cx, cy) = s.instrument_center(&g, id, Part::Close).unwrap();
    // The close is pressed while the key is still held (a second pointer, the keyboard).
    assert!(s.instrument_down(&g, cx, cy));
    assert!(!s.instruments.is_open(id));
    assert!(s.live_midi.sent.contains(&(t, NoteKind::Off { pitch: 64 })));
    assert_eq!(s.instruments.note, None);
    wait_sounding(&mut s, id, |n| n == 0);

    // The mixer's button closing it does the same.
    s.show_instrument(t, id);
    let (x, y) = s.instrument_center(&g, id, Part::Key(65)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    assert!(s.open_plugin(t, id));
    assert!(!s.instruments.is_open(id));
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 65 })));
    wait_sounding(&mut s, id, |n| n == 0);
}

#[test]
fn plg_066_a_drum_kit_pad_plays_its_note() {
    let (mut s, g, t, id) = played("Drum Kit");
    let before = clips_in_song(&s);
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(matches!(
        last_sent(&s),
        Some((tt, NoteKind::On { pitch: 36, .. })) if tt == t
    ));
    wait_sounding(&mut s, id, |n| n > 0);
    assert!(s.instrument_up());
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 36 })));
    wait_sounding(&mut s, id, |n| n == 0);
    assert_eq!(clips_in_song(&s), before);

    // The first pad is at the bottom left, the fourth at the bottom right, the fifth above the first.
    let at = |s: &Studio, p: u8| s.instrument_center(&g, id, Part::Pad(p)).unwrap();
    let (a1, a4, a5) = (at(&s, 36), at(&s, 39), at(&s, 40));
    assert_eq!(a1.1, a4.1);
    assert!(a4.0 > a1.0);
    assert_eq!(a5.0, a1.0);
    assert!(a5.1 < a1.1);
}

#[test]
fn plg_066_the_keys_and_the_pads_read_their_notes() {
    let (s, g, _, id) = played(DEFAULT);
    let marks = crate::a11y::marks(&s, &g);
    let key = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-key-60"))
        .expect("the key C3")
        .node
        .clone();
    assert_eq!(key.role(), accesskit::Role::Button);
    assert_eq!(key.label(), Some("C3"));
    assert_eq!(
        marks
            .iter()
            .filter(|m| m.key.starts_with(&format!("device-{id}-key-")))
            .count(),
        49
    );

    let (s, g, _, id) = played("Drum Kit");
    let marks = crate::a11y::marks(&s, &g);
    let pad = marks
        .iter()
        .find(|m| m.key == format!("device-{id}-pad-36"))
        .expect("the pad A1")
        .node
        .clone();
    assert_eq!(pad.role(), accesskit::Role::Button);
    assert_eq!(pad.label(), Some("A1 C1"));
}
