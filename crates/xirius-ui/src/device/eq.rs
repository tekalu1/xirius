//! The EQ's window (UI-326): the curve over a frequency plane with a point
//! per band and the analyser behind it, the output's meter and knob beside
//! it, and under it a column of knobs per band. A narrow window keeps one
//! band's column at a time and tabs to choose it.

use xirius_design::{FontSize, Radius, Weight};
use xirius_engine::stock::Tap;
use xirius_engine::stock::eq::Response;
use xirius_params::{BandType, BuiltinDef, EQ_BANDS, EqBand};

use super::control::{self, DIAL, DIAL_OUT, DIAL_SM, Look, NAME_H, VALUE_H};
use super::graph::{self, Axis, Choice, Drive, Point, View};
use super::layout::{Body, Name, Shape, Slot, Tab};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The width the window opens at.
pub const NATURAL_W: f32 = 760.0;
/// The narrowest it goes.
pub const MIN_W: f32 = 420.0;
/// The bands' names, low to high (Pro EQ3's, without its MF).
pub const NAMES: [&str; 6] = ["LC", "LF", "LMF", "HMF", "HF", "HC"];
/// The band chosen when the window opens.
pub const FIRST_BAND: usize = 2;
/// What the analyser shows, in its menu's order.
pub const SPECTRUM: [&str; 4] = ["Off", "Input", "Output", "Input + Output"];

const PAD: f32 = 8.0;
const PAD_FOOT: f32 = 6.0;
const GAP: f32 = 6.0;
const GRAPH_H: f32 = 260.0;
const SIDE_W: f32 = 76.0;
const SIDE_GAP: f32 = 8.0;
const METER_W: f32 = 5.0;
const METER_H: f32 = 200.0;
/// Below this the bands' area keeps one band and tabs.
const NARROW: f32 = 600.0;
const TAB_H: f32 = 28.0;
const COL_GAP: f32 = 2.0;
const HEAD_H: f32 = 24.0;
const DOT_BUTTON: f32 = 20.0;
const TYPE_H: f32 = 22.0;
const FREQ_W: f32 = 64.0;
const SMALL_W: f32 = 54.0;
/// The gap between the knobs of a narrow window's one band.
const ROW_GAP: f32 = 12.0;
const FMIN: f64 = 20.0;
const FMAX: f64 = 20_000.0;
/// The dB the plane spans each way from 0.
const RANGE: f64 = 18.0;
/// The analyser's scale: its floor and its span, in dB.
const SPEC_FLOOR: f32 = -96.0;
const SPEC_SPAN: f32 = 102.0;
/// A band's own curve is drawn only where it is this far from 0 dB (on 0 dB
/// it would read as a line of the scale).
const OWN_MIN: f64 = 0.15;

fn cell_h(dial: f32) -> f32 {
    dial + 2.0 + VALUE_H + NAME_H
}

/// Does a body `w` wide keep one band at a time?
pub fn narrow(w: f32) -> bool {
    w - PAD * 2.0 < NARROW
}

/// The band `i` as the values have it: on, and its type.
fn band(def: &BuiltinDef, s: &Shape, i: usize) -> (bool, BandType) {
    let b = &EQ_BANDS[i];
    (s.plain(def, b.on) >= 0.5, b.type_of(s.plain(def, b.kind)))
}

/// The band whose parameter `k` is, if one is.
pub fn band_of(def: &BuiltinDef, k: usize) -> Option<usize> {
    let id = def.params.get(k)?.id;
    EQ_BANDS
        .iter()
        .position(|b| [b.on, b.kind, b.freq, b.gain, b.q, b.slope].contains(&id))
}

fn index(def: &BuiltinDef, id: &str) -> usize {
    def.param(id).map_or(0, |(k, _)| k)
}

