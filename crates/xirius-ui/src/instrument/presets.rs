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
    pub has_fx: bool,
    pub chain: bool,
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
    pub external: BTreeMap<String, Vec<String>>,
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

    /// The device's preset file named `name`, letter case aside as the session finds it
    /// (L-pre-2): the one of that very name first.
    fn file_of(&self, def: &BuiltinDef, name: &str) -> Option<&Saved> {
        let saved = self.saved.as_ref()?;
        let mine = || saved.iter().filter(|s| s.device == def.uid);
        mine().find(|s| s.name == name).or_else(|| {
            let fold = |s: &str| s.trim().to_lowercase();
            mine().find(|s| fold(&s.name) == fold(name))
        })
    }

    /// The knobs' values (0..1) of the preset `name`, a knob its file leaves out at its
    /// default.
    pub fn values_of(&self, def: &BuiltinDef, name: &str) -> Option<Vec<f64>> {
        if self.saved.is_none() || name == DEFAULT {
            return preset_values(def, name);
        }
        let s = self.file_of(def, name)?;
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
    /// A preset whose file is gone (renamed, removed, replaced by a newer factory set)
    /// keeps its name, marked: nothing holds those knobs any more, and 上書き保存 writes it
    /// again.
    pub fn tag_of(&self, plugin: &Plugin) -> (String, bool) {
        let Some(def) = plugin.builtin() else {
            return (UNKNOWN.into(), false);
        };
        if let Some(name) = plugin.preset.as_deref() {
            let off = match self.values_of(def, name) {
                Some(v) => !at(def, plugin, &v),
                // Until the files were read once, only the factory set is known.
                None => self.saved.is_some(),
            };
            // By the file's own name, so the list's ✓ and ‹ › find its row.
            let name = self
                .file_of(def, name)
                .map_or_else(|| name.to_string(), |s| s.name.clone());
            return (name, off);
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
                    has_fx: r["has_fx"] == true,
                    chain: r["chain"] == true,
                    device: def.uid,
                    name: r["name"].as_str()?.to_string(),
                    params,
                })
            })
            .collect();
        let mut external = BTreeMap::<String, Vec<String>>::new();
        for row in rows {
            let (Some(device), Some(name)) = (row["device"].as_str(), row["name"].as_str()) else {
                continue;
            };
            if xirius_params::find(device).is_none() {
                external
                    .entry(device.to_ascii_lowercase())
                    .or_default()
                    .push(name.to_string());
            }
        }
        let changed = self.saved.as_ref() != Some(&saved) || self.external != external;
        self.saved = Some(saved);
        self.external = external;
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
        if ((self.browser.open && self.browser.is_track_browser())
            || !self.instruments.open.is_empty()
            || !self.mixer.editors.is_empty())
            && self.browser.track_library.is_none()
        {
            self.browser.track_library = Some(Vec::new());
            // design-lint: allow(ui-blocking-io) BRW-017: the file index is read on the song owner through io_command::handles.
            if !self.run_command("browser.preset.list", json!({})) {
                self.browser.track_library = None;
            }
        }
        let listed = self.browser.open
            && matches!(
                self.browser.tab,
                crate::browser::RailTab::Instruments | crate::browser::RailTab::Effects
            );
        if (self.instruments.open.is_empty() && self.mixer.editors.is_empty() && !listed)
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
        if overwrite
            && let Some(e) = self
                .browser
                .track_library
                .as_ref()
                .and_then(|es| {
                    let p = self.project();
                    let x = p.tracks.get(&t)?.inserts.iter().find(|x| x.id == id)?;
                    es.iter()
                        .find(|e| e.source == "User" && e.device == x.uid && e.name == name)
                })
                .cloned()
        {
            // design-lint: allow(ui-blocking-io) BRW-017: the file save is handled by the song owner through io_command::handles.
            return self.run_command("browser.preset.save", json!({"track": t.to_string(), "of": id.to_string(), "kind": xirius_core::xpreset::kind_name(kind), "name": name, "include_fx": e.has_fx, "folder": e.folder.join("/"), "overwrite": true}));
        }
        if overwrite
            && self
                .browser
                .track_library
                .as_ref()
                .is_some_and(|es| es.iter().any(|e| e.source == "Factory" && e.name == name))
        {
            return self.save_preset_as(id);
        }
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

    /// The device's preset named `name`, letter case aside (as the session finds it).
    fn preset_named(&self, id: PluginId, name: &str) -> Option<String> {
        let p = self.project();
        let def = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == id)?
            .builtin()?;
        let fold = |s: &str| s.trim().to_lowercase();
        self.instruments
            .presets
            .names(def)
            .into_iter()
            .find(|n| fold(n) == fold(name))
    }

    /// 名前を付けて保存…: the box for the name under the list's tag. It starts with a name
    /// no preset of the device has, the tag's with a number (empty at Default; the next
    /// number when it ends in one, `Bass 2` → `Bass 3`), so Enter saves a new one.
    pub(crate) fn save_preset_as(&mut self, id: PluginId) -> bool {
        let Some((t, _)) = self.device_place(id) else {
            return false;
        };
        let p = self.project();
        let initial = p
            .tracks
            .get(&t)
            .and_then(|t| t.inserts.iter().find(|x| x.id == id))
            .and_then(|x| self.instruments.presets.origin(x))
            .filter(|n| n != DEFAULT)
            .map(|n| {
                let (base, from) = n
                    .rsplit_once(' ')
                    .and_then(|(b, k)| {
                        Some((b.to_string(), k.parse::<u32>().ok()?.checked_add(1)?))
                    })
                    .unwrap_or_else(|| (n.clone(), 2));
                (from..=u32::MAX)
                    .map(|k| format!("{base} {k}"))
                    .find(|c| self.preset_named(id, c).is_none())
                    .unwrap_or(n)
            })
            .unwrap_or_default();
        let x = p.tracks[&t].inserts.iter().find(|x| x.id == id).unwrap();
        let include_fx = self.instruments.presets.saved.as_ref().is_some_and(|es| {
            es.iter().any(|e| {
                e.device == x.uid && x.preset.as_deref() == Some(e.name.as_str()) && e.has_fx
            })
        }) || self.browser.track_library.as_ref().is_some_and(|es| {
            es.iter().any(|e| {
                e.device == x.uid && x.preset.as_deref() == Some(e.name.as_str()) && e.has_fx
            })
        });
        let fx_names = xirius_core::chain_preset::Chain::capture(
            &p.tracks[&t],
            xirius_core::chain_preset::ChainKind::Instrument,
            Some(xirius_core::id::AnyId::Plugin(id)),
        )
        .ok()
        .map(|(c, _)| {
            c.devices
                .iter()
                .filter(|x| x.kind == PluginKind::Effect)
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>()
                .join(" · ")
        })
        .unwrap_or_default();
        let scope = format!(
            "{} · {}",
            if x.kind == PluginKind::Instrument {
                "インストゥルメント"
            } else {
                "FX"
            },
            x.name
        );
        self.open_track_preset_save("browser.preset.save", json!({"track": t.to_string(), "of": id.to_string(), "kind": xirius_core::xpreset::kind_name(x.kind)}), &initial, &scope, include_fx, fx_names);
        true
    }

    /// The name box of 名前を付けて保存… (`over`: the name it asks again to write over).
    fn ask_save_name(&mut self, label: &str, id: PluginId, over: Option<&str>, initial: &str) {
        let at = self.instruments.menu_at.unwrap_or_default();
        self.ask_name(
            label,
            "ui.preset.save",
            json!({ "id": id.to_string(), "over": over }),
            "name",
            initial,
            (at.x, at.bottom() + 4.0),
        );
    }

    /// Enter in the name box of 名前を付けて保存…: a new name saves; a name the device has
    /// asks once more (上書きして保存…) and Enter on it again writes over that file.
    /// Default is no file: the session refuses it at once, before anything is asked.
    pub(crate) fn save_preset_named(&mut self, id: PluginId, name: &str, over: Option<&str>) {
        let fold = |s: &str| s.trim().to_lowercase();
        if fold(name) == fold(DEFAULT) {
            self.save_preset(id, name, false);
            return;
        }
        match self.preset_named(id, name) {
            Some(had) if over.is_none_or(|o| fold(o) != fold(&had)) => {
                self.ask_save_name("上書きして保存…", id, Some(&had), &had);
            }
            had => {
                self.save_preset(id, name, had.is_some());
            }
        }
    }
}
