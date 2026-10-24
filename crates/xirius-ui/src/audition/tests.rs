//! The audition plays the chosen file on a player of its own, silently in a test (BRW-007,
//! BRW-020). The player reads the file on its own threads, so a test turns the
//! screen's look until what it waits for has come.

use std::time::{Duration, Instant};

use std::sync::{Arc, Mutex};

use super::*;
use crate::browser::RailTab;

/// A place of WAV and MIDI files in a folder of the test's own, which goes when the guard is
/// dropped (also when the test fails).
fn place(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(&format!("xirius-ui-audition-{name}-"))
        .tempdir()
        .unwrap();
    let top = dir.path().join("素材");
    crate::import_shots::make_place(&top);
    (dir, top)
}

/// How long a wait for the player may take before the test gives up: far over what it needs,
/// for a machine busy with a parallel build.
const GUARD: Duration = Duration::from_secs(60);

fn studio(mode: OutputMode, top: &Path) -> Studio {
    let mut s = Studio::sample();
    s.transport.mode = mode;
    // The peaks the player makes go beside the test's files, not in the user's store.
    s.browser.peak_store = Some(top.parent().unwrap().join("Peaks"));
    s.browser.press_rail(RailTab::Files);
    s
}

/// Turns the screen's look until `done`.
fn until(s: &mut Studio, what: &str, done: impl Fn(&Studio) -> bool) {
    let started = Instant::now();
    while !done(s) {
        assert!(started.elapsed() < GUARD, "{what}");
        s.follow_audition();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_chosen_file_plays_to_its_end_and_lets_go() {
    let (_tmp, top) = place("ends");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let kick = top.join("Drums/kick_01.wav").to_string_lossy().into_owned();
    s.browser.add_place(&top).unwrap();
    s.browser.select_file(&kick);
    assert!(s.follow_audition());
    assert!(is_playing());
    let started = Instant::now();
    let mut seen = 0.0f32;
    while s.browser.playing.is_some() {
        assert!(started.elapsed() < GUARD, "still playing at {seen}");
        // Read just before the look that decides whether it has ended, so the last value read
        // is where it was when it let go.
        if let Some(p) = polled(&kick) {
            seen = seen.max(p);
        }
        s.follow_audition();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(s.error, None);
    assert_eq!(s.browser.playing, None, "it ended by itself");
    assert!(!is_playing());
    assert!(seen > 0.5, "{seen}");
    // Half a second, paced: not over at once.
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(drawn(&kick), "its peaks stay whole for the row");
}

#[test]
fn another_file_or_stop_ends_what_plays() {
    let (_tmp, top) = place("switch");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    let lp = top
        .join("Loops/loop_120bpm.wav")
        .to_string_lossy()
        .into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    assert!(progress(&pad).is_some(), "asked for at once");
    until(&mut s, "the pad plays", |_| {
        current().is_some_and(|n| n.phase == Phase::Playing)
    });
    s.browser.select_file(&lp);
    s.follow_audition();
    assert_eq!(progress(&pad), None);
    assert!(progress(&lp).is_some());
    s.browser.toggle_audition();
    s.follow_audition();
    assert!(!is_playing());
    assert_eq!(s.browser.playing, None);
}

#[test]
fn letting_go_of_the_device_stops_what_plays_and_the_row_follows() {
    let (_tmp, top) = place("release");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    until(&mut s, "the pad plays", |_| {
        current().is_some_and(|n| n.phase == Phase::Playing)
    });
    // What the song's player does before it opens the device alone.
    release();
    assert!(!is_playing());
    s.follow_audition();
    assert_eq!(s.browser.playing, None);
    s.follow_audition();
    assert!(!is_playing(), "it does not start again by itself");
    assert_eq!(s.error, None);
}

#[test]
fn without_sound_the_file_shows_as_playing_and_nothing_starts() {
    let (_tmp, top) = place("none");
    let mut s = studio(OutputMode::None, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    s.follow_audition();
    assert_eq!(s.browser.playing.as_deref(), Some(pad.as_str()));
    assert!(!is_playing());
    assert_eq!(s.error, None);
}

#[test]
fn a_file_that_cannot_be_read_says_so_and_stops() {
    let (_tmp, top) = place("broken");
    let bad = top.join("broken.wav");
    std::fs::write(&bad, b"RIFF....WAVE").unwrap();
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    s.browser.select_file(&bad.to_string_lossy());
    s.follow_audition();
    until(&mut s, "it says it cannot be read", |s| {
        s.browser.playing.is_none()
    });
    assert!(s.error.is_some());
}

#[test]
fn pressing_the_row_hands_the_file_over_without_waiting_for_the_look() {
    let (_tmp, top) = place("press");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    // What a press does after the browser takes it (`Studio::press`, `Hit::Browser`).
    s.follow_audition();
    let n = current().expect("the player is made by the press");
    assert_eq!(n.path.as_deref(), Some(Path::new(&pad)));
    assert!(matches!(n.phase, Phase::Loading | Phase::Playing));
}

#[test]
fn peaks_go_in_the_songs_folder_for_its_files_and_elsewhere_otherwise() {
    let mut b = Browser::new();
    let song = std::env::temp_dir().join("xirius-ui-audition-song");
    b.song_dir = Some(song.clone());
    assert_eq!(
        store_for(&b, &song.join("Audio/a.wav")),
        peaks::store_dir(Some(&song))
    );
    let other = std::env::temp_dir().join("xirius-ui-audition-peaks");
    b.peak_store = Some(other.clone());
    assert_eq!(store_for(&b, Path::new("D:/elsewhere/b.wav")), other);
}

#[test]
fn exclusive_preview_uses_the_prepared_song_output() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use xirius_session::output::{Drivers, Output, SilentOutput};
    let (_tmp, top) = place("exclusive");
    let opens = Arc::new(AtomicUsize::new(0));
    let count = opens.clone();
    let mut s = studio(OutputMode::Device, &top);
    let mut drivers = Drivers::system();
    drivers.exclusive_out = Some(Arc::new(move |audio, _, _| {
        count.fetch_add(1, SeqCst);
        Ok(Output::Silent(SilentOutput::start(audio, true)))
    }));
    drivers.shared_out = Arc::new(|_, _, _| Err("device is held by the song".into()));
    s.transport.set_drivers(drivers);
    s.prepare_song();
    while s.transport.preparing() {
        s.poll_preparing();
    }
    assert!(s.transport.is_live());
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    s.follow_audition();
    until(&mut s, "preview is heard on the song output", |s| {
        assert!(s.error.is_none(), "{:?}", s.error);
        progress(&pad).is_some_and(|p| p > 0.1)
    });
    assert!(!s.transport.playing);
    assert_eq!(opens.load(SeqCst), 1);
    let at = Instant::now();
    let p = s.project();
    s.transport.play(&p, None, 0).unwrap();
    eprintln!("prepared song play with mixed preview: {:?}", at.elapsed());
    assert_eq!(opens.load(SeqCst), 1, "play reuses the same output");
    s.transport.pause();
    close();
}

#[test]
fn review_song_play_and_record_stop_preview_sound_and_display() {
    for mixed in [false, true] {
        for record in [false, true] {
            close();
            let (_tmp, top) = place("song-start");
            let mut s = studio(OutputMode::Device, &top);
            let driver = if mixed {
                "wasapi-exclusive"
            } else {
                "wasapi-shared"
            };
            s.transport
                .io_prefs
                .set("audio.driver", &serde_json::json!(driver))
                .unwrap();
            s.song
                .settings()
                .lock()
                .unwrap()
                .prefs
                .set("audio.driver", &serde_json::json!(driver))
                .unwrap();
            let mut drivers = xirius_session::output::Drivers::system();
            drivers.devices = Arc::new(Vec::new);
            drivers.shared_out = Arc::new(|audio, _, _| {
                Ok(xirius_session::output::Output::Silent(
                    xirius_session::output::SilentOutput::start(audio, true),
                ))
            });
            drivers.exclusive_out = Some(Arc::new(|audio, _, _| {
                Ok(xirius_session::output::Output::Silent(
                    xirius_session::output::SilentOutput::start(audio, true),
                ))
            }));
            drivers.exclusive_in = None;
            drivers.shared_in = Arc::new(|rate, _, _| {
                Ok(xirius_session::output::DeviceInput::new(
                    "test input".into(),
                    false,
                    Arc::new(std::sync::atomic::AtomicU64::new(0)),
                    Box::new(xirius_engine::record::MockInput::sine(
                        f64::from(rate),
                        0.0,
                        0.0,
                    )),
                ))
            });
            s.transport.set_drivers(drivers);
            s.song.save_as(&top.parent().unwrap().join("song")).unwrap();
            s.prepare_song();
            while s.transport.preparing() {
                s.poll_preparing();
            }
            if record {
                let vocal = s
                    .project()
                    .tracks
                    .values()
                    .find(|t| t.name == "Vocal")
                    .unwrap()
                    .id;
                s.song.toggle_arm(vocal).unwrap();
            }
            let key = top.join("pad_44k.wav").to_string_lossy().into_owned();
            s.browser.select_file(&key);
            s.follow_audition();
            until(&mut s, "preview started", |_| {
                progress(&key).is_some_and(|p| p > 0.0)
            });
            if record {
                s.toggle_record();
            } else {
                s.toggle_play();
            }
            assert!(
                s.browser.playing.is_none(),
                "play/record must clear the stop button"
            );
            assert!(
                !is_playing(),
                "play/record must invalidate the sound request"
            );
            assert_eq!(progress(&key), None);
            let end = Instant::now() + GUARD;
            while !s.transport.playing {
                assert!(Instant::now() < end, "{:?}", s.error);
                s.tick();
                std::thread::yield_now();
            }
            if record {
                s.stop_transport();
            } else {
                s.transport.pause();
            }
            close();
        }
    }
}

#[test]
fn review_replacing_or_losing_an_engine_cancels_its_preview_port() {
    for how in ["reset", "export", "lost", "settings"] {
        close();
        let (_tmp, top) = place("lifetime");
        let mut s = studio(OutputMode::Device, &top);
        let mut drivers = xirius_session::output::Drivers::system();
        drivers.exclusive_out = Some(Arc::new(|audio, _, _| {
            Ok(xirius_session::output::Output::Silent(
                xirius_session::output::SilentOutput::start(audio, true),
            ))
        }));
        drivers.shared_out = Arc::new(|_, _, _| Err("test refuses physical output".into()));
        s.transport.set_drivers(drivers);
        s.prepare_song();
        while s.transport.preparing() {
            s.poll_preparing();
        }
        let key = top.join("pad_44k.wav").to_string_lossy().into_owned();
        s.browser.select_file(&key);
        s.follow_audition();
        until(&mut s, "mixed preview started", |_| {
            progress(&key).is_some_and(|p| p > 0.0)
        });
        let old = s.transport.audition_port().unwrap();
        match how {
            "reset" => s.transport.reset(),
            "export" => s.transport.suspend_for_export(),
            "lost" => {
                // A silent stream exposes its device-lost signal without real hardware.
                s.transport.review_lose_output();
                assert!(s.transport.take_lost().is_some());
            }
            _ => {
                let mut prefs = s.transport.io_prefs.clone();
                prefs.set("audio.buffer", &serde_json::json!(1024)).unwrap();
                let p = s.project();
                s.transport.configure_io(&p, None, prefs).unwrap();
            }
        }
        assert!(!old.alive());
        assert_eq!(old.feed(99, &[1.0], &[1.0]), 0);
        s.follow_audition();
        assert!(s.browser.playing.is_none(), "{how} retained a dead Port");
        assert!(!is_playing());
        assert!(s.error.is_none(), "{how}: {:?}", s.error);
        if how == "reset" {
            s.prepare_song();
        }
        if how == "lost" {
            s.transport.review_reopen_now();
        }
        if how != "export" {
            let end = Instant::now() + GUARD;
            while !s.transport.is_live() {
                assert!(Instant::now() < end);
                s.poll_preparing();
                s.poll_device();
                std::thread::yield_now();
            }
            s.follow_audition();
            assert!(s.browser.playing.is_none());
            assert!(s.error.is_none(), "{how}: {:?}", s.error);
        }
        close();
    }
}

struct ReviewStream {
    out: Option<xirius_session::output::SilentOutput>,
    held: Arc<std::sync::atomic::AtomicUsize>,
    close: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
}
struct ReviewRelease(Option<std::sync::mpsc::Sender<()>>);
impl Drop for ReviewRelease {
    fn drop(&mut self) {
        if let Some(go) = self.0.take() {
            let _ = go.send(());
        }
    }
}
impl xirius_session::output::SharedStream for ReviewStream {
    fn name(&self) -> String {
        "test shared stream".into()
    }
    fn lost(&self) -> xirius_engine::device::Lost {
        self.out.as_ref().unwrap().lost()
    }
    fn stop(mut self: Box<Self>) -> Option<xirius_engine::AudioEngine> {
        if let Some((began, go)) = self.close.take() {
            began.send(()).unwrap();
            go.recv_timeout(Duration::from_secs(20)).unwrap();
        }
        self.out.take();
        self.held.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        None
    }
}

#[test]
fn review_shared_preview_releases_before_exclusive_output_opens() {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use xirius_session::output::{DeviceOutput, Drivers, Output, Refused, SilentOutput};
    for ended in [false, true] {
        close();
        let (_tmp, top) = place("shared-release");
        let held = Arc::new(AtomicUsize::new(0));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (began, closing) = std::sync::mpsc::channel();
        let (go, proceed) = std::sync::mpsc::channel();
        let close_gate = Arc::new(Mutex::new(Some((began, proceed))));
        let mut release_close = ReviewRelease(Some(go));
        let mut s = studio(OutputMode::Device, &top);
        s.transport
            .io_prefs
            .set("audio.driver", &serde_json::json!("wasapi-shared"))
            .unwrap();
        let mut drivers = Drivers::system();
        let h = held.clone();
        let close_gate = close_gate.clone();
        drivers.shared_out = Arc::new(move |audio, _, _| {
            let h = h.clone();
            let close_gate = close_gate.clone();
            DeviceOutput::open_with(audio, move |audio| {
                let previous = h.fetch_add(1, SeqCst);
                let close = if previous == 1 {
                    close_gate.lock().unwrap().take()
                } else {
                    None
                };
                Ok(Box::new(ReviewStream {
                    out: Some(SilentOutput::start(audio, true)),
                    held: h,
                    close,
                }))
            })
            .map(Output::Device)
        });
        let h = held.clone();
        let accepted_out = accepted.clone();
        drivers.exclusive_out = Some(Arc::new(move |audio, _, _| {
            if h.load(SeqCst) != 0 {
                return Err(Refused {
                    error: "held by shared preview".into(),
                    audio: Some(Box::new(audio)),
                });
            }
            accepted_out.fetch_add(1, SeqCst);
            Ok(Output::Silent(SilentOutput::start(audio, true)))
        }));
        s.transport.set_drivers(drivers);
        s.prepare_song();
        while s.transport.preparing() {
            s.poll_preparing();
        }
        let key = top.join("Drums/kick_01.wav").to_string_lossy().into_owned();
        s.browser.select_file(&key);
        s.follow_audition();
        until(&mut s, "shared preview owns a stream", |_| {
            held.load(SeqCst) == 2
        });
        if ended {
            until(&mut s, "preview ended", |s| s.browser.playing.is_none());
        } else {
            s.browser.playing = None;
            s.follow_audition();
        }
        let mut prefs = s.transport.io_prefs.clone();
        prefs
            .set("audio.driver", &serde_json::json!("wasapi-exclusive"))
            .unwrap();
        let p = s.project();
        s.transport.configure_io(&p, None, prefs).unwrap();
        closing.recv_timeout(GUARD).unwrap();
        assert!(
            !s.transport.is_live(),
            "the new engine must wait for output close acknowledgement"
        );
        for _ in 0..20 {
            s.poll_device();
            assert_eq!(accepted.load(SeqCst), 0);
        }
        release_close.0.take().unwrap().send(()).unwrap();
        let end = Instant::now() + GUARD;
        while !s.transport.is_live() {
            assert!(Instant::now() < end);
            s.poll_device();
            std::thread::yield_now();
        }
        assert_eq!(
            accepted.load(SeqCst),
            1,
            "the shared audition prevented exclusive open"
        );
        assert!(
            s.transport.take_driver_notice().is_none(),
            "must not fall back while closing preview"
        );
        close();
        s.transport.reset();
    }
}
