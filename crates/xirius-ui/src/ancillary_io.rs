//! FIFO for application files and desktop requests (PRJ-031).
//! Work never holds the reply lock. Abandoned queued reads are skipped; detached
//! writes remain accepted. Shutdown observes `pending` before leaving the executor.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

type Work = Box<dyn FnOnce() + Send>;
struct Owner {
    send: mpsc::Sender<Work>,
    pending: Arc<AtomicUsize>,
}
static OWNER: OnceLock<Result<Owner, String>> = OnceLock::new();

pub(crate) struct Ticket<T> {
    result: Arc<Mutex<Option<Result<T, String>>>>,
    cancelled: Arc<AtomicBool>,
    detached: bool,
}
impl<T> Ticket<T> {
    pub(crate) fn poll(&self) -> Option<Result<T, String>> {
        self.result.try_lock().ok()?.take()
    }
    fn detach(mut self) {
        self.detached = true;
    }
}
impl<T> Drop for Ticket<T> {
    fn drop(&mut self) {
        if !self.detached {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}

pub(crate) fn pending() -> bool {
    OWNER
        .get()
        .and_then(|o| o.as_ref().ok())
        .is_some_and(|o| o.pending.load(Ordering::Acquire) != 0)
}

pub(crate) fn submit<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<Ticket<T>, String> {
    let owner = OWNER
        .get_or_init(|| {
            let (send, receive) = mpsc::channel::<Work>();
            let pending = Arc::new(AtomicUsize::new(0));
            let count = pending.clone();
            std::thread::Builder::new()
                .name("application files".into())
                .spawn(move || {
                    for work in receive {
                        work();
                        count.fetch_sub(1, Ordering::Release);
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(Owner { send, pending })
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let result = Arc::new(Mutex::new(None));
    let reply = result.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    owner.pending.fetch_add(1, Ordering::Release);
    if owner
        .send
        .send(Box::new(move || {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| Err("付随処理が停止しました".into()));
            if let Err(e) = &got {
                use std::io::Write;
                // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only failure report after catch_unwind; no reply mutex is held during stderr I/O.
                let _ = writeln!(std::io::stderr(), "PRJ-031: {e}");
            }
            // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
            *reply.lock().unwrap_or_else(|e| e.into_inner()) = Some(got);
        }))
        .is_err()
    {
        owner.pending.fetch_sub(1, Ordering::Release);
        return Err("付随処理の窓口が閉じています".into());
    }
    Ok(Ticket {
        result,
        cancelled,
        detached: false,
    })
}

/// A write or desktop action whose failure is reported in the log.
pub(crate) fn dispatch(
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    submit(work).map(Ticket::detach)
}

/// stderr may be a slow file or pipe. Never write it from a window callback.
pub(crate) fn write_message(message: String) {
    let _ = dispatch(move || {
        #[cfg(any(test, feature = "loading-probe"))]
        xirius_core::io_test::before("log_write", std::path::Path::new("stderr"))
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        writeln!(std::io::stderr(), "{message}").map_err(|e| e.to_string())
    });
}
macro_rules! log {
    ($($arg:tt)*) => { $crate::ancillary_io::write_message(format!($($arg)*)) };
}
pub(crate) use log;

#[derive(Default)]
pub(crate) struct ViewRead {
    pub(crate) picker: Option<crate::browser_pick::PickerIntent>,
    pub(crate) picker_generation: u64,
    pub(crate) history_points: Option<crate::automation_point_edit::HistoryPoints>,
    pub(crate) groove: Option<(crate::quantize_import::Intent, Ticket<serde_json::Value>)>,
    pub(crate) groove_generation: u64,
    pub(crate) cleanup: crate::cleanup_dialog::Read,
    pub(crate) left: Option<Ticket<usize>>,
    pub(crate) later: Option<(std::path::PathBuf, Ticket<Vec<std::path::PathBuf>>)>,
    job: Option<Ticket<(crate::drawer_edge::DrawerSize, crate::side_edge::SideSize)>>,
    base: Option<(crate::drawer_edge::DrawerSize, crate::side_edge::SideSize)>,
    pub(crate) drawer_changed: std::cell::Cell<bool>,
    pub(crate) side_changed: std::cell::Cell<bool>,
}

impl crate::studio::Studio {
    /// After the last window closes, keep the executor pumping while the FIFO
    /// checks/launches an installer and finishes or defers cleanup.
    pub(crate) fn finish_window_io(&mut self) -> Result<(), String> {
        let close = self.open_closing.take();
        let tidies = std::mem::take(&mut self.tidying);
        let install = self.update.pending.take();
        let installer = self.update.ready.clone();
        let key = self.update.key.clone();
        let root = self.update.root.clone();
        let spawn = self.update.spawn.clone();
        dispatch(move || {
            if let Some(install) = install {
                let checked = installer
                    .ok_or_else(|| "インストーラーがありません".to_string())
                    .and_then(|i| i.verify_with(&key));
                match checked {
                    Ok(()) => {
                        if let Err(e) = spawn(&install) {
                            eprintln!("注意: {e}");
                        }
                    }
                    Err(e) => {
                        if let Some(root) = root {
                            let _ = std::fs::write(root.join(xirius_session::update::FAILED), b"");
                        }
                        eprintln!("注意: {e}");
                    }
                }
            }
            let until = std::time::Instant::now() + crate::tidy::CLOSE_WAIT;
            let n = close.map_or(0, |c| c.wait(crate::tidy::CLOSE_WAIT))
                + crate::tidy::wait_all(
                    tidies,
                    until.saturating_duration_since(std::time::Instant::now()),
                    &crate::notice::later_path(),
                    &mut |e| eprintln!("注意: {e}"),
                );
            crate::notice::leave(&crate::notice::left_path(), n).map_err(|e| e.to_string())
        })
    }

    pub(crate) fn read_view(&mut self, path: std::path::PathBuf) {
        self.drawer_size.file = Some(path.clone());
        self.side_size.file = Some(path.clone());
        self.view_read.drawer_changed.set(false);
        self.view_read.side_changed.set(false);
        self.view_read.base = Some((self.drawer_size.clone(), self.side_size.clone()));
        match submit(move || {
            #[cfg(any(test, feature = "loading-probe"))]
            xirius_core::io_test::before("view_read", &path).map_err(|e| e.to_string())?;
            Ok((
                crate::drawer_edge::DrawerSize::load(&path),
                crate::side_edge::SideSize::load(&path),
            ))
        }) {
            Ok(job) => self.view_read.job = Some(job),
            Err(e) => self.error = Some(e),
        }
    }

    pub(crate) fn poll_view(&mut self) -> bool {
        let versions = self.song.poll_file_reads();
        if versions && !self.input_blocked() {
            self.refresh_popup();
        }
        let notices = self.poll_left_notices()
            | self.poll_cleanup_read()
            | self.poll_groove_import()
            | versions;
        let Some(result) = self.view_read.job.as_ref().and_then(Ticket::poll) else {
            return notices;
        };
        self.view_read.job = None;
        let base = self.view_read.base.take();
        match result {
            Ok((drawer, side)) => {
                if let Some((old_drawer, old_side)) = base {
                    if !self.view_read.drawer_changed.get() && self.drawer_size == old_drawer {
                        self.drawer_size = drawer;
                    }
                    if !self.view_read.side_changed.get() && self.side_size == old_side {
                        self.side_size = side;
                    }
                }
            }
            Err(e) => self.error = Some(e),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn stopped_stderr_does_not_hold_window_callbacks() {
        let mut s = crate::studio::Studio::new(crate::song::Song::empty());
        s.bind_window_io();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let (tx, entered) = mpsc::channel();
        let ui = std::thread::current().id();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, _| {
            if op == "log_write" {
                assert_ne!(std::thread::current().id(), ui);
                let _ = tx.send(());
                drop(
                    gate.1
                        .wait_while(gate.0.lock().unwrap(), |go| !*go)
                        .unwrap(),
                );
            }
            Ok(())
        }));
        let scope = xirius_core::io_test::forbid_ui_io();
        log!("PRJ-031 stopped stderr fixture");
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        for _ in 0..8 {
            s.tick();
            s.idle();
            let g = s.geometry(880.0, 600.0);
            s.key(&g, "escape", false, false);
        }
        // Reaching here acknowledges the callbacks while the actual write stays held.
        drop(scope);
        drop(release);
    }

    #[test]
    fn abandoned_reads_are_skipped_but_accepted_writes_and_later_work_survive() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (tx, entered) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let held = submit(move || {
            tx.send(()).unwrap();
            let _ = gate.recv();
            Ok(())
        })
        .unwrap();
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let read = reads.clone();
        let obsolete = submit(move || {
            read.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        drop(obsolete);
        let writes = Arc::new(AtomicUsize::new(0));
        let write = writes.clone();
        dispatch(move || {
            write.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
        let failure: Ticket<()> = submit(|| panic!("fixture failure")).unwrap();
        let later = submit(|| Ok(7)).unwrap();
        release.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(got) = later.poll() {
                assert_eq!(got.unwrap(), 7);
                break;
            }
            assert!(Instant::now() < end, "FIFO completion watchdog");
            std::thread::yield_now();
        }
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert!(failure.poll().unwrap().is_err());
        assert_eq!(held.poll(), Some(Ok(())));
    }

    #[test]
    fn version_cleanup_groove_and_first_draft_reads_have_one_request_while_held() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for operation in [
            "version_list",
            "version_detail",
            "cleanup_list",
            "groove_read",
            "draft_create",
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut s = crate::studio::Studio::new(crate::song::Song::empty());
            if operation != "draft_create" {
                s.song.save_as(&root.path().join("song")).unwrap();
                s.song.save_version("Fixture").unwrap();
            } else {
                s.keep = crate::keep::Keep::at(root.path().to_owned());
            }
            let mut midi = b"MThd".to_vec();
            midi.extend([0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
            midi.extend(b"MTrk");
            midi.extend([
                0, 0, 0, 12, 0, 0x90, 60, 80, 96, 0x80, 60, 0, 0, 0xff, 0x2f, 0,
            ]);
            let file = root.path().join("groove.mid");
            std::fs::write(&file, midi).unwrap();
            let version = s
                .song
                .versions()
                .first()
                .map(|v| v.id.clone())
                .unwrap_or_default();
            s.bind_window_io();
            let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
            struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
            impl Drop for Release {
                fn drop(&mut self) {
                    *self.0.0.lock().unwrap() = true;
                    self.0.1.notify_all();
                }
            }
            let release = Release(gate.clone());
            let calls = Arc::new(AtomicUsize::new(0));
            let count = calls.clone();
            let (tx, entered) = mpsc::channel();
            let ui = std::thread::current().id();
            let source = root.path().to_owned();
            let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if path.starts_with(&source) && op == operation {
                    assert_ne!(std::thread::current().id(), ui);
                    count.fetch_add(1, Ordering::SeqCst);
                    let _ = tx.send(());
                    drop(
                        gate.1
                            .wait_while(gate.0.lock().unwrap(), |go| !*go)
                            .unwrap(),
                    );
                }
                Ok(())
            }));
            let scope = xirius_core::io_test::forbid_ui_io();
            match operation {
                "version_list" => {
                    s.song.versions();
                }
                "version_detail" => {
                    s.song.version_detail(&version);
                }
                "cleanup_list" => {
                    s.open_cleanup();
                }
                "groove_read" => {
                    s.import_groove(&file).unwrap();
                }
                _ => {
                    assert!(s.run_command("track.add.audio", serde_json::json!({"name":"Draft"})));
                    assert!(s.run_command(
                        "track.add.audio",
                        serde_json::json!({"name":"Same callback"})
                    ));
                    assert!(
                        s.saving.is_none(),
                        "edits in one callback must keep the actual Session"
                    );
                    s.popup = Some(crate::popup::Popup::new(
                        crate::popup::PopupKind::Grid,
                        crate::studio::Rect::default(),
                    ));
                    s.pointer_held = true;
                    s.keep_beat(xirius_core::store::now_ms());
                    assert!(
                        s.popup.is_some(),
                        "draft creation preserves the current popover"
                    );
                    assert!(
                        s.saving.is_none(),
                        "a held drag must not be cancelled by draft creation"
                    );
                    s.pointer_held = false;
                    s.keep_beat(xirius_core::store::now_ms());
                    assert!(s.popup.is_some(), "an open popover retains its interaction");
                    assert!(
                        s.saving.is_none(),
                        "draft handoff waits for the popover too"
                    );
                    s.key(&s.geometry(880.0, 600.0), "escape", false, false);
                    s.keep_beat(xirius_core::store::now_ms());
                }
            }
            entered.recv_timeout(Duration::from_secs(30)).unwrap();
            for _ in 0..8 {
                s.tick();
                s.idle();
                let g = s.geometry(880.0, 600.0);
                s.key(&g, "escape", false, false);
                if operation == "version_list" {
                    assert!(s.song.versions().is_empty());
                }
                if operation == "version_detail" {
                    assert!(s.song.version_detail(&version).is_none());
                }
            }
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "duplicate {operation} read while held"
            );
            if operation == "draft_create" {
                assert!(s.song.record_dir().is_none());
                assert!(s.saving.is_some());
            }
            drop(scope);
            drop(release);
            let end = Instant::now() + Duration::from_secs(30);
            loop {
                s.tick();
                s.idle();
                let done = match operation {
                    "version_list" => !s.song.versions().is_empty(),
                    "version_detail" => s.song.version_detail(&version).is_some(),
                    "cleanup_list" => s.view_read.cleanup.job.is_none(),
                    "groove_read" => s.view_read.groove.is_none(),
                    _ => s.saving.is_none(),
                };
                if done {
                    break;
                }
                assert!(
                    Instant::now() < end,
                    "{operation} completion watchdog: {:?}",
                    s.error
                );
                std::thread::yield_now();
            }
            if operation == "draft_create" {
                assert!(s.song.is_draft());
                assert!(s.song.session().is_dirty());
                assert!(
                    s.project()
                        .tracks
                        .values()
                        .any(|t| t.name == "Same callback")
                );
                s.keep.settle_recent();
            }
            if operation == "groove_read" {
                assert!(s.error.is_none(), "{:?}", s.error);
                assert!(s.quant.groove_on);
            }
        }
    }

    #[test]
    fn held_view_read_does_not_block_gestures_frames_or_idle_and_writes_follow_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("view.json");
        std::fs::write(
            &path,
            r#"{"drawer_h":{"roll":390},"browser_w":230,"other":7}"#,
        )
        .unwrap();
        let (entered, arrived) = mpsc::channel();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        let release = Release(gate.clone());
        let watched = path.clone();
        let ui = std::thread::current().id();
        let _observe = xirius_core::io_test::observe(Arc::new(move |op, p| {
            if p == watched && matches!(op, "view_read" | "view_write") {
                assert_ne!(std::thread::current().id(), ui);
                if op == "view_read" {
                    entered.send(()).unwrap();
                    drop(
                        gate.1
                            .wait_while(gate.0.lock().unwrap(), |go| !*go)
                            .unwrap(),
                    );
                }
            }
            Ok(())
        }));
        let mut s = crate::studio::Studio::sample();
        s.bind_window_io();
        s.read_view(path.clone());
        arrived.recv_timeout(Duration::from_secs(30)).unwrap();
        let ui_scope = xirius_core::io_test::forbid_ui_io();
        // Real press, move and release callbacks while the actual read is held.
        let g = s.geometry(1440.0, 820.0);
        let edge = s.drawer_edge(&g).unwrap();
        let y = edge.y + edge.h / 2.0;
        assert!(s.edge_down(&g, 720.0, y, 1));
        assert!(s.edge_move(&g, y - 50.0));
        assert!(s.edge_up());
        // Return to the default too: equality with the starting value is not
        // evidence that no gesture happened while the read was outstanding.
        let g = s.geometry(1440.0, 820.0);
        let edge = s.drawer_edge(&g).unwrap();
        assert!(s.edge_down(&g, 720.0, edge.y + edge.h / 2.0, 2));
        s.tick();
        s.idle();
        let _ = s.geometry(1100.0, 700.0);
        assert!(s.view_read.job.is_some(), "I/O remains held after UI Ack");
        assert_eq!(s.drawer_size.roll, None);
        drop(ui_scope);
        drop(release);
        let end = Instant::now() + Duration::from_secs(30);
        while s.view_read.job.is_some() {
            s.poll_view();
            assert!(Instant::now() < end, "worker completion watchdog");
            std::thread::yield_now();
        }
        assert_eq!(
            s.drawer_size.roll, None,
            "late read must not undo the gesture"
        );
        let verify = submit(move || Ok(crate::drawer_edge::DrawerSize::load(&path))).unwrap();
        loop {
            if let Some(got) = verify.poll() {
                assert_eq!(got.unwrap().roll, None);
                break;
            }
            assert!(Instant::now() < end, "FIFO write watchdog");
            std::thread::yield_now();
        }
    }
}
