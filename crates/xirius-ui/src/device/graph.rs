//! A device's graph (UI-325): the plane an EQ's curve, a compressor's knee
//! or a sample's wave sits on, and the points on it that move parameters. A
//! point's x and y each drive a parameter through the graph's axes; a drag
//! moves both and goes into the song as one step when it is let go, the
//! wheel turns a third, a double click types the first, Alt puts the second
//! and the third back.
//!
//! A curve is worked out once and kept until what shapes it changes
//! ([`curve`]); it is painted with `paint_path`, and the scale is written
//! beside the plane, with faces for its decades and no thin lines across it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::id::PluginId;
use xirius_design::{FontSize, Radius, Weight};
use xirius_params::{BuiltinDef, ParamSpec};

use super::control;
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// How big a point looks; hovered or chosen, a little bigger.
pub const POINT: f32 = 12.0;
pub const POINT_HOT: f32 = 16.0;
/// How far round a point a press takes it.
pub const POINT_HIT: f32 = 24.0;
/// How far the drag moves a point with Shift held, against the pointer.
pub const FINE: f32 = 0.25;
/// The tip over a point: its padding.
const TIP_PAD: (f32, f32) = (8.0, 4.0);
const TIP_H: f32 = 22.0;

/// A scale along one side of a graph: the value `lo` at `a` px, `hi` at `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub a: f32,
    pub b: f32,
    pub lo: f64,
    pub hi: f64,
    /// Equal steps are equal ratios (a frequency, a time).
    pub log: bool,
}

impl Axis {
    pub fn lin(a: f32, b: f32, lo: f64, hi: f64) -> Axis {
        Axis {
            a,
            b,
            lo,
            hi,
            log: false,
        }
    }

    pub fn log(a: f32, b: f32, lo: f64, hi: f64) -> Axis {
        Axis {
            a,
            b,
            lo,
            hi,
            log: true,
        }
    }

    /// How far along the value is (0 at `lo`, 1 at `hi`; past them outside).
    fn along(&self, v: f64) -> f64 {
        if self.log {
            (v.max(f64::MIN_POSITIVE) / self.lo).ln() / (self.hi / self.lo).ln()
        } else {
            (v - self.lo) / (self.hi - self.lo)
        }
    }

    /// Where a value sits.
    pub fn px(&self, v: f64) -> f32 {
        self.a + (self.b - self.a) * self.along(v) as f32
    }

    /// The value at a place, held to the scale's ends.
    pub fn value(&self, at: f32) -> f64 {
        let t = f64::from((at - self.a) / (self.b - self.a)).clamp(0.0, 1.0);
        if self.log {
            self.lo * (self.hi / self.lo).powf(t)
        } else {
            self.lo + (self.hi - self.lo) * t
        }
    }

    /// The same scale moved by `d` px.
    pub fn shift(self, d: f32) -> Axis {
        Axis {
            a: self.a + d,
            b: self.b + d,
            ..self
        }
    }
}

/// A side of a point that moves a parameter (`k`, in its own units along
/// the axis).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    pub k: usize,
    pub axis: Axis,
}

impl Drive {
    /// The parameter's value (0..1) once the point has moved `d` px from
    /// where `start` (0..1) put it, on the parameter's own steps.
    pub fn moved(&self, spec: &ParamSpec, start: f64, d: f32) -> f64 {
        let at = self.axis.px(spec.to_plain(start)) + d;
        control::snap(spec, spec.to_norm(self.axis.value(at)))
    }
}

/// A row of a point's menu: what it sets (parameter and value, 0..1), and
/// whether it is what the point is now. `None` among the rows is a gap.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub set: Vec<(usize, f64)>,
    pub on: bool,
}

/// A point on a graph.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// What a screen reader calls it (`LMF Peaking`).
    pub name: String,
    /// The short name written over it (`LMF`).
    pub label: String,
    /// What it reads as (`350 Hz, −3.5 dB, Q 1.40`): the tip and the reader.
    pub value: String,
    /// Where it is on the screen.
    pub at: (f32, f32),
    pub x: Option<Drive>,
    pub y: Option<Drive>,
    /// The parameter the wheel turns over it.
    pub wheel: Option<usize>,
    /// It does something (a band switched off is dim).
    pub on: bool,
    pub menu: Vec<Option<Choice>>,
    /// What else its menu's «back to the defaults» puts back (a band's type,
    /// its on, its slope).
    pub rest: Vec<usize>,
}