/// The body at the width `s.w`: the graph and its side, the tabs of a narrow
/// one, and the bands' columns (only the chosen one's while narrow).
pub fn body(def: &BuiltinDef, s: &Shape) -> Body {
    let w = s.w.max(MIN_W);
    let inner = w - PAD * 2.0;
    let mut b = Body {
        w,
        ..Body::default()
    };
    let plane = Rect::new(PAD, PAD, inner - SIDE_W - SIDE_GAP, GRAPH_H);
    b.graphs.push(plane);
    b.spectrum = Some(Rect::new(plane.x + 6.0, plane.y + 6.0, 0.0, 22.0));
    // The side: the output's meter at the top, its knob at the foot.
    let side = Rect::new(plane.right() + SIDE_GAP, PAD, SIDE_W, GRAPH_H);
    let pair = METER_W * 2.0 + 2.0;
    let mx = side.x + (SIDE_W - pair) / 2.0;
    for ch in 0..2 {
        let x = mx + ch as f32 * (METER_W + 2.0);
        b.meters.push(Rect::new(x, side.y + 8.0, METER_W, METER_H));
    }
    let knob_y = side.y + 8.0 + METER_H;
    b.slots.push(Slot {
        k: index(def, "output"),
        look: Look::Knob,
        dial: DIAL_OUT,
        cell: Rect::new(side.x, knob_y, SIDE_W, cell_h(DIAL_OUT)),
        label: None,
        dim: false,
    });
    let mut y = PAD + GRAPH_H.max(knob_y + cell_h(DIAL_OUT) + 2.0 - PAD) + GAP;
    let chosen = s.band.min(5);
    if narrow(w) {
        let tw = (inner - COL_GAP * 5.0) / 6.0;
        for (i, name) in NAMES.iter().enumerate() {
            let (on, _) = band(def, s, i);
            b.tabs.push(Tab {
                r: Rect::new(PAD + i as f32 * (tw + COL_GAP), y, tw, TAB_H),
                text: name,
                chosen: i == chosen,
                on,
            });
        }
        y += TAB_H + GAP;
        let col = Rect::new(PAD, y, inner, 0.0);
        y += column(def, s, chosen, col, true, &mut b);
    } else {
        let cw = (inner - COL_GAP * 5.0) / 6.0;
        let mut h: f32 = 0.0;
        for i in 0..6 {
            let col = Rect::new(PAD + i as f32 * (cw + COL_GAP), y, cw, 0.0);
            h = h.max(column(def, s, i, col, false, &mut b));
        }
        y += h;
    }
    b.h = y + PAD_FOOT;
    b
}

