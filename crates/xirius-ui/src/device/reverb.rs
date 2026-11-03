//! The Reverb's window (UI-330): how its sound dies away, worked out from
//! the parameters as the engine uses them. Across is the time from 10 ms to
//! 20 s on a log scale, up and down the level down to -60 dB: the dry
//! column, the early reflections, the tail falling to -60 dB at mid
//! frequencies and, quicker, at 8 kHz. The foot's point moves the
//! pre-delay, the tail's the decay (its wheel the damping), the fourth
//! reflection's the size across and the early level up and down. Under it
//! the Color band (Low Cut and Damping at its ends) and the six knobs. The
//! reflections light and a dot runs down the tail as what went in sounds
//! (ENG-072's onsets).

use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::stock::reverb;
use xirius_params::BuiltinDef;

use super::control::{self, DIAL, Look, NAME_H, VALUE_H};
use super::delay::{self, BAND_H};
use super::graph::{self, Axis, Choice, Drive, Point, View};
use super::layout::{Body, Name, Shape, Slot};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The width the window opens at.
pub const NATURAL_W: f32 = 640.0;
/// The narrowest it goes.
pub const MIN_W: f32 = 360.0;
/// The knobs in their row.
pub const KNOBS: [&str; 6] = ["size", "decay", "predelay", "early", "width", "mix"];
/// The decays the tail's point offers (s).
pub const DECAYS: [f64; 4] = [0.8, 1.5, 2.5, 4.0];
/// The ends of the Color band.
pub const COLOR: [&str; 2] = ["low_cut", "damping"];
/// The high frequency whose tail is drawn too (Hz).
pub const HIGH: f64 = 8000.0;
/// The middle one, whose tail is the decay (Hz).
pub const MIDDLE: f64 = 1000.0;
/// How long a reflection stays lit once it sounds (s).
pub const LIT: f64 = 0.08;
/// The time across: the scale is `1 + t / T0`, up to `TMAX` (s).
pub const T0: f64 = 0.01;
pub const TMAX: f64 = 24.0;
/// How low the plane goes (dB).
pub const FLOOR: f64 = -60.0;
/// The reflection whose head is the size and early level's point.
pub const GRIP: usize = 3;
/// Where the tail starts against a full wet sound: about the network's sum
/// at its head.
const TAIL_TOP: f64 = 0.45;

const PAD: f32 = 8.0;
const PAD_FOOT: f32 = 4.0;
const GAP: f32 = 8.0;
const PLANE_H: f32 = 210.0;
const PLANE_H_NARROW: f32 = 180.0;
/// Below this the plane is lower.
const NARROW: f32 = 560.0;
/// How far in from the plane's sides the time runs.
const INSET: f32 = 22.0;
/// How far down from the plane's top 0 dB is, and up from its foot -60.
const TOP: f32 = 10.0;
const FOOT: f32 = 30.0;
const DRY_W: f32 = 6.0;
const EARLY_W: f32 = 4.0;
const LIVE: f32 = 4.0;
/// How far the size's point goes from end to end across, and the early
/// level's up (px).
const SIZE_TRAVEL: f32 = 200.0;
const EARLY_TRAVEL: f32 = 150.0;
const KNOB_W: f32 = 76.0;
const KNOB_GAP: f32 = 2.0;
const TIME_MARKS: [(f64, &str); 6] = [
    (0.01, "10 ms"),
    (0.05, "50 ms"),
    (0.2, "200 ms"),
    (1.0, "1 s"),
    (5.0, "5 s"),
    (20.0, "20 s"),
];
const LEVEL_MARKS: [(f64, &str); 2] = [(-20.0, "-20"), (-40.0, "-40")];

fn cell_h(dial: f32) -> f32 {
    dial + 2.0 + VALUE_H + NAME_H
}

fn index(def: &BuiltinDef, id: &str) -> usize {
    def.param(id).map_or(0, |(k, _)| k)
}

