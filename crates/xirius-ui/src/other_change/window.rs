//! The watcher and project reads stay with one worker. A request carries the
//! saved fingerprint and model it observed; a later save invalidates its reply.
use super::*;
use std::sync::mpsc::{self, Receiver, SyncSender};
use xirius_core::{model::Project, store::Store};
use xirius_session::background::Job;

struct Request {
    sequence: u64,
    now: Instant,
    model: Arc<Project>,
    saved: Option<String>,
    force: bool,
}
type Disk = xirius_core::session::ExternalProject;
struct Reply {
    request: Request,
    disk: Result<Option<Disk>, String>,
}

pub(super) struct Monitor {
    dir: PathBuf,
    lane: u32,
    send: SyncSender<Request>,
    receive: Receiver<Reply>,
    _owner: Job<()>,
    pending: bool,
    pub(super) accept: bool,
    pub(super) accept_after: u64,
    pub(super) accept_model: Option<Arc<Project>>,
    pub(super) next: u64,
    recheck: bool,
}

impl Monitor {
    fn start(store: Store) -> Result<Self, String> {
        let (send, requests) = mpsc::sync_channel::<Request>(1);
        let (replies, receive) = mpsc::sync_channel(1);
        let dir = store.dir.clone();
        let lane = store.lane;
        let owner = Job::spawn("external project watcher", move || {
            let mut watch = Watch::new(store.dir.clone());
            let mut heard = Some(Instant::now());
            while let Ok(request) = requests.recv() {
                let now = request.now;
                if let Some(t) = watch.changed(now) {
                    heard = Some(t);
                }
                let due = request.force || heard.is_some_and(|t| now >= t + SETTLE);
                let disk = if due {
                    heard = None;
                    #[cfg(any(test, feature = "loading-probe"))]
                    xirius_core::io_test::before("external_project_read", &store.dir)
                        .map_err(|e| e.to_string())?;
                    Disk::read(store.clone(), request.saved.clone())
                        .map(Some)
                        .map_err(|e| e.to_string())
                } else {
                    Ok(None)
                };
                if replies.send(Reply { request, disk }).is_err() {
                    break;
                }
            }
            // RecommendedWatcher destruction can close/join an OS watcher.
            // It is deliberately never sent back to the window.
            Ok(())
        })?;
        Ok(Self {
            dir,
            lane,
            send,
            receive,
            _owner: owner,
            pending: false,
            accept: false,
            accept_after: 0,
            accept_model: None,
            next: 0,
            recheck: false,
        })
    }
}

