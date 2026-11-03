//! The Delay's window (UI-329): the top row (Sync, the tempo and the Beats
//! box, or the Time box; Ping-Pong at the right), the taps' plane with the
//! dry column and the repeats (their heights their levels, up and down with
//! Ping-Pong), the Color band with Low Cut and High Cut at its ends, and the
//! Feedback and Mix knobs. The first repeat's head moves the time (the beats
//! while synced), the second's the feedback. The columns light as the repeats
//! of what went in sound (ENG-072's onsets).

use std::time::{Duration, Instant};

use xirius_design::{FontSize, Radius};
use xirius_params::{BuiltinDef, DIVISIONS};

use super::control::{self, DIAL, Look, NAME_H, TOGGLE_H, VALUE_H};
use super::graph::{self, Axis, Choice, Drive, Point, View};
use super::layout::{Body, Name, Shape, Slot};
use crate::draw::Pen;
use crate::studio::Rect;

/// The width the window opens at.
pub const NATURAL_W: f32 = 640.0;
/// The narrowest it goes.
pub const MIN_W: f32 = 360.0;
/// The knobs at the right of the Color band.
pub const KNOBS: [&str; 2] = ["feedback", "mix"];
/// How far the taps' plane goes: seconds, or beats while synced.
pub const SECONDS: f64 = 2.2;
pub const BEATS: f64 = 4.4;
/// How long a column stays lit once its repeat sounds (s).
pub const LIT: f64 = 0.12;
/// How long after the last onset the columns keep lighting.
pub const ECHO_FOR: Duration = Duration::from_secs(6);
/// A note's length in wholes, and what its feel makes of it (the engine's).
pub const WHOLES: [f64; 6] = [1.0 / 32.0, 1.0 / 16.0, 1.0 / 8.0, 1.0 / 4.0, 1.0 / 2.0, 1.0];
pub const FEELS: [f64; 3] = [1.0, 1.5, 2.0 / 3.0];
/// What the Beats box adds for a feel.
const FEEL_TAGS: [&str; 3] = ["", " D", " T"];
/// The quietest repeat drawn, under the dry (dB).
const RANGE_DB: f64 = 48.0;

const PAD: f32 = 8.0;
const PAD_FOOT: f32 = 6.0;
const GAP: f32 = 8.0;
const TAPS_H: f32 = 200.0;
const TAPS_H_NARROW: f32 = 180.0;
pub const BAND_H: f32 = 62.0;
/// Below this the knobs go under the Color band.
const NARROW: f32 = 520.0;
/// How far in from the plane's sides the time runs.
const INSET: f32 = 22.0;
/// How far in from its top and its foot the loudest column reaches.
const EDGE: f32 = 14.0;
const BAR_W: f32 = 6.0;
const KNOB_W: f32 = 76.0;
const KNOB_GAP: f32 = 2.0;
const BOX_W: f32 = 84.0;
const TEMPO_W: f32 = 60.0;
/// Where the band's points sit, down from its top.
const BAND_DOT: f32 = 38.0;
/// The ends of the Color band.
const CUTS: [&str; 2] = ["low_cut", "high_cut"];
const BAND_MARKS: [(f64, &str); 3] = [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")];
const TIME_MARKS: [(f64, &str); 4] = [(0.5, "500 ms"), (1.0, "1 s"), (1.5, "1.5 s"), (2.0, "2 s")];

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

/// Does a body `w` wide put the knobs under the Color band?
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
    let mut y = PAD;
    let top = |k: usize, look: Look, x: f32, cw: f32| Slot {
        k,
        look,
        dial: TOGGLE_H,
        cell: Rect::new(x, y, cw, TOGGLE_H),
        label: None,
        dim: false,
    };
    let sync_w = control::toggle_w("Sync");
    b.slots
        .push(top(index(def, "sync"), Look::Toggle, PAD, sync_w));
    let mut x = PAD + sync_w + GAP;
    if s.plain(def, "sync") >= 0.5 {
        b.tempo = Some(Rect::new(x, y, TEMPO_W, TOGGLE_H));
        x += TEMPO_W + GAP;
        b.slots
            .push(top(index(def, "division"), Look::Menu, x, BOX_W));
    } else {
        b.slots.push(top(index(def, "time"), Look::Field, x, BOX_W));
    }
    let pp_w = control::toggle_w("Ping-Pong");
    b.slots.push(top(
        index(def, "ping_pong"),
        Look::Toggle,
        w - PAD - pp_w,
        pp_w,
    ));
    y += TOGGLE_H + GAP;
    let taps_h = if narrow(w) { TAPS_H_NARROW } else { TAPS_H };
    let taps = Rect::new(PAD, y, inner, taps_h);
    y += taps_h + GAP;
    let span = KNOBS.len() as f32 * KNOB_W + (KNOBS.len() - 1) as f32 * KNOB_GAP;
    let knob_h = cell_h(DIAL);
    let (band, kx, ky) = if narrow(w) {
        let band = Rect::new(PAD, y, inner, BAND_H);
        y += BAND_H + GAP;
        let ky = y;
        y += knob_h;
        (band, PAD + (inner - span) / 2.0, ky)
    } else {
        let row = knob_h.max(BAND_H);
        let band = Rect::new(
            PAD,
            y + (row - BAND_H) / 2.0,
            inner - span - GAP * 2.0,
            BAND_H,
        );
        let ky = y + (row - knob_h) / 2.0;
        y += row;
        (band, w - PAD - span, ky)
    };
    b.graphs = vec![taps, band];
    b.names.push(Name {
        r: Rect::new(band.x + 4.0, band.y, 60.0, 14.0),
        text: "Color",
        size: FontSize::Caption,
        strong: false,
    });
    for (i, id) in KNOBS.iter().enumerate() {
        b.slots.push(Slot {
            k: index(def, id),
            look: Look::Knob,
            dial: DIAL,
            cell: Rect::new(kx + i as f32 * (KNOB_W + KNOB_GAP), ky, KNOB_W, knob_h),
            label: None,
            dim: false,
        });
    }
    b.h = y + PAD_FOOT;
    b
}

