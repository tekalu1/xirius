//! Asynchronous prefetch of a track's stateful chain (ENG-015).
//!
//! An instrument and its inserts keep state, so their blocks cannot be mixed ahead of time and
//! thrown away the way pure timeline audio can ([`crate::anticipation`]). Here the whole chain
//! is *lent* to a worker thread that renders it ahead of the transport into a small ring of
//! blocks; the audio callback only copies the block it needs. The chain is at one side at a
//! time: home (the audio thread renders it itself) or away (a worker renders it).
//!
//! The worker renders in blocks of `max_block` from consecutive song positions, so the ring is a
//! plain stream of frames and the callback may take any block size out of it. Whatever the
//! timing of the worker, the frames the callback gets are the ones the chain renders in order;
//! nothing is skipped or reordered. An edit (a new timeline, a note played by hand, a seek, a
//! change of the live flag) calls the chain back and drops what was prefetched: its state is
//! then ahead of the transport, which the caller puts right the way a seek does.

use std::cell::UnsafeCell;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::Thread;
use std::time::Duration;

/// Blocks the ring holds. The worker stays this far ahead of the callback at most.
pub const DEPTH: usize = 4;

/// What a worker renders: the stateful part of one track.
pub trait Chained: Send + 'static {
    /// Renders the block that starts `pos` frames into the song; the block is `left.len()` long.
    fn render_ahead(&mut self, pos: u64, left: &mut [f32], right: &mut [f32]);
}

/// How a prefetching track is doing, for tests and the UI's health display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrefetchStatus {
    /// The chain is with a worker.
    pub lent: bool,
    /// The track takes live input (a note played by hand) and renders on the audio thread.
    pub hot: bool,
    /// Blocks rendered ahead and not yet taken.
    pub queued_blocks: usize,
    /// Frames the workers rendered for this track.
    pub rendered_frames: u64,
    /// Times an edit or a jump called the chain back and dropped the blocks.
    pub discards: u64,
    /// Times the callback had to wait for a block that was not ready.
    pub waits: u64,
    /// A processor panicked on a worker: the track renders on the audio thread from then on.
    pub failed: bool,
}

struct Block {
    pos: u64,
    frames: usize,
    left: Vec<f32>,
    right: Vec<f32>,
}

struct WorkerSide<C> {
    chain: Option<Box<C>>,
    next: u64,
}

struct Shared<C: Chained> {
    block: usize,
    slots: Box<[UnsafeCell<Block>]>,
    /// Counters, not indices: `head` is the callback's, `tail` the worker's.
    head: AtomicUsize,
    tail: AtomicUsize,
    to_worker: AtomicPtr<C>,
    to_home: AtomicPtr<C>,
    start: AtomicU64,
    recall: AtomicBool,
    failed: AtomicBool,
    closed: AtomicBool,
    claim: AtomicBool,
    rendered: AtomicU64,
    worker: UnsafeCell<WorkerSide<C>>,
}

// SAFETY: a slot is written by the worker only while `tail - head < DEPTH` (then it is not
// readable) and read by the callback only while `head < tail`; each side publishes with
// Release and reads the other's counter with Acquire. `worker` is touched only by whoever holds
// `claim`. The chain crosses threads as a pointer through the two mailboxes.
unsafe impl<C: Chained> Sync for Shared<C> {}
unsafe impl<C: Chained> Send for Shared<C> {}

trait Job: Send + Sync {
    /// Does one block of work if there is any; whether it did.
    fn step(&self) -> bool;
}

impl<C: Chained> Job for Shared<C> {
    fn step(&self) -> bool {
        if self.closed.load(Ordering::Relaxed) || self.claim.swap(true, Ordering::Acquire) {
            return false;
        }
        // SAFETY: `claim` is ours.
        let did = unsafe { self.step_claimed() };
        self.claim.store(false, Ordering::Release);
        did
    }
}

