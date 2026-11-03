//! How a device's controls move (UI-324). A drag, the wheel, the keys and a
//! controller's knob change only what is heard and shown while they go on; the
//! song takes the value once (`plugin.param.set`, one step to undo) when the
//! drag is let go, or once the wheel, the keys or the knob have been still a
//! moment. A typed value, Alt+press, Delete, a press on a segment or a button
//! go into the song at once.

use std::time::Instant;

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{PluginId, TrackId};
use xirius_params::{BuiltinDef, ParamSpec};
use xirius_prefs::midimap::Target;

use super::control::{self, Look, Step};
use super::graph::{self, Point};
use super::layout::{self, Slot};
use crate::context::Item;
use crate::instrument::window::{Grab, Part, Win, live, values};
use crate::settings::FieldId;
use crate::settings::field::Field;
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;

/// A value being typed in a knob: it opens with the value it has, and the
/// first letter typed takes the place of all of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub plugin: PluginId,
    pub k: usize,
    pub field: Field,
    /// Nothing typed yet.
    pub fresh: bool,
}

impl Entry {
    /// What is typed, the old value going at the first letter.
    pub fn type_text(&mut self, range: Option<std::ops::Range<usize>>, text: &str, marked: bool) {
        if std::mem::take(&mut self.fresh) {
            self.field.set(String::new());
        }
        if marked {
            self.field.mark_text(range, text);
        } else {
            self.field.replace_text(range, text);
        }
    }
}

/// A control the wheel, the keys or a controller's knob turns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    pub track: TrackId,
    pub plugin: PluginId,
    pub k: usize,
    pub value: f64,
    /// When it last moved.
    pub at: Instant,
}

impl Studio {
    /// The built-in device `plugin`: its part, what it is and its values.
    fn device(&self, plugin: PluginId) -> Option<(TrackId, &'static BuiltinDef, Vec<f64>)> {
        let p = self.project();
        p.tracks.iter().find_map(|(t, track)| {
            let x = track.inserts.iter().find(|x| x.id == plugin)?;
            Some((*t, x.builtin()?, values(x)))
        })
    }

    /// The value the control `k` of `plugin` shows: while it turns, where it is.
    pub(crate) fn device_value(&self, plugin: PluginId, k: usize) -> Option<f64> {
        self.instruments
            .turning(plugin, k)
            .or_else(|| self.device(plugin)?.2.get(k).copied())
    }

    /// The control `k` of `plugin` goes to `value` in the song (one step);
    /// false when it is there already.
    pub(crate) fn device_set(&mut self, plugin: PluginId, k: usize, value: f64) -> bool {
        let Some((_, def, now)) = self.device(plugin) else {
            return false;
        };
        let (Some(spec), Some(before)) = (def.params.get(k), now.get(k)) else {
            return false;
        };
        if (before - value).abs() <= 1e-9 {
            return false;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "plugin.param.set",
            json!({ "id": plugin.to_string(), "param": spec.id, "value": value }),
        );
        true
    }

