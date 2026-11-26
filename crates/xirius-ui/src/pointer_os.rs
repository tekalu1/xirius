//! The tool pictures as the system's cursor (UI-184). The pictures (`pointer::picture`) are made
//! into cursors once and handed to the system when it asks the window which cursor to show
//! (`WM_SETCURSOR`), so the cursor itself is the tool, at the pointer's own position, with no
//! frame of delay.
//!
//! GPUI sets the window's cursor from a fixed list of the system's shapes and has no cursor of
//! its own to give, so the window is subclassed: while a tool's picture is asked for, the
//! subclass answers `WM_SETCURSOR` itself, and passes everything else (and every other time) on
//! to GPUI's window procedure. Nothing of GPUI is changed (`PATCHED.md` stays empty).
//!
//! Each window keeps the cursor it answers with (an `HCURSOR`, 0 while the system's shape is
//! asked for) as its subclass's own data, so the main window and the windows of their own
//! (UI-384) each show theirs.

use std::collections::HashMap;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::{LazyLock, Mutex};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, ScreenToClient,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetCapture;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, GetWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetClientRect, GetCursorPos, GetIconInfo, GetMessageExtraInfo, HTCLIENT,
    ICONINFO, SM_CXCURSOR, SendMessageW, SetCursor, WM_MOUSEMOVE, WM_SETCURSOR, WM_USER,
    WindowFromPoint,
};

use crate::pointer::{Shape, picture};

/// GPUI's own message when the cursor it wants changes (`WM_USER + 1`, its `events.rs`).
const GPUI_CURSOR_CHANGED: u32 = WM_USER + 1;
const SUBCLASS_ID: usize = 0x5849_5249; // "XIRI"

static CURSORS: LazyLock<Mutex<HashMap<(Shape, usize), isize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether the mouse message being handled was made from a touch: Windows marks those it makes
/// from a pen or a finger, and a finger's with one more bit (MIX-045 makes rows 32 high).
pub fn from_touch() -> bool {
    // SAFETY: reads the extra information of the thread's last message; no pointers.
    let extra = unsafe { GetMessageExtraInfo() } as u32;
    extra & 0xFFFF_FF80 == 0xFF51_5780
}

/// Whether the mouse message being handled was made from a pen or a finger: both tremble
/// more than a mouse as they lift.
pub fn from_pen_or_touch() -> bool {
    // SAFETY: reads the extra information of the thread's last message; no pointers.
    let extra = unsafe { GetMessageExtraInfo() } as u32;
    extra & 0xFFFF_FF00 == 0xFF51_5700
}

/// The size of a cursor at the window's scale: the box the system draws its own cursors in
/// (UI-382), so a picture of ours is as big as the arrow and the hand beside it.
pub fn size_for(hwnd: isize) -> usize {
    // SAFETY: a plain query on a window handle.
    let dpi = unsafe { GetDpiForWindow(hwnd as HWND) };
    size_at(dpi)
}

/// The system's cursor box at `dpi` (48 pixels at 175 % on most machines, not 32 × 1.75).
pub fn size_at(dpi: u32) -> usize {
    let dpi = dpi.max(96);
    // SAFETY: a plain query of a system metric.
    let system = unsafe { GetSystemMetricsForDpi(SM_CXCURSOR, dpi) };
    box_size(dpi, system)
}

/// The box from the system's answer, or 32 at 96 dpi when it gives none.
fn box_size(dpi: u32, system: i32) -> usize {
    if system > 0 {
        (system as usize).clamp(16, 256)
    } else {
        (32 * dpi as usize / 96).clamp(32, 128)
    }
}

/// The cursor of `shape` at `size` pixels, made on first use and kept.
pub fn cursor(shape: Shape, size: usize) -> isize {
    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
    let mut all = CURSORS.lock().unwrap_or_else(|e| e.into_inner());
    *all.entry((shape, size))
        .or_insert_with(|| make(shape, size))
}