// ------------------------------------------------------------------ the taps

/// What the taps' plane shows: the gap between repeats (in the plane's
/// units: seconds, or beats while synced), the feedback (0..0.95) and
/// whether the repeats go from side to side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taps {
    pub synced: bool,
    pub period: f64,
    pub feedback: f64,
    pub ping_pong: bool,
}

fn chosen(def: &BuiltinDef, values: &[f64], id: &str, n: usize) -> usize {
    (plain(def, values, id).round().max(0.0) as usize).min(n - 1)
}

/// The note and its feel the Beats box is on.
pub fn note(def: &BuiltinDef, values: &[f64]) -> (usize, usize) {
    (
        chosen(def, values, "division", WHOLES.len()),
        chosen(def, values, "feel", FEELS.len()),
    )
}

/// The taps of the values (0..1).
pub fn taps(def: &BuiltinDef, values: &[f64]) -> Taps {
    let synced = plain(def, values, "sync") >= 0.5;
    let (d, f) = note(def, values);
    Taps {
        synced,
        period: if synced {
            WHOLES[d] * FEELS[f] * 4.0
        } else {
            plain(def, values, "time") / 1000.0
        },
        feedback: plain(def, values, "feedback") / 100.0,
        ping_pong: plain(def, values, "ping_pong") >= 0.5,
    }
}

/// The gap between the repeats in seconds at the tempo `bpm`.
pub fn seconds(t: &Taps, bpm: f64) -> f64 {
    if t.synced {
        t.period * 60.0 / bpm.max(1.0)
    } else {
        t.period
    }
}

/// How tall the `n`th repeat (the first is 1) stands, as a share of the
/// dry's: its level on a scale of 48 dB.
pub fn height(feedback: f64, n: usize) -> f64 {
    if n <= 1 {
        return 1.0;
    }
    let level = feedback.max(0.0).powi(n as i32 - 1);
    if level <= 0.0 {
        return 0.0;
    }
    (1.0 + 20.0 * level.log10() / RANGE_DB).clamp(0.0, 1.0)
}

/// Is the `i`th column (the dry is 0) lit `since` seconds after an onset,
/// the repeats `period` seconds apart?
pub fn lit(since: f64, period: f64, i: usize) -> bool {
    let at = i as f64 * period;
    since >= at && since < at + LIT
}

/// The Beats box's label for a note and its feel (`1/8 D`).
pub fn beats_label(division: usize, feel: usize) -> String {
    format!(
        "{}{}",
        DIVISIONS.get(division).copied().unwrap_or(""),
        FEEL_TAGS.get(feel).copied().unwrap_or("")
    )
}

/// What the Beats box says for the values.
pub fn beats_text(def: &BuiltinDef, values: &[f64]) -> String {
    let (d, f) = note(def, values);
    beats_label(d, f)
}

