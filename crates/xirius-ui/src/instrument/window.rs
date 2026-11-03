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
//! ways a control moves are `device::input`'s (UI-324). A preset sets the device as its
//! file holds it, the knobs and the samples (`preset.load`, one step).

use std::collections::HashMap;

use gpui::{Bounds, Corners, point, px, size};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::NoteKind;
use xirius_params::{BuiltinDef, ParamSpec};

use super::menu::Menu;
use crate::context::{Act, Item};
use crate::device::control::{self, KnobView, Look};
use crate::device::delay;
use crate::device::dynamics::{self, Dynamics};
use crate::device::eq;
use crate::device::graph::{self, Point};
use crate::device::layout::{self, Body, Slot};
use crate::device::play;
use crate::device::sampler;
use crate::draw::{Align, Pen};
use crate::mixer::meters::Meter;
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar;

/// How much of a window stays on the screen when it is dragged off.
const KEEP_W: f32 = 80.0;

/// The preset that is the device's own defaults (PLG-063).
pub const DEFAULT: &str = xirius_core::xpreset::DEFAULT_PRESET;

/// A built-in's presets: its Default first, then its factory presets (PLG-064). The
/// Sampler and the Drum Kit have only their Default.
pub fn presets(def: &BuiltinDef) -> Vec<&'static str> {
    let mut names = xirius_params::factory_names(def);
    // By name, as the session lists the files.
    names.sort_unstable();
    let mut v = vec![DEFAULT];
    v.extend(names);
    v
}

