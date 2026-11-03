//! The window of a built-in plug-in (UI-098, PLG-045): the shared title bar
//! (UI-322) over its knobs, floating over the screen. The keys button at the
//! head of an instrument track opens an instrument's, and the plug-in's row
//! in the mixer opens an instrument's or an effect's. A plug-in of its own
//! (VST3) opens its own window instead.
//!
//! The controls are the built-in's parameters as `xirius-params` lists them,
//! where the device's layout puts them (`device::layout`): a parameter added
//! there gets its control here. A knob turns while it is dragged and goes into
//! the song when it is let go (`plugin.param.set`, 0..1, one step); the other
//! ways a control moves are `device::input`'s (UI-324). A preset sets its
//! knobs at once (`plugin.preset.load`, one step).

use std::collections::HashMap;

use gpui::{Bounds, Corners, point, px, size};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_design::Radius;
use xirius_params::{BuiltinDef, ParamSpec};

use super::menu::Menu;
use crate::device::control::{self, KnobView, Look};
use crate::device::graph::{self, Point};
use crate::device::layout::{self, Slot};
use crate::draw::Pen;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar;

/// How much of a window stays on the screen when it is dragged off.
const KEEP_W: f32 = 80.0;

/// The presets, the first being the defaults.
pub const PRESETS: [&str; 4] = ["初期", "やさしく", "強め", "声を前に"];

/// What the preset box says when the knobs match no preset.
pub const CUSTOM: &str = "カスタム";

/// The knobs' values (0..1) of a preset: `初期` is the defaults, the rest are the
/// factory presets `xirius-params` ships (PLG-063), which leave the knobs no preset
/// sets ([`ParamSpec::preset`]) at their defaults.
pub fn preset_values(def: &BuiltinDef, preset: &str) -> Option<Vec<f64>> {
    if preset == "初期" {
        return Some(def.params.iter().map(|p| p.default_norm()).collect());
    }
    xirius_params::factory_presets(def)
        .into_iter()
        .find(|(name, _)| *name == preset)
        .map(|(_, values)| values)
}

/// The knobs' values (0..1) of a built-in plugin: what it keeps, else the
/// default. Empty for any other plugin.
pub fn values(plugin: &Plugin) -> Vec<f64> {
    plugin.builtin().map_or_else(Vec::new, |def| {
        def.params
            .iter()
            .map(|p| {
                plugin
                    .params
                    .get(p.id)
                    .copied()
                    .unwrap_or_else(|| p.default_norm())
            })
            .collect()
    })
}

/// The preset the knobs a preset sets are at, or [`CUSTOM`].
pub fn preset_of(plugin: &Plugin) -> &'static str {
    let Some(def) = plugin.builtin() else {
        return CUSTOM;
    };
    let now = values(plugin);
    PRESETS
        .iter()
        .find(|p| {
            preset_values(def, p).is_some_and(|v| {
                def.params
                    .iter()
                    .zip(v.iter().zip(&now))
                    .all(|(s, (a, b))| !s.preset || (a - b).abs() < 1e-6)
            })
        })
        .copied()
        .unwrap_or(CUSTOM)
}

/// A knob's value as it reads (`-12.0 dB`, `1.20 kHz`, `付点`).
pub fn text(spec: &ParamSpec, norm: f64) -> String {
    control::text(spec, norm)
}

/// Does this plugin open in the window here (a built-in instrument or effect)?
pub fn is_built_in(plugin: &Plugin) -> bool {
    plugin.builtin().is_some()
}

/// An open window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Win {
    pub track: TrackId,
    pub plugin: PluginId,
    /// How many controls it has.
    pub knobs: usize,
    /// Its body's size, as the device's layout wants it.
    pub body: (f32, f32),
    /// Where its corner is, once it was dragged; else it sits where new ones go.
    pub at: Option<(f32, f32)>,
    /// The pin is down. UI-323 decides what an unpinned window does; the seat
    /// and the look are the shared bar's already (UI-322).
    pub pinned: bool,
}