impl<C: Chained> Shared<C> {
    /// # Safety
    /// The caller holds `claim`.
    unsafe fn step_claimed(&self) -> bool {
        // SAFETY: by the contract.
        let w = unsafe { &mut *self.worker.get() };
        if w.chain.is_none() {
            let p = self.to_worker.swap(std::ptr::null_mut(), Ordering::Acquire);
            if p.is_null() {
                return false;
            }
            // SAFETY: the pointer came from `Box::into_raw` in `lend` and was taken once.
            w.chain = Some(unsafe { Box::from_raw(p) });
            w.next = self.start.load(Ordering::Relaxed);
        }
        let give_back = self.recall.load(Ordering::Acquire) || self.failed.load(Ordering::Relaxed);
        if give_back {
            return match w.chain.take() {
                Some(chain) => {
                    self.to_home.store(Box::into_raw(chain), Ordering::Release);
                    true
                }
                None => false,
            };
        }
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        if tail - head >= DEPTH {
            return false;
        }
        // SAFETY: the slot at `tail` is free: the callback reads only `head..tail`.
        let slot = unsafe { &mut *self.slots[tail % DEPTH].get() };
        let Some(chain) = w.chain.as_mut() else {
            return false;
        };
        let (pos, n) = (w.next, self.block);
        let done = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _audio = crate::rt_lock::AudioScope::enter();
            assert_no_alloc::assert_no_alloc(|| {
                chain.render_ahead(pos, &mut slot.left[..n], &mut slot.right[..n]);
            });
        }));
        if done.is_err() {
            self.failed.store(true, Ordering::Release);
            if let Some(chain) = w.chain.take() {
                self.to_home.store(Box::into_raw(chain), Ordering::Release);
            }
            return true;
        }
        slot.pos = pos;
        slot.frames = n;
        w.next = pos + n as u64;
        self.rendered.fetch_add(n as u64, Ordering::Relaxed);
        self.tail.store(tail + 1, Ordering::Release);
        true
    }
}

struct Pool {
    jobs: Mutex<Vec<Arc<dyn Job>>>,
    version: AtomicU64,
    threads: Vec<Thread>,
    next: AtomicUsize,
}

static POOL: OnceLock<Option<Pool>> = OnceLock::new();

impl Pool {
    /// The pool, started on first use. `None` when no thread could be started.
    fn get() -> Option<&'static Pool> {
        POOL.get_or_init(Self::start).as_ref()
    }

    fn start() -> Option<Pool> {
        let count = std::thread::available_parallelism()
            .map_or(1, |n| n.get() / 2)
            .clamp(1, 4);
        let mut handles = Vec::new();
        for i in 0..count {
            // The workers need the pool, which needs their handles: they wait for it.
            let h = std::thread::Builder::new()
                .name(format!("xirius-ahead-{i}"))
                .spawn(|| {
                    let _priority = crate::rt::ProAudioThread::join();
                    let pool = loop {
                        match POOL.get() {
                            Some(Some(p)) => break p,
                            // The pool could not be made whole: this thread has no pool to serve.
                            Some(None) => return,
                            None => std::thread::sleep(Duration::from_millis(1)),
                        }
                    };
                    pool.work();
                })
                .ok()?;
            handles.push(h);
        }
        Some(Pool {
            jobs: Mutex::new(Vec::new()),
            version: AtomicU64::new(0),
            threads: handles.iter().map(|h| h.thread().clone()).collect(),
            next: AtomicUsize::new(0),
        })
    }

    fn work(&self) -> ! {
        let mut jobs: Vec<Arc<dyn Job>> = Vec::new();
        let mut seen = u64::MAX;
        loop {
            let v = self.version.load(Ordering::Acquire);
            if v != seen {
                jobs = self.jobs.lock().map(|j| j.clone()).unwrap_or_default();
                seen = v;
            }
            let mut any = false;
            for job in &jobs {
                any |= job.step();
            }
            if !any {
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }
    }

    /// Wakes one worker; allocation-free, for the audio thread.
    fn wake(&self) {
        let i = self.next.fetch_add(1, Ordering::Relaxed);
        self.threads[i % self.threads.len()].unpark();
    }

    fn register(&self, job: Arc<dyn Job>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.push(job);
        }
        self.version.fetch_add(1, Ordering::Release);
        self.wake();
    }

    fn unregister(&self, job: &Arc<dyn Job>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.retain(|j| !Arc::ptr_eq(j, job));
        }
        self.version.fetch_add(1, Ordering::Release);
    }
}

fn wake_pool() {
    if let Some(Some(pool)) = POOL.get() {
        pool.wake();
    }
}

/// The callback's end of one track's prefetch.
pub struct Prefetch<C: Chained> {
    shared: Arc<Shared<C>>,
    job: Arc<dyn Job>,
    lent: bool,
    /// Frames of the head block already taken.
    taken: usize,
    hot: bool,
    discards: u64,
    waits: u64,
}

/// What [`Prefetch::take`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Take {
    /// The block was copied out.
    Hit,
    /// The ring does not continue at the asked position, or the worker failed.
    Miss,
}

impl<C: Chained> Prefetch<C> {
    /// A prefetch for blocks of `block` frames; `None` when no worker thread could start.
    pub fn new(block: usize) -> Option<Self> {
        let pool = Pool::get()?;
        let slots = (0..DEPTH)
            .map(|_| {
                UnsafeCell::new(Block {
                    pos: 0,
                    frames: 0,
                    left: vec![0.0; block],
                    right: vec![0.0; block],
                })
            })
            .collect();
        let shared = Arc::new(Shared {
            block,
            slots,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            to_worker: AtomicPtr::new(std::ptr::null_mut()),
            to_home: AtomicPtr::new(std::ptr::null_mut()),
            start: AtomicU64::new(0),
            recall: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            claim: AtomicBool::new(false),
            rendered: AtomicU64::new(0),
            worker: UnsafeCell::new(WorkerSide {
                chain: None,
                next: 0,
            }),
        });
        let job: Arc<dyn Job> = shared.clone();
        pool.register(job.clone());
        Some(Self {
            shared,
            job,
            lent: false,
            taken: 0,
            hot: false,
            discards: 0,
            waits: 0,
        })
    }

