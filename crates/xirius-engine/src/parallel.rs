//! Persistent workers for independent tracks (ENG-014). The caller lends disjoint track
//! index lists for one block and joins them before routing sends, buses and the master.
//! With sidechain keys the caller is a render level: every node's sources are
//! in an earlier level, so a level's tracks are independent (ENG-078).

use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread::JoinHandle;

use crate::mixer::{Plan, TrackNode};
use crate::sandbox::Event;

#[derive(Clone, Copy)]
struct Work {
    tracks: *mut Option<Box<TrackNode>>,
    indices: *const usize,
    len: usize,
    frames: usize,
    position: Option<u64>,
}

struct Worker {
    // Idle (0), work published (1), completed (2), stopping (3), failed but joined (4).
    state: AtomicU8,
    work: UnsafeCell<Option<Work>>,
    wake: Event,
}

// SAFETY: the caller writes work only while idle, then publishes with Release. One worker
// reads it after Acquire and signals completion before the caller reuses the storage.
unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

pub(crate) struct TrackPool {
    failed: bool,
    workers: Vec<Arc<Worker>>,
    threads: Vec<JoinHandle<()>>,
}

impl TrackPool {
    pub fn new(count: usize) -> std::io::Result<Self> {
        let mut pool = Self {
            failed: false,
            workers: Vec::new(),
            threads: Vec::new(),
        };
        for i in 0..count.saturating_sub(1) {
            let worker = Arc::new(Worker {
                state: AtomicU8::new(0),
                work: UnsafeCell::new(None),
                wake: Event::create("")?,
            });
            let w = worker.clone();
            let thread = std::thread::Builder::new()
                .name(format!("xirius-track-{i}"))
                .spawn(move || {
                    let _priority = crate::rt::ProAudioThread::join();
                    loop {
                        w.wake.wait(u32::MAX);
                        match w.state.load(Ordering::Acquire) {
                            3 => break,
                            1 => {
                                // SAFETY: the caller published a disjoint, valid range and will
                                // not access it or drop the plan until completion is acquired.
                                let work = unsafe { (*w.work.get()).expect("published work") };
                                // A processor panic must release the join before the plan can drop.
                                let result = std::panic::catch_unwind(
                                    std::panic::AssertUnwindSafe(|| unsafe {
                                        Self::run_work(work);
                                    }),
                                );
                                w.state
                                    .store(if result.is_ok() { 2 } else { 4 }, Ordering::Release);
                            }
                            _ => {}
                        }
                    }
                })?;
            pool.workers.push(worker);
            pool.threads.push(thread);
        }
        Ok(pool)
    }

    pub fn count(&self) -> usize {
        self.workers.len() + 1
    }

    unsafe fn run_work(work: Work) {
        let _audio = crate::rt_lock::AudioScope::enter();
        // SAFETY: work lends this index list exclusively until all workers have joined.
        let indices = unsafe { std::slice::from_raw_parts(work.indices, work.len) };
        for &i in indices {
            // SAFETY: a level lists each node once, and no two levels run at once.
            let t = unsafe { &mut *work.tracks.add(i) };
            if let Some(t) = t {
                assert_no_alloc::assert_no_alloc(|| {
                    Plan::render_track(t, work.frames, work.position)
                });
            }
        }
    }

    /// Renders the listed tracks across the workers and joins them before
    /// returning. `indices` names the tracks of one level: every index is a
    /// track slot, and no index appears twice (ENG-078).
    pub fn render(
        &mut self,
        tracks: &mut [Option<Box<TrackNode>>],
        indices: &[usize],
        frames: usize,
        position: Option<u64>,
    ) {
        assert!(!self.failed, "a failed track pool cannot be reused");
        let width = indices.len().div_ceil(self.workers.len() + 1);
        if width == 0 {
            return;
        }
        let base = tracks.as_mut_ptr();
        let mut started = 0;
        for (i, worker) in self.workers.iter().enumerate() {
            let begin = i * width;
            if begin >= indices.len() {
                break;
            }
            // SAFETY: index chunks are disjoint, belong to `indices`, and
            // outlive the join below; the level's indices are all distinct.
            unsafe {
                *worker.work.get() = Some(Work {
                    tracks: base,
                    indices: indices.as_ptr().add(begin),
                    len: width.min(indices.len() - begin),
                    frames,
                    position,
                });
            }
            worker.state.store(1, Ordering::Release);
            worker.wake.set();
            started += 1;
        }
        // The calling audio thread renders the last chunk instead of idling at the join.
        let begin = (started * width).min(indices.len());
        // SAFETY: this tail is disjoint from all published ranges.
        let local = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            Self::run_work(Work {
                tracks: base,
                indices: indices.as_ptr().add(begin),
                len: indices.len() - begin,
                frames,
                position,
            });
        }));
        let mut failed = false;
        for worker in &self.workers[..started] {
            while worker.state.load(Ordering::Acquire) < 2 {
                std::hint::spin_loop();
            }
            failed |= worker.state.load(Ordering::Relaxed) == 4;
            worker.state.store(0, Ordering::Relaxed);
        }
        self.failed = failed || local.is_err();
        if let Err(panic) = local {
            std::panic::resume_unwind(panic);
        }
        assert!(!failed, "a track processor panicked on a worker");
    }
}

impl Drop for TrackPool {
    fn drop(&mut self) {
        // Plans are retired on the controller thread, after the last joined render.
        for w in &self.workers {
            w.state.store(3, Ordering::Release);
            w.wake.set();
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
