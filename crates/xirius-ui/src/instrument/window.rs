//! The window of a built-in plug-in (UI-098, PLG-045): its knobs and its
//! preset, floating over the screen. The keys button at the head of an
//! instrument track opens an instrument's, and the plug-in's row in the
//! mixer opens an instrument's or an effect's. A plug-in of its own (VST3)
//! opens its own window instead.
//!
//! The knobs are the built-in's parameters as `xirius-params` lists them, in
//! its order: a parameter added there gets its knob here. A knob turns while
//! it is dragged and goes into the song when it is let go (`plugin.param.set`,
//! 0..1, one step); a preset sets every knob at once (`plugin.preset.load`,
//! one step).

use gpui::{Bounds, Corners, point, px, size};
use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};
use xirius_params::{BuiltinDef, Curve, ParamSpec};

use super::menu::Menu;
use crate::draw::{Align, Pen};
use crate::popup::PopupKind;
use crate::studio::{Geometry, Rect, Studio};

/// The bar with the icon, the title and the close button.
const HEAD_H: f32 = 30.0;
const CLOSE: f32 = 22.0;
const PAD: f32 = 12.0;
/// A knob, its value and its name.
const CELL_W: f32 = 92.0;
const CELL_H: f32 = 60.0;
const KNOB: f32 = 30.0;
const COL_GAP: f32 = 8.0;
const ROW_GAP: f32 = 12.0;
const PRESET_H: f32 = 26.0;
const FOOT_H: f32 = PRESET_H + 8.0;
/// How far the pointer goes for the whole of a knob's sweep.
const KNOB_TRAVEL: f32 = 150.0;
/// How much of a window stays on the screen when it is dragged off.
const KEEP_W: f32 = 80.0;

/// The presets, the first being the defaults.
pub const PRESETS: [&str; 4] = ["初期", "やさしく", "強め", "声を前に"];

/// What the preset box says when the knobs match no preset.
pub const CUSTOM: &str = "カスタム";

/// Knobs in a row: three, four for a long list (the EQ's).
fn columns(n: usize) -> usize {
    if n > 12 { 4 } else { 3 }
}

/// A window's width for `n` knobs.
pub fn width(n: usize) -> f32 {
    let c = columns(n);
    PAD * 2.0 + c as f32 * CELL_W + (c - 1) as f32 * COL_GAP
}

fn body_h(n: usize) -> f32 {
    let rows = n.div_ceil(columns(n)).max(1) as f32;
    PAD * 2.0 + rows * CELL_H + (rows - 1.0) * ROW_GAP
}

/// A value on a parameter's own steps (a choice, a whole number, on/off).
fn snap(spec: &ParamSpec, norm: f64) -> f64 {
    let n = norm.clamp(0.0, 1.0);
    match spec.steps() {
        Some(_) => spec.to_norm(spec.to_plain(n)),
        None => (n * 10_000.0).round() / 10_000.0,
    }
}

/// The knobs' values (0..1) of a preset: each sweeping knob moved from its
/// default by a share of its sweep, every other one the other way; the
/// stepped ones stay at their defaults.
pub fn preset_values(def: &BuiltinDef, preset: &str) -> Option<Vec<f64>> {
    let f = match preset {
        "初期" => 0.0,
        "やさしく" => -0.18,
        "強め" => 0.22,
        "声を前に" => 0.1,
        _ => return None,
    };
    Some(
        def.params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let d = p.default_norm();
                if p.steps().is_some() {
                    return d;
                }
                let sign = if i % 2 == 1 { -1.0 } else { 1.0 };
                (d + f * sign).clamp(0.0, 1.0)
            })
            .collect(),
    )
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

/// The preset the knobs are at, or [`CUSTOM`].
pub fn preset_of(plugin: &Plugin) -> &'static str {
    let Some(def) = plugin.builtin() else {
        return CUSTOM;
    };
    let now = values(plugin);
    PRESETS
        .iter()
        .find(|p| {
            preset_values(def, p)
                .is_some_and(|v| v.iter().zip(&now).all(|(a, b)| (a - b).abs() < 1e-6))
        })
        .copied()
        .unwrap_or(CUSTOM)
}

