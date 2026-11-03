//! The Compressor's window (UI-327): the transfer curve (in across, out up,
//! without the makeup) with a point at its threshold and one at its ratio,
//! the last four seconds of the input, the output and the reduction, the In,
//! GR and Out meters, the knobs in a row and the sidechain's strip under
//! them. A narrow window puts the history under the curve.

use std::time::Duration;

use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::stock::reduction;
use xirius_params::BuiltinDef;

use super::control::{self, DIAL, DIAL_SM, Look, NAME_H, TOGGLE_H, VALUE_H};
use super::dynamics::{self, FLOOR, LABELS, LEVEL_MARKS};
use super::graph::{self, Axis, Drive, Point, View};
use super::layout::{Body, Name, Shape, Slot};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The width the window opens at.
pub const NATURAL_W: f32 = 680.0;
/// The narrowest it goes.
pub const MIN_W: f32 = 360.0;
/// How long the history is.
pub const SPAN: Duration = Duration::from_secs(4);
/// The knobs in their row, in Studio Pro's order.
pub const KNOBS: [&str; 8] = [
    "threshold",
    "ratio",
    "knee",
    "attack",
    "release",
    "gain",
    "makeup",
    "mix",
];

const PAD: f32 = 8.0;
const PAD_FOOT: f32 = 4.0;
const GAP: f32 = 6.0;
const PLANE_GAP: f32 = 8.0;
const XFER: f32 = 208.0;
/// Below this the history goes under the curve.
const NARROW: f32 = 560.0;
const HISTORY_H: f32 = 120.0;
const KNOB_W: f32 = 76.0;
const KNOB_GAP: f32 = 2.0;
const SC_KNOB_W: f32 = 64.0;
const SC_PAD: f32 = 10.0;
/// How far a drag of the ratio's point takes it from 1:1 to 20:1.
const RATIO_TRAVEL: f32 = 150.0;

fn cell_h(dial: f32) -> f32 {
    dial + 2.0 + VALUE_H + NAME_H
}

fn index(def: &BuiltinDef, id: &str) -> usize {
    def.param(id).map_or(0, |(k, _)| k)
}

/// Does a body `w` wide put the history under the curve?
pub fn narrow(w: f32) -> bool {
    w < NARROW
}

/// The body at the width `s.w`.
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(MIN_W);
    let inner = w - PAD * 2.0;
    let mut b = Body {
        w,
        ..Body::default()
    };
    let side_w = dynamics::side_w(&LABELS);
    let mut y = PAD;
    if narrow(w) {
        let x = (inner - side_w - PLANE_GAP).min(XFER);
        let xfer = Rect::new(PAD, y, x, x);
        let side = Rect::new(xfer.right() + PLANE_GAP, y, side_w, x);
        y += x + PLANE_GAP;
        let hist = Rect::new(PAD, y, inner, HISTORY_H);
        y += HISTORY_H;
        b.graphs = vec![xfer, hist];
        b.levels = dynamics::side(side, &LABELS);
    } else {
        let xfer = Rect::new(PAD, y, XFER, XFER);
        let side = Rect::new(w - PAD - side_w, y, side_w, XFER);
        let hx = xfer.right() + PLANE_GAP;
        let hist = Rect::new(hx, y, side.x - PLANE_GAP - hx, XFER);
        y += XFER;
        b.graphs = vec![xfer, hist];
        b.levels = dynamics::side(side, &LABELS);
    }
    y += GAP;
    // The knobs: one row, or two of four in a narrow window.
    let per = if inner >= KNOBS.len() as f32 * (KNOB_W + KNOB_GAP) {
        KNOBS.len()
    } else {
        KNOBS.len() / 2
    };
    let span = per as f32 * KNOB_W + (per - 1) as f32 * KNOB_GAP;
    let x0 = PAD + (inner - span) / 2.0;
    for (i, id) in KNOBS.iter().enumerate() {
        let (col, row) = ((i % per) as f32, (i / per) as f32);
        b.slots.push(Slot {
            k: index(def, id),
            look: Look::Knob,
            dial: DIAL,
            cell: Rect::new(
                x0 + col * (KNOB_W + KNOB_GAP),
                y + row * (cell_h(DIAL) + GAP),
                KNOB_W,
                cell_h(DIAL),
            ),
            label: None,
            dim: false,
        });
    }
    let rows = KNOBS.len().div_ceil(per) as f32;
    y += rows * cell_h(DIAL) + (rows - 1.0) * GAP + GAP;
    // The sidechain's strip: its name, the filter and listen, the cuts.
    let strip_h = cell_h(DIAL_SM) + 8.0;
    let strip = Rect::new(PAD, y, inner, strip_h);
    b.faces.push(strip);
    let mid = strip.y + strip_h / 2.0;
    let mut x = strip.x + SC_PAD;
    let name_w = crate::fonts::measure("Sidechain", FontSize::Small, Weight::Semibold).ceil();
    b.names.push(Name {
        r: Rect::new(x, mid - NAME_H / 2.0, name_w, NAME_H),
        text: "Sidechain",
        size: FontSize::Small,
        strong: true,
    });
    x += name_w + 12.0;
    for id in ["filter", "listen"] {
        let k = index(def, id);
        let tw = control::toggle_w(control::name(def, &def.params[k]));
        b.slots.push(Slot {
            k,
            look: Look::Toggle,
            dial: TOGGLE_H,
            cell: Rect::new(x, mid - TOGGLE_H / 2.0, tw, TOGGLE_H),
            label: None,
            dim: false,
        });
        x += tw + 6.0;
    }
    x += 6.0;
    for id in ["low_cut", "high_cut"] {
        b.slots.push(Slot {
            k: index(def, id),
            look: Look::Knob,
            dial: DIAL_SM,
            cell: Rect::new(x, strip.y + 4.0, SC_KNOB_W, cell_h(DIAL_SM)),
            label: None,
            dim: false,
        });
        x += SC_KNOB_W;
    }
    y += strip_h;
    b.h = y + PAD_FOOT;
    b
}

