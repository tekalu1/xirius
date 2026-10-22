//! The PC keys as a keyboard (UI-074). Notes go to a silent output that keeps
//! time: nothing reaches the speakers.

use std::time::{Duration, Instant};

use xirius_session::host::Strip;
use xirius_session::output::OutputMode;

use super::keys::PcKeys;
use super::*;
use crate::song::Song;

/// A new song after 「MIDI を演奏」: one armed track playing the sampler.
fn played(mode: OutputMode) -> (Studio, TrackId) {
    let mut s = Studio::new(Song::empty());
    s.transport.mode = mode;
    assert!(s.start_midi());
    let t = s.pc_target().expect("the armed instrument track");
    (s, t)
}

/// Waits for fresh meter reports, with a guard for an engine that stops reporting.
fn wait_level(s: &mut Studio, t: TrackId, predicate: impl Fn(f32) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut matched = 0;
    let mut last = None;
    while Instant::now() < deadline {
        s.transport.poll();
        if let Some(r) = s.transport.take_levels().get(&Strip::Track(t)) {
            let peak = r.peak[0].max(r.peak[1]);
            last = Some(peak);
            matched = if predicate(peak) { matched + 1 } else { 0 };
            if matched == 3 {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    panic!("the track did not reach the expected level; last peak: {last:?}");
}

#[test]
fn a_to_l_are_the_white_keys_from_c_and_z_x_move_the_octave() {
    let keys = PcKeys::default();
    let white: Vec<_> = ["a", "s", "d", "f", "g", "h", "j", "k", "l"]
        .iter()
        .map(|k| keys.pitch(k).unwrap())
        .collect();
    assert_eq!(white, [60, 62, 64, 65, 67, 69, 71, 72, 74]);
    let black: Vec<_> = ["w", "e", "t", "y", "u", "o"]
        .iter()
        .map(|k| keys.pitch(k).unwrap())
        .collect();
    assert_eq!(black, [61, 63, 66, 68, 70, 73]);
    assert_eq!(keys.pitch("q"), None);

    let (mut s, _) = played(OutputMode::None);
    assert!(s.pc_key_down("z", false));
    assert_eq!(s.pc_keys.pitch("a"), Some(48));
    for _ in 0..5 {
        s.pc_key_down("z", false);
    }
    assert_eq!(s.pc_keys.octave, 2, "Z stops at octave 2");
    for _ in 0..9 {
        s.pc_key_down("x", false);
    }
    assert_eq!(s.pc_keys.octave, 6, "X stops at octave 6");
    assert_eq!(s.pc_keys.pitch("a"), Some(84));
}

#[test]
fn a_key_sounds_on_the_armed_track_until_let_go() {
    let (mut s, t) = played(OutputMode::Silent { paced: true });
    // The player is started by the first key; an idle song has no meter reports yet.
    assert!(
        s.transport
            .take_levels()
            .get(&Strip::Track(t))
            .is_none_or(|r| r.peak[0].max(r.peak[1]) < 1e-4)
    );
    assert!(s.pc_key_down("a", false));
    assert_eq!(s.error, None);
    assert_eq!(s.roll.audition, Some(60), "the key lights on the roll");
    wait_level(&mut s, t, |peak| peak > 0.01);
    // The OS repeating the held key plays nothing more.
    assert!(s.pc_key_down("a", true));
    assert!(s.pc_key_down("a", false));
    assert_eq!(s.pc_keys.down.len(), 1);

    assert!(s.pc_key_up("a"));
    assert!(s.pc_keys.down.is_empty());
    assert_eq!(s.roll.audition, None);
    wait_level(&mut s, t, |peak| peak < 1e-3);
    assert!(!s.pc_key_up("a"), "a key not held lets nothing go");
}

/// The lit key and the note it ends follow the key's own pitch, while the note sent carries
/// the track's transpose: letting go with a value other than 0 still puts the light out
/// (UI-312).
#[test]
fn a_key_lit_and_let_go_follows_the_keys_own_pitch_when_the_track_transposes() {
    let (mut s, t) = played(OutputMode::Silent { paced: true });
    s.song
        .run(
            "track.transpose",
            serde_json::json!({"ids": [t.to_string()], "semis": 12}),
        )
        .unwrap();
    assert!(s.pc_key_down("a", false));
    assert_eq!(s.roll.audition, Some(60), "the key lights at its own pitch");
    let held = s.pc_keys.down.first().expect("a key held");
    assert_eq!((held.2, held.3), (60, 72), "own pitch, then the sent one");
    assert!(s.pc_key_up("a"));
    assert_eq!(s.roll.audition, None, "the light goes off");
}

#[test]
fn the_note_a_key_started_ends_even_after_the_octave_moved() {
    let (mut s, _) = played(OutputMode::Silent { paced: true });
    assert!(s.pc_key_down("a", false));
    assert!(s.pc_key_down("x", false));
    assert!(s.pc_key_up("a"));
    assert!(s.pc_keys.down.is_empty());
    assert_eq!(s.error, None);
}

#[test]
fn the_keys_stay_letters_until_turned_on_and_escape_turns_them_off() {
    let mut s = Studio::sample();
    assert!(!s.pc_key_down("a", false), "off: A is the arrange key");
    let (mut s, _) = played(OutputMode::Silent { paced: true });
    assert!(s.pc_key_down("s", false));
    assert!(s.pc_key_down("escape", false));
    assert!(!s.pc_keys.on);
    assert!(s.pc_keys.down.is_empty(), "turning off lets the keys go");
    assert!(!s.pc_key_down("a", false));
}

#[test]
fn the_keys_leave_letters_to_the_browser_search_and_the_palette() {
    let (mut s, _) = played(OutputMode::Silent { paced: true });
    s.browser.typing = true;
    assert!(!s.pc_key_down("a", false));
    s.browser.typing = false;
    s.palette = Some(crate::palette::Palette::new(Vec::new()));
    assert!(!s.pc_key_down("a", false));
}

#[test]
fn without_an_output_a_key_says_so() {
    let (mut s, _) = played(OutputMode::None);
    assert!(s.pc_key_down("a", false));
    assert_eq!(s.error.as_deref(), Some("オーディオ出力がありません"));
    assert!(s.pc_keys.down.is_empty());
}

#[test]
fn the_selected_armed_track_is_played_first() {
    let (mut s, first) = played(OutputMode::None);
    assert!(s.start_midi_again());
    let second = s.pc_target().unwrap();
    assert_ne!(first, second, "the new track is selected");
    s.selected_row = Some(crate::tree::RowKind::Track(first));
    assert_eq!(s.pc_target(), Some(first));
}

impl Studio {
    /// Another instrument track, armed and selected, as the entry makes one.
    fn start_midi_again(&mut self) -> bool {
        let data = self
            .song
            .run(
                "track.add.with-instrument",
                serde_json::json!({ "instrument": DEFAULT, "arm": true }),
            )
            .unwrap();
        self.after_edit();
        let id = data["id"].as_str().unwrap().parse().unwrap();
        self.selected_row = Some(crate::tree::RowKind::Track(id));
        true
    }
}

#[test]
fn the_roll_keyboard_sounds_the_pitch_it_lights_until_let_go() {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    let t = s.roll_target().expect("the Keys clip in the roll").track.id;
    // As a press on the roll's keyboard does it: the roll lights the key and the studio applies it.
    let g = s.geometry(1440.0, 820.0);
    s.apply_roll(&g, crate::roll::Out::default());
    s.roll.audition = Some(60);
    s.apply_roll(&g, crate::roll::Out::default());
    assert_eq!(s.pc_keys.roll, Some((t, 60)));
    wait_level(&mut s, t, |peak| peak > 0.01);
    // Dragged to another pitch: the first ends and the next begins.
    s.roll.audition = Some(64);
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, Some((t, 64)));
    s.roll.audition = None;
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, None);
    wait_level(&mut s, t, |peak| peak < 1e-3);
    // Without an output the lit key stays silent and nothing is remembered.
    s.transport.mode = OutputMode::None;
    s.transport = crate::transport::Transport::new(OutputMode::None);
    s.roll.audition = Some(60);
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, None);
    assert_eq!(s.error, None);
}

