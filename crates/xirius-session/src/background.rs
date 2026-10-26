//! Non-RT work whose abandoned result must also be destroyed off the UI (PRJ-030).
//!
//! A plain result channel is insufficient: dropping its receiver can destroy a
//! queued output/engine on the dropping thread. Here the worker retains ownership
//! until an explicit nonblocking claim. Abandonment acknowledges immediately;
//! `finished` acknowledges destruction separately. No Drop implementation joins.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RUNNING: u8 = 0;
const READY: u8 = 1;
const CLAIMED: u8 = 2;
const CANCELLED: u8 = 3;

struct Shared<T> {
    state: AtomicU8,
    result: Mutex<Option<Result<T, String>>>,
    finished: AtomicBool,
}

pub struct Job<T> {
    shared: Arc<Shared<T>>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        name: &str,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<Self, String> {
        let shared = Arc::new(Shared {
            state: AtomicU8::new(RUNNING),
            result: Mutex::new(None),
            finished: AtomicBool::new(false),
        });
        let worker = shared.clone();
        let work = Arc::new(Mutex::new(Some(work)));
        let task = work.clone();
        let spawned = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let work = task
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .expect("one worker");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                    .unwrap_or_else(|_| Err("裏の処理が停止しました".into()));
                {
                    let mut slot = worker.result.lock().unwrap_or_else(|e| e.into_inner());
                    *slot = Some(result);
                    let _ = worker.state.compare_exchange(
                        RUNNING,
                        READY,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                }
                loop {
                    match worker.state.load(Ordering::Acquire) {
                        CLAIMED => break,
                        CANCELLED => {
                            // Destroy outside the lock; the result may join an
                            // output or return audio to a blocked plugin owner.
                            let result = worker
                                .result
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .take();
                            drop(result);
                            break;
                        }
                        _ => std::thread::sleep(Duration::from_millis(2)),
                    }
                }
                worker.finished.store(true, Ordering::Release);
            });
        if let Err(e) = spawned {
            // Thread exhaustion must not cause a captured output/engine to be
            // synchronously destroyed by the UI. Quarantine this one task and
            // report failure rather than claiming retirement or retrying here.
            if let Some(work) = work.lock().unwrap_or_else(|e| e.into_inner()).take() {
                std::mem::forget(work);
            }
            return Err(format!(
                "裏の処理を開始できません。資源を保持しています: {e}"
            ));
        }
        Ok(Job { shared })
    }

    /// Claim a completed value. `try_lock` means UI callbacks never wait on the
    /// producer. The heavy result remains worker-owned until this exact point.
    pub fn poll(&self) -> Option<Result<T, String>> {
        if self.shared.state.load(Ordering::Acquire) != READY {
            return None;
        }
        let mut result = self.shared.result.try_lock().ok()?;
        self.shared
            .state
            .compare_exchange(READY, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        result.take()
    }
}

impl<T> Job<T> {
    /// Acceptance only; work already inside a DLL or an I/O call is not killed.
    pub fn cancel(&self) {
        let mut state = self.shared.state.load(Ordering::Acquire);
        while matches!(state, RUNNING | READY) {
            match self.shared.state.compare_exchange_weak(
                state,
                CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(new) => state = new,
            }
        }
    }

    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Acquire)
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    struct Probe {
        dropped: mpsc::Sender<thread::ThreadId>,
        release: mpsc::Receiver<()>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.dropped.send(thread::current().id()).unwrap();
            self.release.recv().unwrap();
        }
    }

    #[test]
    fn cancel_and_drop_never_wait_for_work_or_for_abandoned_result_destruction() {
        let ui = thread::current().id();
        let (entered, arrived) = mpsc::channel();
        let (go, gate) = mpsc::channel();
        let (dropped, dropping) = mpsc::channel();
        let (release, retiring) = mpsc::channel();
        let job = Job::spawn("background-gate", move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(Probe {
                dropped,
                release: retiring,
            })
        })
        .unwrap();
        arrived.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(job.poll().is_none());
        job.cancel();
        assert!(!job.finished());
        go.send(()).unwrap();
        let owner = dropping.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_ne!(owner, ui);
        assert!(!job.finished(), "destruction still held at gate");
        assert!(job.poll().is_none());
        drop(job);
        release.send(()).unwrap();
    }

    #[test]
    fn dropping_a_completed_unclaimed_result_destroys_it_on_its_worker() {
        let (dropped, dropping) = mpsc::channel();
        let (release, retiring) = mpsc::channel();
        let job = Job::spawn("background-unclaimed", move || {
            Ok(Probe {
                dropped,
                release: retiring,
            })
        })
        .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while job.shared.state.load(Ordering::Acquire) != READY {
            assert!(Instant::now() < end, "completion watchdog");
            thread::yield_now();
        }
        drop(job);
        assert_ne!(
            dropping.recv_timeout(Duration::from_secs(10)).unwrap(),
            thread::current().id()
        );
        release.send(()).unwrap();
    }

    #[test]
    fn only_the_claimed_result_moves_to_the_caller_and_panics_are_failures() {
        let job = Job::spawn("background-result", || Ok(42)).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = job.poll() {
                assert_eq!(result.unwrap(), 42);
                break;
            }
            assert!(Instant::now() < end, "claim watchdog");
            thread::yield_now();
        }
        assert!(job.poll().is_none());
        let job = Job::<()>::spawn("background-panic", || panic!("fixture")).unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = job.poll() {
                assert!(result.is_err());
                break;
            }
            assert!(Instant::now() < end, "panic watchdog");
            thread::yield_now();
        }
    }
}
