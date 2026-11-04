//! The parameter values a built-in shares with the thread that edits them.
//!
//! Each value is an `f32` (0..1) in an atomic, and a counter goes up on every write, so the
//! audio thread only reads the values again after a change. Nothing here locks or allocates
//! after [`ParamBank::new`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use xirius_params::{BuiltinDef, MAX_PARAMS, Scope};

use super::files::{FileOutbox, FileSet, FileSlot};
use super::probe::Probe;
use crate::rt_lock::Mutex;

pub struct ParamBank {
    def: &'static BuiltinDef,
    values: [AtomicU32; MAX_PARAMS],
    changes: AtomicU32,
    /// Beats per minute as `f64` bits, for what follows the tempo.
    tempo: AtomicU64,
    /// What the processor shows of its sound while watched (ENG-072).
    probe: Arc<Probe>,
}

impl ParamBank {
    pub fn new(def: &'static BuiltinDef) -> ParamBank {
        let bank = ParamBank {
            def,
            values: std::array::from_fn(|_| AtomicU32::new(0)),
            changes: AtomicU32::new(0),
            tempo: AtomicU64::new(120f64.to_bits()),
            probe: Arc::new(Probe::new()),
        };
        for (i, p) in def.params.iter().enumerate() {
            bank.values[i].store((p.default_norm() as f32).to_bits(), Ordering::Relaxed);
        }
        bank
    }

    pub fn def(&self) -> &'static BuiltinDef {
        self.def
    }

    pub fn probe(&self) -> &Arc<Probe> {
        &self.probe
    }

    /// The 0..1 value of parameter `i`.
    pub fn norm(&self, i: usize) -> f32 {
        f32::from_bits(self.values[i].load(Ordering::Relaxed))
    }

    /// The plain value (in the parameter's unit) of parameter `i`.
    pub fn plain(&self, i: usize) -> f64 {
        self.def.params[i].to_plain(f64::from(self.norm(i)))
    }

    pub fn set_norm(&self, i: usize, value: f64) {
        if i < self.def.params.len() {
            let v = value.clamp(0.0, 1.0) as f32;
            self.values[i].store(v.to_bits(), Ordering::Relaxed);
            self.changes.fetch_add(1, Ordering::Release);
        }
    }

    /// Goes up whenever a value or the tempo is written.
    pub fn changes(&self) -> u32 {
        self.changes.load(Ordering::Acquire)
    }

    pub fn tempo(&self) -> f64 {
        f64::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    pub fn set_tempo(&self, bpm: f64) {
        if bpm.is_finite() && bpm > 0.0 && bpm != self.tempo() {
            self.tempo.store(bpm.to_bits(), Ordering::Relaxed);
            self.changes.fetch_add(1, Ordering::Release);
        }
    }
}

/// What the session keeps of a built-in it gave the engine: it moves the parameters of the
/// running processor without a new mix, and hands it the files it reads (ENG-077).
#[derive(Clone)]
pub struct StockHandle {
    bank: Arc<ParamBank>,
    /// The channel a file-taking built-in got; `None` for one that does not read files.
    files: Option<Arc<Mutex<Files>>>,
}

/// What a file-taking built-in's handle keeps to lay its files out again when a file's or a
/// pad's value moves, without reading them again (PLG-067, PLG-068).
struct Files {
    out: FileOutbox,
    slots: Vec<Option<FileSlot>>,
    /// The values kept per file and per pad (0..1), under the keys the song keeps them by.
    scoped: BTreeMap<String, f64>,
    /// The last layout did not get through (the ring was full): the next change or
    /// [`StockHandle::resend_files`] sends it.
    pending: bool,
}

impl Files {
    fn send(&mut self, def: &'static BuiltinDef) -> bool {
        let set = FileSet::layout(def, &self.scoped, self.slots.clone());
        self.pending = !self.out.set(set);
        !self.pending
    }
}

impl StockHandle {
    pub(crate) fn new(bank: Arc<ParamBank>, files: Option<FileOutbox>) -> StockHandle {
        StockHandle {
            bank,
            files: files.map(|out| {
                Arc::new(Mutex::new(Files {
                    out,
                    slots: Vec::new(),
                    scoped: BTreeMap::new(),
                    pending: false,
                }))
            }),
        }
    }

