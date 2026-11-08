//! Keeping a fault inside an in-process ARA plugin from ending the app (PLG-058).
//!
//! `docs/ara-host.md` (section 2) has the design and its limits. The host calls the plugin
//! only from its ARA thread ([`crate::service`]), marked with [`enter`] around each call. A
//! fault that nothing in the plugin handles reaches Windows's last-chance filter; there,
//! [`last_chance`] ends the faulting thread alone when it is a marked thread, or a thread
//! running inside a plugin module that was registered with [`add_module`] (the plugin's own
//! analysis threads). The faults are counted, in all ([`faults`]) and by module
//! ([`faults_in`]), and the service marks the plugin as crashed: a fault on its ARA thread is
//! seen by the [`Watch`], one on a thread of the plugin's own by the module's count. A call
//! that does not come back in time is found by the [`Watch`] and its thread is ended.
//!
//! The decision is taken at the last chance, not first: plugins may catch their own access
//! violations (a copy protection that probes memory), and a first-chance handler would end
//! those threads.
//!
//! Not contained: a write that corrupts the heap, a thread ended while it holds a lock the
//! app needs, and a stack overflow.

use std::cell::Cell;
use std::sync::Arc;
#[cfg(windows)]
use std::sync::Once;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
#[cfg(windows)]
use windows_sys::Win32::System::Diagnostics::Debug::{
    EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    ExitThread, GetCurrentThreadId, OpenThread, THREAD_SYNCHRONIZE, THREAD_TERMINATE,
    TerminateThread, WaitForSingleObject,
};

/// The exit code of a thread this module ended.
pub const CONTAINED: u32 = 0xA7A0_0DEA;

const ACCESS_VIOLATION: u32 = 0xC000_0005;
const ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;
const PRIVILEGED_INSTRUCTION: u32 = 0xC000_0096;
const INTEGER_DIVIDE_BY_ZERO: u32 = 0xC000_0094;
const IN_PAGE_ERROR: u32 = 0xC000_0006;
#[cfg(windows)]
const CONTINUE_SEARCH: i32 = 0;

thread_local! {
    /// How deep this thread is in calls into a plugin.
    static IN_PLUGIN: Cell<u32> = const { Cell::new(0) };
    /// The watch of this thread's calls, if it has one.
    static WATCH: Cell<Option<&'static WatchState>> = const { Cell::new(None) };
}

/// Plugin modules' address ranges, `[start, end)`, as pairs. A zero start is a free slot.
const SLOTS: usize = 32;
static RANGES: [AtomicUsize; SLOTS * 2] = [const { AtomicUsize::new(0) }; SLOTS * 2];
/// The faults contained in each range's code.
static RANGE_FAULTS: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];
static FAULTS: AtomicU64 = AtomicU64::new(0);
static LAST_CODE: AtomicU32 = AtomicU32::new(0);

/// How many faults were contained in this process.
pub fn faults() -> u64 {
    FAULTS.load(Ordering::Acquire)
}

/// The exception code of the last contained fault.
pub fn last_code() -> u32 {
    LAST_CODE.load(Ordering::Acquire)
}

/// A sentence for a contained fault's code.
pub fn describe(code: u32) -> &'static str {
    match code {
        ACCESS_VIOLATION => "access violation",
        ILLEGAL_INSTRUCTION => "illegal instruction",
        PRIVILEGED_INSTRUCTION => "privileged instruction",
        INTEGER_DIVIDE_BY_ZERO => "integer division by zero",
        IN_PAGE_ERROR => "in-page error",
        _ => "fault",
    }
}

/// Marks this thread as inside the plugin until dropped. Nested calls (the plugin calling the
/// host calling the plugin) keep the mark.
pub struct Call {
    watched: bool,
}

pub fn enter() -> Call {
    let depth = IN_PLUGIN.with(|d| {
        let n = d.get() + 1;
        d.set(n);
        n
    });
    let watched = depth == 1
        && WATCH.with(|w| match w.get() {
            Some(s) => {
                s.started.store(s.now(), Ordering::Release);
                true
            }
            None => false,
        });
    Call { watched }
}

