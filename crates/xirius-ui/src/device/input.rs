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
use super::sampler;
use crate::context::Item;
use crate::instrument::window::{
    DOUBLE, Flip, Grab, Part, Pressed, TAP, TAP_COARSE, Win, live, values_of,
};
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

/// A stop of Tab in a device's window: the control `k`, or the point `point` (its graph
/// and its index) whose value `k` is; `lone`: a point whose controls are not on show.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stop {
    k: usize,
    point: Option<(usize, usize)>,
    lone: bool,
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
            // A Sampler's zone's, a Drum Kit's pad's parameters follow its window (UI-333).
            let def = self.instruments.def_of(x)?;
            Some((*t, def, values_of(x, def)))
        })
    }

    /// The value the control `k` of `plugin` shows: while it turns, where it is; else where
    /// its lane has it at the playhead ([`Studio::auto_shown`]), so a press or a grab starts
    /// from what the window draws (AUT-016).
    pub(crate) fn device_value(&self, plugin: PluginId, k: usize) -> Option<f64> {
        self.instruments.turning(plugin, k).or_else(|| {
            let p = self.auto_shown();
            let x = p
                .tracks
                .values()
                .find_map(|t| t.inserts.iter().find(|x| x.id == plugin))?;
            let def = self.instruments.def_of(x)?;
            values_of(x, def).get(k).copied()
        })
    }

    /// The mirror a reversed Sampler's loop's end `k` shows in (low 1): the wheel, the keys,
    /// a value typed and a screen reader move what it shows, the way it moves on the wave.
    fn device_mirror(&self, plugin: PluginId, k: usize) -> Option<(f64, f64)> {
        let (_, def, values) = self.device(plugin)?;
        sampler::mirror(def, &values, k).map(|(_, ends)| ends)
    }

    /// [`Self::device_turn`] of what the control shows: a mirrored loop's end turns its
    /// mirror ([`Self::device_mirror`]).
    fn device_turn_shown(
        &mut self,
        plugin: PluginId,
        k: usize,
        to: impl FnOnce(&ParamSpec, f64) -> f64,
    ) -> bool {
        match self.device_mirror(plugin, k) {
            Some(m) => self.device_turn(plugin, k, |spec, v| {
                sampler::mirrored(spec, to(spec, sampler::mirrored(spec, v, m)), m)
            }),
            None => self.device_turn(plugin, k, to),
        }
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
        let done = self.run_command(
            "plugin.param.set",
            json!({ "id": plugin.to_string(), "param": spec.id, "value": value }),
        );
        self.device_unheard(done);
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
        let done = self.run_command("plugin.param.set", args);
        self.device_unheard(done);
        true
    }

    /// A turn the song did not take (`done` false: a save or a macro holds the input, or
    /// the command failed) leaves the sound at the song's value, as the screen shows: the
    /// value heard while it turned goes (N-ui-1). The grab and the sweep are gone already.
    fn device_unheard(&mut self, done: bool) {
        if !done {
            self.device_hear();
        }
    }

    /// The player hears what the devices' windows are turning (a drag, a sweep) while the
    /// song keeps the old values: all of them in one sync (M-ui-1), as every other sync
    /// through [`Studio::auto_heard`] does.
    pub(crate) fn device_hear(&mut self) {
        if !self.transport.is_live() {
            return;
        }
        let heard = self.auto_heard(&self.project());
        self.transport.sync(&heard, self.song().dir());
    }

    /// The control under a press loses the keys: a typed value goes in, a
    /// sweep ends.
    pub(crate) fn device_blur(&mut self) {
        self.device_commit();
        self.device_sweep_end();
        self.instruments.focus = None;
        self.instruments.edge = None;
        self.instruments.on_point = None;
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
                let plain = match self.device_mirror(e.plugin, e.k) {
                    Some(ends) => sampler::flip(plain, ends),
                    None => plain,
                };
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
        self.instruments.on_point = None;
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
            Look::Knob | Look::Field if clicks >= 2 => self.device_type(w.plugin, k, spec, now),
            Look::Knob | Look::Field => {
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
    /// hold of. An EQ band's point that has the keys (chosen by a press on the graph)
    /// switches its band when it is let go unmoved (UI-326); a second press within
    /// [`DOUBLE`] takes that step back and types.
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
        // Chosen in its column, on a tab, by its menu or by Tab, the point only takes the keys.
        let was = self.instruments.on_point == Some((w.plugin, at))
            && self.instruments.chosen.get(&w.plugin) == Some(&at);
        self.instruments.chosen.insert(w.plugin, at);
        self.instruments.on_point = Some((w.plugin, at));
        let first = point.params().next();
        self.instruments.focus = first.map(|k| (w.plugin, k));
        let free = |s: &Studio, k: usize| {
            def.params
                .get(k)
                .is_some_and(|spec| !s.param_locked(w.track, w.plugin, spec.id))
        };
        let switch = graph::switch(def, at.0, at.1);
        let now = Instant::now();
        let again =
            self.instruments.pressed.take().filter(|p| {
                p.plugin == w.plugin && p.at == at && now.duration_since(p.when) <= DOUBLE
            });
        self.instruments.pressed = Some(Pressed {
            plugin: w.plugin,
            at,
            when: now,
            flip: None,
        });
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
        // A point that switches its band counts its own double press: the first one may
        // have switched it, and a slow second one switches it again.
        if clicks >= 2 && (switch.is_none() || again.is_some()) {
            if let Some(flip) = again.and_then(|p| p.flip) {
                self.device_unflip(w.plugin, flip);
            }
            if let Some(k) = point.x.or(point.y).map(|d| d.k).filter(|&k| free(self, k))
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
        // The first press on a point only chooses it.
        let tap = switch.filter(|&k| was && free(self, k));
        if x.is_some() || y.is_some() || tap.is_some() {
            self.instruments.grab = Some(Grab::Point {
                plugin: w.plugin,
                graph: at.0,
                i: at.1,
                pointer,
                x,
                y,
                tap,
            });
        }
    }

    /// A point let go before it moved [`TAP`] (UI-326): its band switches, one step, and
    /// the press remembers it so a double press puts it back.
    pub(crate) fn device_point_tap(&mut self, plugin: PluginId, k: usize) -> bool {
        let Some(before) = self.device_value(plugin, k) else {
            return false;
        };
        let was = self.song.session().last_step();
        let done = self.device_set(plugin, k, 1.0 - before.round());
        // A switch the song did not take (a save holds it) is no step: a double press then
        // sets the band back rather than undo a step of its own (review 2 L1).
        let step = Some(self.song.session().last_step()).filter(|&n| n != was);
        if done && let Some(p) = &mut self.instruments.pressed {
            p.flip = Some((k, before, step));
        }
        done
    }

    /// A double press puts back the band its first press switched: the switch's step is
    /// taken back, so the history keeps nothing of it. Past another step (or one undone),
    /// the band is set back as a step of its own.
    fn device_unflip(&mut self, plugin: PluginId, (k, before, step): Flip) {
        let s = self.song.session();
        if step == Some(s.last_step()) && !s.can_redo() && s.can_undo() {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            let done = self.run_command("history.undo", json!({}));
            self.device_unheard(done);
        } else {
            self.device_set(plugin, k, before);
        }
    }

    /// Space on a graph's point that has the keys (UI-326): the band of the EQ point
    /// switches, before the transport takes Space. False when the keys are on no such point
    /// (a column's knob, or a point chosen another way) or its lane holds the band: Space is
    /// the transport's then.
    fn device_point_space(&mut self, g: &Geometry, plugin: PluginId, k: usize) -> bool {
        let Some(&(n, i)) = self.instruments.chosen.get(&plugin) else {
            return false;
        };
        if self.instruments.on_point != Some((plugin, (n, i))) {
            return false;
        }
        let Some(at) = self
            .instruments
            .open
            .iter()
            .position(|w| w.plugin == plugin)
        else {
            return false;
        };
        let Some((def, point)) = self.point(g, at, n, i) else {
            return false;
        };
        let Some(on) = graph::switch(def, n, i) else {
            return false;
        };
        if !point.params().any(|p| p == k) {
            return false;
        }
        let w = self.instruments.open[at];
        if self.param_locked(w.track, plugin, def.params[on].id) {
            return false;
        }
        let Some(v) = self.device_value(plugin, on) else {
            return false;
        };
        self.device_sweep_end();
        self.device_set(plugin, on, 1.0 - v.round());
        true
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
            tap,
        }) = self.instruments.grab
        else {
            return false;
        };
        // Within [`TAP`] of the press it may still be one that switches the band; past it,
        // it is a drag and switches nothing.
        if tap.is_some() {
            let slop = if self.coarse_press { TAP_COARSE } else { TAP };
            if (px - pointer.0).hypot(py - pointer.1) <= slop {
                return true;
            }
            if let Some(Grab::Point { tap, .. }) = &mut self.instruments.grab {
                *tap = None;
            }
        }
        let p = self.auto_shown();
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
        let r = self.instruments.frame(at, g);
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
        // Both axes at once: a point pulled aslant is heard moving both ways (M-ui-1).
        self.device_hear();
        true
    }

    /// Opens the field a knob's value is typed in.
    fn device_type(&mut self, plugin: PluginId, k: usize, spec: &ParamSpec, now: f64) {
        self.instruments.grab = None;
        let now = match self.device_mirror(plugin, k) {
            Some(m) => sampler::mirrored(spec, now, m),
            None => now,
        };
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
            self.device_hear();
        }
        true
    }

    /// The window lost the keyboard (M-ui-3): the button will be let go elsewhere, so it is
    /// let go here. A held key or pad ends its note, a knob or a point goes in as one step.
    /// True when something was held.
    pub(crate) fn device_focus_lost(&mut self) -> bool {
        let target = self.instruments.detached_target;
        let scrolling = self
            .instruments
            .keyboard_drag
            .is_some_and(|(id, _)| target.is_none_or(|t| t == id));
        if scrolling {
            self.instruments.keyboard_drag = None;
        }
        let held = if self
            .instruments
            .grab
            .as_ref()
            .is_some_and(|g| target.is_none_or(|t| t == g.plugin()))
        {
            self.instrument_up()
        } else {
            false
        };
        let note = if self
            .instruments
            .note
            .is_some_and(|n| target.is_none_or(|t| t == n.plugin))
        {
            self.window_note_off()
        } else {
            false
        };
        note || held || scrolling
    }

    /// A sweep goes into the song (one step). Also before anything that reads or moves the
    /// song's history or file: undo, redo, save, another song (M-par-1), so the turn is its
    /// own step there and not lost or put after them.
    pub(crate) fn device_sweep_end(&mut self) -> bool {
        match self.instruments.sweep.take() {
            Some(s) => self.device_set(s.plugin, s.k, s.value),
            None => false,
        }
    }

    /// A sweep not yet in the song goes: what was heard of it goes back to the song's.
    fn device_sweep_cancel(&mut self) -> bool {
        if self.instruments.sweep.take().is_none() {
            return false;
        }
        self.device_hear();
        true
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
        let p = self.auto_shown();
        for (i, w) in self.instruments.open.iter().enumerate() {
            if self.instruments.detached_channel
                || self.instruments.detached_target != Some(w.plugin)
            {
                continue;
            }
            let Some((_, plugin, def)) = crate::instrument::window::live(&p, w) else {
                continue;
            };
            let body = self
                .instruments
                .body(self.instruments.frame(i, g), plugin, def);
            if let Some(k) = body.keyboard.filter(|k| k.view.contains(x, y)) {
                let scroll =
                    (k.scroll - notches as f32 * 40.0).clamp(0.0, (k.extent - k.view.w).max(0.0));
                self.instruments.keyboard_scroll.insert(w.plugin, scroll);
                return true;
            }
        }
        let Some((i, part)) = self.instruments.at(&p, g, x, y) else {
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
        self.device_turn_shown(plugin, k, |spec, v| control::wheel(spec, v, notches, fine));
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
        let p = self.auto_shown();
        let w = self.instruments.open.get(i)?;
        let (_, x, def) = live(&p, w)?;
        let r = self.instruments.frame(i, g);
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
        if def.uid == "delay" {
            let Some((track, _, values)) = self.device(w.plugin) else {
                return;
            };
            let held = ["division", "feel"]
                .iter()
                .find_map(|id| self.param_held(track, w.plugin, id));
            let items = super::delay::beats_menu(def, &values)
                .iter()
                .map(|row| match row {
                    Some(c) => {
                        let args = set_args(def, w.plugin, &c.set);
                        let item = self.ctx_as(&c.label, "plugin.param.set", args);
                        crate::auto_lock::gate(item.on(c.on), held)
                    }
                    None => Item::gap(),
                })
                .collect();
            self.open_classified("graph-85", items, x, y);
            return;
        }
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
    pub(super) fn point_menu(
        &mut self,
        g: &Geometry,
        i: usize,
        at: (usize, usize),
        x: f32,
        y: f32,
    ) {
        let Some((def, pt)) = self.point(g, i, at.0, at.1) else {
            return;
        };
        let w = self.instruments.open[i];
        self.device_blur();
        self.instruments.chosen.insert(w.plugin, at);
        let held = pt
            .resets()
            .into_iter()
            .find_map(|k| self.param_held(w.track, w.plugin, def.params[k].id));
        let set_args = |set: &[(usize, f64)]| set_args(def, w.plugin, set);
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
        // A row that already puts all of it back (a filter opened) stands for the defaults.
        let same =
            |set: &[(usize, f64)]| set.len() == back.len() && back.iter().all(|b| set.contains(b));
        // The approved Delay menu names reset separately from its 18 beat choices.
        if (def.uid == "delay" && !pt.menu.is_empty())
            || !pt.menu.iter().flatten().any(|c| same(&c.set))
        {
            if !items.is_empty() {
                items.push(Item::gap());
            }
            let reset = self.ctx_as("既定に戻す", "plugin.param.set", set_args(&back));
            items.push(crate::auto_lock::gate(reset, held));
        }
        let target = if def.uid == "eq" {
            match at.1 {
                0 | 1 => "graph-81",
                2 | 3 => "graph-82",
                _ => "graph-83",
            }
        } else if def.uid == "delay" && !pt.menu.is_empty() {
            "graph-85"
        } else if def.uid == "limiter" && !pt.menu.is_empty() {
            "graph-limiter"
        } else if def.uid == "reverb" && !pt.menu.is_empty() {
            "graph-reverb"
        } else if !pt.menu.is_empty() {
            "graph-84"
        } else {
            "graph-86"
        };
        self.open_classified(target, items, x, y);
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
    pub(crate) fn device_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> bool {
        let Some(target) = self.instruments.detached_target else {
            return false;
        };
        if self.instruments.focus.is_some_and(|(id, _)| id != target)
            || self.instruments.edge.is_some_and(|(id, _)| id != target)
            || self
                .instruments
                .entry
                .as_ref()
                .is_some_and(|e| e.plugin != target)
        {
            return false;
        }
        // Esc during a press lets go of what it holds, taking nothing in (L-par-3).
        if key == "escape" && self.instrument_cancel() {
            return true;
        }
        // A menu or another field open takes Esc before a sweep does (theirs are further on);
        // a value typed here, below.
        let other = self.popup.is_some()
            || self
                .ime_target()
                .is_some_and(|t| t != crate::interaction::ImeTarget::Device)
            || self.bar_edit.is_typing()
            || self.inspector.field.is_some()
            || self.field_typing().is_some();
        if key == "escape" && other {
            return false;
        }
        // During a sweep of the wheel or the keys not yet in, it goes (L2-ui-6).
        if key == "escape" && self.instruments.entry.is_none() && self.device_sweep_cancel() {
            return true;
        }
        let blocked = self.input_blocked();
        if let Some(e) = &mut self.instruments.entry {
            if !e.field.marked.is_empty() {
                return true;
            }
            // While a save or an undo on the worker holds the song, Enter and Tab keep what
            // is typed: the song would not take it, and it would go unsaid. The window's
            // keys and text wait for a short save before they come here (`held_input`); this
            // is for the keys that come another way.
            if matches!(key, "enter" | "tab") && blocked {
                return true;
            }
            match key {
                "escape" => self.instruments.entry = None,
                "backspace" if e.fresh => {
                    e.fresh = false;
                    e.field.set(String::new());
                }
                "backspace" => e.field.backspace(),
                "enter" => {
                    self.device_commit();
                }
                // Tab puts the value in and goes on (M-ui-4).
                "tab" if !mods.ctrl && !mods.alt => {
                    self.device_commit();
                    self.device_key(g, key, mods);
                    return true;
                }
                _ => {}
            }
            return true;
        }
        if !mods.ctrl && !mods.alt && self.layer_edge_key(key, mods.shift) {
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
            // Space on an EQ's point switches its band (UI-326); else it is the
            // transport's, whatever has the keys (M-ui-4).
            "space" => return !mods.shift && self.device_point_space(g, plugin, k),
            "tab" => {
                let order = self.device_stops(track, plugin, def, &values);
                let at = order.iter().position(|s| s.k == k).unwrap_or(0);
                self.device_sweep_end();
                // Tab stays on this native surface, including at either end.
                let next = if mods.shift {
                    at.checked_sub(1)
                } else {
                    Some(at + 1)
                };
                match next.and_then(|i| order.get(i)) {
                    Some(&stop) => self.device_stop(track, plugin, def, stop, !mods.shift),
                    None => {
                        self.device_blur();
                        self.device_enter(!mods.shift);
                    }
                }
                return true;
            }
            "escape" => {
                self.device_blur();
                self.instruments.passed = true;
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
            self.device_turn_shown(plugin, k, |spec, v| control::step(spec, v, dir, by));
            return true;
        }
        let ends = (control::snap(spec, 0.0), control::snap(spec, 1.0));
        let ends = match self.device_mirror(plugin, k) {
            Some(m) => (
                sampler::mirrored(spec, ends.0, m),
                sampler::mirrored(spec, ends.1, m),
            ),
            None => ends,
        };
        let to = match key {
            "home" => ends.0,
            "end" => ends.1,
            "delete" | "backspace" => control::snap(spec, spec.default_norm()),
            "enter" => match control::look(spec) {
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

    /// The stops Tab goes through in the window of `plugin` (UI-324, M-ui-4): its controls
    /// in their order, a point's chosen as the keys reach it; and where a graph shows the
    /// controls of one point at a time (a narrow EQ's band), every point of it in its place:
    /// the chosen one by its controls, the others by the first value each moves.
    fn device_stops(
        &self,
        track: TrackId,
        plugin: PluginId,
        def: &'static BuiltinDef,
        values: &[f64],
    ) -> Vec<Stop> {
        let win = self.instruments.win_of(plugin);
        let p = self.auto_shown();
        let x = p
            .tracks
            .get(&track)
            .and_then(|t| t.inserts.iter().find(|x| x.id == plugin));
        let shape = layout::Shape {
            w: win.map_or_else(|| layout::natural_w(def), |w| w.body.0),
            band: self.instruments.band(plugin),
            values,
            plugin: x,
            pick: win.map_or(0, |w| w.pick),
            layer: win.map_or(0, |w| w.layer),
            bank: win.and_then(|w| w.bank),
        };
        let body = layout::body(def, &shape);
        let shown: Vec<usize> = body.slots.iter().map(|s| s.k).collect();
        let (mut points, mut every): (Vec<_>, Vec<_>) = (Vec::new(), Vec::new());
        for (n, &g) in body.graphs.iter().enumerate() {
            let pts: Vec<((usize, usize), Point)> = graph::points(def, n, g, values)
                .into_iter()
                .enumerate()
                .map(|(i, pt)| ((n, i), pt))
                .collect();
            let hidden = pts
                .iter()
                .any(|(_, pt)| pt.params().next().is_some_and(|k| !shown.contains(&k)));
            if hidden {
                points.extend(pts.iter().cloned());
            }
            every.extend(pts);
        }
        let find = |pts: &[((usize, usize), Point)], k: usize| {
            pts.iter()
                .find(|(_, pt)| pt.resets().contains(&k))
                .map(|(at, _)| *at)
        };
        let owner = |k: usize| find(&points, k);
        let all_points = |out: &mut Vec<Stop>| {
            for (at, pt) in &points {
                let mine: Vec<usize> = shown
                    .iter()
                    .copied()
                    .filter(|&k| owner(k) == Some(*at))
                    .collect();
                if mine.is_empty() {
                    out.extend(pt.params().next().map(|k| Stop {
                        k,
                        point: Some(*at),
                        lone: true,
                    }));
                }
                out.extend(mine.into_iter().map(|k| Stop {
                    k,
                    point: Some(*at),
                    lone: false,
                }));
            }
        };
        let mut out = Vec::new();
        let mut placed = points.is_empty();
        for &k in &shown {
            if owner(k).is_none() {
                out.push(Stop {
                    k,
                    point: find(&every, k),
                    lone: false,
                });
            } else if !placed {
                placed = true;
                all_points(&mut out);
            }
        }
        if !placed {
            all_points(&mut out);
        }
        let mut seen = Vec::new();
        out.retain(|s| {
            let first = !seen.contains(&s.k);
            seen.push(s.k);
            first
        });
        out
    }

    /// A screen reader turns the control `k` of `plugin` (M-ui-2): a step up or down as the
    /// arrow keys make it (the song takes it once still), or to a value in one step.
    pub(crate) fn device_a11y_turn(
        &mut self,
        plugin: PluginId,
        k: usize,
        way: crate::a11y::Turn,
    ) -> bool {
        use crate::a11y::Turn;
        let dir = match way {
            Turn::Up => 1.0,
            Turn::Down => -1.0,
            Turn::To(at) => {
                let Some((track, def, _)) = self.device(plugin) else {
                    return false;
                };
                let Some(spec) = def.params.get(k) else {
                    return false;
                };
                if self.param_locked(track, plugin, spec.id) {
                    return false;
                }
                // A sweep of this control waiting to go in is overtaken: one step, to the
                // value set (L2-ui-3); another control's goes in as its own.
                match self.instruments.sweep {
                    Some(s) if (s.plugin, s.k) == (plugin, k) => self.instruments.sweep = None,
                    _ => {
                        self.device_sweep_end();
                    }
                }
                let at = self
                    .device_mirror(plugin, k)
                    .map_or(at, |ends| sampler::flip(at, ends));
                let v = control::snap(spec, spec.to_norm(at));
                if !self.device_set(plugin, k, v) {
                    // Back where the song has it: what was heard of the sweep goes.
                    self.device_hear();
                }
                return true;
            }
        };
        self.device_turn_shown(plugin, k, |spec, v| {
            control::step(spec, v, dir, Step::Small)
        })
    }

    /// The keys go to the stop `stop` of the window of `plugin`. A point not on show is
    /// chosen, and the keys go on to the first of its controls (`ahead`; else the last).
    fn device_stop(
        &mut self,
        track: TrackId,
        plugin: PluginId,
        def: &'static BuiltinDef,
        stop: Stop,
        ahead: bool,
    ) {
        self.instruments.focus = Some((plugin, stop.k));
        self.instruments.edge = None;
        self.instruments.on_point = None;
        let Some(at) = stop.point else {
            return;
        };
        self.instruments.chosen.insert(plugin, at);
        if !stop.lone {
            return;
        }
        let Some((_, _, values)) = self.device(plugin) else {
            return;
        };
        let order = self.device_stops(track, plugin, def, &values);
        let mut mine = order.iter().filter(|s| s.point == Some(at) && !s.lone);
        let to = if ahead { mine.next() } else { mine.next_back() };
        if let Some(s) = to {
            self.instruments.focus = Some((plugin, s.k));
        }
    }

    /// Tab from the screen's last place (Shift+Tab from its first) goes into the window on
    /// top: the keys go to its first stop (back: its last) (M-ui-4). False with no window.
    pub(crate) fn device_enter(&mut self, ahead: bool) -> bool {
        let Some(w) = self
            .instruments
            .open
            .iter()
            .find(|w| Some(w.plugin) == self.instruments.detached_target)
            .copied()
        else {
            return false;
        };
        let Some((track, def, values)) = self.device(w.plugin) else {
            return false;
        };
        let order = self.device_stops(track, w.plugin, def, &values);
        let to = if ahead { order.first() } else { order.last() };
        let Some(&stop) = to else {
            return false;
        };
        self.device_stop(track, w.plugin, def, stop, ahead);
        true
    }

    /// A right press on a window: on a control, its menu (back to the default,
    /// its automation lane, MIDI learn). True over any window.
    pub(crate) fn instrument_right(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        // The start screen covers the song: no window of a device is over it.
        if self.home_shown() {
            return false;
        }
        let p = self.auto_shown();
        let Some((i, part)) = self.instruments.at(&p, g, x, y) else {
            return false;
        };
        let k = match part {
            Part::Knob(k) => k,
            Part::Point(n, at) => {
                self.point_menu(g, i, (n, at), x, y);
                return true;
            }
            Part::Spot(n) => {
                let r = self.instruments.frame(i, g);
                self.spot_menu(i, r, n, x, y);
                return true;
            }
            Part::Pad(pitch) => {
                self.pad_menu(i, crate::device::play::pad_of(pitch), x, y);
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
        let held = self.param_held(track, w.plugin, spec.id);
        let default = control::snap(spec, spec.default_norm());
        let away = self
            .device_value(w.plugin, k)
            .is_some_and(|v| (v - default).abs() > 1e-9);
        let args = json!({ "id": w.plugin.to_string(), "param": spec.id, "value": default });
        let reset = self.ctx_as("既定に戻す", "plugin.param.set", args);
        let reset = if away { reset } else { reset.enabled(false) };
        let param = AutoTarget::Param(w.plugin, spec.id.to_string());
        let lane = self.lane_item(track, param.clone());
        let to_macro = self.macro_item(track, param);
        let target = Target::Param {
            track: track.to_string(),
            plugin: w.plugin.to_string(),
            param: spec.id.to_string(),
        };
        let mut items = vec![
            crate::auto_lock::gate(reset, held),
            Item::gap(),
            lane,
            to_macro,
            Item::gap(),
        ];
        items.extend(self.midi_rows(target));
        self.open_classified("other-45", items, x, y);
        true
    }
}

/// What `plugin.param.set` takes to set the parameters `set` (index, value
/// 0..1) of `plugin` in one step: the first, then the rest as `also`.
fn set_args(def: &BuiltinDef, plugin: PluginId, set: &[(usize, f64)]) -> serde_json::Value {
    let mut args = json!({ "id": plugin.to_string() });
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
}