    pub fn def(&self) -> &'static BuiltinDef {
        self.bank.def()
    }

    /// What the running processor shows of its sound while the window watches (ENG-072).
    pub fn probe(&self) -> Arc<Probe> {
        self.bank.probe().clone()
    }

    fn files(&self) -> Option<std::sync::MutexGuard<'_, Files>> {
        self.files
            .as_ref()
            .map(|f| f.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Sets parameter `id` to `value` (0..1): one of the device's, or a file's or a pad's
    /// (`file2.start`, `A1.decay`), which lays the files out again. False when there is no
    /// such parameter.
    pub fn set(&self, id: &str, value: f64) -> bool {
        let def = self.bank.def();
        if let Some((i, _)) = def.param(id) {
            self.bank.set_norm(i, value);
            return true;
        }
        let Some(r) = def.resolve(id) else {
            return false;
        };
        let Some(mut files) = self.files() else {
            return false;
        };
        files.scoped.insert(r.key, value.clamp(0.0, 1.0));
        files.send(def);
        true
    }

    /// The 0..1 value of parameter `id` (a file's or a pad's too).
    pub fn get(&self, id: &str) -> Option<f64> {
        let def = self.bank.def();
        if let Some((i, _)) = def.param(id) {
            return Some(f64::from(self.bank.norm(i)));
        }
        let r = def.resolve(id).filter(|r| r.scope != Scope::Device)?;
        let files = self.files()?;
        if let Scope::File(i) = r.scope
            && r.spec.id == "pad"
        {
            let pad = def.file_pad(&files.scoped, i);
            return Some(r.spec.to_norm(pad as f64));
        }
        Some(
            files
                .scoped
                .get(&r.key)
                .copied()
                .unwrap_or_else(|| r.spec.default_norm()),
        )
    }

    /// Sets every parameter from a song's values, and the ones it does not name to their
    /// defaults; a file's and a pad's values lay the files out again when they moved.
    /// Returns the keys this built-in has no parameter for (the ones it no longer has are
    /// dropped without a word).
    pub fn set_all<'a>(
        &self,
        values: impl IntoIterator<Item = (&'a String, &'a f64)>,
    ) -> Vec<String> {
        let def = self.bank.def();
        let mut given = [None; MAX_PARAMS];
        let mut scoped = BTreeMap::new();
        let mut unknown = Vec::new();
        for (k, v) in values {
            if let Some((i, _)) = def.param(k) {
                given[i] = Some(*v);
            } else if let Some(r) = def.resolve(k).filter(|_| self.files.is_some()) {
                scoped.insert(r.key, v.clamp(0.0, 1.0));
            } else if !def.is_retired(k) {
                unknown.push(k.clone());
            }
        }
        for (i, p) in def.params.iter().enumerate() {
            self.bank
                .set_norm(i, given[i].unwrap_or_else(|| p.default_norm()));
        }
        if let Some(mut files) = self.files()
            && (files.scoped != scoped || files.pending)
        {
            files.scoped = scoped;
            files.send(def);
        }
        unknown
    }

    /// The tempo a built-in that follows it plays at (the Delay's note lengths).
    pub fn set_tempo(&self, bpm: f64) {
        self.bank.set_tempo(bpm);
    }

    /// Whether the built-in reads files (the Sampler and the Drum Kit).
    pub fn takes_files(&self) -> bool {
        self.files.is_some()
    }

    /// Hands the device the newest read files, in the order the song lists them, laid out
    /// with the files' and pads' values; played from its next block. `false` when this
    /// built-in takes no files or the ring is full.
    pub fn set_files(&self, slots: Vec<Option<FileSlot>>) -> bool {
        let def = self.bank.def();
        let Some(mut files) = self.files() else {
            return false;
        };
        files.slots = slots;
        files.send(def)
    }

    /// Sends again the layout a full ring turned away, once the device has polled (L2-smp-4):
    /// without it the last change waits for the next one. `true` when nothing is left
    /// waiting. It frees the sets handed back each time, too: while the way back is full
    /// the device leaves the newest set in the queue, and without a change to free them it
    /// would wait there (the fourth review's low 10).
    pub fn resend_files(&self) -> bool {
        let def = self.bank.def();
        let Some(mut files) = self.files() else {
            return true;
        };
        files.out.reclaim();
        !files.pending || files.send(def)
    }

    /// Frees the sets the audio thread handed back. Called inside `set_files`; on its own
    /// when a device is closed so nothing lingers.
    pub fn reclaim_files(&self) {
        if let Some(mut files) = self.files() {
            files.out.reclaim();
        }
    }
}

