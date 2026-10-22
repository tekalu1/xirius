//! Another app saved the song the screen has open (UI-289): `xirius --song …` on the command
//! line, or an AI over MCP. The screen watches the folder's `project.json` (the OS's notice,
//! and a look at its time every [`LOOK`] where none comes) and, [`SETTLE`] after the last
//! write, takes the file as one step when the song has no unsaved changes, or asks first.
//!
//! Its own saves are told apart by the file's content: it is the one the session last wrote
//! (`Session::disk_changed`). Nothing is taken while something is going on (a drag, a take,
//! a name being typed, a save); it waits until that is over.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use notify::Watcher;

use crate::studio::{Geometry, Studio};
use crate::toast::{Answer, Toast};

/// How long after the last write the file is taken, so a run of writes counts once.
pub const SETTLE: Duration = Duration::from_millis(300);
/// How often the file's time is looked at, for when the OS says nothing.
pub const LOOK: Duration = Duration::from_secs(2);

/// The song had no unsaved changes, so the file came in.
pub const LOADED: &str = "ほかのアプリの変更を読み込みました";
/// The song has unsaved changes: the file waits for the person.
pub const ASK: &str = "ほかのアプリで曲が変更されました";
pub const LOAD: &str = "読み込む";
pub const UNDO: &str = "元に戻す";

const FILE: &str = "project.json";

/// The watch on one song folder.
struct Watch {
    dir: PathBuf,
    /// The OS's notice; none where it cannot be had.
    _os: Option<notify::RecommendedWatcher>,
    /// When the OS last said `project.json` changed, not yet taken.
    heard: Arc<Mutex<Option<Instant>>>,
    /// The file's time and length at the last look.
    stamp: Option<(SystemTime, u64)>,
    looked: Instant,
}

impl Watch {
    fn new(dir: PathBuf) -> Watch {
        let heard = Arc::new(Mutex::new(None));
        let os = {
            let heard = heard.clone();
            notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                let Ok(ev) = ev else {
                    return;
                };
                if !ev.kind.is_access()
                    && ev
                        .paths
                        .iter()
                        .any(|p| p.file_name() == Some(OsStr::new(FILE)))
                {
                    *heard.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                }
            })
            .and_then(|mut w| {
                w.watch(&dir, notify::RecursiveMode::NonRecursive)
                    .map(|_| w)
            })
            .ok()
        };
        Watch {
            stamp: stamp(&dir),
            dir,
            _os: os,
            heard,
            looked: Instant::now(),
        }
    }

    /// When the file last changed, if it did since the last call.
    fn changed(&mut self, now: Instant) -> Option<Instant> {
        let heard = self.heard.lock().unwrap_or_else(|e| e.into_inner()).take();
        let mut looked = None;
        if now.duration_since(self.looked) >= LOOK {
            self.looked = now;
            let s = stamp(&self.dir);
            if s != self.stamp {
                self.stamp = s;
                looked = Some(now);
            }
        }
        heard.max(looked)
    }
}