impl Studio {
    pub(super) fn poll_window_change(
        &mut self,
        now: Instant,
        g: impl FnOnce(&Studio) -> Geometry,
    ) -> bool {
        if self.other_busy() {
            return false;
        }
        let Some(store) = self
            .song
            .session()
            .store()
            .cloned()
            .filter(|_| !self.song.is_draft())
        else {
            self.other = OtherChange::default();
            return false;
        };
        if self
            .other
            .window
            .as_ref()
            .is_none_or(|m| m.dir != store.dir || m.lane != store.lane)
        {
            self.other = OtherChange::default();
            match Monitor::start(store) {
                Ok(m) => self.other.window = Some(m),
                Err(e) => self.error = Some(e),
            }
        }
        let mut changed = false;
        if self.other.asked.is_some() && !self.toast_is(Answer::Reload) {
            self.other.asked = None;
            if let Some(m) = &mut self.other.window {
                m.recheck = true;
            }
        }
        if self.other.loaded.is_some()
            && (!self.toast_is(Answer::UndoReload)
                || self.other.loaded != Some(self.song.session().cursor()))
        {
            self.other.loaded = None;
            if self.toast_is(Answer::UndoReload) {
                self.toast = None;
                changed = true;
            }
        }
        let Some(monitor) = &mut self.other.window else {
            return changed;
        };
        let reply = monitor.receive.try_recv().ok();
        if let Some(reply) = reply {
            monitor.pending = false;
            // A local save after request acceptance wins over the older read.
            if self.song.session().disk_fingerprint() == reply.request.saved.as_deref() {
                match reply.disk {
                    Ok(Some(disk)) if self.song.session().disk_hash_is(disk.hash()) => {
                        if let Some(m) = &mut self.other.window {
                            m.accept = false;
                            m.accept_model = None;
                        }
                        if self.other.asked.take().is_some() && self.toast_is(Answer::Reload) {
                            self.toast = None;
                            changed = true;
                        }
                    }
                    Ok(Some(disk)) => {
                        let hash = disk.hash().to_owned();
                        let accepted = self.other.window.as_ref().is_some_and(|m| {
                            m.accept
                                && reply.request.sequence > m.accept_after
                                && m.accept_model
                                    .as_ref()
                                    .is_some_and(|p| Arc::ptr_eq(p, &self.project()))
                        });
                        if self
                            .other
                            .window
                            .as_ref()
                            .is_some_and(|m| m.accept && reply.request.sequence <= m.accept_after)
                        {
                            // The click requests a fresh read, never an older queued reply.
                        } else if accepted
                            || (self.other.asked.as_deref() != Some(&hash)
                                && self.other.refused.as_deref() != Some(&hash))
                        {
                            if (!accepted && self.song.session().is_dirty())
                                || !Arc::ptr_eq(&reply.request.model, &self.project())
                            {
                                self.other.asked = Some(hash);
                                self.other.window.as_mut().unwrap().accept = false;
                                self.toast = Some(Toast {
                                    info: true,
                                    button: Some((LOAD.into(), Answer::Reload)),
                                    ..Toast::plain(ASK.into())
                                });
                                changed = true;
                            } else {
                                self.other.window.as_mut().unwrap().accept = false;
                                match self
                                    .song
                                    .session_mut()
                                    .adopt_disk_project(disk, &xirius_core::command::Author::Human)
                                {
                                    Ok(Some((n, _))) => {
                                        self.error = None;
                                        self.after_edit();
                                        let geometry = g(self);
                                        self.rebuild(&geometry);
                                        self.other.loaded = Some(n);
                                        self.toast = Some(Toast {
                                            info: true,
                                            brief: true,
                                            button: Some((UNDO.into(), Answer::UndoReload)),
                                            ..Toast::plain(LOADED.into())
                                        });
                                        return true;
                                    }
                                    Ok(None) => {}
                                    Err(e) => {
                                        self.other.refused = Some(hash);
                                        self.warn(
                                            format!(
                                                "ほかのアプリの変更を読み込めませんでした: {e}"
                                            ),
                                            None,
                                        );
                                    }
                                }
                                changed = true;
                            }
                        }
                    }
                    Err(e) => {
                        crate::ancillary_io::log!("external project read: {e}");
                        if let Some(m) = &mut self.other.window {
                            m.accept = false;
                        }
                    }
                    _ => {}
                }
            } else if reply.request.force
                && let Some(m) = &mut self.other.window
            {
                m.accept = false;
                m.accept_model = None;
            }
        }
        let request = Request {
            sequence: self.other.window.as_ref().map_or(1, |m| m.next + 1),
            now,
            model: self.project(),
            saved: self.song.session().disk_fingerprint().map(str::to_owned),
            force: self
                .other
                .window
                .as_ref()
                .is_some_and(|m| m.accept || m.recheck),
        };
        if let Some(monitor) = &mut self.other.window
            && !monitor.pending
        {
            match monitor.send.try_send(request) {
                Ok(()) => {
                    monitor.pending = true;
                    monitor.next += 1;
                    monitor.recheck = false;
                }
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.error = Some("外部変更の監視が停止しました".into());
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::song::Song;
    use std::sync::Condvar;

    #[test]
    fn actual_watch_and_reload_gates_leave_frames_input_and_idle_available() {
        for operation in ["watch_register", "watch_metadata", "external_project_read"] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path().join("song");
            let mut s = Studio::new(Song::empty());
            s.song.save_as(&dir).unwrap();
            let mut other = xirius_core::session::Session::open(&dir).unwrap();
            other
                .run_value(
                    "track.add.audio",
                    serde_json::json!({"name":"External"}),
                    &xirius_core::session::RunOptions::human(),
                )
                .unwrap();
            other.save().unwrap();
            drop(other);
            s.bind_window_io();
            let ui = std::thread::current().id();
            let held = Arc::new((Mutex::new(false), Condvar::new()));
            struct Release(Arc<(Mutex<bool>, Condvar)>);
            impl Drop for Release {
                fn drop(&mut self) {
                    *self.0.0.lock().unwrap() = true;
                    self.0.1.notify_all();
                }
            }
            let release = Release(held.clone());
            let (tx, entered) = mpsc::channel();
            let watched = dir.clone();
            let _observe = xirius_core::io_test::observe(Arc::new(move |op, path| {
                if path.starts_with(&watched) {
                    assert_ne!(std::thread::current().id(), ui, "UI reached {op}");
                    if op == operation {
                        let _ = tx.send(());
                        drop(
                            held.1
                                .wait_while(held.0.lock().unwrap(), |go| !*go)
                                .unwrap(),
                        );
                    }
                }
                Ok(())
            }));
            // A virtual poll clock beyond the watchdog: slow registration or
            // scheduling must not consume the synthetic LOOK/SETTLE advance.
            let future = Instant::now() + Duration::from_secs(3600);
            let end = Instant::now() + Duration::from_secs(30);
            loop {
                s.poll_other_change_at(future, |s| s.geometry(1440.0, 820.0));
                if entered.try_recv().is_ok() {
                    break;
                }
                assert!(Instant::now() < end, "{operation} entry watchdog");
                std::thread::yield_now();
            }
            let before = s.project();
            let ui_scope = xirius_core::io_test::forbid_ui_io();
            for (w, h) in [(1440.0, 820.0), (880.0, 600.0)] {
                s.tick();
                s.idle();
                let g = s.geometry(w, h);
                s.key(&g, "escape", false, false);
                s.poll_other_change_at(future, |s| s.geometry(w, h));
            }
            assert!(
                Arc::ptr_eq(&before, &s.project()),
                "no adoption before I/O release"
            );
            assert!(s.other.window.as_ref().unwrap().pending);
            drop(ui_scope);
            drop(release);
            loop {
                s.poll_other_change_at(future, |s| s.geometry(1440.0, 820.0));
                if s.other.loaded.is_some() {
                    break;
                }
                assert!(
                    Instant::now() < end,
                    "{operation} completion watchdog: {:?}",
                    s.error
                );
                std::thread::yield_now();
            }
            assert!(s.project().tracks.values().any(|t| t.name == "External"));
            assert!(!s.song.session().is_dirty());
            assert!(s.preparing.is_none() && s.recovery.is_none());
            s.other_undo(None);
            s.wait_for_save();
            assert!(s.error.is_none(), "{:?}", s.error);
            assert!(
                !s.project().tracks.values().any(|t| t.name == "External"),
                "window Undo must take the external load back as one step"
            );
            s.other = OtherChange::default();
            drop(_observe);
        }
    }
}
