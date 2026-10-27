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
pub struct Registry(Mutex<Vec<Arc<AtomicBool>>>);
impl Registry {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }
    pub fn opening(&self) -> Opening<'_> {
        self.opening_observed(|| {})
    }
    fn opening_observed(&self, waiting: impl FnOnce()) -> Opening<'_> {
        let pending = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        waiting();
        for done in pending {
            wait_closed(&done);
        }
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|d| !d.load(Ordering::Acquire));
        Opening {
            registry: self,
            owner: Some(Owner {
                stop: Arc::new(AtomicBool::new(false)),
                done: Some(Arc::new(AtomicBool::new(false))),
            }),
        }
    }
}

pub struct Opening<'a> {
    registry: &'a Registry,
    owner: Option<Owner>,
}
impl Opening<'_> {
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.owner.as_ref().unwrap().stop.clone()
    }
    /// Install before constructing any resource on the owner thread.
    pub fn completion(&self) -> Finished {
        Finished(self.owner.as_ref().unwrap().done.clone().unwrap())
    }
    /// All readiness outcomes, including timeout/disconnect/driver errors, go here.
    pub fn accept<T>(mut self, result: Result<T, String>) -> Result<(T, Owner), String> {
        let value = result?;
        Ok((value, self.owner.take().unwrap()))
    }
}
impl Drop for Opening<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            owner.request_stop();
            self.registry
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(owner.done.clone().unwrap());
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