/// Lays out band `i`'s column at `col` (its height left to it): its head
/// (the on dot, the name, the type), its frequency's knob and its small
/// pair (the gain or a cut's slope, and the Q). A cut's slope takes the
/// gain's seat and a notch leaves it empty, so the Q stays put. In a row
/// (`row`), the knobs sit side by side under the head. Returns its height.
fn column(def: &BuiltinDef, s: &Shape, i: usize, col: Rect, row: bool, b: &mut Body) -> f32 {
    let band_def: &EqBand = &EQ_BANDS[i];
    let (on, kind) = band(def, s, i);
    let dim = !on;
    let head = Rect::new(col.x + 2.0, col.y + 4.0, col.w - 4.0, HEAD_H);
    let inside = Rect::new(head.x + 2.0, head.y, head.w - 4.0, HEAD_H);
    b.slots.push(Slot {
        k: index(def, band_def.on),
        look: Look::Dot,
        dial: DOT_BUTTON,
        cell: Rect::new(
            inside.x,
            inside.y + (HEAD_H - DOT_BUTTON) / 2.0,
            DOT_BUTTON,
            DOT_BUTTON,
        ),
        label: None,
        dim,
    });
    let name_x = inside.x + DOT_BUTTON + 3.0;
    let name_w = name_width(NAMES[i]);
    b.names.push(Name {
        r: Rect::new(name_x, inside.y, name_w, HEAD_H),
        text: NAMES[i],
        size: FontSize::Small,
        strong: on,
    });
    let type_x = name_x + name_w + 3.0;
    b.slots.push(Slot {
        k: index(def, band_def.kind),
        look: Look::Menu,
        dial: TYPE_H,
        cell: Rect::new(
            type_x,
            inside.y + (HEAD_H - TYPE_H) / 2.0,
            (inside.right() - type_x).max(0.0),
            TYPE_H,
        ),
        label: None,
        dim,
    });
    let knob = |k: usize, dial: f32, w: f32, x: f32, y: f32, label: &'static str| Slot {
        k,
        look: Look::Knob,
        dial,
        cell: Rect::new(x, y, w, cell_h(dial)),
        label: Some(label),
        dim,
    };
    let first: Option<(usize, &'static str)> = if kind.is_cut() {
        Some((index(def, band_def.slope), "Slope"))
    } else if kind.has_gain() {
        Some((index(def, band_def.gain), "Gain"))
    } else {
        None
    };
    // The approved window has no Q on a cut (its slope is what it shapes).
    let q = (!kind.is_cut()).then(|| index(def, band_def.q));
    let (fh, sh) = (cell_h(DIAL), cell_h(DIAL_SM));
    let top = head.bottom() + 2.0;
    let h = if row {
        let span = FREQ_W + ROW_GAP + SMALL_W * 2.0;
        let x = col.x + (col.w - span) / 2.0;
        let small_y = top + (fh - sh) / 2.0;
        b.slots.push(knob(
            index(def, band_def.freq),
            DIAL,
            FREQ_W,
            x,
            top,
            "Freq",
        ));
        let px = x + FREQ_W + ROW_GAP;
        if let Some((k, label)) = first {
            b.slots.push(knob(k, DIAL_SM, SMALL_W, px, small_y, label));
        }
        if let Some(k) = q {
            b.slots
                .push(knob(k, DIAL_SM, SMALL_W, px + SMALL_W, small_y, "Q"));
        }
        4.0 + HEAD_H + 2.0 + fh + 6.0
    } else {
        let cx = col.x + col.w / 2.0;
        b.slots.push(knob(
            index(def, band_def.freq),
            DIAL,
            FREQ_W,
            cx - FREQ_W / 2.0,
            top,
            "Freq",
        ));
        let small_y = top + fh + 2.0;
        if let Some((k, label)) = first {
            b.slots
                .push(knob(k, DIAL_SM, SMALL_W, cx - SMALL_W, small_y, label));
        }
        if let Some(k) = q {
            b.slots.push(knob(k, DIAL_SM, SMALL_W, cx, small_y, "Q"));
        }
        4.0 + HEAD_H + 2.0 + fh + 2.0 + sh + 6.0
    };
    if i == s.band.min(5) {
        b.faces.push(Rect::new(col.x, col.y, col.w, h));
    }
    h
}

/// How wide a band's name is drawn (small, semibold).
fn name_width(name: &str) -> f32 {
    crate::fonts::measure(name, FontSize::Small, Weight::Semibold).ceil()
}

// ------------------------------------------------------------- the analyser

/// The analyser's button at `at` (its corner and height) for what it shows:
/// "Spectrum", the choice and a chevron.
pub fn spectrum_rect(at: Rect, tap: Tap) -> Rect {
    let label = SPECTRUM[tap as usize];
    let w = 8.0
        + crate::fonts::measure("Spectrum", FontSize::Caption, Weight::Regular)
        + 5.0
        + crate::fonts::measure(label, FontSize::Caption, Weight::Medium)
        + 5.0
        + 12.0
        + 4.0;
    Rect::new(at.x, at.y, w.ceil(), at.h)
}

