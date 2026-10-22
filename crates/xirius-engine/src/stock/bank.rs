//! The parameter values a built-in shares with the thread that edits them.
//!
//! Each value is an `f32` (0..1) in an atomic, and a counter goes up on every write, so the
//! audio thread only reads the values again after a change. Nothing here locks or allocates
//! after [`ParamBank::new`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use xirius_params::{BuiltinDef, MAX_PARAMS};

pub struct ParamBank {
    def: &'static BuiltinDef,
    values: [AtomicU32; MAX_PARAMS],
    changes: AtomicU32,
    /// Beats per minute as `f64` bits, for what follows the tempo.
    tempo: AtomicU64,
}

impl ParamBank {
    pub fn new(def: &'static BuiltinDef) -> ParamBank {
        let bank = ParamBank {
            def,
            values: std::array::from_fn(|_| AtomicU32::new(0)),
            changes: AtomicU32::new(0),
            tempo: AtomicU64::new(120f64.to_bits()),
        };
        for (i, p) in def.params.iter().enumerate() {
            bank.values[i].store((p.default_norm() as f32).to_bits(), Ordering::Relaxed);
        }
        bank
    }

    pub fn def(&self) -> &'static BuiltinDef {
        self.def
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
/// running processor without a new mix.
#[derive(Clone)]
pub struct StockHandle {
    bank: Arc<ParamBank>,
}

impl StockHandle {
    pub(crate) fn new(bank: Arc<ParamBank>) -> StockHandle {
        StockHandle { bank }
    }

    pub fn def(&self) -> &'static BuiltinDef {
        self.bank.def()
    }

    /// Sets parameter `id` to `value` (0..1). False when there is no such parameter.
    pub fn set(&self, id: &str, value: f64) -> bool {
        match self.bank.def().param(id) {
            Some((i, _)) => {
                self.bank.set_norm(i, value);
                true
            }
            None => false,
        }
    }

    /// The 0..1 value of parameter `id`.
    pub fn get(&self, id: &str) -> Option<f64> {
        self.bank
            .def()
            .param(id)
            .map(|(i, _)| f64::from(self.bank.norm(i)))
    }

    /// Sets every parameter from a song's values, and the ones it does not name to their
    /// defaults. Returns the keys that are not parameters of this built-in.
    pub fn set_all<'a>(
        &self,
        values: impl IntoIterator<Item = (&'a String, &'a f64)>,
    ) -> Vec<String> {
        let def = self.bank.def();
        let mut given = [None; MAX_PARAMS];
        let mut unknown = Vec::new();
        for (k, v) in values {
            match def.param(k) {
                Some((i, _)) => given[i] = Some(*v),
                None => unknown.push(k.clone()),
            }
        }
        for (i, p) in def.params.iter().enumerate() {
            self.bank
                .set_norm(i, given[i].unwrap_or_else(|| p.default_norm()));
        }
        unknown
    }

    /// The tempo a built-in that follows it plays at (the Delay's note lengths).
    pub fn set_tempo(&self, bpm: f64) {
        self.bank.set_tempo(bpm);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
        let handle = StockHandle::new(bank.clone());
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
        let handle = StockHandle::new(bank.clone());
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
