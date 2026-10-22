//! Where a note played on a MIDI keyboard (or the PC keys) goes, with two instrument
//! tracks (REC-024): the armed ones. With 「インストゥルメントの入力を選択に合わせる」
//! on, choosing a track arms it and lets the others go, so the selected one alone
//! sounds. Notes go to a silent output that keeps time: nothing reaches the
//! speakers, no window opens.

use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::TrackId;
use xirius_engine::record::MockMidi;
use xirius_session::host::Strip;
use xirius_session::output::OutputMode;
use xirius_ui::song::Song;
use xirius_ui::studio::Studio;
use xirius_ui::tree::RowKind;

/// A song with Sampler track A (armed or not) and Sampler track B (not armed).
fn two_tracks(a_armed: bool) -> (Studio, TrackId, TrackId) {
    two_tracks_with(a_armed, |_| {})
}

/// `two_tracks`, the song first put through `before` (a setting, say).
fn two_tracks_with(a_armed: bool, before: impl FnOnce(&mut Song)) -> (Studio, TrackId, TrackId) {
    let mut song = Song::empty();
    let mut add = |name: &str, arm: bool| -> TrackId {
        let data = song
            .run(
                "track.add.with-instrument",
                json!({ "instrument": "Sampler", "name": name, "arm": arm }),
            )
            .expect("track added");
        data["id"].as_str().unwrap().parse().unwrap()
    };
    let a = add("A", a_armed);
    let b = add("B", false);
    before(&mut song);
    let mut s = Studio::new(song);
    s.transport.mode = OutputMode::Silent { paced: true };
    (s, a, b)
}