/// Paints the analyser's button `r`: lighter under the pointer.
pub fn paint_spectrum(pen: &mut Pen, r: Rect, tap: Tap, hot: bool) {
    let c = pen.c;
    pen.round(r, if hot { c.clip } else { c.surface_3 }, Radius::S);
    let mut x = r.x + 8.0;
    let w = pen.measure("Spectrum", FontSize::Caption, Weight::Regular);
    pen.text(
        "Spectrum",
        FontSize::Caption,
        Weight::Regular,
        c.ink,
        Rect::new(x, r.y, w, r.h),
        Align::Left,
    );
    x += w + 5.0;
    let label = SPECTRUM[tap as usize];
    let w = pen.measure(label, FontSize::Caption, Weight::Medium);
    pen.text(
        label,
        FontSize::Caption,
        Weight::Medium,
        c.ink_strong,
        Rect::new(x, r.y, w, r.h),
        Align::Left,
    );
    x += w + 5.0;
    pen.icon(
        "chevron-down",
        Rect::new(x, r.y + (r.h - 12.0) / 2.0, 12.0, 12.0),
        c.ink,
    );
}

// ------------------------------------------------------------------ the plane

/// The plane's frequency axis.
pub fn freq_axis(r: Rect) -> Axis {
    Axis::log(r.x, r.right(), FMIN, FMAX)
}

/// The plane's gain axis: ±18 dB, 14 px in from its top and its foot.
pub fn gain_axis(r: Rect) -> Axis {
    Axis::lin(r.y + 14.0, r.bottom() - 14.0, RANGE, -RANGE)
}

/// What the cached curves of a plane `r` are made of: the values, the band drawn on its own
/// and the rate, which shapes the curve near the top (L-ui-1).
pub(crate) fn curve_stamp(values: &[f64], r: Rect, band: Option<usize>, rate: f64) -> u64 {
    graph::stamp(values, r) ^ band.map_or(0, |b| b as u64 + 1) ^ rate.to_bits().rotate_left(17)
}

/// The rate the curve is worked out at when the song's is not known.
const RATE: f64 = 48_000.0;

/// The bands' points on the plane `r`: x moves the frequency, y the gain of
/// a band with a gain (a cut sits on its curve, a notch on 0 dB), the wheel
/// its Q (a cut's slope). Each one's menu has its types, a cut's slopes and
/// its on.
pub fn points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    points_at(def, r, values, RATE)
}

pub fn points_at(def: &BuiltinDef, r: Rect, values: &[f64], rate: f64) -> Vec<Point> {
    let s = Shape {
        w: r.w,
        values,
        ..Shape::default()
    };
    let fx = freq_axis(r);
    let gy = gain_axis(r);
    let response = Response::new(values, rate);
    let norm = |k: usize| {
        values
            .get(k)
            .copied()
            .unwrap_or(def.params[k].default_norm())
    };
    (0..6)
        .map(|i| {
            let b = &EQ_BANDS[i];
            let (on, kind) = band(def, &s, i);
            let (kf, kg, kq, ks, kk, ko) = (
                index(def, b.freq),
                index(def, b.gain),
                index(def, b.q),
                index(def, b.slope),
                index(def, b.kind),
                index(def, b.on),
            );
            let freq = s.plain(def, b.freq);
            let y = if kind.has_gain() {
                gy.px(s.plain(def, b.gain))
            } else if kind.is_cut() {
                let at = if on {
                    response.band_db(i, freq)
                } else {
                    let mut lit = values.to_vec();
                    lit.resize(def.params.len(), 0.0);
                    for (k, v) in lit.iter_mut().enumerate().skip(values.len()) {
                        *v = def.params[k].default_norm();
                    }
                    lit[ko] = 1.0;
                    Response::new(&lit, rate).band_db(i, freq)
                };
                gy.px(at.clamp(-RANGE, RANGE))
            } else {
                gy.px(0.0)
            };
            let text = |k: usize| control::text(&def.params[k], norm(k));
            let mut tip = vec![text(kf)];
            if kind.has_gain() {
                tip.push(text(kg));
            }
            if !kind.is_cut() {
                tip.push(format!("Q {}", text(kq)));
            }
            if kind.is_cut() {
                tip.push(text(ks));
            }
            let mut menu: Vec<Option<Choice>> = b
                .types
                .iter()
                .enumerate()
                .map(|(t, ty)| {
                    Some(Choice {
                        label: ty.label().to_string(),
                        set: vec![(kk, choice(b.types.len(), t))],
                        on: *ty == kind,
                    })
                })
                .collect();
            if kind.is_cut() {
                menu.push(None);
                let n = xirius_params::SLOPES.len();
                let now = (norm(ks) * (n - 1) as f64).round() as usize;
                for (t, label) in xirius_params::SLOPES.iter().enumerate() {
                    menu.push(Some(Choice {
                        label: label.to_string(),
                        set: vec![(ks, choice(n, t))],
                        on: t == now,
                    }));
                }
            }
            menu.push(None);
            menu.push(Some(Choice {
                label: if on {
                    "バンドをオフ"
                } else {
                    "バンドをオン"
                }
                .to_string(),
                set: vec![(ko, if on { 0.0 } else { 1.0 })],
                on: false,
            }));
            Point {
                name: format!("{} {}", NAMES[i], kind.label()),
                label: NAMES[i].to_string(),
                value: if on {
                    tip.join("  ")
                } else {
                    format!("Off {}", tip.join("  "))
                },
                at: (fx.px(freq), y),
                x: Some(Drive { k: kf, axis: fx }),
                y: kind.has_gain().then_some(Drive { k: kg, axis: gy }),
                wheel: Some(if kind.is_cut() { ks } else { kq }),
                on,
                menu,
                rest: vec![kk, kq, ks, ko, kg],
            }
        })
        .collect()
}

