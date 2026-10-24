//! PRJ-028: one worker owns both sessions until the caller adopts or aborts.
//! No session is put in the ready queue. The final handoff is a rendezvous, so
//! disconnecting a window disposes of its sessions on the worker, never on UI.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use super::{Session, same_path};
use crate::{CmdError, CmdResult};

/// Identity supplied by the owner; both fields must still match at adoption.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenId {
    pub job: u64,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub enum OpenRequest {
    Song(PathBuf),
    Version { id: String, destination: PathBuf },
}

/// Latest progress, independent of the ready and completion channels.
#[derive(Clone, Debug, Default)]
pub struct OpenStatus {
    pub reads: u64,
    pub current: PathBuf,
    pub stage: &'static str,
}

#[derive(Default)]
pub struct OpenProgress {
    decision: AtomicU8,
    reads: AtomicU64,
    status: Mutex<OpenStatus>,
    retiring: AtomicBool,
    blobs: Mutex<std::collections::HashMap<PathBuf, Vec<u8>>>,
    #[cfg(test)]
    hook: Mutex<Option<Hook>>,
    #[cfg(test)]
    ended: Mutex<Option<mpsc::Sender<()>>>,
}

#[cfg(test)]
type Hook = Arc<dyn Fn(&str, &Path) + Send + Sync>;

impl OpenProgress {
    /// False once adoption has committed; otherwise cancellation wins even
    /// if Ready or Adopt was already sent. No OS operation is waited for here.
    pub fn cancel(&self) -> bool {
        self.decision
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || self.cancelled()
    }

    pub fn cancelled(&self) -> bool {
        self.decision.load(Ordering::Acquire) == 1
    }

    /// A busy progress lock never delays a frame.
    pub fn latest(&self) -> Option<OpenStatus> {
        let mut status = self.status.try_lock().ok()?.clone();
        status.reads = self.reads.load(Ordering::Relaxed);
        Some(status)
    }

    fn stage(&self, stage: &'static str, path: &Path) {
        if let Ok(mut s) = self.status.try_lock() {
            s.stage = stage;
            s.current = path.to_path_buf();
        }
    }
}

thread_local! {
    static PROGRESS: RefCell<Option<Arc<OpenProgress>>> = const { RefCell::new(None) };
}

