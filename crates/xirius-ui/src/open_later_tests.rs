//! 開き終わるまで待たない (ENG-065): a song whose plugins run in host processes that take a
//! while to open. The window's frames keep their pace at the first press after 後で読む and
//! during a swap, a held key sounds when its instrument is in and a key let go of before that
//! never does, and what else plays goes on. Everything plays to a silent output that keeps time.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::TrackKind;
use xirius_session::output::OutputMode;

use crate::instrument::keys::{WAIT_GIVE_UP, WAIT_SHOWN};
use crate::studio::{Geometry, Studio};

/// How long the test plugin takes to be made in a host.
const OPEN_MS: u64 = 1500;
/// The longest a frame (or a press) may take while plugins open: far below an opening.
const FRAME: Duration = Duration::from_millis(250);

/// The tests set the plugin's opening time for the hosts they start, one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

struct Slow(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Slow {
    fn on() -> Slow {
        let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        crate::plugin_reload_shots::require_plugin_host();
        // SAFETY: the tests that read it hold the lock; the hosts read it as they start.
        unsafe { std::env::set_var("XIRIUS_TEST_VST3_OPEN_MS", OPEN_MS.to_string()) };
        Slow(guard)
    }
}

impl Drop for Slow {
    fn drop(&mut self) {
        // SAFETY: as in `on`.
        unsafe { std::env::remove_var("XIRIUS_TEST_VST3_OPEN_MS") };
    }
}

fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed())
}

/// The sample song with the test plugin as the Keys track's instrument and on two audio
/// tracks, each in a host process of its own; the Keys track and the instrument's id.
fn song() -> (Studio, TrackId, PluginId) {
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
fn later(s: &mut Studio) -> Geometry {
    s.prepare_song();
    s.tick();
    s.prepare_later();
    assert!(s.preparing.is_none());
    let g = s.geometry(1440.0, 820.0);
    s.apply_roll(&g, crate::roll::Out::default());
    g
}

/// One frame, as the window runs it; its length is kept in `worst`.
fn frame(s: &mut Studio, worst: &mut Duration) {
    let (_, took) = timed(|| s.tick());
    *worst = (*worst).max(took);
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
    let _slow = Slow::on();
    let (mut s, keys, _) = song();
    later(&mut s);
    s.roll.audition = Some(60);
    let (_, press) = timed(|| s.sound_roll_audition());
    let began = Instant::now();
    eprintln!("MEASURE first press after 後で読む: {press:?}");
    assert_eq!(s.pc_keys.roll, None, "its instrument is not in yet");
    assert!(s.pc_keys.waiting.is_some());
    let (mut worst, mut shown) = (Duration::ZERO, false);
    while s.pc_keys.roll.is_none() {
        assert!(began.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s, &mut worst);
        shown |= s.roll_key_waiting() == Some(60);
    }
    let sounded = began.elapsed();
    eprintln!("MEASURE worst frame while it opened: {worst:?}, sounded after {sounded:?}");
    assert_eq!(s.pc_keys.roll, Some((keys, 60)));
    assert!(shown, "the lit key showed the spinner meanwhile");
    assert_eq!(s.roll_key_waiting(), None, "and it is gone once it sounds");
    assert_eq!(s.pc_keys.waiting, None);
    assert!(!s.transport.preparing(), "the player started");
    assert_eq!(s.error, None);
    assert!(press < FRAME, "the press waited: {press:?}");
    assert!(worst < FRAME, "a frame waited: {worst:?}");
    assert!(
        sounded > WAIT_SHOWN,
        "it sounded only once the host answered"
    );
}

#[test]
fn a_key_let_go_of_before_its_instrument_is_in_never_sounds() {
    let _slow = Slow::on();
    let (mut s, keys, _) = song();
    later(&mut s);
    s.roll.audition = Some(62);
    s.sound_roll_audition();
    assert!(s.pc_keys.waiting.is_some());
    s.roll.audition = None;
    s.sound_roll_audition();
    assert_eq!(s.pc_keys.waiting, None);
    let mut worst = Duration::ZERO;
    let p = s.project();
    let dir = s.song.dir();
    wait_until("the instrument came in", || {
        s.tick();
        s.transport
            .instrument_ready(&p, dir.clone(), keys)
            .unwrap_or(false)
    });
    for _ in 0..10 {
        frame(&mut s, &mut worst);
    }
    assert_eq!(s.pc_keys.roll, None, "it was let go of before");
    assert_eq!(s.pc_keys.waiting, None);
    assert_eq!(s.error, None);
}

#[test]
fn a_pc_key_waits_for_its_instrument_the_same_way() {
    let _slow = Slow::on();
    let (mut s, keys, _) = song();
    later(&mut s);
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
    let (mut worst, mut shown) = (Duration::ZERO, false);
    let t = Instant::now();
    while !s.pc_keys.pc_waiting.is_empty() {
        assert!(t.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s, &mut worst);
        shown |= s.roll_key_waiting() == Some(pitch);
    }
    assert!(worst < FRAME, "a frame waited: {worst:?}");
    assert!(shown, "the lit key showed the spinner");
    assert_eq!(s.pc_keys.down.len(), 1, "only the held key goes on");
    assert!(s.pc_key_up("a"));
}

/// The song is opened, a note sounds, and playback runs.
fn playing(s: &mut Studio, keys: TrackId) {
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
    let _slow = Slow::on();
    let (mut s, keys, id) = song();
    later(&mut s);
    playing(&mut s, keys);
    for _ in 0..5 {
        std::thread::sleep(Duration::from_millis(20));
        s.tick();
    }
    let from = s.playhead;
    // The instrument moves to the shared host: a host process starts and opens it again.
    let (done, change) = timed(|| {
        s.run_command(
            "plugin.isolation.set",
            json!({"ids":[id.to_string()], "mode":"shared"}),
        )
    });
    assert!(done);
    eprintln!("MEASURE swap (placement change) stall: {change:?}");
    assert!(
        s.transport.host().expect("the player").swap_pending(),
        "the new mix waits for its plugin"
    );
    s.roll.audition = Some(64);
    let (_, press) = timed(|| s.sound_roll_audition());
    assert_eq!(s.pc_keys.roll, None, "the instrument is on its way");
    assert!(s.pc_keys.waiting.is_some());
    let began = Instant::now();
    let (mut worst, mut shown) = (Duration::ZERO, false);
    let mut moved = false;
    let mut last = from;
    while s.pc_keys.roll.is_none() {
        assert!(began.elapsed() < Duration::from_secs(60), "never sounded");
        frame(&mut s, &mut worst);
        shown |= s.roll_key_waiting() == Some(64);
        let now = s.playhead;
        moved |= now > last;
        last = now;
        assert!(
            s.transport.playing,
            "the song went on playing during the swap"
        );
    }
    eprintln!(
        "MEASURE swap: press {press:?}, worst frame {worst:?}, sounded after {:?}",
        began.elapsed()
    );
    assert_eq!(s.pc_keys.roll, Some((keys, 64)));
    assert!(shown, "the spinner turned while the swap was under way");
    assert_eq!(s.roll_key_waiting(), None, "and went when it sounded");
    assert!(moved && last > from, "the playhead moved: {from} -> {last}");
    assert!(!s.transport.host().unwrap().swap_pending());
    assert!(change < FRAME && press < FRAME && worst < FRAME);
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
    let _slow = Slow::on();
    let (mut s, keys, id) = song();
    later(&mut s);
    playing(&mut s, keys);
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