fn make(shape: Shape, size: usize) -> isize {
    let pic = picture(shape, size);
    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size as i32,
        biHeight: -(size as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        bmiColors: [windows_sys::Win32::Graphics::Gdi::RGBQUAD {
            rgbBlue: 0,
            rgbGreen: 0,
            rgbRed: 0,
            rgbReserved: 0,
        }],
    };
    let mut bits = null_mut();
    // SAFETY: a 32-bit top-down section of `size` x `size`; `bits` is written by the system.
    let color =
        unsafe { CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0) };
    assert!(!color.is_null() && !bits.is_null(), "a cursor bitmap");
    // The system blends cursors with premultiplied colour.
    let premultiplied: Vec<u8> = pic
        .bgra
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u32::from(p[3]);
            let v = (u32::from(p[0]) * a / 255) as u8;
            [v, v, v, p[3]]
        })
        .collect();
    // SAFETY: the section holds `size * size * 4` bytes, as many as `premultiplied`.
    unsafe {
        std::ptr::copy_nonoverlapping(
            premultiplied.as_ptr(),
            bits.cast::<u8>(),
            premultiplied.len(),
        )
    };
    // The 1-bit mask (rows padded to 16 bits) is unused where the colour has alpha.
    let mask_bits = vec![0u8; size.div_ceil(16) * 2 * size];
    // SAFETY: a monochrome bitmap of `size` x `size` over `mask_bits`, copied by the call.
    let mask = unsafe { CreateBitmap(size as i32, size as i32, 1, 1, mask_bits.as_ptr().cast()) };
    let icon = ICONINFO {
        fIcon: 0,
        xHotspot: pic.hot.0 as u32,
        yHotspot: pic.hot.1 as u32,
        hbmMask: mask,
        hbmColor: color,
    };
    // SAFETY: both bitmaps are ours; the system copies them, so they are freed after.
    let hcursor = unsafe {
        let h = CreateIconIndirect(&icon);
        DeleteObject(color);
        DeleteObject(mask);
        h
    };
    assert!(!hcursor.is_null(), "a cursor");
    hcursor as isize
}

/// The working point the system holds for `cursor`.
pub fn hot_of(cursor: isize) -> Option<(u32, u32)> {
    let mut info = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: null_mut(),
        hbmColor: null_mut(),
    };
    // SAFETY: `info` is ours; the bitmaps it names are freed after the call.
    unsafe {
        if GetIconInfo(cursor as _, &mut info) == 0 {
            return None;
        }
        DeleteObject(info.hbmMask);
        DeleteObject(info.hbmColor);
    }
    Some((info.xHotspot, info.yHotspot))
}

/// Takes over the window's cursor answers. Safe to call again for a window already taken.
pub fn install(hwnd: isize) -> bool {
    let hwnd = hwnd as HWND;
    let mut data = 0usize;
    // SAFETY: our own window, on the thread that made it (the subclass is a plain `extern fn`).
    unsafe {
        if GetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, &mut data) != 0 {
            return true;
        }
        SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0) != 0
    }
}

/// Asks for `shape` (or, `None`, the system's own shape again). The window shows it at once
/// when the pointer is over it.
pub fn show(hwnd: isize, shape: Option<Shape>) {
    let window = hwnd as HWND;
    let next = shape.map_or(0, |s| cursor(s, size_for(hwnd)));
    let Some(now) = active(window) else {
        return;
    };
    if now == next {
        return;
    }
    // SAFETY: our own window, already subclassed; only the data the subclass reads changes.
    unsafe { SetWindowSubclass(window, Some(subclass), SUBCLASS_ID, next as usize) };
    if over_client(window) {
        // SAFETY: a message to our own window, as the system sends it on every pointer move.
        unsafe {
            SendMessageW(
                window,
                WM_SETCURSOR,
                window as WPARAM,
                (HTCLIENT as isize) | ((WM_MOUSEMOVE as isize) << 16),
            );
        }
    }
}

/// The cursor the window hands to the system when asked now: what `SetCursor` was given.
/// `None` when it leaves the choice to GPUI.
pub fn handed(hwnd: isize) -> Option<isize> {
    let window = hwnd as HWND;
    // SAFETY: a message to our own window, answered by our subclass (or GPUI's procedure).
    unsafe {
        SetCursor(null_mut());
        SendMessageW(
            window,
            WM_SETCURSOR,
            window as WPARAM,
            (HTCLIENT as isize) | ((WM_MOUSEMOVE as isize) << 16),
        );
        let now = windows_sys::Win32::UI::WindowsAndMessaging::GetCursor();
        (Some(now as isize) == active(window) && !now.is_null()).then_some(now as isize)
    }
}

/// The cursor `hwnd` answers with now, or `None` before [`install`].
fn active(hwnd: HWND) -> Option<isize> {
    let mut data = 0usize;
    // SAFETY: reads the data of our subclass on our own window.
    let installed = unsafe { GetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, &mut data) };
    (installed != 0).then_some(data as isize)
}

/// Whether the pointer is over the window's client area (and not over another window).
fn over_client(hwnd: HWND) -> bool {
    let mut at = POINT { x: 0, y: 0 };
    let mut client = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: plain queries on our window and the pointer.
    unsafe {
        GetCursorPos(&mut at) != 0
            && WindowFromPoint(at) == hwnd
            && ScreenToClient(hwnd, &mut at) != 0
            && GetClientRect(hwnd, &mut client) != 0
            && at.x >= client.left
            && at.x < client.right
            && at.y >= client.top
            && at.y < client.bottom
    }
}

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let ours = data as isize;
    if msg == WM_SETCURSOR && ours != 0 && (lparam & 0xffff) as u32 == HTCLIENT {
        // SAFETY: `ours` is a cursor made and kept by `cursor`.
        unsafe { SetCursor(ours as _) };
        return 1;
    }
    // SAFETY: the next procedure of this window, with the message as it came.
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    if msg == GPUI_CURSOR_CHANGED {
        // SAFETY: a plain query of the thread's capture.
        let captured = unsafe { GetCapture() } == hwnd;
        if let Some(cursor) = after_change(ours, lparam, captured, || over_client(hwnd)) {
            // SAFETY: a cursor of ours or the system's shape GPUI loaded.
            unsafe { SetCursor(cursor as _) };
        }
    }
    result
}