/// What a press in a window took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Grab {
    /// The bar: the window moves with the pointer.
    Move {
        plugin: PluginId,
        from: (f32, f32),
        pointer: (f32, f32),
    },
    /// A knob: its value (0..1) while it turns.
    Knob {
        plugin: PluginId,
        k: usize,
        start: f64,
        y: f32,
        value: f64,
    },
    /// A graph's point (UI-325): the parameters its x and y move, each with
    /// its value when it was taken and now (0..1).
    Point {
        plugin: PluginId,
        graph: usize,
        i: usize,
        pointer: (f32, f32),
        x: Option<(usize, f64, f64)>,
        y: Option<(usize, f64, f64)>,
    },
}

impl Grab {
    fn plugin(&self) -> PluginId {
        match *self {
            Grab::Move { plugin, .. } | Grab::Knob { plugin, .. } | Grab::Point { plugin, .. } => {
                plugin
            }
        }
    }

    /// The point held: its plug-in, its graph and its index.
    fn point(&self) -> Option<(PluginId, usize, usize)> {
        match *self {
            Grab::Point {
                plugin, graph, i, ..
            } => Some((plugin, graph, i)),
            _ => None,
        }
    }
}

/// A part of a window: the title bar's seats first (UI-322), then the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The bar's drag handle: the name's band and the empty stretch.
    Head,
    Power,
    /// The tag «▾ name ●»: opens the preset list.
    Preset,
    /// The ‹ › stepping the presets (gone while the bar is narrow).
    PrevPreset,
    NextPreset,
    /// The side-chain seat: empty until UI-336 fills it.
    Sidechain,
    /// The channel editor's button: a seat until MIX-042 opens its window.
    Channel,
    Pin,
    Close,
    Knob(usize),
    /// The `i`th point of the `graph`th graph (UI-325).
    Point(usize, usize),
    /// A graph's plane away from its points: a press there does nothing.
    Graph(usize),
    Body,
}

/// The open windows, the last on top.
#[derive(Debug, Default)]
pub struct Windows {
    pub open: Vec<Win>,
    pub(crate) grab: Option<Grab>,
    /// The control the keys move (UI-324): its plug-in and its parameter.
    pub focus: Option<(PluginId, usize)>,
    /// The value being typed in a knob.
    pub entry: Option<crate::device::input::Entry>,
    /// A control the wheel or a controller's knob is turning (one step once
    /// it is still).
    pub(crate) sweep: Option<crate::device::input::Sweep>,
    /// Where the open instrument menu hangs, for the menu a row opens next.
    pub menu_at: Option<Rect>,
    /// The part under the pointer, while nothing is held (a point grows).
    pub hot: Option<(PluginId, Part)>,
    /// The point chosen on each device's graph: its graph and its index.
    pub chosen: HashMap<PluginId, (usize, usize)>,
    /// The preset each plug-in was last set to: the tag keeps its name and
    /// marks it while the knobs are off it (UI-332 keeps the list itself).
    base: HashMap<PluginId, &'static str>,
}

impl Windows {
    pub fn is_open(&self, id: PluginId) -> bool {
        self.open.iter().any(|w| w.plugin == id)
    }

    /// The value a control shows while it is dragged or swept.
    pub(crate) fn turning(&self, id: PluginId, k: usize) -> Option<f64> {
        match self.grab {
            Some(Grab::Knob {
                plugin,
                k: at,
                value,
                ..
            }) if plugin == id && at == k => Some(value),
            Some(Grab::Point { plugin, x, y, .. })
                if plugin == id && [x, y].iter().flatten().any(|(at, ..)| *at == k) =>
            {
                [x, y]
                    .into_iter()
                    .flatten()
                    .find(|(at, ..)| *at == k)
                    .map(|(.., now)| now)
            }
            _ => self
                .sweep
                .as_ref()
                .filter(|s| s.plugin == id && s.k == k)
                .map(|s| s.value),
        }
    }

