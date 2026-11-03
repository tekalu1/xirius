//! Where a device's controls sit in its window's body (UI-324): each one a
//! slot with its parameter, its look and its cell. The rects are the body's
//! own (its top left corner at 0, 0); the window moves them to where it is.
//! A device without its own layout has its controls in rows, in the order
//! `xirius-params` lists them.

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

/// A device's body: its size, its controls and its graphs' planes (UI-325).
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub w: f32,
    pub h: f32,
    pub slots: Vec<Slot>,
    pub graphs: Vec<Rect>,
}

impl Body {
    /// The slot of the parameter `k`.
    pub fn slot(&self, k: usize) -> Option<&Slot> {
        self.slots.iter().find(|s| s.k == k)
    }
}

/// Controls in a row: three, four for a long list.
fn columns(n: usize) -> usize {
    if n > 12 { 4 } else { 3 }
}

/// The body of a device.
pub fn body(def: &BuiltinDef) -> Body {
    rows(def)
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
            }
        })
        .collect();
    Body {
        w: PAD * 2.0 + c as f32 * CELL_W + (c - 1) as f32 * COL_GAP,
        h: PAD * 2.0 + lines * CELL_H + (lines - 1.0) * ROW_GAP,
        slots,
        graphs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parameter_has_one_slot_inside_the_body() {
        for def in xirius_params::BUILTINS {
            let b = body(def);
            assert_eq!(b.slots.len(), def.params.len(), "{}", def.uid);
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
