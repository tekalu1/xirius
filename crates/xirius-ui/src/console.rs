//! The program is a GUI program on Windows (subsystem "windows"), so it has no console of its own.
//! Where the start gave it no standard output or error (a terminal started it without a pipe or
//! a file), join the terminal that started it. A pipe or a file handed over is left as it is.

#[cfg(windows)]
pub fn attach() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };

    let missing = |which| {
        // SAFETY: GetStdHandle only reads the process's handle table.
        let h = unsafe { GetStdHandle(which) };
        h.is_null() || h as isize == -1
    };
    let (out, err) = (missing(STD_OUTPUT_HANDLE), missing(STD_ERROR_HANDLE));
    if !(out || err) {
        return;
    }
    // SAFETY: no arguments to get wrong; fails (returns 0) when the parent has no console.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") else {
        return;
    };
    // The handle stays open for the life of the process, as the standard handles do.
    let handle = file.into_raw_handle();
    for (wanted, which) in [(out, STD_OUTPUT_HANDLE), (err, STD_ERROR_HANDLE)] {
        if wanted {
            // SAFETY: `handle` is a valid, open console output handle.
            unsafe { SetStdHandle(which, handle as _) };
        }
    }
}

#[cfg(not(windows))]
pub fn attach() {}
