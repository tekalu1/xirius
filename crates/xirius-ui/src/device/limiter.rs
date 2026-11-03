//! The Limiter's window (UI-328): the last five seconds of the input (with
//! its gain), the output and the reduction, the ceiling's line with a point
//! at its right end and the gain's point at the left, the Input, Reduction
//! and Output meters beside it, and the three knobs in a row. A narrow
//! window puts the meters under the history.

use std::time::Duration;

use xirius_design::{FontSize, Radius, Weight};
use xirius_params::BuiltinDef;

use super::control::{self, DIAL, Look, NAME_H, VALUE_H};
use super::dynamics::{self, STEP, WORDS};
use super::graph::{self, Axis, Choice, Drive, Point, View};
use super::layout::{Body, Shape, Slot};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The width the window opens at.
pub const NATURAL_W: f32 = 600.0;
/// The narrowest it goes.
pub const MIN_W: f32 = 320.0;
/// How long the history is.
pub const SPAN: Duration = Duration::from_secs(5);
/// The knobs in their row.
pub const KNOBS: [&str; 3] = ["input", "ceiling", "release"];
/// The ceilings its point's menu offers (dB).
pub const CEILINGS: [f64; 3] = [-0.1, -0.3, -1.0];
/// The history's scale: its foot and its top (dB).
pub const LO: f64 = -36.0;
pub const HI: f64 = 12.0;
/// The reduction the history's line spans (dB).
pub const GR_SPAN: f32 = 12.0;

const PAD: f32 = 8.0;
const PAD_FOOT: f32 = 4.0;
const GAP: f32 = 6.0;
const PLANE_GAP: f32 = 8.0;
const HISTORY_H: f32 = 208.0;
const HISTORY_H_NARROW: f32 = 180.0;
const SIDE_H: f32 = 202.0;
/// Below this the meters go under the history.
const NARROW: f32 = 560.0;
/// How far in from the plane's sides the history and its points are.
const INSET: f32 = 22.0;
/// How far in from its top and its foot the scale is.
const EDGE: f32 = 8.0;
const KNOB_W: f32 = 76.0;
const KNOB_GAP: f32 = 2.0;
const MARKS: [(f64, &str); 4] = [(6.0, "+6"), (0.0, "0"), (-12.0, "-12"), (-24.0, "-24")];

fn cell_h(dial: f32) -> f32 {
    dial + 2.0 + VALUE_H + NAME_H
}

fn index(def: &BuiltinDef, id: &str) -> usize {
    def.param(id).map_or(0, |(k, _)| k)
}

/// Does a body `w` wide put the meters under the history?
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
    let side_w = dynamics::side_w(&WORDS);
    let mut y = PAD;
    let side = if narrow(w) {
        let hist = Rect::new(PAD, y, inner, HISTORY_H_NARROW);
        y += HISTORY_H_NARROW + PLANE_GAP;
        b.graphs = vec![hist];
        let side = Rect::new(PAD, y, inner, SIDE_H);
        y += SIDE_H;
        side
    } else {
        let side = Rect::new(w - PAD - side_w, y, side_w, HISTORY_H);
        b.graphs = vec![Rect::new(PAD, y, side.x - PLANE_GAP - PAD, HISTORY_H)];
        y += HISTORY_H;
        side
    };
    b.levels = dynamics::side(side, &WORDS);
    for l in &mut b.levels {
        l.loud = true;
    }
    y += GAP;
    let span = KNOBS.len() as f32 * KNOB_W + (KNOBS.len() - 1) as f32 * KNOB_GAP;
    let x0 = PAD + (inner - span) / 2.0;
    for (i, id) in KNOBS.iter().enumerate() {
        b.slots.push(Slot {
            k: index(def, id),
            look: Look::Knob,
            dial: DIAL,
            cell: Rect::new(x0 + i as f32 * (KNOB_W + KNOB_GAP), y, KNOB_W, cell_h(DIAL)),
            label: None,
            dim: false,
        });
    }
    y += cell_h(DIAL);
    b.h = y + PAD_FOOT;
    b
}

// ------------------------------------------------------------------ the plane

/// The history's level axis on the plane `r`.
pub fn level_axis(r: Rect) -> Axis {
    Axis::lin(r.bottom() - EDGE, r.y + EDGE, LO, HI)
}

/// The gain's axis on the plane `r`: none at the foot of the scale, the
/// most at its top.
pub fn gain_axis(def: &BuiltinDef, r: Rect) -> Axis {
    let (lo, hi) = def.params[index(def, "input")].range();
    Axis::lin(r.bottom() - EDGE, r.y + EDGE, lo, hi)
}