    /// A device's values (0..1) as its window shows them: a control that is
    /// dragged or swept where it is turning to.
    pub(crate) fn shown(&self, plugin: &Plugin) -> Vec<f64> {
        let mut v = values(plugin);
        for (k, x) in v.iter_mut().enumerate() {
            if let Some(t) = self.turning(plugin.id, k) {
                *x = t;
            }
        }
        v
    }

    /// The point a graph draws as hot: the one held, else the one under the
    /// pointer.
    fn hot_point(&self, plugin: PluginId, n: usize) -> Option<usize> {
        match (self.grab, self.hot) {
            (Some(g), _) => g
                .point()
                .filter(|(id, graph, _)| (*id, *graph) == (plugin, n))
                .map(|(.., i)| i),
            (None, Some((id, Part::Point(graph, i)))) if (id, graph) == (plugin, n) => Some(i),
            _ => None,
        }
    }

    /// The `i`th window's frame on a screen of `size`.
    pub fn rect(&self, i: usize, size: (f32, f32)) -> Rect {
        let (w, h) = self.open[i].body;
        let h = title_bar::BAR_H + h;
        let (x, y) = self.open[i].at.unwrap_or((
            size.0 / 2.0 - w / 2.0 + i as f32 * 24.0,
            64.0 + i as f32 * 24.0,
        ));
        Rect::new(
            x.clamp(0.0, (size.0 - KEEP_W).max(0.0)),
            y.clamp(0.0, (size.1 - title_bar::BAR_H).max(0.0)),
            w,
            h,
        )
    }

    /// The top window under a point, and the part of it.
    pub fn at(&self, p: &Project, size: (f32, f32), x: f32, y: f32) -> Option<(usize, Part)> {
        (0..self.open.len()).rev().find_map(|i| {
            let r = self.rect(i, size);
            if !r.contains(x, y) {
                return None;
            }
            let w = self.open[i];
            let Some((track, plugin, def)) = live(p, &w) else {
                return Some((i, Part::Body));
            };
            if y < r.y + title_bar::BAR_H {
                let b = bar(r, track, plugin, def);
                let part = [
                    (b.power, Part::Power),
                    (b.prev, Part::PrevPreset),
                    (Some(b.tag), Part::Preset),
                    (b.next, Part::NextPreset),
                    (b.sidechain, Part::Sidechain),
                    (b.channel, Part::Channel),
                    (Some(b.pin), Part::Pin),
                    (Some(b.close), Part::Close),
                ]
                .into_iter()
                .find(|(q, _)| q.is_some_and(|q| q.contains(x, y)))
                .map_or(Part::Head, |(_, part)| part);
                return Some((i, part));
            }
            let planes = graphs(r, def);
            let now = self.shown(plugin);
            let point = planes.iter().enumerate().find_map(|(n, &plane)| {
                graph::point_at(&graph::points(def, n, plane, &now), x, y)
                    .map(|at| Part::Point(n, at))
            });
            let part = point
                .or_else(|| {
                    slots(r, def)
                        .into_iter()
                        .find(|s| s.hit(def).contains(x, y))
                        .map(|s| Part::Knob(s.k))
                })
                .or_else(|| {
                    planes
                        .iter()
                        .position(|g| g.contains(x, y))
                        .map(Part::Graph)
                })
                .unwrap_or(Part::Body);
            Some((i, part))
        })
    }

    /// Brings the `i`th window to the top; returns its new index.
    pub(crate) fn raise(&mut self, i: usize) -> usize {
        let w = self.open.remove(i);
        self.open.push(w);
        self.open.len() - 1
    }

