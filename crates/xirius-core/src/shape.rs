//! The shapes the pencil draws (UI-128): freehand, line, parabola, square, triangle, saw, sine
//! and transform. The same shapes serve the controller lanes, the automation lanes and the
//! tempo row, so the maths lives here and the commands only lay the points down.

use serde::{Deserialize, Serialize};

use crate::time::Tick;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Free,
    Line,
    Parabola,
    Square,
    Triangle,
    Saw,
    Sine,
    /// Not a stroke: the pencil selects a range and opens the transform frame.
    Transform,
}

impl Shape {
    pub const ALL: [Shape; 8] = [
        Shape::Free,
        Shape::Line,
        Shape::Parabola,
        Shape::Square,
        Shape::Triangle,
        Shape::Saw,
        Shape::Sine,
        Shape::Transform,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Free => "free",
            Shape::Line => "line",
            Shape::Parabola => "parabola",
            Shape::Square => "square",
            Shape::Triangle => "triangle",
            Shape::Saw => "saw",
            Shape::Sine => "sine",
            Shape::Transform => "transform",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Shape::Free => "フリーハンド",
            Shape::Line => "直線",
            Shape::Parabola => "放物線",
            Shape::Square => "矩形波",
            Shape::Triangle => "三角波",
            Shape::Saw => "ノコギリ波",
            Shape::Sine => "サイン波",
            Shape::Transform => "トランスフォーム",
        }
    }

    pub fn parse(text: &str) -> Option<Shape> {
        let text = text.trim().to_ascii_lowercase();
        Shape::ALL
            .into_iter()
            .find(|s| s.name() == text)
            .or(match text.as_str() {
                "para" => Some(Shape::Parabola),
                "tri" => Some(Shape::Triangle),
                "xform" => Some(Shape::Transform),
                _ => None,
            })
    }

    /// The shape `by` steps along the list (wraps around).
    pub fn step(self, by: i32) -> Shape {
        let at = Shape::ALL.iter().position(|s| *s == self).unwrap_or(0) as i32;
        Shape::ALL[(at + by).rem_euclid(Shape::ALL.len() as i32) as usize]
    }

    /// A wave: a repeating shape with a period and an amplitude.
    pub fn is_wave(self) -> bool {
        matches!(
            self,
            Shape::Square | Shape::Triangle | Shape::Saw | Shape::Sine
        )
    }

    /// A shape the commands can lay down from the numbers alone (the freehand one needs the
    /// points the pointer visited, and transform draws nothing).
    pub fn is_generated(self) -> bool {
        !matches!(self, Shape::Free | Shape::Transform)
    }
}

/// Bend of a parabola's first segment (the lanes' curve value, -1..=1: it starts slowly).
pub const PARABOLA_CURVE: f64 = -0.8;

/// One stroke: from `from` to `to`, the centre line going from `v0` to `v1`, with a wave of
/// `amp` around it. `period` and `phase` are in ticks.
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub from: Tick,
    pub to: Tick,
    pub v0: f64,
    pub v1: f64,
    pub amp: f64,
    pub period: f64,
    pub phase: f64,
}

/// A point of a stroke. `step` holds the value up to the next point; `curve` bends the line
/// to the next point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dot {
    pub tick: Tick,
    pub value: f64,
    pub step: bool,
    pub curve: f64,
}

impl Dot {
    fn line(tick: Tick, value: f64) -> Dot {
        Dot {
            tick,
            value,
            step: false,
            curve: 0.0,
        }
    }
}

fn frac(x: f64) -> f64 {
    x - x.floor()
}