    pub fn is_lent(&self) -> bool {
        self.lent
    }

    /// A worker panicked: the chain renders on the audio thread for good.
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Acquire)
    }

    /// Marks the track as taking live input (or not): a hot track is not lent.
    pub fn set_hot(&mut self, hot: bool) {
        self.hot = hot;
    }

    pub fn hot(&self) -> bool {
        self.hot
    }

    /// Lends the chain: the worker renders from `pos` on. The ring must be empty (a chain that
    /// came back through [`Self::recall`] leaves it so).
    pub fn lend(&mut self, chain: Box<C>, pos: u64) {
        debug_assert!(!self.lent);
        let s = &self.shared;
        s.start.store(pos, Ordering::Relaxed);
        self.taken = 0;
        self.lent = true;
        s.to_worker.store(Box::into_raw(chain), Ordering::Release);
        wake_pool();
    }

    /// Copies the next `left.len()` frames, which must start at song position `pos`, out of the
    /// ring. Waits when the worker is still rendering them.
    pub fn take(&mut self, pos: u64, left: &mut [f32], right: &mut [f32]) -> Take {
        let n = left.len().min(right.len());
        let s = &self.shared;
        let mut filled = 0;
        let mut waited = false;
        while filled < n {
            let head = s.head.load(Ordering::Relaxed);
            let mut spins = 0u32;
            while s.tail.load(Ordering::Acquire) == head {
                if s.failed.load(Ordering::Acquire) {
                    return Take::Miss;
                }
                if !std::mem::replace(&mut waited, true) {
                    self.waits += 1;
                }
                spins += 1;
                if spins.is_multiple_of(64) {
                    wake_pool();
                    std::thread::yield_now();
                } else {
                    std::hint::spin_loop();
                }
            }
            // SAFETY: `head < tail`, so the worker has published this slot and will not write it
            // before `head` moves.
            let slot = unsafe { &*s.slots[head % DEPTH].get() };
            if slot.pos + self.taken as u64 != pos + filled as u64 {
                return Take::Miss;
            }
            let k = (slot.frames - self.taken).min(n - filled);
            left[filled..filled + k].copy_from_slice(&slot.left[self.taken..self.taken + k]);
            right[filled..filled + k].copy_from_slice(&slot.right[self.taken..self.taken + k]);
            self.taken += k;
            filled += k;
            if self.taken == slot.frames {
                self.taken = 0;
                s.head.store(head + 1, Ordering::Release);
                wake_pool();
            }
        }
        Take::Hit
    }

    /// Calls the chain back and drops what was rendered ahead. Waits for the worker to finish
    /// the block it is in. `None` when the chain is not lent.
    pub fn recall(&mut self) -> Option<Box<C>> {
        if !self.lent {
            return None;
        }
        let s = &self.shared;
        s.recall.store(true, Ordering::Release);
        let mut spins = 0u32;
        let chain = loop {
            let p = s.to_home.swap(std::ptr::null_mut(), Ordering::Acquire);
            if !p.is_null() {
                // SAFETY: the pointer came from `Box::into_raw` in the worker and was taken once.
                break unsafe { Box::from_raw(p) };
            }
            spins += 1;
            if spins.is_multiple_of(64) {
                wake_pool();
                std::thread::yield_now();
            } else {
                std::hint::spin_loop();
            }
        };
        s.recall.store(false, Ordering::Release);
        s.head
            .store(s.tail.load(Ordering::Acquire), Ordering::Release);
        self.taken = 0;
        self.lent = false;
        self.discards += 1;
        Some(chain)
    }

    pub fn status(&self) -> PrefetchStatus {
        let s = &self.shared;
        PrefetchStatus {
            lent: self.lent,
            hot: self.hot,
            queued_blocks: s.tail.load(Ordering::Acquire) - s.head.load(Ordering::Relaxed),
            rendered_frames: s.rendered.load(Ordering::Relaxed),
            discards: self.discards,
            waits: self.waits,
            failed: self.failed(),
        }
    }
}

impl<C: Chained> Drop for Prefetch<C> {
    fn drop(&mut self) {
        // The chain must be back before its owner goes: a worker may be inside it.
        drop(self.recall());
        self.shared.closed.store(true, Ordering::Release);
        if let Some(Some(pool)) = POOL.get() {
            pool.unregister(&self.job);
        }
    }
}
