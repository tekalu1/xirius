//! Automation indicators remain visible when a lane is hidden.
use crate::draw::Pen;
use crate::studio::Rect;
use xirius_core::auto::AutoTarget;
use xirius_core::model::Track;
use xirius_design::Radius;

pub fn present(track: &Track, target: &AutoTarget) -> bool {
    track
        .automation
        .iter()
        .any(|l| &l.target == target && !l.points.is_empty())
}
pub fn paint(pen: &mut Pen, track: &Track, target: AutoTarget, r: Rect) {
    if present(track, &target) {
        pen.round(
            Rect::new(r.right() - 5.0, r.y, 5.0, 5.0),
            pen.c.ink_blue,
            Radius::Pill,
        );
    }
}
