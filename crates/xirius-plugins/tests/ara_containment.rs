//! PLG-039 / PLG-058: can a crash inside an in-process ARA plug-in be contained?
//!
//! ARA has to run in the host's process (PLG-015), so the containment is an exception filter in
//! the host, not a process boundary. This test stands in for the plug-in's call with a thread that
//! faults on purpose, and checks that the host keeps running and learns which call died.
//! `docs/ara-host.md` says what this does and does not cover.
#![cfg(all(windows, target_arch = "x86_64"))]
#![allow(clippy::missing_safety_doc)]

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

type Handler = unsafe extern "system" fn(*mut ExceptionPointers) -> i32;
type ThreadMain = unsafe extern "system" fn(*mut c_void) -> u32;

unsafe extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: Handler) -> *mut c_void;
    fn ExitThread(code: u32) -> !;
    fn CreateThread(
        attrs: *mut c_void,
        stack: usize,
        start: ThreadMain,
        param: *mut c_void,
        flags: u32,
        id: *mut u32,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
    fn GetExitCodeThread(handle: *mut c_void, code: *mut u32) -> i32;
    fn TerminateThread(handle: *mut c_void, code: u32) -> i32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

const CONTINUE_SEARCH: i32 = 0;
const ACCESS_VIOLATION: u32 = 0xC000_0005;
const ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
const CONTAINED: u32 = 0xDEAD;
const WAIT_TIMEOUT: u32 = 0x102;

thread_local! {
    /// Set by the host around a call into the plug-in, on the thread that makes the call.
    static IN_PLUGIN: Cell<bool> = const { Cell::new(false) };
}

/// The last fault the filter contained, so the host can mark the plug-in as crashed.
static FAULT: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn filter(info: *mut ExceptionPointers) -> i32 {
    // SAFETY: Windows passes a valid pointer pair to a vectored handler.
    let code = unsafe { (*(*info).record).code };
    let ours = matches!(code, ACCESS_VIOLATION | ILLEGAL_INSTRUCTION);
    if ours && IN_PLUGIN.with(Cell::get) {
        FAULT.store(code, Ordering::SeqCst);
        // SAFETY: only the faulting plug-in call's thread ends; the host's threads go on.
        unsafe { ExitThread(CONTAINED) }
    }
    CONTINUE_SEARCH
}

/// What a call into the plug-in does in these tests.
#[derive(Clone, Copy)]
enum Call {
    WriteNull,
    Illegal,
    Hang,
    Fine,
}

unsafe extern "system" fn plugin_call(param: *mut c_void) -> u32 {
    IN_PLUGIN.with(|f| f.set(true));
    match param as usize {
        0 => {
            // SAFETY: this is the fault the test is about.
            unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
            0
        }
        1 => {
            // SAFETY: an undefined instruction, to fault on purpose.
            unsafe { std::arch::asm!("ud2") };
            0
        }
        2 => loop {
            std::hint::spin_loop();
        },
        _ => 7,
    }
}

/// Runs one plug-in call on its own thread. `None` means it did not come back in time.
fn run(call: Call, millis: u32) -> Option<u32> {
    let handle = unsafe {
        CreateThread(
            std::ptr::null_mut(),
            0,
            plugin_call,
            call as usize as *mut c_void,
            0,
            std::ptr::null_mut(),
        )
    };
    assert!(!handle.is_null());
    let waited = unsafe { WaitForSingleObject(handle, millis) };
    let result = if waited == WAIT_TIMEOUT {
        // The host gives up on a call that hangs and ends its thread.
        unsafe { TerminateThread(handle, CONTAINED) };
        None
    } else {
        let mut code = 0;
        unsafe { GetExitCodeThread(handle, &mut code) };
        Some(code)
    };
    unsafe { CloseHandle(handle) };
    result
}

#[test]
fn a_fault_inside_a_plugin_call_ends_that_call_and_not_the_host() {
    unsafe { AddVectoredExceptionHandler(1, filter) };

    // A normal call comes back with its own result.
    assert_eq!(run(Call::Fine, 2000), Some(7));
    assert_eq!(FAULT.load(Ordering::SeqCst), 0);

    // A bad write: the call ends, the filter names the fault, and this test goes on.
    assert_eq!(run(Call::WriteNull, 2000), Some(CONTAINED));
    assert_eq!(FAULT.load(Ordering::SeqCst), ACCESS_VIOLATION);

    // An undefined instruction is contained the same way.
    assert_eq!(run(Call::Illegal, 2000), Some(CONTAINED));
    assert_eq!(FAULT.load(Ordering::SeqCst), ILLEGAL_INSTRUCTION);

    // A call that never returns is found by the time limit, not by the filter.
    FAULT.store(0, Ordering::SeqCst);
    assert_eq!(run(Call::Hang, 150), None);
    assert_eq!(FAULT.load(Ordering::SeqCst), 0);

    // After all of that the host still runs calls.
    assert_eq!(run(Call::Fine, 2000), Some(7));
}
