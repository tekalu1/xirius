//! Bounded process-owned I/O workers. Dropping a client cancels queued work,
//! never joins stalled I/O on the UI thread. Keys coalesce outstanding requests.
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

pub const THREADS: usize = 4;
pub const QUEUED: usize = 128;
type Task = (u64, u64, Box<dyn FnOnce() + Send>);
struct Pool {
    tasks: Mutex<VecDeque<Task>>,
    wake: Condvar,
}
fn pool() -> &'static Arc<Pool> {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let p = Arc::new(Pool {
            tasks: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
        });
        for _ in 0..THREADS {
            let work = p.clone();
            std::thread::Builder::new()
                .name("xirius-read".into())
                .spawn(move || {
                    loop {
                        let task = {
                            let mut q = work.tasks.lock().unwrap_or_else(|e| e.into_inner());
                            while q.is_empty() {
                                q = work.wake.wait(q).unwrap_or_else(|e| e.into_inner());
                            }
                            q.pop_front().expect("nonempty queue").2
                        };
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                    }
                })
                .expect("start bounded I/O worker");
        }
        p
    })
}
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Entry<V> {
    id: u64,
    value: Option<Result<V, ()>>,
    wanted: bool,
}
type Results<K, V> = HashMap<K, Entry<V>>;
pub struct ReadJobs<K, V> {
    owner: u64,
    results: Arc<Mutex<Results<K, V>>>,
}
impl<K, V> Default for ReadJobs<K, V> {
    fn default() -> Self {
        Self {
            owner: NEXT.fetch_add(1, Ordering::Relaxed),
            results: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
impl<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static> ReadJobs<K, V> {
    /// None means back pressure; the caller keeps its old display and retries later.
    pub fn request(&self, key: K, read: impl FnOnce() -> V + Send + 'static) -> Option<u64> {
        self.submit(key, read, false)
    }
    /// A write already accepted by the UI outlives its client; still bounded by
    /// the same queue and worker count. Read-only requests use request instead.
    pub fn request_persistent(
        &self,
        key: K,
        read: impl FnOnce() -> V + Send + 'static,
    ) -> Option<u64> {
        self.submit(key, read, true)
    }
    fn submit(
        &self,
        key: K,
        read: impl FnOnce() -> V + Send + 'static,
        persistent: bool,
    ) -> Option<u64> {
        let mut results = self.results.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = results.get_mut(&key)
            && entry.value.is_none()
        {
            entry.wanted = true;
            return Some(entry.id);
        }
        let mut q = pool().tasks.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= QUEUED {
            return None;
        }
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        results.insert(
            key.clone(),
            Entry {
                id,
                value: None,
                wanted: true,
            },
        );
        let weak = Arc::downgrade(&self.results);
        let owned = persistent.then(|| self.results.clone());
        q.push_back((
            if persistent { 0 } else { self.owner },
            id,
            Box::new(move || {
                let Some(results) = owned.or_else(|| weak.upgrade()) else {
                    return;
                };
                let value =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).map_err(|_| ());
                let mut results = results.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = results.get_mut(&key)
                    && entry.id == id
                {
                    if entry.wanted {
                        entry.value = Some(value);
                    } else {
                        results.remove(&key);
                    }
                }
            }),
        ));
        pool().wake.notify_one();
        Some(id)
    }
    pub fn take(&self, key: &K, id: u64) -> Option<Result<V, ()>> {
        let mut r = self.results.lock().unwrap_or_else(|e| e.into_inner());
        if r.get(key).is_some_and(|e| e.id == id && e.value.is_some()) {
            r.remove(key)?.value
        } else {
            None
        }
    }
    pub fn current_id(&self, key: &K) -> Option<u64> {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .map(|e| e.id)
    }
    pub fn peek(&self, key: &K, id: u64) -> Option<Result<V, ()>>
    where
        V: Clone,
    {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .filter(|e| e.id == id)
            .and_then(|e| e.value.clone())
    }
    /// Completed cache data is optional; outstanding handles retry evicted keys.
    pub fn trim_ready(&self, limit: usize) {
        let mut r = self.results.lock().unwrap_or_else(|e| e.into_inner());
        let mut ready: Vec<_> = r
            .iter()
            .filter(|(_, e)| e.value.is_some())
            .map(|(key, e)| (e.id, key.clone()))
            .collect();
        ready.sort_by_key(|(id, _)| *id);
        let remove = ready.len().saturating_sub(limit);
        for (_, key) in ready.into_iter().take(remove) {
            r.remove(&key);
        }
    }
    /// Cancel obsolete queued jobs, retain running keys so revisits coalesce.
    pub fn retain(&self, keep: impl Fn(&K) -> bool) {
        let mut r = self.results.lock().unwrap_or_else(|e| e.into_inner());
        let mut q = pool().tasks.lock().unwrap_or_else(|e| e.into_inner());
        let mut removed = Vec::new();
        let keep_ids: HashSet<_> = r
            .iter()
            .filter(|(k, _)| keep(k))
            .map(|(_, e)| e.id)
            .collect();
        q.retain(|(owner, id, _)| {
            let retain = *owner != self.owner || keep_ids.contains(id);
            if !retain {
                removed.push(*id);
            }
            retain
        });
        r.retain(|k, e| {
            e.wanted = keep(k);
            !removed.contains(&e.id) && (e.value.is_none() || e.wanted)
        });
    }
    pub fn pending(&self) -> bool {
        self.results
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .any(|e| e.value.is_none())
    }
    /// Forget a result without losing the identity of an in-flight job.
    pub fn discard(&self, key: &K, id: u64) {
        let mut r = self.results.lock().unwrap_or_else(|e| e.into_inner());
        if !r.get(key).is_some_and(|e| e.id == id) {
            return;
        }
        let mut q = pool().tasks.lock().unwrap_or_else(|e| e.into_inner());
        let was_queued = q
            .iter()
            .any(|(owner, i, _)| *owner == self.owner && *i == id);
        q.retain(|(owner, i, _)| *owner != self.owner || *i != id);
        if was_queued || r.get(key).is_some_and(|e| e.value.is_some()) {
            r.remove(key);
        } else if let Some(entry) = r.get_mut(key) {
            entry.wanted = false;
        }
    }
    #[cfg(any(test, feature = "test-io"))]
    pub fn counts(&self) -> (usize, usize) {
        let r = self.results.lock().unwrap_or_else(|e| e.into_inner());
        let queued = pool()
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(owner, _, _)| *owner == self.owner)
            .count();
        let pending = r.values().filter(|e| e.value.is_none()).count();
        (pending.saturating_sub(queued), queued)
    }
}
impl<K, V> Drop for ReadJobs<K, V> {
    fn drop(&mut self) {
        pool()
            .tasks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(owner, _, _)| *owner != self.owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }
    #[test]
    fn bounded_queue_coalesces_running_keys_and_cancels_obsolete_work() {
        let jobs = ReadJobs::<usize, usize>::default();
        let release = Release(Arc::new((Mutex::new(false), Condvar::new())));
        let gate = release.0.clone();
        let (tx, entered) = std::sync::mpsc::channel();
        let first = jobs
            .request(0, move || {
                let _ = tx.send(());
                let guard = gate.0.lock().unwrap();
                drop(gate.1.wait_while(guard, |ready| !*ready).unwrap());
                0
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
        for _ in 0..100 {
            assert_eq!(jobs.request(0, || 999), Some(first));
        }
        for key in 1..1000 {
            let gate = release.0.clone();
            let _ = jobs.request(key, move || {
                let guard = gate.0.lock().unwrap();
                drop(gate.1.wait_while(guard, |ready| !*ready).unwrap());
                key
            });
        }
        let (running, queued) = jobs.counts();
        assert!(running <= THREADS);
        assert!(queued <= QUEUED);
        assert!(running + queued <= THREADS + QUEUED);
        jobs.retain(|key| *key == 0);
        assert_eq!(jobs.counts().1, 0);
        drop(release);
        let until = Instant::now() + Duration::from_secs(15);
        while jobs.pending() {
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
        assert_eq!(jobs.take(&0, first), Some(Ok(0)));
        assert_eq!(jobs.counts(), (0, 0));
    }
    #[test]
    fn a_panicking_read_finishes_as_an_error_and_the_pool_stays_usable() {
        let jobs = ReadJobs::<u8, bool>::default();
        let until = Instant::now() + Duration::from_secs(15);
        // The parallel queue-capacity test shares the process-wide pool. None
        // is back pressure, not a failed reader: retry admission as clients do.
        let id = loop {
            if let Some(id) = jobs.request(0, || panic!("failed reader")) {
                break id;
            }
            assert!(Instant::now() < until);
            std::thread::yield_now();
        };
        let result = loop {
            if let Some(result) = jobs.take(&0, id) {
                break result;
            }
            assert!(Instant::now() < until);
            std::thread::yield_now();
        };
        assert_eq!(result, Err(()));
        let id = loop {
            if let Some(id) = jobs.request(1, || true) {
                break id;
            }
            assert!(Instant::now() < until);
            std::thread::yield_now();
        };
        loop {
            if let Some(result) = jobs.take(&1, id) {
                assert_eq!(result, Ok(true));
                break;
            }
            assert!(Instant::now() < until);
            std::thread::yield_now();
        }
    }
}