/// The destination is never owned by cleanup. Only an exclusively created,
/// unpredictable sibling is private, and it stops being ours at publication.
struct CreatedSong {
    root: PathBuf,
    destination: PathBuf,
    published: bool,
}
impl CreatedSong {
    fn prepare(destination: &Path) -> CmdResult<Self> {
        use std::hash::{BuildHasher, Hasher};
        let destination = std::path::absolute(destination)?;
        let parent = destination
            .parent()
            .ok_or_else(|| CmdError::io("曲の保存先を指定できません"))?;
        io::create_dir_all(parent)?;
        let parent = parent.to_path_buf();
        let destination = parent.join(
            destination
                .file_name()
                .ok_or_else(|| CmdError::io("曲の保存先を指定できません"))?,
        );
        // Early diagnosis only: the final no-replace rename is the authority.
        before("metadata", &destination)?;
        match std::fs::symlink_metadata(&destination) {
            Ok(_) => return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        for _ in 0..32 {
            let token = || {
                let mut h = std::collections::hash_map::RandomState::new().build_hasher();
                h.write_u32(std::process::id());
                h.finish()
            };
            let root = parent.join(format!(".xirius-open-{:016x}{:016x}", token(), token()));
            before("mkdir", &root)?;
            match std::fs::create_dir(&root) {
                Ok(()) => {
                    return Ok(Self {
                        root,
                        destination,
                        published: false,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(CmdError::io("曲の作業フォルダーを作成できません"))
    }

    fn publish(&mut self, candidate: Session) -> CmdResult<Session> {
        let registry = candidate.registry.clone();
        // Windows can refuse to rename a directory with open descendants.
        // Retire the private candidate before publication. Never carry its lane
        // across the unlocked publication gap: a public opener may take lane 0.
        drop(candidate);
        before("publish", &self.destination)?;
        // One no-replace rename, with no copy/link fallback. Sibling placement
        // keeps it on one volume; EXDEV/unsupported filesystems fail closed.
        rename_no_replace(&self.root, &self.destination)?;
        // Disarm BEFORE any further operation: a published song can be opened
        // and saved by another session, even if notification later fails.
        self.published = true;
        // A failure to reopen leaves the complete published song untouched and
        // returns the original session. Normal lane acquisition also handles a
        // concurrent public opener without sharing its history lane.
        Session::open_inner(&self.destination, registry, &mut None)
    }

    fn cleanup(self) -> CmdResult<()> {
        if !self.published {
            io::remove_dir_all(&self.root)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (from, to) = (wide(from), wide(to));
    // SAFETY: both NUL-terminated paths outlive the call. Flags 0 prohibit
    // replacement and cross-volume copying, including for directories.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| {
        std::ffi::CString::new(p.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
    };
    let (from, to) = (c(from)?, c(to)?);
    // SAFETY: both NUL-terminated paths outlive the call; exclusive rename
    // never replaces an existing destination, including an empty directory.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    // SAFETY: as above.
    #[cfg(target_os = "macos")]
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn rename_no_replace(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

struct Scope(Option<Arc<OpenProgress>>);
impl Scope {
    fn enter(progress: Arc<OpenProgress>) -> Self {
        Self(PROGRESS.with(|p| p.replace(Some(progress))))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        PROGRESS.with(|p| p.replace(self.0.take()));
    }
}

/// Session teardown must not skip marker removal because its load was
/// cancelled, including intermediate sessions made while copying a version.
pub(crate) struct Retiring(Option<(Arc<OpenProgress>, bool)>);
impl Retiring {
    pub(crate) fn enter() -> Self {
        Self(PROGRESS.with(|p| {
            p.borrow()
                .as_ref()
                .map(|p| (p.clone(), p.retiring.swap(true, Ordering::Relaxed)))
        }))
    }
}
impl Drop for Retiring {
    fn drop(&mut self) {
        if let Some((p, was)) = &self.0 {
            p.retiring.store(*was, Ordering::Relaxed);
        }
    }
}

pub(crate) fn check() -> CmdResult<()> {
    PROGRESS.with(|p| {
        if p.borrow()
            .as_ref()
            .is_some_and(|p| p.cancelled() && !p.retiring.load(Ordering::Relaxed))
        {
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into())
        } else {
            Ok(())
        }
    })
}

/// I/O observation is scoped to this worker. Synchronous CLI readers use the
/// same functions without installing progress, cancellation or a test gate.
pub(crate) fn before(op: &'static str, path: &Path) -> std::io::Result<()> {
    #[cfg(any(test, feature = "test-io"))]
    crate::io_test::before(op, path)?;
    PROGRESS.with(|p| {
        if let Some(p) = p.borrow().as_ref() {
            p.stage(op, path);
            #[cfg(test)]
            let hook = p.hook.lock().unwrap().clone();
            #[cfg(test)]
            if let Some(hook) = hook {
                hook(op, path);
            }
            if p.cancelled() && !p.retiring.load(Ordering::Relaxed) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
        }
        Ok(())
    })
}

/// The lightweight message sent before either session can leave the worker.
#[derive(Debug)]
pub struct OpenReady {
    pub id: OpenId,
    pub name: String,
}

/// A successful adoption, or the original session with its exact in-memory
/// history, selection, AI work, writer and lane on failure/cancellation.
pub struct OpenOutcome {
    pub id: OpenId,
    pub session: Session,
    pub result: CmdResult<()>,
}

/// A failed spawn returns the original session without dropping it.
pub struct OpenStartError {
    pub session: Box<Session>,
    pub error: std::io::Error,
}

pub struct OpenJob {
    pub id: OpenId,
    pub progress: Arc<OpenProgress>,
    ready: mpsc::Receiver<OpenReady>,
    decision: mpsc::Sender<bool>,
    outcome: mpsc::Receiver<OpenOutcome>,
}

fn load_failed() -> CmdError {
    CmdError::io("曲の読み込みとの接続が切れました")
}
fn write_failed() -> CmdError {
    CmdError::io("曲の保存が完了していません")
}

/// Owns every transferable resource until the rendezvous completes. Closing
/// Ready before sending a failure also wakes a caller still waiting for Ready.
struct Delivery {
    session: Option<Session>,
    candidate: Option<Session>,
    loan: Option<super::lane::Lane>,
    same: bool,
    ready: Option<mpsc::Sender<OpenReady>>,
    out: mpsc::SyncSender<OpenOutcome>,
    id: OpenId,
    progress: Arc<OpenProgress>,
    created: Option<CreatedSong>,
    adopted: bool,
}

#[cfg(test)]
struct EndReport(Arc<OpenProgress>);
#[cfg(test)]
impl Drop for EndReport {
    fn drop(&mut self) {
        if let Some(tx) = self.0.ended.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
}
impl Delivery {
    fn send(&mut self, mut result: CmdResult<()>) {
        let _scope = Scope::enter(self.progress.clone());
        let _retiring = Retiring::enter();
        if self.same {
            let lane = self
                .candidate
                .as_mut()
                .and_then(|s| s.lane.take())
                .or_else(|| self.loan.take());
            if let Some(old) = &mut self.session {
                old.lane = lane;
            }
        }
        drop(self.candidate.take());
        drop(self.loan.take());
        if let Some(created) = self.created.take()
            && let Err(error) = created.cleanup()
        {
            result = Err(error);
        }
        drop(self.ready.take());
        self.progress
            .blobs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(session) = self.session.take() {
            let _ = self.out.send(OpenOutcome {
                id: self.id,
                session,
                result,
            });
        }
    }
}
impl Drop for Delivery {
    fn drop(&mut self) {
        if self.session.is_some() {
            // Adoption is already committed once the old session retires. If
            // its Drop unwinds, return the guarded, valid replacement as a
            // success so the screen performs its normal logical switch too.
            let result = if self.adopted {
                self.session
                    .as_mut()
                    .unwrap()
                    .notices
                    .push("前の曲の終了処理を完了できませんでした".into());
                Ok(())
            } else {
                Err(load_failed())
            };
            self.send(result);
        }
    }
}

impl OpenJob {
    pub fn start(old: Session, request: OpenRequest, id: OpenId) -> Result<Self, OpenStartError> {
        Self::start_with(old, request, id, Arc::new(OpenProgress::default()))
    }

    fn start_with(
        old: Session,
        request: OpenRequest,
        id: OpenId,
        progress: Arc<OpenProgress>,
    ) -> Result<Self, OpenStartError> {
        let (ready_tx, ready) = mpsc::channel();
        let (decision, decide) = mpsc::channel();
        let (out, outcome) = mpsc::sync_channel(0);
        // Spawn failure must not drop the closure's session on the calling thread.
        let escrow = Arc::new(Mutex::new(Some(old)));
        let owned = escrow.clone();
        let report = progress.clone();
        let spawned = std::thread::Builder::new()
            .name("xirius-open".into())
            .spawn(move || {
                #[cfg(test)]
                let _ended = EndReport(report.clone());
                let original = owned.lock().unwrap().take().unwrap();
                let mut delivery = Delivery {
                    session: Some(original),
                    candidate: None,
                    loan: None,
                    same: false,
                    ready: Some(ready_tx),
                    out,
                    id,
                    progress: report.clone(),
                    created: None,
                    adopted: false,
                };
                let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _scope = Scope::enter(report.clone());
                    let path = match &request {
                        OpenRequest::Song(path) => path,
                        OpenRequest::Version { destination, .. } => destination,
                    };
                    before("writer", path)?;
                    let old = delivery.session.as_mut().unwrap();
                    old.writer.flush();
                    check()?;
                    if old.writer.has_errors() {
                        return Err(write_failed());
                    }
                    let path = match &request {
                        OpenRequest::Song(path) => path,
                        OpenRequest::Version { id, destination } => {
                            delivery.created = Some(CreatedSong::prepare(destination)?);
                            let stage = &delivery.created.as_ref().unwrap().root;
                            drop(old.open_version_as_song(id, stage)?);
                            stage
                        }
                    };
                    delivery.same = old.store.as_ref().is_some_and(|s| same_path(&s.dir, path));
                    if delivery.same {
                        delivery.loan = old.lane.take();
                    }
                    Session::open_inner(path, old.registry.clone(), &mut delivery.loan)
                }));
                match built.unwrap_or_else(|_| Err(load_failed())) {
                    Ok(candidate) => delivery.candidate = Some(candidate),
                    Err(error) => {
                        delivery.send(Err(error));
                        return;
                    }
                }
                let _retirement = Scope::enter(report.clone());
                let handoff_path = match &request {
                    OpenRequest::Song(path) => path,
                    OpenRequest::Version { destination, .. } => destination,
                };
                // Deliberately outside the construction catch: Delivery also
                // protects the channel/retirement phase and its unwind paths.
                let _ = before("handoff", handoff_path);
                report.blobs.lock().unwrap().clear();
                report.retiring.store(true, Ordering::Relaxed);
                let offered = delivery
                    .ready
                    .as_ref()
                    .unwrap()
                    .send(OpenReady {
                        id,
                        name: delivery.candidate.as_ref().unwrap().current.name.clone(),
                    })
                    .is_ok();
                let adopt = offered && decide.recv().unwrap_or(false) && !report.cancelled();
                let committed = adopt
                    && report
                        .decision
                        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok();
                if committed && let Some(created) = &mut delivery.created {
                    match created.publish(delivery.candidate.take().unwrap()) {
                        Ok(candidate) => delivery.candidate = Some(candidate),
                        Err(error) => {
                            delivery.send(Err(error));
                            return;
                        }
                    }
                }
                let notified = !committed
                    || std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let candidate = delivery.candidate.as_ref().unwrap();
                        let old = delivery.session.as_mut().unwrap();
                        let cx = crate::ExtContext {
                            project: &candidate.current,
                            dir: candidate.store.as_ref().map(|s| s.dir.as_path()),
                            author: &crate::command::Author::Human,
                            registry: &candidate.registry,
                            work_copy: false,
                            lane: candidate.lane.as_ref().map(|l| l.k),
                            uses: candidate.uses.id(),
                        };
                        for extension in &mut old.extensions {
                            extension.song_changed(&cx);
                        }
                        for extension in &mut old.extensions {
                            let _ = extension.take_events();
                        }
                    }))
                    .is_ok();
                let result = if committed && notified {
                    let mut candidate = delivery.candidate.take().unwrap();
                    let old = delivery.session.as_mut().unwrap();
                    candidate.extensions = std::mem::take(&mut old.extensions);
                    old.hand_writer_to(&mut candidate);
                    // The guard owns the adopted session before the old one can
                    // unwind during retirement. Nothing writable is left on UI.
                    let retired = delivery.session.replace(candidate);
                    delivery.adopted = true;
                    delivery.same = false;
                    drop(retired);
                    Ok(())
                } else if !notified {
                    Err(load_failed())
                } else {
                    Err(std::io::Error::from(std::io::ErrorKind::Interrupted).into())
                };
                delivery.send(result);
            });
        match spawned {
            Ok(_) => Ok(Self {
                id,
                progress,
                ready,
                decision,
                outcome,
            }),
            Err(error) => Err(OpenStartError {
                session: Box::new(escrow.lock().unwrap().take().unwrap()),
                error,
            }),
        }
    }

    pub fn try_ready(&self) -> Result<OpenReady, mpsc::TryRecvError> {
        self.ready.try_recv()
    }

    /// An old ready event cannot adopt a new generation. Cancellation is
    /// accepted immediately; the original session returns after OS I/O ends.
    pub fn decide(&self, current: OpenId, adopt: bool) {
        let adopt = adopt && current == self.id && !self.progress.cancelled();
        if !adopt {
            self.progress.cancel();
        }
        let _ = self.decision.send(adopt);
    }

    pub fn cancel(&self) -> bool {
        let accepted = self.progress.cancel();
        let _ = self.decision.send(false);
        accepted
    }

    pub fn try_finish(&self) -> Result<OpenOutcome, mpsc::TryRecvError> {
        self.outcome.try_recv()
    }

    /// Compatibility bridge for existing synchronous screen entry points.
    /// PRJ-029 replaces this wait with tick/idle polling. CLI remains synchronous.
    pub fn finish_blocking(self) -> Result<OpenOutcome, mpsc::RecvError> {
        // Failure can bypass Ready; closing its sender wakes this receiver.
        if let Ok(ready) = self.ready.recv() {
            self.decide(ready.id, true);
        }
        self.outcome.recv()
    }
}

impl Drop for OpenJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Small, operation-specific seam for reads, queries, writes and retirement.
/// File handles and their thread constraints remain the standard library's.
pub(crate) mod io {
    pub use std::fs::*;
    use std::path::Path;

    pub fn read(path: impl AsRef<Path>) -> std::io::Result<Vec<u8>> {
        let path = path.as_ref();
        let progress = super::PROGRESS.with(|p| p.borrow().clone());
        let blob = path.parent().is_some_and(|p| p.ends_with("history/blobs"));
        if blob && let Some(progress) = &progress {
            super::check().map_err(|_| std::io::Error::from(std::io::ErrorKind::Interrupted))?;
            if let Some(bytes) = progress.blobs.lock().unwrap().get(path) {
                return Ok(bytes.clone());
            }
        }
        super::before("read", path)?;
        let bytes = std::fs::read(path)?;
        if blob && let Some(progress) = &progress {
            progress
                .blobs
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), bytes.clone());
        }
        super::PROGRESS.with(|p| {
            if let Some(p) = p.borrow().as_ref() {
                p.reads.fetch_add(1, super::Ordering::Relaxed);
            }
        });
        super::check().map_err(|_| std::io::Error::from(std::io::ErrorKind::Interrupted))?;
        Ok(bytes)
    }

    macro_rules! operation {
        ($name:ident, $op:literal, $result:ty) => {
            pub fn $name(path: impl AsRef<Path>) -> std::io::Result<$result> {
                super::before($op, path.as_ref())?;
                std::fs::$name(path)
            }
        };
    }
    operation!(metadata, "metadata", std::fs::Metadata);
    operation!(read_dir, "read_dir", std::fs::ReadDir);
    operation!(canonicalize, "canonicalize", std::path::PathBuf);
    operation!(create_dir_all, "mkdir", ());
    operation!(remove_file, "remove", ());
    operation!(remove_dir_all, "remove_dir", ());

    pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
        super::before("write", path.as_ref())?;
        std::fs::write(path, bytes)
    }
    pub fn exists(path: &Path) -> bool {
        metadata(path).is_ok()
    }
    pub fn is_file(path: &Path) -> bool {
        metadata(path).is_ok_and(|m| m.is_file())
    }
}

#[cfg(test)]
mod tests;
