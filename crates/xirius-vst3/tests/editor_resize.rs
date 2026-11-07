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
    // SAFETY: queries on our window and its child (the view's window, same thread).
    unsafe {
        let child = GetWindow(editor.viewport_hwnd() as HWND, GW_CHILD);
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

fn assert_bar_is_painted(path: &std::path::Path, width: u32, height: usize) {
    let bmp = std::fs::read(path).unwrap();
    let bar = &bmp[54..54 + width as usize * height * 4];
    let black = bar
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[..3] == [0, 0, 0])
        .count();
    assert!(
        black * 100 < width as usize * height * 5,
        "unpainted bar pixels: {black}"
    );
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
    assert_eq!(client(editor.viewport_hwnd() as HWND), (w0, h0));
    assert_eq!(report(&editor), format!("size={w0}x{h0} scale={scale:.2}"));

    // The bottom-right corner, 123 px wider and 77 px taller: the width snaps to the view's
    // step, the left and top edges stay.
    let before = window_rect(hwnd);
    let initial_frame_width = client(hwnd).0;
    let to = drag(
        &mut editor,
        WMSZ_BOTTOMRIGHT,
        RECT {
            right: before.right + 123,
            bottom: before.bottom + 77,
            ..before
        },
    );
    let want = constrain(initial_frame_width + 123, h0 + 77);
    assert_eq!(want.0 % STEP, 0);
    assert_eq!((to.left, to.top), (before.left, before.top));
    assert_eq!(client(editor.viewport_hwnd() as HWND), want);
    assert_eq!(editor.size(), want);
    assert_eq!(
        report(&editor),
        format!("size={}x{} scale={scale:.2}", want.0, want.1)
    );
    let cap = editor
        .capture_bmp(&shot_dir().join("plg027-editor-dragged.bmp"))
        .expect("capture");
    assert_eq!((cap.width as i32, cap.height as i32), client(hwnd));
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
    let minimum = constrain((412.0 * scale).ceil() as i32, MIN.1).0;
    assert_eq!(client(editor.viewport_hwnd() as HWND).0, minimum);
    assert_eq!(editor.size().0, minimum);

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
    assert_eq!(client(editor.viewport_hwnd() as HWND), want);
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
    let (w, h) = client(editor.viewport_hwnd() as HWND);
    assert_eq!(editor.size(), (w, h));
    assert!(report(&editor).starts_with(&format!("size={w}x{h} ")));
}

#[test]
fn common_bar_and_view_share_the_gui_thread_without_topmost_or_native_caption() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GWL_STYLE, GetDlgItem, GetWindowLongW, GetWindowThreadProcessId, WM_COMMAND,
        WS_CAPTION, WS_EX_TOPMOST,
    };
    use xirius_vst3::{EditorAction, EditorBar};
    let plugin = fixture();
    plugin.set_editor_bar(EditorBar {
        track: "Vocal".into(),
        device: "Test Effect".into(),
        preset: "Default".into(),
        powered: true,
        ..Default::default()
    });
    let mut editor = plugin
        .open_editor_at("Common frame test", Placement::offscreen())
        .unwrap();
    editor.pump(Duration::from_millis(100));
    assert!(!editor.uses_native_frame());
    assert!(!editor.took_foreground());
    let hwnd = editor.hwnd() as HWND;
    // SAFETY: queries and messages only to the frame and viewport made by this test.
    unsafe {
        assert_eq!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION, 0);
        assert_eq!(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST, 0);
        assert_eq!(
            GetWindowThreadProcessId(hwnd, std::ptr::null_mut()),
            GetWindowThreadProcessId(editor.viewport_hwnd() as HWND, std::ptr::null_mut())
        );
        let button = GetDlgItem(hwnd, 3);
        assert!(!button.is_null());
        let mut label = [0u16; 32];
        let n = GetWindowTextW(button, label.as_mut_ptr(), label.len() as i32);
        assert_eq!(String::from_utf16_lossy(&label[..n as usize]), "表示先");
        SendMessageW(hwnd, WM_COMMAND, 1, 0);
        SendMessageW(hwnd, WM_COMMAND, 4, 0);
    }
    assert_eq!(
        editor.take_actions(),
        [EditorAction::Power, EditorAction::Pin]
    );
    let frame = client(hwnd);
    let view = client(editor.viewport_hwnd() as HWND);
    assert!(frame.1 > view.1);
    editor
        .capture_bmp(&shot_dir().join("common-frame-fixture.bmp"))
        .unwrap();
    for id in 1..=5 {
        let button = unsafe { GetDlgItem(hwnd, id) };
        let picture = xirius_vst3::capture_window(
            button as isize,
            &shot_dir().join(format!("common-frame-button-{id}.bmp")),
        )
        .unwrap();
        assert!(
            picture.distinct_colors > 1,
            "bar button {id} draws its face and icon"
        );
    }
    let suggested = window_rect(hwnd);
    // SAFETY: simulate a 175% monitor notification only on this test's parked frame.
    unsafe {
        SendMessageW(
            hwnd,
            WM_DPICHANGED,
            168 | (168 << 16),
            &suggested as *const RECT as isize,
        );
    }
    editor.pump(Duration::from_millis(50));
    let mut origin = windows_sys::Win32::Foundation::POINT::default();
    unsafe {
        windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin);
    }
    let viewport = window_rect(editor.viewport_hwnd() as HWND);
    assert_eq!(viewport.top - origin.y, 63, "36 logical pixels at 175%");
    assert_eq!(
        client(hwnd).1 - client(editor.viewport_hwnd() as HWND).1,
        63
    );
    assert!(report(&editor).ends_with("scale=1.75"));
    let path = shot_dir().join("common-frame-fixture-175.bmp");
    let capture = editor.capture_bmp(&path).unwrap();
    assert_bar_is_painted(&path, capture.width, 63);
}
