//! Serial preference I/O on the bounded readers; UI changes never wait for disk.
use std::{collections::VecDeque, path::PathBuf, time::Instant};
use xirius_session::{
    places::{self, Prefs},
    read_jobs::ReadJobs,
};
#[derive(Clone)]
enum Action {
    Load,
    Add(PathBuf),
}
type Reply = Result<Prefs, String>;
#[derive(Default)]
pub(crate) struct PrefsIo {
    jobs: ReadJobs<u64, Reply>,
    pending: Option<(u64, u64, Action, Prefs, Option<PathBuf>)>,
    adds: VecDeque<PathBuf>,
    next: u64,
    pub(crate) at: Option<Instant>,
    save: Option<crate::browser_prefs_save::Ticket>,
    pub(crate) base: Option<Prefs>,
    failed_save: bool,
}
impl crate::browser::Browser {
    pub(crate) fn start_prefs(&mut self, file: PathBuf) {
        self.prefs_io.base = Some(self.prefs());
        self.prefs_file = Some(file);
        self.prefs_io.at = None;
    }
    pub(crate) fn save_prefs_later(&mut self) {
        let prefs = self.prefs();
        let base = self
            .prefs_io
            .base
            .replace(prefs.clone())
            .unwrap_or_else(|| Prefs::first_run(None));
        self.prefs_io.failed_save = false;
        self.prefs_io.save = self
            .prefs_file
            .as_deref()
            .map(|file| crate::browser_prefs_save::submit(file, base, prefs));
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
        if let Some(result) = self.prefs_io.save.as_ref().and_then(|save| save.result()) {
            self.prefs_io.save = None;
            match result {
                Ok(()) => {
                    self.prefs_io.base = Some(self.prefs());
                    self.prefs_io.failed_save = false;
                }
                Err(e) => {
                    self.prefs_io.failed_save = true;
                    self.prefs_error = Some(e);
                    changed = true;
                }
            }
        }
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
                            let disk = p.clone();
                            let p = crate::browser_prefs_save::merge(&snapshot, &current, p);
                            changed = p != current;
                            self.apply(p);
                            self.prefs_io.base = Some(disk);
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
        } else if !self.prefs_io.failed_save
            && self.prefs_io.save.is_none()
            && self.prefs_file.is_some()
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
        self.prefs_io.next += 1;
        let key = self.prefs_io.next;
        let read = move || {
            #[cfg(test)]
            xirius_core::io_test::before(
                match work {
                    Action::Load => "prefs_load",
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
                Action::Add(path) => {
                    let mut prefs = prefs;
                    prefs.add_place(&path, None)?;
                    Ok(prefs)
                }
            }
        };
        let id = self.prefs_io.jobs.request(key, read);
        if let Some(id) = id {
            self.prefs_io.pending = Some((key, id, action, snapshot, file));
        } else {
            if let Action::Add(p) = action {
                self.prefs_io.adds.push_front(p);
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
                && self.prefs_io.save.is_none()
            {
                return;
            }
            assert!(Instant::now() < end);
            std::thread::yield_now();
        }
    }
}
