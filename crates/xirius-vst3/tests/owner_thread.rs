//! SDK getState is held at an event gate, never by a timing assumption. No editor,
//! sound device, foreground window or OS input is used.
#![cfg(windows)]

use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{CreateEventW, SetEvent, WaitForSingleObject};
use xirius_vst3::owner::{Controller, Open, Ticket};

struct Allocator;
thread_local! {
    static RT: std::cell::Cell<(bool, usize, usize)> = const { std::cell::Cell::new((false, 0, 0)) };
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

// SAFETY: every operation delegates to System with the original pointer/layout;
// the thread-local counters observe only the explicit lease-return scope.
unsafe impl std::alloc::GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        RT.with(|counts| {
            let (on, alloc, free) = counts.get();
            if on {
                counts.set((on, alloc + 1, free));
            }
        });
        // SAFETY: forwarding the caller's allocation contract unchanged.
        unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        RT.with(|counts| {
            let (on, alloc, free) = counts.get();
            if on {
                counts.set((on, alloc, free + 1));
            }
        });
        // SAFETY: forwarding the matching pointer/layout unchanged.
        unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, pointer, layout) }
    }
}

struct Event(HANDLE);
impl Event {
    fn new() -> Self {
        // SAFETY: an unnamed manual reset event, owned until Drop.
        let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        assert!(!h.is_null());
        Self(h)
    }
    fn set(&self) {
        // SAFETY: a live handle owned by self.
        assert_ne!(unsafe { SetEvent(self.0) }, 0);
    }
    fn wait(&self) {
        // SAFETY: a live event; timeout is only a deadlock watchdog.
        assert_eq!(
            unsafe { WaitForSingleObject(self.0, 10_000) },
            WAIT_OBJECT_0
        );
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: releases the one handle we own.
        unsafe { CloseHandle(self.0) };
    }
}

fn wait<T>(ticket: &Ticket<T>) -> Result<T, String> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = ticket.poll() {
            return result;
        }
        assert!(Instant::now() < end, "owner reply watchdog");
        std::thread::yield_now();
    }
}

#[test]
fn state_requests_cancel_without_waiting_and_the_audio_returns_before_termination() {
    xirius_test_vst3::fixture_marker();
    let dll = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius_test_vst3.dll");
    let open = || Open {
        bundle: dll.clone(),
        class: xirius_test_vst3::CID.map(|b| b as u8),
        state: None,
        params: Vec::new(),
        requires_process_main: false,
        sample_rate: 48000.0,
        max_block: 512,
        offline: false,
    };
    let opened = wait(&Controller::open(open()).unwrap()).unwrap();
    let wide: Vec<_> = dll.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: the loaded fixture exports these exact system ABI functions. Its
    // module stays loaded until process exit; all event handles outlive the call.
    let (gate, terminations, away) = unsafe {
        let h = GetModuleHandleW(wide.as_ptr());
        let gate: unsafe extern "system" fn(usize, usize) =
            std::mem::transmute(GetProcAddress(h, c"FixtureStateGate".as_ptr().cast()).unwrap());
        let terminations: unsafe extern "system" fn() -> u64 =
            std::mem::transmute(GetProcAddress(h, c"FixtureTerminations".as_ptr().cast()).unwrap());
        let away: unsafe extern "system" fn() -> u32 =
            std::mem::transmute(GetProcAddress(h, c"FixtureStateAway".as_ptr().cast()).unwrap());
        (gate, terminations, away)
    };
    let entered = Event::new();
    let release = Event::new();
    // SAFETY: handles live through the state completion below.
    unsafe { gate(entered.0 as usize, release.0 as usize) };
    let capture = opened.controller.capture().unwrap();
    entered.wait();
    assert!(capture.poll().is_none());
    capture.cancel();
    assert!(capture.poll().unwrap().is_err());
    assert!(
        capture.poll().is_none(),
        "a cancelled ticket answers only once"
    );
    // Cancel/drop and a second request return while the first SDK call is held.
    drop(capture);
    let second = opened.controller.capture().unwrap();
    assert!(second.poll().is_none());
    let third = opened.controller.capture().unwrap();
    third.cancel();
    drop(third);
    release.set();
    assert!(wait(&second).is_ok());
    assert!(
        second.poll().is_none(),
        "a consumed reply is not an owner failure"
    );
    // SAFETY: both active state requests have returned; disarm the gate.
    unsafe { gate(0, 0) };
    // A new request has its own reply, unaffected by the abandoned first one.
    assert!(wait(&opened.controller.capture().unwrap()).is_ok());
    let retirement = opened.retirement.clone();
    // SAFETY: plain fixture counters, no other instance in this executable.
    let before = unsafe { terminations() };
    drop(opened.controller);
    drop(opened.sender);
    assert!(!retirement.finished(), "audio lease still holds the owner");
    assert!(!retirement.audio_returned());
    assert_eq!(unsafe { terminations() }, before);
    let counts = std::thread::spawn(move || {
        RT.with(|counts| counts.set((true, 0, 0)));
        drop(opened.audio);
        RT.with(|counts| {
            let (_, alloc, free) = counts.replace((false, 0, 0));
            (alloc, free)
        })
    })
    .join()
    .unwrap();
    assert_eq!(
        counts,
        (0, 0),
        "returning the audio lease allocates/frees nothing"
    );
    assert!(retirement.audio_returned());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !retirement.finished() {
        assert!(Instant::now() < deadline, "retirement watchdog");
        std::thread::yield_now();
    }
    let after = unsafe { terminations() };
    assert_eq!(after as u32, before as u32 + 1);
    assert_eq!(after >> 32, before >> 32, "terminate on creation thread");
    assert_eq!(
        unsafe { away() },
        0,
        "both component and controller getState stay on owner"
    );

    let mut main_only = open();
    main_only.requires_process_main = true;
    assert!(
        wait(&Controller::open(main_only).unwrap())
            .err()
            .unwrap()
            .contains("別プロセス")
    );
}
