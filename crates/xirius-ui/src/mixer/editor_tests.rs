//! Real plugin windows: an effect from its insert row and an instrument from the keys
//! button and mixer. Each opens in a plugin host, off every screen, and draws content.
//! State capture of host editor changes is covered by the session's recapture tests.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use xirius_plugins::{Catalog, Entry};
use xirius_session::output::OutputMode;

use super::*;
use crate::instrument::{Pick, current};
use crate::popup::Action;

const W: f32 = 1600.0;
const H: f32 = 1000.0;

/// The effect the tests open: free, and found on many PCs.
const EFFECT: &str = "Kilohearts/kHs Gain.vst3";
/// The instruments, the first found: free (sforzando's licence lets it be loaded; Vital's
/// program is only run, not shipped).
const INSTRUMENTS: [&str; 2] = ["sforzando.vst3", "Vital.vst3"];

/// One test at a time opens a plugin's windows: a plugin may draw every window of the process
/// from one thread (kHs Gain does), as the app opens them, and a window another test's
/// thread opened at the same time may never be drawn.
static WINDOWS: Mutex<()> = Mutex::new(());

struct WindowGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<std::ffi::OsString>,
}

impl Drop for WindowGuard {
    fn drop(&mut self) {
        // SAFETY: the real-window tests serialize changes to this test-only setting.
        unsafe {
            if let Some(previous) = &self.previous {
                std::env::set_var(xirius_vst3::OFFSCREEN_ENV, previous);
            } else {
                std::env::remove_var(xirius_vst3::OFFSCREEN_ENV);
            }
        }
    }
}

fn one_at_a_time() -> WindowGuard {
    keep_com();
    let lock = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os(xirius_vst3::OFFSCREEN_ENV);
    // SAFETY: these tests hold WINDOWS while any host window they start is open.
    unsafe { std::env::set_var(xirius_vst3::OFFSCREEN_ENV, "1") };
    WindowGuard {
        _lock: lock,
        previous,
    }
}

/// Keeps COM up in the test process until it ends, as the app's UI thread does. A thread
/// that opened a window (OLE) and is the last to leave COM tears COM down for the process,
/// holding COM's lock while it waits for the loader's; kHs Gain starts COM while the loader
/// loads it, holding the loader's lock while it waits for COM's: the two wait for each other
/// and every test that loads or ends a thread waits behind them.
fn keep_com() {
    #[cfg(windows)]
    {
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoIncrementMTAUsage(cookie: *mut isize) -> i32;
        }
        static KEEP: std::sync::Once = std::sync::Once::new();
        KEEP.call_once(|| {
            let mut cookie = 0isize;
            // SAFETY: an out-parameter owned here; the usage is never given back.
            let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
            assert!(hr >= 0, "CoIncrementMTAUsage: {hr:#x}");
        });
    }
}

fn entries(bundle: &str) -> Option<Vec<Entry>> {
    let path: PathBuf = xirius_vst3::default_vst3_dir().join(bundle);
    if !path.exists() {
        eprintln!("skipped: {} is not installed", path.display());
        return None;
    }
    match xirius_plugins::probe(&path) {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("skipped: {} does not open: {e}", path.display());
            None
        }
    }
}

/// The sample song with the mixer open, playing nothing into no speaker, and `entries` as
/// the plugins of this PC.
fn studio(entries: Vec<Entry>) -> (Studio, Geometry) {
    let mut s = Studio::sample();
    s.transport.mode = OutputMode::Silent { paced: true };
    s.use_plugins(Catalog {
        entries,
        failures: Vec::new(),
    });
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
    (s, g)
}

fn track(s: &Studio, name: &str) -> TrackId {
    s.project()
        .tracks
        .values()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .unwrap()
}

fn press(s: &mut Studio, g: &Geometry, hit: MixerHit) -> bool {
    let (x, y) = s.mixer_center(g, hit).expect("on screen");
    assert_eq!(s.hit(g, x, y), Hit::Mixer(hit));
    s.press(g, Hit::Mixer(hit))
}

