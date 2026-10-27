//! Ordered file requests. Callers never retain a device/settings lock during I/O.
use super::*;
use crate::io_store::Routes;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

enum Request {
    Read(PathBuf, u64),
    Write(PathBuf, Routes),
    Fence(mpsc::Sender<()>),
}

pub(super) struct Fence(mpsc::SyncSender<Request>);
impl Fence {
    /// Only a command worker waits here. Older automatic route writes must land
    /// before that worker reads/modifies/persists an explicit routing choice.
    pub(super) fn wait(self) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.0.send(Request::Fence(tx)).map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())
    }
}

enum Reply {
    Read(u64, Result<Option<Routes>, String>),
    Write(Result<(), String>),
}
pub(super) struct Files {
    tx: mpsc::SyncSender<Request>,
    rx: mpsc::Receiver<Reply>,
    revision: u64,
    reading: bool,
    due: Instant,
    dir: Option<PathBuf>,
}
impl Files {
    pub(super) fn new() -> Result<Self, String> {
        let (tx, requests) = mpsc::sync_channel(64);
        let (replies, rx) = mpsc::channel();
        let _owner = crate::background::Job::spawn("audio-routing", move || {
            while let Ok(request) = requests.recv() {
                let reply = match request {
                    Request::Read(dir, rev) => {
                        let value = crate::io_store::read(&dir);
                        #[cfg(feature = "test-io")]
                        let _ = xirius_core::io_test::before("io_route_ready", &dir);
                        Reply::Read(rev, value)
                    }
                    Request::Write(dir, routes) => {
                        Reply::Write(crate::io_store::write(&dir, &routes))
                    }
                    Request::Fence(done) => {
                        let _ = done.send(());
                        continue;
                    }
                };
                let _ = replies.send(reply);
            }
            Ok(())
        })?;
        Ok(Self {
            tx,
            rx,
            revision: 0,
            reading: false,
            due: Instant::now(),
            dir: None,
        })
    }
    pub(super) fn load(&mut self, dir: &Path) -> Result<Option<Routes>, String> {
        if self.dir.as_deref() != Some(dir) {
            self.dir = Some(dir.to_owned());
            self.revision += 1;
            self.due = Instant::now();
        }
        let mut ready = None;
        while let Ok(reply) = self.rx.try_recv() {
            match reply {
                Reply::Read(rev, value) => {
                    self.reading = false;
                    if rev == self.revision {
                        ready = value?;
                    }
                }
                Reply::Write(value) => value?,
            }
        }
        if !self.reading && Instant::now() >= self.due {
            self.tx
                .try_send(Request::Read(dir.to_owned(), self.revision))
                .map_err(|_| "入出力設定の読み込みを開始できません".to_string())?;
            self.reading = true;
            self.due = Instant::now() + Duration::from_millis(250);
        }
        Ok(ready)
    }
    pub(super) fn save(&mut self, dir: &Path, routes: Routes) -> Result<(), String> {
        self.tx
            .try_send(Request::Write(dir.to_owned(), routes))
            .map_err(|_| "入出力設定の保存を開始できません".to_string())?;
        self.revision += 1;
        Ok(())
    }
    pub(super) fn invalidate(&mut self) {
        self.revision += 1;
    }
    pub(super) fn fence(&self) -> Fence {
        Fence(self.tx.clone())
    }
}

#[cfg(all(test, feature = "test-io"))]
mod tests {
    use super::*;
    use std::sync::{Condvar, mpsc};
    #[test]
    fn a_late_route_read_is_discarded_after_an_explicit_change() {
        let root = tempfile::tempdir().unwrap();
        crate::io_store::write(
            root.path(),
            &Routes {
                output: [0, 1],
                ..Routes::default()
            },
        )
        .unwrap();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let path = root.path().to_owned();
        let ui = std::thread::current().id();
        let (tx, entered) = mpsc::channel();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, file| {
            if op == "io_route_ready" && file == path {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |done| !*done)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        let mut files = Files::new().unwrap();
        assert!(files.load(root.path()).unwrap().is_none());
        entered.recv_timeout(Duration::from_secs(20)).unwrap();
        crate::io_store::write(
            root.path(),
            &Routes {
                output: [2, 3],
                ..Routes::default()
            },
        )
        .unwrap();
        files.invalidate();
        for _ in 0..16 {
            assert!(files.load(root.path()).unwrap().is_none());
        }
        drop(release);
        let deadline = Instant::now() + Duration::from_secs(20);
        while files.reading && Instant::now() < deadline {
            assert!(files.load(root.path()).unwrap().is_none());
            std::thread::yield_now();
        }
        assert!(!files.reading);
        assert_eq!(
            crate::io_store::read(root.path()).unwrap().unwrap().output,
            [2, 3]
        );
    }
}
