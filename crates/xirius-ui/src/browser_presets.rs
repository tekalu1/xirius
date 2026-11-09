//! Track-browser presets (BRW-017): the shared index, device-window selection and drops.
//!
//! The song owner reads the shared index through `browser.preset.list`; the legacy
//! device-window list remains readable through `preset.list --values`. Indexed drops
//! use `browser.preset.apply`, including new-track creation, as a single undoable edit.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::TrackKind;
use xirius_params::Kind;

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::browser_inst::{Drop, LibDrag};
use crate::instrument::window::DEFAULT;
use crate::studio::{Geometry, Studio};
use crate::tree::RowKind;

/// A preset being dragged out of the browser.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldPreset {
    /// The device's uid.
    pub device: &'static str,
    pub name: String,
    pub effect: bool,
}

impl Studio {
    pub(crate) fn track_browser_answer(&mut self, name: &str, data: &serde_json::Value) {
        if name == "browser.preset.list"
            && let Ok(entries) = serde_json::from_value::<Vec<xirius_core::track_browser::Entry>>(
                data["entries"].clone(),
            )
        {
            if let Some((uid, name)) = &self.browser.selected_preset
                && let Some(entry) = entries
                    .iter()
                    .find(|e| e.device == *uid && e.name == *name && e.file.is_some())
            {
                self.browser.selected_preset =
                    Some((crate::browser::intern(&entry.id), entry.name.clone()));
                self.browser.reveal = true;
            }
            let folders = entries
                .iter()
                .filter(|e| e.source == "User")
                .flat_map(|e| (1..=e.folder.len()).map(move |n| e.folder[..n].join("/")))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            self.browser.track_library = Some(entries);
            if let Some(save) = self
                .ctx
                .as_mut()
                .and_then(|s| s.ask.as_mut())
                .and_then(|a| a.preset.as_mut())
            {
                save.folders = folders;
                save.folder_page = save
                    .folder_page
                    .min(save.folders.len().saturating_sub(1) / 6 * 6);
                self.refresh_popup();
            }
        }
        if matches!(
            name,
            "browser.preset.save"
                | "browser.preset.copy"
                | "preset.save"
                | "preset.remove"
                | "preset.rename"
                | "preset.factory.restore"
                | "track.preset.save"
                | "track.preset.update"
                | "track.preset.remove"
                | "track.preset.rename"
        ) {
            self.browser.track_library = None;
        }
    }
    /// The browser's rows take the preset files as last read (the device's `Default` is
    /// its knobs' defaults, not a file, and has no row).
    pub(crate) fn sync_browser_presets(&mut self) {
        let mut by_device = std::collections::BTreeMap::<&'static str, Vec<String>>::new();
        for s in self.instruments.presets.saved.iter().flatten() {
            if s.name != DEFAULT {
                by_device.entry(s.device).or_default().push(s.name.clone());
            }
        }
        if self.browser.presets != by_device {
            self.browser.track_library = None;
            self.browser.presets = by_device;
        }
    }

    /// The built-in device `uid` on track `t`: the track's instrument that is it (a multi's
    /// layer too, as `preset.load` finds it), else the last of the track's effects that is
    /// it.
    fn device_on(&self, t: TrackId, uid: &str) -> Option<PluginId> {
        let p = self.project();
        let track = p.tracks.get(&t)?;
        let is = |x: &&xirius_core::model::Plugin| x.builtin().is_some_and(|d| d.uid == uid);
        match xirius_params::find(uid)?.kind {
            Kind::Instrument => track
                .inserts
                .iter()
                .filter(|x| x.kind == xirius_core::model::PluginKind::Instrument)
                .find(is)
                .map(|x| x.id),
            Kind::Effect => track.inserts.iter().rev().find(is).map(|x| x.id),
        }
    }

