//! 開き終わるまで待たない (ENG-065): a song whose plugins run in host processes that take a
//! while to open. The window's frames keep their pace at the first press after 後で読む and
//! during a swap, a held key sounds when its instrument is in and a key let go of before that
//! never does, and what else plays goes on. Everything plays to a silent output that keeps time.

use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::TrackKind;
use xirius_session::output::OutputMode;

use crate::instrument::keys::{WAIT_GIVE_UP, WAIT_SHOWN};
use crate::studio::{Geometry, Studio};

/// Every test gets a private copied DLL and a gate read by its host processes.
struct Slow {
    root: tempfile::TempDir,
    dll: std::path::PathBuf,
}
impl Slow {
    fn on() -> Self {
        crate::plugin_reload_shots::require_plugin_host();
        let root = tempfile::tempdir().unwrap();
        let base = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let source = [
            base.join("xirius_test_vst3.dll"),
            base.join("deps/xirius_test_vst3.dll"),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .expect("build fixture");
        let dll = root.path().join("opening.dll");
        std::fs::copy(source, &dll).unwrap();
        Self { root, dll }
    }
    fn hold(&self) {
        let _ = std::fs::remove_file(self.dll.with_extension("entered"));
        let _ = std::fs::remove_file(self.dll.with_extension("timed-out"));
        std::fs::write(self.dll.with_extension("gate"), b"hold").unwrap();
    }
    fn entered(&self) {
        wait_until("host entered its gate", || {
            self.dll.with_extension("entered").exists()
        });
    }
    fn release(&self) {
        assert!(
            !self.dll.with_extension("timed-out").exists(),
            "host gate timed out"
        );
        let _ = std::fs::remove_file(self.dll.with_extension("gate"));
    }
}
impl Drop for Slow {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dll.with_extension("gate"));
        let _ = self.root.path();
    }
}

/// The sample song with the test plugin as the Keys track's instrument and on two audio
/// tracks, each in a host process of its own; the Keys track and the instrument's id.
fn song(slow: &Slow) -> (Studio, TrackId, PluginId) {
    let mut s = Studio::sample();
    let entries = xirius_plugins::probe(&slow.dll).unwrap();
    let entry = entries[0].clone();
    let keys = s.roll_target().expect("the Keys clip in the roll").track.id;
    let tracks: Vec<_> = s
        .project()
        .tracks
        .values()
        .filter(|t| matches!(t.kind, TrackKind::Audio | TrackKind::Instrument))
        .take(3)
        .map(|t| t.id)
        .collect();
    let mut tracks = tracks;
    if !tracks.contains(&keys) {
        tracks.push(keys);
    }
    let mut instrument = None;
    for track in &tracks {
        let is_keys = *track == keys;
        assert!(s.run_command(
            "plugin.add",
            json!({
                "track":track.to_string(), "name":entry.name,
                "kind": if is_keys { "instrument" } else { "effect" },
                "format":"vst3", "uid":entry.uid
            })
        ));
        let inserts = &s.project().tracks[track].inserts;
        let id = if is_keys {
            inserts[0].id
        } else {
            inserts.last().unwrap().id
        };
        if is_keys {
            instrument = Some(id);
        }
        assert!(s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":"dedicated"})
        ));
    }
    s.use_plugins(xirius_plugins::Catalog {
        entries,
        failures: Vec::new(),
    });
    s.transport.mode = OutputMode::Silent { paced: true };
    (s, keys, instrument.unwrap())
}

/// 後で読む right after the card came up: the hosts were asked and have not answered.
fn later(s: &mut Studio, slow: &Slow) -> Geometry {
    slow.hold();
    s.prepare_song();
    s.tick();
    s.prepare_later();
    slow.entered();
    assert!(s.preparing.is_none());
    let g = s.geometry(1440.0, 820.0);
    s.apply_roll(&g, crate::roll::Out::default());
    g
}