/// The knobs' values (0..1) of a preset: [`DEFAULT`] is the defaults, the rest are the
/// factory presets `xirius-params` ships (PLG-064), which leave the knobs they do not
/// name at their defaults.
pub fn preset_values(def: &BuiltinDef, preset: &str) -> Option<Vec<f64>> {
    if preset == DEFAULT {
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

/// The values (0..1) of `def`'s parameters as `plugin` keeps them, else their
/// defaults: a Sampler's zone's and a Drum Kit's pad's under their keys (UI-333,
/// UI-334), as [`sampler::scoped`] gives them.
pub fn values_of(plugin: &Plugin, def: &BuiltinDef) -> Vec<f64> {
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
}

/// The preset of the factory set (`Default` among them) the knobs a preset
/// sets are at, if any. The window's tag reads the files instead
/// ([`super::presets`]).
pub fn preset_of(plugin: &Plugin) -> Option<&'static str> {
    let def = plugin.builtin()?;
    let now = values(plugin);
    presets(def).into_iter().find(|p| {
        preset_values(def, p).is_some_and(|v| {
            def.params
                .iter()
                .zip(v.iter().zip(&now))
                .all(|(s, (a, b))| !s.preset || (a - b).abs() < 1e-6)
        })
    })
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
    /// The pin is down: the window stays when another device opens. An
    /// unpinned one takes the next device in (UI-323).
    pub pinned: bool,
    /// Its place in the cascade while it was not dragged: the count of
    /// windows open when it came, so a window keeps its place when another
    /// device fills it.
    pub seat: usize,
    /// The width its first device centred it by: the corner stays put
    /// when a wider or narrower device fills it.
    pub placed_w: f32,
    /// A Sampler's zone (its file) or a Drum Kit's pad the window shows, and the
    /// pad's layer (UI-333, UI-334).
    pub pick: usize,
    pub layer: usize,
    /// It had samples when it was last laid out: it takes its size again once its
    /// files come or all go.
    pub filled: bool,
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
    /// The corner (UI-331): the body's size when it was taken.
    Size {
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
    /// A key or a pad (PLG-066): its note sounds while it is held and the pointer stays on it.
    Note { plugin: PluginId, pitch: u8 },
}

impl Grab {
    fn plugin(&self) -> PluginId {
        match *self {
            Grab::Move { plugin, .. }
            | Grab::Size { plugin, .. }
            | Grab::Knob { plugin, .. }
            | Grab::Point { plugin, .. }
            | Grab::Note { plugin, .. } => plugin,
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
    /// The bottom right corner: the window's size (UI-331).
    Resize,
    Knob(usize),
    /// The `i`th point of the `graph`th graph (UI-325).
    Point(usize, usize),
    /// A graph's plane away from its points: a press there does nothing.
    Graph(usize),
    /// A tab of a narrow EQ choosing the band it shows (UI-326).
    Tab(usize),
    /// The button choosing what an EQ's analyser shows.
    Spectrum,
    /// The readout under a dynamics device's `i`th meter: a press clears
    /// the most it shows (UI-327).
    Readout(usize),
    /// A key of a Sampler's keyboard and a pad of a Drum Kit, by the note it plays (PLG-066).
    Key(u8),
    Pad(u8),
    /// A Sampler's or a Drum Kit's part that is not a parameter (UI-333, UI-334): a
    /// zone, a bank, a layer, a pad's M and S, the ‹ › of a sample, Browse.
    Spot(usize),
    Body,
}

/// The note a window's key or pad sounds (PLG-066).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Held {
    pub plugin: PluginId,
    pub track: TrackId,
    /// The key's own note, as it lights.
    pub pitch: u8,
    /// The note sent, after the track's transpose: `None` when that took it off the keyboard.
    pub heard: Option<u8>,
}

/// What an EQ's analyser shows and last heard (UI-326, ENG-073).
#[derive(Debug, Default)]
pub(crate) struct Analyser {
    pub tap: xirius_engine::stock::Tap,
    /// The samples gathered since the last look, the input's and the output's.
    gathered: [Vec<f32>; 2],
    /// The levels (dB at log-spaced frequencies) last worked out.
    pub levels: [Vec<f32>; 2],
    /// When they were.
    last: Option<std::time::Instant>,
}

impl Analyser {
    /// How often the levels are worked out again.
    const EVERY: std::time::Duration = std::time::Duration::from_millis(33);
    /// How many levels a line has.
    const POINTS: usize = 200;

    /// Takes what the probe gathered and, 30 times a second, works the
    /// levels out from the last of it. False for a device with no analyser.
    pub(crate) fn feed(&mut self, probe: &xirius_engine::stock::Probe, rate: f64) -> bool {
        use xirius_engine::analyzer;
        probe.set_tap(self.tap);
        let [input, output] = &mut self.gathered;
        if !probe.take_samples(input, output) {
            return false;
        }
        for buf in &mut self.gathered {
            let extra = buf.len().saturating_sub(analyzer::FFT_LEN);
            buf.drain(..extra);
        }
        let now = std::time::Instant::now();
        if self.last.is_some_and(|t| now - t < Self::EVERY) {
            return true;
        }
        self.last = Some(now);
        let wanted = [self.tap.input(), self.tap.output()];
        for ((levels, buf), on) in self.levels.iter_mut().zip(&self.gathered).zip(wanted) {
            *levels = if on && !buf.is_empty() {
                analyzer::spectrum(buf, rate, Self::POINTS)
            } else {
                Vec::new()
            };
        }
        true
    }
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
    /// Each open EQ's analyser.
    pub(crate) analysers: HashMap<PluginId, Analyser>,
    /// Each open window's output meter (ENG-072).
    pub(crate) meters: HashMap<PluginId, Meter>,
    /// Each open dynamics device's levels and their history (UI-327).
    pub(crate) dynamics: HashMap<PluginId, Dynamics>,
    /// Each open Delay's or Reverb's onsets: its repeats or reflections
    /// light from the last (UI-329, UI-330).
    pub(crate) echoes: HashMap<PluginId, delay::Echo>,
    /// The devices whose probes write for a window, so a closed one stops.
    watched: Vec<PluginId>,
    /// The preset files the tags and their lists read (UI-332).
    pub(crate) presets: super::presets::Presets,
    /// The window a device last opened in and when: its frame lights a
    /// moment so the eye finds it (UI-323).
    pub(crate) flash: Option<(PluginId, std::time::Instant)>,
    /// The body's size each kind of device was last left at: its next
    /// window opens so (UI-331).
    pub(crate) sizes: HashMap<&'static str, (f32, f32)>,
    /// The note a key or a pad sounds (PLG-066).
    pub(crate) note: Option<Held>,
}

/// How long an opened window's frame lights.
pub const FLASH: std::time::Duration = std::time::Duration::from_millis(500);
/// How wide the ring of an opened window is.
const FLASH_W: f32 = 3.0;
/// How far each new window sits from the one before it.
pub const CASCADE: f32 = 28.0;
/// The square in the bottom right corner that takes the window's size.
pub const GRIP: f32 = 14.0;

/// The corner of the window whose frame is `r` that takes its size.
pub fn grip(r: Rect) -> Rect {
    Rect::new(r.right() - GRIP, r.bottom() - GRIP, GRIP, GRIP)
}

/// The body's size of `def` at about `want`: no narrower than the device
/// goes, no shorter than its controls need at that width (UI-331).
pub fn fit(def: &BuiltinDef, want: (f32, f32)) -> (f32, f32) {
    fit_of(def, want, None)
}

/// [`fit`] for `plugin`: a Sampler's and a Drum Kit's need follows their files.
pub fn fit_of(def: &BuiltinDef, want: (f32, f32), plugin: Option<&Plugin>) -> (f32, f32) {
    let (w, h) = layout::size_of(def, want.0.max(layout::min_w(def)), plugin);
    (w, want.1.max(h))
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

    /// What is turning now, not yet in the song (M-ui-1): a knob dragged or swept, both
    /// values of a point dragged. Each is its plug-in, the index in its window's parameters
    /// and the value (0..1).
    pub(crate) fn turned(&self) -> Vec<(PluginId, usize, f64)> {
        let mut out = Vec::new();
        match self.grab {
            Some(Grab::Knob {
                plugin, k, value, ..
            }) => out.push((plugin, k, value)),
            Some(Grab::Point { plugin, x, y, .. }) => {
                out.extend(
                    [x, y]
                        .into_iter()
                        .flatten()
                        .map(|(k, _, now)| (plugin, k, now)),
                );
            }
            _ => {}
        }
        if let Some(s) = &self.sweep {
            out.push((s.plugin, s.k, s.value));
        }
        out
    }

    /// A device's values (0..1) as its window shows them: a control that is
    /// dragged or swept where it is turning to.
    pub(crate) fn shown(&self, plugin: &Plugin) -> Vec<f64> {
        let mut v = self
            .def_of(plugin)
            .map_or_else(Vec::new, |def| values_of(plugin, def));
        for (k, x) in v.iter_mut().enumerate() {
            if let Some(t) = self.turning(plugin.id, k) {
                *x = t;
            }
        }
        v
    }

    /// The window of `plugin`, if it is open.
    pub(crate) fn win_of(&self, id: PluginId) -> Option<&Win> {
        self.open.iter().find(|w| w.plugin == id)
    }

    /// The parameters the window of `plugin` shows: a Sampler's zone's, a Drum Kit's
    /// pad's (UI-333, UI-334), else the device's.
    pub(crate) fn def_of(&self, plugin: &Plugin) -> Option<&'static BuiltinDef> {
        sampler::scoped(plugin, self.win_of(plugin.id).map_or(0, |w| w.pick))
    }

    /// The band an EQ's window shows (the point chosen on its graph).
    pub fn band(&self, id: PluginId) -> usize {
        self.chosen
            .get(&id)
            .filter(|c| c.0 == 0)
            .map_or(eq::FIRST_BAND, |c| c.1.min(eq::NAMES.len() - 1))
    }

    /// What an EQ's analyser shows.
    pub fn tap(&self, id: PluginId) -> xirius_engine::stock::Tap {
        self.analysers.get(&id).map(|a| a.tap).unwrap_or_default()
    }

    /// Chooses what the analyser of an open EQ's window shows.
    pub fn set_tap(&mut self, id: PluginId, tap: xirius_engine::stock::Tap) -> bool {
        if !self.open.iter().any(|w| w.plugin == id) {
            return false;
        }
        let a = self.analysers.entry(id).or_default();
        if a.tap != tap {
            a.tap = tap;
            a.levels = Default::default();
        }
        true
    }

    /// The body of the window whose frame is `r`, on the screen, for the
    /// values it shows and the band chosen.
    pub(crate) fn body(&self, r: Rect, plugin: &Plugin, def: &BuiltinDef) -> Body {
        let w = self.win_of(plugin.id);
        let mut b = body_of(
            r,
            def,
            &self.shown(plugin),
            self.band(plugin.id),
            Some(plugin),
            w.map_or((0, 0), |w| (w.pick, w.layer)),
        );
        // The Compressor's Gain drives its key while a sidechain feeds it,
        // else its input (PLG-071).
        if def.uid == "compressor"
            && let Some((k, _)) = def.param("gain")
            && let Some(slot) = b.slots.iter_mut().find(|s| s.k == k)
        {
            let side = plugin.sidechain.as_ref().is_some_and(|s| s.on);
            slot.label = Some(if side { "Side Gain" } else { "Input Gain" });
        }
        b
    }

    /// The controls of the window whose frame is `r`, on the screen.
    pub(crate) fn slots(&self, r: Rect, plugin: &Plugin, def: &BuiltinDef) -> Vec<Slot> {
        self.body(r, plugin, def).slots
    }

    /// The points of the `n`th graph of the window whose frame is `r`.
    pub(crate) fn points_of(
        &self,
        r: Rect,
        plugin: &Plugin,
        def: &BuiltinDef,
        n: usize,
    ) -> Vec<Point> {
        let now = self.shown(plugin);
        self.body(r, plugin, def)
            .graphs
            .get(n)
            .map_or_else(Vec::new, |&g| graph::points(def, n, g, &now))
    }

    /// The analyser's button of an EQ's window whose body is `b`.
    fn spectrum_of(&self, b: &Body, id: PluginId) -> Option<Rect> {
        b.spectrum.map(|at| eq::spectrum_rect(at, self.tap(id)))
    }

    /// Where a part sits in the window whose frame is `r`.
    pub fn part_rect(
        &self,
        r: Rect,
        track: &Track,
        plugin: &Plugin,
        def: &BuiltinDef,
        part: Part,
    ) -> Rect {
        let bar = bar(r, track, plugin, def);
        let body = || self.body(r, plugin, def);
        match part {
            Part::Head => bar.space,
            Part::Power => bar.power.unwrap_or_default(),
            Part::Preset => bar.tag,
            Part::PrevPreset => bar.prev.unwrap_or_default(),
            Part::NextPreset => bar.next.unwrap_or_default(),
            Part::Sidechain => bar.sidechain.unwrap_or_default(),
            Part::Channel => bar.channel.unwrap_or_default(),
            Part::Pin => bar.pin,
            Part::Close => bar.close,
            Part::Resize => grip(r),
            Part::Knob(k) => body().slot(k).map(|s| s.hit(def)).unwrap_or_default(),
            Part::Point(n, i) => self
                .points_of(r, plugin, def, n)
                .get(i)
                .map(|p| {
                    let d = graph::POINT;
                    Rect::new(p.at.0 - d / 2.0, p.at.1 - d / 2.0, d, d)
                })
                .unwrap_or_default(),
            Part::Graph(n) => body().graphs.get(n).copied().unwrap_or_default(),
            Part::Tab(t) => body().tabs.get(t).map(|t| t.r).unwrap_or_default(),
            Part::Spectrum => self.spectrum_of(&body(), plugin.id).unwrap_or_default(),
            Part::Readout(i) => body().levels.get(i).map(|l| l.readout).unwrap_or_default(),
            Part::Key(pitch) => key_rect(&body().keys, pitch),
            Part::Pad(pitch) => key_rect(&body().pads, pitch),
            Part::Spot(n) => body().spots.get(n).map(|s| s.r).unwrap_or_default(),
            // The empty rest under the controls (a press in a gap is Body
            // too; the strip is where a point of it always is).
            Part::Body => Rect::new(r.x, r.bottom() - layout::PAD, r.w, layout::PAD),
        }
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
        let win = self.open[i];
        let n = win.seat as f32;
        let (x, y) = win.at.unwrap_or((
            size.0 / 2.0 - win.placed_w / 2.0 + n * CASCADE,
            64.0 + n * CASCADE,
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
            if grip(r).contains(x, y) {
                return Some((i, Part::Resize));
            }
            let now = self.shown(plugin);
            let b = self.body(r, plugin, def);
            if self
                .spectrum_of(&b, plugin.id)
                .is_some_and(|s| s.contains(x, y))
            {
                return Some((i, Part::Spectrum));
            }
            if let Some(n) = sampler::spot_at(&b, x, y) {
                return Some((i, Part::Spot(n)));
            }
            if let Some(pitch) = play::key_at(&b.keys, x, y) {
                return Some((i, Part::Key(pitch)));
            }
            if let Some(pitch) = play::key_at(&b.pads, x, y) {
                return Some((i, Part::Pad(pitch)));
            }
            let planes = &b.graphs;
            let point = planes.iter().enumerate().find_map(|(n, &plane)| {
                graph::point_at(&graph::points(def, n, plane, &now), x, y)
                    .map(|at| Part::Point(n, at))
            });
            let part = point
                .or_else(|| {
                    b.slots
                        .iter()
                        .find(|s| s.hit(def).contains(x, y))
                        .map(|s| Part::Knob(s.k))
                })
                .or_else(|| {
                    b.tabs
                        .iter()
                        .position(|t| t.r.contains(x, y))
                        .map(Part::Tab)
                })
                .or_else(|| {
                    b.levels
                        .iter()
                        .position(|l| l.readout.contains(x, y))
                        .map(Part::Readout)
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

    /// Is a window's corner held, or the pointer on it (UI-331)?
    pub fn sizing(&self) -> bool {
        match self.grab {
            Some(g) => matches!(g, Grab::Size { .. }),
            None => self.hot.is_some_and(|(_, part)| part == Part::Resize),
        }
    }

    /// How lit the frame of `id` is at `now`: 1 as it opens, 0 after
    /// [`FLASH`].
    pub fn lit(&self, id: PluginId, now: std::time::Instant) -> f32 {
        match self.flash {
            Some((at, since)) if at == id => {
                let t = now.saturating_duration_since(since).as_secs_f32() / FLASH.as_secs_f32();
                (1.0 - t.min(1.0)).powi(2)
            }
            _ => 0.0,
        }
    }

    /// Brings the `i`th window to the top; returns its new index.
    pub(crate) fn raise(&mut self, i: usize) -> usize {
        let w = self.open.remove(i);
        self.open.push(w);
        self.open.len() - 1
    }

    /// Another song is open (M-par-2): its devices' ids are counted afresh, so nothing a
    /// window held (a grab, a sweep, a typed value, a note) or showed may reach one of the
    /// new song's. The windows close; the preset list and the sizes stay.
    pub(crate) fn forget_song(&mut self) {
        let presets = std::mem::take(&mut self.presets);
        let sizes = std::mem::take(&mut self.sizes);
        *self = Windows {
            presets,
            sizes,
            ..Windows::default()
        };
        graph::forget(|_| false);
    }

    /// Forgets the windows whose plugin is gone or is no longer built in.
    fn prune(&mut self, p: &Project) {
        self.open.retain(|w| live(p, w).is_some());
        self.refit(p);
        if self.grab.is_some_and(|g| !self.is_open(g.plugin())) {
            self.grab = None;
        }
        let open: Vec<PluginId> = self.open.iter().map(|w| w.plugin).collect();
        self.chosen.retain(|id, _| open.contains(id));
        self.analysers.retain(|id, _| open.contains(id));
        self.meters.retain(|id, _| open.contains(id));
        self.dynamics.retain(|id, _| open.contains(id));
        self.echoes.retain(|id, _| open.contains(id));
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

    /// A Sampler's or a Drum Kit's window whose files came or all went takes the size
    /// its body now needs, and its zone or pad comes back to the first (UI-333, UI-334).
    /// One whose files were taken away keeps to the ones still there.
    pub(crate) fn refit(&mut self, p: &Project) {
        for n in 0..self.open.len() {
            let w = self.open[n];
            let Some((_, plugin, def)) = live(p, &w) else {
                continue;
            };
            if !sampler::has_files(def) {
                continue;
            }
            let filled = !plugin.samples.is_empty();
            if filled != w.filled {
                let body = layout::size_of(def, w.body.0, Some(plugin));
                self.open[n] = Win {
                    body,
                    pick: 0,
                    layer: 0,
                    filled,
                    ..w
                };
                if self.focus.is_some_and(|(id, _)| id == w.plugin) {
                    self.focus = None;
                }
                self.chosen.remove(&w.plugin);
            } else if def.uid == "sampler" && w.pick >= plugin.samples.len().max(1) {
                self.open[n].pick = plugin.samples.len().saturating_sub(1);
            }
        }
    }

    /// Shows the zone (a Sampler's file) or the pad (a Drum Kit's) `pick` in the window
    /// of `id`, and its `layer`. What was chosen and focused there was the other one's.
    pub(crate) fn pick(&mut self, id: PluginId, pick: usize, layer: usize) -> bool {
        let Some(w) = self.open.iter_mut().find(|w| w.plugin == id) else {
            return false;
        };
        if (w.pick, w.layer) == (pick, layer) {
            return false;
        }
        let moved = w.pick != pick;
        w.pick = pick;
        w.layer = layer;
        if moved {
            if self.focus.is_some_and(|(at, _)| at == id) {
                self.focus = None;
            }
            self.entry = self.entry.take().filter(|e| e.plugin != id);
            self.chosen.remove(&id);
        }
        true
    }

    /// One frame of the probes `probe_of` finds: see [`Studio::poll_probes`].
    pub(crate) fn poll_with(
        &mut self,
        p: &Project,
        rate: f64,
        now: std::time::Instant,
        probe_of: impl Fn(PluginId) -> Option<std::sync::Arc<xirius_engine::stock::Probe>>,
    ) -> bool {
        let open: Vec<PluginId> = self.open.iter().map(|w| w.plugin).collect();
        for id in self.watched.iter().filter(|id| !open.contains(id)) {
            if let Some(probe) = probe_of(*id) {
                probe.watch(false);
            }
        }
        self.watched.clear();
        let mut moving = false;
        for w in self.open.clone() {
            let id = w.plugin;
            let Some(probe) = probe_of(id) else {
                continue;
            };
            probe.watch(true);
            self.watched.push(id);
            let (input, out) = probe.take_peaks();
            let m = self.meters.entry(id).or_default();
            m.step(xirius_engine::mixer::MeterReading {
                peak: out,
                rms: out.map(|v| v * std::f32::consts::FRAC_1_SQRT_2),
            });
            moving |= m.level.iter().chain(&m.hold).any(|v| *v > 0.0);
            if let Some((span, pre)) = live(p, &w)
                .and_then(|(_, x, d)| Some((dynamics::span(d)?, dynamics::pre(d, &values(x)))))
            {
                let d = self.dynamics.entry(id).or_default();
                d.feed((input.map(|v| v * pre), out), probe.gr_db(), now, span);
                moving |= d.moving();
            }
            if live(p, &w).is_some_and(|(_, _, d)| matches!(d.uid, "delay" | "reverb")) {
                let e = self.echoes.entry(id).or_default();
                moving |= e.feed(probe.onsets(), now);
            }
            let a = self.analysers.entry(id).or_default();
            if a.feed(&probe, rate) {
                moving |= a.levels.iter().any(|l| !l.is_empty());
            }
        }
        moving
    }

    /// The tag's «name ●» (see [`super::presets`]).
    pub(crate) fn tag_of(&self, plugin: &Plugin) -> (String, bool) {
        self.presets.tag_of(plugin)
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

/// Where the key or pad playing `pitch` is.
fn key_rect(keys: &[play::Key], pitch: u8) -> Rect {
    keys.iter()
        .find(|k| k.pitch == pitch)
        .map(|k| k.r)
        .unwrap_or_default()
}

/// The body of the window whose frame is `r`, on the screen, for the values
/// (0..1) it shows and the band chosen (an EQ's).
pub(crate) fn body_of(
    r: Rect,
    def: &BuiltinDef,
    now: &[f64],
    band: usize,
    plugin: Option<&Plugin>,
    (pick, layer): (usize, usize),
) -> Body {
    layout::body(
        def,
        &layout::Shape {
            w: r.w,
            band,
            values: now,
            plugin,
            pick,
            layer,
        },
    )
    .grown(r.h - title_bar::BAR_H)
    .at(r.x, r.y + title_bar::BAR_H)
}

/// The track, the plugin and its parameters of a window, while it is still
/// there.
pub(crate) fn live<'a>(
    p: &'a Project,
    w: &Win,
) -> Option<(&'a Track, &'a Plugin, &'static BuiltinDef)> {
    let track = p.tracks.get(&w.track)?;
    let plugin = track.inserts.iter().find(|x| x.id == w.plugin)?;
    let def = sampler::scoped(plugin, w.pick)?;
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

    /// Opens the window of a built-in plugin (UI-323): an open one comes to
    /// the top; else the unpinned window used last takes it in; else, all
    /// pinned, a new window opens a step on. Its frame lights either way.
    pub(crate) fn show_instrument(&mut self, t: TrackId, id: PluginId) {
        let p = self.project();
        self.instruments.prune(&p);
        let ins = &mut self.instruments;
        match ins.open.iter().position(|w| w.plugin == id) {
            Some(i) => {
                ins.raise(i);
            }
            None => {
                let plugin = p
                    .tracks
                    .get(&t)
                    .and_then(|x| x.inserts.iter().find(|x| x.id == id));
                let Some((plugin, def)) = plugin.and_then(|x| Some((x, x.builtin()?))) else {
                    return;
                };
                // The size this kind was last left at, else its own.
                let body = match ins.sizes.get(def.uid) {
                    Some(&want) => fit_of(def, want, Some(plugin)),
                    None => layout::size_of(def, layout::natural_w(def), Some(plugin)),
                };
                let win = Win {
                    track: t,
                    plugin: id,
                    knobs: def.params.len(),
                    body,
                    // Centred as the others were, so it sits a step right
                    // of them as well as a step down.
                    placed_w: ins
                        .open
                        .iter()
                        .rev()
                        .find(|w| w.at.is_none())
                        .map_or(body.0, |w| w.placed_w),
                    at: None,
                    pinned: false,
                    // The first place in the cascade no window sits at.
                    seat: (0..)
                        .find(|n| !ins.open.iter().any(|w| w.seat == *n && w.at.is_none()))
                        .unwrap_or(0),
                    pick: 0,
                    layer: 0,
                    filled: !plugin.samples.is_empty(),
                };
                // The windows are in the order they were last used.
                match ins.open.iter().rposition(|w| !w.pinned) {
                    Some(i) => {
                        let old = ins.open[i];
                        ins.open[i] = Win {
                            at: old.at,
                            seat: old.seat,
                            placed_w: old.placed_w,
                            ..win
                        };
                        ins.raise(i);
                        ins.prune(&p);
                    }
                    None => ins.open.push(win),
                }
            }
        }
        ins.flash = Some((id, std::time::Instant::now()));
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
                if self.instruments.note.is_some_and(|n| n.plugin == w.plugin) {
                    self.window_note_off();
                }
                self.instruments.open.remove(i);
            }
            Part::Key(pitch) => {
                self.window_note_on(w, pitch);
            }
            Part::Pad(pitch) => {
                // A press on a pad chooses it too (UI-334).
                if live(&p, &w).is_some_and(|(_, x, _)| !x.samples.is_empty()) {
                    self.instruments.pick(w.plugin, play::pad_of(pitch), 0);
                }
                let w = self.instruments.open[i];
                self.window_note_on(w, pitch);
            }
            Part::Spot(n) => {
                self.window_spot(i, r, n);
            }
            Part::Head => {
                self.instruments.grab = Some(Grab::Move {
                    plugin: w.plugin,
                    from: (r.x, r.y),
                    pointer: (x, y),
                });
            }
            Part::Resize => {
                self.instruments.grab = Some(Grab::Size {
                    plugin: w.plugin,
                    from: w.body,
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
                let anchor = self
                    .instruments
                    .part_rect(r, track, plugin, def, Part::Preset);
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
                let slot = live(&p, &w).and_then(|(_, x0, d)| {
                    self.instruments
                        .slots(r, x0, d)
                        .into_iter()
                        .find(|s| s.k == k)
                        .map(|s| (s, d))
                });
                if let Some((slot, def)) = slot {
                    // A press in a band's column chooses the band (UI-326).
                    if def.uid == "eq"
                        && let Some(band) = eq::band_of(def, k)
                    {
                        self.instruments.chosen.insert(w.plugin, (0, band));
                    }
                    self.device_press(w, def, slot, x, y, clicks, mods);
                }
            }
            Part::Point(n, at) => {
                let Some((_, x0, def)) = live(&p, &w) else {
                    return true;
                };
                let point = self.instruments.points_of(r, x0, def, n).get(at).cloned();
                if let Some(point) = point {
                    self.device_point_press(w, def, (n, at), &point, (x, y), clicks, mods);
                }
            }
            Part::Tab(band) => {
                self.instruments.chosen.insert(w.plugin, (0, band));
            }
            Part::Spectrum => {
                let Some((track, plugin, def)) = live(&p, &w) else {
                    return true;
                };
                let at = self
                    .instruments
                    .part_rect(r, track, plugin, def, Part::Spectrum);
                self.spectrum_menu(w.plugin, at);
            }
            Part::Readout(at) => {
                let kind = live(&p, &w).and_then(|(_, x0, d)| {
                    self.instruments
                        .body(r, x0, d)
                        .levels
                        .get(at)
                        .map(|l| l.kind)
                });
                if let (Some(kind), Some(d)) = (kind, self.instruments.dynamics.get_mut(&w.plugin))
                {
                    d.clear(dynamics::most_of(kind));
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
        let r = self.instruments.part_rect(
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
            Some(Grab::Size {
                plugin,
                from,
                pointer,
            }) => {
                let p = self.project();
                let Some(i) = self
                    .instruments
                    .open
                    .iter()
                    .position(|w| w.plugin == plugin)
                else {
                    return false;
                };
                let w = self.instruments.open[i];
                let Some((_, plugin, def)) = live(&p, &w) else {
                    return false;
                };
                // The frame stays where it is: its corner keeps on the screen.
                let r = self.instruments.rect(i, g.size);
                let want = (
                    (from.0 + x - pointer.0).min(g.size.0 - r.x),
                    (from.1 + y - pointer.1).min(g.size.1 - r.y - title_bar::BAR_H),
                );
                let body = fit_of(def, want, Some(plugin));
                if body == w.body {
                    return true;
                }
                // Its corner stays: it sits by the width it was placed by.
                self.instruments.open[i].body = body;
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
                self.device_hear();
                true
            }
            Some(Grab::Point { .. }) => self.device_point_moved(g, x, y, fine),
            Some(Grab::Note { plugin, pitch }) => {
                // Off the key or pad held, its note ends; coming back does not play it again.
                let p = self.project();
                let on = self
                    .instruments
                    .at(&p, g.size, x, y)
                    .is_some_and(|(i, part)| {
                        self.instruments.open[i].plugin == plugin
                            && matches!(part, Part::Key(k) | Part::Pad(k) if k == pitch)
                    });
                if !on {
                    self.window_note_off();
                }
                true
            }
            None => false,
        }
    }

    /// Lets go: a turned knob goes into the song as one step.
    pub fn instrument_up(&mut self) -> bool {
        let Some(grab) = self.instruments.grab.take() else {
            return false;
        };
        if let Grab::Note { .. } = grab {
            self.window_note_off();
        }
        if let Grab::Knob {
            plugin, k, value, ..
        } = grab
        {
            let p = self.project();
            let w = self.instruments.open.iter().find(|w| w.plugin == plugin);
            let Some((_, x, def)) = w.and_then(|w| live(&p, w)) else {
                return true;
            };
            let (spec, before) = (&def.params[k], values_of(x, def)[k]);
            if (before - value).abs() > 1e-9 {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.run_command(
                    "plugin.param.set",
                    json!({ "id": plugin.to_string(), "param": spec.id, "value": value }),
                );
            }
        }
        if let Grab::Size { plugin, .. } = grab {
            // The next window of its kind opens at this size.
            let p = self.project();
            if let Some(w) = self.instruments.open.iter().find(|w| w.plugin == plugin)
                && let Some((_, _, def)) = live(&p, w)
            {
                self.instruments.sizes.insert(def.uid, w.body);
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

    /// One frame of the open windows' probes (ENG-072, ENG-073, UI-327):
    /// their output meters move, an EQ's analyser takes what it heard and a
    /// dynamics device its levels. A device whose window closed stops
    /// writing. True while a meter still
    /// moves or an analyser hears something.
    pub(crate) fn poll_probes(&mut self) -> bool {
        // A window that went another way (its device removed) ends the note it played.
        if self
            .instruments
            .note
            .is_some_and(|n| !self.instruments.is_open(n.plugin))
        {
            self.window_note_off();
        }
        let rate = f64::from(self.project().settings.sample_rate);
        let lit = self.motion.enabled.get()
            && self
                .instruments
                .flash
                .is_some_and(|(id, _)| self.instruments.lit(id, std::time::Instant::now()) > 0.0);
        let Some(host) = self.transport.host() else {
            return lit;
        };
        let plugins = host.plugins();
        let p = self.project();
        let now = std::time::Instant::now();
        self.instruments
            .poll_with(&p, rate, now, |id| plugins.stock_probe(id))
            | lit
    }

    /// A key or a pad of the window `w` was pressed: its note plays on the window's track as
    /// the track's input would (its transpose and velocity), not recorded (PLG-066).
    fn window_note_on(&mut self, w: Win, pitch: u8) {
        self.window_note_off();
        let p = self.project();
        let kind = NoteKind::On {
            pitch,
            velocity: super::keys::VELOCITY,
        };
        let heard = crate::midi_live::heard(&p, w.track, kind);
        if let Some(kind) = heard {
            self.live_note(&p, w.track, kind);
        }
        self.instruments.note = Some(Held {
            plugin: w.plugin,
            track: w.track,
            pitch,
            heard: match heard {
                Some(NoteKind::On { pitch, .. }) => Some(pitch),
                _ => None,
            },
        });
        self.instruments.grab = Some(Grab::Note {
            plugin: w.plugin,
            pitch,
        });
    }

    /// Ends the note a window's key or pad sounds; false when none does.
    pub(crate) fn window_note_off(&mut self) -> bool {
        let Some(held) = self.instruments.note.take() else {
            return false;
        };
        if let Some(pitch) = held.heard {
            let p = self.project();
            self.live_note(&p, held.track, NoteKind::Off { pitch });
        }
        true
    }

    /// Opens the menu of what an EQ's analyser shows under its button `at`.
    pub(crate) fn spectrum_menu(&mut self, id: PluginId, at: Rect) {
        let now = self.instruments.tap(id);
        let items = xirius_engine::stock::Tap::ALL
            .into_iter()
            .map(|tap| {
                Item::act(eq::SPECTRUM[tap as usize], "", Act::Analyser(id, tap)).on(tap == now)
            })
            .collect();
        self.open_context(items, at.x, at.bottom() + 4.0);
    }

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

    /// Loads the preset `preset` into the built-in plugin `id` (one step): its knobs and,
    /// for a Sampler or a Drum Kit, its samples. The device keeps the preset's name for its
    /// tag.
    pub fn load_preset(&mut self, id: PluginId, preset: &str) -> bool {
        let p = self.project();
        let Some((t, def)) = p.tracks.values().find_map(|t| {
            t.inserts
                .iter()
                .find(|x| x.id == id)
                .and_then(Plugin::builtin)
                .map(|d| (t.id, d))
        }) else {
            return false;
        };
        // The session's load sets what the file holds, the samples too (H-pre-1); Default is
        // the device as it comes, with no sample.
        if self.instruments.presets.saved.is_some() || preset == DEFAULT {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            return self.run_command(
                "preset.load",
                json!({ "track": t.to_string(), "preset": preset, "of": id.to_string() }),
            );
        }
        // Before the files were read once, the factory set's knobs (it holds no sample).
        let Some(v) = self.instruments.presets.values_of(def, preset) else {
            return false;
        };
        let values: Vec<_> = def
            .params
            .iter()
            .zip(v)
            .filter(|(p, _)| p.preset)
            .map(|(p, v)| json!({ "param": p.id, "value": v }))
            .collect();
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
        let r =
            self.instruments
                .part_rect(self.instruments.rect(i, g.size), track, plugin, def, part);
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
        // The window a device just opened in: a ring of the focus colour
        // round it that fades out (UI-323), none under reduced motion.
        let lit = studio.instruments.lit(w.plugin, std::time::Instant::now());
        if lit > 0.0 && studio.motion.enabled.get() {
            let ring = Rect::new(
                r.x - FLASH_W,
                r.y - FLASH_W,
                r.w + 2.0 * FLASH_W,
                r.h + 2.0 * FLASH_W,
            );
            pen.outline(ring, pen.c.ring.opacity(lit), Radius::M, FLASH_W);
        }
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
                preset: &preset,
                modified,
                pinned: w.pinned,
                seats: title_bar::Seats {
                    power: true,
                    sidechain: title_bar::takes_sidechain(def),
                    channel: true,
                },
            },
        );

        // The body: the chosen band's face, the graphs (UI-325) and what
        // lies on them, the names and tabs, the meters, then the controls,
        // where the device's layout puts them.
        let now = values_of(plugin, def);
        let ins = &studio.instruments;
        let shown = ins.shown(plugin);
        // The peaks of the sample a Sampler's or a Drum Kit's wave shows (UI-333, UI-334).
        let peaks = sampler::wave_file(plugin, w.pick, w.layer).and_then(|file| {
            let dir = studio.song().dir();
            let path = crate::wave::source(dir.as_deref(), file)?;
            let store = xirius_session::peaks::store_dir(dir.as_deref());
            match crate::wave::look(&path, &store) {
                crate::wave::Look::Ready(p) => Some(p),
                _ => None,
            }
        });
        let body = ins.body(r, plugin, def);
        let hot = ins.hot.filter(|h| h.0 == plugin.id).map(|h| h.1);
        for &face in &body.faces {
            pen.round(face, c.surface_1, Radius::S);
        }
        let analyser = ins.analysers.get(&plugin.id);
        let rate = f64::from(studio.project().settings.sample_rate);
        for (n, &plane) in body.graphs.iter().enumerate() {
            let chosen = ins.chosen.get(&plugin.id).filter(|c| c.0 == n).map(|c| c.1);
            let focused = ins.focus.is_some_and(|(id, k)| {
                id == plugin.id
                    && chosen
                        .and_then(|c| graph::points(def, n, plane, &shown).get(c).cloned())
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
                bypassed: !plugin.runs(),
                spectrum: analyser.map_or([&[][..], &[][..]], |a| {
                    [a.levels[0].as_slice(), a.levels[1].as_slice()]
                }),
                rate,
                dynamics: ins.dynamics.get(&plugin.id),
                tempo: studio.tempo_target().1,
                echo: ins
                    .echoes
                    .get(&plugin.id)
                    .and_then(|e| e.since(std::time::Instant::now())),
                wave: peaks.as_deref(),
            };
            graph::paint(pen, def, n, plane, &shown, &view);
        }
        if let Some(button) = ins.spectrum_of(&body, plugin.id) {
            eq::paint_spectrum(pen, button, ins.tap(plugin.id), hot == Some(Part::Spectrum));
        }
        if let Some(at) = body.tempo {
            pen.text(
                &format!("{:.0} BPM", studio.tempo_target().1),
                FontSize::Caption,
                Weight::Regular,
                c.ink,
                at,
                Align::Center,
            );
        }
        for name in &body.names {
            let ink = if name.strong { c.ink_strong } else { c.ink };
            let w = name.r.w + 4.0;
            pen.text(
                name.text,
                name.size,
                Weight::Semibold,
                ink,
                Rect::new(name.r.x, name.r.y, w, name.r.h),
                Align::Left,
            );
        }
        for (i, tab) in body.tabs.iter().enumerate() {
            if tab.chosen {
                pen.round(tab.r, c.surface_1, Radius::S);
            } else if hot == Some(Part::Tab(i)) {
                pen.round(tab.r, c.surface_3, Radius::S);
            }
            let ink = if !tab.on {
                c.scroll_thumb[0]
            } else if tab.chosen {
                c.ink_strong
            } else {
                c.ink
            };
            pen.text(
                tab.text,
                FontSize::Small,
                Weight::Semibold,
                ink,
                tab.r,
                Align::Center,
            );
        }
        if !body.levels.is_empty() {
            let readout = match hot {
                Some(Part::Readout(i)) => Some(i),
                _ => None,
            };
            let none = Dynamics::default();
            let d = ins.dynamics.get(&plugin.id).unwrap_or(&none);
            dynamics::paint_levels(pen, &body.levels, d, readout);
        }
        let down = ins.note.filter(|n| n.plugin == plugin.id).map(|n| n.pitch);
        let pad = match hot {
            Some(Part::Pad(k)) => Some(k),
            _ => None,
        };
        play::paint_keys(pen, &body.keys, down);
        play::paint_pads(pen, &body.pads, down, pad);
        let spot = match hot {
            Some(Part::Spot(n)) => Some(n),
            _ => None,
        };
        sampler::paint_spots(pen, &body, spot);
        let meter = ins.meters.get(&plugin.id).copied().unwrap_or_default();
        for (ch, &m) in body.meters.iter().enumerate() {
            crate::mixer::paint::meter(pen, m, &meter, ch.min(1));
        }
        let entry = ins.entry.as_ref().filter(|e| e.plugin == plugin.id);
        for slot in &body.slots {
            let k = slot.k;
            let spec = &def.params[k];
            let v = ins.turning(plugin.id, k).unwrap_or(now[k]);
            let focused = ins.focus == Some((plugin.id, k));
            let name = slot.label.unwrap_or_else(|| control::name(def, spec));
            let hit = slot.hit(def);
            let over = hot == Some(Part::Knob(k));
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
                        dim: slot.dim,
                    },
                ),
                Look::Segments => {
                    control::paint_segments(pen, hit, spec, v, focused);
                    control::paint_name(pen, slot.cell, slot.dial, name);
                }
                Look::Toggle => control::paint_toggle(pen, hit, name, v >= 0.5, focused),
                Look::Dot => control::paint_dot(pen, hit, spec.to_plain(v) >= 0.5, over, focused),
                Look::Menu => {
                    let mut values = shown.clone();
                    if let Some(x) = values.get_mut(k) {
                        *x = v;
                    }
                    let label = control::menu_text(def, k, &values);
                    if matches!(def.uid, "delay" | "drum-kit") {
                        control::paint_box(pen, hit, &label, over, focused);
                    } else {
                        control::paint_choice(pen, hit, &label, over, focused);
                    }
                }
                Look::Field => control::paint_field(
                    pen,
                    hit,
                    &control::text(spec, v),
                    entry.filter(|e| e.k == k).map(|e| &e.field),
                    over,
                    focused,
                ),
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
        // The corner's grip while the pointer is on it or holds it: three
        // dots in its corner (UI-331).
        let held = matches!(ins.grab, Some(Grab::Size { plugin: id, .. }) if id == plugin.id);
        if held || hot == Some(Part::Resize) {
            let (x, y) = (r.right() - 6.0, r.bottom() - 6.0);
            for (dx, dy) in [(0.0, 0.0), (-4.0, 0.0), (0.0, -4.0)] {
                let dot = Rect::new(x + dx - 1.0, y + dy - 1.0, 2.0, 2.0);
                pen.round(dot, c.scroll_thumb[0], Radius::Xs);
            }
        }
    });
}
