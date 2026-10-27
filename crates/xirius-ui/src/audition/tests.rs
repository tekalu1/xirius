//! The audition plays the chosen file on a player of its own, silently in a test (BRW-007,
//! BRW-020..BRW-023). The player reads the file on its own threads, so a test turns the
//! screen's look until what it waits for has come.

use std::sync::{Condvar, Mutex};

use super::*;
use crate::browser::RailTab;
use xirius_session::audition::{Disk, Limits, Reader};
use xirius_session::peaks::Stamp;

#[test]
fn review_mouse_selection_clears_the_old_folder_key_position() {
    let (_tmp, top) = place("heading");
    let mut s = studio(OutputMode::None, &top);
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    s.input_map.browser_focus = true;
    let g = s.geometry(1440.0, 860.0);
    assert!(s.mapped_key(&g, "down", None, crate::tools::Mods::NONE));
    assert!(s.browser.pool_heading.is_some());
    s.browser
        .select_file(&top.join("pad_44k.wav").to_string_lossy());
    assert!(s.browser.pool_heading.is_none());
    assert!(s.mapped_key(&g, "up", None, crate::tools::Mods::NONE));
    assert_eq!(
        s.browser.selected_file.as_deref(),
        Some(top.join("idea.mid").to_str().unwrap())
    );
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
    close: Option<ReviewClose>,
}
type ReviewClose = (
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<()>,
    std::sync::mpsc::Sender<()>,
);
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
        let closed = self.close.take().map(|(began, go, closed)| {
            began.send(()).unwrap();
            go.recv_timeout(Duration::from_secs(20)).unwrap();
            closed
        });
        self.out.take();
        self.held.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if let Some(closed) = closed {
            let _ = closed.send(());
        }
        None
    }
}