/// One frame while the host gate stays closed. Sleep only yields to worker processes.
fn frame(s: &mut Studio) {
    s.tick();
    std::thread::sleep(Duration::from_millis(8));
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let t = Instant::now();
    while !done() {
        assert!(t.elapsed() < Duration::from_secs(60), "{what}");
        std::thread::sleep(Duration::from_millis(8));
    }
}

#[test]
fn the_first_press_after_later_does_not_stall_the_window_and_the_held_key_sounds_when_it_is_in() {
    let slow = Slow::on();
    let (mut s, keys, _) = song(&slow);
    later(&mut s, &slow);
    s.roll.audition = Some(60);
    s.sound_roll_audition();
    let began = Instant::now();
    assert_eq!(s.pc_keys.roll, None, "its instrument is not in yet");
    assert!(s.pc_keys.waiting.is_some());
    let (want, _) = s.pc_keys.waiting.unwrap();
    s.pc_keys.waiting = Some((want, Instant::now() - WAIT_SHOWN - Duration::from_millis(1)));
    frame(&mut s);
    assert_eq!(s.roll_key_waiting(), Some(60));
    assert!(s.pc_keys.roll.is_none());
    slow.release();
    let mut shown = true;
    while s.pc_keys.roll.is_none() {
        assert!(began.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s);
        shown |= s.roll_key_waiting() == Some(60);
    }
    assert_eq!(s.pc_keys.roll, Some((keys, 60)));
    assert!(shown, "the lit key showed the spinner meanwhile");
    assert_eq!(s.roll_key_waiting(), None, "and it is gone once it sounds");
    assert_eq!(s.pc_keys.waiting, None);
    assert!(!s.transport.preparing(), "the player started");
    assert_eq!(s.error, None);
}

#[test]
fn a_key_let_go_of_before_its_instrument_is_in_never_sounds() {
    let slow = Slow::on();
    let (mut s, keys, _) = song(&slow);
    later(&mut s, &slow);
    s.roll.audition = Some(62);
    s.sound_roll_audition();
    assert!(s.pc_keys.waiting.is_some());
    s.roll.audition = None;
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.waiting, None);
    slow.release();
    let p = s.project();
    let dir = s.song.dir();
    wait_until("the instrument came in", || {
        s.tick();
        s.transport
            .instrument_ready(&p, dir.clone(), keys)
            .unwrap_or(false)
    });
    for _ in 0..10 {
        frame(&mut s);
    }
    assert_eq!(s.pc_keys.roll, None, "it was let go of before");
    assert_eq!(s.pc_keys.waiting, None);
    assert_eq!(s.error, None);
}

#[test]
fn a_pc_key_waits_for_its_instrument_the_same_way() {
    let slow = Slow::on();
    let (mut s, keys, _) = song(&slow);
    later(&mut s, &slow);
    assert!(s.run_command("track.arm", json!({"ids": [keys.to_string()], "on": true})));
    s.pc_keys.on = true;
    assert!(s.pc_key_down("a", false));
    let pitch = s.pc_keys.down[0].2;
    assert_eq!(
        s.pc_keys.pc_waiting.len(),
        1,
        "not sent to an instrument not in"
    );
    // A second key let go of before it is in never sounds.
    assert!(s.pc_key_down("s", false));
    assert!(s.pc_key_up("s"));
    assert_eq!(s.pc_keys.pc_waiting.len(), 1);
    s.pc_keys.pc_waiting[0].3 = Instant::now() - WAIT_SHOWN - Duration::from_millis(1);
    frame(&mut s);
    assert_eq!(s.roll_key_waiting(), Some(pitch));
    assert_eq!(s.pc_keys.down.len(), 1);
    slow.release();
    let mut shown = true;
    let t = Instant::now();
    while !s.pc_keys.pc_waiting.is_empty() {
        assert!(t.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s);
        shown |= s.roll_key_waiting() == Some(pitch);
    }
    assert!(shown, "the lit key showed the spinner");
    assert_eq!(s.pc_keys.down.len(), 1, "only the held key goes on");
    assert!(s.pc_key_up("a"));
}

