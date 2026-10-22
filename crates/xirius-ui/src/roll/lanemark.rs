//! What shows which controller lane is the one at work (EDT-013): the tab of the
//! active lane, its head, the plot of a lane with no points, and the sustain pedal as an on /
//! off band.
//!
//! Pressing a tab only chooses the lane, so without these marks a lane the song has no values
//! for (the sample has no sustain) looked the same before and after the press.

use gpui::Hsla;
use xirius_core::model::{CtrlKind, CtrlPoint};
use xirius_core::time::Tick;
use xirius_design::{FontSize, Radius, Weight};

use super::frame::{LaneBox, View};
use super::lanes::{LaneKind, y_of};
use super::{Ctx, Roll};
use crate::draw::{Align, Pen};
use crate::studio::Rect;

/// The edge of the active lane's head, and the underline of its tab.
const EDGE: f32 = 2.0;

/// The sustain pedal is down from a value of this on.
pub const PEDAL_DOWN: i32 = 64;

/// The stretches (clip ticks) where the pedal is down: from a point at or above
/// [`PEDAL_DOWN`] to the next point below it, or to the clip's end.
pub fn pedal_spans(points: &[CtrlPoint], clip_len: Tick) -> Vec<(Tick, Tick)> {
    let mut out = Vec::new();
    let mut from: Option<Tick> = None;
    for p in points {
        match (from, p.value >= PEDAL_DOWN) {
            (None, true) => from = Some(p.tick),
            (Some(a), false) => {
                out.push((a, p.tick));
                from = None;
            }
            _ => {}
        }
    }
    if let Some(a) = from {
        out.push((a, clip_len.max(a)));
    }
    out
}

/// A lane drawn as steps (on / off) rather than as a slanted line.
pub fn is_step(kind: LaneKind) -> bool {
    kind == LaneKind::Ctrl(CtrlKind::Cc(64))
}

/// The tab of the lane at work: a face and an underline, so it stands out from the others
/// even when the lane is empty.
pub fn tab(pen: &mut Pen, r: Rect, active: bool) -> Hsla {
    if active {
        pen.round(
            Rect::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0),
            pen.c.surface_3,
            Radius::S,
        );
        pen.fill(
            Rect::new(r.x + 6.0, r.bottom() - EDGE - 1.0, r.w - 12.0, EDGE),
            pen.c.ring,
        );
        pen.c.ink_strong
    } else {
        pen.c.ink
    }
}

/// The head of a lane: the active one is raised and has an edge in the ring colour.
pub fn head(pen: &mut Pen, lb: &LaneBox, active: bool) {
    pen.fill(
        lb.head,
        if active {
            pen.c.surface_3
        } else {
            pen.c.surface_1
        },
    );
    if active {
        pen.fill(Rect::new(lb.head.x, lb.head.y, EDGE, lb.head.h), pen.c.ring);
    }
}

/// Under the lane's points: the active lane's plot is lifted a little, the value at rest is a
/// line (fainter while the lane has no points), and the pedal's down stretches are a band.
#[allow(clippy::too_many_arguments)]
pub fn under(
    pen: &mut Pen,
    roll: &Roll,
    ctx: &Ctx,
    v: &View,
    lb: &LaneBox,
    points: &[CtrlPoint],
    color: Hsla,
) {
    let kind = roll.lanes[lb.index].kind;
    let LaneKind::Ctrl(k) = kind else {
        return;
    };
    let active = roll.active_lane == lb.index;
    if active {
        pen.fill(lb.plot, pen.c.surface_1);
        pen.fill(Rect::new(lb.plot.x, lb.plot.y, lb.plot.w, 1.0), pen.c.ring);
    }
    let rest = y_of(kind, &lb.plot, k.rest());
    if is_step(kind) {
        for (a, b) in pedal_spans(points, ctx.clip_len()) {
            let (x0, x1) = (v.x(a).max(lb.plot.x), v.x(b).min(lb.plot.right()));
            if x1 > x0 {
                let top = y_of(kind, &lb.plot, 127);
                pen.fill(Rect::new(x0, top, x1 - x0, rest - top), color.opacity(0.28));
            }
        }
    }
    // Where the value rests: a dotted line across the lane.
    let line = if points.is_empty() {
        pen.c.ink.opacity(0.5)
    } else {
        pen.c.surface_3
    };
    let mut x = lb.plot.x;
    while x < lb.plot.right() {
        pen.fill(Rect::new(x, rest, 3.0, 1.0), line);
        x += 6.0;
    }
    if is_step(kind) {
        pen.text(
            "ON",
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            Rect::new(lb.plot.x + 4.0, lb.plot.y + 1.0, 28.0, 12.0),
            Align::Left,
        );
        pen.text(
            "OFF",
            FontSize::Caption,
            Weight::Medium,
            pen.c.ink,
            Rect::new(lb.plot.x + 4.0, lb.plot.bottom() - 13.0, 28.0, 12.0),
            Align::Left,
        );
    }
}

/// The step line of a lane drawn as steps: flat to the next point, then straight up or down.
pub fn step(pen: &mut Pen, plot: &Rect, a: (f32, f32), next: (f32, f32), color: Hsla) {
    let left = a.0.max(plot.x);
    let right = next.0.min(plot.right());
    if right > left {
        pen.fill(Rect::new(left, a.1 - 0.5, right - left, 1.5), color);
    }
    if next.0 >= plot.x && next.0 <= plot.right() {
        let (top, bottom) = (a.1.min(next.1), a.1.max(next.1));
        pen.fill(
            Rect::new(next.0 - 0.5, top, 1.5, (bottom - top).max(1.0)),
            color,
        );
    }
}

/// The tab pressed unfolds a folded lane, so the choice can be seen.
pub fn show(roll: &mut Roll, index: usize) {
    if let Some(l) = roll.lanes.get_mut(index) {
        l.folded = false;
    }
}