    /// Forgets the windows whose plugin is gone or is no longer built in.
    fn prune(&mut self, p: &Project) {
        self.open.retain(|w| live(p, w).is_some());
        self.base.retain(|id, _| {
            p.tracks
                .values()
                .flat_map(|t| t.inserts.iter())
                .any(|x| x.id == *id)
        });
        self.learn(p);
        if self.grab.is_some_and(|g| !self.is_open(g.plugin())) {
            self.grab = None;
        }
        let open: Vec<PluginId> = self.open.iter().map(|w| w.plugin).collect();
        self.chosen.retain(|id, _| open.contains(id));
        if self.hot.is_some_and(|(id, _)| !open.contains(&id)) {
            self.hot = None;
        }
        graph::forget(|id| open.contains(&id));
        if self.focus.is_some_and(|(id, _)| !self.is_open(id)) {
            self.focus = None;
        }
        if self.entry.as_ref().is_some_and(|e| !self.is_open(e.plugin)) {
            self.entry = None;
        }
    }

    /// Learns the preset each built-in device's knobs are at as the one its
    /// tag keeps once they move off it, however they got there: a load from
    /// the tag, the CLI or the MCP, an undo, a song opened after a restart.
    pub(crate) fn learn(&mut self, p: &Project) {
        for x in p.tracks.values().flat_map(|t| &t.inserts) {
            match preset_of(x) {
                CUSTOM => {}
                name => {
                    self.base.insert(x.id, name);
                }
            }
        }
    }

    /// The tag's «name ●»: the preset the knobs are at, or the one they were
    /// last set to plus the mark while they are off it.
    pub(crate) fn tag_of(&self, plugin: &Plugin) -> (&'static str, bool) {
        match preset_of(plugin) {
            CUSTOM => (*self.base.get(&plugin.id).unwrap_or(&PRESETS[0]), true),
            name => (name, false),
        }
    }
}

/// The shared title bar over a window's frame (UI-322): the same part on
/// every built-in device's window, its seats following the device.
fn bar(r: Rect, track: &Track, plugin: &Plugin, def: &BuiltinDef) -> title_bar::Bar {
    let seats = title_bar::Seats {
        power: true,
        sidechain: title_bar::takes_sidechain(def),
        channel: true,
    };
    let head = Rect::new(r.x, r.y, r.w, title_bar::BAR_H);
    title_bar::layout(head, track.shown_name(), &plugin.name, seats)
}

/// Where a part sits in the window whose frame is `r`.
pub fn part_rect(r: Rect, track: &Track, plugin: &Plugin, def: &BuiltinDef, part: Part) -> Rect {
    let b = bar(r, track, plugin, def);
    match part {
        Part::Head => b.space,
        Part::Power => b.power.unwrap_or_default(),
        Part::Preset => b.tag,
        Part::PrevPreset => b.prev.unwrap_or_default(),
        Part::NextPreset => b.next.unwrap_or_default(),
        Part::Sidechain => b.sidechain.unwrap_or_default(),
        Part::Channel => b.channel.unwrap_or_default(),
        Part::Pin => b.pin,
        Part::Close => b.close,
        Part::Knob(k) => slots(r, def)
            .into_iter()
            .find(|s| s.k == k)
            .map(|s| s.hit(def))
            .unwrap_or_default(),
        Part::Point(n, i) => points_of(r, def, n, &values(plugin))
            .get(i)
            .map(|p| {
                let d = graph::POINT;
                Rect::new(p.at.0 - d / 2.0, p.at.1 - d / 2.0, d, d)
            })
            .unwrap_or_default(),
        Part::Graph(n) => graphs(r, def).get(n).copied().unwrap_or_default(),
        // The empty rest under the controls (a press in a gap is Body too;
        // the strip is where a point of it always is).
        Part::Body => Rect::new(r.x, r.bottom() - layout::PAD, r.w, layout::PAD),
    }
}

