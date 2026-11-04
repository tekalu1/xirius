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
    /// A press on the `n`th spot of the `i`th window, whose frame is `r`, at `x`.
    pub(crate) fn window_spot(&mut self, i: usize, r: Rect, n: usize, x: f32) {
        let p = self.project();
        let w = self.instruments.open[i];
        let Some((_, plugin, def)) = live(&p, &w) else {
            return;
        };
        let body = self.instruments.body(r, plugin, def);
        let Some(spot) = body.spots.get(n).cloned() else {
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
            Mark::Edge(j) => {
                // The strip the layers share: all 128 velocities across it.
                let (lo, hi) = body
                    .spots
                    .iter()
                    .filter(|s| matches!(s.mark, Mark::Layer(_)))
                    .fold((f32::MAX, f32::MIN), |(a, b), s| {
                        (a.min(s.r.x), b.max(s.r.right()))
                    });
                let files = sampler::pad_files(def, plugin, w.pick.min(PADS - 1));
                let range = sampler::edge_range(def, plugin, &files, j);
                let from = sampler::velocities(def, plugin, &files)[j].1;
                self.instruments.edge = Some((id, j));
                self.instruments.grab = Some(super::window::Grab::Edge {
                    plugin: id,
                    j,
                    from,
                    now: from,
                    range,
                    x,
                    w: hi - lo + 2.0,
                });
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

    /// The `j`th velocity layer of the chosen pad of `plugin`'s window ends at `hi`: one
    /// step, the pad's tops all set so they rise (UI-334).
    pub(crate) fn layer_edge_set(
        &mut self,
        plugin: xirius_core::id::PluginId,
        j: usize,
        hi: u8,
    ) -> bool {
        let p = self.project();
        let Some(w) = self.instruments.win_of(plugin).copied() else {
            return false;
        };
        let Some((_, x, def)) = live(&p, &w) else {
            return false;
        };
        let files = sampler::pad_files(def, x, w.pick.min(PADS - 1));
        if j + 1 >= files.len() {
            return false;
        }
        let (lo, max) = sampler::edge_range(def, x, &files, j);
        let tops = sampler::tops_with(def, x, &files, j, hi.clamp(lo, max));
        let Some(((first, value), rest)) = tops.split_first() else {
            return false;
        };
        let also: Vec<_> = rest
            .iter()
            .map(|(k, v)| json!({ "param": k, "value": v }))
            .collect();
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "plugin.param.set",
            json!({ "id": plugin.to_string(), "param": first, "value": value, "also": also }),
        )
    }

    /// ← and → on the edge pressed last (UI-334): four velocities a press, one with Shift.
    pub(crate) fn layer_edge_key(&mut self, key: &str, shift: bool) -> bool {
        let Some((plugin, j)) = self.instruments.edge else {
            return false;
        };
        let by: i16 = match key {
            "left" => -1,
            "right" => 1,
            "escape" => {
                self.instruments.edge = None;
                return true;
            }
            _ => return false,
        };
        let p = self.project();
        let Some(w) = self.instruments.win_of(plugin).copied() else {
            self.instruments.edge = None;
            return false;
        };
        let Some((_, x, def)) = live(&p, &w) else {
            self.instruments.edge = None;
            return false;
        };
        let files = sampler::pad_files(def, x, w.pick.min(PADS - 1));
        let Some(&(_, hi)) = sampler::velocities(def, x, &files).get(j) else {
            self.instruments.edge = None;
            return false;
        };
        let (lo, max) = sampler::edge_range(def, x, &files, j);
        let to = (i16::from(hi) + by * if shift { 1 } else { 4 })
            .clamp(i16::from(lo), i16::from(max)) as u8;
        if to != hi {
            self.layer_edge_set(plugin, j, to);
        }
        true
    }

    /// Files dropped on a Sampler's or a Drum Kit's window: they are its samples, a
    /// Drum Kit's from the pad they fall on, else from the pad chosen (the empty pads from
    /// there), and all of them the layers of one pad while Shift is held. False when the
    /// drop fell elsewhere.
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
        let kit = def.uid == "drum-kit";
        let pad = match part {
            Part::Pad(pitch) if kit => Some(play::pad_of(pitch)),
            _ if kit => Some(w.pick),
            _ => None,
        };
        let layers = kit && std::mem::take(&mut self.instruments.drop_layers);
        let first = live(&p, &w).map_or(0, |(_, x, _)| x.samples.len());
        // A file that is not a sound is passed over and said so, as on the arrangement
        // (N-smp-5); the others go in.
        let (paths, other): (Vec<&PathBuf>, Vec<&PathBuf>) = paths
            .iter()
            // design-lint: allow(ui-blocking-io) PLG-068 review: one metadata read per path the OS has just dropped, once, to keep folders (the session walks them on its job).
            .partition(|p| p.is_dir() || crate::drop_audio::is_audio(p));
        let refused = (!other.is_empty()).then(|| {
            let names: Vec<_> = other
                .iter()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                .collect();
            format!(
                "「{}」は読み込めません（音声ファイルに対応しています）",
                names.join("」「")
            )
        });
        if paths.is_empty() {
            self.error = refused;
            return true;
        }
        // All of them in one load: one step, and the layers of a pad stay together.
        let list: Vec<_> = paths
            .iter()
            .map(|p| json!({ "path": p.to_string_lossy() }))
            .collect();
        let mut args = json!({ "id": w.plugin.to_string(), "paths": list });
        if let Some(pad) = pad {
            args["pad"] = json!(PAD_NAMES[pad]);
        }
        if layers {
            args["layers"] = json!(true);
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if !self.run_command("plugin.samples.load", args) {
            return true;
        }
        if refused.is_some() {
            self.error = refused;
        }
        // The pad the first file went to: the one dropped on, or the next empty one.
        let p = self.project();
        if kit
            && let Some((_, x, _)) = live(&p, &w)
            && let Some(def) = x.builtin()
            && first < x.samples.len()
        {
            self.instruments
                .pick(w.plugin, def.file_pad(&x.params, first), 0);
        }
        true
    }
}