fn plain(def: &BuiltinDef, values: &[f64], id: &str) -> f64 {
    Shape {
        w: 0.0,
        band: 0,
        values,
    }
    .plain(def, id)
}

/// Is a body `w` wide the narrow one?
pub fn narrow(w: f32) -> bool {
    w < NARROW
}

/// The body at the width `s.w`: the plane, the Color band, and the knobs in
/// a row (two rows of three when they do not fit).
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(MIN_W);
    let inner = w - PAD * 2.0;
    let mut b = Body {
        w,
        ..Body::default()
    };
    let mut y = PAD;
    let plane_h = if narrow(w) { PLANE_H_NARROW } else { PLANE_H };
    let plane = Rect::new(PAD, y, inner, plane_h);
    y += plane_h + GAP;
    let band = Rect::new(PAD, y, inner, BAND_H);
    y += BAND_H + GAP;
    b.graphs = vec![plane, band];
    b.names.push(Name {
        r: Rect::new(band.x + 4.0, band.y, 60.0, 14.0),
        text: "Color",
        size: FontSize::Caption,
        strong: false,
    });
    let row_of = |n: usize| n as f32 * KNOB_W + (n - 1) as f32 * KNOB_GAP;
    let per = if row_of(KNOBS.len()) <= inner {
        KNOBS.len()
    } else {
        KNOBS.len() / 2
    };
    let x0 = PAD + (inner - row_of(per)) / 2.0;
    let knob_h = cell_h(DIAL);
    for (i, id) in KNOBS.iter().enumerate() {
        let (row, col) = (i / per, i % per);
        b.slots.push(Slot {
            k: index(def, id),
            look: Look::Knob,
            dial: DIAL,
            cell: Rect::new(
                x0 + col as f32 * (KNOB_W + KNOB_GAP),
                y + row as f32 * knob_h,
                KNOB_W,
                knob_h,
            ),
            label: None,
            dim: false,
        });
    }
    y += knob_h * KNOBS.len().div_ceil(per) as f32;
    b.h = y + PAD_FOOT;
    b
}

// ------------------------------------------------------------------ the plane

/// The amplitude's level (dB), none far down.
fn db(a: f64) -> f64 {
    20.0 * a.max(1e-6).log10()
}

/// What the Reverb's sound is, worked out from its parameters as the engine
/// uses them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sound {
    /// The pre-delay (s).
    pub pre: f64,
    /// The dry and the wet gains (the engine's mix).
    pub dry: f64,
    pub wet: f64,
    /// The early level (0..1).
    pub early: f64,
    /// When the tail starts (s), its level there (dB), and how long it
    /// takes to fall 60 dB at the middle and at the high frequency (s).
    pub start: f64,
    pub top: f64,
    pub decay: f64,
    pub high: f64,
    /// The reflections: when (s) and how loud (gain).
    pub reflections: [(f64, f64); 8],
}

/// The Reverb's sound at its values, at the song's rate.
pub fn shape(def: &BuiltinDef, values: &[f64], rate: f64) -> Sound {
    let p = |id: &str| plain(def, values, id);
    let rate = if rate > 0.0 { rate } else { 48_000.0 };
    let (size, pre_ms, decay, damping) = (p("size"), p("predelay"), p("decay"), p("damping"));
    let mix = p("mix") * 0.01;
    let wet = (2.0 * mix).min(1.0);
    Sound {
        pre: pre_ms * 0.001,
        dry: (2.0 * (1.0 - mix)).min(1.0),
        wet,
        early: p("early") * 0.01,
        start: reverb::tail_start(size, pre_ms),
        top: db(wet * TAIL_TOP),
        decay: reverb::decay_at(size, decay, damping, MIDDLE, rate),
        high: reverb::decay_at(size, decay, damping, HIGH, rate),
        reflections: reverb::reflections(size, pre_ms),
    }
}

impl Sound {
    /// The tail's level (dB) at `t` s along a fall of `rt` s.
    pub fn tail(&self, t: f64, rt: f64) -> f64 {
        self.top + FLOOR * (t - self.start) / rt.max(1e-3)
    }

