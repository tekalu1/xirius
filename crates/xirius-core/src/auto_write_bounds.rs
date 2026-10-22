//! Writes protect the values and curves adjacent to the visited interval.
use crate::{
    auto::{AutoLane, AutoPoint},
    time::Tick,
};
use std::sync::Arc;

impl AutoLane {
    /// Replace a visited interval without moving the line outside it.
    pub fn overwrite(
        &mut self,
        from: Tick,
        to: Tick,
        points: impl IntoIterator<Item = AutoPoint>,
        fallback: Option<f64>,
    ) {
        let original = self.clone();
        let mut guards = Vec::new();
        if let Some(tick) = from.checked_sub(1).filter(|t| *t >= 0)
            && let Some(value) = original.value_at(tick as f64).or(fallback)
        {
            guards.push(AutoPoint::new(tick, value));
        }
        if let Some(tick) = to.checked_add(1)
            && let Some(value) = original.value_at(tick as f64).or(fallback)
        {
            let prior = original.points.iter().rev().find(|p| p.tick <= tick);
            let mut guard = AutoPoint::new(tick, value);
            guard.step = prior.is_some_and(|p| p.step);
            if let Some(point) = prior.filter(|p| p.tick == tick) {
                guard.curve = point.curve;
            }
            guards.push(guard);
            if let Some(prior) = prior.filter(|p| p.curve != 0.0 && !p.step)
                && let Some(next) = original.points.iter().find(|p| p.tick > tick)
                && prior.tick < tick
                && !original.target.stepped()
            {
                sample_remainder(&original, tick, next.tick, &mut guards);
            }
        }
        self.splice(from, to, points);
        let current = Arc::make_mut(&mut self.points);
        for guard in guards {
            if let Some(point) = current.iter_mut().find(|p| p.tick == guard.tick) {
                *point = guard;
            } else {
                current.push(guard);
            }
        }
        current.sort_by_key(|p| p.tick);
    }
}

fn sample_remainder(original: &AutoLane, from: Tick, to: Tick, points: &mut Vec<AutoPoint>) {
    if to - from <= 1 {
        return;
    }
    let mid = from + (to - from) / 2;
    let n = |tick| {
        original
            .target
            .to_norm(original.value_at(tick as f64).unwrap())
    };
    let actual = n(mid);
    let straight = n(from) + (n(to) - n(from)) * (mid - from) as f64 / (to - from) as f64;
    if (actual - straight).abs() > 1e-6 {
        sample_remainder(original, from, mid, points);
        points.push(AutoPoint::new(mid, original.value_at(mid as f64).unwrap()));
        sample_remainder(original, mid, to, points);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto::AutoTarget;
    #[test]
    fn a_write_keeps_both_sides_of_a_linear_or_curved_line() {
        for curve in [0.0, 0.8, -0.8] {
            let mut lane = AutoLane::new(AutoTarget::Volume);
            lane.points = Arc::new(vec![
                AutoPoint {
                    curve,
                    ..AutoPoint::new(0, -40.0)
                },
                AutoPoint::new(400, 0.0),
            ]);
            let before = lane.clone();
            lane.overwrite(
                100,
                200,
                [AutoPoint::new(100, -12.0), AutoPoint::new(200, -12.0)],
                None,
            );
            for tick in (0..100).chain(201..=400) {
                let expected = lane.target.to_norm(before.value_at(tick as f64).unwrap());
                let heard = lane.target.to_norm(lane.value_at(tick as f64).unwrap());
                assert!(
                    (heard - expected).abs() < 2e-6,
                    "{curve} {tick}: {heard} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn a_guard_on_an_existing_point_keeps_its_outgoing_curve_and_step() {
        for step in [false, true] {
            let mut lane = AutoLane::new(AutoTarget::Pan);
            lane.points = Arc::new(vec![
                AutoPoint::new(0, -1.0),
                AutoPoint {
                    curve: 0.8,
                    step,
                    ..AutoPoint::new(100, 0.2)
                },
                AutoPoint::new(200, 1.0),
            ]);
            let before = lane.clone();
            lane.overwrite(
                10,
                99,
                [AutoPoint::new(10, 0.0), AutoPoint::new(99, 0.0)],
                None,
            );
            for tick in 100..=200 {
                assert_eq!(lane.value_at(tick as f64), before.value_at(tick as f64));
            }
        }
    }
}
