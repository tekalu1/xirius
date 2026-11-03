//! The presses on a Sampler's and a Drum Kit's window that are not a parameter (UI-333,
//! UI-334): a zone, a bank, a layer, a pad's M and S, the ‹ › of the sample, Browse; and
//! the files dropped on the window.

use std::path::PathBuf;

use serde_json::json;
use xirius_params::{PAD_NAMES, PADS, pad_key};

use super::window::{Part, live};
use crate::browser::RailTab;
use crate::device::layout::Mark;
use crate::device::{play, sampler};
use crate::studio::{Geometry, Rect, Studio};

impl Studio {
    /// A press on the `n`th spot of the `i`th window, whose frame is `r`.
    pub(crate) fn window_spot(&mut self, i: usize, r: Rect, n: usize) {
        let p = self.project();
        let w = self.instruments.open[i];
        let Some((_, plugin, def)) = live(&p, &w) else {
            return;
        };
        let Some(spot) = self.instruments.body(r, plugin, def).spots.get(n).cloned() else {
            return;
        };
        let id = w.plugin;
        match spot.mark {
            Mark::Prev | Mark::Next => {
                let file = match def.uid {
                    "drum-kit" => {
                        let files = sampler::pad_files(def, plugin, w.pick.min(PADS - 1));
                        files
                            .get(w.layer.min(files.len().saturating_sub(1)))
                            .copied()
                    }
                    _ => Some(w.pick.min(plugin.samples.len().saturating_sub(1))),
                };
                let Some(file) = file.filter(|&f| f < plugin.samples.len()) else {
                    return;
                };
                let by = if spot.mark == Mark::Prev { -1 } else { 1 };
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "plugin.samples.step",
                    json!({ "id": id.to_string(), "index": file + 1, "by": by }),
                );
            }
            Mark::Zone(file) => {
                self.instruments.pick(id, file, 0);
            }
            Mark::Bank(b) => {
                let pad = b * play::PADS + w.pick % play::PADS;
                self.instruments.pick(id, pad.min(PADS - 1), 0);
            }
            Mark::Layer(j) => {
                self.instruments.pick(id, w.pick, j);
            }
            Mark::Mute(pad) | Mark::Solo(pad) => {
                let key = pad_key(
                    pad,
                    if matches!(spot.mark, Mark::Mute(_)) {
                        "mute"
                    } else {
                        "solo"
                    },
                );
                let value = if spot.on { 0.0 } else { 1.0 };
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "plugin.param.set",
                    json!({ "id": id.to_string(), "param": key, "value": value }),
                );
            }
            Mark::Browse => {
                if !(self.browser.open && self.browser.tab == RailTab::Files) {
                    self.browser.press_rail(RailTab::Files);
                }
            }
            Mark::Name | Mark::Drop | Mark::Title | Mark::Pad(_) => {}
        }
    }

    /// Files dropped on a Sampler's or a Drum Kit's window: they are its samples, a
    /// Drum Kit's from the pad they fall on (else the empty pads). False when the drop
    /// fell elsewhere.
    pub(crate) fn window_drop(&mut self, g: &Geometry, x: f32, y: f32, paths: &[PathBuf]) -> bool {
        let p = self.project();
        let Some((i, part)) = self.instruments.at(&p, g, x, y) else {
            return false;
        };
        let w = self.instruments.open[i];
        let Some((_, _, def)) = live(&p, &w) else {
            return false;
        };
        if !sampler::has_files(def) {
            return false;
        }
        let pad = match part {
            Part::Pad(pitch) if def.uid == "drum-kit" => Some(play::pad_of(pitch)),
            _ => None,
        };
        // All of them in one load: one step, and the layers of a pad stay together.
        let list: Vec<_> = paths
            .iter()
            .map(|p| json!({ "path": p.to_string_lossy() }))
            .collect();
        let mut args = json!({ "id": w.plugin.to_string(), "paths": list });
        if let Some(pad) = pad {
            args["pad"] = json!(PAD_NAMES[pad]);
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if !self.run_command("plugin.samples.load", args) {
            return true;
        }
        if let Some(pad) = pad {
            self.instruments.pick(w.plugin, pad, 0);
        }
        true
    }
}