    /// The `k`th reflection's level (dB).
    pub fn reflection(&self, k: usize) -> f64 {
        db(self.wet * self.early * self.reflections[k].1)
    }
}

/// The time axis on the plane `r` (s; from 0).
pub fn time_axis(r: Rect) -> Axis {
    Axis::log_from(r.x + INSET, r.right() - INSET, 0.0, TMAX, T0)
}

/// The level axis on the plane `r` (dB; 0 near the top, -60 near the foot).
pub fn level_axis(r: Rect) -> Axis {
    Axis::lin(r.y + TOP, r.bottom() - FOOT, 0.0, FLOOR)
}

/// The points on the plane `r`: the pre-delay's at the foot, the decay's
/// on the tail halfway along it (its wheel the damping; its menu the usual
/// decays), and the size and early level's on the fourth reflection's head.
pub fn points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let norm = |k: usize| {
        values
            .get(k)
            .copied()
            .unwrap_or(def.params[k].default_norm())
    };
    let text = |k: usize| control::text(&def.params[k], norm(k));
    let s = shape(def, values, 0.0);
    let (tx, ly) = (time_axis(r), level_axis(r));
    let base = ly.px(FLOOR);
    let (kp, kd, kdamp, ks, ke) = (
        index(def, "predelay"),
        index(def, "decay"),
        index(def, "damping"),
        index(def, "size"),
        index(def, "early"),
    );
    // The pre-delay in ms on the time's own scale (10 ms is T0).
    let pre = Point {
        name: control::name(def, &def.params[kp]).to_string(),
        label: String::new(),
        value: text(kp),
        at: (tx.px(s.pre), base),
        x: Some(Drive {
            k: kp,
            axis: Axis::log_from(tx.a, tx.b, 0.0, TMAX * 1000.0, T0 * 1000.0),
        }),
        y: None,
        wheel: Some(kp),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    // Halfway along the tail is `pre + decay / 2`: the decay on the time's
    // scale, held to the pre-delay.
    let decay = plain(def, values, "decay");
    let half = s.pre + decay / 2.0;
    let menu = DECAYS
        .iter()
        .map(|&v| {
            Some(Choice {
                label: format!("Decay を {v} s に"),
                set: vec![(kd, def.params[kd].to_norm(v))],
                on: (decay - v).abs() < 0.01,
            })
        })
        .collect();
    let tail = Point {
        name: control::name(def, &def.params[kd]).to_string(),
        label: String::new(),
        value: text(kd),
        at: (tx.px(half), ly.px(s.tail(half, s.decay).max(FLOOR))),
        x: Some(Drive {
            k: kd,
            axis: Axis::log_from(
                tx.a,
                tx.b,
                -2.0 * s.pre,
                2.0 * (TMAX - s.pre),
                2.0 * (s.pre + T0),
            ),
        }),
        y: None,
        wheel: Some(kdamp),
        on: true,
        menu,
        rest: Vec::new(),
    };
    // Across from where the pre-delay is, which the size does not move.
    let at = tx.px(s.pre);
    let early = Point {
        name: control::name(def, &def.params[ke]).to_string(),
        label: String::new(),
        value: format!("Size {}, Early {}", text(ks), text(ke)),
        at: (
            tx.px(s.reflections[GRIP].0),
            ly.px(s.reflection(GRIP).max(FLOOR)),
        ),
        x: Some(Drive {
            k: ks,
            axis: Axis::lin(at, at + SIZE_TRAVEL, 0.0, 100.0),
        }),
        y: Some(Drive {
            k: ke,
            axis: Axis::lin(base, base - EARLY_TRAVEL, 0.0, 100.0),
        }),
        wheel: Some(ke),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    vec![pre, tail, early]
}

/// Is the reflection at `at` s lit `since` s after an onset?
pub fn lit(since: f64, at: f64) -> bool {
    (at..at + LIT).contains(&since)
}

/// Where the dot running down the tail is `since` s after an onset, while
/// it is on the tail: the time (s) and the level (dB).
pub fn live(s: &Sound, since: f64) -> Option<(f64, f64)> {
    (s.start..s.start + s.decay)
        .contains(&since)
        .then(|| (since, s.tail(since, s.decay)))
}

/// Paints the plane `r`: the scales, the tail at the middle as an area with
/// its line and at 8 kHz under it, the dry column, the reflections, the
/// 8 kHz decay's tag and the dot running down the tail.
pub fn paint_plane(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    let s = shape(def, values, view.rate);
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let local = Rect::new(0.0, 0.0, r.w, r.h);
        let (tx, ly) = (time_axis(local), level_axis(local));
        let base = ly.px(FLOOR);
        for (t, name) in TIME_MARKS {
            let cell = Rect::new(r.x + tx.px(t) - 30.0, r.bottom() - 18.0, 60.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                cell,
                Align::Center,
            );
        }
        for (d, name) in LEVEL_MARKS {
            let cell = Rect::new(r.right() - 46.0, r.y + ly.px(d) - 7.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                c.ink,
                cell,
                Align::Right,
            );
        }
        let curve = |rt: f64| -> Vec<(f32, f32)> {
            (0..=60)
                .map(|i| {
                    let t = s.start + f64::from(i) / 60.0 * rt;
                    (tx.px(t), ly.px(s.tail(t, rt).max(FLOOR)))
                })
                .collect()
        };
        let (mid, high) = (curve(s.decay), curve(s.high));
        let (ink, blue, strong) = if view.bypassed {
            let dim = c.scroll_thumb[0];
            (dim, dim, dim)
        } else {
            (c.ink, c.ink_blue, c.ink_strong)
        };
        if s.wet > 0.0 {
            graph::area(pen, r, &mid, base, c.surface_3);
            graph::area(pen, r, &high, base, c.clip);
            graph::stroke(pen, r, &high, 2.0, blue);
            graph::stroke(pen, r, &mid, 2.0, strong);
        }
        if s.dry > 0.0 {
            let x = tx.px(0.0);
            let bar = [(x, ly.px(db(s.dry).max(FLOOR))), (x, base)];
            graph::stroke(pen, r, &bar, DRY_W, ink);
        }
        let since = view.echo.filter(|_| !view.bypassed);
        if s.early > 0.0 && s.wet > 0.0 {
            for (k, (at, _)) in s.reflections.iter().enumerate() {
                let x = tx.px(*at);
                let on = since.is_some_and(|t| lit(t, *at));
                let bar = [(x, ly.px(s.reflection(k).max(FLOOR))), (x, base)];
                graph::stroke(pen, r, &bar, EARLY_W, if on { strong } else { blue });
            }
        }
        let k = index(def, "decay");
        let tag = format!(
            "Decay @ 8 kHz {}",
            control::text(&def.params[k], def.params[k].to_norm(s.high))
        );
        pen.text(
            &tag,
            FontSize::Micro,
            Weight::Regular,
            blue,
            Rect::new(r.right() - 206.0, r.y + 12.0, 200.0, 14.0),
            Align::Right,
        );
        if let Some((t, d)) = since.and_then(|t| live(&s, t)) {
            // A disc: a ring as wide as its hole.
            let (x, y) = (tx.px(t), ly.px(d));
            let ring: Vec<(f32, f32)> = (0..=16)
                .map(|i| {
                    let a = i as f32 / 16.0 * std::f32::consts::TAU;
                    (x + a.cos() * LIVE / 2.0, y + a.sin() * LIVE / 2.0)
                })
                .collect();
            graph::stroke(pen, r, &ring, LIVE, strong);
        }
    });
}

/// The points on the Color band `r`: Low Cut and Damping at its ends.
pub fn band_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    delay::cut_points(def, r, values, COLOR)
}

/// Paints the Color band `r`.
pub fn paint_band(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64]) {
    delay::paint_cuts(pen, def, r, values, COLOR);
}
