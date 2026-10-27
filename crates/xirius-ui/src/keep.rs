//! Keeping the song while it is edited (PRJ-001, PRJ-009, PRJ-010).
//!
//! Every edit is already written to the song's folder, one step at a time
//! (`Session::push_step`), so what the top bar's mark says is whether that
//! record exists and is up to date:
//!
//! - **保存中** for [`FLASH_MS`] after an edit, an explicit save or an automatic save point, and for as long as
//!   the session's thread is still writing them (PRJ-022);
//! - **保存済み** when the song has a folder and nothing is failing;
//! - **未保存** when the song has been edited and has no folder to be recorded in.
//! - **保存できません** after a failed write, until a save succeeds (UI-288).
//!
//! A song nobody has saved yet gets a *draft* folder of the app's own at its first edit
//! (see `Session::keep_as_draft`), so a crash does not lose it. The folders and the list of
//! recent songs live under [`root`]; with no root (a test) nothing is written. The list is the
//! one `song.recent` reads ([`RecentList`]), with what the window closed on, which the next
//! start opens (UI-261).
//!
//! The automatic save point (`Session::autosave_if_due`, every 30 s) is made from the
//! idle poll, not while a take is being recorded or an export is written: both keep the
//! processor busy and the take is not in the song yet.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use xirius_core::model::Project;
pub use xirius_session::recent::{LastOpen, RECENT_MAX, RecentList, RecentSong, root};

use crate::song::Song;
use crate::studio::Studio;

/// How long 保存中 stays after an edit, long enough to be seen (mock § 06).
pub const FLASH_MS: u64 = 450;
type RecentReplies =
    std::cell::RefCell<Vec<std::sync::mpsc::Receiver<crate::recent_worker::Completion>>>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveMark {
    Saved,
    Unsaved,
    Saving,
    Failed,
}

#[derive(Debug, Default)]
pub struct Keep {
    /// Until a startup song is adopted, closing must retain the previous target.
    preserve_startup_open: bool,
    owner: Option<Arc<crate::recent_worker::Owner>>,
    refresh: RecentReplies,
    applied_revision: u64,
    save_generation: u64,
    recent_close_save_generation: u64,
    closing_recent: Option<std::time::Instant>,
    recent_close_ready: bool,
    recent_close_project: Option<Arc<Project>>,
    recent_close_open: Option<LastOpen>,
    recent_error: std::cell::RefCell<Option<String>>,
    root: Option<PathBuf>,
    /// The song as the last look saw it, to tell an edit.
    seen: Option<Arc<Project>>,
    pub(crate) flash_until: u64,
    /// What went wrong the last time the record was written.
    pub failed: Option<String>,
    /// A disk checkpoint cannot stand in for an unavailable plugin snapshot.
    pub(crate) plugin_state_failed: bool,
    /// Automatic save points made, for the tests.
    pub autosaves: u32,
    auto_pending: bool,
    auto_failed: bool,
    serial: u32,
    /// The recent songs as the last [`Keep::sync`] read them: what the entrance lists, not
    /// read again each frame (UI-261).
    listed: Vec<RecentSong>,
}

