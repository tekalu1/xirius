//! Opaque face transitions and panel geometry. Tests drive the clock directly.
use crate::studio::{Geometry, Rect};
use std::{cell::RefCell, collections::HashMap, time::Instant};
use xirius_design::{Duration, Motion};

#[derive(Clone, Debug, PartialEq)]
struct Step {
    from: f32,
    to: f32,
    at: Instant,
    ms: u32,
}
impl Step {
    fn sample(&self, now: Instant) -> f32 {
        let t = if self.ms == 0 {
            1.0
        } else {
            (now.duration_since(self.at).as_secs_f32() * 1000.0 / self.ms as f32).min(1.0)
        };
        let t = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * t
    }
}
#[derive(Debug, Default, PartialEq)]
pub struct ViewMotion {
    pub enabled: std::cell::Cell<bool>,
    values: RefCell<HashMap<String, Step>>,
}
impl ViewMotion {
    pub fn value(
        &self,
        key: &str,
        target: f32,
        duration: Duration,
        motion: Motion,
        now: Instant,
    ) -> f32 {
        let ms = if self.enabled.get() {
            motion.duration_ms(duration)
        } else {
            0
        };
        let mut values = self.values.borrow_mut();
        let step = values.entry(key.into()).or_insert(Step {
            from: target,
            to: target,
            at: now,
            ms,
        });
        if step.to != target {
            *step = Step {
                from: step.sample(now),
                to: target,
                at: now,
                ms,
            };
        }
        if ms == 0 {
            step.from = target;
            step.ms = 0;
        }
        step.sample(now)
    }
    pub fn moving(&self) -> bool {
        let now = Instant::now();
        self.enabled.get()
            && self.values.borrow().values().any(|s| {
                s.ms > 0
                    && now.duration_since(s.at).as_millis() < u128::from(s.ms)
                    && s.from != s.to
            })
    }
    pub fn geometry(&self, mut g: Geometry, motion: Motion) -> Geometry {
        let now = Instant::now();
        for (key, r) in [
            ("rec", &mut g.rec),
            ("rail", &mut g.rail),
            ("panel", &mut g.panel),
            ("head", &mut g.head),
            ("ruler", &mut g.ruler),
            ("sections", &mut g.section_band),
            ("tempo", &mut g.tempo),
            ("column", &mut g.column),
            ("lanes", &mut g.lanes),
            ("drawer", &mut g.drawer),
            ("drawerbar", &mut g.drawer_bar),
            ("drawerbody", &mut g.drawer_body),
        ] {
            *r = Rect::new(
                self.value(&format!("{key}.x"), r.x, Duration::Base, motion, now),
                self.value(&format!("{key}.y"), r.y, Duration::Base, motion, now),
                self.value(&format!("{key}.w"), r.w, Duration::Base, motion, now),
                self.value(&format!("{key}.h"), r.h, Duration::Base, motion, now),
            );
        }
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buttons_take_120_ms_panels_240_ms_and_reduced_motion_zero() {
        for (duration, ms) in [(Duration::Fast, 120), (Duration::Base, 240)] {
            let v = ViewMotion::default();
            v.enabled.set(true);
            let start = Instant::now();
            let motion = Motion::default();
            v.value("on", 0.0, duration, motion, start);
            assert_eq!(v.value("on", 1.0, duration, motion, start), 0.0);
            let mid = v.value(
                "on",
                1.0,
                duration,
                motion,
                start + std::time::Duration::from_millis(ms / 2),
            );
            assert!(mid > 0.0 && mid < 1.0);
            assert_eq!(
                v.value(
                    "on",
                    1.0,
                    duration,
                    motion,
                    start + std::time::Duration::from_millis(ms)
                ),
                1.0
            );
            assert_eq!(
                v.value(
                    "on",
                    0.0,
                    duration,
                    Motion::new(true),
                    start + std::time::Duration::from_millis(ms)
                ),
                0.0
            );
        }
    }
}
