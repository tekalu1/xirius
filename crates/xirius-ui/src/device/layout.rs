//! Where a device's controls sit in its window's body (UI-324): each one a
//! slot with its parameter, its look and its cell. The rects are the body's
//! own (its top left corner at 0, 0); the window moves them to where it is.
//! A device without its own layout has its controls in rows, in the order
//! `xirius-params` lists them.

use xirius_core::model::Plugin;
use xirius_design::FontSize;
use xirius_params::BuiltinDef;

use super::control::{self, DIAL_OUT, Look, NAME_H, VALUE_H};
use super::play::Key;
use crate::studio::Rect;

/// How far a Drum Kit's pad's M and S sit above its foot.
pub const MS_FOOT: f32 = 4.0;

pub const PAD: f32 = 12.0;
/// A control's cell in the rows: its dial, its value and its name.
pub const CELL_W: f32 = 92.0;
pub const CELL_H: f32 = DIAL_OUT + 2.0 + VALUE_H + NAME_H;
pub const COL_GAP: f32 = 8.0;
pub const ROW_GAP: f32 = 12.0;

/// One control in the body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The parameter's index in the device's list.
    pub k: usize,
    pub look: Look,
    /// The dial of a knob (the line a row of segments or a button sits on).
    pub dial: f32,
    /// The control's cell: its dial, its value and its name.
    pub cell: Rect,
    /// The name it shows under its dial when not its own (a band's column
    /// says `Freq` where the screen reader says `LMF Freq`).
    pub label: Option<&'static str>,
    /// Its band is off: its value is the plain ink.
    pub dim: bool,
}

impl Slot {
    /// What a press takes: a knob's whole cell, a row of segments, a button.
    pub fn hit(&self, def: &BuiltinDef) -> Rect {
        let spec = &def.params[self.k];
        let c = self.cell;
        let band = |w: f32, h: f32| {
            let w = w.min(c.w);
            Rect::new(c.x + (c.w - w) / 2.0, c.y + (self.dial - h) / 2.0, w, h)
        };
        match self.look {
            Look::Knob => c,
            Look::Segments => band(control::segments_w(spec), control::SEG_H),
            Look::Toggle => band(
                control::toggle_w(control::name(def, spec)),
                control::TOGGLE_H,
            ),
            Look::Dot | Look::Menu | Look::Field => c,
        }
    }

    /// The same slot with its rects moved by `(dx, dy)`.
    pub fn at(self, dx: f32, dy: f32) -> Slot {
        Slot {
            cell: Rect::new(self.cell.x + dx, self.cell.y + dy, self.cell.w, self.cell.h),
            ..self
        }
    }
}

/// A short name in the body (a band's over its column).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Name {
    pub r: Rect,
    pub text: &'static str,
    pub size: FontSize,
    /// Strong ink (a band that is on), else the plain ink.
    pub strong: bool,
    /// The dim of a band cut, as its point (an EQ band that is off, UI-326).
    pub dim: bool,
}

/// A tab choosing what the body shows (a narrow EQ's bands).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tab {
    pub r: Rect,
    pub text: &'static str,
    pub chosen: bool,
    /// Dim while what it shows is switched off.
    pub on: bool,
}

/// What a press on a Sampler's or a Drum Kit's spot does (UI-333, UI-334).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// The ‹ › beside a sample's name: the file before or after it in its folder.
    Prev,
    Next,
    /// The sample's name (drawn, takes no press).
    Name,
    /// A Sampler's zone (by its file) in the strip over the keys.
    Zone(usize),
    /// A Drum Kit's bank tab, and a pad's M and S (by its pad).
    Bank(usize),
    Mute(usize),
    Solo(usize),
    /// A layer of the chosen pad (by its place among the pad's files).
    Layer(usize),
    /// The edge between a pad's `j`th velocity layer and the next: dragged, or moved by
    /// ← and → once pressed, it sets where the `j`th ends (UI-334).
    Edge(usize),
    /// The plane a sample is dropped on, and its button opening the files.
    Drop,
    Browse,
    /// The chosen pad's name over its controls, and what a pad says (drawn, take no press).
    Title,
    Pad(usize),
}

impl Mark {
    /// Does a press on it do something?
    pub fn takes(self) -> bool {
        !matches!(self, Mark::Drop | Mark::Title | Mark::Pad(_))
    }
}

