//! Writing the history and the automatic save points (PRJ-022).
//!
//! An edit writes its state and the index; on a slow disk (a USB drive, a synced folder)
//! that takes long enough to stop the window. A session the screen holds hands these
//! writes to a thread of its own ([`Session::write_in_background`]): one job after another,
//! in the order they were made. A command-line run keeps them in its own thread (the
//! default), so its answer is only given once the files are written.
//!
//! What reads the files or writes the song itself (a save, a save as, a named version,
//! opening another song, closing) first waits for the thread ([`Session::flush_writes`]).
//! A write that fails is kept until [`Session::take_write_errors`] hands it over; one made
//! on the spot for an edit comes back in the edit's message instead.

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::CmdResult;
use crate::model::Project;
use crate::session::{AUTO_KEEP, Session};
use crate::store::{HistoryIndex, Manifest, Store, VersionInfo};

/// Failed writes kept for [`Session::take_write_errors`]; more than this are counted only.
const ERRORS_KEPT: usize = 20;

pub(super) enum Job {
    /// A step: its state, then the index naming it, then the states it left behind go.
    /// `before` is the index cut back to the cursor, written first when the step replaces
    /// the steps that were ahead (so the old index never names a state written over).
    Step {
        store: Store,
        before: Option<HistoryIndex>,
        n: u64,
        state: Arc<Project>,
        index: HistoryIndex,
        remove: Vec<u64>,
    },
    /// The cursor moved (undo, redo, a jump) or steps were taken back: the index, then the
    /// states no longer named.
    Index {
        store: Store,
        index: HistoryIndex,
        remove: Vec<u64>,
    },
    /// An automatic save point: its file and the list, then the manifest counting it, then
    /// the oldest automatic points beyond [`AUTO_KEEP`] go.
    Version {
        store: Store,
        info: VersionInfo,
        state: Arc<Project>,
        manifest: Manifest,
    },
    /// A state alone: the first of a history started afresh on open.
    State {
        store: Store,
        n: u64,
        state: Arc<Project>,
    },
    /// The blobs no kept state points to go (after a save).
    Prune { store: Store, first: u64, last: u64 },
}

fn run(job: Job) -> CmdResult<()> {
    match job {
        Job::Step {
            store,
            before,
            n,
            state,
            index,
            remove,
        } => {
            if let Some(b) = before {
                store.write_index(&b)?;
            }
            store.write_state(n, &state)?;
            store.write_index(&index)?;
            for abs in remove {
                store.remove_state(abs);
            }
        }
        Job::Index {
            store,
            index,
            remove,
        } => {
            store.write_index(&index)?;
            for abs in remove {
                store.remove_state(abs);
            }
        }
        Job::Version {
            store,
            info,
            state,
            manifest,
        } => {
            // Another process may have taken the number: the list gives the next free one.
            let added = store.add_version(number(&info), &info.name, info.kind, &state)?;
            let mut manifest = manifest;
            manifest.next_version = manifest.next_version.max(number(&added) + 1);
            store.update_manifest(&manifest)?;
            store.prune_auto(AUTO_KEEP)?;
        }
        Job::State { store, n, state } => store.write_state(n, &state)?,
        Job::Prune { store, first, last } => {
            // Only clearing up: the next save tries again.
            let _ = store.prune_blobs(first, last);
        }
    }
    Ok(())
}

/// Whether `next`, the job right after `job`, writes over all `job` would: the same state of
/// the same history again, with the index cut back first (a field turned on, UI-319). Then
/// `job` need not be written.
fn replaces(next: &Job, job: &Job) -> bool {
    match (next, job) {
        (
            Job::Step {
                store: a,
                before: Some(_),
                n: m,
                ..
            },
            Job::Step {
                store: b,
                n,
                remove,
                ..
            },
        ) => a.dir == b.dir && a.lane == b.lane && m == n && remove.is_empty(),
        _ => false,
    }
}

fn number(info: &VersionInfo) -> u64 {
    info.id[1..].parse().unwrap_or(0)
}

#[derive(Default)]
struct Shared {
    /// Jobs handed to the thread and not yet done.
    pending: Mutex<usize>,
    done: Condvar,
    errors: Mutex<Errors>,
    /// Waited before each job: a slow disk for the tests.
    delay: Mutex<Duration>,
}

#[derive(Default)]
struct Errors {
    kept: Vec<crate::error::CmdError>,
    /// Failed writes beyond [`ERRORS_KEPT`].
    more: usize,
}

impl Shared {
    fn wait_delay(&self) {
        let d = *self.delay.lock().unwrap_or_else(PoisonError::into_inner);
        if !d.is_zero() {
            thread::sleep(d);
        }
    }

    fn failed(&self, message: crate::error::CmdError) {
        let mut e = self.errors.lock().unwrap_or_else(PoisonError::into_inner);
        if e.kept.last() == Some(&message) {
            return;
        }
        if e.kept.len() < ERRORS_KEPT {
            e.kept.push(message);
        } else {
            e.more += 1;
        }
    }

