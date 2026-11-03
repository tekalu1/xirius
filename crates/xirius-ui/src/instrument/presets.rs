//! A built-in device's presets on the screen (UI-332): the list its tag opens, the name the
//! tag shows and whether the knobs are still at it.
//!
//! The list is the device's `Default` and then the preset files the session keeps
//! (PLG-063) by name, the factory ones among them, read with their knobs
//! (`preset.list --values`) when a window or the list opens, after a command and every
//! half second while a window is open. The name is the one the song keeps on the device
//! (`Plugin.preset`, which a load and a save set), so it outlasts a restart and the knobs
//! moving off it; a device that keeps none (a song from before) takes the preset its knobs
//! are at, else [`UNKNOWN`] with the mark. The list's ✓ stays on that row while the knobs
//! are off it.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::id::PluginId;
use xirius_core::model::{Plugin, PluginKind};
use xirius_params::BuiltinDef;

use super::window::{DEFAULT, preset_values, presets, values};
use crate::studio::Studio;

/// The tag of a device whose knobs are at no preset and whose preset is not known.
pub const UNKNOWN: &str = "—";

/// How often the files are read again while a window is open.
const EVERY: Duration = Duration::from_millis(500);

/// A preset file of a built-in device.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    /// The device's uid.
    pub device: &'static str,
    pub name: String,
    /// The knobs it holds (0..1).
    pub params: BTreeMap<String, f64>,
}

/// The preset files as last read.
#[derive(Debug, Default)]
pub struct Presets {
    /// `None` until the files were read once: the factory set stands in.
    pub saved: Option<Vec<Saved>>,
    read_at: Option<Instant>,
}

/// Are the knobs a preset sets of `plugin` at `v`?
fn at(def: &BuiltinDef, plugin: &Plugin, v: &[f64]) -> bool {
    let now = values(plugin);
    def.params
        .iter()
        .zip(v.iter().zip(&now))
        .all(|(s, (a, b))| !s.preset || (a - b).abs() < 1e-6)
}

impl Presets {
    /// The device's list: its `Default`, then its preset files by name.
    pub fn names(&self, def: &BuiltinDef) -> Vec<String> {
        match &self.saved {
            None => presets(def).into_iter().map(str::to_string).collect(),
            Some(saved) => std::iter::once(DEFAULT.to_string())
                .chain(
                    saved
                        .iter()
                        .filter(|s| s.device == def.uid)
                        .map(|s| s.name.clone()),
                )
                .collect(),
        }
    }

    /// The knobs' values (0..1) of the preset `name`, a knob its file leaves out at its
    /// default.
    pub fn values_of(&self, def: &BuiltinDef, name: &str) -> Option<Vec<f64>> {
        let Some(saved) = self.saved.as_ref().filter(|_| name != DEFAULT) else {
            return preset_values(def, name);
        };
        let s = saved
            .iter()
            .find(|s| s.device == def.uid && s.name == name)?;
        Some(
            def.params
                .iter()
                .map(|p| {
                    s.params
                        .get(p.id)
                        .copied()
                        .unwrap_or_else(|| p.default_norm())
                })
                .collect(),
        )
    }

    /// The tag's «name ●»: the preset the device keeps and whether the knobs are off it.
    /// A preset whose file is gone keeps its name, unmarked.
    pub fn tag_of(&self, plugin: &Plugin) -> (String, bool) {
        let Some(def) = plugin.builtin() else {
            return (UNKNOWN.into(), false);
        };
        if let Some(name) = plugin.preset.as_deref() {
            let off = self
                .values_of(def, name)
                .is_some_and(|v| !at(def, plugin, &v));
            return (name.to_string(), off);
        }
        let found = self
            .names(def)
            .into_iter()
            .find(|n| self.values_of(def, n).is_some_and(|v| at(def, plugin, &v)));
        match found {
            Some(n) => (n, false),
            None => (UNKNOWN.into(), true),
        }
    }

    /// The preset the tag names, `None` while it is not known.
    pub fn origin(&self, plugin: &Plugin) -> Option<String> {
        let (name, _) = self.tag_of(plugin);
        (name != UNKNOWN).then_some(name)
    }