impl Drop for Call {
    fn drop(&mut self) {
        IN_PLUGIN.with(|d| d.set(d.get().saturating_sub(1)));
        if self.watched {
            WATCH.with(|w| {
                if let Some(s) = w.get() {
                    s.started.store(0, Ordering::Release);
                }
            });
        }
    }
}

/// Runs `f` marked as inside the plugin.
pub fn call<R>(f: impl FnOnce() -> R) -> R {
    let _c = enter();
    f()
}

/// The slot of the registered range that holds `address`.
fn slot_of(address: usize) -> Option<usize> {
    (0..SLOTS).find(|&i| {
        let start = RANGES[i * 2].load(Ordering::Acquire);
        let end = RANGES[i * 2 + 1].load(Ordering::Acquire);
        start != 0 && (start..end).contains(&address)
    })
}

/// Whether `address` is inside a registered plugin module.
pub fn in_module(address: usize) -> bool {
    slot_of(address).is_some()
}

/// How many faults were contained in the code of the registered module that holds `address`
/// (on any thread: the plugin's own ones too). 0 when it is not registered.
pub fn faults_in(address: usize) -> u64 {
    slot_of(address).map_or(0, |i| RANGE_FAULTS[i].load(Ordering::Acquire))
}

/// A handle to this thread, to see later whether it was ended ([`thread_ended`]); 0 when none
/// could be had. [`close_thread`] lets it go.
pub fn this_thread() -> usize {
    #[cfg(windows)]
    // SAFETY: opens this thread's own handle, for waiting only.
    unsafe {
        OpenThread(THREAD_SYNCHRONIZE, 0, GetCurrentThreadId()) as usize
    }
    #[cfg(not(windows))]
    0
}

/// Whether the thread of a handle from [`this_thread`] has ended (a fault on it was contained,
/// or the watch ended it). False for 0.
pub fn thread_ended(thread: usize) -> bool {
    #[cfg(windows)]
    if thread != 0 {
        // SAFETY: a handle from `this_thread`, not closed yet.
        return unsafe { WaitForSingleObject(thread as HANDLE, 0) } == WAIT_OBJECT_0;
    }
    let _ = thread;
    false
}

/// Lets go a handle from [`this_thread`].
pub fn close_thread(thread: usize) {
    #[cfg(windows)]
    if thread != 0 {
        // SAFETY: a handle from `this_thread`, closed once.
        unsafe { CloseHandle(thread as HANDLE) };
    }
    let _ = thread;
}

/// Registers the address range `[start, end)` of plugin code. Returns false when the table is
/// full; a range already there is not added twice.
pub fn add_range(start: usize, end: usize) -> bool {
    if start == 0 || end <= start {
        return false;
    }
    for i in 0..SLOTS {
        if RANGES[i * 2].load(Ordering::Acquire) == start {
            return true;
        }
    }
    for i in 0..SLOTS {
        if RANGES[i * 2]
            .compare_exchange(0, start, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            RANGES[i * 2 + 1].store(end, Ordering::Release);
            return true;
        }
    }
    false
}

/// Registers the loaded module that contains `address` (any function of the plugin).
#[cfg(windows)]
pub fn add_module(address: usize) -> bool {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleHandleExW,
    };
    let mut module = std::ptr::null_mut();
    // SAFETY: asks for the module of an address, without changing its reference count.
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address as *const u16,
            &mut module,
        )
    };
    if ok == 0 || module.is_null() {
        return false;
    }
    let base = module as usize;
    // SAFETY: a loaded module starts with its PE headers; `e_lfanew` is at 0x3C, and
    // `SizeOfImage` is 0x50 into the NT headers (the same in PE32 and PE32+).
    let size = unsafe {
        let lfanew = std::ptr::read_unaligned((base + 0x3C) as *const i32);
        std::ptr::read_unaligned((base + lfanew as usize + 0x50) as *const u32) as usize
    };
    add_range(base, base + size)
}