#[test]
fn review_shared_preview_releases_before_exclusive_output_opens() {
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    use xirius_session::output::{DeviceOutput, Drivers, Output, Refused, SilentOutput};
    for (ended, windowed, preview_first) in
        [(false, false), (true, false), (false, true), (true, true)]
            .into_iter()
            .flat_map(|(ended, windowed)| [false, true].map(|first| (ended, windowed, first)))
    {
        close();
        let (_tmp, top) = place("shared-release");
        let held = Arc::new(AtomicUsize::new(0));
        let accepted = Arc::new(AtomicUsize::new(0));
        let (began, closing) = std::sync::mpsc::channel();
        let (go, proceed) = std::sync::mpsc::channel();
        let (closed, preview_closed) = std::sync::mpsc::channel();
        let (song_began, song_closing) = std::sync::mpsc::channel();
        let (song_go, song_proceed) = std::sync::mpsc::channel();
        let (song_closed_tx, song_closed) = std::sync::mpsc::channel();
        let close_gate = Arc::new(Mutex::new(std::collections::VecDeque::from([
            (song_began, song_proceed, song_closed_tx),
            (began, proceed, closed),
        ])));
        let mut release_close = ReviewRelease(Some(go));
        let mut release_song = ReviewRelease(Some(song_go));
        let attempted = Arc::new(AtomicUsize::new(0));
        let mut s = studio(OutputMode::Device, &top);
        if windowed {
            s.transport.bind_window();
        }
        s.transport
            .io_prefs
            .set("audio.driver", &serde_json::json!("wasapi-shared"))
            .unwrap();
        let mut drivers = Drivers::system();
        drivers.devices = Arc::new(Vec::new);
        let h = held.clone();
        let close_gate = close_gate.clone();
        drivers.shared_out = Arc::new(move |audio, _, _| {
            let h = h.clone();
            let Some(close) = close_gate.lock().unwrap().pop_front() else {
                return Err("unexpected shared fallback".into());
            };
            DeviceOutput::open_with(audio, move |audio| {
                h.fetch_add(1, SeqCst);
                Ok(Box::new(ReviewStream {
                    out: Some(SilentOutput::start(audio, true)),
                    held: h,
                    close: Some(close),
                }))
            })
            .map(Output::Device)
        });
        let h = held.clone();
        let accepted_out = accepted.clone();
        let attempts = attempted.clone();
        drivers.exclusive_out = Some(Arc::new(move |audio, _, _| {
            attempts.fetch_add(1, SeqCst);
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
        song_closing.recv_timeout(GUARD).unwrap();
        assert!(
            !s.transport.is_live(),
            "the new engine must wait for output close acknowledgement"
        );
        for _ in 0..20 {
            s.poll_preparing();
            s.poll_device();
            assert_eq!(accepted.load(SeqCst), 0);
        }
        // Release only one output. The other is still held by an actual close gate.
        if preview_first {
            release_close.0.take().unwrap().send(()).unwrap();
            preview_closed.recv_timeout(GUARD).unwrap();
            let end = Instant::now() + GUARD;
            while !release_output_ready() {
                assert!(Instant::now() < end, "preview acknowledgement watchdog");
                std::thread::yield_now();
            }
        } else {
            release_song.0.take().unwrap().send(()).unwrap();
            song_closed.recv_timeout(GUARD).unwrap();
        }
        for _ in 0..20 {
            s.poll_preparing();
            s.poll_device();
            assert_eq!(
                attempted.load(SeqCst),
                0,
                "opened before both outputs closed: ended={ended}, windowed={windowed}, preview_first={preview_first}"
            );
        }
        if preview_first {
            release_song.0.take().unwrap().send(()).unwrap();
        } else {
            release_close.0.take().unwrap().send(()).unwrap();
        }
        let end = Instant::now() + GUARD;
        while !s.transport.is_live() {
            assert!(Instant::now() < end);
            s.poll_preparing();
            s.poll_device();
            std::thread::yield_now();
        }
        assert_eq!(
            accepted.load(SeqCst),
            1,
            "the shared audition prevented exclusive open: ended={ended}, windowed={windowed}, preview_first={preview_first}"
        );
        assert_eq!(
            attempted.load(SeqCst),
            1,
            "one exclusive attempt after both close acknowledgements"
        );
        assert!(
            s.transport.take_driver_notice().is_none(),
            "must not fall back while closing preview"
        );
        close();
        s.transport.reset();
    }
}

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
    // Device preparation reports missing sample-song files instead of silently
    // substituting them. These tests exercise preview ownership with valid audio.
    let mut s = if mode == OutputMode::Device {
        let mut song = crate::song::Song::empty();
        song.run("track.add.audio", serde_json::json!({"name":"Vocal"}))
            .unwrap();
        Studio::new(song)
    } else {
        Studio::sample()
    };
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
        crate::browser_files::poll();
        s.follow_audition();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn kept() -> Vec<PathBuf> {
    PLAYER.with(|p| p.borrow().as_ref().map(|a| a.kept().0).unwrap_or_default())
}

/// The disk, where a file opens only once the test lets it: what is asked for waits as long
/// as the test needs.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    told: Condvar,
}

impl Gate {
    /// Makes this test's player read through the gate.
    fn player() -> Arc<Gate> {
        let gate = Arc::new(Gate::default());
        let reader: Arc<dyn Reader> = gate.clone();
        PLAYER.with(|p| *p.borrow_mut() = Some(Audition::with(reader, Limits::default())));
        gate
    }

    fn let_go(&self) {
        *self.open.lock().unwrap() = true;
        self.told.notify_all();
    }
}

impl Reader for Gate {
    fn stat(&self, path: &Path) -> Result<Stamp, String> {
        Disk.stat(path)
    }

    fn open(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>, String> {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.told.wait(open).unwrap();
        }
        drop(open);
        Disk.open(path)
    }
}

/// Lets the gate go when the test ends, also when it fails, so no reader waits on.
struct LetGo(Arc<Gate>);

impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.let_go();
    }
}

