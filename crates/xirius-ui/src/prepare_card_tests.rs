//! 音源を準備中 (ENG-064): the card that waits for a song's plugins, 後で読む, and the lit key
//! that waits for its instrument. Everything plays to a silent output that keeps time.

use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::PluginId;
use xirius_core::model::TrackKind;
use xirius_session::output::OutputMode;
use xirius_session::starting::UnitState;

use crate::instrument::keys::WAIT_SHOWN;
use crate::studio::Studio;

fn paced() -> OutputMode {
    OutputMode::Silent { paced: true }
}

/// The sample song with the test plugin on its first three audio tracks, opened in this
/// process, and the plugins' ids.
fn with_plugins() -> (Studio, Vec<PluginId>, Vec<String>) {
    let mut s = Studio::sample();
    let base = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = [
        base.join("xirius_test_vst3.dll"),
        base.join("deps/xirius_test_vst3.dll"),
    ]
    .into_iter()
    .find(|p| p.is_file())
    .expect("build xirius-test-vst3 first");
    let entries = xirius_plugins::probe(&dll).unwrap();
    let entry = entries[0].clone();
    let p = s.project();
    let tracks: Vec<_> = p
        .tracks
        .values()
        .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
        .take(3)
        .map(|t| (t.id, t.name.clone()))
        .collect();
    assert_eq!(tracks.len(), 3);
    let mut ids = Vec::new();
    for (track, _) in &tracks {
        assert!(s.run_command(
            "plugin.add",
            json!({
                "track":track.to_string(), "name":entry.name, "kind":"effect",
                "format":"vst3", "uid":entry.uid
            })
        ));
        let id = s.project().tracks[track].inserts.last().unwrap().id;
        assert!(s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":"inprocess"})
        ));
        ids.push(id);
    }
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = paced();
    let names = tracks.into_iter().map(|(_, n)| n).collect();
    (s, ids, names)
}

fn opened(s: &Studio, ids: &[PluginId]) -> bool {
    let host = s.transport.host().expect("the player");
    ids.iter()
        .all(|id| host.plugin_timings().opened(id.0).is_some())
}

#[test]
fn a_window_without_the_device_prepares_nothing_when_a_song_opens() {
    // --silent, the screenshots and the tests: no card, no player, no device.
    let (mut s, _, _) = with_plugins();
    s.song_replaced(Ok(None));
    assert!(s.preparing.is_none());
    assert!(!s.transport.preparing());
    assert!(s.transport.host().is_none());
    for _ in 0..3 {
        s.tick();
    }
    assert!(s.transport.host().is_none(), "nothing opened the device");
}

#[test]
fn the_card_counts_the_tracks_as_their_plugins_open_and_closes_when_the_song_sounds() {
    let (mut s, ids, names) = with_plugins();
    s.prepare_song();
    let card = s.preparing.as_ref().expect("the card");
    let total = card.progress.units.len();
    for name in &names {
        assert!(
            card.progress.units.iter().any(|(n, _)| n == name),
            "{name} is listed: {:?}",
            card.progress.units
        );
    }
    assert_eq!(card.progress.done, 0);
    assert!(
        s.transport.host().is_none(),
        "nothing plays before it is ready"
    );
    let mut counts = vec![0];
    let mut saw_working = false;
    let t = Instant::now();
    while s.preparing.is_some() {
        assert!(t.elapsed() < Duration::from_secs(60), "{counts:?}");
        s.tick();
        if let Some(card) = &s.preparing {
            saw_working |= card
                .progress
                .units
                .iter()
                .any(|(_, u)| *u == UnitState::Working);
            if counts.last() != Some(&card.progress.done) {
                counts.push(card.progress.done);
            }
        }
    }
    assert!(saw_working, "the track under way is shown");
    assert!(
        counts.windows(2).all(|w| w[0] < w[1]) && counts.len() > 2,
        "the count goes up a track at a time: {counts:?} of {total}"
    );
    assert_eq!(s.error, None);
    assert!(!s.transport.preparing());
    assert!(opened(&s, &ids), "each plugin's opening time is kept");
}