impl Point {
    /// Is `(x, y)` close enough to take it?
    pub fn hit(&self, x: f32, y: f32) -> bool {
        let (dx, dy) = (x - self.at.0, y - self.at.1);
        dx * dx + dy * dy <= (POINT_HIT / 2.0) * (POINT_HIT / 2.0)
    }

    /// The parameters it moves, in their order: x, y, the wheel's.
    pub fn params(&self) -> impl Iterator<Item = usize> + '_ {
        [self.x.map(|d| d.k), self.y.map(|d| d.k), self.wheel]
            .into_iter()
            .flatten()
    }

    /// The parameters its menu puts back to their defaults: those it moves
    /// and the rest.
    pub fn resets(&self) -> Vec<usize> {
        let mut all: Vec<usize> = self.params().collect();
        for &k in &self.rest {
            if !all.contains(&k) {
                all.push(k);
            }
        }
        all
    }
}

/// The point under `(x, y)`: the closest one that takes it, the later one
/// when two are as close (it is drawn on top).
pub fn point_at(points: &[Point], x: f32, y: f32) -> Option<usize> {
    let d = |p: &Point| (x - p.at.0).powi(2) + (y - p.at.1).powi(2);
    points
        .iter()
        .enumerate()
        .filter(|(_, p)| p.hit(x, y))
        .min_by(|(i, a), (j, b)| d(a).total_cmp(&d(b)).then(j.cmp(i)))
        .map(|(i, _)| i)
}

/// The points of a device's `n`th graph, whose plane is `r` on the screen,
/// for its values (0..1). Each device with a graph says what they are.
pub fn points(def: &BuiltinDef, n: usize, r: Rect, values: &[f64]) -> Vec<Point> {
    match (def.uid, n) {
        ("eq", 0) => super::eq::points(def, r, values),
        _ => Vec::new(),
    }
}

/// What a graph is drawn with besides its values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct View<'a> {
    pub plugin: Option<PluginId>,
    /// The point chosen, and the one under the pointer.
    pub chosen: Option<usize>,
    pub hot: Option<usize>,
    /// A point is being dragged: its tip shows.
    pub held: bool,
    /// The point has the keys.
    pub focused: bool,
    /// The device is bypassed: its curve dims.
    pub bypassed: bool,
    /// What the analyser heard (dB at log-spaced frequencies): the input,
    /// then the output; empty when not shown.
    pub spectrum: [&'a [f32]; 2],
    /// The song's sample rate (0 when not known).
    pub rate: f64,
}

/// Paints a device's `n`th graph on its plane `r`, then its points.
pub fn paint(pen: &mut Pen, def: &BuiltinDef, n: usize, r: Rect, values: &[f64], view: &View) {
    let pts = match (def.uid, n) {
        ("eq", 0) => {
            super::eq::paint(pen, r, values, view);
            let rate = if view.rate > 0.0 { view.rate } else { 48_000.0 };
            super::eq::points_at(def, r, values, rate)
        }
        _ => {
            pen.round(r, pen.c.surface_2, Radius::S);
            points(def, n, r, values)
        }
    };
    paint_points(pen, r, &pts, view);
}

// ------------------------------------------------------------------ curves

/// A curve's points, in its plane's own place.
pub type Curve = Rc<Vec<(f32, f32)>>;

thread_local! {
    /// The last shape of each graph's curves, by what made it.
    static CURVES: RefCell<HashMap<(PluginId, usize), (u64, Curve)>> =
        RefCell::new(HashMap::new());
}

/// A stamp of what shapes a curve: its values and its plane's size.
pub fn stamp(values: &[f64], r: Rect) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in values {
        v.to_bits().hash(&mut h);
    }
    r.w.to_bits().hash(&mut h);
    r.h.to_bits().hash(&mut h);
    h.finish()
}

/// The curve `key` (a device's plug-in and which of its curves), in the
/// plane's own place (its top left at 0, 0): the one kept while `stamp` is
/// the same, else what `make` works out (kept for the next frame).
pub fn curve(key: (PluginId, usize), stamp: u64, make: impl FnOnce() -> Vec<(f32, f32)>) -> Curve {
    CURVES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((s, pts)) = c.get(&key)
            && *s == stamp
        {
            return pts.clone();
        }
        let pts = Rc::new(make());
        c.insert(key, (stamp, pts.clone()));
        pts
    })
}

/// Forgets the curves of plug-ins whose windows are closed.
pub fn forget(open: impl Fn(PluginId) -> bool) {
    CURVES.with(|c| c.borrow_mut().retain(|(id, _), _| open(*id)));
}