#[test]
fn a_chosen_file_plays_to_its_end_and_lets_go() {
    let (_tmp, top) = place("ends");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let kick = top.join("Drums/kick_01.wav").to_string_lossy().into_owned();
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
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
fn the_rows_next_to_the_chosen_one_are_read_ahead() {
    let dir = tempfile::Builder::new()
        .prefix("xirius-ui-audition-ahead-")
        .tempdir()
        .unwrap();
    let top = dir.path().join("並び");
    let tone: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    let names = ["a", "b", "c", "d", "e", "f", "g"];
    for n in names {
        xirius_session::export::write_wav(&top.join(format!("{n}.wav")), &tone, &tone, 48_000, 16)
            .unwrap();
    }
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    let key = |n: &str| top.join(format!("{n}.wav"));
    s.browser.select_file(&key("c").to_string_lossy());
    s.follow_audition();
    // Three below and one above, besides the chosen one.
    let want = ["b", "c", "d", "e", "f"].map(key);
    until(&mut s, "the rows next to it are kept", |_| {
        let k = kept();
        want.iter().all(|p| k.contains(p))
    });
    let k = kept();
    assert!(!k.contains(&key("a")), "{k:?}");
    assert!(!k.contains(&key("g")), "{k:?}");
}

/// What the tip over the audition's button says.
fn tip(s: &Studio) -> Option<String> {
    crate::hover_tip::text(
        s,
        crate::studio::Hit::Browser(crate::browser::BrowserHit::Audition),
    )
    .map(|t| t.to_string())
}

/// The screen reader's button over the audition's.
fn mark(s: &Studio) -> Option<(String, crate::a11y::Mark)> {
    let g = s.geometry(1440.0, 860.0);
    crate::a11y::marks(s, &g)
        .into_iter()
        .find(|m| m.key == "audition")
        .map(|m| (m.node.label().unwrap_or_default().to_string(), m))
}

#[test]
fn the_stop_button_says_the_file_is_read_while_nothing_is_heard() {
    let (_tmp, top) = place("wait");
    let gate = LetGo(Gate::player());
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    s.browser.select_file(&pad);
    s.follow_audition();
    let asked = current().and_then(|n| n.asked).expect("asked for");
    let early = waiting(&pad);
    if asked.elapsed() < SPIN_AFTER {
        assert!(!early, "not before {SPIN_AFTER:?}");
    }
    // The file does not open until the gate lets it: nothing is heard.
    std::thread::sleep(SPIN_AFTER + Duration::from_millis(50));
    s.follow_audition();
    assert!(waiting(&pad));
    assert_eq!(tip(&s).as_deref(), Some(READING));
    assert_eq!(mark(&s).map(|m| m.0).as_deref(), Some(READING));
    gate.0.let_go();
    until(&mut s, "it plays", |_| {
        current().is_some_and(|n| n.phase == Phase::Playing)
    });
    assert!(!waiting(&pad));
    assert_ne!(tip(&s).as_deref(), Some(READING));
    assert_eq!(mark(&s).map(|m| m.0).as_deref(), Some(STOP_LABEL));
}

#[test]
fn a_screen_reader_has_the_audition_button_and_its_press_plays_and_stops() {
    let (_tmp, top) = place("a11y");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    assert!(mark(&s).is_none(), "no button before a file is chosen");
    s.browser.select_file(&pad);
    s.follow_audition();
    until(&mut s, "it plays", |_| {
        current().is_some_and(|n| n.phase == Phase::Playing)
    });
    let (label, m) = mark(&s).expect("a button over the audition's");
    assert_eq!(label, STOP_LABEL);
    assert_eq!(m.node.role(), gpui::accesskit::Role::Button);
    // It lies over the button the pointer presses.
    let g = s.geometry(1440.0, 860.0);
    let l = s.browser.layout(g.panel);
    let (x, y) = s
        .browser
        .center_of(&l, crate::browser::BrowserHit::Audition)
        .expect("on screen");
    let b = m.node.bounds().expect("bounds");
    let y = (y + xirius_design::Layout::TOP_BAR_H) as f64;
    assert!(b.x0 <= x as f64 && (x as f64) <= b.x1 && b.y0 <= y && y <= b.y1);
    // Its press does what the pointer's does: it stops, and a second plays again.
    let hit = m.press.expect("a press");
    crate::a11y::press(&mut s, &g, hit);
    s.follow_audition();
    assert!(!is_playing());
    assert_eq!(mark(&s).map(|m| m.0).as_deref(), Some(PLAY_LABEL));
    crate::a11y::press(&mut s, &g, hit);
    s.follow_audition();
    assert!(is_playing());
}

#[test]
fn reduced_motion_keeps_the_dots_and_spinner_still_on_the_same_frame_path() {
    let motion = xirius_design::Motion::new(true);
    for i in 0..100 {
        assert_eq!(
            breath(i, 1_792_800_000_480, motion),
            breath(i, 1_792_800_000_980, motion)
        );
    }
    let (_tmp, top) = place("still");
    let _gate = LetGo(Gate::player());
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    s.ai.motion = motion;
    s.browser
        .select_file(&top.join("pad_44k.wav").to_string_lossy());
    for _ in 0..20 {
        assert!(s.audition_frame(), "loading uses the usual frame path");
        assert_eq!(s.browser.spin, 0.0);
    }
    assert_eq!(s.spin(), 0.0);
    let at = Instant::now();
    assert_eq!(grown(Some(at), at, motion), (1.0, 1.0));
    assert_eq!(grown(Some(at), at + GROW, motion), (1.0, 1.0));
}

#[test]
fn bars_come_in_as_the_file_is_read_and_grow() {
    let at = Instant::now();
    let path = PathBuf::from("x.wav");
    let peaks = |frames: usize| {
        let x = vec![0.5f32; frames];
        Arc::new(Peaks::from_samples(48_000, &x, &x))
    };
    let now = |covered: usize, whole: u64| Now {
        path: Some(path.clone()),
        phase: Phase::Loading,
        peaks: Some(peaks(covered)),
        covered: covered as u64,
        whole: Some(whole),
        ..Now::default()
    };
    let mut s = Shown::default();
    assert_eq!(
        s.take("x.wav", None, 10, at),
        0,
        "dots until a part is read"
    );
    assert_eq!(s.take("x.wav", Some(&now(2_400, 9_600)), 10, at), 2);
    assert_eq!(s.appear[0], Some(at));
    assert_eq!(s.appear[1], Some(at + GROW_STEP));
    assert_eq!(s.take("x.wav", Some(&now(9_600, 9_600)), 10, at), 10);
    assert_eq!(
        s.appear[2],
        Some(at),
        "the next ones start from when they came in"
    );
    assert_eq!(
        grown(s.appear[0], at, xirius_design::Motion::default()),
        (0.12, 0.35)
    );
    assert_eq!(
        grown(s.appear[0], at + GROW, xirius_design::Motion::default()),
        (1.0, 1.0)
    );
    // Another file starts over.
    assert_eq!(s.take("y.wav", None, 10, at), 0);
}

#[test]
fn a_new_press_of_the_same_file_starts_the_bars_over() {
    let at = Instant::now();
    let peaks = |frames: usize| {
        let x = vec![0.5f32; frames];
        Arc::new(Peaks::from_samples(48_000, &x, &x))
    };
    let now = |ask: u64, covered: usize| Now {
        ask,
        path: Some(PathBuf::from("x.wav")),
        phase: Phase::Loading,
        peaks: Some(peaks(covered)),
        covered: covered as u64,
        whole: Some(9_600),
        ..Now::default()
    };
    let mut s = Shown::default();
    assert_eq!(s.take("x.wav", Some(&now(1, 9_600)), 10, at), 10);
    // Pressed again: what the first press read does not stay, though it covered more.
    assert_eq!(s.take("x.wav", Some(&now(2, 2_400)), 10, at), 2);
    assert_eq!(s.drawn, 2);
    assert!(s.appear[2..].iter().all(Option::is_none));
    // Stopped, the row keeps the bars of the last press.
    assert_eq!(s.take("x.wav", None, 10, at), 2);
    // Pressed again, from what was kept: the bars come in whole, as at a first look.
    assert_eq!(s.take("x.wav", Some(&now(3, 9_600)), 10, at), 10);
    assert!(s.appear.iter().all(Option::is_none), "{:?}", s.appear);
}

#[test]
fn bars_of_a_file_kept_or_with_motion_reduced_come_in_whole() {
    let at = Instant::now();
    let whole = Now {
        path: Some(PathBuf::from("x.wav")),
        phase: Phase::Playing,
        peaks: Some(Arc::new(Peaks::from_samples(
            48_000,
            &[0.5; 4_800],
            &[0.5; 4_800],
        ))),
        covered: 4_800,
        whole: Some(4_800),
        ..Now::default()
    };
    let mut s = Shown::default();
    assert_eq!(s.take("x.wav", Some(&whole), 8, at), 8);
    assert!(s.appear.iter().all(Option::is_none));
    let part = Now {
        whole: Some(9_600),
        path: Some(PathBuf::from("y.wav")),
        ..whole
    };
    let mut s = Shown::default();
    assert_eq!(s.take("y.wav", Some(&part), 8, at), 4);
    assert_eq!(
        grown(s.appear[0], at, xirius_design::Motion::new(true)),
        (1.0, 1.0)
    );
}

#[test]
fn the_dots_breathe_from_left_to_right() {
    let m = xirius_design::Motion::default();
    assert!((breath(0, 0, m) - xirius_design::WaveColor::DOT_MIN).abs() < 1e-3);
    assert!((breath(0, 480, m) - xirius_design::WaveColor::DOT_MAX).abs() < 1e-3);
    assert!(
        (breath(1, 480 + BREATHE_STEP_MS as u64, m) - xirius_design::WaveColor::DOT_MAX).abs()
            < 1e-3
    );
    assert!((breath(0, BREATHE_MS as u64, m) - xirius_design::WaveColor::DOT_MIN).abs() < 1e-3);
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
fn dots_keep_their_phase_at_unix_milliseconds() {
    let ms = 1_792_800_000_480u64;
    assert_ne!(
        breath(0, ms, xirius_design::Motion::default()),
        breath(10, ms, xirius_design::Motion::default())
    );
    assert_ne!(
        breath(0, ms, xirius_design::Motion::default()),
        breath(0, ms + 100, xirius_design::Motion::default())
    );
}

#[test]
fn file_arrow_keys_select_and_hand_the_audio_to_the_player() {
    let (_tmp, top) = place("arrow");
    std::fs::copy(top.join("pad_44k.wav"), top.join("z.wav")).unwrap();
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    s.browser.add_place_fixture(&top).unwrap();
    crate::browser_files::settle_rows(&s.browser);
    s.input_map.browser_focus = true;
    let pad = top.join("pad_44k.wav").to_string_lossy().into_owned();
    s.browser.select_file(&pad);
    let g = s.geometry(1440.0, 860.0);
    assert!(s.mapped_key(&g, "down", None, crate::tools::Mods::NONE));
    let key = s.browser.selected_file.clone().unwrap();
    assert_ne!(key, pad);
    assert_eq!(s.browser.playing.as_deref(), Some(key.as_str()));
    assert_eq!(current().unwrap().path.as_deref(), Some(Path::new(&key)));
    assert!(s.mapped_key(&g, "up", None, crate::tools::Mods::NONE));
    assert_eq!(s.browser.selected_file.as_deref(), Some(pad.as_str()));
    assert_eq!(current().unwrap().path.as_deref(), Some(Path::new(&pad)));
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
    assert!(s.transport.is_live(), "{:?}", s.preparing);
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
fn pool_arrow_keys_sound_the_selected_file_and_read_ahead() {
    let (_tmp, top) = place("pool-arrow");
    let mut s = studio(OutputMode::Silent { paced: true }, &top);
    s.browser.tab = RailTab::SongFiles;
    s.input_map.browser_focus = true;
    s.browser.pool = ["kick_01.wav", "kick_02.wav"]
        .iter()
        .map(|name| xirius_session::file_ops::Entry {
            file: name.to_string(),
            path: top.join("Drums").join(name),
            kind: "import",
            bytes: 0,
            exists: true,
            external: false,
            clips: 1,
            frozen: 0,
            devices: 0,
        })
        .collect();
    let g = s.geometry(1440.0, 860.0);
    assert!(s.mapped_key(&g, "down", None, crate::tools::Mods::NONE));
    assert!(s.mapped_key(&g, "down", None, crate::tools::Mods::NONE));
    let first = s.browser.pool[0].path.clone();
    let second = s.browser.pool[1].path.clone();
    until(
        &mut s,
        "key selection sounds and the next file is kept",
        |_| progress(&first.to_string_lossy()).is_some_and(|p| p > 0.0) && kept().contains(&second),
    );
    assert!(s.mapped_key(&g, "down", None, crate::tools::Mods::NONE));
    assert_eq!(current().unwrap().path.as_deref(), Some(second.as_path()));
    until(&mut s, "the next file plays from the cache", |_| {
        current().is_some_and(|n| n.kept && n.phase == Phase::Playing)
    });
    close();
}