/// The host's window of `id` is on the instance that plays, off every screen, and draws something.
fn check_window(s: &mut Studio, id: PluginId, what: &str, previous: Option<isize>) -> isize {
    assert_eq!(s.error, None, "{what}");
    assert!(s.mixer.is_open(id), "{what}: the window opened");
    assert_eq!(s.mixer.editor_heard(id), Some(true), "{what}: heard");
    let until = Instant::now() + Duration::from_secs(10);
    let hwnd = loop {
        s.tick();
        // Closing and reopening are queued in the host. The old frame can still
        // match by title while the new request is waiting to be handled.
        if s.host_said_editor_open(id)
            && let Some(hwnd) = s.mixer.editor_hwnd(id)
            && Some(hwnd) != previous
        {
            break hwnd;
        }
        assert!(
            Instant::now() < until,
            "{what}: host window did not open: title={:?}, statuses={:?}, origin={:?}, events={:?}, shown={}, error={:?}",
            s.mixer
                .editors
                .iter()
                .find(|e| e.id == id)
                .map(|e| &e.title),
            s.transport.host().map(|h| h.plugin_statuses()),
            s.transport
                .host()
                .and_then(|h| h.plugins().editor_origin(id)),
            s.transport
                .host()
                .and_then(|h| h.plugins().link(id))
                .map(|l| l.take_editor_open_events()),
            s.editor_shown(id),
            s.error
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert_ne!(hwnd, 0);
    assert!(!crate::park::is_foreground(hwnd), "{what}: not in front");
    let (x, _) = crate::park::origin(hwnd).expect("host window origin");
    let off = xirius_vst3::Placement::offscreen().at.unwrap().0;
    assert!(x >= off, "{what}: off every screen ({x} < {off})");
    let dir = tempfile::Builder::new()
        .prefix("xirius-ui-editor-")
        .tempdir()
        .unwrap();
    let shot = dir.path().join(format!("{}.bmp", what.replace(' ', "-")));
    let c = loop {
        let c = xirius_vst3::capture_window(hwnd, &shot).unwrap();
        if c.distinct_colors > 1 || Instant::now() > until {
            break c;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(c.width > 0 && c.height > 0, "{what}: {c:?}");
    assert!(
        c.distinct_colors > 1,
        "{what}: the host window drew ({c:?})"
    );
    hwnd
}

#[test]
fn a_vst3_effects_window_opens_in_its_host_and_draws() {
    // A plugin may write into the temporary folder (Vital leaves `vital_versions.txt`).
    let _temp = crate::shot::TestTemp::new();
    let _one = one_at_a_time();
    let Some(entries) = entries(EFFECT) else {
        return;
    };
    let (mut s, g) = studio(entries);
    let t = track(&s, "Vocal");
    assert!(press(&mut s, &g, MixerHit::AddInsert(t)));
    let pick = s
        .popup
        .as_ref()
        .unwrap()
        .choices()
        .iter()
        .find(|c| c.label.starts_with("kHs Gain"))
        .map(|c| c.to.clone())
        .expect("kHs Gain in the insert list");
    s.popup_action(&g, Action::Pick(pick));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(s.mixer.is_open(id), "inserting opens the editor");
    let hwnd = check_window(&mut s, id, "effect editor", None);

    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert_eq!(s.mixer.open_editors(), 1);
    assert_eq!(s.mixer.editor_hwnd(id), Some(hwnd));
}

#[test]
fn a_vst3_instruments_window_opens_from_the_keys_button_and_the_mixer() {
    // A plugin may write into the temporary folder (Vital leaves `vital_versions.txt`).
    let _temp = crate::shot::TestTemp::new();
    let _one = one_at_a_time();
    let Some((entries, name)) = INSTRUMENTS
        .iter()
        .find_map(|b| entries(b).map(|e| (e, b.trim_end_matches(".vst3"))))
    else {
        return;
    };
    let entry = entries
        .iter()
        .find(|e| e.kind == PluginKind::Instrument)
        .cloned()
        .expect("an instrument");
    let (mut s, _) = studio(entries);
    let t = track(&s, "Keys");
    assert!(s.set_instrument(
        t,
        &Pick {
            name: entry.name.clone(),
            format: entry.format.clone(),
            uid: entry.uid.clone(),
        }
    ));
    let id = current(&s.project().tracks[&t]).unwrap().id;

    // The keys button at the head of the track, with the drawer shut.
    s.drawer_open = false;
    let g = s.geometry(W, H);
    let (x, y) = s.center_of(&g, Hit::Keys(t)).expect("on screen");
    assert_eq!(s.hit(&g, x, y), Hit::Keys(t));
    assert!(s.press(&g, Hit::Keys(t)));
    let hwnd = check_window(&mut s, id, name, None);
    // Pressed again, it stays open (it comes to the top), as a built-in's window does.
    assert!(s.press(&g, Hit::Keys(t)));
    assert_eq!(s.mixer.editor_hwnd(id), Some(hwnd));

    // The instrument's row in the mixer also brings the same frame forward.
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert!(s.mixer.is_open(id));
    assert_eq!(s.mixer.editor_hwnd(id), Some(hwnd));
}

#[test]
fn a_window_that_cannot_open_says_why() {
    let (mut s, g) = studio(Vec::new());
    let t = track(&s, "Vocal");
    // A plugin the song names that this PC does not have.
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Comp", "format": "vst3",
                "uid": "0123456789ABCDEF0123456789ABCDEF" }),
    ));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert!(!s.mixer.is_open(id));
    assert_eq!(
        s.error.as_deref(),
        Some("Comp がこの PC に見つからないため、窓を開けません")
    );
}