/// A knob's value as it reads (`-12.0 dB`, `1.20 kHz`, `付点`).
pub fn text(spec: &ParamSpec, norm: f64) -> String {
    spec.text(spec.to_plain(norm))
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
    /// How many knobs it has (its size follows).
    pub knobs: usize,
    /// Where its corner is, once it was dragged; else it sits where new ones go.
    pub at: Option<(f32, f32)>,
}

/// What a press in a window took hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Grab {
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
}

/// A part of a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Head,
    Close,
    Knob(usize),
    Preset,
    Body,
}

/// The open windows, the last on top.
#[derive(Debug, Default)]
pub struct Windows {
    pub open: Vec<Win>,
    grab: Option<Grab>,
    /// Where the open instrument menu hangs, for the menu a row opens next.
    pub menu_at: Option<Rect>,
}

impl Windows {
    pub fn is_open(&self, id: PluginId) -> bool {
        self.open.iter().any(|w| w.plugin == id)
    }

    /// The value a knob shows while it is dragged.
    fn turning(&self, id: PluginId, k: usize) -> Option<f64> {
        match self.grab {
            Some(Grab::Knob {
                plugin,
                k: at,
                value,
                ..
            }) if plugin == id && at == k => Some(value),
            _ => None,
        }
    }

    /// The `i`th window's frame on a screen of `size`.
    pub fn rect(&self, i: usize, size: (f32, f32)) -> Rect {
        let n = self.open[i].knobs;
        let (w, h) = (width(n), HEAD_H + body_h(n) + FOOT_H);
        let (x, y) = self.open[i].at.unwrap_or((
            size.0 / 2.0 - w / 2.0 + i as f32 * 24.0,
            64.0 + i as f32 * 24.0,
        ));
        Rect::new(
            x.clamp(0.0, (size.0 - KEEP_W).max(0.0)),
            y.clamp(0.0, (size.1 - HEAD_H).max(0.0)),
            w,
            h,
        )
    }

    /// The top window under a point, and the part of it.
    pub fn at(&self, size: (f32, f32), x: f32, y: f32) -> Option<(usize, Part)> {
        (0..self.open.len()).rev().find_map(|i| {
            let r = self.rect(i, size);
            if !r.contains(x, y) {
                return None;
            }
            let n = self.open[i].knobs;
            let part = if part_rect(r, n, Part::Close).contains(x, y) {
                Part::Close
            } else if part_rect(r, n, Part::Head).contains(x, y) {
                Part::Head
            } else if part_rect(r, n, Part::Preset).contains(x, y) {
                Part::Preset
            } else {
                (0..n)
                    .find(|&k| part_rect(r, n, Part::Knob(k)).contains(x, y))
                    .map_or(Part::Body, Part::Knob)
            };
            Some((i, part))
        })
    }

    /// Brings the `i`th window to the top; returns its new index.
    fn raise(&mut self, i: usize) -> usize {
        let w = self.open.remove(i);
        self.open.push(w);
        self.open.len() - 1
    }

    /// Forgets the windows whose plugin is gone or is no longer built in.
    fn prune(&mut self, p: &Project) {
        self.open.retain(|w| live(p, w).is_some());
        if let Some(Grab::Move { plugin, .. } | Grab::Knob { plugin, .. }) = self.grab
            && !self.is_open(plugin)
        {
            self.grab = None;
        }
    }
}

/// Where a part sits in a window of `n` knobs whose frame is `r`.
pub fn part_rect(r: Rect, n: usize, part: Part) -> Rect {
    match part {
        Part::Head => Rect::new(r.x, r.y, r.w, HEAD_H),
        Part::Close => Rect::new(
            r.right() - 4.0 - CLOSE,
            r.y + (HEAD_H - CLOSE) / 2.0,
            CLOSE,
            CLOSE,
        ),
        Part::Knob(k) => {
            let c = columns(n);
            let (col, row) = ((k % c) as f32, (k / c) as f32);
            Rect::new(
                r.x + PAD + col * (CELL_W + COL_GAP),
                r.y + HEAD_H + PAD + row * (CELL_H + ROW_GAP),
                CELL_W,
                CELL_H,
            )
        }
        Part::Preset => Rect::new(
            r.x + Space::X2.px(),
            r.y + HEAD_H + body_h(n),
            r.w - Space::X2.px() * 2.0,
            PRESET_H,
        ),
        Part::Body => r,
    }
}