#[test]
fn the_audition_tool_plays_the_clips_track_alone_while_held_and_stops_on_release() {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    let target = s.roll_target().expect("the Keys clip in the roll");
    let (t, at) = (target.track.id, target.clip.start);
    let g = s.geometry(1440.0, 820.0);
    s.apply_roll(&g, crate::roll::Out::default());
    s.playhead = 123;
    s.roll.listen = Some(at);
    s.apply_roll(&g, crate::roll::Out::default());
    assert_eq!(s.error, None);
    assert!(s.transport.playing);
    assert_eq!(s.pc_keys.listening, Some(123));
    assert_eq!(s.transport.from, at);
    assert!(
        s.project().tracks.values().all(|tr| !tr.solo),
        "the song itself keeps its mix"
    );
    wait_level(&mut s, t, |peak| peak > 0.01);
    // Let go: it stops, and the playhead is where it was.
    s.roll.listen = None;
    s.apply_roll(&g, crate::roll::Out::default());
    assert!(!s.transport.playing);
    assert_eq!(s.pc_keys.listening, None);
    assert_eq!(s.playhead, 123);
    // A song that is playing already is left alone.
    s.toggle_play();
    s.roll.listen = Some(at);
    s.apply_roll(&g, crate::roll::Out::default());
    assert_eq!(s.pc_keys.listening, None);
    s.roll.listen = None;
    s.stop_transport();
}

