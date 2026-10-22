//! What other vendors' editors put under the frame (PLG-048): the class of every child window
//! and which of them own a caret or an input context. Each test skips when its plugin is not
//! installed. The windows open off every screen and never take the foreground; no OS input is
//! sent.
//!
//! Every plugin of this file is made, shown and released on one thread ([`on_ui_thread`]), the
//! way a host has one UI thread. The tests themselves run on several threads at once, and two
//! kHs editors opened from two of them at the same moment crashed inside Kilohearts' shared
//! `HeartCore.core_64` (an access violation, about once in 25 runs on a busy machine).

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GW_CHILD, GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetWindow,
    GetWindowLongW, GetWindowThreadProcessId, PostMessageW, WM_KEYDOWN, WM_KEYUP, WS_EX_NOACTIVATE,
};
use xirius_vst3::{Module, Placement, Plugin, default_vst3_dir};

type Job = Box<dyn FnOnce() + Send>;
/// A job and where its outcome (or its panic's message) goes.
type Errand = (Job, Sender<Result<(), String>>);

/// How long a busy machine may take before a window or a key shows its effect.
const PATIENCE: Duration = Duration::from_secs(20);

/// Runs `job` on the one thread that makes and shows this file's plugins, and returns when it
/// is done. The thread lives as long as the process, so the windows a vendor keeps for itself
/// on it stay valid between tests. A panic in `job` fails the calling test with its message:
/// the UI thread's own output goes to whichever test started it.
fn on_ui_thread(job: impl FnOnce() + Send + 'static) {
    static UI: OnceLock<Sender<Errand>> = OnceLock::new();
    let ui = UI.get_or_init(|| {
        let (jobs, next) = channel::<Errand>();
        std::thread::Builder::new()
            .name("plugin-ui".into())
            .spawn(move || {
                for (job, done) in next {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                        .map_err(|panic| {
                            panic
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_default()
                        });
                    let _ = done.send(result);
                }
            })
            .expect("the UI thread");
        jobs
    });
    let (done, result) = channel();
    ui.send((Box::new(job), done))
        .expect("the UI thread is running");
    if let Err(message) = result.recv().expect("the UI thread answers") {
        panic!("{message}");
    }
}

/// Whether `hwnd` is a window of this process (the frame, a vendor's child or its popup).
fn ours(hwnd: HWND) -> bool {
    let mut pid = 0;
    // SAFETY: a plain query; a null or stale handle leaves `pid` at 0.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid == std::process::id()
}

/// Runs the editor's messages until `done` holds, for at most [`PATIENCE`].
fn pump_until(
    editor: &mut xirius_vst3::Editor,
    mut done: impl FnMut(&xirius_vst3::Editor) -> bool,
) {
    let end = Instant::now() + PATIENCE;
    while !done(editor) && Instant::now() < end {
        editor.pump(Duration::from_millis(20));
    }
}

fn load(relative: &str) -> Option<Plugin> {
    let path: PathBuf = default_vst3_dir().join(relative);
    if !path.exists() {
        eprintln!("skipped: {} is not installed", path.display());
        return None;
    }
    let module = Arc::new(Module::load(&path).expect("load"));
    let class = module
        .classes()
        .into_iter()
        .find(|c| c.is_audio_processor())
        .expect("an audio class");
    Some(Plugin::new(module, &class).expect("create"))
}

fn classes(frame: HWND) -> Vec<String> {
    unsafe extern "system" fn visit(hwnd: HWND, out: LPARAM) -> i32 {
        let mut name = [0u16; 128];
        // SAFETY: the buffer is as long as stated; `out` is the caller's vector.
        unsafe {
            let n = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32).max(0) as usize;
            let ime = ImmGetContext(hwnd);
            let has = !ime.is_null();
            if has {
                ImmReleaseContext(hwnd, ime);
            }
            (*(out as *mut Vec<String>)).push(format!(
                "{} ime={has}",
                String::from_utf16_lossy(&name[..n])
            ));
        }
        1
    }
    let mut out: Vec<String> = Vec::new();
    // SAFETY: the callback only runs inside this call.
    unsafe { EnumChildWindows(frame, Some(visit), &mut out as *mut Vec<String> as LPARAM) };
    out
}

