//! Keep the physical key and keypad identity that GPUI's character event omits.
//! The hook observes messages on this window's thread; it never sends OS input.

pub struct Native {
    pub key: Option<String>,
    pub code: Option<String>,
    pub character: Option<String>,
}

#[cfg(windows)]
mod windows {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;

    #[repr(C)]
    struct Message {
        lparam: isize,
        wparam: usize,
        message: u32,
        hwnd: *mut c_void,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWindowsHookExW(
            kind: i32,
            proc: unsafe extern "system" fn(i32, usize, isize) -> isize,
            module: *mut c_void,
            thread: u32,
        ) -> *mut c_void;
        fn CallNextHookEx(hook: *mut c_void, code: i32, wparam: usize, lparam: isize) -> isize;
        fn UnhookWindowsHookEx(hook: *mut c_void) -> i32;
        fn GetKeyboardState(state: *mut u8) -> i32;
        fn GetKeyboardLayout(thread: u32) -> *mut c_void;
        fn ToUnicodeEx(
            key: u32,
            scan: u32,
            state: *const u8,
            text: *mut u16,
            length: i32,
            flags: u32,
            layout: *mut c_void,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadId() -> u32;
    }
    struct Hook(*mut c_void);
    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: this is the live hook returned by SetWindowsHookExW on this thread.
            unsafe {
                UnhookWindowsHookEx(self.0);
            }
        }
    }
    thread_local! {
        static HOOK:RefCell<Option<Hook>>=const {RefCell::new(None)};
        static KEY:Cell<Option<(usize,u16)>>=const {Cell::new(None)};
    }
    unsafe extern "system" fn observe(code: i32, wparam: usize, lparam: isize) -> isize {
        if code >= 0 {
            // SAFETY: WH_CALLWNDPROC supplies CWPSTRUCT for nonnegative hook codes.
            let message = unsafe { &*(lparam as *const Message) };
            if matches!(message.message, 0x0100 | 0x0104) {
                KEY.with(|key| {
                    key.set(Some((
                        message.wparam,
                        ((message.lparam >> 16) & 0x1ff) as u16,
                    )))
                });
            }
        }
        // SAFETY: every observed message must continue through the hook chain unchanged.
        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }
    pub fn install() {
        HOOK.with(|slot| {
            if slot.borrow().is_some() {
                return;
            }
            // SAFETY: the callback lives for the program and the hook is restricted to this thread.
            let handle = unsafe {
                SetWindowsHookExW(4, observe, std::ptr::null_mut(), GetCurrentThreadId())
            };
            if !handle.is_null() {
                *slot.borrow_mut() = Some(Hook(handle));
            }
        });
    }
    pub fn take() -> Option<super::Native> {
        let (virtual_key, scan) = KEY.with(|k| k.take())?;
        let keypad = match virtual_key {
            0x60..=0x69 => Some(format!("NumPad{}", virtual_key - 0x60)),
            0x6a => Some("NumPad*".into()),
            0x6b => Some("NumPad+".into()),
            0x6d => Some("NumPad-".into()),
            0x6e => Some("NumPad.".into()),
            0x6f => Some("NumPad/".into()),
            _ => None,
        };
        let physical = super::scan_name(scan).map(|s| format!("code:{s}"));
        let mut state = [0u8; 256];
        let mut text = [0u16; 8];
        // SAFETY: buffers have the sizes required by the APIs and stay live for each call.
        // Flag 4 prevents ToUnicodeEx from changing the keyboard's dead-key state.
        let character = unsafe {
            if GetKeyboardState(state.as_mut_ptr()) != 0 {
                for index in [0x11, 0x12, 0xa2, 0xa3, 0xa4, 0xa5] {
                    state[index] = 0;
                }
                let count = ToUnicodeEx(
                    virtual_key as u32,
                    u32::from(scan),
                    state.as_ptr(),
                    text.as_mut_ptr(),
                    text.len() as i32,
                    4,
                    GetKeyboardLayout(0),
                );
                (count == 1).then(|| String::from_utf16_lossy(&text[..1]))
            } else {
                None
            }
        };
        Some(super::Native {
            key: keypad,
            code: physical,
            character,
        })
    }
}

#[cfg(windows)]
pub use windows::{install, take};
#[cfg(not(windows))]
pub fn install() {}
#[cfg(not(windows))]
pub fn take() -> Option<Native> {
    None
}

pub fn scan_name(scan: u16) -> Option<&'static str> {
    Some(match scan {
        0x02 => "Digit1",
        0x03 => "Digit2",
        0x04 => "Digit3",
        0x05 => "Digit4",
        0x06 => "Digit5",
        0x07 => "Digit6",
        0x08 => "Digit7",
        0x09 => "Digit8",
        0x0a => "Digit9",
        0x0b => "Digit0",
        0x0c => "Minus",
        0x0d => "Equal",
        0x10 => "KeyQ",
        0x11 => "KeyW",
        0x12 => "KeyE",
        0x13 => "KeyR",
        0x14 => "KeyT",
        0x15 => "KeyY",
        0x16 => "KeyU",
        0x17 => "KeyI",
        0x18 => "KeyO",
        0x19 => "KeyP",
        0x1a => "BracketLeft",
        0x1b => "BracketRight",
        0x1e => "KeyA",
        0x1f => "KeyS",
        0x20 => "KeyD",
        0x21 => "KeyF",
        0x22 => "KeyG",
        0x23 => "KeyH",
        0x24 => "KeyJ",
        0x25 => "KeyK",
        0x26 => "KeyL",
        0x27 => "Semicolon",
        0x28 => "Quote",
        0x29 => "Backquote",
        0x2b => "Backslash",
        0x2c => "KeyZ",
        0x2d => "KeyX",
        0x2e => "KeyC",
        0x2f => "KeyV",
        0x30 => "KeyB",
        0x31 => "KeyN",
        0x32 => "KeyM",
        0x33 => "Comma",
        0x34 => "Period",
        0x35 => "Slash",
        0x73 => "IntlRo",
        0x7d => "IntlYen",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn physical_position_survives_layout_characters() {
        assert_eq!(super::scan_name(0x35), Some("Slash"));
        assert_eq!(super::scan_name(0x0d), Some("Equal"));
        assert_eq!(super::scan_name(0x7d), Some("IntlYen"));
        assert_eq!(super::scan_name(0x135), None); // keypad Divide is distinct
    }
}