/// The planes of the graphs of the window whose frame is `r`, on the screen.
pub(crate) fn graphs(r: Rect, def: &BuiltinDef) -> Vec<Rect> {
    layout::body(def)
        .graphs
        .into_iter()
        .map(|g| Rect::new(r.x + g.x, r.y + title_bar::BAR_H + g.y, g.w, g.h))
        .collect()
}

/// The points of the `n`th graph of the window whose frame is `r`, for the
/// values (0..1) it shows.
pub(crate) fn points_of(r: Rect, def: &BuiltinDef, n: usize, now: &[f64]) -> Vec<Point> {
    graphs(r, def)
        .get(n)
        .map_or_else(Vec::new, |&g| graph::points(def, n, g, now))
}

/// The controls of the window whose frame is `r`, where they are on the screen.
pub(crate) fn slots(r: Rect, def: &BuiltinDef) -> Vec<Slot> {
    layout::body(def)
        .slots
        .into_iter()
        .map(|s| s.at(r.x, r.y + title_bar::BAR_H))
        .collect()
}

/// The track, the plugin and its parameters of a window, while it is still
/// there.
pub(crate) fn live<'a>(
    p: &'a Project,
    w: &Win,
) -> Option<(&'a Track, &'a Plugin, &'static BuiltinDef)> {
    let track = p.tracks.get(&w.track)?;
    let plugin = track.inserts.iter().find(|x| x.id == w.plugin)?;
    let def = plugin.builtin()?;
    Some((track, plugin, def))
}

