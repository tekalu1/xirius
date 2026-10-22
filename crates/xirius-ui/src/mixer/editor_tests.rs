//! Real VST3 windows (UI-040, PLG-027): an effect from its insert row, an instrument from the
//! keys button and from the mixer's instrument row. Each opens a window of this process on the
//! instance that plays (off every screen, never in front), which draws something; what is
//! turned in it becomes one step of the song. The plugins are the ones on this PC (kHs Gain;
//! sforzando, else Vital): without them a test says why and passes.

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

fn one_at_a_time() -> MutexGuard<'static, ()> {
    keep_com();
    WINDOWS.lock().unwrap_or_else(|e| e.into_inner())
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

/// The window of `id` is this process's, on the instance that plays, off every screen, not
/// in front, and draws something.
fn check_window(s: &mut Studio, id: PluginId, what: &str) {
    assert_eq!(s.error, None, "{what}");
    assert!(s.mixer.is_open(id), "{what}: the window opened");
    assert_eq!(s.mixer.editor_heard(id), Some(true), "{what}: heard");
    let hwnd = s.mixer.editor_hwnd(id).expect("a window of this process");
    assert_ne!(hwnd, 0);
    let editor = s
        .mixer
        .editors
        .iter()
        .find(|e| e.id == id)
        .and_then(|e| e.editor())
        .unwrap();
    assert!(!editor.took_foreground(), "{what}: not in front");
    let (x, _) = editor.origin();
    let off = xirius_vst3::Placement::offscreen().at.unwrap().0;
    assert!(x >= off, "{what}: off every screen ({x} < {off})");
    let dir = tempfile::Builder::new()
        .prefix("xirius-ui-editor-")
        .tempdir()
        .unwrap();
    let shot = dir.path().join(format!("{}.bmp", what.replace(' ', "-")));
    // The plugin draws at its own pace.
    let t = Instant::now();
    let c = loop {
        s.mixer.pump_editors(Duration::from_millis(250));
        let c = xirius_vst3::capture_window(hwnd, &shot).unwrap();
        if c.distinct_colors > 1 || t.elapsed() > Duration::from_secs(10) {
            break c;
        }
    };
    assert!(c.width > 0 && c.height > 0, "{what}: {c:?}");
    assert!(c.distinct_colors > 1, "{what}: the window drew ({c:?})");
}

/// Polls the windows until the song takes a change, or `wait` goes by.
fn poll_until_taken(s: &mut Studio, steps: usize, wait: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < wait {
        s.mixer.pump_editors(Duration::from_millis(30));
        s.poll_editors();
        if s.song.steps() > steps {
            return true;
        }
    }
    false
}

#[test]
fn a_vst3_effects_window_opens_on_the_instance_that_plays_and_its_knob_is_one_step() {
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
        .choices
        .iter()
        .find(|c| c.label.starts_with("kHs Gain"))
        .map(|c| c.to.clone())
        .expect("kHs Gain in the insert list");
    s.popup_action(&g, Action::Pick(pick));
    let id = s.project().tracks[&t].inserts[0].id;
    assert!(s.mixer.is_open(id), "inserting opens the editor");
    check_window(&mut s, id, "kHs Gain");

    // A knob turned in the window: the instance that plays turns it, the song takes it as one
    // step once let go, and undo puts it back.
    let before = s.project().tracks[&t].inserts[0].clone();
    let steps = s.song.steps();
    {
        let host = s.transport.host().unwrap();
        let (plugin, _) = host.plugins().local(id).expect("the instance that plays");
        let param = plugin
            .params()
            .into_iter()
            .find(|p| p.step_count == 0)
            .expect("a knob");
        let to = if param.default > 0.5 { 0.1 } else { 0.9 };
        plugin.edit_like_editor(param.id, &[(param.default + to) / 2.0, to]);
    }
    assert!(
        poll_until_taken(&mut s, steps, Duration::from_secs(5)),
        "the song took the knob"
    );
    assert_eq!(s.song.steps(), steps + 1, "one step");
    let after = s.project().tracks[&t].inserts[0].clone();
    assert_ne!(after.state, before.state, "the plugin's state moved");
    assert!(s.song.undo());
    assert_eq!(s.project().tracks[&t].inserts[0].state, before.state);

    // Pressing the row again shuts it.
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert_eq!(s.mixer.open_editors(), 0);
    assert_eq!(s.mixer.editor_hwnd(id), None);
}

#[test]
fn a_vst3_instruments_window_opens_from_the_keys_button_and_the_mixer() {
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
    check_window(&mut s, id, name);
    let hwnd = s.mixer.editor_hwnd(id);
    // Pressed again, it stays open (it comes to the top), as a built-in's window does.
    assert!(s.press(&g, Hit::Keys(t)));
    assert_eq!(s.mixer.editor_hwnd(id), hwnd);

    // The instrument's row in the mixer shuts it, and opens it again.
    s.drawer_open = true;
    s.tab = DrawerTab::Mixer;
    let g = s.geometry(W, H);
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    assert!(!s.mixer.is_open(id));
    assert!(press(&mut s, &g, MixerHit::Open(t, id)));
    check_window(&mut s, id, name);
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

/// With no player (no output), the window shows an instance of its own, and says its changes
/// are not heard.
#[test]
fn without_output_the_window_is_of_an_instance_apart() {
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
    assert_eq!(s.error, None);
    assert_eq!(s.mixer.editor_heard(id), Some(false));
    assert!(s.mixer.editor_hwnd(id).is_some());
}