#[cfg(not(windows))]
pub fn add_module(_address: usize) -> bool {
    false
}

/// The filter for Windows's last chance. Ends the faulting thread when the fault is the
/// plugin's; otherwise the search goes on (to the crash report).
///
/// # Safety
/// Called by Windows (or by another last-chance filter) with the exception's pointers.
#[cfg(windows)]
pub unsafe extern "system" fn last_chance(info: *const EXCEPTION_POINTERS) -> i32 {
    if info.is_null() {
        return CONTINUE_SEARCH;
    }
    // SAFETY: Windows passes valid pointers for the duration of the call.
    let (code, address) = unsafe {
        let r = (*info).ExceptionRecord;
        if r.is_null() {
            return CONTINUE_SEARCH;
        }
        ((*r).ExceptionCode as u32, (*r).ExceptionAddress as usize)
    };
    let ours = matches!(
        code,
        ACCESS_VIOLATION
            | ILLEGAL_INSTRUCTION
            | PRIVILEGED_INSTRUCTION
            | INTEGER_DIVIDE_BY_ZERO
            | IN_PAGE_ERROR
    );
    let slot = slot_of(address);
    if ours && (IN_PLUGIN.with(Cell::get) > 0 || slot.is_some()) {
        LAST_CODE.store(code, Ordering::Release);
        if let Some(i) = slot {
            RANGE_FAULTS[i].fetch_add(1, Ordering::AcqRel);
        }
        FAULTS.fetch_add(1, Ordering::AcqRel);
        // SAFETY: only this thread ends; the app's threads go on.
        unsafe { ExitThread(CONTAINED) }
    }
    CONTINUE_SEARCH
}

#[cfg(windows)]
type Filter = unsafe extern "system" fn(*const EXCEPTION_POINTERS) -> i32;
#[cfg(windows)]
static PREVIOUS: AtomicUsize = AtomicUsize::new(0);

#[cfg(windows)]
unsafe extern "system" fn own_filter(info: *const EXCEPTION_POINTERS) -> i32 {
    // SAFETY: forwarded from Windows.
    let r = unsafe { last_chance(info) };
    let previous = PREVIOUS.load(Ordering::Acquire);
    if previous != 0 {
        // SAFETY: the filter that was there before, saved by `install_own_filter`.
        let f: Filter = unsafe { std::mem::transmute::<usize, Filter>(previous) };
        // SAFETY: as above.
        return unsafe { f(info) };
    }
    r
}

/// Makes [`last_chance`] the process's last-chance filter, passing other faults on to the
/// filter that was there. For a process without a crash report of its own (tests, tools); the
/// app calls [`last_chance`] from its own filter instead.
#[cfg(windows)]
pub fn install_own_filter() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: process-wide; the previous filter is kept and called.
        let previous = unsafe { SetUnhandledExceptionFilter(Some(own_filter)) };
        if let Some(p) = previous {
            PREVIOUS.store(p as usize, Ordering::Release);
        }
    });
}

#[cfg(not(windows))]
pub fn install_own_filter() {}

/// Watches the calls of one thread: a call longer than the limit has its thread ended.
pub struct Watch {
    state: &'static WatchState,
}

struct WatchState {
    /// Milliseconds since `origin` + 1 when the current call started; 0 when not in a call.
    started: AtomicU64,
    limit_ms: AtomicU64,
    origin: Instant,
    #[cfg(windows)]
    thread: AtomicUsize,
    #[cfg(windows)]
    thread_id: AtomicU32,
}

impl WatchState {
    fn now(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64 + 1
    }
}

/// What the watch found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// The thread runs (or waits between calls).
    Alive,
    /// The thread ended, by a contained fault or otherwise.
    Ended,
    /// A call ran past the limit; the thread was ended.
    Hung,
}

impl Watch {
    /// A watch with calls limited to `limit`. Leaked on purpose: the thread it watches may
    /// be ended without running destructors.
    pub fn new(limit: Duration) -> Arc<Watch> {
        let state = Box::leak(Box::new(WatchState {
            started: AtomicU64::new(0),
            limit_ms: AtomicU64::new(limit.as_millis() as u64),
            origin: Instant::now(),
            #[cfg(windows)]
            thread: AtomicUsize::new(0),
            #[cfg(windows)]
            thread_id: AtomicU32::new(0),
        }));
        Arc::new(Watch { state })
    }