    /// The controls `set` (parameter, value) of `plugin` go into the song as
    /// one step (a graph's point moves two); false when they are there
    /// already.
    pub(crate) fn device_set_all(&mut self, plugin: PluginId, set: &[(usize, f64)]) -> bool {
        let Some((_, def, now)) = self.device(plugin) else {
            return false;
        };
        let moved: Vec<(&str, f64)> = set
            .iter()
            .filter(|(k, v)| now.get(*k).is_some_and(|b| (b - v).abs() > 1e-9))
            .filter_map(|&(k, v)| Some((def.params.get(k)?.id, v)))
            .collect();
        let Some(((first, value), rest)) = moved.split_first() else {
            return false;
        };
        let also: Vec<_> = rest
            .iter()
            .map(|(id, v)| json!({ "param": id, "value": v }))
            .collect();
        let mut args = json!({ "id": plugin.to_string(), "param": first, "value": value });
        if !also.is_empty() {
            args["also"] = json!(also);
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command("plugin.param.set", args);
        true
    }

    /// The player hears `value` for the parameter `id` while the song keeps
    /// the old one (a drag, a sweep).
    pub(crate) fn device_hear(&mut self, track: TrackId, plugin: PluginId, id: &str, value: f64) {
        if !self.transport.is_live() {
            return;
        }
        let mut p = (*self.project()).clone();
        let Some(x) = p.tracks.get_mut(&track).and_then(|t| {
            std::sync::Arc::make_mut(t)
                .inserts
                .iter_mut()
                .find(|x| x.id == plugin)
        }) else {
            return;
        };
        x.params.insert(id.to_string(), value);
        let heard = self.auto_heard(&p);
        self.transport.sync(&heard, self.song().dir());
    }

    /// The control under a press loses the keys: a typed value goes in, a
    /// sweep ends.
    pub(crate) fn device_blur(&mut self) {
        self.device_commit();
        self.device_sweep_end();
        self.instruments.focus = None;
    }

    /// What is typed goes into the song when it reads as a value; else the
    /// knob keeps the one it had.
    fn device_commit(&mut self) -> bool {
        let Some(e) = self.instruments.entry.take() else {
            return false;
        };
        let Some((_, def, _)) = self.device(e.plugin) else {
            return false;
        };
        let Some(spec) = def.params.get(e.k) else {
            return false;
        };
        match spec.typed(&e.field.text()) {
            Some(plain) => {
                let v = control::snap(spec, spec.to_norm(plain));
                self.device_set(e.plugin, e.k, v)
            }
            None => false,
        }
    }

    /// A press on a control (`slot` of the window `w`): it takes the keys;
    /// Alt puts it back to its default, a button flips, a segment is chosen,
    /// a knob is taken hold of (typed in on a double click).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn device_press(
        &mut self,
        w: Win,
        def: &'static BuiltinDef,
        slot: Slot,
        x: f32,
        y: f32,
        clicks: u32,
        mods: Mods,
    ) {
        let k = slot.k;
        if self
            .instruments
            .entry
            .as_ref()
            .is_some_and(|e| (e.plugin, e.k) != (w.plugin, k))
        {
            self.device_commit();
        }
        self.device_sweep_end();
        self.instruments.focus = Some((w.plugin, k));
        let Some(spec) = def.params.get(k) else {
            return;
        };
        if self.param_locked(w.track, w.plugin, spec.id) {
            return;
        }
        let Some(now) = self.device_value(w.plugin, k) else {
            return;
        };
        if mods.alt {
            self.device_set(w.plugin, k, control::snap(spec, spec.default_norm()));
            return;
        }
        match slot.look {
            Look::Toggle | Look::Dot => {
                self.device_set(w.plugin, k, 1.0 - now.round());
            }
            Look::Menu => {
                let hit = slot.hit(def);
                self.device_menu(w, def, k, now, hit.x, hit.bottom() + 4.0);
            }
            Look::Segments => {
                if let Some(v) = control::segment_value(slot.hit(def), spec, x) {
                    self.device_set(w.plugin, k, v);
                }
            }
            Look::Knob if clicks >= 2 => self.device_type(w.plugin, k, spec, now),
            Look::Knob => {
                self.instruments.grab = Some(Grab::Knob {
                    plugin: w.plugin,
                    k,
                    start: now,
                    y,
                    value: now,
                });
            }
        }
    }