/// A part of a Sampler's or a Drum Kit's body that is not a parameter: a zone, a bank, a
/// layer, the sample's name and its arrows, the drop plane.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub r: Rect,
    pub mark: Mark,
    pub text: String,
    /// What it says second (a pad's note, its layers `Vel 3`).
    pub sub: String,
    /// Chosen (a zone, a bank, a layer) or on (M, S).
    pub on: bool,
    /// A bank with samples has a dot under its letter; a zone's root note; a pad that has
    /// files.
    pub dot: bool,
    pub note: Option<u8>,
    /// A pad that does not sound: muted, or another pad soloed (UI-334).
    pub dim: bool,
    /// A pad or a bank files dragged over would fill (UI-334).
    pub ghost: bool,
}

impl Spot {
    pub fn new(r: Rect, mark: Mark, text: impl Into<String>) -> Spot {
        Spot {
            r,
            mark,
            text: text.into(),
            sub: String::new(),
            on: false,
            dot: false,
            note: None,
            dim: false,
            ghost: false,
        }
    }
}

/// A device's body: its size, its controls, its graphs' planes (UI-325)
/// and what else it draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body {
    pub w: f32,
    pub h: f32,
    pub slots: Vec<Slot>,
    pub graphs: Vec<Rect>,
    /// Faces a little lighter than the body (the chosen band's column).
    pub faces: Vec<Rect>,
    pub names: Vec<Name>,
    pub tabs: Vec<Tab>,
    /// The button choosing what a graph's analyser shows.
    pub spectrum: Option<Rect>,
    /// The device's meters (its output's, its gain reduction's).
    pub meters: Vec<Rect>,
    /// A dynamics device's In, GR and Out columns (UI-327).
    pub levels: Vec<super::dynamics::Level>,
    /// Where a synced device says the tempo (UI-329).
    pub tempo: Option<Rect>,
    /// The graph that grows when the window is taller (UI-331).
    pub main: usize,
    /// A Sampler's keyboard and a Drum Kit's pads (PLG-066).
    pub keys: Vec<Key>,
    pub keyboard: Option<super::play::Keyboard>,
    pub pads: Vec<Key>,
    /// A Sampler's and a Drum Kit's other parts (UI-333, UI-334).
    pub spots: Vec<Spot>,
}

impl Body {
    /// The slot of the parameter `k`.
    pub fn slot(&self, k: usize) -> Option<&Slot> {
        self.slots.iter().find(|s| s.k == k)
    }

    /// The same body with all its rects moved by `(dx, dy)`.
    pub fn at(self, dx: f32, dy: f32) -> Body {
        let m = |r: Rect| Rect::new(r.x + dx, r.y + dy, r.w, r.h);
        Body {
            slots: self.slots.into_iter().map(|s| s.at(dx, dy)).collect(),
            graphs: self.graphs.into_iter().map(m).collect(),
            faces: self.faces.into_iter().map(m).collect(),
            names: self
                .names
                .into_iter()
                .map(|n| Name { r: m(n.r), ..n })
                .collect(),
            tabs: self
                .tabs
                .into_iter()
                .map(|t| Tab { r: m(t.r), ..t })
                .collect(),
            spectrum: self.spectrum.map(m),
            meters: self.meters.into_iter().map(m).collect(),
            levels: self.levels.into_iter().map(|l| l.at(dx, dy)).collect(),
            tempo: self.tempo.map(m),
            keyboard: self.keyboard.map(|k| k.at(dx, dy)),
            keys: self
                .keys
                .into_iter()
                .map(|k| Key { r: m(k.r), ..k })
                .collect(),
            pads: self
                .pads
                .into_iter()
                .map(|k| Key { r: m(k.r), ..k })
                .collect(),
            spots: self
                .spots
                .into_iter()
                .map(|s| Spot { r: m(s.r), ..s })
                .collect(),
            ..self
        }
    }

