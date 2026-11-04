//! UI-282: the window closed the way the OS closes it (the caption's ×, Alt+F4, the taskbar:
//! WM_CLOSE) closes without falling over. The first close waits for the song to be saved, and
//! asking for a frame there used to panic inside the window procedure, where a panic cannot
//! unwind: the process ended before anything after the window (the installer of an update
//! among it) could run. Only messages to the window itself are sent; nothing takes the focus.
//! The window starts on the start screen (UI-350). A close that comes while the start still
//! looks for a song to reopen asks first, by design, and the window stays: the close is sent
//! again until the process ends, with no fixed wait for the start.
#![cfg(windows)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows::core::BOOL;

/// The visible windows of the child process.
fn windows_of(pid: u32) -> Vec<HWND> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let (want, out) = unsafe { &mut *(lp.0 as *mut (u32, Vec<HWND>)) };
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid == *want && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            out.push(hwnd);
        }
        BOOL(1)
    }
    let mut found = (pid, Vec::new());
    unsafe {
        let _ = EnumWindows(
            Some(each),
            LPARAM(&mut found as *mut (u32, Vec<HWND>) as isize),
        );
    }
    found.1
}

/// The parked window, everything it writes kept in a temp folder.
fn launch() -> (Child, tempfile::TempDir) {
    let tmp = tempfile::Builder::new()
        .prefix("xirius-close-")
        .tempdir()
        .unwrap();
    let keep = tmp.path().join("keep");
    std::fs::create_dir_all(&keep).unwrap();
    std::fs::write(
        keep.join("recent.json"),
        r#"{"songs": [], "open": "sample"}"#,
    )
    .unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_xirius-ui"))
        .args(["--park", "--silent"])
        .env("XIRIUS_CONFIG_DIR", tmp.path().join("prefs"))
        .env("XIRIUS_CRASH_DIR", tmp.path().join("crashes"))
        .env("XIRIUS_KEEP_DIR", &keep)
        .env("XIRIUS_TRASH_DIR", tmp.path().join("trash"))
        .env("XIRIUS_PLUGIN_CACHE", tmp.path().join("plugins.json"))
        .env("XIRIUS_PEAK_CACHE", tmp.path().join("peaks"))
        .env("XIRIUS_TRACK_PRESETS", tmp.path().join("presets"))
        .stdout(Stdio::null())
        // A file, not a pipe: a pipe nobody reads until the end would hold the close up.
        .stderr(std::fs::File::create(tmp.path().join("stderr.txt")).unwrap())
        .spawn()
        .unwrap();
    (child, tmp)
}

fn close_once_or_twice(twice: bool) {
    let (mut child, tmp) = launch();
    let limit = Instant::now() + Duration::from_secs(60);
    let window = loop {
        if let Some(&hwnd) = windows_of(child.id()).first() {
            break hwnd;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the window ended before it opened: {status:?}");
        }
        if Instant::now() > limit {
            let _ = child.kill();
            panic!("no window in 60 s");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let limit = Instant::now() + Duration::from_secs(60);
    let mut next_close = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= next_close {
            // The window may be gone already: a failed post is the close under way.
            let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            if twice {
                // A second × while the first close still waits for the save.
                std::thread::sleep(Duration::from_millis(30));
                let _ = unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
            next_close = Instant::now() + Duration::from_secs(5);
        }
        if Instant::now() > limit {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = std::fs::read_to_string(tmp.path().join("stderr.txt")).unwrap_or_default();
            panic!(
                "the window did not close
{stderr}"
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stderr = std::fs::read_to_string(tmp.path().join("stderr.txt")).unwrap_or_default();
    assert!(status.success(), "{status:?}\n{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    let crashes = tmp.path().join("crashes");
    let left = std::fs::read_dir(&crashes)
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name()).collect())
        .unwrap_or_else(|_| Vec::new());
    assert!(left.is_empty(), "crash records: {left:?}\n{stderr}");
}

#[test]
fn the_os_close_closes_the_window_without_falling_over() {
    close_once_or_twice(false);
}

#[test]
fn a_second_close_while_the_first_waits_closes_once() {
    close_once_or_twice(true);
}
