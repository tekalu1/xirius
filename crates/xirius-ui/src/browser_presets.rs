//! The presets of the built-in devices in the browser (BRW-017): the rows under a device,
//! loading one by a drop on a track, Enter or a double click, and the device window's
//! ブラウザーに表示.
//!
//! The rows come from the same read as the window's list (`preset.list --values`, read again
//! when the files change), so the list and the windows never disagree. Loading puts the
//! preset on the track's device of that kind (the last one of an effect); on a track
//! without one, the device goes in with the preset as one step (`preset.load` with
//! `device`). On no track at all it is two: the new track, then the preset.

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
    /// The browser's rows take the preset files as last read (the device's `Default` is
    /// its knobs' defaults, not a file, and has no row).
    pub(crate) fn sync_browser_presets(&mut self) {
        let mut by_device = std::collections::BTreeMap::<&'static str, Vec<String>>::new();
        for s in self.instruments.presets.saved.iter().flatten() {
            if s.name != DEFAULT {
                by_device.entry(s.device).or_default().push(s.name.clone());
            }
        }
        self.browser.presets = by_device;
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
        let effect = xirius_params::find(uid).is_some_and(|d| d.kind == Kind::Effect);
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
        let Some(def) = xirius_params::find(device) else {
            return false;
        };
        self.mixer.browser_fx_focus = true;
        self.browser.typing = false;
        if clicks >= 2 {
            self.browser.drag = None;
            return self.load_selected_preset();
        }
        self.browser.drag = Some(LibDrag {
            name: def.name,
            icon: if effect { "effects" } else { "keys" },
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
            .map(|n| (def.uid, n.to_string()));
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
