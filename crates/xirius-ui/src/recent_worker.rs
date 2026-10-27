//! PRJ-031: a FIFO supervisor per root; submit/poll never wait on filesystem locks.
use crate::keep::{LastOpen, RecentList, RecentSong};
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock, Weak, mpsc},
    time::{Duration, Instant},
};
pub(crate) const CLOSE_WAIT: Duration = Duration::from_secs(2);
#[derive(Clone, Debug)]
pub(crate) enum Update {
    Remember(PathBuf, String, bool, u64),
    Forget(PathBuf),
    Closed(LastOpen),
    Refresh,
}
#[derive(Debug)]
pub(crate) struct Completion {
    pub revision: u64,
    pub result: Result<Vec<RecentSong>, String>,
}
#[derive(Clone, Debug)]
struct Request {
    revision: u64,
    update: Update,
    reply: mpsc::Sender<Completion>,
}
#[derive(Debug, Default)]
struct State {
    queue: VecDeque<Request>,
    revision: u64,
    failure: Option<(u64, String)>,
}
#[derive(Debug, Default)]
pub(crate) struct Owner {
    state: Mutex<State>,
    wake: Condvar,
}
fn owners() -> &'static Mutex<HashMap<PathBuf, Weak<Owner>>> {
    static OWNERS: OnceLock<Mutex<HashMap<PathBuf, Weak<Owner>>>> = OnceLock::new();
    OWNERS.get_or_init(Default::default)
}
impl Owner {
    pub fn at(root: &Path) -> Arc<Self> {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let mut registry = owners().lock().unwrap_or_else(|e| e.into_inner());
        registry.retain(|_, owner| owner.strong_count() > 0);
        if let Some(owner) = registry.get(root).and_then(Weak::upgrade) {
            return owner;
        }
        let owner = Arc::new(Self::default());
        let alive = owner.clone();
        let key = root.to_path_buf();
        std::thread::Builder::new()
            .name("recent-list".into())
            .spawn(move || alive.run(key))
            .expect("start recent-list owner");
        registry.insert(root.to_path_buf(), Arc::downgrade(&owner));
        owner
    }
    fn run(self: Arc<Self>, root: PathBuf) {
        let list = RecentList::in_dir(&root);
        loop {
            let next = {
                // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.queue.is_empty()
                    || state
                        .failure
                        .as_ref()
                        .is_some_and(|(r, _)| *r == state.revision)
                {
                    let (state, _) = self
                        .wake
                        // design-lint: allow(ui-blocking-io) PRJ-031: worker-only helper; UI callers submit/poll and never call this boundary directly.
                        .wait_timeout(state, Duration::from_secs(1))
                        .unwrap_or_else(|e| e.into_inner());
                    drop(state);
                    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                    let mut registry = owners().lock().unwrap_or_else(|e| e.into_inner());
                    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
                    let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                    if Arc::strong_count(&self) == 1 && state.queue.is_empty() {
                        registry.remove(&root);
                        return;
                    }
                    None
                } else {
                    Some((
                        state.queue.front().expect("nonempty queue").clone(),
                        state.revision,
                    ))
                }
            };
            let Some((request, attempt)) = next else {
                continue;
            };
            // The supervisor retains the command until update AND readback succeed.
            // A panic cannot disconnect submit or discard accepted FIFO entries.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match &request.update {
                    Update::Remember(dir, name, draft, now) => {
                        list.remember(dir, name, *draft, *now)
                    }
                    Update::Forget(dir) => list.forget(dir),
                    Update::Closed(open) => list.closed_with(open),
                    Update::Refresh => Ok(()),
                }
                .map_err(|e| e.to_string())?;
                #[cfg(test)]
                let _ = xirius_core::io_test::before("recent_readback", list.path());
                Ok(list.songs())
            }))
            .unwrap_or_else(|_| Err("recent-list worker panicked; update retained".into()));
            if let Err(error) = &result {
                // design-lint: allow(ui-blocking-io) PRJ-031: worker-only error report, outside the request/state mutex.
                eprintln!("recent list update failed: {error}");
            }
            let _ = request.reply.send(Completion {
                revision: request.revision,
                result: result.clone(),
            });
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match result {
                Ok(_) => {
                    state.queue.pop_front();
                    state.failure = None;
                }
                Err(error) => state.failure = Some((attempt, error)),
            }
        }
    }
    pub fn submit(&self, update: Update) -> mpsc::Receiver<Completion> {
        let (reply, rx) = mpsc::channel();
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.revision += 1;
        let revision = state.revision;
        state.queue.push_back(Request {
            revision,
            update,
            reply,
        });
        self.wake.notify_one();
        rx
    }
    pub fn status(&self) -> Option<Result<(), String>> {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((revision, error)) = &state.failure
            && *revision == state.revision
        {
            Some(Err(error.clone()))
        } else if state.queue.is_empty() {
            Some(Ok(()))
        } else {
            None
        }
    }
    #[cfg(test)]
    pub fn pending(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .queue
            .len()
    }
    #[cfg(test)]
    pub fn settle(&self) {
        let end = Instant::now() + Duration::from_secs(15);
        while self.status().is_none() {
            assert!(Instant::now() < end, "recent-list owner did not finish");
            std::thread::yield_now();
        }
    }
}
/// Terminal errors/timeouts are logged once by the caller; no invisible wait forever.
pub(crate) fn finish(since: Instant) -> Option<Vec<String>> {
    let statuses: Vec<_> = owners()
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .map(|o| o.status())
        .collect();
    finish_statuses(since.elapsed(), statuses)
}
fn finish_statuses(
    elapsed: Duration,
    statuses: Vec<Option<Result<(), String>>>,
) -> Option<Vec<String>> {
    if elapsed < CLOSE_WAIT && statuses.iter().any(Option::is_none) {
        return None;
    }
    Some(
        statuses
            .into_iter()
            .filter_map(|s| match s {
                Some(Ok(())) => None,
                Some(Err(e)) => Some(e),
                None => Some("recent list update unfinished at shutdown deadline".into()),
            })
            .collect(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shutdown_is_terminal_on_failure_or_deadline() {
        assert!(finish_statuses(Duration::ZERO, vec![None]).is_none());
        assert_eq!(finish_statuses(CLOSE_WAIT, vec![None]).unwrap().len(), 1);
        assert_eq!(
            finish_statuses(Duration::ZERO, vec![Some(Err("denied".into()))]),
            Some(vec!["denied".into()])
        );
    }
}