    /// The same body `h` tall when that is taller (UI-331): the main graph
    /// takes the rest, what is under it moves down, and in its row beside it
    /// a meter or a column of levels stretches and a control at their foot
    /// moves down with the graph's foot. The controls keep their size.
    pub fn grown(mut self, h: f32) -> Body {
        let dy = h - self.h;
        // A Drum Kit's pads are a main graph too (UI-331): their grid takes the height
        // with the wave or the drop plane beside it.
        let grid = self.pads.iter().map(|k| k.r).reduce(|a, b| {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            Rect::new(
                x,
                y,
                a.right().max(b.right()) - x,
                a.bottom().max(b.bottom()) - y,
            )
        });
        let Some(g) = self.graphs.get(self.main).copied().or(grid) else {
            return self;
        };
        if dy <= 0.0 {
            return self;
        }
        const E: f32 = 0.5;
        let under = |r: &Rect| r.y >= g.bottom() - E;
        let row = |r: &Rect| r.y >= g.y - E && r.bottom() <= g.bottom() + E;
        let beside = |r: &Rect| r.y >= g.y - E && r.y < g.bottom() && r.x >= g.right();
        let fit = |r: &mut Rect| {
            if under(r) {
                r.y += dy;
            } else if row(r) {
                r.h += dy;
            }
        };
        for s in &mut self.slots {
            if under(&s.cell) || beside(&s.cell) {
                s.cell.y += dy;
            }
        }
        for (n, r) in self.graphs.iter_mut().enumerate() {
            if n == self.main {
                r.h += dy;
            } else if under(r) {
                r.y += dy;
            }
        }
        self.faces.iter_mut().for_each(fit);
        self.meters.iter_mut().for_each(fit);
        for n in &mut self.names {
            if under(&n.r) {
                n.r.y += dy;
            }
        }
        for t in &mut self.tabs {
            if under(&t.r) {
                t.r.y += dy;
            }
        }
        if let Some(r) = &mut self.tempo
            && under(r)
        {
            r.y += dy;
        }
        for k in self.keys.iter_mut().chain(&mut self.pads) {
            if under(&k.r) {
                k.r.y += dy;
            }
        }
        if let Some(k) = &mut self.keyboard
            && under(&k.view)
        {
            *k = k.clone().at(0.0, dy);
        }
        for s in &mut self.spots {
            if under(&s.r) {
                s.r.y += dy;
            } else if s.mark == Mark::Drop && row(&s.r) {
                s.r.h += dy;
            } else if s.mark == Mark::Browse && row(&s.r) {
                s.r.y += dy / 2.0;
            }
        }
        for l in &mut self.levels {
            if under(&l.name) {
                *l = l.clone().at(0.0, dy);
            } else if row(&Rect::new(
                l.name.x,
                l.name.y,
                0.0,
                l.readout.bottom() - l.name.y,
            )) {
                // Its bars stretch and its readout stays under them.
                l.bars.iter_mut().for_each(|b| b.h += dy);
                l.readout.y += dy;
            }
        }
        // Beside the main graph (not over it, as in a narrow Drum Kit), the pads stretch
        // down by `dy` in all; a pad's M and S stay at its foot.
        if let Some(grid) = grid.filter(|r| r.h > 0.0 && r.y < g.bottom() && g.y < r.bottom()) {
            let s = (grid.h + dy) / grid.h;
            let y_of = |y: f32| grid.y + (y - grid.y) * s;
            for k in &mut self.pads {
                k.r = Rect::new(k.r.x, y_of(k.r.y), k.r.w, k.r.h * s);
            }
            for sp in &mut self.spots {
                match sp.mark {
                    Mark::Pad(_) => {
                        sp.r = Rect::new(sp.r.x, y_of(sp.r.y), sp.r.w, sp.r.h * s);
                    }
                    Mark::Mute(_) | Mark::Solo(_) => {
                        let foot = sp.r.bottom() + MS_FOOT;
                        sp.r.y += y_of(foot) - foot;
                    }
                    _ => {}
                }
            }
        }
        self.h = h;
        self
    }
}

/// What a body is laid out for: its width, the band chosen (a narrow EQ
/// shows that one's knobs) and the values (0..1) it shows (a band's type
/// swaps its knobs). A Sampler's and a Drum Kit's also follow the plug-in's
/// files: the zone or pad chosen (`pick`) and the pad's layer (UI-333, UI-334).
#[derive(Clone, Copy, Debug, Default)]
pub struct Shape<'a> {
    pub w: f32,
    pub band: usize,
    pub values: &'a [f64],
    pub plugin: Option<&'a Plugin>,
    pub pick: usize,
    pub layer: usize,
    /// The Drum Kit's bank shown while it is not the chosen pad's.
    pub bank: Option<usize>,
}

impl Shape<'_> {
    /// The plain value of the parameter `id` of `def`, its default when the
    /// values leave it out.
    pub fn plain(&self, def: &BuiltinDef, id: &str) -> f64 {
        def.param(id).map_or(0.0, |(k, spec)| {
            spec.to_plain(self.values.get(k).copied().unwrap_or(spec.default_norm()))
        })
    }
}

/// Controls in a row: three, four for a long list.
fn columns(n: usize) -> usize {
    if n > 12 { 4 } else { 3 }
}

/// The body of a device.
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    match def.uid {
        "eq" => super::eq::body(def, s),
        "compressor" => super::compressor::body(def, s),
        "limiter" => super::limiter::body(def, s),
        "delay" => super::delay::body(def, s),
        "reverb" => super::reverb::body(def, s),
        "sampler" => super::sampler::body(def, s),
        "drum-kit" => super::sampler::kit_body(def, s),
        _ => rows(def),
    }
}

