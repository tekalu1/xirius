//! Allocations counted on the thread that makes them, for the measurement window (MIX-045).
//!
//! Built with `--features alloc-count`, the binary takes [`Counting`] as its allocator and the
//! benches read [`here`] at each frame; without the feature nothing counts and [`here`] is
//! `None`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static MADE: Cell<u64> = const { Cell::new(0) };
}

/// The system allocator, counting each allocation and reallocation on its thread.
pub struct Counting;

fn count() {
    // During a thread's teardown the counter may be gone already; that one is not counted.
    let _ = MADE.try_with(|n| n.set(n.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller keeps `GlobalAlloc::alloc`'s contract, passed on as it is.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as in `alloc`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count();
        // SAFETY: as in `alloc`; `ptr` came from this allocator, which is the system's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as in `realloc`.
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// Allocations made on this thread so far, when they are counted.
pub fn here() -> Option<u64> {
    cfg!(feature = "alloc-count").then(|| MADE.with(Cell::get))
}
