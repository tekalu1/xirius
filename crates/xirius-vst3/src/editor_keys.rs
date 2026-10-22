//! Playback keys on the editor's frame; text controls keep their own keyboard input.
use std::cell::Cell;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetPropW, RemovePropW, SetPropW, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS,
};

fn property() -> &'static [u16] {
    static NAME: OnceLock<Vec<u16>> = OnceLock::new();
    NAME.get_or_init(|| {
        "Xirius.EditorPlaybackKeys"
            .encode_utf16()
            .chain([0])
            .collect()
    })
}

#[derive(Default)]
pub(crate) struct Keys {
    interactive: bool,
    modifiers: Cell<u16>,
    pending: Cell<u32>,
}

impl Keys {
    pub fn install(hwnd: HWND, interactive: bool) -> Option<Box<Self>> {
        let keys = Box::new(Self {
            interactive,
            ..Default::default()
        });
        // SAFETY: Editor retains this stable allocation until it removes the property.
        let ok = unsafe {
            SetPropW(
                hwnd,
                property().as_ptr(),
                (&*keys as *const Self).cast_mut().cast(),
            )
        };
        (ok != 0).then_some(keys)
    }
    pub fn remove(hwnd: HWND) {
        // SAFETY: the property belongs to our editor frame, and its allocation is still alive.
        unsafe {
            RemovePropW(hwnd, property().as_ptr());
        }
    }
    pub fn take(&self) -> u32 {
        self.pending.replace(0)
    }

    fn receive(&self, msg: u32, key: usize, lp: LPARAM, current_modifiers: Option<bool>) -> bool {
        if msg == WM_KILLFOCUS {
            self.modifiers.set(0);
            return false;
        }
        let mask = match key {
            0x10 => 1,
            0x11 => 2,
            0x12 => 4,
            0xa0 => 8,
            0xa1 => 16,
            0xa2 => 32,
            0xa3 => 64,
            0xa4 => 128,
            0xa5 => 256,
            0x5b => 512,
            0x5c => 1024,
            _ => 0,
        };
        if msg == WM_KEYUP {
            self.modifiers.set(self.modifiers.get() & !mask);
            return false;
        }
        if msg != WM_KEYDOWN {
            return false;
        }
        self.modifiers.set(self.modifiers.get() | mask);
        if key != 0x20 || current_modifiers.unwrap_or(self.modifiers.get() != 0) {
            return false;
        }
        if lp & (1 << 30) == 0 {
            self.pending
                .set(self.pending.get().saturating_add(1).min(32));
        }
        true
    }
}

pub(crate) fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> bool {
    if !matches!(msg, WM_KEYDOWN | WM_KEYUP | WM_KILLFOCUS) {
        return false;
    }
    // SAFETY: only Editor installs this property; Win32 dispatches on its owning GUI thread.
    let ptr = unsafe { GetPropW(hwnd, property().as_ptr()) }.cast::<Keys>();
    if ptr.is_null() {
        return false;
    }
    // SAFETY: the installed Box stays alive on this GUI thread. Interactive windows use
    // that thread's current keyboard state; inactive tools use their explicit messages.
    let keys = unsafe { &*ptr };
    let modified = keys.interactive.then(|| {
        [0x10, 0x11, 0x12, 0x5b, 0x5c].into_iter().any(|key| {
            // SAFETY: a GUI-thread key-state query, without sending input or querying async state.
            unsafe { GetKeyState(key) < 0 }
        })
    });
    keys.receive(msg, wp, lp, modified)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_plain_space_on_the_frame_is_forwarded_once_per_press() {
        let keys = Keys::default();
        assert!(!keys.receive(WM_KEYDOWN, 0x41, 1, None));
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1, None));
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1 | (1 << 30), None));
        assert_eq!(keys.take(), 1);
        keys.receive(WM_KEYDOWN, 0x11, 1, None);
        assert!(!keys.receive(WM_KEYDOWN, 0x20, 1, None));
        keys.receive(WM_KEYUP, 0x11, 1, None);
        keys.receive(WM_KEYDOWN, 0xa3, 1, None);
        assert!(!keys.receive(WM_KEYDOWN, 0x20, 1, None));
        keys.receive(WM_KEYUP, 0xa3, 1, None);
        keys.receive(WM_KEYDOWN, 0x10, 1, None);
        assert!(!keys.receive(WM_KEYDOWN, 0x20, 1, None));
        keys.receive(WM_KILLFOCUS, 0, 0, None);
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1, None));
        assert_eq!(keys.take(), 1);
        assert_eq!(keys.take(), 0);
    }
    #[test]
    fn the_space_an_input_method_takes_is_not_a_play_key() {
        // While a conversion is open the system hands the Space over as VK_PROCESSKEY.
        let keys = Keys::default();
        assert!(!keys.receive(WM_KEYDOWN, 0xe5, 1, None));
        assert_eq!(keys.take(), 0);
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1, None));
        assert_eq!(keys.take(), 1);
    }
    #[test]
    fn the_current_keyboard_state_overrides_frame_history_after_focus_changes() {
        let keys = Keys::default();
        keys.receive(WM_KILLFOCUS, 0, 0, None);
        assert!(!keys.receive(WM_KEYDOWN, 0x20, 1, Some(true)));
        assert_eq!(keys.take(), 0);
        keys.receive(WM_KEYDOWN, 0x11, 1, None);
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1, Some(false)));
        assert_eq!(keys.take(), 1);
    }
    #[test]
    fn inactive_frames_keep_the_other_control_key_when_one_is_released() {
        let keys = Keys::default();
        keys.receive(WM_KEYDOWN, 0xa2, 1, None);
        keys.receive(WM_KEYDOWN, 0xa3, 1, None);
        keys.receive(WM_KEYUP, 0xa2, 1, None);
        assert!(!keys.receive(WM_KEYDOWN, 0x20, 1, None));
        assert_eq!(keys.take(), 0);
        keys.receive(WM_KEYUP, 0xa3, 1, None);
        assert!(keys.receive(WM_KEYDOWN, 0x20, 1, None));
        assert_eq!(keys.take(), 1);
    }
}
