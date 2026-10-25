//! Serial preference I/O on the bounded readers; UI changes never wait for disk.
use std::{collections::VecDeque, path::PathBuf, time::Instant};
use xirius_session::{
    places::{self, Prefs},
    read_jobs::ReadJobs,
};
#[derive(Clone)]
enum Action {
    Load,
    Save,
    Add(PathBuf),
}
type Reply = Result<Prefs, String>;
type Save = (PathBuf, Prefs, u64);
static SAVE_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn write_snapshot(file: &std::path::Path, prefs: &Prefs, version: u64) -> Reply {
    use std::sync::{Arc, Mutex, OnceLock};
    type Writers = std::collections::HashMap<PathBuf, Arc<Mutex<u64>>>;
    static WRITERS: OnceLock<Mutex<Writers>> = OnceLock::new();
    let lock = WRITERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(file.to_path_buf())
        .or_default()
        .clone();
    let mut written = lock.lock().unwrap_or_else(|e| e.into_inner());
    if version > *written {
        places::save(file, prefs)?;
        *written = version;
        #[cfg(test)]
        let _ = xirius_core::io_test::before("prefs_written", file);
    }
    Ok(prefs.clone())
}
#[derive(Default)]
pub(crate) struct PrefsIo {
    jobs: ReadJobs<u64, Reply>,
    pending: Option<(u64, u64, Action, Prefs, Option<PathBuf>)>,
    adds: VecDeque<PathBuf>,
    dirty: bool,
    next: u64,
    pub(crate) at: Option<Instant>,
    latest_save: Option<Save>,
}
impl crate::browser::Browser {
    pub(crate) fn start_prefs(&mut self, file: PathBuf) {
        self.prefs_file = Some(file);
        self.prefs_io.at = None;
    }
    pub(crate) fn save_prefs_later(&mut self) {
        self.prefs_io.dirty = self.prefs_file.is_some();
        self.prefs_io.latest_save = self.prefs_file.clone().map(|file| {
            (
                file,
                self.prefs(),
                SAVE_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            )
        });
    }
    pub(crate) fn add_place_later(&mut self, path: PathBuf) -> bool {
        if self.prefs_io.adds.len() >= 64 {
            self.prefs_error = Some("場所の読み込みを待っています".into());
            return false;
        }
        self.prefs_io.adds.push_back(path);
        true
    }
    pub(crate) fn poll_prefs_io(&mut self) -> bool {
        let mut changed = false;
        if let Some((key, id, action, snapshot, file)) = self.prefs_io.pending.clone()
            && let Some(reply) = self
                .prefs_io
                .jobs
                .take(&key, id)
                .map(|r| r.unwrap_or_else(|_| Err("設定の読み込みが止まりました".into())))
        {
            self.prefs_io.pending = None;
            self.prefs_io.at = Some(Instant::now());
            if file == self.prefs_file {
                match reply {
                    Ok(p) => match action {
                        Action::Load => {
                            let current = self.prefs();
                            let local = current != snapshot;
                            let mut p = p;
                            if current.favourites != snapshot.favourites {
                                p.favourites = current.favourites.clone();
                            }
                            if current.hidden != snapshot.hidden {
                                p.hidden = current.hidden.clone();
                            }
                            if current.show_hidden != snapshot.show_hidden {
                                p.show_hidden = current.show_hidden;
                            }
                            if current.places != snapshot.places {
                                p.places = current.places.clone();
                            }
                            changed = p != current;
                            self.apply(p);
                            if local {
                                self.save_prefs_later();
                            }
                        }
                        Action::Add(path) => {
                            if snapshot.places == self.places {
                                self.places = p.places;
                                self.prefs_notice = self
                                    .places
                                    .last()
                                    .map(|p| format!("場所のタブ「{}」を足しました", p.name));
                                self.set_location(self.places.len());
                                self.save_prefs_later();
                                changed = true;
                            } else {
                                self.prefs_io.adds.push_front(path);
                            }
                        }
                        _ => {}
                    },
                    Err(e) => {
                        self.prefs_error = Some(e);
                        changed = true;
                    }
                }
            }
        }
        if self.prefs_io.pending.is_some() {
            return changed;
        }
        let action = if let Some(path) = self.prefs_io.adds.pop_front() {
            Action::Add(path)
        } else if self.prefs_io.dirty {
            self.prefs_io.dirty = false;
            Action::Save
        } else if self.prefs_file.is_some()
            && self
                .prefs_io
                .at
                .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1))
        {
            Action::Load
        } else {
            return changed;
        };
        let snapshot = self.prefs();
        let prefs = snapshot.clone();
        let file = self.prefs_file.clone();
        let path = file.clone();
        let work = action.clone();
        let save = self.prefs_io.latest_save.clone();
        self.prefs_io.next += 1;
        let key = self.prefs_io.next;
        let read = move || {
            #[cfg(test)]
            xirius_core::io_test::before(
                match work {
                    Action::Load => "prefs_load",
                    Action::Save => "prefs_save",
                    Action::Add(_) => "place_add",
                },
                match &work {
                    Action::Add(p) => p.as_path(),
                    _ => path.as_deref().unwrap_or(std::path::Path::new("")),
                },
            )
            .map_err(|e| e.to_string())?;
            match work {
                Action::Load => places::load(path.as_deref().expect("preference path")),
                Action::Save => {
                    if let Some((file, prefs, version)) = save {
                        write_snapshot(&file, &prefs, version)
                    } else {
                        Ok(prefs)
                    }
                }
                Action::Add(path) => {
                    let mut prefs = prefs;
                    prefs.add_place(&path, None)?;
                    Ok(prefs)
                }
            }
        };
        let id = if matches!(action, Action::Save) {
            self.prefs_io.jobs.request_persistent(key, read)
        } else {
            self.prefs_io.jobs.request(key, read)
        };
        if let Some(id) = id {
            self.prefs_io.pending = Some((key, id, action, snapshot, file));
        } else {
            match action {
                Action::Add(p) => self.prefs_io.adds.push_front(p),
                Action::Save => self.prefs_io.dirty = true,
                _ => {}
            }
        }
        changed
    }
    #[cfg(test)]
    pub(crate) fn settle_prefs_io(&mut self) {
        let end = Instant::now() + std::time::Duration::from_secs(15);
        loop {
            self.poll_prefs_io();
            if self.prefs_io.pending.is_none()
                && self.prefs_io.adds.is_empty()
                && !self.prefs_io.dirty
            {
                return;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
}
impl Drop for PrefsIo {
    fn drop(&mut self) {
        if let Some((file, prefs, version)) = self.latest_save.take() {
            // Already accepted preference writes outlive the window. A version
            // guard prevents an old delayed write replacing a newer one.
            let _ = self
                .jobs
                .request_persistent(self.next.wrapping_add(1), move || {
                    write_snapshot(&file, &prefs, version)
                });
        }
    }
}
