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
/// the PC.
#[cfg(test)]
pub fn hold(down: bool) {
    HELD.with(|h| h.set(down));
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