fn wave(shape: Shape, ph: f64) -> f64 {
    let t = frac(ph);
    match shape {
        Shape::Square => {
            if t < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Shape::Triangle => {
            if t < 0.25 {
                4.0 * t
            } else if t < 0.75 {
                2.0 - 4.0 * t
            } else {
                4.0 * t - 4.0
            }
        }
        Shape::Saw => 2.0 * t - 1.0,
        Shape::Sine => (ph * std::f64::consts::TAU).sin(),
        _ => 0.0,
    }
}

/// Where in one period a wave has a point, and whether the point holds its value.
fn knots(shape: Shape) -> Vec<(f64, bool)> {
    match shape {
        Shape::Square => vec![(0.0, true), (0.5, true)],
        Shape::Triangle => vec![(0.0, false), (0.25, false), (0.5, false), (0.75, false)],
        Shape::Saw => vec![(0.0, false), (1.0 - 1.0 / 256.0, true)],
        Shape::Sine => (0..12).map(|i| (f64::from(i) / 12.0, false)).collect(),
        _ => Vec::new(),
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The value of a stroke's wave at `tick` (before clamping to the lane's range).
pub fn value_at(shape: Shape, s: &Stroke, tick: f64) -> f64 {
    let span = (s.to - s.from).max(1) as f64;
    let centre = lerp(s.v0, s.v1, ((tick - s.from as f64) / span).clamp(0.0, 1.0));
    if shape.is_wave() && s.period > 0.0 {
        centre + s.amp * wave(shape, (tick - (s.from as f64 + s.phase)) / s.period)
    } else {
        centre
    }
}

/// The points of a generated stroke, in time order. A line is its two ends, a parabola its two
/// ends with a bent first segment, and a wave a point at each corner of its period (a sine at
/// twelve places). Values are not clamped.
pub fn generate(shape: Shape, s: &Stroke) -> Vec<Dot> {
    let (a, b) = (s.from, s.to.max(s.from + 1));
    match shape {
        Shape::Line | Shape::Parabola => {
            let mut first = Dot::line(a, s.v0);
            if shape == Shape::Parabola {
                first.curve = PARABOLA_CURVE;
            }
            vec![first, Dot::line(b, s.v1)]
        }
        s2 if s2.is_wave() && s.period >= 1.0 => {
            let origin = s.from as f64 + s.phase;
            let knots = knots(shape);
            let mut dots = vec![Dot {
                step: knots.first().is_some_and(|k| k.1),
                ..Dot::line(a, value_at(shape, s, a as f64))
            }];
            let mut n = ((a as f64 - origin) / s.period).floor() as i64 - 1;
            while origin + (n as f64) * s.period <= b as f64 {
                for (k, hold) in &knots {
                    let at = (origin + (n as f64 + k) * s.period).round() as Tick;
                    if at > a && at < b {
                        dots.push(Dot {
                            step: *hold,
                            ..Dot::line(at, value_at(shape, s, at as f64))
                        });
                    }
                }
                n += 1;
            }
            dots.push(Dot::line(b, value_at(shape, s, b as f64)));
            dots.dedup_by_key(|d| d.tick);
            dots
        }
        // A wave without a period is its centre line.
        _ => vec![Dot::line(a, s.v0), Dot::line(b, s.v1)],
    }
}

/// Thin a freehand stroke: keep the points that move the line by more than `eps` (in the
/// stroke's own units) away from the straight line between the points kept around them.
pub fn thin(points: &[(Tick, f64)], eps: f64) -> Vec<(Tick, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (p, q) = (points[0], points[points.len() - 1]);
    let span = (q.0 - p.0).max(1) as f64;
    let (mut worst, mut at) = (0.0, 0);
    for (i, r) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let on = lerp(p.1, q.1, (r.0 - p.0) as f64 / span);
        let d = (r.1 - on).abs();
        if d > worst {
            (worst, at) = (d, i);
        }
    }
    if worst <= eps {
        return vec![p, q];
    }
    let mut left = thin(&points[..=at], eps);
    left.pop();
    left.extend(thin(&points[at..], eps));
    left
}

/// A freehand stroke read at every `step` ticks from `from` to `to` (the grid when snapping).
pub fn resample(points: &[(Tick, f64)], from: Tick, to: Tick, step: Tick) -> Vec<(Tick, f64)> {
    let mut sorted = points.to_vec();
    sorted.sort_by_key(|p| p.0);
    let Some(&(_, first)) = sorted.first() else {
        return Vec::new();
    };
    let last = sorted[sorted.len() - 1].1;
    let at = |t: Tick| -> f64 {
        if t <= sorted[0].0 {
            return first;
        }
        for w in sorted.windows(2) {
            if t <= w[1].0 {
                let span = (w[1].0 - w[0].0).max(1) as f64;
                return lerp(w[0].1, w[1].1, (t - w[0].0) as f64 / span);
            }
        }
        last
    };
    let step = step.max(1);
    let mut out = Vec::new();
    let mut t = from;
    while t <= to {
        out.push((t, at(t)));
        t += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> Stroke {
        Stroke {
            from: 0,
            to: 3840,
            v0: 0.0,
            v1: 0.0,
            amp: 1.0,
            period: 960.0,
            phase: 0.0,
        }
    }

    #[test]
    fn names_round_trip_and_cycle() {
        for s in Shape::ALL {
            assert_eq!(Shape::parse(s.name()), Some(s));
        }
        assert_eq!(Shape::parse("para"), Some(Shape::Parabola));
        assert_eq!(Shape::Free.step(-1), Shape::Transform);
        assert_eq!(Shape::Transform.step(1), Shape::Free);
        assert_eq!(Shape::Line.step(2), Shape::Square);
    }

    #[test]
    fn a_line_is_its_two_ends_and_a_parabola_bends_the_first() {
        let mut s = stroke();
        s.v0 = 10.0;
        s.v1 = 50.0;
        let line = generate(Shape::Line, &s);
        assert_eq!(line.len(), 2);
        assert_eq!((line[0].value, line[1].value), (10.0, 50.0));
        assert_eq!(line[0].curve, 0.0);
        let para = generate(Shape::Parabola, &s);
        assert_eq!(para[0].curve, PARABOLA_CURVE);
        assert_eq!(para[1].tick, 3840);
    }

    #[test]
    fn a_square_wave_holds_two_values_per_period() {
        let dots = generate(Shape::Square, &stroke());
        // 4 periods: a point every half period, then the end.
        assert_eq!(dots.len(), 9);
        assert!(dots[..8].iter().all(|d| d.step));
        assert_eq!(dots[0].value, 1.0);
        assert_eq!(dots[1].value, -1.0);
        assert_eq!(dots[1].tick, 480);
    }

    #[test]
    fn triangle_saw_and_sine_have_their_corners() {
        let tri = generate(Shape::Triangle, &stroke());
        assert_eq!(tri[1].tick, 240);
        assert!((tri[1].value - 1.0).abs() < 1e-9);
        assert!((tri[3].value + 1.0).abs() < 1e-9);
        let saw = generate(Shape::Saw, &stroke());
        assert!((saw[0].value + 1.0).abs() < 1e-9);
        assert!(saw[1].step);
        let sine = generate(Shape::Sine, &stroke());
        assert_eq!(sine.len(), 4 * 12 + 1);
        assert!((sine[3].value - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_centre_line_follows_the_two_ends_and_the_phase_shifts_the_wave() {
        let mut s = stroke();
        s.amp = 0.0;
        s.v0 = 0.0;
        s.v1 = 100.0;
        assert!((value_at(Shape::Sine, &s, 1920.0) - 50.0).abs() < 1e-9);
        let mut shifted = stroke();
        shifted.phase = 240.0;
        let dots = generate(Shape::Triangle, &shifted);
        assert_eq!(dots[1].tick, 240);
        assert!(dots[1].value.abs() < 1e-9);
    }

    #[test]
    fn thinning_keeps_the_corners_of_a_stroke() {
        let pts: Vec<(Tick, f64)> = (0..=20)
            .map(|i| (i * 10, if i <= 10 { i as f64 } else { 20.0 - i as f64 }))
            .collect();
        let kept = thin(&pts, 0.01);
        assert_eq!(kept, vec![(0, 0.0), (100, 10.0), (200, 0.0)]);
        let flat: Vec<(Tick, f64)> = (0..=9).map(|i| (i * 10, 5.0)).collect();
        assert_eq!(thin(&flat, 0.01).len(), 2);
    }

    #[test]
    fn resampling_reads_the_stroke_at_the_grid() {
        let pts = [(0, 0.0), (100, 100.0)];
        let out = resample(&pts, 0, 100, 25);
        assert_eq!(out.len(), 5);
        assert_eq!(out[2], (50, 50.0));
        assert!(resample(&[], 0, 100, 25).is_empty());
    }
}