    /// Takes a `preset.list --values` answer; true when the list changed.
    fn take(&mut self, data: &Value) -> bool {
        let Some(rows) = data["presets"].as_array() else {
            return false;
        };
        let saved: Vec<Saved> = rows
            .iter()
            .filter(|r| r["default"] != true)
            .filter_map(|r| {
                let def = xirius_params::find(r["device"].as_str()?)?;
                let params = r["params"]
                    .as_object()?
                    .iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
                    .collect();
                Some(Saved {
                    device: def.uid,
                    name: r["name"].as_str()?.to_string(),
                    params,
                })
            })
            .collect();
        let changed = self.saved.as_ref() != Some(&saved);
        self.saved = Some(saved);
        changed
    }
}

impl Studio {
    /// Reads the preset files again; true when the list changed. The window's read is the
    /// session's last finished one (it reads the files off the window's thread).
    pub(crate) fn refresh_presets(&mut self) -> bool {
        self.read_presets(true)
    }

    /// `seed`: a test's folder takes the factory set first (never from the window's tick,
    /// which must not write).
    fn read_presets(&mut self, seed: bool) -> bool {
        self.instruments.presets.read_at = Some(Instant::now());
        // design-lint: allow(ui-blocking-io) UI-332 review: on the window thread the session answers from its cache and reads the files on a background job.
        let Ok(data) = self.song.run("preset.list", json!({ "values": true })) else {
            return false;
        };
        #[cfg(test)]
        let data = if seed { self.seeded(data) } else { data };
        #[cfg(not(test))]
        let _ = seed;
        let changed = self.instruments.presets.take(&data);
        self.sync_browser_presets();
        changed
    }

    /// A test's folder starts empty; the app copies the factory set in when it starts.
    #[cfg(test)]
    fn seeded(&mut self, data: Value) -> Value {
        let Some(dir) = data["dir"].as_str().map(std::path::PathBuf::from) else {
            return data;
        };
        if dir.join(".factory-seeded").exists() {
            return data;
        }
        let _ = xirius_session::xpreset::seed(&dir);
        let again = self.song.run("preset.list", json!({ "values": true }));
        again.unwrap_or(data)
    }

    /// Every half second while a device's window is open or the browser lists the devices
    /// (BRW-017), so a preset saved, renamed or deleted elsewhere (the CLI) shows; true when
    /// the list changed.
    pub(crate) fn poll_presets(&mut self) -> bool {
        let listed = self.browser.open
            && matches!(
                self.browser.tab,
                crate::browser::RailTab::Instruments | crate::browser::RailTab::Effects
            );
        if (self.instruments.open.is_empty() && !listed)
            || self
                .instruments
                .presets
                .read_at
                .is_some_and(|t| t.elapsed() < EVERY)
        {
            return false;
        }
        self.read_presets(false)
    }

    /// The track and the kind of the built-in device `id`.
    fn device_place(&self, id: PluginId) -> Option<(xirius_core::id::TrackId, PluginKind)> {
        let p = self.project();
        p.tracks.values().find_map(|t| {
            t.inserts
                .iter()
                .find(|x| x.id == id && x.builtin().is_some())
                .map(|x| (t.id, x.kind))
        })
    }

    /// Saves the device `id` as the preset `name` (`overwrite`: over its file); the device
    /// then keeps that name.
    pub fn save_preset(&mut self, id: PluginId, name: &str, overwrite: bool) -> bool {
        let Some((t, kind)) = self.device_place(id) else {
            return false;
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "preset.save",
            json!({
                "track": t.to_string(),
                "kind": xirius_core::xpreset::kind_name(kind),
                "of": id.to_string(),
                "name": name,
                "overwrite": overwrite,
            }),
        )
    }

    /// 名前を付けて保存…: the box for the name under the list's tag.
    pub(crate) fn save_preset_as(&mut self, id: PluginId) -> bool {
        let Some((t, kind)) = self.device_place(id) else {
            return false;
        };
        let p = self.project();
        let initial = p
            .tracks
            .get(&t)
            .and_then(|t| t.inserts.iter().find(|x| x.id == id))
            .and_then(|x| self.instruments.presets.origin(x))
            .filter(|n| n != DEFAULT)
            .unwrap_or_default();
        let at = self.instruments.menu_at.unwrap_or_default();
        self.ask_name(
            "名前を付けて保存…",
            "preset.save",
            json!({
                "track": t.to_string(),
                "kind": xirius_core::xpreset::kind_name(kind),
                "of": id.to_string(),
            }),
            "name",
            &initial,
            (at.x, at.bottom() + 4.0),
        );
        true
    }
}