/// The loudest peak of each track: waits (up to 60 s, for the player to start) until one
/// track is loud, then keeps reading for `secs` seconds more.
fn peaks(s: &mut Studio, tracks: &[TrackId], secs: f32) -> Vec<f32> {
    let mut max = vec![0f32; tracks.len()];
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut until = None;
    while Instant::now() < until.unwrap_or(deadline) {
        s.transport.poll();
        let levels = s.transport.take_levels();
        for (i, t) in tracks.iter().enumerate() {
            if let Some(r) = levels.get(&Strip::Track(*t)) {
                max[i] = max[i].max(r.peak[0].max(r.peak[1]));
            }
        }
        if until.is_none() && max.iter().any(|&x| x > 0.01) {
            until = Some(Instant::now() + Duration::from_secs_f32(secs));
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    max
}

fn keyboard_note() -> Box<MockMidi> {
    Box::new(MockMidi::with_ports(
        vec!["EDIROL PC-50".into()],
        vec![(0, 0, [0x90, 60, 100])],
    ))
}

/// The loudest peak of each track in a fixed window, whether it sounds or not.
fn peak_for(s: &mut Studio, tracks: &[TrackId], secs: f32) -> Vec<f32> {
    let mut max = vec![0f32; tracks.len()];
    let until = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < until {
        s.transport.poll();
        let levels = s.transport.take_levels();
        for (i, t) in tracks.iter().enumerate() {
            if let Some(r) = levels.get(&Strip::Track(*t)) {
                max[i] = max[i].max(r.peak[0].max(r.peak[1]));
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    max
}

/// A track's transpose reaches the keys played: a note the transpose takes off the keyboard
/// does not sound (UI-312).
#[test]
fn the_tracks_transpose_reaches_the_keys_played() {
    let (mut s, a, _) = two_tracks_with(true, |song| {
        let a = song
            .project()
            .tracks
            .values()
            .find(|t| t.name == "A")
            .unwrap()
            .id;
        song.run(
            "track.transpose",
            json!({"ids": [a.to_string()], "semis": -64}),
        )
        .unwrap();
    });
    s.select_row(Some(RowKind::Track(a)));
    assert_eq!(s.project().tracks[&a].transpose, -64);
    s.live_midi_input(keyboard_note());
    s.poll_live_midi();
    let p = peak_for(&mut s, &[a], 3.0);
    eprintln!("MIDI keyboard 60, track transpose -64: peak {:.4}", p[0]);
    assert!(p[0] < 1e-3, "60 - 64 is off the keyboard, so it is silent");
}

/// A track's velocity reaches the keys played: -100% nearly silences a note that otherwise
/// sounds (UI-312).
#[test]
fn the_tracks_velocity_reaches_the_keys_played() {
    let (mut base, a, _) = two_tracks(true);
    base.select_row(Some(RowKind::Track(a)));
    base.live_midi_input(keyboard_note());
    base.poll_live_midi();
    let loud = peaks(&mut base, &[a], 2.0)[0];
    assert!(loud > 0.01, "without the track's velocity it sounds");

    let (mut s, a, _) = two_tracks_with(true, |song| {
        let a = song
            .project()
            .tracks
            .values()
            .find(|t| t.name == "A")
            .unwrap()
            .id;
        song.run(
            "track.velocity",
            json!({"ids": [a.to_string()], "percent": -100}),
        )
        .unwrap();
    });
    s.select_row(Some(RowKind::Track(a)));
    assert_eq!(s.project().tracks[&a].velocity, -100);
    s.live_midi_input(keyboard_note());
    s.poll_live_midi();
    let quiet = peak_for(&mut s, &[a], 3.0)[0];
    eprintln!("MIDI keyboard 100/127, track velocity -100%: peak {quiet:.4} (was {loud:.4})");
    assert!(quiet < loud / 5.0, "the note is much quieter");
}

fn armed(s: &Studio) -> Vec<TrackId> {
    s.project()
        .tracks
        .values()
        .filter(|t| t.armed)
        .map(|t| t.id)
        .collect()
}

/// The MIDI keyboard: selecting B arms it and lets A go; only B sounds.
#[test]
fn a_keyboard_note_sounds_on_the_selected_track_only() {
    let (mut s, a, b) = two_tracks(true);
    s.select_row(Some(RowKind::Track(b)));
    assert_eq!(armed(&s), [b], "selecting B armed it and let A go");
    s.live_midi_input(keyboard_note());
    s.poll_live_midi();
    assert_eq!(s.error, None);
    let p = peaks(&mut s, &[a, b], 4.0);
    eprintln!(
        "MIDI keyboard, A was armed, B selected: peak A {:.4}, B {:.4}",
        p[0], p[1]
    );
    assert!(p[1] > 0.01, "the selected track B sounds");
    assert!(p[0] < 1e-3, "the unselected track A stays silent");
}

/// Moving the selection moves the arm: A armed, then B chosen lets A go, and the
/// song file holds the new arm.
#[test]
fn the_arm_follows_the_selection_and_is_saved_in_the_song() {
    let (mut s, a, b) = two_tracks(true);
    s.select_row(Some(RowKind::Track(a)));
    s.select_row(Some(RowKind::Track(b)));
    let p = s.project();
    assert!(!p.tracks[&a].armed && !p.tracks[&a].monitor, "A let go");
    assert!(p.tracks[&b].armed && p.tracks[&b].monitor, "B taken over");
    let saved = serde_json::to_value(&*p.tracks[&b]).unwrap();
    assert_eq!(
        saved["armed"],
        json!(true),
        "the arm is written to the song"
    );
}

/// The PC keys play the selected track, not another armed one.
#[test]
fn the_pc_keys_play_the_selected_track() {
    let (mut s, a, b) = two_tracks(true);
    s.select_row(Some(RowKind::Track(b)));
    assert_eq!(s.pc_target(), Some(b), "the PC keys aim at the selected B");
    s.pc_keys.on = true;
    assert!(s.pc_key_down("a", false));
    let p = peaks(&mut s, &[a, b], 4.0);
    eprintln!("PC keys, B selected: peak A {:.4}, B {:.4}", p[0], p[1]);
    assert!(p[1] > 0.01, "the selected B sounds");
    assert!(p[0] < 1e-3, "A stays silent");
    s.pc_key_up("a");
}

/// The setting off: the selection touches nothing; the armed track alone sounds,
/// selected or not.
#[test]
fn with_the_setting_off_only_the_armed_track_sounds() {
    let (mut s, a, b) = two_tracks_with(true, |song| {
        song.set(
            "prefs.set",
            json!({"key": "midi.follow-selection", "value": "false"}),
        )
        .unwrap();
    });
    s.select_row(Some(RowKind::Track(b)));
    assert_eq!(armed(&s), [a], "selecting B changed nothing");
    assert_eq!(s.pc_target(), Some(a), "the PC keys aim at the armed one");
    s.live_midi_input(keyboard_note());
    s.poll_live_midi();
    let p = peaks(&mut s, &[a, b], 4.0);
    eprintln!(
        "MIDI keyboard, setting off: peak A {:.4}, B {:.4}",
        p[0], p[1]
    );
    assert!(p[0] > 0.01, "the armed A sounds");
    assert!(p[1] < 1e-3, "the selected B stays silent");
}

/// An audio track's arm is not the instrument input's business: it stays.
#[test]
fn an_audio_tracks_arm_is_left_alone() {
    let mut song = Song::empty();
    song.run("track.add.audio", json!({ "name": "Vocal" }))
        .unwrap();
    let vocal: TrackId = {
        let p = song.project();
        p.tracks
            .values()
            .find(|t| t.kind == xirius_core::model::TrackKind::Audio)
            .map(|t| t.id)
            .unwrap()
    };
    song.run("track.arm", json!({ "ids": [vocal.to_string()] }))
        .unwrap();
    let made = song
        .run(
            "track.add.with-instrument",
            json!({ "instrument": "Sampler", "name": "Keys", "arm": true }),
        )
        .unwrap();
    let a: TrackId = made["id"].as_str().unwrap().parse().unwrap();
    let made = song
        .run(
            "track.add.with-instrument",
            json!({ "instrument": "Sampler", "name": "Pad" }),
        )
        .unwrap();
    let b: TrackId = made["id"].as_str().unwrap().parse().unwrap();
    let mut s = Studio::new(song);
    s.select_row(Some(RowKind::Track(b)));
    let p = s.project();
    assert!(!p.tracks[&a].armed, "the other instrument let go");
    assert!(p.tracks[&b].armed && p.tracks[&b].monitor);
    assert!(p.tracks[&vocal].armed, "the audio track stays armed");
}

/// Arms kept in a saved song are normalized once an instrument track is chosen:
/// the chosen one alone is armed.
#[test]
fn arms_left_over_in_an_opened_song_are_normalized_on_selection() {
    let mut song = Song::empty();
    let mut add = |name: &str| -> TrackId {
        song.run(
            "track.add.with-instrument",
            json!({ "instrument": "Sampler", "name": name, "arm": true }),
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let (a, b) = (add("A"), add("B"));
    let mut s = Studio::new(song);
    assert_eq!(armed(&s).len(), 2, "as the song was saved");
    s.select_row(Some(RowKind::Track(b)));
    assert_eq!(armed(&s), [b], "the selected one alone is armed");
    let _ = a;
}

/// 「MIDI を演奏」 twice: the second track is the one armed, and it alone sounds.
#[test]
fn midi_wo_ensou_twice_arms_only_the_last_track() {
    let mut s = Studio::new(Song::empty());
    s.transport.mode = OutputMode::Silent { paced: true };
    assert!(s.start_midi());
    assert!(s.start_midi());
    let armed = armed(&s);
    assert_eq!(armed.len(), 1, "the last made track alone is armed");
    s.live_midi_input(keyboard_note());
    s.poll_live_midi();
    let peaks = peaks(&mut s, &armed, 4.0);
    eprintln!("「MIDI を演奏」 x2, a keyboard note: peaks {peaks:?}");
    assert!(peaks.iter().all(|&x| x > 0.01), "it sounds");
}

/// A duplicated armed track is armed too — until the selection follows the input
/// to the one chosen.
#[test]
fn a_duplicate_of_an_armed_track_is_armed() {
    let mut song = Song::empty();
    let data = song
        .run(
            "track.add.with-instrument",
            json!({ "instrument": "Sampler", "name": "A", "arm": true }),
        )
        .unwrap();
    let a = data["id"].as_str().unwrap().to_string();
    song.run("track.duplicate", json!({ "ids": [a] })).unwrap();
    let p = song.project();
    let armed: Vec<_> = p
        .tracks
        .values()
        .filter(|t| t.armed)
        .map(|t| t.name.clone())
        .collect();
    eprintln!("armed after duplicating A: {armed:?}");
    assert_eq!(armed.len(), 2, "the copy is armed as well");
}

/// REC-025: choosing a frozen instrument track leaves the arms as they are —
/// it cannot take a take and its instrument is silent, so arming it would only
/// drop the other tracks' takes (freeze::check turns a frozen track's take
/// away, and the whole `record.take.add` with it).
#[test]
fn a_frozen_instrument_track_is_not_armed_by_the_selection() {
    let mut song = Song::empty();
    let mut add = |name: &str, arm: bool| -> TrackId {
        song.run(
            "track.add.with-instrument",
            json!({ "instrument": "Sampler", "name": name, "arm": arm }),
        )
        .unwrap()["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap()
    };
    let (a, b) = (add("A", true), add("B", false));
    // A clip is needed to freeze; the file's name is only stored.
    song.run(
        "clip.add.midi",
        json!({ "track": b.to_string(), "at": 0, "bars": 1 }),
    )
    .unwrap();
    song.run(
        "track.freeze.apply",
        json!({ "tracks": [{ "id": b.to_string(), "file": "Freeze/B.wav", "start": 0 }] }),
    )
    .unwrap();
    let mut s = Studio::new(song);
    s.select_row(Some(RowKind::Track(b)));
    let p = s.project();
    assert!(
        !p.tracks[&b].armed && !p.tracks[&b].monitor,
        "the frozen track is not armed"
    );
    assert!(
        p.tracks[&a].armed,
        "the other track's arm is left as it was"
    );
}
