//! Whether Shift is held while files from another app are dragged over the window (N4-kit-2).
//! GPUI hands an OS file drag on as mouse moves with no keys, and the window's own key state
//! stays as it was while the other app has the keyboard: the key is read from the OS. It only
//! reads; it never sends input.

#[cfg(test)]
thread_local! {
    static HELD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// True while a Shift key is down. On Windows the OS says it (what the window last saw may be
/// from before the drag); elsewhere `seen`, what the event said, counts too.
pub fn held(seen: bool) -> bool {
    if cfg!(windows) { os() } else { seen || os() }
}

/// The tests' Shift, in place of the OS's: they never read the keyboard of the person at
/// the PC. Let go when it drops, so a test that fails holding it leaves none to the next on
/// its thread (the fifth review's low 12). It drops back to what it found, so one held
/// inside another leaves the outer one's (the sixth review's low 6).
#[cfg(test)]
#[must_use = "Shift is let go as soon as this drops"]
pub fn hold() -> Hold {
    Hold {
        was: HELD.with(std::cell::Cell::get),
    }
}

#[cfg(test)]
#[must_use = "Shift is let go as soon as this drops"]
pub struct Hold {
    was: bool,
}

#[cfg(test)]
impl Hold {
    pub fn set(&self, down: bool) {
        HELD.with(|h| h.set(down));
    }
}

#[cfg(test)]
impl Drop for Hold {
    fn drop(&mut self) {
        HELD.with(|h| h.set(self.was));
    }
}

#[cfg(test)]
#[test]
fn the_tests_shift_lets_go_as_it_drops() {
    {
        let shift = hold();
        shift.set(true);
        assert!(held(false));
        {
            let inner = hold();
            inner.set(false);
            assert!(!held(false));
        }
        assert!(held(false), "the inner one let the outer go");
    }
    assert!(!held(false));
}

#[cfg(test)]
fn os() -> bool {
    HELD.with(std::cell::Cell::get)
}

#[cfg(all(windows, not(test)))]
fn os() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetAsyncKeyState(key: i32) -> i16;
    }
    const VK_SHIFT: i32 = 0x10;
    // SAFETY: a read of the key's state as the OS has it now (the window's own state is stale
    // while the drag's source has the keyboard); no input is sent.
    unsafe { GetAsyncKeyState(VK_SHIFT) < 0 }
}

#[cfg(all(not(windows), not(test)))]
fn os() -> bool {
    false
}
