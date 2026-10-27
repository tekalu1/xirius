//! Serial preference I/O on the bounded readers; UI changes never wait for disk.
use std::{collections::VecDeque, path::PathBuf, time::Instant};
use xirius_session::{
    places::{self, Prefs},
    read_jobs::ReadJobs,
};
#[derive(Clone)]
enum Action {
    Load,
    Add(PathBuf, u64),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::Browser;
    use std::time::Duration;

    fn ready(browser: &Browser) {
        let pending = browser.prefs_io.pending.as_ref().unwrap();
        let end = Instant::now() + Duration::from_secs(15);
        while browser
            .prefs_io
            .jobs
            .peek(&pending.key, pending.id)
            .is_none()
        {
            assert!(Instant::now() < end, "read completion watchdog");
            std::thread::yield_now();
        }
    }

    #[test]
    fn a_completed_read_cannot_undo_a_later_edit_back_to_the_same_value() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("browser.json");
        let mut disk = Prefs::first_run(None);
        places::save(&file, &disk).unwrap();
        let mut browser = Browser::default();
        browser.use_prefs_fixture(file.clone()).unwrap();
        disk.favourites.insert("Choice".into());
        disk.hidden.insert("Unread field".into());
        places::save(&file, &disk).unwrap();
        browser.poll_prefs_io();
        ready(&browser); // The old data is already returned, but not yet adopted.
        let _ui = xirius_core::io_test::forbid_ui_io();
        browser.toggle_favourite("Choice");
        browser.toggle_favourite("Choice");
        browser.poll_prefs_io();
        assert!(!browser.favourites.contains("Choice"));
        browser.settle_prefs_io();
        assert!(!browser.favourites.contains("Choice"));
        assert!(browser.hidden.contains("Unread field"));
        drop(_ui);
        assert!(!places::load(&file).unwrap().favourites.contains("Choice"));
    }

    #[test]
    fn a_late_place_add_keeps_later_navigation_and_file_selection() {
        for moved in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let old = root.path().join("Old");
            let new = root.path().join("New");
            let last = root.path().join("Last");
            std::fs::create_dir_all(old.join("sub")).unwrap();
            std::fs::create_dir_all(&new).unwrap();
            std::fs::create_dir_all(&last).unwrap();
            let selected = old.join("sub/held.wav").to_string_lossy().into_owned();
            std::fs::write(&selected, b"fixture").unwrap();
            let file = root.path().join("browser.json");
            let mut prefs = Prefs::first_run(None);
            prefs.places.push(places::Place {
                name: "Old".into(),
                path: old.clone(),
            });
            places::save(&file, &prefs).unwrap();
            let mut browser = Browser::default();
            browser.use_prefs_fixture(file).unwrap();
            browser.set_location(1);
            browser.add_place_later(new.clone());
            browser.add_place_later(last.clone());
            browser.poll_prefs_io();
            ready(&browser);
            let _ui = xirius_core::io_test::forbid_ui_io();
            if moved {
                browser.set_location(0);
                browser.set_location(1);
                browser.enter_dir(&old.join("sub").to_string_lossy());
                browser.select_file(&selected);
            }
            browser.poll_prefs_io();
            browser.settle_prefs_io();
            assert!(browser.places.iter().any(|p| p.path == new));
            assert!(browser.places.iter().any(|p| p.path == last));
            assert_eq!(
                browser.here_dir(),
                Some(if moved { old.join("sub") } else { last })
            );
            assert_eq!(browser.selected_file, moved.then_some(selected));
        }
    }
}
type Reply = Result<Prefs, String>;
#[derive(Clone)]
struct Pending {
    key: u64,
    id: u64,
    action: Action,
    snapshot: Prefs,
    file: Option<PathBuf>,
    edits: u64,
}

#[derive(Default)]
pub(crate) struct PrefsIo {
    jobs: ReadJobs<u64, Reply>,
    pending: Option<Pending>,
    edits: u64,
    selection: u64,
    adds: VecDeque<(PathBuf, u64)>,
    next: u64,
    pub(crate) at: Option<Instant>,
    save: Option<crate::browser_prefs_save::Ticket>,
    pub(crate) base: Option<Prefs>,
    failed_save: bool,
}
impl crate::browser::Browser {
    pub(crate) fn prefs_selection_changed(&mut self) {
        self.prefs_io.selection = self
            .prefs_io
            .selection
            .checked_add(1)
            .expect("browser selection generation exhausted");
    }
    fn prefs_edited(&mut self) {
        self.prefs_io.edits = self
            .prefs_io
            .edits
            .checked_add(1)
            .expect("browser preferences generation exhausted");
    }
    pub(crate) fn start_prefs(&mut self, file: PathBuf) {
        self.prefs_edited();
        self.prefs_io.base = Some(self.prefs());
        self.prefs_file = Some(file);
        self.prefs_io.at = None;
    }
    pub(crate) fn save_prefs_later(&mut self) {
        self.prefs_edited();
        let prefs = self.prefs();
        let base = self
            .prefs_io
            .base
            .replace(prefs.clone())
            // design-lint: allow(ui-blocking-io) PRJ-031 review: first_run(None) constructs memory defaults without inspecting any home directory.
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
        let selection = self.prefs_io.selection;
        self.prefs_io.adds.push_back((path, selection));
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
        if let Some(pending) = self.prefs_io.pending.clone()
            && let Some(reply) = self
                .prefs_io
                .jobs
                .take(&pending.key, pending.id)
                .map(|r| r.unwrap_or_else(|_| Err("設定の読み込みが止まりました".into())))
        {
            self.prefs_io.pending = None;
            self.prefs_io.at = Some(Instant::now());
            let Pending {
                action,
                snapshot,
                file,
                edits,
                ..
            } = pending;
            if file == self.prefs_file
                && matches!(action, Action::Load)
                && edits != self.prefs_io.edits
            {
                // A new local intent wins even if its value returned to the old snapshot.
                // Retry after the accepted writer has completed; do not show stale errors.
                self.prefs_io.at = None;
            } else if file == self.prefs_file {
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
                        Action::Add(path, selection) => {
                            if snapshot.places == self.places {
                                self.places = p.places;
                                self.prefs_notice = self
                                    .places
                                    .last()
                                    .map(|p| format!("場所のタブ「{}」を足しました", p.name));
                                if selection == self.prefs_io.selection {
                                    self.select_added_place(self.places.len());
                                }
                                self.save_prefs_later();
                                changed = true;
                            } else {
                                self.prefs_io.adds.push_front((path, selection));
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
        let action = if let Some((path, selection)) = self.prefs_io.adds.pop_front() {
            Action::Add(path, selection)
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
                    Action::Add(..) => "place_add",
                },
                match &work {
                    Action::Add(p, _) => p.as_path(),
                    _ => path.as_deref().unwrap_or(std::path::Path::new("")),
                },
            )
            .map_err(|e| e.to_string())?;
            match work {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only browser transaction/read helper or cleanup read_view; UI only submits and polls.
                Action::Load => places::load(path.as_deref().expect("preference path")),
                Action::Add(path, _) => {
                    let mut prefs = prefs;
                    prefs.add_place(&path, None)?;
                    Ok(prefs)
                }
            }
        };
        let id = self.prefs_io.jobs.request(key, read);
        if let Some(id) = id {
            self.prefs_io.pending = Some(Pending {
                key,
                id,
                action,
                snapshot,
                file,
                edits: self.prefs_io.edits,
            });
        } else {
            if let Action::Add(p, selection) = action {
                self.prefs_io.adds.push_front((p, selection));
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
