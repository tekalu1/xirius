//! PRJ-031: one FIFO owner per recent-list root. No disk/OS lock is held by submit/poll.
use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak, mpsc},
};

use crate::keep::{LastOpen, RecentList, RecentSong};

#[derive(Clone, Debug)]
pub(crate) enum Update {
    Remember(PathBuf, String, bool, u64),
    Forget(PathBuf),
    Closed(LastOpen),
    Refresh,
}
#[derive(Debug, Default)]
struct State {
    pending: usize,
    revision: u64,
    failure: Option<(u64, String)>,
}
type Completion = (Vec<RecentSong>, Option<String>);
#[derive(Debug)]
pub(crate) struct Owner {
    tx: mpsc::Sender<(u64, Update, mpsc::Sender<Completion>)>,
    state: Arc<Mutex<State>>,
}
fn owners() -> &'static Mutex<HashMap<PathBuf, Weak<Owner>>> {
    static OWNERS: OnceLock<Mutex<HashMap<PathBuf, Weak<Owner>>>> = OnceLock::new();
    OWNERS.get_or_init(Default::default)
}
impl Owner {
    pub fn at(root: &Path) -> Arc<Self> {
        let mut registry = owners().lock().unwrap();
        registry.retain(|_, owner| owner.strong_count() > 0);
        if let Some(owner) = registry.get(root).and_then(Weak::upgrade) {
            return owner;
        }
        let (tx, rx) = mpsc::channel::<(u64, Update, mpsc::Sender<Completion>)>();
        let state = Arc::new(Mutex::new(State::default()));
        let owner = Arc::new(Self {
            tx,
            state: state.clone(),
        });
        let alive = owner.clone();
        let key = root.to_path_buf();
        let list = RecentList::in_dir(root);
        std::thread::Builder::new()
            .name("recent-list".into())
            .spawn(move || {
                let mut queue = VecDeque::new();
                // A failure retains the exact prefix. A later submission retries it
                // before newer edits; a failed remember can never follow its forget.
                loop {
                    let message = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                        Ok(message) => message,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            let mut owners = owners().lock().unwrap();
                            if Arc::strong_count(&alive) == 1 && state.lock().unwrap().pending == 0
                            {
                                owners.remove(&key);
                                break;
                            }
                            continue;
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let (revision, update, reply) = message;
                    queue.push_back((update, reply));
                    while let Some((update, reply)) = queue.front() {
                        let result = match update {
                            Update::Remember(dir, name, draft, now) => {
                                list.remember(dir, name, *draft, *now)
                            }
                            Update::Forget(dir) => list.forget(dir),
                            Update::Closed(open) => list.closed_with(open),
                            Update::Refresh => Ok(()),
                        };
                        match result {
                            Ok(()) => {
                                let songs = list.songs();
                                let _ = reply.send((songs, None));
                                queue.pop_front();
                                let mut state = state.lock().unwrap();
                                state.pending -= 1;
                                state.failure = None;
                            }
                            Err(error) => {
                                let error = error.to_string();
                                let _ = reply.send((Vec::new(), Some(error.clone())));
                                state.lock().unwrap().failure = Some((revision, error));
                                break;
                            }
                        }
                    }
                }
            })
            .expect("start recent-list owner");
        registry.insert(root.to_path_buf(), Arc::downgrade(&owner));
        owner
    }
    pub fn submit(&self, update: Update) -> mpsc::Receiver<Completion> {
        let (tx, rx) = mpsc::channel();
        // Serialize enqueue and accounting, never the transaction itself.
        let mut state = self.state.lock().unwrap();
        state.pending += 1;
        state.revision += 1;
        state.failure = None;
        self.tx
            .send((state.revision, update, tx))
            .expect("recent-list owner alive");
        rx
    }
    pub fn status(&self) -> Option<Result<(), String>> {
        let state = self.state.lock().unwrap();
        if let Some((revision, error)) = &state.failure
            && *revision == state.revision
        {
            Some(Err(error.clone()))
        } else if state.pending == 0 {
            Some(Ok(()))
        } else {
            None
        }
    }
    #[cfg(test)]
    pub fn settle(&self) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while self.status().is_none() {
            assert!(
                std::time::Instant::now() < end,
                "recent-list owner did not finish"
            );
            std::thread::yield_now();
        }
    }
}

/// Last-window fallback: keep pumping until every accepted update has completed.
/// Normal closes check status while their window is still available to show errors.
pub(crate) fn finish() -> bool {
    owners()
        .lock()
        .unwrap()
        .values()
        .filter_map(Weak::upgrade)
        .all(|owner| matches!(owner.status(), Some(Ok(()))))
}