/// The roll's press and release as the screen delivers them.
fn roll_click(s: &mut Studio, g: &crate::studio::Geometry, p: (f32, f32), drag_to: (f32, f32)) {
    let m = crate::tools::Mods::NONE;
    assert!(s.roll_press(g, p.0, p.1, m, 1));
    if drag_to != p {
        let c = s.roll_ctx(g).unwrap();
        let out = s.roll.moved(&c, drag_to.0, drag_to.1, m);
        s.apply_roll(g, out);
    }
    let sounding = s.pc_keys.roll;
    let c = s.roll_ctx(g).unwrap();
    let out = s.roll.up(&c, drag_to.0, drag_to.1, m);
    s.apply_roll(g, out);
    assert_eq!(s.error, None);
    assert_eq!(s.pc_keys.roll, None, "the note ends with the release");
    assert!(sounding.is_some(), "it sounded while held");
}

#[test]
fn ui_285_a_note_drawn_or_picked_sounds_with_the_settings_as_they_come() {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    let g = s.geometry(1440.0, 820.0);
    s.apply_edit_prefs();
    let c = s.roll_ctx(&g).unwrap();
    let ids: Vec<String> = c.notes().iter().map(|n| n.id.to_string()).collect();
    s.song
        .run("note.delete", serde_json::json!({ "ids": ids }))
        .unwrap();
    s.resized(&g);
    let t = s.roll_target().unwrap().track.id;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let grid = s.roll.frame(c.body).grid;
    // Two pitches in sight, a row apart from the grid's middle.
    let low = v.pitch_at(grid.y + grid.h / 2.0) as u8;
    let high = low + 2;
    let at = |pitch: u8, tick: i64| (v.x(tick), v.y(pitch) + 3.0);
    let m = crate::tools::Mods::NONE;
    // The pencil's click: the new note sounds while the button is held.
    assert!(s.edit_key(&g, "5", m));
    let p = at(low, 960);
    assert!(s.roll_press(&g, p.0, p.1, m, 1));
    assert_eq!(s.pc_keys.roll, Some((t, low)));
    wait_level(&mut s, t, |peak| peak > 0.01);
    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.up(&c, p.0, p.1, m);
    s.apply_roll(&g, out);
    assert_eq!(s.pc_keys.roll, None);
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 1);
    // The pencil dragged to set the length, and the select tool's click on a note.
    let p = at(high, 2880);
    roll_click(&mut s, &g, p, (p.0 + 120.0, p.1));
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 2);
    assert!(s.edit_key(&g, "1", m));
    let p = at(low, 960);
    roll_click(&mut s, &g, (p.0 + 4.0, p.1), (p.0 + 4.0, p.1));
    assert_eq!(s.roll.selected.len(), 1);
    wait_level(&mut s, t, |peak| peak < 1e-3);
}

