//! The quantize panel's settings (EDT-073): where a note's start and end are pulled to
//! (a grid with tuplets and swing, or a groove taken from a clip) and how far (start,
//! end and velocity %), for the notes within a range of their line.

use crate::time::Tick;

/// The lines notes are pulled to.
#[derive(Clone, Debug, PartialEq)]
pub enum Lines {
    /// Every `step`, `tuplet` notes in the time of the next lower power of two of them
    /// (3 in 2, 5 and 7 in 4), every second line later by `swing` % of half a step.
    Grid { step: Tick, tuplet: u32, swing: u32 },
    /// The starts of a clip's notes, with their velocities, repeated every `period`
    /// from `origin`; song ticks.
    Groove {
        origin: Tick,
        period: Tick,
        hits: Vec<(Tick, u8)>,
    },
}

/// How far, in %, each part of a note is pulled.
#[derive(Clone, Debug, PartialEq)]
pub struct Quantize {
    pub lines: Lines,
    pub start: u32,
    /// None leaves the ends where they are.
    pub end: Option<u32>,
    /// Towards the groove's velocity; grid lines have none.
    pub velocity: u32,
    /// Only notes within this % of half the space between lines move.
    pub range: u32,
}

impl Lines {
    /// The line nearest `t`, its velocity (a groove's), and half the space around it.
    pub fn nearest(&self, t: Tick) -> Option<(Tick, Option<u8>, f64)> {
        match self {
            Lines::Grid {
                step,
                tuplet,
                swing,
            } => {
                let step = (*step).max(1);
                let n = Tick::from((*tuplet).max(1));
                let span = step * prev_pow2(n);
                let g = t.div_euclid(span);
                let line = |i: Tick| {
                    let (g, k) = (i.div_euclid(n), i.rem_euclid(n));
                    let mut at = g * span + (k * span + n / 2) / n;
                    if n == 1 && *swing > 0 && i.rem_euclid(2) == 1 {
                        at += step * Tick::from((*swing).min(100)) / 200;
                    }
                    at
                };
                let best = ((g - 1) * n..=(g + 2) * n)
                    .map(line)
                    .min_by_key(|at| ((at - t).abs(), *at))?;
                Some((best, None, span as f64 / n as f64 / 2.0))
            }
            Lines::Groove {
                origin,
                period,
                hits,
            } => {
                let period = (*period).max(1);
                let base = (t - origin).div_euclid(period) * period + origin;
                let best = [base - period, base, base + period]
                    .into_iter()
                    .flat_map(|b| hits.iter().map(move |(at, v)| (b + at, *v)))
                    .min_by_key(|(at, _)| ((at - t).abs(), *at))?;
                Some((
                    best.0,
                    Some(best.1),
                    period as f64 / hits.len().max(1) as f64 / 2.0,
                ))
            }
        }
    }
}

impl Quantize {
    /// A note's `(start, length, velocity)` after quantizing, `offset` being where its
    /// clip starts in the song.
    pub fn note(&self, offset: Tick, start: Tick, length: Tick, velocity: u8) -> (Tick, Tick, u8) {
        let (mut s, mut e, mut v) = (offset + start, offset + start + length, velocity);
        if let Some((line, groove_v, half)) = self.lines.nearest(s)
            && self.within(s, line, half)
        {
            s = pull(s, line, self.start);
            if let Some(gv) = groove_v {
                let pulled = pull(Tick::from(v), Tick::from(gv), self.velocity);
                v = pulled.clamp(1, 127) as u8;
            }
        }
        match self.end {
            // The whole note moves with its start.
            None => e = s + length,
            Some(end) => {
                if let Some((line, _, half)) = self.lines.nearest(e)
                    && self.within(e, line, half)
                {
                    e = pull(e, line, end);
                }
            }
        }
        let length = (e - s).max(1);
        ((s - offset).max(0), length, v)
    }