/// The width a device's window opens at.
pub fn natural_w(def: &BuiltinDef) -> f32 {
    match def.uid {
        "eq" => super::eq::NATURAL_W,
        "compressor" => super::compressor::NATURAL_W,
        "limiter" => super::limiter::NATURAL_W,
        "delay" => super::delay::NATURAL_W,
        "reverb" => super::reverb::NATURAL_W,
        "sampler" => super::sampler::NATURAL_W,
        "drum-kit" => super::sampler::KIT_W,
        _ => rows(def).w,
    }
}

/// The narrowest a device's window goes (UI-331).
pub fn min_w(def: &BuiltinDef) -> f32 {
    match def.uid {
        "eq" => super::eq::MIN_W,
        "compressor" => super::compressor::MIN_W,
        "limiter" => super::limiter::MIN_W,
        "delay" => super::delay::MIN_W,
        "reverb" => super::reverb::MIN_W,
        "sampler" | "drum-kit" => super::sampler::MIN_W,
        _ => rows(def).w,
    }
}

/// The size of a device's body at the width `w`, its values at their
/// defaults (its height follows its width, not its values).
pub fn size(def: &BuiltinDef, w: f32) -> (f32, f32) {
    size_of(def, w, None)
}

/// The size of the body of `plugin`'s device at `w`: a Sampler's and a Drum Kit's
/// follow their files (UI-333, UI-334).
pub fn size_of(def: &BuiltinDef, w: f32, plugin: Option<&Plugin>) -> (f32, f32) {
    let values: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    let b = body(
        def,
        &Shape {
            w,
            values: &values,
            plugin,
            ..Shape::default()
        },
    );
    (b.w, b.h)
}

/// The controls in rows, in their order.
fn rows(def: &BuiltinDef) -> Body {
    let n = def.params.len();
    let c = columns(n);
    let lines = n.div_ceil(c).max(1) as f32;
    let slots = def
        .params
        .iter()
        .enumerate()
        .map(|(k, spec)| {
            let (col, row) = ((k % c) as f32, (k / c) as f32);
            Slot {
                k,
                look: control::look(spec),
                dial: DIAL_OUT,
                cell: Rect::new(
                    PAD + col * (CELL_W + COL_GAP),
                    PAD + row * (CELL_H + ROW_GAP),
                    CELL_W,
                    CELL_H,
                ),
                label: None,
                dim: false,
            }
        })
        .collect();
    Body {
        w: PAD * 2.0 + c as f32 * CELL_W + (c - 1) as f32 * COL_GAP,
        h: PAD * 2.0 + lines * CELL_H + (lines - 1.0) * ROW_GAP,
        slots,
        ..Body::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_has_one_slot_inside_the_body() {
        for def in xirius_params::BUILTINS {
            let values: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
            let shape = Shape {
                w: natural_w(def),
                values: &values,
                ..Shape::default()
            };
            let b = body(def, &shape);
            if def.uid == "eq" {
                // Its bands' knobs follow their types; each band has its own.
                assert!(b.slots.len() <= def.params.len());
            } else if def.uid == "delay" {
                // Time or Beats (Feel goes with Beats), and the band's cuts
                // and the taps' feedback are points.
                assert!(b.slots.len() < def.params.len());
            } else if def.uid == "reverb" {
                // The Color band's ends are points.
                assert_eq!(b.slots.len(), def.params.len() - 2);
            } else if matches!(def.uid, "sampler" | "drum-kit") {
                // With no samples there is nothing to set but the drop plane (UI-333,
                // UI-334); the tests of `sampler` lay one out with its files.
                assert!(b.slots.is_empty(), "{}", def.uid);
            } else {
                assert_eq!(b.slots.len(), def.params.len(), "{}", def.uid);
            }
            for s in &b.slots {
                let h = s.hit(def);
                assert!(h.x >= 0.0 && h.right() <= b.w, "{} {}", def.uid, s.k);
                assert!(h.y >= 0.0 && h.bottom() <= b.h, "{} {}", def.uid, s.k);
            }
            for g in &b.graphs {
                assert!(g.x >= 0.0 && g.right() <= b.w, "{}", def.uid);
                assert!(g.y >= 0.0 && g.bottom() <= b.h, "{}", def.uid);
                for s in &b.slots {
                    let h = s.hit(def);
                    let apart = h.right() <= g.x
                        || h.x >= g.right()
                        || h.bottom() <= g.y
                        || h.y >= g.bottom();
                    assert!(apart, "{} {}", def.uid, s.k);
                }
            }
        }
    }
}
