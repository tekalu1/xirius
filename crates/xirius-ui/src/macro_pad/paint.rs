//! The macro controls' face (MIX-041): the window's knobs and pad, and the knob cell the
//! channel editor's row shares with it.

use xirius_core::controls::ControlId;
use xirius_core::model::Track;
use xirius_design::{FontSize, Radius, Weight};

use super::{AXES, CHEV, DIAL_GAP, LINE_H, Part, View, dial_rect, knob_value, percent};
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, Studio};
use crate::title_bar;

/// The dial's ring.
const RING_W: f32 = 3.0;
/// The pad's grid of dots: how many across and up, and a dot's size.
const GRID: usize = 5;
const GRID_DOT: f32 = 3.0;
/// The pad's point.
const POINT: f32 = 18.0;

/// One knob: the dial at the cell's top, its value under it, then its name.
pub fn paint_knob(pen: &mut Pen, cell: Rect, v: f64, name: &str, hot: bool) {
    let c = pen.c;
    let dial = dial_rect(cell);
    crate::mixer::paint::knob(pen, dial, v as f32, 0.0, RING_W, true);
    let ink = if hot { c.ink_strong } else { c.ink };
    let y = dial.bottom() + DIAL_GAP;
    pen.text(
        &percent(v),
        FontSize::Small,
        Weight::Semibold,
        c.ink_strong,
        Rect::new(cell.x, y, cell.w, LINE_H),
        Align::Center,
    );
    pen.text(
        name,
        FontSize::Small,
        Weight::Regular,
        ink,
        Rect::new(cell.x, y + LINE_H, cell.w, LINE_H),
        Align::Center,
    );
}

/// Paints the macro window: over every other window.
pub fn paint(pen: &mut Pen, s: &Studio, g: &Geometry) {
    let Some(w) = &s.macro_pad else {
        return;
    };
    let Some(v) = s.macro_view(g) else {
        return;
    };
    let p = s.project();
    let Some(t) = p.tracks.get(&v.track) else {
        return;
    };
    let c = pen.c;
    pen.floating(v.r, c.paper, Radius::M);
    title_bar::paint_plain_of(
        pen,
        v.bar_rect(),
        "macro",
        &v.name,
        Some(super::NAME),
        w.pinned,
    );
    for (i, cell) in v.knobs.iter().enumerate() {
        let k = i as u8;
        let hot = w.hot == Some(Part::Knob(k));
        paint_knob(
            pen,
            *cell,
            knob_value(s, t, k),
            &t.control_label(ControlId::Knob(k)),
            hot,
        );
    }
    paint_pad(pen, s, w.focused, t, &v);
    for (axis, r) in v.picks.iter().enumerate() {
        let k = t.controls.xy_knobs()[axis];
        paint_pick(
            pen,
            *r,
            AXES[axis],
            &t.control_label(k),
            w.hot == Some(Part::Pick(axis)),
        );
    }
}

/// The pad: its dark face, the grid of dots and the point where its two knobs are.
fn paint_pad(pen: &mut Pen, s: &Studio, focused: bool, t: &Track, v: &View) {
    let c = pen.c;
    let pad = v.pad;
    pen.round(pad, c.surface_0, Radius::M);
    if focused {
        pen.outline(pad, c.ring, Radius::M, 2.0);
    }
    for i in 0..GRID {
        for j in 0..GRID {
            let x = pad.x + pad.w * (i as f32 + 0.5) / GRID as f32;
            let y = pad.y + pad.h * (j as f32 + 0.5) / GRID as f32;
            pen.round(
                Rect::new(x - GRID_DOT / 2.0, y - GRID_DOT / 2.0, GRID_DOT, GRID_DOT),
                c.line[1],
                Radius::Pill,
            );
        }
    }
    let (px, py) = super::pad_point(pad, s.pad_now(t));
    let (face, _) = c.primary;
    pen.round(
        Rect::new(px - POINT / 2.0, py - POINT / 2.0, POINT, POINT),
        face,
        Radius::Pill,
    );
}

/// A pick under the pad: the axis's letter, the knob's name and the chevron.
fn paint_pick(pen: &mut Pen, r: Rect, axis: &str, name: &str, hot: bool) {
    let c = pen.c;
    pen.round(r, if hot { c.surface_3 } else { c.surface_2 }, Radius::S);
    let mut x = r.x + super::PICK_L;
    x += pen.text(
        axis,
        FontSize::Small,
        Weight::Semibold,
        c.ink_strong,
        Rect::new(x, r.y, r.w, r.h),
        Align::Left,
    );
    x += super::PICK_IN;
    x += pen.text(
        name,
        FontSize::Small,
        Weight::Regular,
        c.ink_strong,
        Rect::new(x, r.y, (r.right() - x).max(0.0), r.h),
        Align::Left,
    );
    x += super::PICK_IN;
    pen.icon(
        "chevron-down",
        Rect::new(x, r.y + (r.h - CHEV) / 2.0, CHEV, CHEV),
        c.ink,
    );
}