    /// Loads the preset `name` of the device `uid` on track `t`, putting the device there
    /// with it when the track has none (an instrument in place of the track's), one step.
    /// The track is selected.
    pub fn load_browser_preset(&mut self, t: TrackId, uid: &str, name: &str) -> bool {
        if self
            .browser
            .track_library
            .as_ref()
            .is_some_and(|es| es.iter().any(|e| e.id == uid))
        {
            // design-lint: allow(ui-blocking-io) BRW-017: the guarded dispatcher sends preset I/O to the song owner via io_command::handles.
            return self.run_command(
                "browser.preset.apply",
                json!({"track": t.to_string(), "entry": uid, "target": "track"}),
            );
        }
        let Some(def) = xirius_params::find(uid) else {
            return false;
        };
        if self.device_on(t, uid).is_none()
            && name != DEFAULT
            && self.instruments.presets.saved.is_some()
        {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            if !self.run_command(
                "preset.load",
                json!({ "track": t.to_string(), "preset": name, "device": def.uid }),
            ) {
                return false;
            }
            self.select_row(Some(RowKind::Track(t)));
            // As a device added by hand: an effect's window opens on it.
            if def.kind == Kind::Effect
                && let Some(id) = self.device_on(t, uid)
            {
                self.reveal_last(t, crate::mixer::List::Inserts);
                self.open_when_loaded(t, id);
            }
            return true;
        }
        // Default (the device as it comes) is the device alone; before the files were read
        // once, the factory set's knobs go on after it.
        if self.device_on(t, uid).is_none() {
            let added = match def.kind {
                Kind::Instrument => self.use_on_track(t, def.name),
                Kind::Effect => match self.effect_pick(def.name) {
                    Some(pick) => {
                        let n = self.project().tracks.get(&t).map_or(0, |x| x.inserts.len());
                        self.add_effect(t, n, &pick)
                    }
                    None => false,
                },
            };
            if !added {
                return false;
            }
        }
        let Some(id) = self.device_on(t, uid) else {
            return false;
        };
        self.select_row(Some(RowKind::Track(t)));
        self.load_preset(id, name)
    }

    /// A new instrument track playing the device `uid`, then its preset `name` (two steps).
    pub fn preset_on_new_track(&mut self, uid: &str, name: &str) -> bool {
        if self
            .browser
            .track_library
            .as_ref()
            .is_some_and(|es| es.iter().any(|e| e.id == uid))
        {
            // design-lint: allow(ui-blocking-io) BRW-017: the guarded dispatcher sends preset I/O to the song owner via io_command::handles.
            return self.run_command(
                "browser.preset.apply",
                json!({"entry": uid, "target": "new"}),
            );
        }
        let Some(def) = xirius_params::find(uid) else {
            return false;
        };
        if !self.use_on_new_track(def.name) {
            return false;
        }
        match self.selected_row {
            Some(RowKind::Track(t)) => self.load_browser_preset(t, uid, name),
            _ => false,
        }
    }

    /// Enter and a double click on a preset row: an effect's goes on the chosen track (else
    /// the master), an instrument's on the chosen instrument track (else a new one).
    pub fn load_selected_preset(&mut self) -> bool {
        let Some((uid, name)) = self.browser.selected_preset.clone() else {
            return false;
        };
        let effect = self
            .browser
            .track_library
            .as_ref()
            .and_then(|es| es.iter().find(|e| e.id == uid))
            .map_or_else(
                || xirius_params::find(uid).is_some_and(|d| d.kind == Kind::Effect),
                |e| e.category.tab() == "effects",
            );
        if self.browser.track_library.as_ref().is_some_and(|es| {
            es.iter()
                .any(|e| e.id == uid && e.category == xirius_core::track_browser::Category::Track)
        }) {
            return match self.selected_row {
                Some(RowKind::Track(t)) if t != TrackId::MASTER => {
                    self.load_browser_preset(t, uid, &name)
                }
                _ => self.preset_on_new_track(uid, &name),
            };
        }
        if effect {
            let t = match self.selected_row {
                Some(RowKind::Track(t)) => t,
                _ => TrackId::MASTER,
            };
            return self.load_browser_preset(t, uid, &name);
        }
        match self.selected_track_of(TrackKind::Instrument) {
            Some(t) => self.load_browser_preset(t, uid, &name),
            None => self.preset_on_new_track(uid, &name),
        }
    }

