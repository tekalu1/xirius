//! How much of its time each plugin takes (ENG-008): the rows `plugin.stats` and the CPU list
//! show, and the mark of a heavy one.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{PluginKind, Project};
use xirius_engine::timing::{Reading, Timings};

/// A plugin lights the heavy mark at this share of the time its audio lasts…
pub const HEAVY_ON: f64 = 0.15;
/// …and keeps it until it falls below this one.
pub const HEAVY_OFF: f64 = 0.12;

/// What a plugin is doing in the song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Running,
    Bypassed,
    Deactivated,
    /// The track is frozen: the plugin is not loaded.
    Frozen,
}

impl Run {
    pub fn name(self) -> &'static str {
        match self {
            Run::Running => "running",
            Run::Bypassed => "bypass",
            Run::Deactivated => "deactivated",
            Run::Frozen => "frozen",
        }
    }
}

/// One plugin of the song with its reading.
#[derive(Clone, Debug)]
pub struct PluginLoad {
    pub id: PluginId,
    pub name: String,
    pub instrument: bool,
    pub track: TrackId,
    pub track_name: String,
    pub run: Run,
    pub reading: Reading,
}

/// Every plugin of `p` (of `only`'s track, when given), the heaviest first. A plugin that is
/// not running has no reading and sorts after those that are.
pub fn loads(p: &Project, timings: &Timings, only: Option<TrackId>) -> Vec<PluginLoad> {
    let mut out = Vec::new();
    for (tid, t) in &p.tracks {
        if only.is_some_and(|o| o != *tid) {
            continue;
        }
        for x in &t.inserts {
            let run = if t.frozen.is_some() {
                Run::Frozen
            } else if x.deactivated {
                Run::Deactivated
            } else if x.bypass {
                Run::Bypassed
            } else {
                Run::Running
            };
            let reading = if run == Run::Running {
                timings.reading(x.id.0).unwrap_or_default()
            } else {
                Reading::default()
            };
            out.push(PluginLoad {
                id: x.id,
                name: x.name.clone(),
                instrument: x.kind == PluginKind::Instrument,
                track: *tid,
                track_name: t.name.clone(),
                run,
                reading,
            });
        }
    }
    out.sort_by(|a, b| {
        b.reading
            .load
            .partial_cmp(&a.reading.load)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.track_name.cmp(&b.track_name))
    });
    out
}

/// The heavy marks as they are now: a mark lights above [`HEAVY_ON`] and goes out below
/// [`HEAVY_OFF`], so a plugin near the line does not flicker.
#[derive(Clone, Debug, Default)]
pub struct Heavy {
    on: BTreeSet<PluginId>,
}

impl Heavy {
    pub fn update(&mut self, rows: &[PluginLoad]) {
        let before = std::mem::take(&mut self.on);
        for r in rows {
            if r.run != Run::Running || r.reading.blocks == 0 {
                continue;
            }
            let lit = before.contains(&r.id);
            if r.reading.load >= HEAVY_ON || (lit && r.reading.load >= HEAVY_OFF) {
                self.on.insert(r.id);
            }
        }
    }

    pub fn is_heavy(&self, id: PluginId) -> bool {
        self.on.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.on.len()
    }

    pub fn is_empty(&self) -> bool {
        self.on.is_empty()
    }
}

/// The rows of `plugin.stats`.
pub fn rows_json(rows: &[PluginLoad], crashed: &BTreeSet<PluginId>) -> Vec<Value> {
    rows.iter()
        .map(|r| {
            json!({
                "id": r.id.to_string(),
                "name": r.name,
                "track": r.track.to_string(),
                "track_name": r.track_name,
                "instrument": r.instrument,
                "state": r.run.name(),
                "crashed": crashed.contains(&r.id),
                "blocks": r.reading.blocks,
                "load": round(r.reading.load, 4),
                "mean_us": round(r.reading.mean_us, 1),
                "max_us": round(r.reading.max_us, 1),
                "peak_us": round(r.reading.peak_us, 1),
                "max_load": round(r.reading.max_load, 4),
                "measured_in": if r.reading.child { "plugin-host" } else { "engine" },
            })
        })
        .collect()
}

fn round(v: f64, digits: i32) -> f64 {
    let k = 10f64.powi(digits);
    (v * k).round() / k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, load: f64) -> PluginLoad {
        PluginLoad {
            id: PluginId(id),
            name: format!("p{id}"),
            instrument: false,
            track: TrackId(1),
            track_name: "t".into(),
            run: Run::Running,
            reading: Reading {
                blocks: 10,
                load,
                ..Reading::default()
            },
        }
    }

    #[test]
    fn the_mark_lights_at_fifteen_and_goes_out_below_twelve() {
        let mut h = Heavy::default();
        h.update(&[row(1, 0.14)]);
        assert!(!h.is_heavy(PluginId(1)));
        h.update(&[row(1, 0.15)]);
        assert!(h.is_heavy(PluginId(1)));
        // Between the two it keeps what it was.
        h.update(&[row(1, 0.13)]);
        assert!(h.is_heavy(PluginId(1)));
        h.update(&[row(1, 0.119)]);
        assert!(!h.is_heavy(PluginId(1)));
        // And a plugin that was never lit does not light in between.
        h.update(&[row(1, 0.13)]);
        assert!(!h.is_heavy(PluginId(1)));
    }

    #[test]
    fn a_plugin_that_does_not_run_or_has_no_reading_is_not_heavy() {
        let mut h = Heavy::default();
        let mut off = row(1, 0.9);
        off.run = Run::Deactivated;
        let mut none = row(2, 0.9);
        none.reading.blocks = 0;
        h.update(&[off, none]);
        assert!(h.is_empty());
    }
}