/// The Beats box's menu: each note straight, then dotted, then as a
/// triplet, the one it is on ticked. Each row sets the note and its feel.
pub fn beats_menu(def: &BuiltinDef, values: &[f64]) -> Vec<Option<Choice>> {
    let (kd, kf) = (index(def, "division"), index(def, "feel"));
    let (sd, sf) = (&def.params[kd], &def.params[kf]);
    let now = note(def, values);
    let mut rows = Vec::new();
    for f in 0..FEELS.len() {
        if f > 0 {
            rows.push(None);
        }
        for d in 0..WHOLES.len() {
            rows.push(Some(Choice {
                label: beats_label(d, f),
                set: vec![(kd, sd.to_norm(d as f64)), (kf, sf.to_norm(f as f64))],
                on: now == (d, f),
            }));
        }
    }
    rows
}

/// The taps' time axis on the plane `r`.
pub fn time_axis(r: Rect, synced: bool) -> Axis {
    let hi = if synced { BEATS } else { SECONDS };
    Axis::lin(r.x + INSET, r.right() - INSET, 0.0, hi)
}

/// Where the `n`th column stands across the plane (the dry is 0), held to
/// the plane.
fn column_x(r: Rect, t: &Taps, n: usize) -> f32 {
    time_axis(r, t.synced)
        .px(t.period * n as f64)
        .min(r.right() - INSET)
}

fn half(r: Rect) -> f32 {
    r.h / 2.0 - EDGE
}

/// The ends of the `n`th column (top, foot), the dry's whole height.
fn column(r: Rect, t: &Taps, n: usize) -> (f32, f32) {
    let mid = r.y + r.h / 2.0;
    let h = half(r) * height(t.feedback, n) as f32;
    match (n, t.ping_pong) {
        (0, _) => (mid - half(r), mid + half(r)),
        (_, false) => (mid - h, mid + h),
        (n, true) if n % 2 == 1 => (mid - h, mid),
        _ => (mid, mid + h),
    }
}

/// How many repeats the plane shows.
fn repeats(t: &Taps) -> usize {
    let hi = if t.synced { BEATS } else { SECONDS };
    if t.period <= 0.0 {
        return 0;
    }
    ((hi / t.period).floor() as usize).clamp(1, 64)
}

/// The points on the taps' plane `r`: the first repeat's head (Time, or
/// Beats while synced, across) and the second's (Feedback, up and down).
pub fn points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    let t = taps(def, values);
    let norm = |k: usize| {
        values
            .get(k)
            .copied()
            .unwrap_or(def.params[k].default_norm())
    };
    let text = |k: usize| control::text(&def.params[k], norm(k));
    let (a, b) = (r.x + INSET, r.right() - INSET);
    let first = if t.synced {
        let kd = index(def, "division");
        Point {
            name: "Beats".to_string(),
            label: String::new(),
            value: beats_text(def, values),
            at: (column_x(r, &t, 1), column(r, &t, 1).0),
            x: Some(Drive {
                k: kd,
                axis: Axis::lin(a, b, -0.5, WHOLES.len() as f64 - 0.5),
            }),
            y: None,
            wheel: Some(kd),
            on: true,
            menu: beats_menu(def, values),
            rest: vec![index(def, "feel")],
        }
    } else {
        let kt = index(def, "time");
        Point {
            name: "Time".to_string(),
            label: String::new(),
            value: text(kt),
            at: (column_x(r, &t, 1), column(r, &t, 1).0),
            x: Some(Drive {
                k: kt,
                axis: Axis::lin(a, b, 0.0, SECONDS * 1000.0),
            }),
            y: None,
            wheel: Some(kt),
            on: true,
            menu: Vec::new(),
            rest: Vec::new(),
        }
    };
    let kf = index(def, "feedback");
    let mid = r.y + r.h / 2.0;
    let (top, foot) = column(r, &t, 2);
    // The second column's head follows its level: on a log scale of the
    // feedback, none at the middle line.
    let (y, reach) = if t.ping_pong {
        (foot, mid + half(r))
    } else {
        (top, mid - half(r))
    };
    let quiet = 100.0 * 10f64.powf(-RANGE_DB / 20.0);
    let second = Point {
        name: "Feedback".to_string(),
        label: String::new(),
        value: text(kf),
        at: (column_x(r, &t, 2), y),
        x: None,
        y: Some(Drive {
            k: kf,
            axis: Axis::log(mid, reach, quiet, 100.0),
        }),
        wheel: Some(kf),
        on: true,
        menu: Vec::new(),
        rest: Vec::new(),
    };
    vec![first, second]
}

