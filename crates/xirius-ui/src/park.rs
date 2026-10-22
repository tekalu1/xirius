//! Moves the spike window outside every display without activating it.
//!
//! GPUI puts a window whose bounds miss every display back on the default
//! bounds of a display, so asking for an off-screen position at open time does
//! not work. Moving the window afterwards does.

/// Where a parked window goes: far outside any display layout.
pub const PARK_AT: i32 = -32000;

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowExW(parent: Hwnd, after: Hwnd, class: *const u16, title: *const u16) -> Hwnd;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn GetForegroundWindow() -> Hwnd;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn SetWindowPos(
            hwnd: Hwnd,
            after: Hwnd,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;

    pub fn own_window(title: &str) -> Hwnd {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let mut hwnd = std::ptr::null_mut();
        // SAFETY: the title is NUL-terminated and the PID output lives across
        // the call. Only a handle belonging to this process is returned.
        unsafe {
            loop {
                hwnd = FindWindowExW(std::ptr::null_mut(), hwnd, std::ptr::null(), wide.as_ptr());
                if hwnd.is_null() {
                    return hwnd;
                }
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == std::process::id() {
                    return hwnd;
                }
            }
        }
    }

    /// The first top-level window titled `title`, of any process.
    pub fn any_window(title: &str) -> Hwnd {
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: the title is NUL-terminated.
        unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                wide.as_ptr(),
            )
        }
    }

    pub fn foreground() -> Hwnd {
        // SAFETY: a plain query.
        unsafe { GetForegroundWindow() }
    }

    pub fn origin(hwnd: Hwnd) -> Option<(i32, i32)> {
        let mut r = Rect::default();
        // SAFETY: the rectangle outlives the call; a gone window fails it.
        (unsafe { GetWindowRect(hwnd, &mut r) } != 0).then_some((r.left, r.top))
    }

    /// Returns true when the window was found and moved.
    pub fn park(title: &str, x: i32, y: i32) -> bool {
        // SAFETY: the handle belongs to this process; the flags keep the
        // window from being resized, re-ordered or activated.
        unsafe {
            let hwnd = own_window(title);
            if hwnd.is_null() {
                return false;
            }
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) != 0
        }
    }
}

#[cfg(windows)]
pub(crate) use imp::own_window;

/// The handle of this process's window titled `title`, if it has one (a plugin's window is
/// owned by it).
pub fn own_handle(title: &str) -> Option<isize> {
    #[cfg(windows)]
    {
        let hwnd = imp::own_window(title);
        (!hwnd.is_null()).then_some(hwnd as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        None
    }
}

/// The handle of a window titled `title` of any process (a plugin host's window).
pub fn any_window(title: &str) -> Option<isize> {
    #[cfg(windows)]
    {
        let hwnd = imp::any_window(title);
        (!hwnd.is_null()).then_some(hwnd as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        None
    }
}

/// The window `hwnd` is the one in front, taking the keys.
pub fn is_foreground(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        imp::foreground() as isize == hwnd
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// The top-left corner of the window `hwnd` on the desktop, in physical pixels.
pub fn origin(hwnd: isize) -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        imp::origin(hwnd as *mut std::ffi::c_void)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        None
    }
}

/// Moves the window titled `title` to (`x`, `y`) in physical pixels. Does
/// nothing (and returns true, so the caller stops trying) off Windows.
pub fn park(title: &str, x: i32, y: i32) -> bool {
    #[cfg(windows)]
    {
        imp::park(title, x, y)
    }
    #[cfg(not(windows))]
    {
        let _ = (title, x, y);
        true
    }
}
