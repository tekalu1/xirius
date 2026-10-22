use crate::{auto::AutoLane, time::Tick};
use std::{collections::BTreeSet, sync::Arc};

impl AutoLane {
    /// Move from the original positions, pushing neighbours without losing their values.
    /// Recomputing a drag from its original lane restores points as it moves back.
    pub fn push_points(&mut self, ticks: &BTreeSet<Tick>, by: Tick, value_by: f64) -> Tick {
        let Some(first) = self.points.iter().position(|p| ticks.contains(&p.tick)) else {
            return 0;
        };
        let last = self
            .points
            .iter()
            .rposition(|p| ticks.contains(&p.tick))
            .unwrap();
        let by = by.clamp(
            first as Tick - self.points[first].tick,
            Tick::MAX - (self.points.len() - last - 1) as Tick - self.points[last].tick,
        );
        let range = self.target.range();
        let pts = Arc::make_mut(&mut self.points);
        for p in pts.iter_mut() {
            if ticks.contains(&p.tick) {
                p.tick += by;
                p.value = (p.value + value_by).clamp(range.0, range.1);
            }
        }
        if by > 0 {
            for i in 1..pts.len() {
                pts[i].tick = pts[i].tick.max(pts[i - 1].tick + 1);
            }
        } else if by < 0 {
            for i in (0..pts.len() - 1).rev() {
                pts[i].tick = pts[i].tick.min(pts[i + 1].tick - 1);
            }
        }
        by
    }

    pub fn push_point(&mut self, index: usize, at: Tick, value: f64) -> Option<Tick> {
        let original = *self.points.get(index)?;
        let by = self.push_points(
            &BTreeSet::from([original.tick]),
            at.max(0).saturating_sub(original.tick),
            value - original.value,
        );
        Some(original.tick + by)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::{AutoPoint, AutoTarget};
    fn lane() -> AutoLane {
        let mut l = AutoLane::new(AutoTarget::Pan);
        l.points = vec![
            AutoPoint::new(10, -0.5),
            AutoPoint::new(20, 0.0),
            AutoPoint::new(30, 0.5),
        ]
        .into();
        l
    }
    #[test]
    fn pushes_in_both_directions_and_restores_from_the_original() {
        let original = lane();
        let mut l = original.clone();
        assert_eq!(l.push_point(1, 45, 0.25), Some(45));
        assert_eq!(
            l.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
            [10, 45, 46]
        );
        assert_eq!(l.points[2].value, 0.5);
        let mut back = original.clone();
        back.push_point(1, 20, 0.0);
        assert_eq!(back, original);
        l = original;
        assert_eq!(l.push_point(1, 0, -0.25), Some(1));
        assert_eq!(
            l.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
            [0, 1, 30]
        );
    }
    #[test]
    fn selected_points_move_relative_values_and_keep_unselected_points() {
        let mut l = lane();
        assert_eq!(l.push_points(&BTreeSet::from([10, 20]), 25, 0.25), 25);
        assert_eq!(
            l.points.iter().map(|p| p.tick).collect::<Vec<_>>(),
            [35, 45, 46]
        );
        assert_eq!(
            l.points.iter().map(|p| p.value).collect::<Vec<_>>(),
            [-0.25, 0.25, 0.5]
        );
        l.push_point(2, Tick::MAX, 1.0);
        assert_eq!(l.points[2].tick, Tick::MAX);
    }
}
