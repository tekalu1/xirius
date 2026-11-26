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

/// Properties over any axis and any rows (ARC-054): proptest makes the values, and a failure
/// comes back as the smallest case that fails.
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed};

    /// The same cases on every run and every machine, and no regression file left behind.
    fn config() -> Config {
        Config {
            cases: 512,
            failure_persistence: None,
            rng_seed: RngSeed::Fixed(54),
            ..Config::default()
        }
    }

    /// An axis whose scale is somewhere between its bounds.
    fn axis() -> impl Strategy<Value = Axis> {
        (-1e4..1e4f64, 0.01..1e3f64, 1.0..1e3f64, 0.0..1.0f64).prop_map(
            |(origin, min, span, at)| {
                let max = min * span;
                Axis::new(origin, min + (max - min) * at, min, max)
            },
        )
    }

    /// A pixel length in quarter pixels, which the sums and products below keep exact.
    fn px(lo: u32, hi: u32) -> impl Strategy<Value = f64> {
        (lo * 4..=hi * 4).prop_map(|q| f64::from(q) / 4.0)
    }

    proptest! {
        #![proptest_config(config())]

        /// The unit under the pointer stays under it, and the scale stays inside its bounds:
        /// exactly the product where that is inside them, the bound it passed where it is not.
        #[test]
        fn a_zoom_keeps_the_unit_under_the_anchor_and_the_scale_in_bounds(
            mut a in axis(),
            anchor in 0.0..4000.0f64,
            factors in prop::collection::vec(1e-3..1e3f64, 1..8),
        ) {
            for factor in factors {
                let unit = a.to_unit(anchor);
                let wanted = a.scale * factor;
                a.zoom_at(anchor, factor);
                let off = (a.to_unit(anchor) - unit).abs();
                let room = 1e-9 * (1.0 + unit.abs() + anchor / a.scale);
                prop_assert!(off <= room, "x{factor}: the anchor moved by {off}");
                prop_assert_eq!(a.scale, wanted.clamp(a.min_scale, a.max_scale), "x{}", factor);
            }
        }

        /// After a clamp the view starts at 0 or later, ends at the content's end or before
        /// (or starts at 0 where the content is shorter than the view), and a view that was
        /// already inside does not move.
        #[test]
        fn a_clamp_keeps_the_view_inside_the_content(
            mut a in axis(),
            view in px(1, 4000),
            total in 0.0..1e5f64,
        ) {
            let was = a.origin;
            a.clamp(view, total);
            let visible = view / a.scale;
            prop_assert!(a.origin >= 0.0, "starts at {}", a.origin);
            if total > visible {
                let end = a.to_unit(view);
                prop_assert!(end <= total + 1e-9 * total, "ends at {end} past {total}");
                if (0.0..=total - visible).contains(&was) {
                    prop_assert_eq!(a.origin, was, "a view inside the content moved");
                }
            } else {
                prop_assert_eq!(a.origin, 0.0);
            }
        }

        /// Scrolled anywhere, the rows stay inside the content (at 0 when they are shorter
        /// than the view), and `visible` gives exactly the rows that show some of themselves.
        #[test]
        fn scrolled_rows_stay_in_the_content_and_visible_gives_the_rows_that_show(
            row_h in px(1, 200),
            count in 0usize..400,
            view in px(1, 2000),
            moves in prop::collection::vec(px(0, 20_000), 1..6),
            back in prop::collection::vec(any::<bool>(), 6),
        ) {
            let mut r = Rows { scroll: 0.0, row_h, count };
            for (dy, back) in moves.into_iter().zip(back) {
                r.scroll_by(if back { -dy } else { dy }, view);
                let max = (r.content_h() - view).max(0.0);
                prop_assert!((0.0..=max).contains(&r.scroll), "scrolled to {} of {max}", r.scroll);
                let shows: Vec<usize> = (0..count)
                    .filter(|&i| r.row_top(i) < view && r.row_top(i) + row_h > 0.0)
                    .collect();
                let range: Vec<usize> = r.visible(view).collect();
                prop_assert_eq!(range, shows, "scrolled to {}", r.scroll);
            }
        }
    }
}
