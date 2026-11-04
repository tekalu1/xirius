//! The presses on a Sampler's and a Drum Kit's window that are not a parameter (UI-333,
//! UI-334): a zone, a bank, a layer, a pad's M and S, the ‹ › of the sample, Browse; and
//! the files dropped on the window.

use std::path::{Path, PathBuf};

use serde_json::json;
use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_params::{BuiltinDef, PAD_NAMES, PADS, pad_key};

use super::window::{Part, Win, live};
use crate::browser::RailTab;
use crate::context::{Act, Item};
use crate::device::layout::Mark;
use crate::device::{play, sampler};
use crate::studio::{FileRequest, Geometry, Rect, Studio};

/// The file the sample's row of the window `w` shows: the zone chosen, or the chosen
/// pad's layer.
fn shown_file(def: &BuiltinDef, plugin: &Plugin, w: &Win) -> Option<usize> {
    let file = match def.uid {
        "drum-kit" => {
            let files = sampler::pad_files(def, plugin, w.pick.min(PADS - 1));
            files
                .get(w.layer.min(files.len().saturating_sub(1)))
                .copied()
        }
        _ => Some(w.pick.min(plugin.samples.len().saturating_sub(1))),
    };
    file.filter(|&f| f < plugin.samples.len())
}

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
                let Some(file) = shown_file(def, plugin, &w) else {
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
            Mark::Name => self.sample_menu(i, spot.r.x, spot.r.bottom()),
            Mark::Drop | Mark::Title | Mark::Pad(_) => {}
        }
    }

    /// The menu of the sample's name in the `i`th window, at `(x, y)` (UI-333, UI-334): put
    /// another file in its place, show it in the browser; take it away (a Sampler's zone, or
    /// all of them; a Drum Kit's layer, while the pad has another).
    pub(crate) fn sample_menu(&mut self, i: usize, x: f32, y: f32) {
        let p = self.project();
        let w = self.instruments.open[i];
        let Some((_, plugin, def)) = live(&p, &w) else {
            return;
        };
        let Some(file) = shown_file(def, plugin, &w) else {
            return;
        };
        let id = w.plugin;
        let mut items = self.sample_rows(id, plugin, file);
        items.push(Item::gap());
        let one = json!({ "id": id.to_string(), "index": file + 1 });
        if def.uid == "drum-kit" {
            let layers = sampler::pad_files(def, plugin, w.pick.min(PADS - 1)).len();
            let item = self.ctx_as("このレイヤーを外す", "plugin.samples.remove", one);
            items.push(item.enabled(layers > 1));
        } else {
            items.push(self.ctx_as("このゾーンを外す", "plugin.samples.remove", one));
            let all = json!({ "id": id.to_string(), "all": true });
            items.push(self.ctx_as("すべてのサンプルを外す", "plugin.samples.remove", all));
        }
        self.open_context(items, x, y);
    }

    /// 「サンプルを置き換え…」 and 「ブラウザーで表示」 for the file `file` of `plugin`.
    fn sample_rows(&self, id: PluginId, plugin: &Plugin, file: usize) -> Vec<Item> {
        let shown = plugin
            .samples
            .get(file)
            .and_then(|name| self.sample_path(name))
            .is_some_and(|path| self.browser.holds_file(&path));
        vec![
            Item::act("サンプルを置き換え…", "", Act::SampleReplace(id, file)),
            Item::act("ブラウザーで表示", "", Act::SampleShow(id, file)).enabled(shown),
        ]
    }

    /// A right press on the `n`th spot of the `i`th window, whose frame is `r`: a zone's
    /// menu (it is chosen), a layer's (it is not), the sample name's (UI-333, UI-334).
    pub(crate) fn spot_menu(&mut self, i: usize, r: Rect, n: usize, x: f32, y: f32) {
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
            Mark::Zone(file) => {
                self.instruments.pick(id, file, 0);
                let one = json!({ "id": id.to_string(), "index": file + 1 });
                let items = vec![self.ctx_as("このゾーンを外す", "plugin.samples.remove", one)];
                self.open_context(items, x, y);
            }
            Mark::Layer(j) => {
                let files = sampler::pad_files(def, plugin, w.pick.min(PADS - 1));
                let Some(&file) = files.get(j) else {
                    return;
                };
                let one = json!({ "id": id.to_string(), "index": file + 1 });
                let item = self.ctx_as("このレイヤーを外す", "plugin.samples.remove", one);
                self.open_context(vec![item.enabled(files.len() > 1)], x, y);
            }
            Mark::Name => self.sample_menu(i, x, y),
            _ => {}
        }
    }

    /// A right press on the pad `pad` of the `i`th window (a Drum Kit's): it is chosen, and
    /// its menu put another file in place of its sample, add files as its layers, empty it
    /// or its whole bank (UI-334).
    pub(crate) fn pad_menu(&mut self, i: usize, pad: usize, x: f32, y: f32) {
        let p = self.project();
        let w = self.instruments.open[i];
        let Some((_, plugin, def)) = live(&p, &w) else {
            return;
        };
        if def.uid != "drum-kit" {
            return;
        }
        let pad = pad.min(PADS - 1);
        let id = w.plugin;
        self.instruments.pick(id, pad, 0);
        let files = sampler::pad_files(def, plugin, pad);
        let replace = Item::act(
            "サンプルを置き換え…",
            "",
            Act::SampleReplace(id, files.first().copied().unwrap_or(0)),
        );
        let bank = &PAD_NAMES[pad][..1];
        let items = vec![
            replace.enabled(!files.is_empty()),
            Item::act("サンプルを足す…", "", Act::SampleAdd(id, pad)),
            Item::gap(),
            self.ctx_as(
                "パッドを空にする",
                "plugin.samples.remove",
                json!({ "id": id.to_string(), "pad": PAD_NAMES[pad] }),
            ),
            self.ctx_as(
                "バンクを空にする",
                "plugin.samples.remove",
                json!({ "id": id.to_string(), "bank": bank }),
            ),
        ];
        self.open_context(items, x, y);
    }

    /// Where the sample `name` is on the disk: as it is, or under the song's folder.
    fn sample_path(&self, name: &str) -> Option<PathBuf> {
        let path = Path::new(name.trim());
        if path.is_absolute() {
            return Some(path.to_path_buf());
        }
        self.song().dir().map(|dir| dir.join(path))
    }

    /// The sample `file` of `plugin` in the browser's Files tab (UI-333). False when no
    /// tab's folder holds it.
    pub(crate) fn reveal_sample(&mut self, plugin: PluginId, file: usize) -> bool {
        let p = self.project();
        let Some(path) = p
            .tracks
            .values()
            .flat_map(|t| t.inserts.iter())
            .find(|x| x.id == plugin)
            .and_then(|x| x.samples.get(file))
            .and_then(|name| self.sample_path(name))
        else {
            return false;
        };
        if !self.browser.holds_file(&path) {
            return false;
        }
        if !(self.browser.open && self.browser.tab == RailTab::Files) {
            self.browser.press_rail(RailTab::Files);
        }
        self.browser.show_file(&path)
    }

    /// The files the OS's window gave for 「サンプルを置き換え…」 or 「サンプルを足す…」:
    /// the first in place of the sample, or all of them the pad's new layers, in one step.
    pub(crate) fn samples_picked(&mut self, request: FileRequest, paths: &[PathBuf]) {
        let list: Vec<_> = paths
            .iter()
            .map(|p| json!({ "path": p.to_string_lossy() }))
            .collect();
        let args = match request {
            FileRequest::Sample(id, file) => {
                let Some(first) = list.first() else { return };
                json!({ "id": id.to_string(), "path": first["path"], "index": file + 1 })
            }
            FileRequest::PadSamples(id, pad) => {
                if list.is_empty() {
                    return;
                }
                json!({
                    "id": id.to_string(),
                    "paths": list,
                    "pad": PAD_NAMES[pad.min(PADS - 1)],
                    "layers": true,
                })
            }
            _ => return,
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command("plugin.samples.load", args);
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
        let hi = hi.clamp(lo, max);
        // Where it ends already: no step to undo (low 19).
        if sampler::velocities(def, x, &files).get(j).map(|r| r.1) == Some(hi) {
            return false;
        }
        let tops = sampler::tops_with(def, x, &files, j, hi);
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
    /// there), and all of them the layers of one pad while Shift is held or when they fall
    /// on its layers' bar. A Sampler's take the place of its samples, or join them when
    /// they fall on the zones' strip (the mock's targets). False when the drop fell
    /// elsewhere.
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
            _ if kit => Some(w.pick.min(PADS - 1)),
            _ => None,
        };
        let on = self.drop_target(g, x, y).map(|d| d.1);
        let shift = std::mem::take(&mut self.instruments.drop_layers);
        let layers = kit && (shift || on == Some(sampler::DropOn::Layers));
        let replace = on == Some(sampler::DropOn::Samples);
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
        if replace {
            args["replace"] = json!(true);
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if !self.run_command("plugin.samples.load", args) {
            return true;
        }
        if refused.is_some() {
            self.error = refused;
        }
        // The pad the first file went to: the one dropped on (N3-kit-2: the pads from it
        // take the files, the ones that had files too).
        if let Some(pad) = pad {
            self.instruments.pick(w.plugin, pad.min(PADS - 1), 0);
        }
        true
    }
}
