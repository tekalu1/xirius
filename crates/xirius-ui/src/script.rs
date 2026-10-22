//! The automatic scroll / zoom / drag script for the spikes.
//!
//! A scenario is a function of time (seconds since the scenario started) that
//! gives target positions in 0..=1. The app turns the difference between the
//! target and the current position into real wheel and mouse events, so the
//! measured path is the same one a person would use.

use std::f64::consts::TAU;

/// Which done-when the scenario is evidence for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spike {
    /// UI-023: 200 tracks and 10,000 notes at 60 fps.
    One,
    /// UI-024: 64 meters running while scrolling and dragging.
    Two,
    /// Not a pass / fail item.
    Reference,
}

#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub spike: Spike,
    pub secs: f64,
    pub meters: bool,
    pub tl_scroll: bool,
    pub tl_zoom: bool,
    pub pr_scroll: bool,
    pub pr_zoom: bool,
    /// Scrub the playhead with the mouse on the ruler.
    pub drag: bool,
    /// Track row height in logical pixels.
    pub track_h: f32,
}

const BASE: Scenario = Scenario {
    name: "",
    spike: Spike::One,
    secs: 8.0,
    meters: false,
    tl_scroll: false,
    tl_zoom: false,
    pr_scroll: false,
    pr_zoom: false,
    drag: false,
    track_h: 44.0,
};

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "still",
        spike: Spike::Reference,
        ..BASE
    },
    Scenario {
        name: "timeline-scroll",
        tl_scroll: true,
        ..BASE
    },
    Scenario {
        name: "timeline-zoom",
        tl_zoom: true,
        ..BASE
    },
    Scenario {
        name: "pianoroll-scroll-zoom",
        pr_scroll: true,
        pr_zoom: true,
        ..BASE
    },
    Scenario {
        name: "spike1-all-moving",
        tl_scroll: true,
        tl_zoom: true,
        pr_scroll: true,
        pr_zoom: true,
        ..BASE
    },
    Scenario {
        name: "meters-only",
        spike: Spike::Two,
        meters: true,
        ..BASE
    },
    Scenario {
        name: "spike2-meters-scroll-zoom-drag",
        spike: Spike::Two,
        meters: true,
        tl_scroll: true,
        tl_zoom: true,
        pr_scroll: true,
        pr_zoom: true,
        drag: true,
        ..BASE
    },
    Scenario {
        name: "stress-small-tracks-meters",
        spike: Spike::Reference,
        meters: true,
        tl_scroll: true,
        tl_zoom: true,
        pr_scroll: true,
        pr_zoom: true,
        drag: true,
        track_h: 16.0,
        ..BASE
    },
];

pub fn find(name: &str) -> Option<&'static Scenario> {
    SCENARIOS.iter().find(|s| s.name == name)
}

/// Where everything should be at time `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Targets {
    /// Fraction of the scrollable range, 0 = start.
    pub tl_x: f64,
    pub tl_y: f64,
    /// Position on the log zoom range, 0 = zoomed out.
    pub tl_zoom: f64,
    pub pr_x: f64,
    pub pr_y: f64,
    pub pr_zoom: f64,
    /// Pointer position along the ruler (0..1) while dragging.
    pub drag_x: Option<f64>,
}

/// 0 -> 1 -> 0 with zero speed at both ends.
fn swing(t: f64, period: f64, phase: f64) -> f64 {
    0.5 - 0.5 * (TAU * (t / period + phase)).cos()
}

/// Resting places for things that do not move in a scenario: a middle zoom and
/// the top-left corner.
pub const REST_TL_ZOOM: f64 = 0.55;
pub const REST_PR_ZOOM: f64 = 0.6;

impl Scenario {
    pub fn targets(&self, t: f64) -> Targets {
        Targets {
            tl_x: if self.tl_scroll {
                swing(t, 8.0, 0.0)
            } else {
                0.0
            },
            tl_y: if self.tl_scroll {
                swing(t, 6.0, 0.0)
            } else {
                0.0
            },
            tl_zoom: if self.tl_zoom {
                swing(t, 5.0, 0.0)
            } else {
                REST_TL_ZOOM
            },
            pr_x: if self.pr_scroll {
                swing(t, 7.0, 0.25)
            } else {
                0.0
            },
            pr_y: if self.pr_scroll {
                swing(t, 5.0, 0.0)
            } else {
                0.3
            },
            pr_zoom: if self.pr_zoom {
                swing(t, 4.0, 0.5)
            } else {
                REST_PR_ZOOM
            },
            drag_x: self.drag.then(|| swing(t, 3.0, 0.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_findable() {
        let mut names: Vec<_> = SCENARIOS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), SCENARIOS.len());
        assert!(find("spike1-all-moving").is_some());
        assert!(find("nope").is_none());
    }

    #[test]
    fn targets_stay_in_the_unit_range_and_are_smooth() {
        for s in SCENARIOS {
            let mut prev = s.targets(0.0);
            for step in 1..=800 {
                let t = f64::from(step) * 0.01;
                let cur = s.targets(t);
                for (a, b) in [
                    (prev.tl_x, cur.tl_x),
                    (prev.tl_y, cur.tl_y),
                    (prev.tl_zoom, cur.tl_zoom),
                    (prev.pr_x, cur.pr_x),
                    (prev.pr_y, cur.pr_y),
                    (prev.pr_zoom, cur.pr_zoom),
                ] {
                    assert!((0.0..=1.0).contains(&b), "{}: {b}", s.name);
                    assert!((a - b).abs() < 0.06, "{} jumps at t={t}", s.name);
                }
                prev = cur;
            }
        }
    }

    #[test]
    fn a_still_scenario_does_not_move() {
        let s = find("still").unwrap();
        assert_eq!(s.targets(0.0), s.targets(5.0));
    }

    #[test]
    fn only_the_drag_scenarios_drag() {
        for s in SCENARIOS {
            assert_eq!(s.targets(1.0).drag_x.is_some(), s.drag, "{}", s.name);
        }
    }

    #[test]
    fn spike_two_scenarios_run_meters() {
        for s in SCENARIOS.iter().filter(|s| s.spike == Spike::Two) {
            assert!(s.meters, "{}", s.name);
        }
    }
}
