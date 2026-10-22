//! DAWproject 1.0 (EXP-006, EXP-007): the open exchange format of Bitwig and PreSonus
//! (github.com/bitwig/dawproject, MIT). A `.dawproject` is a ZIP of `project.xml`,
//! `metadata.xml`, the audio the clips play (`audio/…`) and the plugins' states (`plugins/…`).
//!
//! Writing keeps the tracks, folders and buses, the fader, pan, mute, solo and sends, the
//! clips with their notes, controllers and audio, the automation, the tempo and meter and the
//! markers. VST3 and CLAP plugins go with their state; the EQ, Compressor and Limiter are
//! written as DAWproject's own `Equalizer`, `Compressor` and `Limiter`. Reading makes a new
//! song of the same. What either side cannot carry is listed in [`Skips`], never dropped
//! without a word.

mod read;
mod vstpreset;
mod write;
mod xml;
pub mod zip;

#[cfg(test)]
mod reference_tests;
#[cfg(test)]
mod tests;

use std::path::Path;

use crate::model::{Plugin, Project};

pub use read::{Imported, import, import_from, import_with};
pub use write::{Export, export, export_with, skipped_on_export};

/// The extension of the files.
pub const EXTENSION: &str = "dawproject";

/// Whether a path names a `.dawproject` file (by its extension).
pub fn is_dawproject(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Adds to `skips` the VST3 and CLAP plugins of `p` this PC does not have, as `here` tells
/// (`None` when it cannot, as with no scan to go by): 「Pad の Diva（この PC に無い）」. The song
/// keeps them with their state, for a PC that has them.
pub fn not_here(p: &Project, skips: &mut Skips, here: &mut dyn FnMut(&Plugin) -> Option<bool>) {
    for t in p.tracks.values() {
        for x in &t.inserts {
            if matches!(x.format.as_str(), "vst3" | "clap") && here(x) == Some(false) {
                skips.item(format!("{} の {}（この PC に無い）", t.name, x.name));
            }
        }
    }
}

/// What a writer needs to know of an audio file a clip plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub seconds: f64,
}

/// One row of what was left out: a thing named on its own, or a count of things.
#[derive(Clone, Debug, PartialEq)]
pub struct Skip {
    pub label: String,
    pub count: usize,
    pub unit: &'static str,
    counted: bool,
}

impl Skip {
    /// The row as the list shows it: 「Kick の Vintage Comp」, 「ミュートしたノート 4」,
    /// 「Kick のワープ 2 か所」.
    pub fn text(&self) -> String {
        if !self.counted && self.count == 1 {
            return self.label.clone();
        }
        if self.unit.is_empty() {
            format!("{} {}", self.label, self.count)
        } else {
            format!("{} {} {}", self.label, self.count, self.unit)
        }
    }

    /// How many, as the list shows it at the right of the row: 「4」, 「2 か所」; empty for one
    /// thing named by itself.
    pub fn detail(&self) -> String {
        if !self.counted && self.count == 1 {
            String::new()
        } else if self.unit.is_empty() {
            self.count.to_string()
        } else {
            format!("{} {}", self.count, self.unit)
        }
    }
}

/// What a file did not carry over, in the order it was met.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skips {
    rows: Vec<Skip>,
}

impl Skips {
    /// A thing left out, by name. The same name again counts it.
    pub fn item(&mut self, label: impl Into<String>) {
        self.add(label.into(), 1, "", false);
    }

    /// `n` things of a kind left out.
    pub fn count(&mut self, label: impl Into<String>, n: usize, unit: &'static str) {
        if n > 0 {
            self.add(label.into(), n, unit, true);
        }
    }

    fn add(&mut self, label: String, n: usize, unit: &'static str, counted: bool) {
        match self.rows.iter_mut().find(|r| r.label == label) {
            Some(r) => r.count += n,
            None => self.rows.push(Skip {
                label,
                count: n,
                unit,
                counted,
            }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many rows the list has.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn rows(&self) -> &[Skip] {
        &self.rows
    }

    pub fn lines(&self) -> Vec<String> {
        self.rows.iter().map(Skip::text).collect()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!(self.lines())
    }
}
