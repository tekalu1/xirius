//! Process-owned, coalescing preference writer, independent of the read queue.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
};
use xirius_session::places::{self, Prefs};
type Result = std::result::Result<(), String>;
#[derive(Clone)]
pub(crate) struct Ticket(Arc<Mutex<Option<Result>>>);
impl Ticket {
    pub fn result(&self) -> Option<Result> {
        // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
#[derive(Clone, Default)]
struct Intent {
    favourites: std::collections::BTreeMap<String, bool>,
    hidden: std::collections::BTreeMap<String, bool>,
    show_hidden: Option<bool>,
    places: Vec<(PathBuf, Option<places::Place>)>,
    order: Option<Vec<PathBuf>>,
}
impl Intent {
    fn include(&mut self, base: &Prefs, new: &Prefs) {
        for (changes, old, new) in [
            (&mut self.favourites, &base.favourites, &new.favourites),
            (&mut self.hidden, &base.hidden, &new.hidden),
        ] {
            for value in old.symmetric_difference(new) {
                changes.insert(value.clone(), new.contains(value));
            }
        }
        if base.show_hidden != new.show_hidden {
            self.show_hidden = Some(new.show_hidden);
        }
        for path in base.places.iter().chain(&new.places).map(|p| &p.path) {
            let old = base.places.iter().find(|p| &p.path == path);
            let next = new.places.iter().find(|p| &p.path == path);
            if old != next {
                if let Some((_, value)) = self.places.iter_mut().find(|(p, _)| p == path) {
                    *value = next.cloned();
                } else {
                    self.places.push((path.clone(), next.cloned()));
                }
            }
        }
        if base.places != new.places {
            self.order = Some(new.places.iter().map(|p| p.path.clone()).collect());
        }
    }
    fn apply(&self, current: &mut Prefs) {
        for (values, changes) in [
            (&mut current.favourites, &self.favourites),
            (&mut current.hidden, &self.hidden),
        ] {
            for (value, present) in changes {
                if *present {
                    values.insert(value.clone());
                } else {
                    values.remove(value);
                }
            }
        }
        if let Some(show) = self.show_hidden {
            current.show_hidden = show;
        }
        for (path, place) in &self.places {
            match place {
                Some(place) => {
                    if let Some(old) = current.places.iter_mut().find(|p| &p.path == path) {
                        *old = place.clone();
                    } else {
                        current.places.push(place.clone());
                    }
                }
                None => current.places.retain(|p| &p.path != path),
            }
        }
        if let Some(order) = &self.order {
            let mut reordered: Vec<_> = current
                .places
                .iter()
                .filter(|p| order.contains(&p.path))
                .cloned()
                .collect();
            reordered.sort_by_key(|p| order.iter().position(|path| path == &p.path));
            let mut next = reordered.into_iter();
            for place in &mut current.places {
                if order.contains(&place.path) {
                    *place = next.next().expect("same known places");
                }
            }
        }
    }
}
/// Rebase local edits on a completed read without overwriting previously unread fields.
pub(crate) fn merge(base: &Prefs, new: &Prefs, mut disk: Prefs) -> Prefs {
    let mut edits = Intent::default();
    edits.include(base, new);
    edits.apply(&mut disk);
    disk
}
struct Save {
    intent: Intent,
    revision: u64,
    tickets: Vec<Ticket>,
}
#[derive(Default)]
struct State {
    queued: HashMap<PathBuf, Save>,
    dirty: HashMap<PathBuf, (u64, Intent)>,
    next: u64,
    active: Option<PathBuf>,
    failures: HashMap<PathBuf, String>,
}
#[derive(Default)]
struct Writer {
    state: Mutex<State>,
    wake: Condvar,
}
/// This read/modify/write transaction uses the dedicated writer, never a read slot or the UI.
fn write(file: &Path, save: &Save) -> Result {
    places::transaction(file, || {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only browser transaction/read helper or cleanup read_view; UI only submits and polls.
        let mut current = places::load(file)?;
        save.intent.apply(&mut current);
        // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only browser transaction/read helper or cleanup read_view; UI only submits and polls.
        places::save(file, &current)
    })
}
fn writer() -> &'static Arc<Writer> {
    static WRITER: OnceLock<Arc<Writer>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let writer = Arc::new(Writer::default());
        let work = writer.clone();
        std::thread::Builder::new()
            .name("browser-prefs".into())
            .spawn(move || {
                loop {
                    let (file, save) = {
                        let mut s = work.state.lock().unwrap_or_else(|e| e.into_inner());
                        while s.queued.is_empty() {
                            s = work.wake.wait(s).unwrap_or_else(|e| e.into_inner());
                        }
                        let file = s.queued.keys().next().unwrap().clone();
                        s.active = Some(file.clone());
                        (file.clone(), s.queued.remove(&file).unwrap())
                    };
                    let result = std::panic::catch_unwind(|| write(&file, &save))
                        .unwrap_or_else(|_| Err("設定を保存できません".into()));
                    #[cfg(test)]
                    let _ = xirius_core::io_test::before("prefs_written", &file);
                    for ticket in save.tickets {
                        *ticket.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
                    }
                    let mut s = work.state.lock().unwrap_or_else(|e| e.into_inner());
                    if let Err(e) = result {
                        s.failures.insert(file, e);
                    } else {
                        s.failures.remove(&file);
                        if s.dirty
                            .get(&file)
                            .is_some_and(|(revision, _)| *revision == save.revision)
                        {
                            s.dirty.remove(&file);
                        }
                    }
                    s.active = None;
                    work.wake.notify_all();
                }
            })
            .expect("start preference writer");
        writer
    })
}
/// Latest snapshot per file; accepting a change does not need a read-queue slot.
pub(crate) fn submit(file: &Path, base: Prefs, prefs: Prefs) -> Ticket {
    let ticket = Ticket(Arc::new(Mutex::new(None)));
    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
    let mut s = writer().state.lock().unwrap_or_else(|e| e.into_inner());
    s.next += 1;
    let revision = s.next;
    let edits = s.dirty.entry(file.to_path_buf()).or_default();
    edits.0 = revision;
    edits.1.include(&base, &prefs);
    let intent = edits.1.clone();
    let save = s.queued.entry(file.to_path_buf()).or_insert_with(|| Save {
        intent: intent.clone(),
        revision,
        tickets: Vec::new(),
    });
    save.intent = intent;
    save.revision = revision;
    // Superseded subscribers complete when the newest snapshot has been written.
    save.tickets.retain(|t| Arc::strong_count(&t.0) > 1);
    save.tickets.push(ticket.clone());
    writer().wake.notify_all();
    ticket
}