// ------------------------------------------------------------------ the curve

/// The curve's input axis on the plane `r`.
pub fn in_axis(r: Rect) -> Axis {
    Axis::lin(r.x, r.right(), FLOOR, 0.0)
}

/// The curve's output axis on the plane `r`.
pub fn out_axis(r: Rect) -> Axis {
    Axis::lin(r.bottom(), r.y, FLOOR, 0.0)
}

/// What the curve's knobs are: the threshold, the ratio and the knee (dB,
/// x:1, dB).
pub fn shape(def: &BuiltinDef, values: &[f64]) -> (f64, f64, f64) {
    let s = Shape {
        w: 0.0,
        band: 0,
        values,
    };
    (
        s.plain(def, "threshold"),
        s.plain(def, "ratio"),
        s.plain(def, "knee"),
    )
}

/// The output (dB) for an input (dB), without the makeup: the engine's own
/// curve.
pub fn out_db(level: f64, (t, r, k): (f64, f64, f64)) -> f64 {
    level - reduction(level, t, r, k)
}

/// The curve across the plane `r`, in its own place.
pub fn curve(r: Rect, knobs: (f64, f64, f64)) -> Vec<(f32, f32)> {
    let local = Rect::new(0.0, 0.0, r.w, r.h);
    let (ax, ay) = (in_axis(local), out_axis(local));
    let n = (r.w / 2.0).round().max(60.0) as usize;
    (0..=n)
        .map(|i| {
            let x = i as f32 / n as f32 * r.w;
            let v = ax.value(x);
            (x, ay.px(out_db(v, knobs)))
        })
        .collect()
}

/// The points on the curve's plane `r`: the ratio's at its right end (up
/// and down, a drag down a stronger ratio; the wheel turns it too), then the
/// threshold's where it bends (across; the wheel its knee), drawn on top.
pub fn points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let knobs = shape(def, values);
    let (ax, ay) = (in_axis(r), out_axis(r));
    let (kt, kr, kk) = (
        index(def, "threshold"),
        index(def, "ratio"),
        index(def, "knee"),
    );
    let norm = |k: usize| {
        values
            .get(k)
            .copied()
            .unwrap_or(def.params[k].default_norm())
    };
    let text = |k: usize| control::text(&def.params[k], norm(k));
    let ratio = Point {
        name: "Ratio".to_string(),
        label: String::new(),
        value: text(kr),
        at: (r.right() - 8.0, ay.px(out_db(0.0, knobs))),
        x: None,
        y: Some(Drive {
            k: kr,
            axis: Axis::log(r.y, r.y + RATIO_TRAVEL, 1.0, 20.0),
        }),
        wheel: Some(kr),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    let threshold = Point {
        name: "Threshold".to_string(),
        label: String::new(),
        value: format!("{}  Knee {}", text(kt), text(kk)),
        at: (ax.px(knobs.0), ay.px(out_db(knobs.0, knobs))),
        x: Some(Drive { k: kt, axis: ax }),
        y: None,
        wheel: Some(kk),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    vec![ratio, threshold]
}

/// Paints the curve's plane `r`: the face over the threshold a step darker,
/// the scale, the curve, and the dot where the input is now.
pub fn paint_curve(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    let knobs = shape(def, values);
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let (ax, ay) = (in_axis(r), out_axis(r));
        let tx = ax.px(knobs.0).clamp(r.x, r.right());
        pen.fill(Rect::new(tx, r.y, r.right() - tx, r.h), c.surface_0);
        let foot: Vec<(f64, &str)> = LEVEL_MARKS.iter().rev().copied().collect();
        graph::ticks(pen, r, ax, &foot, true);
        for (v, name) in LEVEL_MARKS {
            let cell = Rect::new(r.x + 4.0, ay.px(v) - 7.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                cell,
                Align::Left,
            );
        }
        let line = if view.bypassed {
            c.scroll_thumb[0]
        } else {
            c.ink_strong
        };
        graph::stroke(pen, r, &curve(r, knobs), 2.0, line);
        if let Some(d) = view.dynamics
            && !view.bypassed
            && f64::from(d.now.0) > FLOOR
        {
            let v = f64::from(d.now.0);
            let (x, y) = (ax.px(v), ay.px(out_db(v, knobs)));
            pen.round(
                Rect::new(x - 4.0, y - 4.0, 8.0, 8.0),
                c.ink_strong,
                Radius::Pill,
            );
        }
    });
}

/// Paints the history's plane `r`.
pub fn paint_history(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let empty = Default::default();
    let history = view.dynamics.map_or(&empty, |d| &d.history);
    let (t, ..) = shape(def, values);
    dynamics::paint_history(pen, r, history, SPAN, Some(t), view.bypassed);
}