/// The audio thread's view of a bank: the plain values as of the last change.
pub struct Params {
    bank: Arc<ParamBank>,
    seen: u32,
    plain: [f64; MAX_PARAMS],
    tempo: f64,
}

impl Params {
    pub fn new(bank: Arc<ParamBank>) -> Params {
        let mut p = Params {
            seen: bank.changes().wrapping_sub(1),
            bank,
            plain: [0.0; MAX_PARAMS],
            tempo: 120.0,
        };
        p.refresh();
        p
    }

    /// Reads the values again when one changed. True when they did.
    pub fn refresh(&mut self) -> bool {
        let now = self.bank.changes();
        if now == self.seen {
            return false;
        }
        self.seen = now;
        for i in 0..self.bank.def().params.len() {
            self.plain[i] = self.bank.plain(i);
        }
        self.tempo = self.bank.tempo();
        true
    }

    pub fn get(&self, i: usize) -> f64 {
        self.plain[i]
    }

    pub fn on(&self, i: usize) -> bool {
        self.plain[i] >= 0.5
    }

    pub fn index(&self, i: usize) -> usize {
        self.plain[i].max(0.0).round() as usize
    }

    pub fn tempo(&self) -> f64 {
        self.tempo
    }

    pub fn probe(&self) -> &Probe {
        self.bank.probe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The fourth review's low 10: the newest set left in the queue while the way back was
    /// full goes in with the next poll of the session, though nothing changes.
    #[test]
    fn a_set_held_by_a_full_way_back_goes_in_with_the_next_resend() {
        use super::super::files::{QUEUE, channel};
        let def = &xirius_params::SAMPLER;
        let (out, mut inbox) = channel();
        let h = StockHandle::new(Arc::new(ParamBank::new(def)), Some(out));
        let set = || FileSet::layout(def, &BTreeMap::new(), Vec::new());
        let mut cur = None;
        // The audio thread takes sets while the session frees none: the way back fills.
        for _ in 0..=QUEUE {
            assert!(h.files().unwrap().out.push_raw(set()));
            inbox.poll(&mut cur);
        }
        assert!(h.files().unwrap().out.push_raw(set()));
        assert!(!inbox.poll(&mut cur), "the newest waits for room");
        assert!(h.resend_files(), "nothing was turned away");
        assert!(inbox.poll(&mut cur), "it went in");
    }

    #[test]
    fn a_new_bank_holds_the_defaults() {
        let bank = Arc::new(ParamBank::new(&xirius_params::COMPRESSOR));
        let p = Params::new(bank);
        assert!((p.get(0) - -18.0).abs() < 1e-4);
        assert!((p.get(1) - 4.0).abs() < 1e-4);
    }

    #[test]
    fn params_see_a_write_once() {
        let bank = Arc::new(ParamBank::new(&xirius_params::EQ));
        let handle = StockHandle::new(bank.clone(), None);
        let mut p = Params::new(bank);
        assert!(!p.refresh());
        assert!(handle.set("output", 1.0));
        assert!(p.refresh());
        assert!((p.get(16) - 24.0).abs() < 1e-4);
        assert!(!p.refresh());
        assert!(!handle.set("nope", 0.5));
    }

    #[test]
    fn set_all_fills_in_defaults_and_reports_unknown_keys() {
        let bank = Arc::new(ParamBank::new(&xirius_params::LIMITER));
        let handle = StockHandle::new(bank.clone(), None);
        handle.set("input", 1.0);
        let mut song = BTreeMap::new();
        song.insert("ceiling".to_string(), 0.0);
        song.insert("gain_db".to_string(), 3.0);
        let unknown = handle.set_all(&song);
        assert_eq!(unknown, ["gain_db"]);
        let p = Params::new(bank);
        assert_eq!(p.get(0), 0.0, "input went back to its default");
        assert_eq!(p.get(1), -24.0);
    }
}