/// A line across a plane, in its own place.
type Line = Vec<(f32, f32)>;

/// The value (0..1) of the `t`th of `n` choices.
pub fn choice(n: usize, t: usize) -> f64 {
    if n <= 1 {
        0.0
    } else {
        t as f64 / (n - 1) as f64
    }
}

/// The plane's frequencies named along its foot: fewer on a narrow one.
fn freq_marks(w: f32) -> &'static [(f64, &'static str)] {
    if w < 420.0 {
        &[(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")]
    } else {
        &[
            (50.0, "50"),
            (100.0, "100"),
            (200.0, "200"),
            (500.0, "500"),
            (1000.0, "1k"),
            (2000.0, "2k"),
            (5000.0, "5k"),
            (10_000.0, "10k"),
        ]
    }
}

const GAIN_MARKS: [(f64, &str); 5] = [
    (12.0, "+12"),
    (6.0, "+6"),
    (0.0, "0"),
    (-6.0, "\u{2212}6"),
    (-12.0, "\u{2212}12"),
];

/// How many points a curve across a plane `w` wide has.
fn steps(w: f32) -> usize {
    ((w / 3.0).round() as usize).max(120)
}

/// The whole curve across the plane `r` (its own place), and the chosen
/// band's own where it leaves 0 dB (runs of points).
pub fn curves(values: &[f64], r: Rect, band: Option<usize>, rate: f64) -> (Line, Vec<Line>) {
    let response = Response::new(values, rate);
    let local = Rect::new(0.0, 0.0, r.w, r.h);
    let (fx, gy) = (freq_axis(local), gain_axis(local));
    let n = steps(r.w);
    let mut whole = Vec::with_capacity(n + 1);
    let mut own: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut run = false;
    for k in 0..=n {
        let x = k as f32 / n as f32 * r.w;
        let f = fx.value(x);
        let y = |db: f64| gy.px(db).clamp(-4.0, r.h + 4.0);
        whole.push((x, y(response.db(f))));
        let db = band.map_or(0.0, |b| response.band_db(b, f));
        if db.abs() > OWN_MIN && db.is_finite() {
            if !run {
                own.push(Vec::new());
            }
            if let Some(last) = own.last_mut() {
                last.push((x, y(db)));
            }
            run = true;
        } else if db.abs() > OWN_MIN {
            // A notch's own frequency: down past the foot.
            if !run {
                own.push(Vec::new());
            }
            if let Some(last) = own.last_mut() {
                last.push((x, r.h + 4.0));
            }
            run = true;
        } else {
            run = false;
        }
    }
    (whole, own)
}

/// The analyser's line across the plane `r` (its own place) for levels in
/// dB at frequencies spaced evenly on its axis.
fn spectrum_line(levels: &[f32], r: Rect) -> Vec<(f32, f32)> {
    let last = levels.len().saturating_sub(1).max(1) as f32;
    levels
        .iter()
        .enumerate()
        .map(|(k, d)| {
            let y = r.h - (d - SPEC_FLOOR) / SPEC_SPAN * r.h;
            (k as f32 / last * r.w, y.clamp(0.0, r.h))
        })
        .collect()
}

/// Paints the plane `r`: its decades' faces, the analyser, the scale, the
/// chosen band's own curve and the whole curve (dim while the EQ is off).
pub fn paint(pen: &mut Pen, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let fx = freq_axis(r);
        graph::faces(
            pen,
            r,
            fx,
            &[FMIN, 100.0, 1000.0, 10_000.0, FMAX],
            c.surface_0,
        );
        let [input, output] = view.spectrum;
        if !input.is_empty() {
            graph::area(pen, r, &spectrum_line(input, r), r.h, c.surface_3);
        }
        if !output.is_empty() {
            graph::area(pen, r, &spectrum_line(output, r), r.h, c.clip);
        }
        graph::ticks(pen, r, fx, freq_marks(r.w), true);
        graph::ticks(pen, r, gain_axis(r), &GAIN_MARKS, false);
        let rate = if view.rate > 0.0 { view.rate } else { RATE };
        let band = view.chosen.filter(|&b| {
            EQ_BANDS.get(b).is_some_and(|eb| {
                xirius_params::EQ.param(eb.on).is_some_and(|(k, spec)| {
                    spec.to_plain(values.get(k).copied().unwrap_or(spec.default_norm())) >= 0.5
                })
            })
        });
        let make = || curves(values, r, band, rate);
        let (whole, own) = match view.plugin {
            Some(id) => {
                let stamp = curve_stamp(values, r, band, rate);
                let both = graph::curve((id, 0), stamp, || make().0);
                let mine = graph::curve((id, 1), stamp, || {
                    // The runs of the band's own curve, one after another;
                    // a NaN between them breaks the line.
                    let mut flat = Vec::new();
                    for run in make().1 {
                        if !flat.is_empty() {
                            flat.push((f32::NAN, f32::NAN));
                        }
                        flat.extend(run);
                    }
                    flat
                });
                (both.to_vec(), split(&mine))
            }
            None => make(),
        };
        let (line, blue) = if view.bypassed {
            (c.scroll_thumb[0], c.scroll_thumb[0])
        } else {
            (c.ink_strong, c.ink_blue)
        };
        for run in &own {
            graph::stroke(pen, r, run, 2.0, blue);
        }
        graph::stroke(pen, r, &whole, 2.5, line);
    });
}

