//! Turns "be at this scroll / zoom" into the wheel deltas that get there.
//!
//! The handlers in `app.rs` and the plans here use the same conventions, so
//! the bench can send real wheel events and still land on the scripted view.
//!
//! * Zoom: Ctrl + wheel, `scale *= exp(delta_y * ZOOM_K)`, anchored at the
//!   pointer.
//! * Scroll: `origin -= delta_x / scale` (content follows the wheel), rows
//!   `scroll -= delta_y`.

use crate::viewport::{Axis, Rows};

/// Natural-log zoom per wheel pixel.
pub const ZOOM_K: f64 = 0.01;

/// Position of the current scale on the log range between `min` and `max`.
pub fn zoom_pos(axis: &Axis) -> f64 {
    (axis.scale / axis.min_scale).ln() / (axis.max_scale / axis.min_scale).ln()
}

pub fn scale_at(axis: &Axis, pos: f64) -> f64 {
    axis.min_scale * (axis.max_scale / axis.min_scale).powf(pos.clamp(0.0, 1.0))
}

/// Applies a Ctrl+wheel to the axis.
pub fn apply_zoom(axis: &mut Axis, anchor_px: f64, wheel_dy: f64, view_px: f64, total: f64) {
    axis.zoom_at(anchor_px, (wheel_dy * ZOOM_K).exp());
    axis.clamp(view_px, total);
}

/// Applies a plain wheel to the axis.
pub fn apply_scroll(axis: &mut Axis, wheel_dx: f64, view_px: f64, total: f64) {
    axis.scroll_px(-wheel_dx);
    axis.clamp(view_px, total);
}

/// The wheel deltas for one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisPlan {
    pub zoom_dy: Option<f64>,
    pub scroll_dx: Option<f64>,
}

/// `zoom` and `scroll` are `Some(target)` for the parts that should move.
/// `scroll` is the fraction (0..=1) of the scrollable range.
pub fn plan_axis(
    axis: &Axis,
    view_px: f64,
    total: f64,
    anchor_px: f64,
    zoom: Option<f64>,
    scroll: Option<f64>,
) -> AxisPlan {
    let mut predicted = *axis;
    let mut plan = AxisPlan::default();
    if let Some(pos) = zoom {
        let factor = scale_at(axis, pos) / axis.scale;
        if (factor - 1.0).abs() > 1e-9 {
            let dy = factor.ln() / ZOOM_K;
            plan.zoom_dy = Some(dy);
            apply_zoom(&mut predicted, anchor_px, dy, view_px, total);
        }
    }
    if let Some(frac) = scroll {
        let max_origin = (total - view_px / predicted.scale).max(0.0);
        let want = frac.clamp(0.0, 1.0) * max_origin;
        let dx = -(want - predicted.origin) * predicted.scale;
        if dx.abs() > 1e-9 {
            plan.scroll_dx = Some(dx);
        }
    }
    plan
}

/// The wheel `delta_y` that puts the rows at `frac` of their scroll range.
pub fn plan_rows(rows: &Rows, view_h: f64, frac: f64) -> Option<f64> {
    let max = (rows.content_h() - view_h).max(0.0);
    let want = frac.clamp(0.0, 1.0) * max;
    let dy = -(want - rows.scroll);
    (dy.abs() > 1e-9).then_some(dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis() -> Axis {
        Axis::new(0.0, 20.0, 3.0, 375.0)
    }

    #[test]
    fn zoom_position_round_trips() {
        let a = axis();
        for pos in [0.0, 0.25, 0.5, 1.0] {
            let b = Axis::new(0.0, scale_at(&a, pos), a.min_scale, a.max_scale);
            assert!((zoom_pos(&b) - pos).abs() < 1e-9);
        }
    }

    #[test]
    fn a_plan_lands_on_the_target_view() {
        let (view, total, anchor) = (1400.0, 240.0, 700.0);
        for (zoom, scroll) in [(0.8, 0.5), (0.1, 1.0), (0.55, 0.0), (1.0, 0.33)] {
            let mut a = axis();
            a.origin = 17.0;
            let plan = plan_axis(&a, view, total, anchor, Some(zoom), Some(scroll));
            if let Some(dy) = plan.zoom_dy {
                apply_zoom(&mut a, anchor, dy, view, total);
            }
            if let Some(dx) = plan.scroll_dx {
                apply_scroll(&mut a, dx, view, total);
            }
            assert!((zoom_pos(&a) - zoom).abs() < 1e-6, "zoom {zoom}");
            let max_origin = (total - view / a.scale).max(0.0);
            assert!(
                (a.origin - scroll * max_origin).abs() < 1e-6,
                "scroll {scroll}: {} vs {}",
                a.origin,
                scroll * max_origin
            );
        }
    }

    #[test]
    fn nothing_to_do_gives_an_empty_plan() {
        let a = axis();
        let pos = zoom_pos(&a);
        let plan = plan_axis(&a, 1400.0, 240.0, 0.0, Some(pos), Some(0.0));
        assert_eq!(plan, AxisPlan::default());
        let plan = plan_axis(&a, 1400.0, 240.0, 0.0, None, None);
        assert_eq!(plan, AxisPlan::default());
    }

    #[test]
    fn rows_plan_lands_on_the_target() {
        let mut rows = Rows {
            scroll: 123.0,
            row_h: 46.0,
            count: 200,
        };
        let view = 300.0;
        for frac in [0.0, 0.5, 1.0] {
            let dy = plan_rows(&rows, view, frac);
            if let Some(dy) = dy {
                rows.scroll_by(-dy, view);
            }
            let max = rows.content_h() - view;
            assert!((rows.scroll - frac * max).abs() < 1e-6);
        }
        assert_eq!(plan_rows(&rows, view, 1.0), None);
    }
}