impl Keep {
    /// Keeps under `root`.
    pub fn at(root: PathBuf) -> Keep {
        Keep {
            owner: Some(crate::recent_worker::Owner::at(&root)),
            root: Some(root),
            ..Keep::default()
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn mark(&self, song: &Song, now: u64) -> SaveMark {
        if self.failed.is_some() || self.plugin_state_failed {
            return SaveMark::Failed;
        }
        if song.record_dir().is_some() && self.failed.is_none() {
            if now < self.flash_until || song.writes_pending() {
                SaveMark::Saving
            } else {
                SaveMark::Saved
            }
        } else if song.steps() == 0 && self.failed.is_none() {
            // Nothing has been done yet: nothing to lose.
            SaveMark::Saved
        } else {
            SaveMark::Unsaved
        }
    }

    /// The song was opened or made new: what it is now is not an edit.
    pub fn sync(&mut self, project: &Arc<Project>) {
        self.plugin_state_failed = false;
        self.preserve_startup_open = false;
        self.seen = Some(project.clone());
        self.flash_until = 0;
        self.failed = None;
        self.queue_recent(crate::recent_worker::Update::Refresh);
    }

    pub(crate) fn sync_open(&mut self, project: &Arc<Project>, dir: Option<PathBuf>, draft: bool) {
        self.plugin_state_failed = false;
        self.preserve_startup_open = false;
        self.seen = Some(project.clone());
        self.flash_until = 0;
        self.failed = None;
        if let Some(dir) = dir {
            self.remember(&dir, &project.name, draft, xirius_core::store::now_ms());
        } else {
            self.queue_recent(crate::recent_worker::Update::Refresh);
        }
    }

    pub(crate) fn poll_recent(&mut self) -> bool {
        let mut changed = false;
        #[cfg(test)]
        let root = self.root.clone();
        self.refresh.get_mut().retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok(completion) => {
                        if completion.revision < self.applied_revision {
                            continue;
                        }
                        self.applied_revision = completion.revision;
                        changed = true;
                        match completion.result {
                            Ok(songs) => {
                                self.listed = songs;
                                self.recent_error.take();
                            }
                            Err(error) => {
                                self.recent_error.replace(Some(error));
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        #[cfg(test)]
                        if let Some(root) = &root {
                            let _ = xirius_core::io_test::before(
                                "recent_poll_empty",
                                &root.join("recent.json"),
                            );
                        }
                        return true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
                }
            }
        });
        changed
    }

    fn queue_recent(&self, update: crate::recent_worker::Update) {
        if let Some(owner) = &self.owner {
            self.refresh.borrow_mut().push(owner.submit(update));
        }
    }

    #[cfg(test)]
    pub(crate) fn settle_recent(&self) {
        if let Some(owner) = &self.owner {
            owner.settle();
        }
    }

    #[cfg(test)]
    pub(crate) fn settled_recent(&self) -> Vec<RecentSong> {
        self.settle_recent();
        self.recent()
    }

    #[cfg(test)]
    pub(crate) fn settled_last_open(&self) -> Option<LastOpen> {
        self.settle_recent();
        self.last_open()
    }

    pub(crate) fn save_started(&mut self) -> bool {
        self.save_generation += 1;
        let closing = self.closing_recent.take().is_some();
        self.recent_close_ready = false;
        self.recent_close_project = None;
        self.recent_close_open = None;
        closing
    }

    pub(crate) fn recent_close_since(&self) -> Option<std::time::Instant> {
        self.closing_recent
    }

    /// The recent songs as the last [`Keep::sync`] read them.
    pub fn listed(&self) -> &[RecentSong] {
        &self.listed
    }

    /// Notes that the project is not the one last seen. True once per edit.
    fn changed(&mut self, project: &Arc<Project>) -> bool {
        match &self.seen {
            Some(s) if Arc::ptr_eq(s, project) => false,
            Some(_) => {
                self.seen = Some(project.clone());
                true
            }
            None => {
                self.seen = Some(project.clone());
                false
            }
        }
    }

    fn drafts(&self) -> Option<PathBuf> {
        self.root.as_ref().map(|r| r.join("drafts"))
    }

    /// Is `dir` one of the app's own draft folders?
    pub fn is_draft_dir(&self, dir: &Path) -> bool {
        self.drafts().is_some_and(|d| dir.starts_with(d))
    }

    fn new_draft_dir(&mut self, now: u64) -> Option<PathBuf> {
        self.serial += 1;
        let dir = self
            .drafts()?
            .join(format!("{now}-{}-{}", std::process::id(), self.serial));
        Some(dir)
    }

    // ------------------------------------------------------------- recents

    fn list(&self) -> Option<RecentList> {
        self.root.as_deref().map(RecentList::in_dir)
    }

    /// The songs remembered, newest first. A folder that is gone is left out.
    pub fn recent(&self) -> Vec<RecentSong> {
        self.list().map(|l| l.songs()).unwrap_or_default()
    }

    /// Puts the song in `dir` first in the list.
    pub fn remember(&self, dir: &Path, name: &str, draft: bool, now: u64) {
        self.queue_recent(crate::recent_worker::Update::Remember(
            dir.to_path_buf(),
            name.to_string(),
            draft,
            now,
        ));
    }

    /// Takes the song in `dir` out of the list.
    pub fn forget(&self, dir: &Path) {
        self.queue_recent(crate::recent_worker::Update::Forget(dir.to_path_buf()));
    }

    /// The window closed on `open`: the next start opens it.
    pub fn closed_with(&self, open: &LastOpen) {
        if !self.preserve_startup_open {
            self.queue_recent(crate::recent_worker::Update::Closed(open.clone()));
        }
    }

    /// What the start opens: with no root (a test), the song the studio was made with.
    pub fn last_open(&self) -> Option<LastOpen> {
        self.list().map(|l| l.last_open())
    }

    pub(crate) fn preserves_startup_open(&self) -> bool {
        self.preserve_startup_open
    }

    /// The newest remembered song whose folder was left locked by a process that is
    /// gone: the one a crash interrupted.
    pub fn crashed(&self) -> Option<RecentSong> {
        self.recent()
            .into_iter()
            .find(|s| xirius_core::recovery::crashed(&s.dir))
    }

    /// Deletes a draft folder the song has left (saved elsewhere). Refuses any folder
    /// that is not one of the app's own drafts.
    pub fn delete_draft(&self, dir: &Path) {
        if self.is_draft_dir(dir) {
            // design-lint: allow(ui-blocking-io) PRJ-031: non-window adapter only; window path uses error/cache/worker without disk access.
            let _ = std::fs::remove_dir_all(dir);
            self.forget(dir);
        }
    }
}

impl Studio {
    /// Auxiliary metadata gets at most two seconds; failure never vetoes a close.
    pub(crate) fn request_recent_close(&mut self) -> bool {
        if self.saving.is_some() {
            return false;
        }
        let same = self.keep.recent_close_save_generation == self.keep.save_generation
            && self.keep.recent_close_open.as_ref() == Some(&self.open_now())
            && self
                .keep
                .recent_close_project
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(p, &self.project()));
        if self.keep.owner.is_none() || (self.keep.recent_close_ready && same) {
            return true;
        }
        if !same {
            self.keep.closing_recent = None;
            self.keep.recent_close_ready = false;
        }
        if self.keep.closing_recent.is_none() {
            self.keep.recent_close_save_generation = self.keep.save_generation;
            self.keep.recent_close_project = Some(self.project());
            self.keep.recent_close_open = Some(self.open_now());
            self.window_ask = None;
            if self.open_closing.is_none() {
                self.keep_leaving();
                self.keep.closed_with(&self.open_now());
            }
            self.keep
                .queue_recent(crate::recent_worker::Update::Refresh);
            self.keep.closing_recent = Some(std::time::Instant::now());
        }
        false
    }

    pub(crate) fn poll_recent_close(&mut self) -> bool {
        if self.saving.is_some() || self.keep.recent_close_ready {
            return false;
        }
        let Some(since) = self.keep.closing_recent else {
            return false;
        };
        if self.keep.recent_close_save_generation != self.keep.save_generation {
            return false;
        }
        let result = match self.keep.owner.as_ref().and_then(|o| o.status()) {
            Some(result) => result,
            None if since.elapsed() >= crate::recent_worker::CLOSE_WAIT => {
                Err("recent list update unfinished at shutdown deadline".into())
            }
            None => return false,
        };
        if let Err(error) = result {
            crate::ancillary_io::log!("recent list close: {error}");
        }
        // Keep the original deadline for last-window fallback; do not start another wait.
        self.keep.recent_close_ready = true;
        self.window_ask = Some(crate::context::key_more::WindowAsk::Quit);
        true
    }

    pub(crate) fn poll_recent_keep(&mut self) -> bool {
        let changed = self.keep.poll_recent();
        let error = self.keep.recent_error.take();
        if let Some(error) = error {
            if self.keep.closing_recent.is_some() {
                crate::ancillary_io::log!("recent list close: {error}");
                return true;
            }
            self.warn(
                format!("最近の曲と復旧の一覧を記録できません（{error}）"),
                None,
            );
            return true;
        }
        changed
    }
    /// The mark the top bar shows.
    pub fn save_mark(&self) -> SaveMark {
        if self.input_blocked() {
            return SaveMark::Saving;
        }
        self.keep.mark(&self.song, xirius_core::store::now_ms())
    }