/// Paints the taps' plane `r`: the middle line, the time's marks, the dry
/// column and the repeats, those sounding now lit.
pub fn paint_taps(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], view: &View) {
    let c = pen.c;
    let t = taps(def, values);
    let bpm = if view.tempo > 0.0 { view.tempo } else { 120.0 };
    pen.round(r, c.surface_2, Radius::S);
    pen.mask(r, |pen| {
        let mid = r.h / 2.0;
        let line = [(INSET, mid), (r.w - INSET, mid)];
        graph::stroke(pen, r, &line, 1.0, c.scroll_thumb[0]);
        let ax = time_axis(r, t.synced);
        let per = if t.synced { bpm / 60.0 } else { 1.0 };
        let marks: Vec<(f64, &str)> = TIME_MARKS.iter().map(|&(s, n)| (s * per, n)).collect();
        graph::ticks(pen, r, ax, &marks, true);
        let gap = seconds(&t, bpm);
        let since = view.echo.filter(|_| !view.bypassed);
        for n in 0..=repeats(&t) {
            let x = column_x(r, &t, n);
            let (top, foot) = column(r, &t, n);
            if foot - top < 1.0 {
                continue;
            }
            let on = since.is_some_and(|s| lit(s, gap, n));
            let ink = if view.bypassed {
                c.scroll_thumb[0]
            } else if on {
                c.ink_strong
            } else if n == 0 {
                c.ink
            } else {
                c.ink_blue
            };
            let bar = [(x - r.x, top - r.y), (x - r.x, foot - r.y)];
            graph::stroke(pen, r, &bar, BAR_W, ink);
        }
    });
}

// ------------------------------------------------------------------ the band

/// The Color band's frequency axis on its plane `r`.
pub fn band_axis(r: Rect) -> Axis {
    Axis::log(r.x + INSET, r.right() - INSET, 20.0, 20_000.0)
}

/// The points on the Color band `r`: Low Cut and High Cut at its ends, each
/// with its value over it.
pub fn band_points(def: &BuiltinDef, r: Rect, values: &[f64]) -> Vec<Point> {
    cut_points(def, r, values, CUTS)
}

/// The points at the ends of a band `r` whose ends are the parameters `ids`
/// (the Delay's cuts, the Reverb's Low Cut and Damping).
pub fn cut_points(def: &BuiltinDef, r: Rect, values: &[f64], ids: [&str; 2]) -> Vec<Point> {
    let ax = band_axis(r);
    let y = r.y + BAND_DOT;
    ids.iter()
        .map(|id| {
            let k = index(def, id);
            let v = values
                .get(k)
                .copied()
                .unwrap_or(def.params[k].default_norm());
            let text = control::text(&def.params[k], v);
            Point {
                name: control::name(def, &def.params[k]).to_string(),
                label: text.clone(),
                value: text,
                at: (ax.px(def.params[k].to_plain(v)), y),
                x: Some(Drive { k, axis: ax }),
                y: None,
                wheel: Some(k),
                on: true,
                menu: Vec::new(),
                rest: Vec::new(),
            }
        })
        .collect()
}

/// Paints the Color band `r`: its rail, lighter between the cuts, and its
/// frequencies under it.
pub fn paint_band(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64]) {
    paint_cuts(pen, def, r, values, CUTS);
}

/// Paints a band `r` whose ends are the parameters `ids`.
pub fn paint_cuts(pen: &mut Pen, def: &BuiltinDef, r: Rect, values: &[f64], ids: [&str; 2]) {
    let c = pen.c;
    let ax = band_axis(r);
    let y = r.y + BAND_DOT;
    let rail = Rect::new(ax.a, y - 4.0, ax.b - ax.a, 8.0);
    pen.round(rail, c.surface_2, Radius::Pill);
    let (lo, hi) = (
        ax.px(plain(def, values, ids[0])),
        ax.px(plain(def, values, ids[1])),
    );
    if hi > lo {
        pen.round(
            Rect::new(lo, y - 4.0, hi - lo, 8.0),
            c.surface_3,
            Radius::Pill,
        );
    }
    let foot = Rect::new(r.x, r.y, r.w, r.h + 2.0);
    graph::ticks(pen, foot, ax, &BAND_MARKS, true);
}

// ------------------------------------------------------------------ lighting

/// The onsets a Delay's probe has counted and when the last came: its
/// columns light from then.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Echo {
    count: Option<u32>,
    pub at: Option<Instant>,
}

impl Echo {
    /// Takes the probe's running count of onsets at `now`: a rise is an
    /// onset (the first look only learns the count). True while the columns
    /// light.
    pub fn feed(&mut self, count: u32, now: Instant) -> bool {
        if self.count.is_some_and(|had| count != had) {
            self.at = Some(now);
        }
        self.count = Some(count);
        self.at
            .is_some_and(|t| now.saturating_duration_since(t) < ECHO_FOR)
    }

    /// Seconds since the last onset, while the columns light.
    pub fn since(&self, now: Instant) -> Option<f64> {
        let d = now.saturating_duration_since(self.at?);
        (d < ECHO_FOR).then_some(d.as_secs_f64())
    }
}