    /// After a press on a preset row: the keys go to it (Enter loads it), a double click
    /// loads it, else it starts a drag.
    pub(crate) fn preset_grab(
        &mut self,
        g: &Geometry,
        i: usize,
        x: f32,
        y: f32,
        clicks: usize,
    ) -> bool {
        let l = self.browser.layout(g.panel);
        let Some(BRow::Preset {
            device,
            name,
            effect,
            ..
        }) = l.rows.get(i).map(|p| p.row.clone())
        else {
            return false;
        };
        let (drag_name, drag_icon) = if let Some(e) = self
            .browser
            .track_library
            .as_ref()
            .and_then(|es| es.iter().find(|e| e.id == device))
        {
            (crate::browser::intern(&e.name), self.browser.entry_icon(e))
        } else if let Some(def) = xirius_params::find(device) {
            (
                def.name,
                crate::browser::LIB
                    .iter()
                    .find(|item| item.name == def.name)
                    .map_or(if effect { "effects" } else { "keys" }, |item| item.icon),
            )
        } else {
            return false;
        };
        self.mixer.browser_fx_focus = true;
        self.browser.typing = false;
        if clicks >= 2 {
            self.browser.drag = None;
            return self.load_selected_preset();
        }
        self.browser.drag = Some(LibDrag {
            name: drag_name,
            icon: drag_icon,
            from: (x, y),
            at: (x, y),
            moved: false,
            preset: Some(HeldPreset {
                device,
                name,
                effect,
            }),
        });
        true
    }

    /// What dropping a preset at a point does: an effect's goes on a track the effects
    /// would (the mixer's strips, the track rows), an instrument's as an instrument would.
    pub fn preset_drop_at(&self, g: &Geometry, x: f32, y: f32, effect: bool) -> Drop {
        if let Some(held) = self.browser.drag.as_ref().and_then(|d| d.preset.as_ref())
            && self
                .browser
                .track_library
                .as_ref()
                .is_some_and(|es| es.iter().any(|e| e.id == held.device))
        {
            let Some(args) = self.track_preset_drop_args(g, x, y, held.device) else {
                return Drop::Nothing;
            };
            return args["track"]
                .as_str()
                .and_then(|t| t.parse().ok())
                .map_or(Drop::New, Drop::Replace);
        }
        if !effect {
            return self.lib_drop_at(g, x, y);
        }
        match self.fx_target(g, x, y) {
            Some((t, _)) => Drop::Replace(t),
            None => Drop::Nothing,
        }
    }

    /// Letting go of a held preset that moved.
    pub(crate) fn preset_drop(&mut self, g: &Geometry, x: f32, y: f32, held: &HeldPreset) {
        if self
            .browser
            .track_library
            .as_ref()
            .is_some_and(|es| es.iter().any(|e| e.id == held.device))
        {
            if let Some(args) = self.track_preset_drop_args(g, x, y, held.device) {
                // design-lint: allow(ui-blocking-io) BRW-017: the guarded dispatcher sends preset I/O to the song owner via io_command::handles.
                self.run_command("browser.preset.apply", args);
            }
            return;
        }
        match self.preset_drop_at(g, x, y, held.effect) {
            Drop::Replace(t) => {
                self.load_browser_preset(t, held.device, &held.name);
            }
            Drop::New => {
                self.preset_on_new_track(held.device, &held.name);
            }
            Drop::Nothing => {}
        }
    }