/// The cursor to set once GPUI has changed the shape it wants (`lparam`, 0 for none). A tool's
/// picture goes over GPUI's. While a button is held the window has the mouse captured and the
/// system sends no `WM_SETCURSOR`, so GPUI's new shape (a drop refused in the middle of a drag,
/// UI-381) is set here or it would wait for the button to be let go.
fn after_change(
    ours: isize,
    lparam: LPARAM,
    captured: bool,
    over: impl FnOnce() -> bool,
) -> Option<isize> {
    let wanted = if ours != 0 {
        ours
    } else if captured && lparam != 0 {
        lparam
    } else {
        return None;
    };
    over().then_some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::null;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetCursor, WS_POPUP,
    };

    /// A window of ours that is never shown, as GPUI's is to this module: a plain one.
    fn window() -> isize {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: a hidden pop-up of the system's own class, on this thread.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_POPUP,
                -32000,
                -32000,
                100,
                100,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            )
        };
        assert!(!hwnd.is_null());
        hwnd as isize
    }

    #[test]
    fn the_window_hands_the_tools_cursor_to_the_system_and_takes_it_back() {
        let hwnd = window();
        assert!(install(hwnd));
        assert!(install(hwnd), "again is harmless");
        for shape in Shape::ALL {
            show(hwnd, Some(shape));
            let handed = handed(hwnd).expect("the tool's cursor");
            assert_eq!(handed, cursor(shape, size_for(hwnd)));
            // SAFETY: a query.
            assert_eq!(unsafe { GetCursor() } as isize, handed);
            let (x, y) = hot_of(handed).expect("a cursor");
            let size = size_for(hwnd) as f32;
            let want = shape.hot();
            assert_eq!(
                (x, y),
                ((want.0 * size / 32.0) as u32, (want.1 * size / 32.0) as u32),
                "{shape:?}: the working point"
            );
        }
        // Back to the system's shape: the window no longer answers with a cursor of ours.
        show(hwnd, None);
        assert_eq!(handed(hwnd), None);
        // SAFETY: our own window.
        unsafe { DestroyWindow(hwnd as HWND) };
    }

    #[test]
    fn each_window_keeps_its_own_cursor() {
        let (main, own) = (window(), window());
        // Not taken yet: nothing is asked of it.
        show(own, Some(Shape::Pencil));
        assert_eq!(handed(own), None);
        assert!(install(main) && install(own));
        show(main, Some(Shape::Pencil));
        show(own, Some(Shape::Eraser));
        assert_eq!(handed(main), Some(cursor(Shape::Pencil, size_for(main))));
        assert_eq!(handed(own), Some(cursor(Shape::Eraser, size_for(own))));
        // A window of its own back to the system's shape leaves the main window's tool.
        show(own, None);
        assert_eq!(handed(own), None);
        assert_eq!(handed(main), Some(cursor(Shape::Pencil, size_for(main))));
        // SAFETY: our own windows.
        unsafe {
            DestroyWindow(main as HWND);
            DestroyWindow(own as HWND);
        }
    }

    #[test]
    fn the_cursor_box_is_the_systems_at_every_scale() {
        for dpi in [96, 120, 144, 168, 192] {
            // SAFETY: a plain query of a system metric.
            let system = unsafe { GetSystemMetricsForDpi(SM_CXCURSOR, dpi) };
            assert!(system > 0, "{dpi}: the system answers");
            assert_eq!(size_at(dpi), system as usize, "{dpi}");
        }
        // No answer: 32 at 96 dpi, as before.
        assert_eq!(box_size(168, 0), 56);
        assert_eq!(box_size(96, 0), 32);
        assert_eq!(box_size(168, 48), 48);
    }

    #[test]
    fn a_shape_changed_mid_drag_is_set_while_the_mouse_is_captured() {
        // A tool's picture always goes over GPUI's, over the window only.
        assert_eq!(after_change(7, 3, false, || true), Some(7));
        assert_eq!(after_change(7, 3, true, || false), None);
        // A held button: the system asks for no cursor, so GPUI's new one is set at once.
        assert_eq!(after_change(0, 3, true, || true), Some(3));
        // No button held: the system asks on the next move, as usual.
        assert_eq!(after_change(0, 3, false, || true), None);
        // GPUI hid the pointer: nothing to hand on.
        assert_eq!(after_change(0, 0, true, || true), None);
    }
}