    /// Starts keeping under `keep`, and opens the song a crash interrupted when there is
    /// one (PRJ-012); otherwise what the last window closed on (UI-261): an empty song the
    /// first time, then the song, the sample, or an empty song. Called by the app only; a test
    /// names its own folder.
    pub fn start_keeping(&mut self, keep: Keep) {
        self.keep = keep;
        self.keep.sync_open(&self.song.project(), None, false);
        // Startup discovery, including existence and crash-marker checks, is worker I/O.
        let root = self.keep.root().map(Path::to_path_buf);
        if let Some(root) = root {
            self.keep.preserve_startup_open = true;
            self.request_open(crate::opening::Request::Startup(root));
        }
    }

    /// The sample song in place of this one (the entrance's list).
    pub fn open_sample(&mut self) {
        self.request_open(crate::opening::Request::Sample);
    }

    #[cfg(test)]
    pub fn open_sample_sync(&mut self) {
        self.keep_leaving();
        let closing = self.closing();
        self.song = Song::sample_after(&self.song);
        self.error = None;
        self.song_replaced(closing);
    }

    /// What the window has, for the next start.
    fn open_now(&self) -> LastOpen {
        match self.song.record_dir() {
            Some(dir) => LastOpen::Song(dir),
            None if self.song.is_sample() && self.song.steps() == 0 => LastOpen::Sample,
            None => LastOpen::Empty,
        }
    }

    /// The song was edited: a never-saved song gets its draft folder, and the mark flashes.
    pub(crate) fn keep_edit(&mut self, now: u64) {
        if self
            .keep
            .seen
            .as_ref()
            .is_none_or(|seen| !Arc::ptr_eq(seen, &self.song.project()))
        {
            if self.keep.closing_recent.is_some() || self.keep.recent_close_ready {
                self.leaving_ready = false;
            }
            self.keep.recent_close_ready = false;
        }
        if self.song.steps() > 0 {
            self.keep.preserve_startup_open = false;
        }
        if !self.transport.is_windowed()
            && self.song.record_dir().is_none()
            && self.song.steps() > 0
            && let Some(dir) = self.keep.new_draft_dir(now)
        {
            match self.song.keep_draft_checked(&dir) {
                Ok(()) => {
                    self.keep.failed = None;
                    self.keep.remember(&dir, &self.project().name, true, now);
                }
                Err(e) => self.save_failed(&e),
            }
        }
        if self.keep.changed(&self.song.project()) && self.song.record_dir().is_some() {
            self.keep.flash_until = now + FLASH_MS;
        }
    }

    /// What the idle poll does for the record: notices an edit, makes the automatic
    /// save point when it is due, and lets the flash end. True when the mark changed.
    pub fn keep_beat(&mut self, now: u64) -> bool {
        if self.input_blocked() {
            return false;
        }
        let before = self.keep.mark(&self.song, now);
        self.keep_edit(now);
        // after_edit can be called several times in one input callback. Only
        // hand off its Session at the next idle boundary, after a drag ended.
        if self.transport.is_windowed()
            && self.song.record_dir().is_none()
            && self.song.steps() > 0
            && !self.pointer_held
            && self.arrange.drag.is_none()
            && self.roll.drag.is_none()
            && let Some(dir) = self.keep.new_draft_dir(now)
        {
            self.start_draft_record(dir);
            return true;
        }
        let busy = self.transport.recording || self.export.running();
        if !busy && self.song.record_dir().is_some() {
            match self.song.autosave_checked(now) {
                Ok(true) => {
                    self.keep.autosaves += 1;
                    self.keep.auto_pending = true;
                    self.keep.auto_failed = false;
                    self.keep.flash_until = now + FLASH_MS;
                }
                Ok(false) => {}
                Err(e) => self.save_failed(&e),
            }
        }
        // A failed step or save point keeps the warning. Handing another point to the writer
        // cannot clear it; that batch must finish without a failed write.
        let errors = self.song.session().take_write_failures();
        if !errors.is_empty() {
            self.keep.auto_failed = true;
        }
        for error in &errors {
            self.save_failed(error);
        }
        if self.keep.auto_pending && !self.song.writes_pending() {
            self.keep.auto_pending = false;
            if !self.keep.auto_failed {
                self.save_succeeded();
            }
        }
        self.keep.mark(&self.song, now) != before
    }

    /// The song is about to be replaced (another opened, a new one made, the window
    /// closed): the list remembers it, a draft by its folder.
    pub(crate) fn keep_leaving(&mut self) {
        // What the plugins hold now goes into the song before it is let go of (PLG-057).
        if !self.leaving_ready {
            self.pull_plugins();
        }
        if let Some(dir) = self.song.record_dir() {
            let name = self.project().name.clone();
            self.keep.remember(
                &dir,
                &name,
                self.song.is_draft(),
                xirius_core::store::now_ms(),
            );
        }
    }

    /// The window closes: the song's folder is let go of, so the next start does not take
    /// this close for a crash.
    pub fn release_song(&mut self) {
        if !self.keep.recent_close_ready {
            self.keep_leaving();
            self.keep.closed_with(&self.open_now());
        }
        self.song.release();
    }
}