#[test]
fn ui_287_only_the_pressed_note_sounds_at_its_current_pitch_while_moving_and_deselecting_stays_silent()
 {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    let g = s.geometry(1440.0, 820.0);
    s.apply_edit_prefs();
    let c = s.roll_ctx(&g).unwrap();
    let ids: Vec<String> = c.notes().iter().map(|n| n.id.to_string()).collect();
    s.song
        .run("note.delete", serde_json::json!({ "ids": ids }))
        .unwrap();
    s.resized(&g);
    let t = s.roll_target().unwrap().track.id;
    let c = s.roll_ctx(&g).unwrap();
    let v = s.roll.view(&c);
    let grid = s.roll.frame(c.body).grid;
    let row_h = s.roll.row_h;
    let mid = v.pitch_at(grid.y + grid.h / 2.0) as u8;
    let low = mid - 3;
    let high = mid + 3;
    let at = |pitch: u8, tick: i64| (v.x(tick) + 6.0, v.y(pitch) + row_h * 0.5);
    let m = crate::tools::Mods::NONE;
    assert!(s.edit_key(&g, "5", m));
    for pitch in [low, mid, high] {
        let p = at(pitch, 960);
        roll_click(&mut s, &g, p, p);
    }
    assert_eq!(s.roll_ctx(&g).unwrap().notes().len(), 3);
    assert!(s.edit_key(&g, "1", m));

    // Marquee selection over the three notes stays silent.
    let (x0, y0) = (v.x(480), v.y(high + 1));
    let (x1, y1) = (v.x(1920), v.y(low - 1) + row_h);
    assert!(s.roll_press(&g, x0, y0, m, 1));
    assert_eq!(s.roll.audition, None);
    assert_eq!(s.pc_keys.roll, None);
    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.moved(&c, x1, y1, m);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, None);
    assert_eq!(s.pc_keys.roll, None);
    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.up(&c, x1, y1, m);
    s.apply_roll(&g, out);
    assert_eq!(s.pc_keys.roll, None);
    assert_eq!(s.roll.selected.len(), 3);

    // Grabbing the middle note (`mid`) among the three selected notes sounds `mid` -> `mid + 1` -> `mid + 2`,
    // never `low` or `high`, and horizontal movement keeps `mid` without re-triggering.
    let p = at(mid, 960);
    assert!(s.roll_press(&g, p.0, p.1, m, 1));
    assert_eq!(s.pc_keys.roll, Some((t, mid)));

    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.moved(&c, p.0 + 80.0, p.1, m);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, Some(mid));
    assert_eq!(s.pc_keys.roll, Some((t, mid)));

    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.moved(&c, p.0 + 80.0, p.1 - row_h, m);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, Some(mid + 1));
    assert_eq!(s.pc_keys.roll, Some((t, mid + 1)));

    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.moved(&c, p.0 + 80.0, p.1 - row_h * 2.0, m);
    s.apply_roll(&g, out);
    assert_eq!(s.roll.audition, Some(mid + 2));
    assert_eq!(s.pc_keys.roll, Some((t, mid + 2)));

    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.up(&c, p.0 + 80.0, p.1 - row_h * 2.0, m);
    s.apply_roll(&g, out);
    assert_eq!(s.pc_keys.roll, None);
    assert_eq!(s.roll.selected.len(), 3);

    // Shift+clicking a selected note to remove it from the selection stays silent.
    let c = s.roll_ctx(&g).unwrap();
    let moved_mid = c.notes().iter().find(|n| n.pitch == mid + 2).unwrap().start;
    let p_moved = at(mid + 2, moved_mid);
    let shift = crate::tools::Mods { shift: true, ..m };
    assert!(s.roll_press(&g, p_moved.0, p_moved.1, shift, 1));
    assert_eq!(s.roll.audition, None);
    assert_eq!(s.pc_keys.roll, None);
    let c = s.roll_ctx(&g).unwrap();
    let out = s.roll.up(&c, p_moved.0, p_moved.1, shift);
    s.apply_roll(&g, out);
    assert_eq!(s.pc_keys.roll, None);
    assert_eq!(s.roll.selected.len(), 2);
}