    fn within(&self, t: Tick, line: Tick, half: f64) -> bool {
        (t - line).abs() as f64 <= half * f64::from(self.range.min(100)) / 100.0 + 0.5
    }
}

/// `value` moved towards `to` by `percent`.
fn pull(value: Tick, to: Tick, percent: u32) -> Tick {
    value + ((to - value) as f64 * f64::from(percent.min(100)) / 100.0).round() as Tick
}

/// The largest power of two below `n` (2 for 3, 4 for 5 and 7), or 1.
fn prev_pow2(n: Tick) -> Tick {
    if n <= 1 {
        return 1;
    }
    let mut p = 1;
    while p * 2 < n {
        p *= 2;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(step: Tick, tuplet: u32, swing: u32) -> Quantize {
        Quantize {
            lines: Lines::Grid {
                step,
                tuplet,
                swing,
            },
            start: 100,
            end: None,
            velocity: 0,
            range: 100,
        }
    }

    #[test]
    fn a_plain_grid_pulls_starts_like_the_old_quantize() {
        let q = grid(240, 1, 0);
        for start in [0, 7, 119, 121, 250, 370, 1000] {
            assert_eq!(
                q.note(0, start, 100, 90),
                (crate::time::quantize_tick(start, 240, 100), 100, 90)
            );
        }
        let mut half = grid(240, 1, 0);
        half.start = 50;
        assert_eq!(half.note(0, 300, 60, 90), (270, 60, 90));
    }

    #[test]
    fn tuplets_fit_n_notes_in_the_time_of_a_power_of_two() {
        // 5 sixteenths in a beat of 960: 0, 192, 384, 576, 768.
        let q = grid(240, 5, 0);
        assert_eq!(q.note(0, 200, 50, 90).0, 192);
        assert_eq!(q.note(0, 1000, 50, 90).0, 960);
        // 7 in a beat: the lines are rounded from the beat, not stacked, so they never drift.
        let q7 = grid(240, 7, 0);
        assert_eq!(q7.note(0, 960 * 100 + 140, 50, 90).0, 960 * 100 + 137);
        // 3 eighths in 2: 0, 320, 640.
        assert_eq!(grid(480, 3, 0).note(0, 300, 50, 90).0, 320);
    }

    #[test]
    fn swing_moves_every_second_line_later() {
        let q = grid(240, 1, 50);
        // The off line at 240 moves to 240 + 60.
        assert_eq!(q.note(0, 290, 50, 90).0, 300);
        assert_eq!(q.note(0, 470, 50, 90).0, 480);
    }

    #[test]
    fn range_leaves_notes_far_from_their_line_and_ends_follow_their_own_percent() {
        let mut q = grid(240, 1, 0);
        q.range = 50;
        // Half of 120 is 60: 50 ticks off moves, 80 does not.
        assert_eq!(q.note(0, 50, 100, 90).0, 0);
        assert_eq!(q.note(0, 80, 100, 90).0, 80);
        q.range = 100;
        q.end = Some(100);
        // Start 250 to 240; the end, 610, to 720 (110 away, where 480 is 130).
        assert_eq!(q.note(0, 250, 360, 90), (240, 480, 90));
    }

    #[test]
    fn a_groove_pulls_starts_and_velocities_and_repeats() {
        let q = Quantize {
            lines: Lines::Groove {
                origin: 3840,
                period: 1920,
                hits: vec![(0, 120), (520, 60), (960, 110)],
            },
            start: 100,
            end: None,
            velocity: 50,
            range: 100,
        };
        // Clip at 0, a note at 500 + one period from the groove's start.
        assert_eq!(
            q.note(0, 3840 + 1920 + 500, 100, 100),
            (3840 + 1920 + 520, 100, 80)
        );
        // Before the groove's clip it repeats backwards.
        assert_eq!(q.note(0, 970, 100, 100), (960, 100, 105));
    }
}