// ------------------------------------------------------------------ painting

/// A line through `pts` (in the plane `r`'s own place).
pub fn stroke(pen: &mut Pen, r: Rect, pts: &[(f32, f32)], width: f32, color: Hsla) {
    let Some(first) = pts.first() else {
        return;
    };
    let (ox, oy) = (pen.origin.0 + r.x, pen.origin.1 + r.y);
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(px(ox + first.0), px(oy + first.1)));
    for p in &pts[1..] {
        path.line_to(point(px(ox + p.0), px(oy + p.1)));
    }
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

/// The area between `pts` (in the plane `r`'s own place) and the line `base`
/// across it.
pub fn area(pen: &mut Pen, r: Rect, pts: &[(f32, f32)], base: f32, color: Hsla) {
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else {
        return;
    };
    let (ox, oy) = (pen.origin.0 + r.x, pen.origin.1 + r.y);
    let mut path = PathBuilder::fill();
    path.move_to(point(px(ox + first.0), px(oy + base)));
    for p in pts {
        path.line_to(point(px(ox + p.0), px(oy + p.1)));
    }
    path.line_to(point(px(ox + last.0), px(oy + base)));
    path.close();
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

/// The faces of a scale's stretches across the plane `r`: one every other
/// stretch between `marks` (the decades of a frequency), so the scale reads
/// without thin lines.
pub fn faces(pen: &mut Pen, r: Rect, axis: Axis, marks: &[f64], color: Hsla) {
    for (i, w) in marks.windows(2).enumerate() {
        if i % 2 == 1 {
            let (a, b) = (axis.px(w[0]).max(r.x), axis.px(w[1]).min(r.right()));
            if b > a {
                pen.fill(Rect::new(a, r.y, b - a, r.h), color);
            }
        }
    }
}

/// The names of a scale's marks along the plane's foot (`below`) or down its
/// right side.
pub fn ticks(pen: &mut Pen, r: Rect, axis: Axis, marks: &[(f64, &str)], below: bool) {
    let ink = pen.c.ink;
    for (v, name) in marks {
        let at = axis.px(*v);
        if below {
            let cell = Rect::new(at + 3.0, r.bottom() - 16.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                ink,
                cell,
                Align::Left,
            );
        } else {
            let cell = Rect::new(r.right() - 44.0, at - 7.0, 40.0, 14.0);
            pen.text(
                name,
                FontSize::Micro,
                Weight::Regular,
                ink,
                cell,
                Align::Right,
            );
        }
    }
}

/// How big a point is drawn.
pub fn point_size(hot: bool) -> f32 {
    if hot { POINT_HOT } else { POINT }
}

/// The points over a graph: the chosen and the hot one light, a switched-off
/// one dim, each with its short name over it, and the tip of the one held or
/// under the pointer.
pub fn paint_points(pen: &mut Pen, plane: Rect, points: &[Point], view: &View) {
    let c = pen.c;
    for (i, p) in points.iter().enumerate() {
        let chosen = view.chosen == Some(i);
        let hot = chosen || view.hot == Some(i);
        let d = point_size(hot);
        let dot = Rect::new(p.at.0 - d / 2.0, p.at.1 - d / 2.0, d, d);
        let face = if !p.on {
            c.scroll_thumb[0]
        } else if hot {
            c.fill
        } else {
            c.ink
        };
        pen.round(dot, face, Radius::Pill);
        if chosen && view.focused {
            let ring = Rect::new(dot.x - 3.0, dot.y - 3.0, dot.w + 6.0, dot.h + 6.0);
            pen.outline(ring, c.ring, Radius::Pill, 2.0);
        }
        let name = Rect::new(p.at.0 - 30.0, dot.y - 16.0, 60.0, 14.0);
        let ink = if chosen { c.ink_strong } else { c.ink };
        pen.text(
            &p.label,
            FontSize::Micro,
            Weight::Semibold,
            ink,
            name,
            Align::Center,
        );
    }
    let tip = view
        .hot
        .filter(|_| view.held)
        .or(view.hot)
        .and_then(|i| points.get(i));
    if let Some(p) = tip {
        paint_tip(pen, plane, p);
    }
}

/// Where the value of a point `w` wide goes: over it, or under it near the
/// plane's top; never past the plane's sides.
pub fn tip_rect(plane: Rect, at: (f32, f32), w: f32) -> Rect {
    let y = if at.1 - plane.y > 44.0 {
        at.1 - 36.0
    } else {
        at.1 + 18.0
    };
    let x = (at.0 - w / 2.0)
        .min(plane.right() - w - 4.0)
        .max(plane.x + 4.0);
    Rect::new(x, y, w, TIP_H)
}

/// The value of a point in a box over it.
fn paint_tip(pen: &mut Pen, plane: Rect, p: &Point) {
    let c = pen.c;
    let w = pen.measure(&p.value, FontSize::Caption, Weight::Regular) + TIP_PAD.0 * 2.0;
    let r = tip_rect(plane, p.at, w);
    pen.floating(r, c.surface_3, Radius::S);
    pen.text(
        &p.value,
        FontSize::Caption,
        Weight::Regular,
        c.ink_strong,
        Rect::new(
            r.x + TIP_PAD.0,
            r.y + TIP_PAD.1,
            w - TIP_PAD.0 * 2.0,
            r.h - TIP_PAD.1 * 2.0,
        ),
        Align::Center,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_axis_goes_both_ways() {
        let f = Axis::log(0.0, 600.0, 20.0, 20_000.0);
        assert!((f.px(20.0) - 0.0).abs() < 1e-3);
        assert!((f.px(20_000.0) - 600.0).abs() < 1e-3);
        assert!((f.px(632.455_5) - 300.0).abs() < 1e-2);
        for at in [0.0, 37.0, 300.0, 599.0] {
            assert!((f.px(f.value(at)) - at).abs() < 1e-2);
        }
        assert_eq!(f.value(-50.0), 20.0);
        let g = Axis::lin(14.0, 246.0, 18.0, -18.0);
        assert!((g.px(0.0) - 130.0).abs() < 1e-3);
        assert!((g.value(14.0) - 18.0).abs() < 1e-9);
        assert!((g.value(1000.0) + 18.0).abs() < 1e-9);
    }

    fn at(x: f32, y: f32) -> Point {
        Point {
            name: String::new(),
            label: String::new(),
            value: String::new(),
            at: (x, y),
            x: None,
            y: None,
            wheel: None,
            on: true,
            menu: Vec::new(),
            rest: Vec::new(),
        }
    }

    #[test]
    fn a_point_takes_a_press_within_its_reach() {
        let pts = [at(100.0, 100.0), at(110.0, 100.0)];
        assert_eq!(point_at(&pts, 100.0, 100.0), Some(0));
        assert_eq!(point_at(&pts, 106.0, 100.0), Some(1));
        // Equally close: the later, drawn on top.
        assert_eq!(point_at(&pts, 105.0, 100.0), Some(1));
        assert_eq!(point_at(&pts, 100.0, 100.0 + POINT_HIT / 2.0), Some(0));
        assert_eq!(point_at(&pts, 100.0, 100.0 + POINT_HIT / 2.0 + 0.5), None);
        assert_eq!(point_at(&pts, 80.0, 100.0), None);
    }

    #[test]
    fn a_drive_moves_its_parameter_along_the_axis() {
        let def = xirius_params::find("delay").unwrap();
        let (k, spec) = def.param("time").unwrap();
        let axis = Axis::lin(0.0, 400.0, spec.min, spec.max);
        let d = Drive { k, axis };
        let start = spec.to_norm(spec.min);
        let half = d.moved(spec, start, 200.0);
        let mid = (spec.min + spec.max) / 2.0;
        assert!((spec.to_plain(half) - mid).abs() <= (spec.max - spec.min) * 1e-3);
        assert_eq!(d.moved(spec, start, -50.0), control::snap(spec, 0.0));
        assert_eq!(d.moved(spec, start, 900.0), control::snap(spec, 1.0));
    }

    #[test]
    fn a_curve_is_worked_out_once_while_its_shape_holds() {
        let id = PluginId(7_325);
        let mut made = 0;
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        let s = stamp(&[0.5, 0.25], r);
        for _ in 0..3 {
            curve((id, 0), s, || {
                made += 1;
                vec![(0.0, 0.0)]
            });
        }
        assert_eq!(made, 1);
        curve((id, 0), stamp(&[0.5, 0.3], r), || {
            made += 1;
            Vec::new()
        });
        assert_eq!(made, 2);
        assert_ne!(s, stamp(&[0.5, 0.25], Rect::new(0.0, 0.0, 90.0, 50.0)));
        forget(|_| false);
        curve((id, 0), s, || {
            made += 1;
            Vec::new()
        });
        assert_eq!(made, 3);
    }
}
