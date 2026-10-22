//! The panel's meters and MIDI lights: the only parts in the meter's colours.

use xirius_design::Radius;

use crate::draw::Pen;
use crate::paint::Colors;
use crate::studio::Rect;

/// How wide a meter's bar is for a level: 60 dB across, as the mock's meters.
pub fn level_width(a: f32) -> f32 {
    if a <= 0.0005 {
        0.0
    } else {
        (1.0 + a.max(1e-3).log10() / 3.0).clamp(0.02, 1.0)
    }
}

fn level_color(c: &Colors, a: f32) -> gpui::Hsla {
    if a >= 0.98 {
        c.over
    } else if a >= 0.5 {
        c.hot
    } else {
        c.ok
    }
}

pub(super) fn meter(pen: &mut Pen, r: Rect, a: f32, radius: Radius) {
    pen.round(r, pen.c.well, radius);
    let w = r.w * level_width(a);
    if w > 0.0 {
        let color = level_color(pen.c, a);
        pen.round(Rect::new(r.x, r.y, w, r.h), color, radius);
    }
}

/// A MIDI input's light: lit while it hears notes.
pub(super) fn midi(pen: &mut Pen, dot: Rect, lit: bool) {
    let c = pen.c;
    pen.round(dot, if lit { c.ok } else { c.surface_3 }, Radius::Pill);
}