impl Studio {
    /// The keys button of an instrument track, or its instrument's row:
    /// the window here for a built-in instrument (on top if it is open),
    /// the plug-in's own for the others, the instrument list when there is
    /// none yet.
    pub fn open_instrument(&mut self, t: TrackId, anchor: Rect) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let Some(plugin) = super::current(track) else {
            self.open_popup(PopupKind::Instrument(t), anchor);
            return true;
        };
        let id = plugin.id;
        if !is_built_in(plugin) {
            // Its window comes to the top when open, as a built-in's does.
            if self.editor_shown(id) {
                self.raise_editor(id);
                return true;
            }
            return self.open_plugin(t, id);
        }
        self.show_instrument(t, id);
        true
    }

    /// Opens the window of a built-in plugin, or brings it to the top.
    pub(crate) fn show_instrument(&mut self, t: TrackId, id: PluginId) {
        self.instruments.prune(&self.project());
        match self.instruments.open.iter().position(|w| w.plugin == id) {
            Some(i) => {
                self.instruments.raise(i);
            }
            None => {
                let p = self.project();
                let def = p
                    .tracks
                    .get(&t)
                    .and_then(|x| x.inserts.iter().find(|x| x.id == id))
                    .and_then(Plugin::builtin);
                if let Some(def) = def {
                    let body = layout::body(def);
                    self.instruments.open.push(Win {
                        track: t,
                        plugin: id,
                        knobs: def.params.len(),
                        body: (body.w, body.h),
                        at: None,
                        pinned: false,
                    });
                }
            }
        }
    }

    /// Is the track an instrument track?
    pub(crate) fn is_instrument(&self, t: TrackId) -> bool {
        self.project()
            .tracks
            .get(&t)
            .is_some_and(|x| x.kind == TrackKind::Instrument)
    }

    /// Is a window under the point?
    pub(crate) fn over_instrument(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.instruments.at(&self.project(), g.size, x, y).is_some()
    }

    /// A press: true when it was on a window (nothing under it gets it).
    pub fn instrument_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.instrument_press(g, x, y, 1, crate::tools::Mods::NONE)
    }

    /// A press with its count of clicks and its keys held (UI-324): a
    /// double click types a knob's value, Alt puts it back to its default.
    pub fn instrument_press(
        &mut self,
        g: &Geometry,
        x: f32,
        y: f32,
        clicks: u32,
        mods: crate::tools::Mods,
    ) -> bool {
        self.instruments.prune(&self.project());
        let p = self.project();
        let Some((i, part)) = self.instruments.at(&p, g.size, x, y) else {
            self.device_blur();
            return false;
        };
        if !matches!(part, Part::Knob(_) | Part::Point(..)) {
            self.device_blur();
        }
        self.error = None;
        let i = self.instruments.raise(i);
        let w = self.instruments.open[i];
        let r = self.instruments.rect(i, g.size);
        match part {
            Part::Close => {
                self.instruments.open.remove(i);
            }
            Part::Head => {
                self.instruments.grab = Some(Grab::Move {
                    plugin: w.plugin,
                    from: (r.x, r.y),
                    pointer: (x, y),
                });
            }
            Part::Power => {
                // The power of a switched-off plug-in turns it on again
                // (ENG-008), as the mixer's mark does.
                let off = live(&p, &w).is_some_and(|(_, x, _)| x.deactivated);
                let command = if off {
                    "plugin.activate"
                } else {
                    "plugin.bypass"
                };
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(command, json!({ "ids": [w.plugin.to_string()] }));
            }
            Part::Preset => {
                let Some((track, plugin, def)) = live(&p, &w) else {
                    return true;
                };
                let anchor = part_rect(r, track, plugin, def, Part::Preset);
                self.open_inst_menu(Menu::Preset(w.track, w.plugin), anchor);
            }
            // The press raised this window to the top, so the shared step
            // lands on it.
            Part::PrevPreset => {
                self.step_preset(false);
            }
            Part::NextPreset => {
                self.step_preset(true);
            }
            Part::Pin => {
                self.instruments.open[i].pinned = !w.pinned;
            }
            Part::Knob(k) => {
                let slot = live(&p, &w).and_then(|(_, _, d)| {
                    slots(r, d).into_iter().find(|s| s.k == k).map(|s| (s, d))
                });
                if let Some((slot, def)) = slot {
                    self.device_press(w, def, slot, x, y, clicks, mods);
                }
            }
            Part::Point(n, at) => {
                let Some((_, x0, def)) = live(&p, &w) else {
                    return true;
                };
                let now = self.instruments.shown(x0);
                if let Some(point) = points_of(r, def, n, &now).get(at) {
                    self.device_point_press(w, def, (n, at), point, (x, y), clicks, mods);
                }
            }
            Part::Graph(_) | Part::Body | Part::Sidechain | Part::Channel => {}
        }
        true
    }

    /// The knob under a point: its track, plug-in, parameter and rect.
    pub fn knob_under(
        &self,
        g: &Geometry,
        x: f32,
        y: f32,
    ) -> Option<(TrackId, PluginId, &'static str, Rect)> {
        let p = self.project();
        let (i, Part::Knob(k)) = self.instruments.at(&p, g.size, x, y)? else {
            return None;
        };
        let w = self.instruments.open[i];
        let (track, plugin, def) = live(&p, &w)?;
        let spec = def.params.get(k)?;
        let r = part_rect(
            self.instruments.rect(i, g.size),
            track,
            plugin,
            def,
            Part::Knob(k),
        );
        Some((w.track, w.plugin, spec.id, r))
    }

    /// The pointer moves while a window or a knob is held. `fine` turns slower.
    pub fn instrument_moved(&mut self, g: &Geometry, x: f32, y: f32, fine: bool) -> bool {
        match self.instruments.grab {
            Some(Grab::Move {
                plugin,
                from,
                pointer,
            }) => {
                let at = (
                    (from.0 + x - pointer.0).clamp(0.0, (g.size.0 - KEEP_W).max(0.0)),
                    (from.1 + y - pointer.1).clamp(0.0, (g.size.1 - title_bar::BAR_H).max(0.0)),
                );
                if let Some(w) = self
                    .instruments
                    .open
                    .iter_mut()
                    .find(|w| w.plugin == plugin)
                {
                    w.at = Some(at);
                }
                true
            }
            Some(Grab::Knob {
                plugin,
                k,
                start,
                y: y0,
                ..
            }) => {
                let p = self.project();
                let Some(w) = self
                    .instruments
                    .open
                    .iter()
                    .find(|w| w.plugin == plugin)
                    .copied()
                else {
                    return false;
                };
                let Some(spec) = live(&p, &w).and_then(|(_, _, d)| d.params.get(k)) else {
                    return false;
                };
                let next = control::dragged(spec, start, y - y0, fine);
                if let Some(Grab::Knob { value, .. }) = &mut self.instruments.grab {
                    if *value == next {
                        return true;
                    }
                    *value = next;
                }
                // Only the sound and the screen follow the drag; the song takes
                // the value once it is let go.
                self.device_hear(w.track, plugin, spec.id, next);
                true
            }
            Some(Grab::Point { .. }) => self.device_point_moved(g, x, y, fine),
            None => false,
        }
    }

    /// Lets go: a turned knob goes into the song as one step.
    pub fn instrument_up(&mut self) -> bool {
        let Some(grab) = self.instruments.grab.take() else {
            return false;
        };
        if let Grab::Knob {
            plugin, k, value, ..
        } = grab
        {
            let p = self.project();
            let w = self.instruments.open.iter().find(|w| w.plugin == plugin);
            let Some((_, x, def)) = w.and_then(|w| live(&p, w)) else {
                return true;
            };
            let (spec, before) = (&def.params[k], values(x)[k]);
            if (before - value).abs() > 1e-9 {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "plugin.param.set",
                    json!({ "id": plugin.to_string(), "param": spec.id, "value": value }),
                );
            }
        }
        if let Grab::Point { plugin, x, y, .. } = grab {
            // Both of a point's values go in as one step.
            let moved: Vec<(usize, f64)> = [x, y]
                .into_iter()
                .flatten()
                .map(|(k, _, now)| (k, now))
                .collect();
            self.device_set_all(plugin, &moved);
        }
        true
    }

    /// The pointer over the windows while nothing is held: the part under it,
    /// for a point to grow. True when that changed.
    pub fn instrument_hover(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        if self.instruments.grab.is_some() {
            return false;
        }
        let p = self.project();
        let hot = self
            .instruments
            .at(&p, g.size, x, y)
            .map(|(i, part)| (self.instruments.open[i].plugin, part));
        let changed = hot != self.instruments.hot;
        self.instruments.hot = hot;
        changed
    }

    /// Sets every knob a preset sets of a built-in plugin (one step). The tag
    /// keeps the preset's name while the knobs stay on it.
    pub fn load_preset(&mut self, id: PluginId, preset: &str) -> bool {
        let p = self.project();
        let Some(def) = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|x| x.id == id)
            .and_then(Plugin::builtin)
        else {
            return false;
        };
        let Some(v) = preset_values(def, preset) else {
            return false;
        };
        let values: Vec<_> = def
            .params
            .iter()
            .zip(v)
            .filter(|(p, _)| p.preset)
            .map(|(p, v)| json!({ "param": p.id, "value": v }))
            .collect();
        // The edit's own pass learns the preset as the tag's base.
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "plugin.preset.load",
            json!({ "id": id.to_string(), "preset": preset, "values": values }),
        )
    }

    /// The centre of a part of the window of `id`, for the tests.
    pub fn instrument_center(&self, g: &Geometry, id: PluginId, part: Part) -> Option<(f32, f32)> {
        let i = self.instruments.open.iter().position(|w| w.plugin == id)?;
        let p = self.project();
        let (track, plugin, def) = live(&p, &self.instruments.open[i])?;
        let r = part_rect(self.instruments.rect(i, g.size), track, plugin, def, part);
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        // Only if nothing covers it.
        (self.instruments.at(&p, g.size, at.0, at.1) == Some((i, part))).then_some(at)
    }
}