/// The song is opened, a note sounds, and playback runs.
fn playing(s: &mut Studio, keys: TrackId, slow: &Slow) {
    slow.release();
    s.roll.audition = Some(60);
    let (p, dir) = (s.project(), s.song.dir());
    wait_until("the instrument came in", || {
        s.tick();
        s.transport
            .instrument_ready(&p, dir.clone(), keys)
            .unwrap_or(false)
    });
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, Some((keys, 60)));
    s.roll.audition = None;
    s.sound_roll_audition();
    assert!(s.transport.play(&p, dir, 0).is_ok());
    s.tick();
    assert!(s.transport.playing);
}

#[test]
fn a_swap_shows_the_spinner_and_playback_goes_on() {
    let slow = Slow::on();
    let (mut s, keys, id) = song(&slow);
    later(&mut s, &slow);
    playing(&mut s, keys, &slow);
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(20));
        s.tick();
    }
    let from = s.playhead;
    // The instrument moves to the shared host: a host process starts and opens it again.
    slow.hold();
    let done = s.run_command(
        "plugin.isolation.set",
        json!({"ids":[id.to_string()], "mode":"shared"}),
    );
    assert!(done);
    wait_until("swap host entered its gate", || {
        s.tick();
        slow.dll.with_extension("entered").exists()
    });
    assert!(
        s.transport.host().expect("the player").swap_pending(),
        "the new mix waits for its plugin"
    );
    s.roll.audition = Some(64);
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.roll, None, "the instrument is on its way");
    assert!(s.pc_keys.waiting.is_some());
    let (want, _) = s.pc_keys.waiting.unwrap();
    s.pc_keys.waiting = Some((want, Instant::now() - WAIT_SHOWN - Duration::from_millis(1)));
    frame(&mut s);
    assert_eq!(s.roll_key_waiting(), Some(64));
    assert!(s.transport.playing);
    wait_until("playback progresses while the swap is held", || {
        s.tick();
        s.playhead > from
    });
    slow.release();
    let began = Instant::now();
    let mut shown = true;
    let mut moved = false;
    let mut last = from;
    while s.pc_keys.roll.is_none() {
        assert!(began.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s);
        shown |= s.roll_key_waiting() == Some(64);
        let now = s.playhead;
        moved |= now > last;
        last = now;
        assert!(
            s.transport.playing,
            "the song went on playing during the swap"
        );
    }
    assert_eq!(s.pc_keys.roll, Some((keys, 64)));
    assert!(shown, "the spinner turned while the swap was under way");
    assert_eq!(s.roll_key_waiting(), None, "and went when it sounded");
    assert!(moved && last > from, "the playhead moved: {from} -> {last}");
    assert!(!s.transport.host().unwrap().swap_pending());
    // The sample's audio files are not on disk; that is all the swap may say.
    assert!(
        s.error
            .as_deref()
            .is_none_or(|e| !e.contains("読み込めません")),
        "{:?}",
        s.error
    );
}

#[test]
fn a_key_gives_up_after_the_wait_when_the_swap_does_not_come() {
    let slow = Slow::on();
    let (mut s, keys, id) = song(&slow);
    later(&mut s, &slow);
    playing(&mut s, keys, &slow);
    slow.hold();
    assert!(s.run_command(
        "plugin.isolation.set",
        json!({"ids":[id.to_string()], "mode":"shared"})
    ));
    s.roll.audition = Some(65);
    s.sound_roll_audition();
    let (want, _) = s.pc_keys.waiting.expect("it waits");
    s.pc_keys.waiting = Some((
        want,
        Instant::now() - WAIT_GIVE_UP - Duration::from_millis(1),
    ));
    s.tick();
    assert_eq!(s.pc_keys.waiting, None, "it gave up");
    assert_eq!(s.pc_keys.missed, Some((keys, 65)));
    slow.release();
    // It does not sound later, when the instrument is in.
    let (p, dir) = (s.project(), s.song.dir());
    wait_until("the instrument came in", || {
        s.tick();
        s.transport
            .instrument_ready(&p, dir.clone(), keys)
            .unwrap_or(false)
    });
    s.tick();
    assert_eq!(s.pc_keys.roll, None);
}
