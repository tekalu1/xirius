//! The autosave clock (PRJ-009): calls `Session::autosave_if_due` at a fixed
//! interval so the UI, the CLI's MCP server and anything else that holds a
//! shared session need no timer of their own.
//!
//! The song's own work is in `autosave_if_due` (it skips when nothing changed
//! since the last save point). This module only supplies the beat.

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::Session;
use crate::store::{VersionInfo, now_ms};

/// The default beat: every 30 seconds (PRJ-009).
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(30);

/// One beat for a caller that has its own timer (a UI frame loop, a test).
pub fn tick(session: &mut Session) -> Option<VersionInfo> {
    session.autosave_if_due(now_ms()).ok().flatten()
}

type Hook = Box<dyn Fn(&VersionInfo) + Send + 'static>;

/// A background thread that beats until it is stopped or dropped.
pub struct Autosave {
    stop: Arc<(Mutex<bool>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Autosave {
    /// Starts the clock on a shared session. `interval` also becomes the
    /// session's own interval, so a save point is made on every beat that
    /// finds the song changed. `on_save` hears about each save point.
    pub fn start(
        session: Arc<Mutex<Session>>,
        interval: Duration,
        on_save: Option<Hook>,
    ) -> Autosave {
        let interval = interval.max(Duration::from_millis(10));
        session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .auto_interval_ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("xirius-autosave".into())
            .spawn(move || {
                let (lock, cv) = &*flag;
                loop {
                    let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
                    let (guard, _) = cv
                        .wait_timeout_while(guard, interval, |stopped| !*stopped)
                        .unwrap_or_else(PoisonError::into_inner);
                    if *guard {
                        return;
                    }
                    drop(guard);
                    let made = {
                        let mut s = session.lock().unwrap_or_else(PoisonError::into_inner);
                        tick(&mut s)
                    };
                    if let (Some(v), Some(f)) = (made, on_save.as_ref()) {
                        f(&v);
                    }
                }
            })
            .ok();
        Autosave { stop, thread }
    }

    /// Stops the beat and waits for the thread.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        let (lock, cv) = &*self.stop;
        *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
        cv.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.halt();
    }
}
