//! Where a device's controls sit in its window's body (UI-324): each one a
//! slot with its parameter, its look and its cell. The rects are the body's
//! own (its top left corner at 0, 0); the window moves them to where it is.
//! A device without its own layout has its controls in rows, in the order
//! `xirius-params` lists them.

use xirius_design::FontSize;
use xirius_params::BuiltinDef;

use super::control::{self, DIAL_OUT, Look, NAME_H, VALUE_H};
use crate::studio::Rect;

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
            Look::Dot | Look::Menu => c,
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
            ..self
        }
    }
}

/// What a body is laid out for: its width, the band chosen (a narrow EQ
/// shows that one's knobs) and the values (0..1) it shows (a band's type
/// swaps its knobs).
#[derive(Clone, Copy, Debug)]
pub struct Shape<'a> {
    pub w: f32,
    pub band: usize,
    pub values: &'a [f64],
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
        _ => rows(def),
    }
}

/// The width a device's window opens at.
pub fn natural_w(def: &BuiltinDef) -> f32 {
    match def.uid {
        "eq" => super::eq::NATURAL_W,
        "compressor" => super::compressor::NATURAL_W,
        "limiter" => super::limiter::NATURAL_W,
        _ => rows(def).w,
    }
}

/// The size of a device's body at the width `w`, its values at their
/// defaults (its height follows its width, not its values).
pub fn size(def: &BuiltinDef, w: f32) -> (f32, f32) {
    let values: Vec<f64> = def.params.iter().map(|p| p.default_norm()).collect();
    let b = body(
        def,
        &Shape {
            w,
            band: 0,
            values: &values,
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
                band: 0,
                values: &values,
            };
            let b = body(def, &shape);
            if def.uid == "eq" {
                // Its bands' knobs follow their types; each band has its own.
                assert!(b.slots.len() <= def.params.len());
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