/// With no audio output, opening a window still uses the plugin host.
#[test]
fn without_output_opens_only_a_hosted_editor() {
    // A plugin may write into the temporary folder (Vital leaves `vital_versions.txt`).
    let _temp = crate::shot::TestTemp::new();
    let _one = one_at_a_time();
    let Some(entries) = entries(EFFECT) else {
        return;
    };
    let (mut s, g) = studio(entries.clone());
    s.transport.mode = OutputMode::None;
    let uid = entries[0].uid.clone();
    let t = track(&s, "Vocal");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "kHs Gain", "format": "vst3", "uid": uid }),
    ));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert_ne!(s.mixer.editor_heard(id), Some(false));
    if s.mixer.is_open(id) {
        assert!(s.transport.host().is_some());
        assert_eq!(s.mixer.editor_heard(id), Some(true));
    } else {
        assert!(s.error.is_some());
    }
}

#[cfg(windows)]
#[test]
fn a_slow_hosted_editor_stays_requested_until_its_open_report_arrives() {
    struct Delay;
    impl Drop for Delay {
        fn drop(&mut self) {
            // SAFETY: this test owns the setting until its host has shut down.
            unsafe { std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_OPEN_MS") };
        }
    }
    let _one = one_at_a_time();
    // SAFETY: nextest isolates tests in processes and the fixture host inherits this value.
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_EDITOR_OPEN_MS", "3500") };
    let _delay = Delay;
    let entries = xirius_plugins::probe(&crate::plugin_fixture_shots::fixture_path()).unwrap();
    let (mut s, g) = studio(entries.clone());
    let t = track(&s, "Vocal");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Fixture", "format": "vst3",
                "uid": entries[0].uid }),
    ));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(3100) {
        s.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        s.mixer.is_open(id),
        "the client cancelled a slow editor request"
    );
    assert!(
        !s.host_said_editor_open(id),
        "the fixture did not stay slow"
    );
    let end = Instant::now() + Duration::from_secs(8);
    while !s.host_said_editor_open(id) {
        s.tick();
        assert!(
            Instant::now() < end,
            "the fixture editor did not open: {:?}",
            s.error
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(s.error, None);
    assert!(s.mixer.editor_hwnd(id).is_some(), "fixture editor handle");
}

#[cfg(windows)]
#[test]
fn the_hosted_editor_power_button_keeps_its_window_and_restores_audio() {
    let _temp = crate::shot::TestTemp::new();
    let _one = one_at_a_time();
    let entries = xirius_plugins::probe(&crate::plugin_fixture_shots::fixture_path()).unwrap();
    let (mut s, g) = studio(entries.clone());
    let t = track(&s, "Vocal");
    assert!(s.run_command(
        "plugin.add",
        json!({"track": t.to_string(), "name": "Fixture", "format": "vst3", "uid": entries[0].uid}),
    ));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    let end = Instant::now() + Duration::from_secs(10);
    while !s.host_said_editor_open(id) {
        s.tick();
        assert!(
            Instant::now() < end,
            "fixture editor did not open: {:?}",
            s.error
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let hwnd = s.mixer.editor_hwnd(id).expect("fixture editor frame");
    let click = |s: &Studio, button_id| {
        let mut result = 0;
        let hwnd = s
            .mixer
            .editor_hwnd(id)
            .expect("current fixture editor frame");
        // SAFETY: BM_CLICK follows the native child button path without desktop input.
        let button = unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd as _, button_id)
        };
        assert!(!button.is_null(), "button {button_id} on frame {hwnd}");
        assert_ne!(
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
                    button,
                    windows_sys::Win32::UI::WindowsAndMessaging::BM_CLICK,
                    0,
                    0,
                    windows_sys::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG,
                    5_000,
                    &mut result,
                )
            },
            0
        );
    };
    // A native editor remains clickable while the song dispatcher is held. Its first
    // press must reach the song after the hold ends, rather than disappear in the host queue.
    s.transport.finishing_take = true;
    click(&s, 1);
    let end = Instant::now() + Duration::from_secs(5);
    while s.mixer.editor_pending.is_empty() {
        s.poll_editor_bar();
        assert!(Instant::now() < end, "blocked native action was lost");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(s.project().tracks[&t].inserts[0].runs());
    assert_eq!(s.mixer.editor_pending.len(), 1);
    s.transport.finishing_take = false;
    for (i, expected) in [false, true, false, true].into_iter().enumerate() {
        if i != 0 {
            click(&s, 1);
        }
        let end = Instant::now() + Duration::from_secs(10);
        while s.project().tracks[&t].inserts[0].runs() != expected {
            s.tick();
            assert!(
                Instant::now() < end,
                "power action did not reach the song: {:?}; runs={}; preparing={}; bypass={}",
                s.error,
                s.project().tracks[&t].inserts[0].runs(),
                s.sound_preparing(),
                s.project().tracks[&t].inserts[0].bypass
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(s.editor_shown(id), "power action closed the plugin editor");
        assert_eq!(
            s.mixer.editor_hwnd(id),
            Some(hwnd),
            "power action replaced the editor frame"
        );
    }
    click(&s, 4);
    let end = Instant::now() + Duration::from_secs(5);
    while !s.mixer.editor_pins.contains(&id) {
        s.tick();
        assert!(
            Instant::now() < end,
            "pin action did not reach the window state"
        );
    }
    for name in ["Header A", "Header B"] {
        assert!(s.run_command(
            "preset.save",
            json!({"track":t,"of":id,"kind":"fx","name":name})
        ));
        let end = Instant::now() + Duration::from_secs(5);
        while s.input_blocked() {
            s.tick();
            assert!(Instant::now() < end, "preset save did not settle");
        }
    }
    let end = Instant::now() + Duration::from_secs(5);
    while !s
        .mixer
        .editor_bars
        .get(&id)
        .is_some_and(|b| b.presets.len() >= 3)
    {
        s.tick();
        assert!(
            Instant::now() < end,
            "saved presets did not reach the header"
        );
    }
    assert_eq!(
        s.mixer.editor_hwnd(id),
        Some(hwnd),
        "saving a preset replaced the editor frame"
    );
    assert!(s.run_command(
        "preset.load",
        json!({"track":t,"of":id,"preset":"Header A"})
    ));
    let end = Instant::now() + Duration::from_secs(5);
    while s.project().tracks[&t].inserts[0].preset.as_deref() != Some("Header A") {
        s.tick();
        assert!(Instant::now() < end, "preset load did not settle");
    }
    click(&s, 11);
    let end = Instant::now() + Duration::from_secs(5);
    while s.project().tracks[&t].inserts[0].preset.as_deref() == Some("Header A") {
        s.tick();
        assert!(Instant::now() < end, "next preset did not load");
    }
    click(&s, 8);
    let end = Instant::now() + Duration::from_secs(5);
    while s.project().tracks[&t].inserts[0].preset.as_deref() != Some("Header A") {
        s.tick();
        assert!(Instant::now() < end, "previous preset did not load");
    }
    click(&s, 6);
    let end = Instant::now() + Duration::from_secs(5);
    while s.channel.is_none() {
        s.tick();
        assert!(
            Instant::now() < end,
            "channel action did not open its panel"
        );
    }
    click(&s, 5);
    let end = Instant::now() + Duration::from_secs(5);
    while s.editor_shown(id) {
        s.tick();
        assert!(
            Instant::now() < end,
            "close action did not close the editor"
        );
    }
}

#[cfg(windows)]
#[test]
fn a_hosted_editor_failure_reaches_the_studio_with_its_reason() {
    struct Failure;
    impl Drop for Failure {
        fn drop(&mut self) {
            // SAFETY: this test owns the setting until its host has shut down.
            unsafe { std::env::remove_var("XIRIUS_TEST_VST3_EDITOR_FAIL") };
        }
    }
    let _one = one_at_a_time();
    // SAFETY: nextest isolates tests in processes and the fixture host inherits this value.
    unsafe { std::env::set_var("XIRIUS_TEST_VST3_EDITOR_FAIL", "1") };
    let _failure = Failure;
    let entries = xirius_plugins::probe(&crate::plugin_fixture_shots::fixture_path()).unwrap();
    let (mut s, g) = studio(entries.clone());
    let t = track(&s, "Vocal");
    assert!(s.run_command(
        "plugin.add",
        json!({ "track": t.to_string(), "name": "Fixture", "format": "vst3",
                "uid": entries[0].uid }),
    ));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    let end = Instant::now() + Duration::from_secs(8);
    while s.error.is_none() {
        s.tick();
        assert!(Instant::now() < end, "the host failure was not shown");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!s.mixer.is_open(id));
    assert!(s.error.as_deref().unwrap().contains("attached"));
}