/// The track, the plugin and its parameters of a window, while it is still
/// there.
fn live<'a>(p: &'a Project, w: &Win) -> Option<(&'a Track, &'a Plugin, &'static BuiltinDef)> {
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
                let knobs = p
                    .tracks
                    .get(&t)
                    .and_then(|x| x.inserts.iter().find(|x| x.id == id))
                    .and_then(Plugin::builtin)
                    .map(|d| d.params.len());
                if let Some(knobs) = knobs {
                    self.instruments.open.push(Win {
                        track: t,
                        plugin: id,
                        knobs,
                        at: None,
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
        self.instruments.at(g.size, x, y).is_some()
    }

    /// A press: true when it was on a window (nothing under it gets it).
    pub fn instrument_down(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        self.instruments.prune(&self.project());
        let Some((i, part)) = self.instruments.at(g.size, x, y) else {
            return false;
        };
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
            Part::Knob(k) => {
                let p = self.project();
                let held = live(&p, &w)
                    .and_then(|(_, _, d)| d.params.get(k))
                    .is_some_and(|spec| self.param_locked(w.track, w.plugin, spec.id));
                if held {
                    return true;
                }
                if let Some(value) = live(&p, &w).and_then(|(_, x, _)| values(x).get(k).copied()) {
                    self.instruments.grab = Some(Grab::Knob {
                        plugin: w.plugin,
                        k,
                        start: value,
                        y,
                        value,
                    });
                }
            }
            Part::Preset => {
                let anchor = part_rect(r, w.knobs, Part::Preset);
                self.open_inst_menu(Menu::Preset(w.track, w.plugin), anchor);
            }
            Part::Body => {}
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
        let (i, Part::Knob(k)) = self.instruments.at(g.size, x, y)? else {
            return None;
        };
        let w = self.instruments.open[i];
        let p = self.project();
        let spec = live(&p, &w)?.2.params.get(k)?;
        let r = part_rect(self.instruments.rect(i, g.size), w.knobs, Part::Knob(k));
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
                    (from.1 + y - pointer.1).clamp(0.0, (g.size.1 - HEAD_H).max(0.0)),
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
                let w = self.instruments.open.iter().find(|w| w.plugin == plugin);
                let Some(spec) = w
                    .and_then(|w| live(&p, w))
                    .and_then(|(_, _, d)| d.params.get(k))
                else {
                    return false;
                };
                let scale = if fine { 0.25 } else { 1.0 };
                let t = start - f64::from((y - y0) / KNOB_TRAVEL * scale);
                let next = snap(spec, t);
                if let Some(Grab::Knob { value, .. }) = &mut self.instruments.grab {
                    *value = next;
                }
                true
            }
            None => false,
        }
    }

    /// Lets go: a turned knob goes into the song as one step. A click on an
    /// on/off knob that did not turn it flips it.
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
            let value = if (before - value).abs() < 1e-9 && matches!(spec.curve, Curve::Toggle) {
                1.0 - before.round()
            } else {
                value
            };
            if (before - value).abs() > 1e-9 {
                self.run_command(
                    "plugin.param.set",
                    json!({ "id": plugin.to_string(), "param": spec.id, "value": value }),
                );
            }
        }
        true
    }

    /// Sets every knob of a built-in plugin to a preset (one step).
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
            .map(|(p, v)| json!({ "param": p.id, "value": v }))
            .collect();
        self.run_command(
            "plugin.preset.load",
            json!({ "id": id.to_string(), "preset": preset, "values": values }),
        )
    }

    /// The centre of a part of the window of `id`, for the tests.
    pub fn instrument_center(&self, g: &Geometry, id: PluginId, part: Part) -> Option<(f32, f32)> {
        let i = self.instruments.open.iter().position(|w| w.plugin == id)?;
        let n = self.instruments.open[i].knobs;
        let r = part_rect(self.instruments.rect(i, g.size), n, part);
        let at = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        // Only if nothing covers it.
        (self.instruments.at(g.size, at.0, at.1) == Some((i, part))).then_some(at)
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
        paint_window(pen, studio, r, track, plugin, def);
    }
}