    /// Makes the calling thread the watched one. Call it first on that thread.
    pub fn attach(&self) {
        WATCH.with(|w| w.set(Some(self.state)));
        #[cfg(windows)]
        // SAFETY: opens a handle to this thread for waiting and ending it; closed in `Drop`.
        unsafe {
            let id = GetCurrentThreadId();
            let h = OpenThread(THREAD_SYNCHRONIZE | THREAD_TERMINATE, 0, id);
            self.state.thread.store(h as usize, Ordering::Release);
            self.state.thread_id.store(id, Ordering::Release);
        }
    }

    /// Changes the limit (an analysis started by a call may take longer than a plain call).
    pub fn set_limit(&self, limit: Duration) {
        self.state
            .limit_ms
            .store(limit.as_millis() as u64, Ordering::Release);
    }

    /// Looks at the thread. Ends it when a call has run past the limit.
    pub fn check(&self) -> Seen {
        #[cfg(windows)]
        {
            let h = self.state.thread.load(Ordering::Acquire) as HANDLE;
            if h.is_null() {
                return Seen::Alive;
            }
            // SAFETY: a handle opened in `attach`.
            if unsafe { WaitForSingleObject(h, 0) } == WAIT_OBJECT_0 {
                return Seen::Ended;
            }
            let started = self.state.started.load(Ordering::Acquire);
            let limit = self.state.limit_ms.load(Ordering::Acquire);
            if started != 0 && self.state.now().saturating_sub(started) > limit {
                // SAFETY: the thread is stuck in the plugin; nothing of the host's runs on it.
                unsafe {
                    TerminateThread(h, CONTAINED);
                    WaitForSingleObject(h, 1000);
                }
                return Seen::Hung;
            }
        }
        Seen::Alive
    }

    /// Whether the watched thread is in a call into the plugin now.
    pub fn busy(&self) -> bool {
        self.state.started.load(Ordering::Acquire) != 0
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let h = self.state.thread.swap(0, Ordering::AcqRel) as HANDLE;
            if !h.is_null() {
                // SAFETY: the handle from `attach`.
                unsafe { CloseHandle(h) };
            }
        }
    }
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_added_once_and_found() {
        assert!(add_range(0x7000_0000_0000, 0x7000_0001_0000));
        assert!(add_range(0x7000_0000_0000, 0x7000_0001_0000));
        assert!(in_module(0x7000_0000_8000));
        assert!(!in_module(0x7000_0001_0000));
        assert!(!add_range(0, 10));
    }

    /// Waits until the watched thread is not [`Seen::Alive`].
    fn settle(watch: &Watch) -> Seen {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match watch.check() {
                Seen::Alive if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                s => return s,
            }
        }
    }

    #[test]
    fn a_fault_on_a_marked_thread_ends_that_thread_only() {
        install_own_filter();
        let before = faults();
        let watch = Watch::new(Duration::from_secs(60));
        let w = watch.clone();
        // Not joined: a thread ended this way never reports back to `join`.
        let _t = std::thread::spawn(move || {
            w.attach();
            let _c = enter();
            // SAFETY: the fault this test is about.
            unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
        });
        assert_eq!(settle(&watch), Seen::Ended);
        assert!(faults() > before);
        assert_eq!(last_code(), ACCESS_VIOLATION);
    }

    #[test]
    fn a_hung_call_is_ended_by_the_watch() {
        let watch = Watch::new(Duration::from_millis(100));
        let w = watch.clone();
        let _t = std::thread::spawn(move || {
            w.attach();
            let _c = enter();
            loop {
                std::hint::spin_loop();
            }
        });
        let seen = settle(&watch);
        assert_eq!(seen, Seen::Hung);
        assert_eq!(watch.check(), Seen::Ended);
    }
}