/// Runs of points broken where a NaN is.
fn split(flat: &[(f32, f32)]) -> Vec<Vec<(f32, f32)>> {
    flat.split(|p| p.0.is_nan())
        .filter(|r| !r.is_empty())
        .map(<[_]>::to_vec)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_engine::stock::eq::response_db;

    fn defaults() -> Vec<f64> {
        xirius_params::EQ
            .params
            .iter()
            .map(|p| p.default_norm())
            .collect()
    }

    fn set(v: &mut [f64], id: &str, plain: f64) {
        let (k, spec) = xirius_params::EQ.param(id).unwrap();
        v[k] = spec.to_norm(plain);
    }

    fn shape(w: f32, band: usize, values: &[f64]) -> Shape<'_> {
        Shape {
            w,
            band,
            values,
            ..Shape::default()
        }
    }

    #[test]
    fn ui_326_the_curve_is_the_engines_within_a_hundredth_of_a_db() {
        let mut v = defaults();
        set(&mut v, "low_gain", 4.0);
        set(&mut v, "mid1_freq", 350.0);
        set(&mut v, "mid1_gain", -6.0);
        set(&mut v, "mid1_q", 2.5);
        set(&mut v, "b4_type", 3.0);
        set(&mut v, "mid2_freq", 3000.0);
        set(&mut v, "high_cut_on", 1.0);
        set(&mut v, "high_cut_freq", 12_000.0);
        set(&mut v, "output", -2.0);
        let r = Rect::new(0.0, 0.0, 640.0, GRAPH_H);
        let (fx, gy) = (freq_axis(r), gain_axis(r));
        let rate = 48_000.0;
        let (whole, _) = curves(&v, r, None, rate);
        let n = whole.len() - 1;
        for i in 0..64 {
            let k = i * n / 63;
            let (x, y) = whole[k];
            let f = fx.value(x);
            let want = response_db(&v, f, rate);
            if want.abs() < RANGE {
                assert!(
                    (gy.value(y) - want).abs() < 0.01,
                    "{f} Hz: {} against {want}",
                    gy.value(y)
                );
            }
        }
    }

    #[test]
    fn ui_326_the_bands_are_named_like_pro_eq_and_each_has_its_column() {
        let v = defaults();
        let def = &xirius_params::EQ;
        let b = body(def, &shape(NATURAL_W, FIRST_BAND, &v));
        let names: Vec<_> = b.names.iter().map(|n| n.text).collect();
        assert_eq!(names, ["LC", "LF", "LMF", "HMF", "HF", "HC"]);
        assert!(b.tabs.is_empty());
        assert_eq!(b.faces.len(), 1);
        // The low cut is off by default: its name is the plain ink.
        assert!(!b.names[0].strong && b.names[2].strong);
        let freq = |id: &str| b.slots.iter().find(|s| s.k == index(def, id)).copied();
        let lmf = freq("mid1_freq").unwrap();
        assert_eq!(lmf.label, Some("Freq"));
        assert!(b.faces[0].contains(lmf.cell.x + 1.0, lmf.cell.y + 1.0));
        // A cut has its slope in the gain's seat and no Q; a notch no gain.
        assert!(freq("low_cut_slope").is_some());
        assert!(freq("b1_gain").is_none() && freq("b1_q").is_none());
        let mut n = v.clone();
        set(&mut n, "b4_type", 3.0);
        let b = body(def, &shape(NATURAL_W, FIRST_BAND, &n));
        let has = |id: &str| b.slots.iter().any(|s| s.k == index(def, id));
        assert!(!has("mid2_gain") && has("mid2_q") && !has("b4_slope"));
        for s in &b.slots {
            assert!(s.cell.right() <= b.w + 0.5 && s.cell.bottom() <= b.h + 0.5);
        }
    }

    #[test]
    fn ui_326_a_narrow_window_has_tabs_and_one_band() {
        let v = defaults();
        let def = &xirius_params::EQ;
        let b = body(def, &shape(MIN_W, 4, &v));
        assert_eq!(b.tabs.len(), 6);
        assert!(b.tabs[4].chosen && !b.tabs[0].on);
        assert_eq!(b.names.len(), 1);
        assert_eq!(b.names[0].text, "HF");
        let freqs = b.slots.iter().filter(|s| s.label == Some("Freq")).count();
        assert_eq!(freqs, 1);
        let wide = body(def, &shape(NATURAL_W, 4, &v));
        assert!(b.h < wide.h);
        assert!(narrow(MIN_W) && !narrow(NATURAL_W));
        // Its height follows its width, not the band chosen.
        assert_eq!(body(def, &shape(MIN_W, 0, &v)).h, b.h);
    }

    #[test]
    fn ui_326_a_point_moves_its_band_and_its_menu_sets_its_type() {
        let mut v = defaults();
        let def = &xirius_params::EQ;
        let r = Rect::new(10.0, 40.0, 600.0, GRAPH_H);
        let pts = points(def, r, &v);
        assert_eq!(pts.len(), 6);
        let lmf = &pts[2];
        assert_eq!(lmf.label, "LMF");
        assert_eq!(lmf.name, "LMF Peaking");
        assert_eq!(lmf.x.unwrap().k, index(def, "mid1_freq"));
        assert_eq!(lmf.y.unwrap().k, index(def, "mid1_gain"));
        assert_eq!(lmf.wheel, Some(index(def, "mid1_q")));
        assert!((lmf.at.0 - freq_axis(r).px(400.0)).abs() < 0.5);
        assert!((lmf.at.1 - (r.y + r.h / 2.0)).abs() < 0.5);
        // The low cut: off, on its curve, its wheel the slope, no y.
        let lc = &pts[0];
        assert!(!lc.on && lc.y.is_none());
        assert_eq!(lc.wheel, Some(index(def, "low_cut_slope")));
        assert!(
            lc.at.1 > r.y + r.h / 2.0,
            "a cut sits below 0 dB at its corner"
        );
        let labels: Vec<_> = lc.menu.iter().flatten().map(|c| c.label.clone()).collect();
        assert_eq!(
            labels,
            [
                "Low Cut",
                "Low Shelf",
                "Peaking",
                "Notch",
                "12 dB/oct",
                "24 dB/oct",
                "48 dB/oct",
                "バンドをオン"
            ]
        );
        // Choosing Notch for the HMF puts its point on 0 dB and drops its y.
        let notch = pts[3].menu[3].clone().unwrap();
        for (k, x) in notch.set {
            v[k] = x;
        }
        let p = &points(def, r, &v)[3];
        assert_eq!(p.name, "HMF Notch");
        assert!(p.y.is_none());
        // Its menu's «back to the defaults» puts its type back too.
        assert!(p.resets().contains(&index(def, "b4_type")));
        // A drag right raises the frequency by the axis.
        let x = p.x.unwrap();
        let spec = &def.params[x.k];
        let up = x.moved(spec, v[x.k], 60.0);
        assert!(spec.to_plain(up) > spec.to_plain(v[x.k]));
    }

    #[test]
    fn ui_326_the_chosen_bands_own_curve_is_where_it_leaves_0_db() {
        let mut v = defaults();
        set(&mut v, "mid1_gain", 9.0);
        let r = Rect::new(0.0, 0.0, 600.0, GRAPH_H);
        let (_, own) = curves(&v, r, Some(2), RATE);
        assert_eq!(own.len(), 1);
        let (_, none) = curves(&v, r, Some(3), RATE);
        assert!(none.is_empty(), "a flat band draws nothing of its own");
    }

    #[test]
    fn ui_326_every_band_parameter_is_its_bands() {
        let def = &xirius_params::EQ;
        assert_eq!(band_of(def, index(def, "mid2_q")), Some(3));
        assert_eq!(band_of(def, index(def, "low_cut_on")), Some(0));
        assert_eq!(band_of(def, index(def, "output")), None);
    }

    #[test]
    fn ui_326_a_1khz_tone_peaks_over_1khz_on_the_plane() {
        let rate = 48_000.0;
        let tone: Vec<f32> = (0..xirius_engine::analyzer::FFT_LEN)
            .map(|n| (std::f64::consts::TAU * 1000.0 * n as f64 / rate).sin() as f32 * 0.5)
            .collect();
        let levels = xirius_engine::analyzer::spectrum(&tone, rate, 200);
        let r = Rect::new(0.0, 0.0, 600.0, 260.0);
        let line = spectrum_line(&levels, r);
        let top = line.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        let want = freq_axis(r).px(1000.0);
        assert!((top.0 - want).abs() < 6.0, "peak at {} not {want}", top.0);
        assert!(top.1 < r.h * 0.2, "it stands high: {}", top.1);
    }
}