    fn track_preset_drop_args(
        &self,
        g: &Geometry,
        x: f32,
        y: f32,
        id: &str,
    ) -> Option<serde_json::Value> {
        use crate::mixer::{MixerHit, flow::Pick};
        use xirius_core::track_browser::Category;
        let e = self
            .browser
            .track_library
            .as_ref()?
            .iter()
            .find(|e| e.id == id)?;
        let effect = e.category.tab() == "effects";
        let mut args = json!({"entry": id, "target": "track"});
        if let Some(hit) = self.mixer_hit(g, x, y) {
            let t = hit.track();
            if !effect
                && e.category != Category::Track
                && self.project().tracks.get(&t)?.kind != TrackKind::Instrument
            {
                return None;
            }
            args["track"] = json!(t.to_string());
            match hit {
                MixerHit::Open(_, p) | MixerHit::Bypass(_, p) => {
                    let project = self.project();
                    let plugin = project.tracks.get(&t)?.inserts.iter().find(|x| x.id == p)?;
                    if plugin.kind == xirius_core::model::PluginKind::Instrument {
                        if effect || e.category == Category::Track {
                            return None;
                        }
                        args["target"] = json!("instrument");
                    } else {
                        if !effect {
                            return None;
                        }
                        args["target"] = json!("effect");
                        args["of"] = json!(p.to_string());
                    }
                }
                MixerHit::After(_, pick) => {
                    if !effect {
                        return None;
                    }
                    args["target"] = json!("insert");
                    args["at"] = json!(match pick {
                        Pick::Plugin(p) => format!("after:{p}"),
                        Pick::Split(n) => format!("after:{n}"),
                        Pick::Branch(n) => format!("branch:{n}"),
                    });
                }
                MixerHit::AddInstrument(_) => {
                    if effect || e.category == Category::Track {
                        return None;
                    }
                    args["target"] = json!("instrument");
                }
                MixerHit::AddInsert(_) => {
                    if !effect {
                        return None;
                    }
                    args["target"] = json!("insert");
                }
                MixerHit::Strip(_) => {}
                _ => return None,
            }
            return Some(args);
        }
        if !g.column.contains(x, y) && !g.lanes.contains(x, y) {
            return None;
        }
        if let Some(RowKind::Track(t)) = self
            .tree()
            .row_at(y - g.column.y + self.scroll.1)
            .map(|r| r.kind)
        {
            if !effect
                && e.category != Category::Track
                && self.project().tracks.get(&t)?.kind != TrackKind::Instrument
            {
                return None;
            }
            args["track"] = json!(t.to_string());
        } else {
            args["target"] = json!("new");
        }
        Some(args)
    }

    /// The device window's ブラウザーに表示: the browser opens on the device's tab with the
    /// preset's row chosen and in view (the device's row for `Default` or an unknown one).
    pub fn show_preset_in_browser(&mut self, uid: &str, preset: Option<&str>) -> bool {
        let Some(def) = xirius_params::find(uid) else {
            return false;
        };
        self.refresh_presets();
        let tab = match def.kind {
            Kind::Effect => RailTab::Effects,
            Kind::Instrument => RailTab::Instruments,
        };
        let row = preset
            .filter(|n| *n != DEFAULT)
            .filter(|n| {
                self.browser
                    .presets
                    .get(def.uid)
                    .is_some_and(|all| all.iter().any(|x| x == n))
            })
            .map(|n| {
                let id = self
                    .browser
                    .track_library
                    .as_ref()
                    .and_then(|es| {
                        es.iter()
                            .find(|e| e.device == def.uid && e.name == n && e.file.is_some())
                    })
                    .map_or(def.uid, |e| crate::browser::intern(&e.id));
                (id, n.to_string())
            });
        self.browser.show_device(tab, def.name, row);
        self.mixer.browser_fx_focus = true;
        true
    }

    /// The preset row a press hit, as (device, name).
    pub(crate) fn preset_row(
        &self,
        g: &Geometry,
        hit: BrowserHit,
    ) -> Option<(&'static str, String)> {
        let (BrowserHit::Preset(i) | BrowserHit::Fav(i)) = hit else {
            return None;
        };
        let l = self.browser.layout(g.panel);
        match &l.rows.get(i)?.row {
            BRow::Preset { device, name, .. } => Some((device, name.clone())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
