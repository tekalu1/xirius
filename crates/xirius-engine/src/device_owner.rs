//! Device owner lifecycle shared by CPAL and MIDI. Waiting is worker-only.
//!
//! A readiness error abandons an opening, not its driver thread. The opening
//! requests stop and registers the actual release token before returning an
//! error. Retries retain that token until the owner has destroyed its resources.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Default)]
pub struct Registry(Mutex<Vec<Registered>>);
#[derive(Clone)]
struct Registered {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicBool>,
}
impl Registry {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }
    pub fn opening(&self) -> Opening {
        self.opening_observed(|| {})
    }
    fn opening_observed(&self, waiting: impl FnOnce()) -> Opening {
        let pending = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        waiting();
        for owner in pending {
            if owner.stop.load(Ordering::Acquire) {
                wait_closed(&owner.done);
            }
        }
        let owner = Owner {
            stop: Arc::new(AtomicBool::new(false)),
            done: Some(Arc::new(AtomicBool::new(false))),
        };
        let mut owners = self.0.lock().unwrap_or_else(|e| e.into_inner());
        owners.retain(|owner| !owner.done.load(Ordering::Acquire));
        // Register before starting the driver, not on RT Drop. A plain stop and
        // an abandoned opening therefore have exactly the same retry barrier.
        owners.push(Registered {
            stop: owner.stop.clone(),
            done: owner.done.clone().unwrap(),
        });
        Opening {
            owner: Some(owner),
            started: std::cell::Cell::new(false),
        }
    }
}
pub struct Opening {
    owner: Option<Owner>,
    started: std::cell::Cell<bool>,
}
impl Opening {
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.owner.as_ref().unwrap().stop.clone()
    }
    /// Install before constructing any resource on the owner thread.
    pub fn completion(&self) -> Finished {
        assert!(
            !self.started.replace(true),
            "a device has exactly one owning thread"
        );
        Finished(self.owner.as_ref().unwrap().done.clone().unwrap())
    }
    /// All readiness outcomes, including timeout/disconnect/driver errors, go here.
    pub fn accept<T>(mut self, result: Result<T, String>) -> Result<(T, Owner), String> {
        let value = result?;
        assert!(
            self.started.get(),
            "readiness requires an owner completion guard"
        );
        Ok((value, self.owner.take().unwrap()))
    }
}
impl Drop for Opening {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            owner.request_stop();
            if !self.started.get() {
                // No owner ever started, including a rejected thread spawn.
                owner.done.as_ref().unwrap().store(true, Ordering::Release);
            }
        }
    }
}

pub struct Owner {
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) done: Option<Arc<AtomicBool>>,
}
impl Owner {
    /// An external owner supplies the stop flag; its wrapper owns completion.
    pub fn external(stop: Arc<AtomicBool>) -> Self {
        Self { stop, done: None }
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
    pub fn closed(&self) -> Option<Arc<AtomicBool>> {
        self.done.clone()
    }
    /// Only retirement workers may wait for actual release. Plain Drop only signals.
    pub fn close(&self) {
        self.request_stop();
        if let Some(done) = &self.done {
            wait_closed(done);
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.request_stop();
    }
}

pub struct Finished(pub Arc<AtomicBool>);
impl Drop for Finished {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}
pub fn wait_closed(done: &AtomicBool) {
    while !done.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn a_normal_drop_uses_the_same_release_barrier_without_locking_on_rt() {
        let registry = Arc::new(Registry::new());
        let opening = registry.opening();
        let stop = opening.stop_flag();
        let done = opening.completion();
        let (_, owner) = opening.accept(Ok(())).unwrap();
        let (stopped_tx, stopped) = mpsc::channel();
        let (finish, closed) = mpsc::channel();
        let owner_thread = std::thread::spawn(move || {
            let _done = done;
            wait_closed(&stop);
            stopped_tx.send(()).unwrap();
            closed.recv_timeout(Duration::from_secs(20)).unwrap();
        });
        // A registry mutex held elsewhere cannot delay the RT stop request.
        let held = registry.0.lock().unwrap();
        drop(owner);
        drop(held);
        stopped.recv_timeout(Duration::from_secs(20)).unwrap();
        let (waiting_tx, waiting) = mpsc::channel();
        let (entered_tx, entered) = mpsc::channel();
        let next = registry.clone();
        let retry = std::thread::spawn(move || {
            let _opening = next.opening_observed(|| waiting_tx.send(()).unwrap());
            entered_tx.send(()).unwrap();
        });
        waiting.recv_timeout(Duration::from_secs(20)).unwrap();
        assert!(entered.try_recv().is_err());
        finish.send(()).unwrap();
        owner_thread.join().unwrap();
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        retry.join().unwrap();
        // An opening abandoned before any owner starts leaves no ghost token.
        drop(registry.opening());
        drop(registry.opening());
    }
    #[test]
    fn every_readiness_failure_stops_even_a_successfully_sent_late_reply_and_serializes_retry() {
        for sent_before_abandon in [false, true] {
            let registry = Arc::new(Registry::new());
            let opening = registry.opening();
            let stop = opening.stop_flag();
            let done = opening.completion();
            let (ready_tx, ready_rx) = mpsc::channel();
            let (release, held) = mpsc::channel();
            let (closing_tx, closing) = mpsc::channel();
            let (finish, closed) = mpsc::channel();
            let owner = std::thread::spawn(move || {
                let _done = done;
                held.recv_timeout(Duration::from_secs(20)).unwrap();
                let _resource = std::rc::Rc::new(());
                let _ = ready_tx.send(());
                wait_closed(&stop);
                closing_tx.send(()).unwrap();
                closed.recv_timeout(Duration::from_secs(20)).unwrap();
            });
            if sent_before_abandon {
                release.send(()).unwrap();
                ready_rx.recv_timeout(Duration::from_secs(20)).unwrap();
            }
            assert!(
                opening
                    .accept::<()>(Err("readiness abandoned".into()))
                    .is_err()
            );
            if !sent_before_abandon {
                release.send(()).unwrap();
            }
            closing.recv_timeout(Duration::from_secs(20)).unwrap();
            let (entered_tx, entered) = mpsc::channel();
            let (waiting_tx, waiting) = mpsc::channel();
            let retry_registry = registry.clone();
            let retry = std::thread::spawn(move || {
                let _next = retry_registry.opening_observed(|| waiting_tx.send(()).unwrap());
                entered_tx.send(()).unwrap();
            });
            waiting.recv_timeout(Duration::from_secs(20)).unwrap();
            assert!(entered.try_recv().is_err());
            finish.send(()).unwrap();
            owner.join().unwrap();
            entered.recv_timeout(Duration::from_secs(20)).unwrap();
            retry.join().unwrap();
        }
    }
}