    fn finish_one(&self) {
        let mut n = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        *n = n.saturating_sub(1);
        if *n == 0 {
            #[cfg(any(test, feature = "test-io"))]
            let _ = crate::io_test::before("writer_complete", std::path::Path::new(""));
            self.done.notify_all();
        }
    }
}

struct Thread {
    tx: mpsc::Sender<Job>,
    handle: JoinHandle<()>,
}

/// Where a session's writes go: done on the spot, or by a thread of its own.
#[derive(Default)]
pub(super) struct Writer {
    shared: Arc<Shared>,
    thread: Option<Thread>,
}

pub(super) fn message(e: &crate::error::CmdError) -> String {
    format!("履歴を書き込めませんでした: {}", e.message)
}

impl Writer {
    pub(super) fn is_background(&self) -> bool {
        self.thread.is_some()
    }

    pub(super) fn set_background(&mut self, on: bool) {
        if on == self.is_background() {
            return;
        }
        if !on {
            self.stop();
            return;
        }
        let (tx, rx) = mpsc::channel::<Job>();
        let shared = self.shared.clone();
        let handle = thread::Builder::new()
            .name("xirius-history-writer".into())
            .spawn(move || {
                let mut queued = VecDeque::new();
                while let Some(job) = queued.pop_front().or_else(|| rx.recv().ok()) {
                    queued.extend(rx.try_iter());
                    if queued.front().is_some_and(|next| replaces(next, &job)) {
                        shared.finish_one();
                        continue;
                    }
                    shared.wait_delay();
                    match catch_unwind(AssertUnwindSafe(|| run(job))) {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => shared.failed(e),
                        Err(_) => {
                            shared.failed(crate::error::CmdError::io("履歴を書き込めませんでした"))
                        }
                    }
                    shared.finish_one();
                }
            });
        // Without a thread the writes are made on the spot, as before.
        if let Ok(handle) = handle {
            self.thread = Some(Thread { tx, handle });
        }
    }

    /// Hands a job over. On the spot, its error comes back; from the thread it comes later
    /// through [`Writer::take_errors`].
    pub(super) fn submit(&self, job: Job) -> CmdResult<()> {
        if let Some(t) = &self.thread {
            *self
                .shared
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner) += 1;
            match t.tx.send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::SendError(job)) => {
                    // The thread is gone: write here.
                    self.shared.finish_one();
                    self.shared.wait_delay();
                    return run(job);
                }
            }
        }
        self.shared.wait_delay();
        run(job)
    }

    pub(super) fn pending(&self) -> bool {
        *self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            > 0
    }

    pub(super) fn has_errors(&self) -> bool {
        let errors = self
            .shared
            .errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        !errors.kept.is_empty() || errors.more > 0
    }

    /// Waits until every job handed over is done.
    pub(super) fn flush(&self) {
        let n = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        #[cfg(any(test, feature = "test-io"))]
        if *n > 0 {
            let _ = crate::io_test::before("writer_pending", std::path::Path::new(""));
        }
        let _n = self
            .shared
            .done
            .wait_while(n, |n| *n > 0)
            .unwrap_or_else(PoisonError::into_inner);
    }

    /// Keeps a failed write made on the spot whose caller has no answer to carry it in.
    pub(super) fn keep_error(&self, message: crate::error::CmdError) {
        self.shared.failed(message);
    }

    pub(super) fn take_errors(&self) -> Vec<crate::error::CmdError> {
        let mut e = self
            .shared
            .errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut out = std::mem::take(&mut e.kept);
        if e.more > 0 {
            out.push(crate::error::CmdError::io(format!(
                "ほかに {} 件の書き込みに失敗しました",
                e.more
            )));
            e.more = 0;
        }
        out
    }

    pub(super) fn set_delay(&self, d: Duration) {
        *self
            .shared
            .delay
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = d;
    }

    /// Lets the thread finish what it was given, and ends it.
    fn stop(&mut self) {
        if let Some(t) = self.thread.take() {
            drop(t.tx);
            let _ = t.handle.join();
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Session {
    /// Hands the history's and the automatic save points' writes to a thread of their own,
    /// so an edit returns before they reach the disk (the screen's session). Off, they are
    /// written before the command returns (the default).
    pub fn write_in_background(&mut self, on: bool) {
        self.writer.set_background(on);
    }

    pub fn writes_in_background(&self) -> bool {
        self.writer.is_background()
    }

    /// Writes handed to the thread and not done yet.
    pub fn writes_pending(&self) -> bool {
        self.writer.pending()
    }

    /// Waits until the writes handed to the thread are done.
    pub fn flush_writes(&self) {
        self.writer.flush();
    }

    /// The writes the thread could not make since the last call, oldest first.
    pub fn take_write_errors(&self) -> Vec<String> {
        self.writer.take_errors().iter().map(message).collect()
    }

    /// OS kinds are retained for the save warning (UI-288).
    pub fn take_write_failures(&self) -> Vec<crate::error::CmdError> {
        self.writer.take_errors()
    }

    /// Waits this long before each write, as a slow disk would (for the tests).
    #[doc(hidden)]
    pub fn simulate_slow_disk(&self, delay: Duration) {
        self.writer.set_delay(delay);
    }
}