fn paint_window(
    pen: &mut Pen,
    studio: &Studio,
    r: Rect,
    track: &Track,
    plugin: &Plugin,
    def: &BuiltinDef,
) {
    let c = pen.c;
    let n = def.params.len();
    // A floating panel: the soft shadow all round it (UI-007), then its face.
    let at = Bounds::new(
        point(px(pen.origin.0 + r.x), px(pen.origin.1 + r.y)),
        size(px(r.w), px(r.h)),
    );
    pen.window
        .paint_drop_shadows(at, Corners::all(px(Radius::M.px())), &c.float);
    pen.round(r, c.paper, Radius::M);
    pen.mask(r, |pen| {
        // The bar: icon, title, close.
        let head = part_rect(r, n, Part::Head);
        pen.fill(head, c.surface_1);
        let icon = Rect::new(head.x + 10.0, head.y + (HEAD_H - 14.0) / 2.0, 14.0, 14.0);
        let glyph = match plugin.kind {
            PluginKind::Instrument => "keys",
            PluginKind::Effect => "effects",
        };
        pen.icon(glyph, icon, c.ink_strong);
        let close = part_rect(r, n, Part::Close);
        let title = Rect::new(
            icon.right() + 6.0,
            head.y,
            close.x - icon.right() - 12.0,
            HEAD_H,
        );
        pen.mask(title, |pen| {
            pen.text(
                &format!("{} · {}", plugin.name, track.shown_name()),
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                title,
                Align::Left,
            );
        });
        pen.icon(
            "close",
            Rect::new(
                close.x + (CLOSE - Layout::ICON_SIZE) / 2.0,
                close.y + (CLOSE - Layout::ICON_SIZE) / 2.0,
                Layout::ICON_SIZE,
                Layout::ICON_SIZE,
            ),
            c.ink,
        );

        // The knobs: the knob, its value, its name. A knob of a range across
        // zero (a gain, a tune) lights from its middle.
        let now = values(plugin);
        for (k, spec) in def.params.iter().enumerate() {
            let cell = part_rect(r, n, Part::Knob(k));
            let v = studio.instruments.turning(plugin.id, k).unwrap_or(now[k]);
            let from = if spec.min < 0.0 && spec.max > 0.0 {
                spec.to_norm(0.0) as f32
            } else {
                0.0
            };
            let knob = Rect::new(cell.x + (cell.w - KNOB) / 2.0, cell.y, KNOB, KNOB);
            crate::mixer::paint::knob(pen, knob, v as f32, from, 3.0, true);
            crate::auto_mark::paint(
                pen,
                track,
                xirius_core::auto::AutoTarget::Param(plugin.id, spec.id.to_string()),
                knob,
            );
            let value = Rect::new(cell.x, knob.bottom() + 2.0, cell.w, 14.0);
            pen.text(
                &text(spec, v),
                FontSize::Micro,
                Weight::Regular,
                c.ink_strong,
                value,
                Align::Center,
            );
            let name = Rect::new(cell.x, value.bottom(), cell.w, 14.0);
            pen.mask(name, |pen| {
                pen.text(
                    spec.label,
                    FontSize::Micro,
                    Weight::Regular,
                    c.ink,
                    name,
                    Align::Center,
                );
            });
        }

        // The preset.
        let box_ = part_rect(r, n, Part::Preset);
        pen.round(box_, c.surface_3, Radius::S);
        let text = Rect::new(
            box_.x + Space::X2.px(),
            box_.y,
            box_.w - Layout::ICON_SIZE - Space::X3.px(),
            box_.h,
        );
        pen.mask(text, |pen| {
            pen.text(
                preset_of(plugin),
                FontSize::Small,
                Weight::Regular,
                c.ink_strong,
                text,
                Align::Left,
            );
        });
        pen.icon(
            "chevron-down",
            Rect::new(
                box_.right() - Layout::ICON_SIZE - Space::X2.px(),
                box_.y + (box_.h - Layout::ICON_SIZE) / 2.0,
                Layout::ICON_SIZE,
                Layout::ICON_SIZE,
            ),
            c.ink,
        );
    });
}