fn stamp(dir: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(dir.join(FILE)).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// What the screen knows of the other apps' saves.
#[derive(Default)]
pub struct OtherChange {
    watch: Option<Watch>,
    /// The last write heard; the file is taken [`SETTLE`] after it.
    heard: Option<Instant>,
    /// The file the question is about (its SHA-256), while it shows.
    asked: Option<String>,
    /// The file the person said no to (×): asked again only once it changes again.
    refused: Option<String>,
    /// The step the file came in as, while the toast that can take it back shows.
    loaded: Option<u64>,
}

impl Studio {
    /// Looks for another app's save. True when the screen changed.
    pub(crate) fn poll_other_change(&mut self, g: impl FnOnce(&Studio) -> Geometry) -> bool {
        let now = Instant::now();
        let dir = self.song.dir();
        if self.other.watch.as_ref().map(|w| &w.dir) != dir.as_ref() {
            // Another song, or the same in a new place: its file is the session's own.
            self.other = OtherChange {
                watch: dir.map(Watch::new),
                ..OtherChange::default()
            };
            return false;
        }
        let mut changed = false;
        if let Some(t) = self.other.watch.as_mut().and_then(|w| w.changed(now)) {
            self.other.heard = Some(self.other.heard.map_or(t, |h| h.max(t)));
        }
        if self.other.loaded.is_some()
            && (!self.toast_is(Answer::UndoReload)
                || self.other.loaded != Some(self.song.session().cursor()))
        {
            // Undone, or another edit came after it: the toast's button would take back
            // something else.
            self.other.loaded = None;
            if self.toast_is(Answer::UndoReload) {
                self.toast = None;
                changed = true;
            }
        }
        if self.other.asked.is_some() && !self.toast_is(Answer::Reload) {
            // Another toast took the question's place: ask again when there is room.
            self.other.asked = None;
            self.other.heard.get_or_insert(now - SETTLE);
        }
        let Some(t) = self.other.heard else {
            return changed;
        };
        if now < t + SETTLE || self.other_busy() {
            return changed;
        }
        self.other.heard = None;
        let g = g(self);
        self.other_check(&g) || changed
    }

    /// Something is going on that a new song under it would break: it waits.
    pub(crate) fn other_busy(&self) -> bool {
        self.saving.is_some()
            || self.pointer_held
            || self.transport.recording
            || self.transport.take.is_some()
            || self.export.running()
            || self.input_map.name.is_some()
            || self.browser.typing
            || self.popup.as_ref().is_some_and(|p| p.naming.is_some())
            || self.preparing.is_some()
            || self.recovery.is_some()
            // A warning is not replaced by the news.
            || self.toast_shown().is_some_and(|t| t.alert)
    }

    fn toast_is(&self, answer: Answer) -> bool {
        self.toast_shown()
            .is_some_and(|t| t.button.as_ref().is_some_and(|(_, a)| *a == answer))
    }

    /// The file settled: takes it, or asks.
    fn other_check(&mut self, g: &Geometry) -> bool {
        let Some(hash) = self.song.disk_changed() else {
            // The file is the song's own again (it was saved over): nothing to ask.
            if self.other.asked.take().is_some() && self.toast_is(Answer::Reload) {
                self.toast = None;
                return true;
            }
            return false;
        };
        if [&self.other.refused, &self.other.asked].contains(&&Some(hash.clone())) {
            return false;
        }
        if self.song.session().is_dirty() {
            self.other.asked = Some(hash);
            self.toast = Some(Toast {
                info: true,
                button: Some((LOAD.to_string(), Answer::Reload)),
                ..Toast::plain(ASK.to_string())
            });
            return true;
        }
        self.other_take(Some(g), hash)
    }

    /// 「読み込む」: takes the file as it is now.
    pub(crate) fn other_load(&mut self, g: Option<&Geometry>) {
        self.other.asked = None;
        if let Some(hash) = self.song.disk_changed() {
            self.other_take(g, hash);
        }
    }

    fn other_take(&mut self, g: Option<&Geometry>, hash: String) -> bool {
        match self.song.reload_from_disk() {
            Ok(Some(n)) => {
                self.error = None;
                self.after_edit();
                if let Some(g) = g {
                    self.rebuild(g);
                }
                self.other.loaded = Some(n);
                self.toast = Some(Toast {
                    info: true,
                    brief: true,
                    button: Some((UNDO.to_string(), Answer::UndoReload)),
                    ..Toast::plain(LOADED.to_string())
                });
            }
            // It held the song as it is: it only counts as saved now.
            Ok(None) => {}
            Err(e) => {
                eprintln!("UI-289: project.json could not be taken: {e}");
                self.other.refused = Some(hash);
                self.warn("ほかのアプリの変更を読み込めませんでした".to_string(), None);
            }
        }
        true
    }

    /// 「元に戻す」: the same as Ctrl+Z.
    pub(crate) fn other_undo(&mut self, g: Option<&Geometry>) {
        self.other.loaded = None;
        match g {
            Some(g) => {
                self.undo(g);
            }
            None => {
                if self.automation_history(false) {
                    self.after_edit();
                }
            }
        }
    }

    /// The question was closed with ×: the screen keeps its song, until the file changes again.
    pub(crate) fn other_refused(&mut self) {
        if self.toast_is(Answer::Reload) {
            self.other.refused = self.other.asked.take();
        }
    }
}

#[cfg(test)]
#[path = "other_change/tests.rs"]
mod tests;