#[test]
fn later_closes_the_card_and_what_is_left_opens_when_first_needed() {
    let (mut s, ids, _) = with_plugins();
    s.prepare_song();
    s.tick();
    let g = s.geometry(1440.0, 820.0);
    let button = s.preparing.as_ref().unwrap().layout(g.size).button;
    // A press anywhere else belongs to the card and does nothing.
    assert!(s.prepare_press(&g, 2.0, 2.0));
    assert!(s.preparing.is_some());
    assert!(s.prepare_hover(&g, button.x + 2.0, button.y + 2.0));
    assert!(s.preparing.as_ref().unwrap().hover);
    assert!(s.prepare_press(&g, button.x + 2.0, button.y + 2.0));
    assert!(s.preparing.is_none(), "後で読む closes the card");
    assert!(
        !s.prepare_press(&g, 2.0, 2.0),
        "and the song takes presses again"
    );
    // Nothing opens by itself any more…
    let roll_open = s.drawer_open;
    s.drawer_open = false;
    for _ in 0..3 {
        s.tick();
    }
    assert!(s.transport.preparing());
    assert!(s.transport.host().is_none());
    // …until the song wants the player, as the roll of an instrument track does: then the
    // rest opens on the spot (UI-285).
    assert!(roll_open, "the sample opens with the Keys clip in the roll");
    s.drawer_open = true;
    s.tick();
    assert!(!s.transport.preparing());
    assert!(opened(&s, &ids));
}

#[test]
fn escape_is_later() {
    let (mut s, _, _) = with_plugins();
    s.prepare_song();
    assert!(s.prepare_key("a"), "every key waits while the card is up");
    assert!(s.preparing.is_some());
    assert!(s.prepare_key("escape"));
    assert!(s.preparing.is_none());
    assert!(!s.prepare_key("escape"));
}

#[test]
fn a_held_key_sounds_once_its_instrument_is_ready_and_never_after_it_is_let_go() {
    let mut s = Studio::sample();
    s.transport.mode = paced();
    let t = s.roll_target().expect("the Keys clip in the roll").track.id;
    let g = s.geometry(1440.0, 820.0);
    s.apply_roll(&g, crate::roll::Out::default());
    s.transport.not_ready = vec![t];
    s.roll.audition = Some(60);
    s.apply_roll(&g, crate::roll::Out::default());
    assert_eq!(
        s.pc_keys.roll, None,
        "nothing is sent to an instrument not ready"
    );
    assert!(s.pc_keys.waiting.is_some());
    assert_eq!(s.roll_key_waiting(), None, "a short wait shows nothing");
    std::thread::sleep(WAIT_SHOWN + Duration::from_millis(20));
    assert_eq!(
        s.roll_key_waiting(),
        Some(60),
        "the lit key shows the spinner"
    );
    assert!(s.tick(), "frames keep coming while it waits");
    assert_eq!(s.pc_keys.roll, None);
    // Ready while still held: it sounds from then on.
    s.transport.not_ready.clear();
    s.tick();
    assert_eq!(s.pc_keys.roll, Some((t, 60)));
    assert_eq!(s.pc_keys.waiting, None);
    assert_eq!(s.roll_key_waiting(), None);
    s.roll.audition = None;
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, None);

    // Let go of before it was ready: it never sounds.
    s.transport.not_ready = vec![t];
    s.roll.audition = Some(62);
    s.sound_roll_audition();
    assert!(s.pc_keys.waiting.is_some());
    s.roll.audition = None;
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.waiting, None);
    s.transport.not_ready.clear();
    s.tick();
    assert_eq!(s.pc_keys.roll, None);

    // An instrument that is never ready: the key gives up as every spinner does.
    s.transport.not_ready = vec![t];
    s.roll.audition = Some(64);
    s.sound_roll_audition();
    let (want, _) = s.pc_keys.waiting.unwrap();
    s.pc_keys.waiting = Some((
        want,
        Instant::now() - crate::instrument::keys::WAIT_GIVE_UP - Duration::from_millis(1),
    ));
    s.tick();
    assert_eq!(s.pc_keys.waiting, None);
    assert_eq!(s.pc_keys.missed, Some((t, 64)));
    s.transport.not_ready.clear();
    s.tick();
    assert_eq!(
        s.pc_keys.roll, None,
        "given up: it is not tried on every frame"
    );
    assert_eq!(s.error, None);
}
