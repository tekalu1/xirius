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
    give_sample(&mut s, id);
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    assert!(s.instruments.is_open(id));
    let g = s.geometry(1600.0, 1000.0);
    (s, g, t, id)
}

thread_local! {
    /// The test's sample files, kept until the test's thread ends.
    static FILES: std::cell::RefCell<Vec<tempfile::TempDir>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// A tone for the device `id` to play: the Sampler and the Drum Kit sound only with a file
/// (PLG-067, PLG-068).
pub(crate) fn give_sample(s: &mut Studio, id: PluginId) {
    let dir = tempfile::Builder::new()
        .prefix("xirius-ui-play-")
        .tempdir()
        .unwrap();
    let x: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 262.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    let file = dir.path().join("tone_C3.wav");
    xirius_session::export::write_wav(&file, &x, &x, 48_000, 16).unwrap();
    assert!(s.run_command(
        "plugin.samples.add",
        serde_json::json!({"id": id, "file": file.display().to_string()})
    ));
    FILES.with(|f| f.borrow_mut().push(dir));
}

/// The sample song has MIDI tracks without a device. Tests that measure sound assign both.
pub(crate) fn give_track_sample(s: &mut Studio, track: TrackId) {
    assert!(s.set_instrument(track, &Pick::built_in(DEFAULT)));
    let id = current(&s.project().tracks[&track]).unwrap().id;
    give_sample(s, id);
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
fn sample_io_windowed_empty_to_loaded_rebuilds_the_playing_route() {
    for device in ["Sampler", "Drum Kit"] {
        sample_io_transition(device, false, false);
    }
}

#[test]
fn sample_io_windowed_failed_load_keeps_the_empty_route_silent() {
    for device in ["Sampler", "Drum Kit"] {
        sample_io_transition(device, true, false);
    }
}

#[test]
fn sample_io_windowed_removing_the_last_sample_returns_to_silence() {
    for device in ["Sampler", "Drum Kit"] {
        sample_io_transition(device, false, true);
    }
}

fn sample_io_empty_is_silent(s: &mut Studio, t: TrackId, id: PluginId, pitch: u8) {
    assert!(current(&s.project().tracks[&t]).unwrap().samples.is_empty());
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.transport.step_swap().0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let probe = s
        .transport
        .host()
        .unwrap()
        .plugins()
        .stock_probe(id)
        .unwrap();
    probe.watch(true);
    // Drain the reports from the graph that just retired before testing a new note.
    // Three fresh quiet reports establish that the silent route is running.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut quiet = 0;
    while quiet < 3 {
        s.transport.poll();
        if let Some(level) = s
            .transport
            .take_levels()
            .get(&xirius_session::host::Strip::Track(t))
        {
            quiet = if level.peak == [0.0, 0.0] {
                quiet + 1
            } else {
                0
            };
        }
        assert!(Instant::now() < deadline, "the empty route must settle");
        std::thread::sleep(Duration::from_millis(16));
    }
    probe.take_peaks();
    s.pc_keys.octave = (pitch / 12) as i8 - 1;
    assert!(s.pc_key_down("a", false));
    assert!(
        s.pc_keys
            .down
            .iter()
            .any(|(key, track, _, sent)| key == "a" && *track == t && *sent == pitch),
        "the PC key must send the intended track and pitch: {:?}",
        s.error
    );
    assert!(
        s.pc_keys.pc_waiting.is_empty(),
        "the route must already be ready"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reports = 0;
    while reports < 3 {
        s.transport.poll();
        if let Some(level) = s
            .transport
            .take_levels()
            .get(&xirius_session::host::Strip::Track(t))
        {
            assert_eq!(
                level.peak,
                [0.0, 0.0],
                "an empty instrument must stay silent"
            );
            reports += 1;
        }
        assert!(Instant::now() < deadline, "fresh audio reports must arrive");
        std::thread::sleep(Duration::from_millis(16));
    }
    assert_eq!(probe.take_peaks().1, [0.0, 0.0]);
    assert_eq!(probe.sounding(), 0);
    assert!(s.pc_key_up("a"));
}

fn sample_io_transition(device: &str, fail: bool, remove: bool) {
    let mut s = Studio::new(Song::empty());
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.start_midi());
    let t = s.pc_target().unwrap();
    assert!(s.set_instrument(t, &Pick::built_in(device)));
    let id = current(&s.project().tracks[&t]).unwrap().id;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("tone_C3.wav");
    let wave: Vec<f32> = (0..48_000)
        .map(|i| (i as f32 * 262.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    xirius_session::export::write_wav(&file, &wave, &wave, 48_000, 16).unwrap();
    s.show_instrument(t, id);
    s.instruments.detached_target = Some(id);
    s.bind_window_io();
    s.prepare_song();
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.preparing.is_some() {
        s.poll_preparing();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let g = s.geometry(1600.0, 1000.0);
    let part = if device == "Drum Kit" {
        Part::Pad(36)
    } else {
        Part::Key(60)
    };
    let pitch = if device == "Drum Kit" { 36 } else { 60 };
    sample_io_empty_is_silent(&mut s, t, id, pitch);
    let (x, y) = if device == "Drum Kit" {
        s.instrument_center(&g, id, part).unwrap()
    } else {
        let w = s.instruments.open[0];
        let p = s.project();
        let (_, plugin, def) = super::window::live(&p, &w).unwrap();
        let r = s.instruments.frame(0, &g);
        let b = s.instruments.body(r, plugin, def);
        let r = b
            .spots
            .iter()
            .find(|s| s.mark == crate::device::layout::Mark::Drop)
            .unwrap()
            .r;
        (r.x + r.w / 2.0, r.y + 12.0)
    };
    let path = if fail {
        dir.path().join("missing.wav")
    } else {
        file
    };
    assert!(s.window_drop(&g, x, y, &[path]));
    assert!(s.saving.is_some());
    s.wait_for_save();
    if fail {
        assert!(
            s.error
                .as_deref()
                .is_some_and(|e| e.contains("見つかりません")),
            "{:?}",
            s.error
        );
        assert!(!s.instruments.open[0].filled);
        sample_io_empty_is_silent(&mut s, t, id, pitch);
        return;
    }
    assert!(s.error.is_none(), "{:?}", s.error);
    let deadline = Instant::now() + Duration::from_secs(60);
    while s.transport.step_swap().0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let (x, y) = s.instrument_center(&g, id, part).unwrap();
    let probe = s
        .transport
        .host()
        .unwrap()
        .plugins()
        .stock_probe(id)
        .unwrap();
    probe.watch(true);
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut file_voice = false;
    let mut peak = 0.0_f32;
    loop {
        file_voice |= probe.voice().is_some_and(|v| v.file);
        let (_, output) = probe.take_peaks();
        peak = peak.max(output[0]).max(output[1]);
        if file_voice && peak > 0.001 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a sample voice must replace the empty route and produce audio: {file_voice}, {peak}"
        );
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(s.instrument_up());
    wait_sounding(&mut s, id, |n| n == 0);
    if remove {
        assert!(s.run_command(
            "plugin.samples.remove",
            serde_json::json!({"id":id, "index":1})
        ));
        assert!(!s.instruments.open[0].filled);
        sample_io_empty_is_silent(&mut s, t, id, pitch);
    }
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
    s.instruments.detached_target = Some(id);
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

/// M-ui-3: the window losing the keyboard with a pad held (Alt+Tab) ends its note, since
/// the release goes to the other window.
#[test]
fn plg_066_a_pad_held_ends_when_the_window_loses_the_keyboard() {
    let (mut s, g, t, id) = played("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    assert!(s.device_focus_lost());
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 36 })));
    assert_eq!(s.instruments.note, None);
    assert!(s.instruments.grab.is_none());
    wait_sounding(&mut s, id, |n| n == 0);
    assert!(!s.device_focus_lost(), "nothing is held any more");
}

/// L-ui-11: a pad held while its device or its track is deleted ends its note; the next
/// frame sees the window gone.
#[test]
fn plg_066_a_pad_held_ends_when_its_device_or_track_goes() {
    for (command, of_track) in [("plugin.remove", false), ("track.delete", true)] {
        let (mut s, g, t, id) = played("Drum Kit");
        let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
        assert!(s.instrument_down(&g, x, y));
        wait_sounding(&mut s, id, |n| n > 0);
        let ids = if of_track {
            t.to_string()
        } else {
            id.to_string()
        };
        assert!(
            s.run_command(command, serde_json::json!({ "ids": [ids] })),
            "{command}: {:?}",
            s.error
        );
        s.poll_probes();
        assert_eq!(s.instruments.note, None, "{command}");
        assert_eq!(
            last_sent(&s),
            Some((t, NoteKind::Off { pitch: 36 })),
            "{command}"
        );
        assert!(
            !s.instrument_up() || s.instruments.grab.is_none(),
            "{command}"
        );
    }
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
    assert_eq!(pad.label(), Some("A1 C1, tone_C3"));
}

/// L2-ui-4: a pad held while the song closes ends its note before the song goes, not only
/// by the transport's reset.
#[test]
fn plg_066_a_pad_held_ends_when_the_song_closes() {
    let (mut s, g, t, id) = played("Drum Kit");
    let (x, y) = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
    assert!(s.instrument_down(&g, x, y));
    wait_sounding(&mut s, id, |n| n > 0);
    s.keep_leaving();
    assert_eq!(last_sent(&s), Some((t, NoteKind::Off { pitch: 36 })));
    assert_eq!(s.instruments.note, None);
}