fn plain(def: &BuiltinDef, values: &[f64], id: &str) -> f64 {
    Shape {
        w: 0.0,
        band: 0,
        values,
    }
    .plain(def, id)
}

/// The points on the history's plane `r`: the gain's at the left end, then
/// the ceiling's at the right end of its line, drawn on top. Both go up and
/// down; the ceiling's menu offers the usual ceilings.
pub fn points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let (kg, kc) = (index(def, "input"), index(def, "ceiling"));
    let norm = |k: usize| {
        values
            .get(k)
            .copied()
            .unwrap_or(def.params[k].default_norm())
    };
    let text = |k: usize| control::text(&def.params[k], norm(k));
    let ga = gain_axis(def, r);
    let la = level_axis(r);
    let ceiling = plain(def, values, "ceiling");
    let gain = Point {
        name: "Gain".to_string(),
        label: String::new(),
        value: text(kg),
        at: (r.x + INSET, ga.px(plain(def, values, "input"))),
        x: None,
        y: Some(Drive { k: kg, axis: ga }),
        wheel: Some(kg),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    let spec = &def.params[kc];
    let menu = CEILINGS
        .iter()
        .map(|&v| {
            Some(Choice {
                label: format!("Ceiling を {v:.1} dB に"),
                set: vec![(kc, spec.to_norm(v))],
                on: (ceiling - v).abs() < 0.01,
            })
        })
        .collect();
    let top = Point {
        name: "Ceiling".to_string(),
        label: String::new(),
        value: text(kc),
        at: (r.right() - INSET, la.px(ceiling)),
        x: None,
        y: Some(Drive { k: kc, axis: la }),
        wheel: Some(kc),
        on: true,
        menu,
        rest: Vec::new(),
    };
    vec![gain, top]
}

/// Paints the history's plane `r`: the face over the ceiling a step darker,
/// the input as an area, the output as a line, the reduction hanging from
/// the top, the scale, the ceiling's line and the gain's tag.
pub fn paint_history(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    let empty = Default::default();
    let history = view.dynamics.map_or(&empty, |d| &d.history);
    let ceiling = plain(def, values, "ceiling");
    let gain = plain(def, values, "input");
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let ay = level_axis(r);
        let cy = ay.px(ceiling);
        pen.fill(Rect::new(r.x, r.y, r.w, cy - r.y), c.surface_0);
        let cap = (SPAN.as_micros() / STEP.as_micros()).max(2) as f32;
        let n = history.len();
        let (a, b) = (INSET, r.w - INSET);
        let x = |i: usize| b - (n - 1 - i) as f32 / (cap - 1.0) * (b - a);
        let local = level_axis(Rect::new(0.0, 0.0, r.w, r.h));
        let line = |f: &dyn Fn(&dynamics::Sample) -> f32| -> Vec<(f32, f32)> {
            history
                .iter()
                .enumerate()
                .map(|(i, s)| (x(i), f(s)))
                .collect()
        };
        let (ink, blue) = if view.bypassed {
            (c.scroll_thumb[0], c.scroll_thumb[0])
        } else {
            (c.ink_strong, c.ink_blue)
        };
        if n > 1 {
            let input = line(&|s| local.px(f64::from(s.input)));
            graph::area(pen, r, &input, r.h, c.surface_3);
        }
        for (v, name) in MARKS {
            let cell = Rect::new(r.right() - 40.0 - 40.0, ay.px(v) - 7.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                cell,
                Align::Right,
            );
        }
        if n > 1 {
            let output = line(&|s| local.px(f64::from(s.output)));
            let span = r.h - EDGE * 2.0;
            let gr = line(&|s| (s.gr / GR_SPAN).clamp(0.0, 1.0) * span + EDGE);
            graph::stroke(pen, r, &output, 2.0, ink);
            graph::stroke(pen, r, &gr, 2.0, blue);
        }
        let ceiling_line = [(INSET, cy - r.y), (r.w - INSET, cy - r.y)];
        graph::stroke(pen, r, &ceiling_line, 1.5, ink);
        let gy = gain_axis(def, r).px(gain);
        let k = index(def, "input");
        let tag = format!(
            "Gain +{}",
            control::text(&def.params[k], def.params[k].to_norm(gain))
        );
        pen.text(
            &tag,
            FontSize::Micro,
            Weight::Regular,
            c.ink_strong,
            Rect::new(r.x + 40.0, gy - 7.0, 120.0, 14.0),
            Align::Left,
        );
    });
}