impl Studio {
    pub(crate) fn delete_old_draft(&mut self, dir: &Path) {
        if !self.transport.is_windowed() {
            self.keep.delete_draft(dir);
            return;
        }
        if self.keep.is_draft_dir(dir) {
            let path = dir.to_owned();
            if let Err(e) = crate::ancillary_io::dispatch(move || {
                #[cfg(any(test, feature = "loading-probe"))]
                xirius_core::io_test::before("draft_remove", &path).map_err(|e| e.to_string())?;
                std::fs::remove_dir_all(path).map_err(|e| e.to_string())
            }) {
                self.error = Some(e);
            }
            self.keep.forget(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-ui-keep-{tag}-"))
            .tempdir()
            .unwrap()
    }

    /// A studio that keeps its songs in a folder of the test's own; the folder goes when the
    /// guard is dropped.
    fn kept(tag: &str) -> (Studio, PathBuf, tempfile::TempDir) {
        let root_tmp = scratch(tag);
        let root = root_tmp.path().to_path_buf();
        let mut s = Studio::new(Song::empty());
        s.keep = Keep::at(root.clone());
        s.keep.sync(&s.song.project());
        (s, root, root_tmp)
    }

    fn add(s: &mut Studio, name: &str) {
        assert!(s.run_command("track.add.audio", json!({ "name": name })));
    }

    struct Gate(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);
    impl Gate {
        fn new() -> Self {
            Self(Arc::new((
                std::sync::Mutex::new(false),
                std::sync::Condvar::new(),
            )))
        }
        fn release(&self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }
    }
    impl Drop for Gate {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[test]
    fn draft_save_as_and_close_callbacks_return_while_recent_write_is_stopped() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        use std::time::{Duration, Instant};
        let (mut s, root, _tmp) = kept("callbacks");
        s.keep.settle_recent();
        let gate = Gate::new();
        let held = gate.0.clone();
        let watch = root.join("recent.json");
        let ui = std::thread::current().id();
        let once = AtomicBool::new(false);
        let (tx, entered) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "write" && path == watch {
                assert_ne!(std::thread::current().id(), ui);
                if !once.swap(true, Ordering::SeqCst) {
                    tx.send(()).unwrap();
                    let lock = held.0.lock().unwrap();
                    drop(held.1.wait_while(lock, |released| !*released).unwrap());
                }
            }
            Ok(())
        }));
        add(&mut s, "draft");
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
        let draft = s.song.record_dir().unwrap();
        let destination = root.join("saved");
        s.start_file_request(crate::studio::FileRequest::SaveAs, &destination);
        s.wait_for_save();
        assert_eq!(s.song.dir(), Some(destination.clone()));
        assert!(!s.request_close());
        assert!(!s.poll_recent_close());
        add(&mut s, "edit while closing");
        assert!(!s.leaving_ready, "new edits need their own writer barrier");
        // Advance the decision's clock, without making test success depend on speed.
        s.keep.closing_recent = Some(Instant::now() - Duration::from_secs(3));
        s.window_ask = None;
        assert!(s.poll_recent_close());
        assert!(s.error.is_none());
        assert_eq!(
            s.window_ask,
            Some(crate::context::key_more::WindowAsk::Quit)
        );
        gate.release();
        s.keep.settle_recent();
        s.song.flush_writes();
        assert!(!s.request_close());
        s.keep.settle_recent();
        assert!(s.poll_recent_close());
        assert!(s.request_close());
        s.release_song();
        let list = RecentList::in_dir(&root);
        assert_eq!(list.last_open(), LastOpen::Song(destination.clone()));
        assert_eq!(list.songs().len(), 1);
        assert_eq!(list.songs()[0].dir, destination);
        assert!(!list.songs()[0].draft);
        assert!(!draft.exists());
        drop(observation);
    }

    #[test]
    fn recent_failure_never_vetoes_the_first_close() {
        for kind in [
            std::io::ErrorKind::ReadOnlyFilesystem,
            std::io::ErrorKind::StorageFull,
            std::io::ErrorKind::Other,
        ] {
            let (mut s, root, _tmp) = kept("close-error");
            add(&mut s, "draft");
            s.keep.settle_recent();
            s.keep.poll_recent();
            let watch = root.join("recent.json");
            let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if op == "write" && path == watch {
                    return Err(std::io::Error::from(kind));
                }
                Ok(())
            }));
            assert!(!s.request_close());
            s.keep.settle_recent();
            assert!(s.poll_recent_close());
            s.poll_recent_keep();
            assert!(s.error.is_none());
            assert!(s.toast.is_none());
            assert_eq!(
                s.window_ask.take(),
                Some(crate::context::key_more::WindowAsk::Quit)
            );
            assert!(
                s.request_close(),
                "the single close request finishes without lifting the failure"
            );
            s.release_song();
            drop(observation);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_readonly_recent_file_does_not_prevent_the_first_close() {
        let (mut s, root, _tmp) = kept("readonly-close");
        add(&mut s, "saved");
        s.keep.settle_recent();
        s.keep.poll_recent();
        let path = root.join("recent.json");
        struct Restore(PathBuf, std::fs::Permissions);
        impl Drop for Restore {
            fn drop(&mut self) {
                std::fs::set_permissions(&self.0, self.1.clone()).unwrap();
            }
        }
        let restore = Restore(
            path.clone(),
            std::fs::metadata(&path).unwrap().permissions(),
        );
        let mut readonly = restore.1.clone();
        readonly.set_readonly(true);
        std::fs::set_permissions(&path, readonly).unwrap();
        assert!(!s.request_close());
        s.keep.settle_recent();
        assert!(s.keep.owner.as_ref().unwrap().status().unwrap().is_err());
        assert!(s.poll_recent_close());
        s.poll_recent_keep();
        assert!(s.error.is_none() && s.toast.is_none());
        assert_eq!(
            s.window_ask.take(),
            Some(crate::context::key_more::WindowAsk::Quit)
        );
        assert!(s.request_close());
        s.release_song();
        drop(restore);
    }

    #[test]
    fn recent_lock_child() {
        use std::io::{BufRead, Write};
        let Some(root) = std::env::var_os("XIRIUS_RECENT_CLOSE_LOCK_TEST") else {
            return;
        };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(root).join("recent.lock"))
            .unwrap();
        file.lock().unwrap();
        println!("RECENT_LOCK_HELD");
        std::io::stdout().flush().unwrap();
        let mut release = String::new();
        std::io::stdin().lock().read_line(&mut release).unwrap();
        assert_eq!(release.trim(), "release");
    }

    #[test]
    fn a_lock_held_by_another_process_cannot_veto_close_or_restart_the_deadline() {
        use std::io::{BufRead, BufReader, Write};
        use std::process::{Child, Command, Stdio};
        use std::sync::mpsc;
        use std::time::{Duration, Instant};
        let (mut s, root, _tmp) = kept("process-lock-close");
        s.keep.settle_recent();
        s.keep.poll_recent();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "keep::tests::recent_lock_child", "--nocapture"])
            .env("XIRIUS_RECENT_CLOSE_LOCK_TEST", &root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        struct Locker(Child);
        impl Drop for Locker {
            fn drop(&mut self) {
                if self.0.try_wait().ok().flatten().is_none() {
                    let _ = self.0.kill();
                    let _ = self.0.wait();
                }
            }
        }
        let mut locker = Locker(command.spawn().unwrap());
        let output = locker.0.stdout.take().unwrap();
        let (held_tx, held) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if line.unwrap().contains("RECENT_LOCK_HELD") {
                    let _ = held_tx.send(());
                }
            }
        });
        held.recv_timeout(Duration::from_secs(15)).unwrap();
        let watch = root.join("recent.lock");
        let (tx, entered) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "recent_lock_wait" && path == watch {
                let _ = tx.send(());
            }
            Ok(())
        }));
        add(&mut s, "draft");
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
        assert!(!s.request_close());
        assert!(!s.poll_recent_close());
        let since = Instant::now() - crate::recent_worker::CLOSE_WAIT;
        s.keep.closing_recent = Some(since);
        assert!(s.poll_recent_close());
        assert!(s.error.is_none() && s.toast.is_none());
        assert_eq!(
            s.window_ask.take(),
            Some(crate::context::key_more::WindowAsk::Quit)
        );
        assert!(s.request_close());
        assert_eq!(s.keep.recent_close_since(), Some(since));
        assert!(
            crate::recent_worker::finish(since).is_some(),
            "last-window fallback shares the expired deadline"
        );
        assert!(
            locker.0.try_wait().unwrap().is_none(),
            "the other process still holds the lock"
        );
        s.release_song();
        writeln!(locker.0.stdin.as_mut().unwrap(), "release").unwrap();
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = locker.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
        s.keep.settle_recent();
        drop(observation);
    }

    #[test]
    fn os_lock_wait_and_multiple_handles_share_one_fifo_without_blocking_the_ui() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (mut s, root, _tmp) = kept("os-lock");
        s.keep.settle_recent();
        s.keep.poll_recent();
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("recent.lock"))
            .unwrap();
        lock.lock().unwrap();
        let watch = root.join("recent.lock");
        let ui = std::thread::current().id();
        let (tx, entered) = mpsc::channel();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "recent_lock_wait" && path == watch {
                assert_ne!(std::thread::current().id(), ui);
                tx.send(()).unwrap();
            }
            Ok(())
        }));
        add(&mut s, "old");
        entered.recv_timeout(Duration::from_secs(15)).unwrap();
        let dir = s.song.record_dir().unwrap();
        let second = Keep::at(root.clone());
        assert!(Arc::ptr_eq(
            s.keep.owner.as_ref().unwrap(),
            second.owner.as_ref().unwrap()
        ));
        second.forget(&dir);
        s.keep.closed_with(&LastOpen::Sample);
        assert!(!s.poll_recent_keep());
        drop(lock);
        s.keep.settle_recent();
        assert!(RecentList::in_dir(&root).songs().is_empty());
        assert_eq!(RecentList::in_dir(&root).last_open(), LastOpen::Sample);
        drop(observation);
    }

    #[test]
    fn save_as_during_recent_close_waits_for_the_actual_save_job() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        use std::time::Duration;
        for fail in [false, true] {
            let (mut s, root, _tmp) = kept("close-save-as");
            add(&mut s, "saved");
            let original = root.join("original");
            s.finish_file_request_sync(crate::studio::FileRequest::SaveAs, &original);
            s.keep.settle_recent();
            s.keep.poll_recent();
            let project = s.project();
            let recent_gate = Gate::new();
            let save_gate = Gate::new();
            let recent_held = recent_gate.0.clone();
            let save_held = save_gate.0.clone();
            let watch_recent = root.join("recent.json");
            let destination = root.join("copy");
            let watch_save = destination.clone();
            let once_recent = AtomicBool::new(false);
            let once_save = AtomicBool::new(false);
            let (recent_tx, recent_entered) = mpsc::channel();
            let (save_tx, save_entered) = mpsc::channel();
            let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if op == "write"
                    && path == watch_recent
                    && !once_recent.swap(true, Ordering::SeqCst)
                {
                    recent_tx.send(()).unwrap();
                    let lock = recent_held.0.lock().unwrap();
                    drop(
                        recent_held
                            .1
                            .wait_while(lock, |released| !*released)
                            .unwrap(),
                    );
                }
                if op == "write"
                    && path.starts_with(&watch_save)
                    && !once_save.swap(true, Ordering::SeqCst)
                {
                    save_tx.send(()).unwrap();
                    let lock = save_held.0.lock().unwrap();
                    drop(save_held.1.wait_while(lock, |released| !*released).unwrap());
                    if fail {
                        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
                    }
                }
                Ok(())
            }));
            assert!(!s.request_close());
            recent_entered
                .recv_timeout(Duration::from_secs(15))
                .unwrap();
            s.start_file_request(crate::studio::FileRequest::SaveAs, &destination);
            save_entered.recv_timeout(Duration::from_secs(15)).unwrap();
            assert!(
                Arc::ptr_eq(&project, &s.project()),
                "the placeholder retains the old Arc"
            );
            assert_eq!(s.song.dir(), Some(original.clone()));
            recent_gate.release();
            s.keep.settle_recent();
            assert!(!s.poll_recent_close());
            assert!(
                !s.request_close(),
                "a completed recent barrier cannot bypass Save As"
            );
            assert!(s.saving.is_some());
            assert!(s.window_ask.is_none());
            save_gate.release();
            s.wait_for_save();
            if fail {
                assert_eq!(s.song.dir(), Some(original));
                assert!(s.toast.as_ref().is_some_and(|t| t.alert));
                assert!(
                    s.window_ask.is_none(),
                    "save failure keeps its existing confirmation flow"
                );
                assert!(!s.keep.recent_close_ready);
            } else {
                assert_eq!(s.song.dir(), Some(destination.clone()));
                assert_eq!(
                    s.window_ask.take(),
                    Some(crate::context::key_more::WindowAsk::Quit)
                );
                assert!(!s.request_close());
                s.keep.settle_recent();
                assert!(s.poll_recent_close());
                assert!(s.request_close());
                s.release_song();
                assert_eq!(
                    RecentList::in_dir(&root).last_open(),
                    LastOpen::Song(destination)
                );
            }
            drop(observation);
        }
    }

    #[test]
    fn a_completion_between_empty_and_the_next_receiver_never_rolls_back_the_cache() {
        use crate::recent_worker::Completion;
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        for old_failure in [false, true] {
            let tmp = scratch("completion-order");
            let mut keep = Keep::at(tmp.path().to_path_buf());
            let (a_tx, a_rx) = mpsc::channel();
            let (b_tx, b_rx) = mpsc::channel();
            keep.refresh.get_mut().extend([a_rx, b_rx]);
            let newer = RecentSong {
                dir: tmp.path().join("new"),
                name: "new".into(),
                draft: false,
                at_ms: 2,
            };
            let expected = newer.clone();
            let watch = tmp.path().join("recent.json");
            let once = AtomicBool::new(false);
            let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if op == "recent_poll_empty" && path == watch && !once.swap(true, Ordering::SeqCst)
                {
                    a_tx.send(Completion {
                        revision: 1,
                        result: if old_failure {
                            Err("old failure".into())
                        } else {
                            Ok(Vec::new())
                        },
                    })
                    .unwrap();
                    b_tx.send(Completion {
                        revision: 2,
                        result: Ok(vec![newer.clone()]),
                    })
                    .unwrap();
                }
                Ok(())
            }));
            assert!(keep.poll_recent());
            assert_eq!(keep.listed(), std::slice::from_ref(&expected));
            assert!(!keep.poll_recent());
            assert_eq!(keep.listed(), &[expected]);
            assert!(keep.recent_error.take().is_none());
            drop(observation);
        }
    }

    #[test]
    fn worker_panics_retain_the_fifo_and_allow_poll_submit_and_close() {
        use std::sync::atomic::{AtomicBool, Ordering};
        for operation in ["write", "recent_readback"] {
            let (mut s, root, _tmp) = kept("worker-panic");
            s.keep.settle_recent();
            s.keep.poll_recent();
            let watch = root.join("recent.json");
            let once = AtomicBool::new(false);
            let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if op == operation && path == watch && !once.swap(true, Ordering::SeqCst) {
                    panic!("injected recent worker panic");
                }
                Ok(())
            }));
            add(&mut s, "recoverable");
            s.keep.settle_recent();
            assert!(s.keep.owner.as_ref().unwrap().pending() > 0);
            assert!(s.poll_recent_keep());
            assert!(s.toast.as_ref().unwrap().text.contains("panicked"));
            let draft = s.song.record_dir().unwrap();
            s.keep.queue_recent(crate::recent_worker::Update::Refresh);
            s.keep.settle_recent();
            assert!(s.keep.recent().iter().any(|r| r.dir == draft && r.draft));
            s.poll_recent_keep();
            assert_eq!(s.keep.owner.as_ref().unwrap().pending(), 0);
            assert!(!s.request_close());
            s.keep.settle_recent();
            assert!(s.poll_recent_close());
            assert!(s.request_close());
            s.release_song();
            drop(observation);
        }
    }

    #[test]
    fn a_failed_draft_registration_is_reported_without_losing_the_draft_files() {
        let (mut s, root, _tmp) = kept("recent-failure");
        let recent = root.join("recent.json");
        let watch = recent.clone();
        let observation = xirius_core::io_test::observe(Arc::new(move |op, path| {
            if op == "write" && path == watch {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "test recent failure",
                ));
            }
            Ok(())
        }));
        add(&mut s, "recoverable");
        let draft = s.song.record_dir().unwrap();
        assert!(draft.join("project.json").exists());
        assert!(!recent.exists());
        s.keep.settle_recent();
        assert!(s.poll_recent_keep());
        let toast = s.toast.as_ref().unwrap();
        assert!(toast.alert);
        assert!(toast.text.contains("復旧の一覧を記録できません"));
        assert!(toast.text.contains("test recent failure"));
        assert!(!s.poll_recent_keep());
        drop(observation);
        // A later successful registration remains possible; never delete the
        // recoverable song to compensate for a recent-list failure.
        s.keep.remember(&draft, "recoverable", true, 1);
        assert!(
            s.keep
                .settled_recent()
                .iter()
                .any(|entry| entry.dir == draft && entry.draft)
        );
    }

    #[test]
    fn a_song_with_nothing_done_has_nothing_to_lose() {
        let (s, _, _tmp) = kept("nothing");
        assert_eq!(s.keep.mark(&s.song, 1000), SaveMark::Saved);
    }

    #[test]
    fn an_edit_flashes_saving_then_saved_with_no_save_made() {
        let (mut s, root, _tmp) = kept("flash");
        let before = xirius_core::store::now_ms();
        add(&mut s, "a");
        let after = xirius_core::store::now_ms();
        assert!(s.song.is_draft(), "the first edit made a draft folder");
        assert!(!s.song.is_saved() && s.song.dir().is_none());
        assert!(
            s.song
                .record_dir()
                .unwrap()
                .starts_with(root.join("drafts"))
        );
        // The flash runs from the edit itself: writing the draft may take a while on a busy
        // machine, so the clock is read around the edit rather than after it.
        let until = s.keep.flash_until;
        assert!(
            (before + FLASH_MS..=after + FLASH_MS).contains(&until),
            "{before} {until} {after}"
        );
        assert_eq!(s.keep.mark(&s.song, until - 1), SaveMark::Saving);
        assert_eq!(s.keep.mark(&s.song, until), SaveMark::Saved);
    }

    #[test]
    fn a_song_edited_with_nowhere_to_keep_it_is_unsaved() {
        let mut s = Studio::new(Song::empty());
        add(&mut s, "a");
        assert_eq!(s.keep.mark(&s.song, 1_000_000_000_000), SaveMark::Unsaved);
    }

    #[test]
    fn the_draft_is_listed_and_goes_when_the_song_is_saved_in_a_place_of_its_own() {
        let (mut s, root, _tmp) = kept("draft");
        add(&mut s, "a");
        let draft = s.song.record_dir().unwrap();
        assert_eq!(s.keep.settled_recent().len(), 1);
        assert!(s.keep.settled_recent()[0].draft);
        let place = root.join("my-song");
        s.finish_file_request_sync(crate::studio::FileRequest::SaveAs, &place);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(!s.song.is_draft());
        assert_eq!(s.song.dir().as_deref(), Some(place.as_path()));
        assert!(!draft.exists(), "the draft folder is deleted");
        let recent = s.keep.settled_recent();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].dir, place);
        assert!(!recent[0].draft);
    }

    #[test]
    fn delete_draft_refuses_a_folder_that_is_not_a_draft() {
        let (s, root, _tmp) = kept("refuse");
        let other = root.join("someone-elses-song");
        std::fs::create_dir_all(&other).unwrap();
        s.keep.delete_draft(&other);
        assert!(other.exists());
    }

    #[test]
    fn the_recent_list_is_newest_first_and_leaves_out_folders_that_are_gone() {
        let (s, root, _tmp) = kept("recent");
        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("project.json"), "{}").unwrap();
            s.keep.remember(&d, name, false, 100 + i as u64);
        }
        let names: Vec<String> = s
            .keep
            .settled_recent()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["c", "b", "a"]);
        // Remembering one again moves it to the front without a second row.
        s.keep.remember(&root.join("a"), "a", false, 200);
        let names: Vec<String> = s
            .keep
            .settled_recent()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["a", "c", "b"]);
        std::fs::remove_dir_all(root.join("c")).unwrap();
        let names: Vec<String> = s
            .keep
            .settled_recent()
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(names, ["a", "b"]);
    }

    fn autosaves_in(s: &Studio) -> usize {
        s.song
            .versions()
            .iter()
            .filter(|v| v.name == "自動保存")
            .count()
    }

    #[test]
    fn the_beat_makes_an_automatic_save_point_every_thirty_seconds_and_only_for_a_change() {
        let (mut s, _, _tmp) = kept("beat");
        add(&mut s, "a");
        let t0 = xirius_core::store::now_ms();
        assert!(!s.keep_beat(t0 + 1_000) || s.keep.autosaves == 0);
        assert_eq!(s.keep.autosaves, 0, "not yet due");
        // Due: one point, and the mark flashes 保存中 for it.
        s.keep_beat(t0 + 31_000);
        assert_eq!((s.keep.autosaves, autosaves_in(&s)), (1, 1));
        assert_eq!(s.keep.mark(&s.song, t0 + 31_000), SaveMark::Saving);
        // The point is written on the writer's thread; the mark says 保存中 until it is.
        s.song.flush_writes();
        assert_eq!(
            s.keep.mark(&s.song, t0 + 31_000 + FLASH_MS),
            SaveMark::Saved
        );
        // Nothing changed since: the next beats make none.
        s.keep_beat(t0 + 62_000);
        s.keep_beat(t0 + 93_000);
        assert_eq!(autosaves_in(&s), 1);
        // A change makes the next one.
        add(&mut s, "b");
        s.keep_beat(t0 + 124_000);
        assert_eq!(autosaves_in(&s), 2);
    }

    #[test]
    fn an_automatic_save_point_waits_while_a_take_is_recorded_and_comes_after() {
        let (mut s, _, _tmp) = kept("recording");
        add(&mut s, "a");
        let t0 = xirius_core::store::now_ms();
        s.transport.recording = true;
        s.keep_beat(t0 + 31_000);
        s.keep_beat(t0 + 62_000);
        assert_eq!(autosaves_in(&s), 0, "none during the take");
        s.transport.recording = false;
        s.keep_beat(t0 + 63_000);
        assert_eq!(autosaves_in(&s), 1, "the first beat after it");
    }

    #[test]
    fn an_automatic_save_point_waits_while_an_export_runs() {
        let (mut s, _, _tmp) = kept("export");
        add(&mut s, "a");
        let t0 = xirius_core::store::now_ms();
        s.export.jobs.push(json!({ "state": "running" }));
        assert!(s.export.running());
        s.keep_beat(t0 + 31_000);
        assert_eq!(autosaves_in(&s), 0);
        s.export.jobs.clear();
        s.keep_beat(t0 + 32_000);
        assert_eq!(autosaves_in(&s), 1);
    }

    #[test]
    fn a_failed_record_keeps_the_failure_mark_until_a_save_point_is_written() {
        let (mut s, _, _tmp) = kept("failed");
        add(&mut s, "a");
        let t0 = xirius_core::store::now_ms();
        s.keep.failed = Some("disk full".into());
        assert_eq!(s.keep.mark(&s.song, t0 + 10_000), SaveMark::Failed);
        s.keep_beat(t0 + 31_000);
        s.song.flush_writes();
        s.keep_beat(t0 + 31_100);
        assert!(s.keep.failed.is_none(), "only a point written clears it");
        assert_eq!(s.keep.mark(&s.song, t0 + 40_000), SaveMark::Saved);
    }

    #[test]
    fn edits_on_a_slow_disk_do_not_stop_the_window() {
        let (mut s, _, _tmp) = kept("slow");
        add(&mut s, "a");
        s.song.flush_writes();
        let slow = std::time::Duration::from_millis(300);
        s.song.simulate_slow_disk(slow);
        // The versions' pop-over reads the list after each edit; it does not wait either.
        s.open_popup(
            crate::popup::PopupKind::Versions,
            crate::studio::Rect::new(0.0, 0.0, 0.0, 0.0),
        );
        let started = std::time::Instant::now();
        for i in 0..4 {
            add(&mut s, &format!("t{i}"));
            s.keep_beat(xirius_core::store::now_ms());
        }
        let took = started.elapsed();
        // Written in the window's own thread, the four would take four times `slow`.
        assert!(took < slow, "{took:?}");
        let later = xirius_core::store::now_ms() + FLASH_MS + 1;
        assert!(s.song.writes_pending());
        assert_eq!(s.keep.mark(&s.song, later), SaveMark::Saving);
        s.song.flush_writes();
        s.keep_beat(later);
        assert_eq!(s.keep.mark(&s.song, later), SaveMark::Saved);
    }

    #[test]
    fn a_step_the_thread_could_not_write_shows_the_failure_mark_and_warning() {
        let (mut s, _, _tmp) = kept("thread-failed");
        add(&mut s, "a");
        s.song.flush_writes();
        let history = s.song.record_dir().unwrap().join("history");
        let next = std::fs::read_dir(&history)
            .unwrap()
            .filter_map(|e| {
                e.ok()?
                    .file_name()
                    .to_str()?
                    .strip_suffix(".json")?
                    .parse::<u64>()
                    .ok()
            })
            .max()
            .unwrap()
            + 1;
        // A folder in the place of the next step's file.
        std::fs::create_dir(history.join(format!("{next}.json"))).unwrap();
        add(&mut s, "b");
        s.song.flush_writes();
        let now = xirius_core::store::now_ms();
        s.keep_beat(now);
        let failed = s.keep.failed.clone().unwrap_or_default();
        assert!(failed.contains("保存できません"), "{failed}");
        assert!(s.toast.as_ref().is_some_and(|t| t.alert));
        assert_eq!(s.keep.mark(&s.song, now + FLASH_MS + 1), SaveMark::Failed);
    }

    #[test]
    fn the_automatic_save_point_costs_little_on_the_sample_song() {
        let root_tmp = scratch("cost");
        let root = root_tmp.path().to_path_buf();
        let mut s = Studio::sample();
        s.keep = Keep::at(root);
        s.keep.sync(&s.song.project());
        s.run_command("track.add.audio", json!({ "name": "edit" }));
        let t0 = xirius_core::store::now_ms();
        let started = std::time::Instant::now();
        s.keep_beat(t0 + 31_000);
        let took = started.elapsed();
        assert_eq!(s.keep.autosaves, 1);
        crate::ancillary_io::log!("autosave of the sample song: {took:?}");
        // The 250 ms beat must not stall the window: a point is far under one frame budget
        // of a second even in a debug build on a slow disk.
        assert!(took.as_millis() < 1000, "{took:?}");
    }

    #[test]
    fn a_new_song_goes_back_to_the_empty_entrance_and_keeps_the_one_it_left() {
        let (mut s, _, _tmp) = kept("new");
        add(&mut s, "a");
        let left = s.song.record_dir().unwrap();
        assert!(!s.start_shown());
        s.new_song_sync();
        assert!(s.start_shown(), "the entrance screen is back");
        assert_eq!(s.project().tracks.len(), 1, "only the master");
        assert!(
            s.song.record_dir().is_none(),
            "nothing is made until the first edit"
        );
        assert_eq!(s.save_mark(), SaveMark::Saved, "nothing to lose");
        // The song it left is in the list and opens again as it was.
        let listed = s.keep.settled_recent();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].dir, left);
        s.finish_file_request_sync(crate::studio::FileRequest::Open, &left);
        assert!(s.error.is_none(), "{:?}", s.error);
        assert!(!s.start_shown());
        assert!(s.project().tracks.values().any(|t| t.name == "a"));
    }

    #[test]
    fn the_new_entry_of_the_song_menu_and_ctrl_n_do_the_same() {
        let (mut s, _, _tmp) = kept("new-entry");
        add(&mut s, "a");
        let g = s.geometry(1440.0, 860.0);
        assert!(s.key(&g, "n", true, false));
        assert!(s.opening.is_some());
        s.wait_for_open_for_test();
        assert!(s.start_shown());
        add(&mut s, "b");
        s.open_popup(
            crate::popup::PopupKind::Song,
            crate::studio::Rect::new(40.0, -34.0, 160.0, 28.0),
        );
        let l = s.popup.as_ref().unwrap().layout(g.size);
        let new = l.cells.iter().find(|c| c.text == "新規").unwrap();
        assert!(s.popup_action(&g, new.action.clone().unwrap()));
        s.wait_for_open_for_test();
        assert!(s.popup.is_none() && s.start_shown());
    }

    /// A launch of the app on `root`: the sample it is made with, then the keeping.
    fn launch(root: &Path) -> Studio {
        let mut s = Studio::sample();
        let keep = Keep::at(root.to_path_buf());
        keep.settle_recent();
        s.start_keeping(keep);
        s.wait_for_open_for_test();
        s
    }

    #[test]
    fn the_first_start_is_an_empty_song_and_the_next_opens_what_the_last_closed_on() {
        let tmp = scratch("launch");
        let root = tmp.path().to_path_buf();
        // The first start: nothing written yet, an empty song.
        let mut s = launch(&root);
        assert_eq!(s.error, None);
        assert!(s.start_shown() && !s.song.is_sample());
        add(&mut s, "mine");
        let place = root.join("mine");
        s.finish_file_request_sync(crate::studio::FileRequest::SaveAs, &place);
        assert_eq!(s.error, None);
        s.release_song();
        drop(s);

        // The second: the song the window closed on.
        let mut s = launch(&root);
        assert_eq!(s.error, None);
        assert_eq!(s.song.record_dir(), Some(place.clone()));
        assert!(s.project().tracks.values().any(|t| t.name == "mine"));
        // Closed on the sample, the sample comes back.
        s.open_sample_sync();
        assert!(s.song.is_sample());
        s.release_song();
        drop(s);
        let mut s = launch(&root);
        assert!(s.song.is_sample());
        assert_eq!(s.keep.settled_last_open(), Some(LastOpen::Sample));
        // Closed on a new song, a new song.
        s.new_song_sync();
        s.release_song();
        drop(s);
        let s = launch(&root);
        assert!(s.start_shown() && !s.song.is_sample());
        drop(s);

        // A song whose folder is gone: an empty song, and nothing said.
        let gone = root.join("gone");
        RecentList::in_dir(&root)
            .closed_with(&LastOpen::Song(gone))
            .unwrap();
        let s = launch(&root);
        assert_eq!(s.error, None);
        assert!(s.start_shown());
    }

    #[test]
    fn an_edited_sample_reopens_as_the_draft_it_was_kept_in() {
        let tmp = scratch("sample-edit");
        let root = tmp.path().to_path_buf();
        let mut s = launch(&root);
        s.open_sample_sync();
        add(&mut s, "added");
        let draft = s.song.record_dir().expect("kept as a draft");
        s.release_song();
        drop(s);
        let s = launch(&root);
        assert_eq!(s.song.record_dir(), Some(draft));
        assert!(s.project().tracks.values().any(|t| t.name == "added"));
    }

    #[test]
    fn a_recent_song_in_the_menu_opens_that_song() {
        let (mut s, root, _tmp) = kept("menu-recent");
        // Two songs, each left behind as the other is started.
        add(&mut s, "first");
        let first = s.song.record_dir().unwrap();
        s.new_song_sync();
        add(&mut s, "second");
        s.keep.settle_recent();
        s.keep.poll_recent();
        let g = s.geometry(1440.0, 860.0);
        s.open_popup(
            crate::popup::PopupKind::Song,
            crate::studio::Rect::new(40.0, -34.0, 160.0, 28.0),
        );
        let p = s.popup.as_ref().unwrap();
        assert_eq!(p.recent.len(), 1, "{:?}", p.recent);
        assert_eq!(p.recent[0].dir, first);
        let l = p.layout(g.size);
        let row = l
            .cells
            .iter()
            .find(|c| c.action == Some(crate::popup::Action::OpenRecent(0)))
            .unwrap();
        assert!(
            row.text.ends_with("（保存前）"),
            "a draft says so: {}",
            row.text
        );
        s.popup_action(&g, row.action.clone().unwrap());
        assert!(s.error.is_none(), "{:?}", s.error);
        s.wait_for_open_for_test();
        assert!(s.project().tracks.values().any(|t| t.name == "first"));
        assert!(!s.project().tracks.values().any(|t| t.name == "second"));
        // And the one it left is now the recent one.
        let left = s.keep.settled_recent();
        assert!(
            left.iter()
                .any(|r| r.dir != first && r.dir.starts_with(root.join("drafts")))
        );
    }
}