fn survey(relative: &'static str) {
    on_ui_thread(move || survey_here(relative));
}

fn survey_here(relative: &str) {
    let Some(plugin) = load(relative) else {
        return;
    };
    let before = unsafe { GetForegroundWindow() };
    let mut editor = plugin
        .open_editor_at(relative, Placement::offscreen())
        .expect("an editor");
    editor.pump(Duration::from_millis(1500));
    // A vendor that makes its windows late (a busy machine) is waited for.
    pump_until(&mut editor, |e| !classes(e.hwnd() as HWND).is_empty());
    let found = classes(editor.hwnd() as HWND);
    eprintln!("{relative}: {found:?}");
    assert!(!editor.took_foreground(), "{relative}: {found:?}");
    // Another program (or the person) may move the foreground meanwhile; none of ours may.
    let now = unsafe { GetForegroundWindow() };
    assert!(
        now == before || !ours(now),
        "{relative}: our window took the foreground"
    );
    assert!(
        !found.is_empty(),
        "{relative}: the view has windows below the frame"
    );
    // SAFETY: a plain query on our window.
    let child = unsafe { GetWindow(editor.hwnd() as HWND, GW_CHILD) };
    assert!(!child.is_null());
    let space = |child: HWND| {
        // SAFETY: window messages to the vendor's own child window, which no input device sends.
        unsafe {
            PostMessageW(child, WM_KEYDOWN, 0x20, 1);
            PostMessageW(child, WM_KEYUP, 0x20, 1 | (3 << 30));
        }
    };
    let mut seen = 0;
    space(child);
    pump_until(&mut editor, |e| {
        seen += e.take_play_requests();
        seen > 0
    });
    // A second request from the same press would come with the same messages.
    editor.pump(Duration::from_millis(300));
    seen += editor.take_play_requests();
    assert_eq!(
        seen, 1,
        "{relative}: a Space the vendor's window leaves alone reaches the frame once"
    );
    // SAFETY: as above; a letter is the vendor's own to take.
    unsafe {
        PostMessageW(child, WM_KEYDOWN, 0x41, 1);
        PostMessageW(child, WM_KEYUP, 0x41, 1 | (3 << 30));
    }
    // A Space behind the letter marks that the letter has been handled: the child's messages
    // arrive in order.
    space(child);
    let mut after = 0;
    pump_until(&mut editor, |e| {
        after += e.take_play_requests();
        after > 0
    });
    editor.pump(Duration::from_millis(300));
    after += editor.take_play_requests();
    assert_eq!(after, 1, "{relative}: only Space");
}

#[test]
fn a_gain_from_kilohearts_has_child_windows() {
    survey("Kilohearts/kHs Gain.vst3");
}

#[test]
fn supermassive_has_child_windows() {
    survey("ValhallaSupermassive.vst3");
}

#[test]
fn vital_has_child_windows() {
    survey("Vital.vst3");
}

/// A frame opened where a person asked for it takes focus from a click; the others stay
/// out of the way. The system leaves a click on a `WS_EX_NOACTIVATE` window without focus,
/// so the live window's style is the whole policy, and no click is sent.
#[test]
fn a_requested_frame_takes_focus_from_a_click_and_a_tool_does_not() {
    on_ui_thread(requested_frame_takes_focus);
}

fn requested_frame_takes_focus() {
    let Some(plugin) = load("Kilohearts/kHs Gain.vst3") else {
        return;
    };
    let away = Placement::offscreen();
    let noactivate = |place: Placement| {
        let editor = plugin.open_editor_at("focus", place).expect("an editor");
        // SAFETY: a plain query on our own frame.
        let style = unsafe { GetWindowLongW(editor.hwnd() as HWND, GWL_EXSTYLE) } as u32;
        assert!(!editor.took_foreground());
        style & WS_EX_NOACTIVATE != 0
    };
    assert!(noactivate(away));
    assert!(!noactivate(Placement { top: true, ..away }));
}