    /// A press on a graph's point (`at`: its graph and its index): it is
    /// chosen and takes the keys; Alt puts its y and its wheel's values back
    /// to their defaults, a double click types its x's, else it is taken
    /// hold of.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn device_point_press(
        &mut self,
        w: Win,
        def: &'static BuiltinDef,
        at: (usize, usize),
        point: &Point,
        pointer: (f32, f32),
        clicks: u32,
        mods: Mods,
    ) {
        self.device_commit();
        self.device_sweep_end();
        self.instruments.chosen.insert(w.plugin, at);
        let first = point.params().next();
        self.instruments.focus = first.map(|k| (w.plugin, k));
        let free = |s: &Studio, k: usize| {
            def.params
                .get(k)
                .is_some_and(|spec| !s.param_locked(w.track, w.plugin, spec.id))
        };
        if mods.alt {
            let back: Vec<(usize, f64)> = [point.y.map(|d| d.k), point.wheel]
                .into_iter()
                .flatten()
                .filter(|&k| free(self, k))
                .map(|k| {
                    (
                        k,
                        control::snap(&def.params[k], def.params[k].default_norm()),
                    )
                })
                .collect();
            self.device_set_all(w.plugin, &back);
            return;
        }
        if clicks >= 2 {
            if let Some(k) = point.x.map(|d| d.k).filter(|&k| free(self, k))
                && let Some(now) = self.device_value(w.plugin, k)
            {
                self.device_type(w.plugin, k, &def.params[k], now);
            }
            return;
        }
        let side = |s: &Studio, d: Option<graph::Drive>| {
            let k = d.map(|d| d.k).filter(|&k| free(s, k))?;
            let v = s.device_value(w.plugin, k)?;
            Some((k, v, v))
        };
        let (x, y) = (side(self, point.x), side(self, point.y));
        if x.is_some() || y.is_some() {
            self.instruments.grab = Some(Grab::Point {
                plugin: w.plugin,
                graph: at.0,
                i: at.1,
                pointer,
                x,
                y,
            });
        }
    }

    /// The pointer moves a held point: its x and y follow it (a quarter as
    /// far with Shift); only the sound and the screen follow until it is let
    /// go.
    pub(crate) fn device_point_moved(
        &mut self,
        g: &Geometry,
        px: f32,
        py: f32,
        fine: bool,
    ) -> bool {
        let Some(Grab::Point {
            plugin,
            graph: n,
            i,
            pointer,
            x,
            y,
        }) = self.instruments.grab
        else {
            return false;
        };
        let p = self.project();
        let Some(at) = self
            .instruments
            .open
            .iter()
            .position(|w| w.plugin == plugin)
        else {
            return false;
        };
        let w = self.instruments.open[at];
        let Some((_, plugin_now, def)) = live(&p, &w) else {
            return false;
        };
        let r = self.instruments.rect(at, g.size);
        let Some(point) = self
            .instruments
            .points_of(r, plugin_now, def, n)
            .get(i)
            .cloned()
        else {
            return false;
        };
        let scale = if fine { graph::FINE } else { 1.0 };
        let step = |side: Option<(usize, f64, f64)>, drive: Option<graph::Drive>, d: f32| {
            let (k, start, _) = side?;
            let drive = drive.filter(|dr| dr.k == k)?;
            Some((k, start, drive.moved(&def.params[k], start, d * scale)))
        };
        let nx = step(x, point.x, px - pointer.0).or(x);
        let ny = step(y, point.y, py - pointer.1).or(y);
        if (nx, ny) == (x, y) {
            return true;
        }
        if let Some(Grab::Point { x, y, .. }) = &mut self.instruments.grab {
            *x = nx;
            *y = ny;
        }
        for (k, _, v) in [nx, ny].into_iter().flatten() {
            self.device_hear(w.track, plugin, def.params[k].id, v);
        }
        true
    }

    /// Opens the field a knob's value is typed in.
    fn device_type(&mut self, plugin: PluginId, k: usize, spec: &ParamSpec, now: f64) {
        self.instruments.grab = None;
        self.instruments.entry = Some(Entry {
            plugin,
            k,
            field: Field::new(FieldId::Value, control::text(spec, now)),
            fresh: true,
        });
    }

    /// Turns the control `k` of `plugin` to what `to` makes of where it is;
    /// the song takes it once it has been still (`device_quiet`).
    fn device_turn(
        &mut self,
        plugin: PluginId,
        k: usize,
        to: impl FnOnce(&ParamSpec, f64) -> f64,
    ) -> bool {
        if self.instruments.grab.is_some() {
            return false;
        }
        let Some((track, def, now)) = self.device(plugin) else {
            return false;
        };
        let (Some(spec), Some(&had)) = (def.params.get(k), now.get(k)) else {
            return false;
        };
        if self.param_locked(track, plugin, spec.id) {
            return true;
        }
        if self
            .instruments
            .sweep
            .is_some_and(|s| (s.plugin, s.k) != (plugin, k))
        {
            self.device_sweep_end();
        }
        let from = self.instruments.sweep.map_or(had, |s| s.value);
        let next = to(spec, from);
        self.instruments.sweep = Some(Sweep {
            track,
            plugin,
            k,
            value: next,
            at: Instant::now(),
        });
        if next != from {
            self.device_hear(track, plugin, spec.id, next);
        }
        true
    }

    /// A sweep goes into the song (one step).
    pub(crate) fn device_sweep_end(&mut self) -> bool {
        match self.instruments.sweep.take() {
            Some(s) => self.device_set(s.plugin, s.k, s.value),
            None => false,
        }
    }

    /// A sweep that has been still a moment goes into the song.
    pub(crate) fn device_quiet(&mut self) -> bool {
        match self.instruments.sweep {
            Some(s) if s.at.elapsed() >= crate::mixer::knob::QUIET => self.device_sweep_end(),
            _ => false,
        }
    }

    /// The wheel over a window: a control under it turns (`notches` up is
    /// positive, Shift finer). True over any window (nothing under it scrolls).
    pub(crate) fn device_wheel(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        notches: f64,
        fine: bool,
    ) -> bool {
        let p = self.project();
        let Some((i, part)) = self.instruments.at(&p, g.size, x, y) else {
            return false;
        };
        let k = match part {
            Part::Knob(k) => k,
            Part::Point(n, at) => {
                let Some(k) = self.point(g, i, n, at).and_then(|(_, pt)| pt.wheel) else {
                    return true;
                };
                k
            }
            _ => return true,
        };
        let plugin = self.instruments.open[i].plugin;
        self.device_turn(plugin, k, |spec, v| control::wheel(spec, v, notches, fine));
        true
    }

    /// The `at`th point of the `n`th graph of the `i`th window, and the
    /// device's parameters.
    fn point(
        &self,
        g: &Geometry,
        i: usize,
        n: usize,
        at: usize,
    ) -> Option<(&'static BuiltinDef, Point)> {
        let p = self.project();
        let w = self.instruments.open.get(i)?;
        let (_, x, def) = live(&p, w)?;
        let r = self.instruments.rect(i, g.size);
        let pt = self.instruments.points_of(r, x, def, n).get(at).cloned()?;
        Some((def, pt))
    }

    /// The menu of a control that is a choice shown as its name (an EQ
    /// band's type), at `(x, y)`: each choice, the one it is on checked.
    fn device_menu(
        &mut self,
        w: Win,
        def: &'static BuiltinDef,
        k: usize,
        now: f64,
        x: f32,
        y: f32,
    ) {
        let spec = &def.params[k];
        let Some(band) = super::eq::band_of(def, k) else {
            return;
        };
        let b = &xirius_params::EQ_BANDS[band];
        let at = b.type_of(spec.to_plain(now));
        let items = b
            .types
            .iter()
            .enumerate()
            .map(|(t, ty)| {
                let args = json!({
                    "id": w.plugin.to_string(),
                    "param": spec.id,
                    "value": super::eq::choice(b.types.len(), t),
                });
                self.ctx_as(ty.label(), "plugin.param.set", args)
                    .on(*ty == at)
            })
            .collect();
        self.open_context(items, x, y);
    }

    /// A right press on a graph's point: what it can be (its menu's rows),
    /// then back to the defaults of all it moves.
    fn point_menu(&mut self, g: &Geometry, i: usize, at: (usize, usize), x: f32, y: f32) {
        let Some((def, pt)) = self.point(g, i, at.0, at.1) else {
            return;
        };
        let w = self.instruments.open[i];
        self.device_blur();
        self.instruments.chosen.insert(w.plugin, at);
        let held = pt
            .resets()
            .into_iter()
            .any(|k| self.param_locked(w.track, w.plugin, def.params[k].id));
        let set_args = |set: &[(usize, f64)]| {
            let mut args = json!({ "id": w.plugin.to_string() });
            if let Some(((k, v), rest)) = set.split_first() {
                args["param"] = json!(def.params[*k].id);
                args["value"] = json!(v);
                if !rest.is_empty() {
                    args["also"] = json!(
                        rest.iter()
                            .map(|(k, v)| json!({ "param": def.params[*k].id, "value": v }))
                            .collect::<Vec<_>>()
                    );
                }
            }
            args
        };
        let mut items = Vec::new();
        for row in &pt.menu {
            let item = match row {
                Some(c) => {
                    let item = self.ctx_as(&c.label, "plugin.param.set", set_args(&c.set));
                    crate::auto_lock::gate(item.on(c.on), held)
                }
                None => Item::gap(),
            };
            items.push(item);
        }
        let back: Vec<(usize, f64)> = pt
            .resets()
            .into_iter()
            .map(|k| {
                (
                    k,
                    control::snap(&def.params[k], def.params[k].default_norm()),
                )
            })
            .collect();
        if !items.is_empty() {
            items.push(Item::gap());
        }
        let reset = self.ctx_as("既定に戻す", "plugin.param.set", set_args(&back));
        items.push(crate::auto_lock::gate(reset, held));
        self.open_context(items, x, y);
    }

    /// A controller's knob learnt for a parameter moves it (0..=127 over the
    /// whole range), whether its window is open or not.
    pub(crate) fn device_midi(&mut self, track: TrackId, plugin: PluginId, param: &str, cc: u8) {
        let Some((t, def, _)) = self.device(plugin) else {
            return;
        };
        let Some((k, _)) = def.param(param).filter(|_| t == track) else {
            return;
        };
        let n = f64::from(cc.min(127)) / 127.0;
        self.device_turn(plugin, k, |spec, _| control::snap(spec, n));
    }

    /// The keys of the control that has them, or of the value being typed.
    pub(crate) fn device_key(&mut self, key: &str, mods: Mods) -> bool {
        if let Some(e) = &mut self.instruments.entry {
            if !e.field.marked.is_empty() {
                return true;
            }
            match key {
                "escape" => self.instruments.entry = None,
                "backspace" if e.fresh => {
                    e.fresh = false;
                    e.field.set(String::new());
                }
                "backspace" => e.field.backspace(),
                "enter" | "tab" => {
                    self.device_commit();
                }
                _ => {}
            }
            return true;
        }
        let Some((plugin, k)) = self.instruments.focus else {
            return false;
        };
        if mods.ctrl || mods.alt {
            return false;
        }
        let Some((track, def, values)) = self.device(plugin) else {
            self.instruments.focus = None;
            return false;
        };
        let Some(spec) = def.params.get(k) else {
            return false;
        };
        let now = self.device_value(plugin, k).unwrap_or(spec.default_norm());
        match key {
            "tab" => {
                let w = self
                    .instruments
                    .open
                    .iter()
                    .find(|w| w.plugin == plugin)
                    .map_or_else(|| layout::natural_w(def), |w| w.body.0);
                let shape = layout::Shape {
                    w,
                    band: self.instruments.band(plugin),
                    values: &values,
                };
                let order: Vec<usize> = layout::body(def, &shape)
                    .slots
                    .iter()
                    .map(|s| s.k)
                    .collect();
                let at = order.iter().position(|&x| x == k).unwrap_or(0);
                let n = order.len().max(1);
                let next = if mods.shift { at + n - 1 } else { at + 1 } % n;
                self.device_sweep_end();
                self.instruments.focus = order.get(next).map(|&k| (plugin, k));
                return true;
            }
            "escape" => {
                self.device_blur();
                return true;
            }
            _ => {}
        }
        let small = if mods.shift { Step::Fine } else { Step::Small };
        let by = match key {
            "up" | "right" => Some((1.0, small)),
            "down" | "left" => Some((-1.0, small)),
            "pageup" => Some((1.0, Step::Large)),
            "pagedown" => Some((-1.0, Step::Large)),
            _ => None,
        };
        if let Some((dir, by)) = by {
            self.device_turn(plugin, k, |spec, v| control::step(spec, v, dir, by));
            return true;
        }
        let to = match key {
            "home" => control::snap(spec, 0.0),
            "end" => control::snap(spec, 1.0),
            "delete" | "backspace" => control::snap(spec, spec.default_norm()),
            "enter" | "space" => match control::look(spec) {
                Look::Toggle => 1.0 - now.round(),
                Look::Knob if key == "enter" => {
                    if !self.param_locked(track, plugin, spec.id) {
                        self.device_type(plugin, k, spec, now);
                    }
                    return true;
                }
                _ => return true,
            },
            _ => return false,
        };
        self.device_sweep_end();
        if !self.param_locked(track, plugin, spec.id) {
            self.device_set(plugin, k, to);
        }
        true
    }

    /// A right press on a window: on a control, its menu (back to the default,
    /// its automation lane, MIDI learn). True over any window.
    pub(crate) fn instrument_right(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let p = self.project();
        let Some((i, part)) = self.instruments.at(&p, g.size, x, y) else {
            return false;
        };
        let k = match part {
            Part::Knob(k) => k,
            Part::Point(n, at) => {
                self.point_menu(g, i, (n, at), x, y);
                return true;
            }
            _ => return true,
        };
        let w = self.instruments.open[i];
        let Some((track, def, _)) = self.device(w.plugin) else {
            return true;
        };
        let Some(spec) = def.params.get(k) else {
            return true;
        };
        self.device_blur();
        self.instruments.focus = Some((w.plugin, k));
        let held = self.param_locked(track, w.plugin, spec.id);
        let default = control::snap(spec, spec.default_norm());
        let away = self
            .device_value(w.plugin, k)
            .is_some_and(|v| (v - default).abs() > 1e-9);
        let args = json!({ "id": w.plugin.to_string(), "param": spec.id, "value": default });
        let reset = self.ctx_as("既定に戻す", "plugin.param.set", args);
        let reset = if away { reset } else { reset.enabled(false) };
        let lane = self.lane_item(track, AutoTarget::Param(w.plugin, spec.id.to_string()));
        let target = Target::Param {
            track: track.to_string(),
            plugin: w.plugin.to_string(),
            param: spec.id.to_string(),
        };
        let mut items = vec![
            crate::auto_lock::gate(reset, held),
            Item::gap(),
            lane,
            Item::gap(),
        ];
        items.extend(self.midi_rows(target));
        self.open_context(items, x, y);
        true
    }
}