/// Worker-side ordering barrier: commands observe browser edits already accepted
/// by the UI. It never drains receipts/failures or runs on the window thread.
pub(crate) fn wait_accepted(file: &Path) {
    let work = writer();
    // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only ordering barrier; window callbacks only submit/poll and never enter this wait.
    let mut state = work.state.lock().unwrap_or_else(|e| e.into_inner());
    while state.active.as_deref() == Some(file) || state.queued.contains_key(file) {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: worker-only ordering barrier; window callbacks only submit/poll and never enter this wait.
        state = work.wake.wait(state).unwrap_or_else(|e| e.into_inner());
    }
}
/// Used by the application's asynchronous last-window shutdown, also by its process test.
/// Pending keeps the application alive. Done includes terminal, explicit save failures.
pub(crate) fn finish() -> Option<Vec<String>> {
    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
    let mut s = writer().state.lock().unwrap_or_else(|e| e.into_inner());
    if s.active.is_some() || !s.queued.is_empty() {
        None
    } else {
        Some(s.failures.drain().map(|(_, e)| e).collect())
    }
}

/// A window waits only for its own preference path, and shows a failed save before closing.
pub(crate) fn status(file: &Path) -> Option<Result> {
    // design-lint: allow(ui-blocking-io) PRJ-030/031: memory-only snapshot/queue; producers release this mutex before I/O.
    let mut s = writer().state.lock().unwrap_or_else(|e| e.into_inner());
    if s.active.as_deref() == Some(file) || s.queued.contains_key(file) {
        None
    } else {
        Some(s.failures.remove(file).map_or(Ok(()), Err))
    }
}
impl crate::studio::Studio {
    pub(crate) fn poll_browser_prefs_close(&mut self) -> bool {
        if !self.prefs_closing {
            return false;
        }
        let result = self
            .browser
            .prefs_file
            .as_deref()
            .map_or(Some(Ok(())), status);
        let Some(result) = result else { return false };
        self.prefs_closing = false;
        match result {
            Ok(()) => self.window_ask = Some(crate::context::key_more::WindowAsk::Quit),
            Err(e) => self.error = Some(e),
        }
        true
    }
}
