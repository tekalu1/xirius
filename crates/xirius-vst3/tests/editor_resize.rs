//! The editor window's resize paths (PLG-027) against the fixture's resizable view: a drag on
//! the border goes through `checkSizeConstraint` and `onSize`, and the window's DPI reaches the
//! view as a content scale. The window opens off every screen and never takes the foreground;
//! no OS input is sent, only the window messages a drag and a DPI change produce.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GW_CHILD, GetClientRect, GetSystemMetrics, GetWindow, GetWindowRect, GetWindowTextW,
    SM_CXVIRTUALSCREEN, SM_XVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SendMessageW,
    SetWindowPos, WM_DPICHANGED, WM_SIZING, WMSZ_BOTTOMRIGHT, WMSZ_LEFT,
};
use xirius_test_vst3::editor::{MIN, STEP, constrain};
use xirius_vst3::{Editor, Module, Placement, Plugin};

fn fixture() -> Plugin {
    xirius_test_vst3::fixture_marker();
    let deps: PathBuf = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = deps.join("xirius_test_vst3.dll");
    assert!(dll.exists(), "the fixture's cdylib is built with the tests");
    let module = Arc::new(Module::load(&dll).expect("load"));
    let class = module.classes().into_iter().next().expect("a class");
    Plugin::new(module, &class).expect("create")
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a plain query on our window.
    unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

fn client(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: a plain query on our window.
    unsafe { GetClientRect(hwnd, &mut r) };
    (r.right - r.left, r.bottom - r.top)
}

/// What the fixture's view says it was told: "size=WxH scale=S".
fn report(editor: &Editor) -> String {
    let hwnd = editor.hwnd() as HWND;
    // SAFETY: queries on our window and its child (the view's window, same thread).
    unsafe {
        let child = GetWindow(hwnd, GW_CHILD);
        assert!(!child.is_null(), "the view has a window");
        let mut buf = [0u16; 128];
        let n = GetWindowTextW(child, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

/// A drag of the border from `edge` to the outer rect `to`, as the system sends it: the
/// window procedure adjusts the rect, then the window takes it.
fn drag(editor: &mut Editor, edge: u32, mut to: RECT) -> RECT {
    let hwnd = editor.hwnd() as HWND;
    // SAFETY: messages to our own window, with a rect that outlives the call.
    unsafe {
        SendMessageW(
            hwnd,
            WM_SIZING,
            edge as usize,
            &mut to as *mut RECT as isize,
        );
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            to.left,
            to.top,
            to.right - to.left,
            to.bottom - to.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    editor.pump(Duration::from_millis(30));
    to
}

/// Where the shots go to be looked at: `XIRIUS_EDITOR_SHOTS`, else the target folder (written
/// over by the next run) and not the temp folder (ARC-028).
fn shot_dir() -> PathBuf {
    std::env::var_os("XIRIUS_EDITOR_SHOTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")))
}

#[test]
fn a_drag_and_a_dpi_change_reach_the_view_through_its_constraint() {
    let plugin = fixture();
    let mut editor = plugin
        .open_editor_at("Xirius Test Latency", Placement::offscreen())
        .expect("open");
    editor.pump(Duration::from_millis(50));
    let hwnd = editor.hwnd() as HWND;
    assert!(!editor.took_foreground());
    // Off every screen.
    // SAFETY: plain queries.
    let right =
        unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    assert!(editor.origin().0 >= right, "the window is off screen");
    assert!(editor.can_resize());
    let scale = editor
        .content_scale()
        .expect("the view takes a content scale");
    let (w0, h0) = editor.size();
    assert_eq!(client(hwnd), (w0, h0));
    assert_eq!(report(&editor), format!("size={w0}x{h0} scale={scale:.2}"));

    // The bottom-right corner, 123 px wider and 77 px taller: the width snaps to the view's
    // step, the left and top edges stay.
    let before = window_rect(hwnd);
    let to = drag(
        &mut editor,
        WMSZ_BOTTOMRIGHT,
        RECT {
            right: before.right + 123,
            bottom: before.bottom + 77,
            ..before
        },
    );
    let want = constrain(w0 + 123, h0 + 77);
    assert_eq!(want.0 % STEP, 0);
    assert_eq!((to.left, to.top), (before.left, before.top));
    assert_eq!(client(hwnd), want);
    assert_eq!(editor.size(), want);
    assert_eq!(
        report(&editor),
        format!("size={}x{} scale={scale:.2}", want.0, want.1)
    );
    let cap = editor
        .capture_bmp(&shot_dir().join("plg027-editor-dragged.bmp"))
        .expect("capture");
    assert_eq!((cap.width as i32, cap.height as i32), want);
    assert!(cap.distinct_colors > 1, "the view draws after the resize");

    // The left edge, far below the smallest size: the right edge stays and the view gets its
    // minimum.
    let before = window_rect(hwnd);
    let to = drag(
        &mut editor,
        WMSZ_LEFT,
        RECT {
            left: before.right - 30,
            ..before
        },
    );
    assert_eq!(to.right, before.right);
    assert_eq!(client(hwnd).0, MIN.0);
    assert_eq!(editor.size().0, MIN.0);

    // A move to a monitor at 150 %: the view gets the scale and asks for its larger size
    // through the frame.
    let (w1, h1) = editor.size();
    let requests = editor.resize_requests();
    let mut suggested = window_rect(hwnd);
    // SAFETY: a message to our own window, with a rect that outlives the call.
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            (144 << 16) | 144,
            &mut suggested as *mut RECT as isize,
        )
    };
    editor.pump(Duration::from_millis(30));
    assert_eq!(editor.content_scale(), Some(1.5));
    let want = constrain(
        (w1 as f32 * 1.5 / scale).round() as i32,
        (h1 as f32 * 1.5 / scale).round() as i32,
    );
    assert_eq!(editor.resize_requests(), requests + 1);
    assert_eq!(editor.size(), want);
    assert_eq!(client(hwnd), want);
    assert_eq!(
        report(&editor),
        format!("size={}x{} scale=1.50", want.0, want.1)
    );

    // Back to 100 % through the host's own call.
    assert!(editor.set_content_scale(1.0));
    editor.pump(Duration::from_millis(30));
    assert_eq!(editor.content_scale(), Some(1.0));
    assert!(report(&editor).ends_with("scale=1.00"));
    let cap = editor
        .capture_bmp(&shot_dir().join("plg027-editor-scaled.bmp"))
        .expect("capture");
    assert!(cap.distinct_colors > 1);
    assert!(!editor.took_foreground());
}

#[test]
fn a_window_resized_without_a_drag_still_tells_the_view() {
    let plugin = fixture();
    let mut editor = plugin
        .open_editor_at("Xirius Test Latency", Placement::offscreen())
        .expect("open");
    editor.pump(Duration::from_millis(30));
    let hwnd = editor.hwnd() as HWND;
    let r = window_rect(hwnd);
    // As maximizing does: the window gets a size without WM_SIZING; the view still hears it.
    // SAFETY: our own window.
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            r.right - r.left + 40,
            r.bottom - r.top + 20,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
    editor.pump(Duration::from_millis(30));
    let (w, h) = client(hwnd);
    assert_eq!(editor.size(), (w, h));
    assert!(report(&editor).starts_with(&format!("size={w}x{h} ")));
}
