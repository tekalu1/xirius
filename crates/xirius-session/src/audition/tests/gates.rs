use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
#[derive(Default)]
pub(super) struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    entered: AtomicUsize,
}

impl Gate {
    pub(super) fn wait(&self) {
        let mut open = lock(&self.open);
        self.entered.fetch_add(1, SeqCst);
        self.changed.notify_all();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
    pub(super) fn entered(&self, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut open = lock(&self.open);
        while self.entered.load(SeqCst) < n {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "gate not entered {n} times");
            open = self.changed.wait_timeout(open, left).unwrap().0;
        }
    }
    pub(super) fn release(&self) {
        *lock(&self.open) = true;
        self.changed.notify_all();
    }
}

pub(super) struct LetGo(pub(super) Arc<Gate>);
impl Drop for LetGo {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub(super) struct Gated {
    pub stat: Arc<Gate>,
    pub open: Arc<Gate>,
    pub block_stat: AtomicBool,
}
impl Reader for Gated {
    fn stat(&self, path: &Path) -> Result<Stamp, String> {
        if self.block_stat.load(SeqCst) {
            self.stat.wait();
        }
        Disk.stat(path)
    }
    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>, String> {
        self.open.wait();
        Disk.open(path)
    }
}

pub(super) fn reader() -> Arc<Gated> {
    Arc::new(Gated {
        stat: Arc::default(),
        open: Arc::default(),
        block_stat: AtomicBool::new(false),
    })
}

use super::*;

/// Per-file gates hold metadata, open, or the read after the first 64 KiB.
#[derive(Default)]
pub(super) struct ReaderGates {
    pub stat: Mutex<HashMap<PathBuf, Arc<Gate>>>,
    pub open: Mutex<HashMap<PathBuf, Arc<Gate>>>,
    pub read: Mutex<HashMap<PathBuf, Arc<Gate>>>,
    opens: Mutex<HashMap<PathBuf, usize>>,
}

impl ReaderGates {
    pub fn opens(&self, path: &Path) -> usize {
        *lock(&self.opens).get(path).unwrap_or(&0)
    }
}

impl Reader for ReaderGates {
    fn stat(&self, path: &Path) -> Result<Stamp, String> {
        let gate = lock(&self.stat).get(path).cloned();
        if let Some(g) = gate {
            g.wait();
        }
        Disk.stat(path)
    }
    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>, String> {
        *lock(&self.opens).entry(path.to_path_buf()).or_default() += 1;
        let gate = lock(&self.open).get(path).cloned();
        if let Some(g) = gate {
            g.wait();
        }
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        Ok(Box::new(SteppedRead {
            file,
            gate: lock(&self.read).get(path).cloned(),
            read: 0,
        }))
    }
}

struct SteppedRead {
    file: std::fs::File,
    gate: Option<Arc<Gate>>,
    read: usize,
}
impl Read for SteppedRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if let Some(g) = &self.gate
            && self.read >= 64 << 10
        {
            g.wait();
        }
        let n = if self.gate.is_some() && self.read < 64 << 10 {
            buf.len().min((64 << 10) - self.read)
        } else {
            buf.len()
        };
        let n = self.file.read(&mut buf[..n])?;
        self.read += n;
        Ok(n)
    }
}

pub(super) fn hold(map: &Mutex<HashMap<PathBuf, Arc<Gate>>>, path: &Path) -> LetGo {
    let gate = Arc::new(Gate::default());
    lock(map).insert(path.to_path_buf(), gate.clone());
    LetGo(gate)
}

pub(super) fn barrier(a: &Audition) {
    let (tx, rx) = mpsc::channel();
    a.tx.send(Msg::Barrier(tx)).unwrap();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
}

pub(super) fn drained(a: &Audition, reader: &Arc<ReaderGates>) {
    barrier(a);
    let end = Instant::now() + Duration::from_secs(20);
    while Arc::strong_count(reader) > 2 {
        assert!(Instant::now() < end, "reader did not return");
        std::thread::yield_now();
    }
    barrier(a);
}
