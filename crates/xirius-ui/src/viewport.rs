//! Scroll / zoom state for the timeline and the piano roll.
//!
//! Plain `f64` maths with no GPUI types, so it can be tested.

/// One axis of a viewport: where the left edge is, and how many pixels a unit
/// (second or beat) takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    /// Unit at the left edge of the view.
    pub origin: f64,
    /// Pixels per unit.
    pub scale: f64,
    pub min_scale: f64,
    pub max_scale: f64,
}

impl Axis {
    pub fn new(origin: f64, scale: f64, min_scale: f64, max_scale: f64) -> Axis {
        Axis {
            origin,
            scale: scale.clamp(min_scale, max_scale),
            min_scale,
            max_scale,
        }
    }

    pub fn to_px(&self, unit: f64) -> f64 {
        (unit - self.origin) * self.scale
    }

    pub fn to_unit(&self, px: f64) -> f64 {
        self.origin + px / self.scale
    }

    /// Multiplies the scale by `factor`, keeping the unit under `anchor_px`
    /// where it is on screen.
    pub fn zoom_at(&mut self, anchor_px: f64, factor: f64) {
        let anchor_unit = self.to_unit(anchor_px);
        self.scale = (self.scale * factor).clamp(self.min_scale, self.max_scale);
        self.origin = anchor_unit - anchor_px / self.scale;
    }

    pub fn scroll_px(&mut self, dx_px: f64) {
        self.origin += dx_px / self.scale;
    }

    /// Keeps the view inside `[0, total]` (or at 0 when the content is
    /// shorter than the view).
    pub fn clamp(&mut self, view_px: f64, total: f64) {
        let visible = view_px / self.scale;
        let max_origin = (total - visible).max(0.0);
        self.origin = self.origin.clamp(0.0, max_origin);
    }
}

/// Vertical scroll in pixels over rows of equal height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rows {
    pub scroll: f64,
    pub row_h: f64,
    pub count: usize,
}

impl Rows {
    pub fn content_h(&self) -> f64 {
        self.row_h * self.count as f64
    }

    pub fn scroll_by(&mut self, dy: f64, view_h: f64) {
        self.scroll += dy;
        self.clamp(view_h);
    }

    pub fn clamp(&mut self, view_h: f64) {
        let max = (self.content_h() - view_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    /// Rows that intersect the view, as `first..end` indices.
    pub fn visible(&self, view_h: f64) -> std::ops::Range<usize> {
        let first = (self.scroll / self.row_h).floor().max(0.0) as usize;
        let end = (((self.scroll + view_h) / self.row_h).ceil() as usize).min(self.count);
        first.min(end)..end
    }

    /// Top of row `i` relative to the view.
    pub fn row_top(&self, i: usize) -> f64 {
        i as f64 * self.row_h - self.scroll
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_anchor_still() {
        let mut a = Axis::new(10.0, 50.0, 1.0, 400.0);
        let anchor = 300.0;
        let unit = a.to_unit(anchor);
        for factor in [1.5, 0.7, 2.0, 0.1] {
            a.zoom_at(anchor, factor);
            assert!((a.to_unit(anchor) - unit).abs() < 1e-9, "factor {factor}");
        }
    }

    #[test]
    fn zoom_is_clamped() {
        let mut a = Axis::new(0.0, 50.0, 2.0, 100.0);
        a.zoom_at(0.0, 100.0);
        assert_eq!(a.scale, 100.0);
        a.zoom_at(0.0, 0.0001);
        assert_eq!(a.scale, 2.0);
    }

    #[test]
    fn clamp_keeps_the_view_in_the_content() {
        let mut a = Axis::new(-5.0, 10.0, 1.0, 100.0);
        a.clamp(500.0, 240.0);
        assert_eq!(a.origin, 0.0);
        a.origin = 1000.0;
        a.clamp(500.0, 240.0);
        assert_eq!(a.origin, 240.0 - 50.0);
        // Content shorter than the view.
        a.origin = 3.0;
        a.clamp(500.0, 20.0);
        assert_eq!(a.origin, 0.0);
    }

    #[test]
    fn rows_visible_range() {
        let mut r = Rows {
            scroll: 0.0,
            row_h: 40.0,
            count: 200,
        };
        assert_eq!(r.visible(100.0), 0..3);
        r.scroll_by(50.0, 100.0);
        assert_eq!(r.visible(100.0), 1..4);
        r.scroll_by(1e9, 100.0);
        assert_eq!(r.scroll, 200.0 * 40.0 - 100.0);
        assert_eq!(r.visible(100.0).end, 200);
        r.scroll_by(-1e9, 100.0);
        assert_eq!(r.scroll, 0.0);
        assert_eq!(r.row_top(2), 80.0);
    }

    #[test]
    fn rows_shorter_than_the_view() {
        let mut r = Rows {
            scroll: 10.0,
            row_h: 40.0,
            count: 2,
        };
        r.clamp(500.0);
        assert_eq!(r.scroll, 0.0);
        assert_eq!(r.visible(500.0), 0..2);
    }
}