/// The open windows, the last on top.
pub fn paint(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let p = studio.project();
    for (i, w) in studio.instruments.open.iter().enumerate() {
        let Some((track, plugin, def)) = live(&p, w) else {
            continue;
        };
        let r = studio.instruments.rect(i, g.size);
        paint_window(pen, studio, r, *w, track, plugin, def);
    }
}

fn paint_window(
    pen: &mut Pen,
    studio: &Studio,
    r: Rect,
    w: Win,
    track: &Track,
    plugin: &Plugin,
    def: &BuiltinDef,
) {
    let c = pen.c;
    // A floating panel: the soft shadow all round it (UI-007), then its face.
    let at = Bounds::new(
        point(px(pen.origin.0 + r.x), px(pen.origin.1 + r.y)),
        size(px(r.w), px(r.h)),
    );
    pen.window
        .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &c.float);
    pen.round(r, c.paper, Radius::M);
    pen.mask(r, |pen| {
        // The shared title bar (UI-322): the same part on every window.
        let (preset, modified) = studio.instruments.tag_of(plugin);
        title_bar::paint(
            pen,
            Rect::new(r.x, r.y, r.w, title_bar::BAR_H),
            &title_bar::View {
                track: track.shown_name(),
                device: &plugin.name,
                icon: match plugin.kind {
                    PluginKind::Instrument => "piano",
                    _ => "ins",
                },
                powered: plugin.runs(),
                preset,
                modified,
                pinned: w.pinned,
                seats: title_bar::Seats {
                    power: true,
                    sidechain: title_bar::takes_sidechain(def),
                    channel: true,
                },
            },
        );

        // The graphs (UI-325), then the controls, where the device's layout
        // puts them.
        let now = values(plugin);
        let ins = &studio.instruments;
        let shown = ins.shown(plugin);
        for (n, plane) in graphs(r, def).into_iter().enumerate() {
            let chosen = ins.chosen.get(&plugin.id).filter(|c| c.0 == n).map(|c| c.1);
            let focused = ins.focus.is_some_and(|(id, k)| {
                id == plugin.id
                    && chosen
                        .and_then(|c| points_of(r, def, n, &shown).get(c).cloned())
                        .is_some_and(|pt| pt.params().any(|x| x == k))
            });
            let view = graph::View {
                plugin: Some(plugin.id),
                chosen,
                hot: ins.hot_point(plugin.id, n),
                held: ins
                    .grab
                    .and_then(|g| g.point())
                    .is_some_and(|(id, graph, _)| (id, graph) == (plugin.id, n)),
                focused,
            };
            graph::paint(pen, def, n, plane, &shown, &view);
        }
        let entry = ins.entry.as_ref().filter(|e| e.plugin == plugin.id);
        for slot in slots(r, def) {
            let k = slot.k;
            let spec = &def.params[k];
            let v = ins.turning(plugin.id, k).unwrap_or(now[k]);
            let focused = ins.focus == Some((plugin.id, k));
            let name = control::name(def, spec);
            let hit = slot.hit(def);
            match slot.look {
                Look::Knob => control::paint_knob(
                    pen,
                    slot.cell,
                    &KnobView {
                        spec,
                        name,
                        value: v,
                        dial: slot.dial,
                        entry: entry.filter(|e| e.k == k).map(|e| &e.field),
                        focused,
                    },
                ),
                Look::Segments => {
                    control::paint_segments(pen, hit, spec, v, focused);
                    control::paint_name(pen, slot.cell, slot.dial, name);
                }
                Look::Toggle => control::paint_toggle(pen, hit, name, v >= 0.5, focused),
            }
            let mark = match slot.look {
                Look::Knob => control::dial_rect(slot.cell, slot.dial),
                _ => hit,
            };
            crate::auto_mark::paint(
                pen,
                track,
                xirius_core::auto::AutoTarget::Param(plugin.id, spec.id.to_string()),
                mark,
            );
        }
    });
}
