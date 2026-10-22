//! Ticks to seconds and frames across the song's tempo points, jumps and ramps alike.
//!
//! A point with `ramp` glides linearly in bpm to the next point; any other point holds its bpm
//! until the next one. The time a ramp takes has a closed form (the integral of 60 / (bpm · ppq)
//! over ticks), so a position never depends on a step size.

use crate::model::{Project, TempoPoint};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pt {
    tick: i64,
    bpm: f64,
    ramp: bool,
    /// Seconds from tick 0 to this point.
    secs: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    ppq: f64,
    pts: Vec<Pt>,
}

const MIN_BPM: f64 = 1.0;

impl TempoMap {
    pub fn from_project(p: &Project) -> TempoMap {
        TempoMap::new(p.ppq, &p.tempo)
    }

    pub fn new(ppq: u32, points: &[TempoPoint]) -> TempoMap {
        let ppq = f64::from(ppq.max(1));
        let mut sorted: Vec<(i64, f64, bool)> = points
            .iter()
            .map(|t| (t.tick.max(0), t.bpm.max(MIN_BPM), t.ramp))
            .collect();
        sorted.sort_by_key(|p| p.0);
        sorted.dedup_by_key(|p| p.0);
        if sorted.first().is_none_or(|p| p.0 > 0) {
            let bpm = sorted.first().map_or(120.0, |p| p.1);
            sorted.insert(0, (0, bpm, false));
        }
        let mut pts: Vec<Pt> = Vec::with_capacity(sorted.len());
        for &(tick, bpm, ramp) in &sorted {
            let secs = match pts.last() {
                None => 0.0,
                Some(prev) => {
                    prev.secs + segment_secs(ppq, prev, Some((tick, bpm)), tick - prev.tick)
                }
            };
            pts.push(Pt {
                tick,
                bpm,
                ramp,
                secs,
            });
        }
        TempoMap { ppq, pts }
    }

    pub fn ppq(&self) -> u32 {
        self.ppq as u32
    }

    /// The bpm at a tick (inside a ramp, the glided value).
    pub fn bpm_at(&self, tick: i64) -> f64 {
        let i = self.index_of_tick(tick);
        let p = &self.pts[i];
        match self.pts.get(i + 1) {
            Some(n) if p.ramp && n.tick > p.tick => {
                let f = (tick - p.tick) as f64 / (n.tick - p.tick) as f64;
                p.bpm + (n.bpm - p.bpm) * f
            }
            _ => p.bpm,
        }
    }

    fn index_of_tick(&self, tick: i64) -> usize {
        self.pts
            .partition_point(|p| p.tick <= tick)
            .saturating_sub(1)
    }

    /// Seconds from the start of the song. Before tick 0 the first tempo continues backwards.
    pub fn seconds_at(&self, tick: i64) -> f64 {
        if tick < 0 {
            return tick as f64 * 60.0 / (self.pts[0].bpm * self.ppq);
        }
        let i = self.index_of_tick(tick);
        let p = &self.pts[i];
        let next = self.pts.get(i + 1).map(|n| (n.tick, n.bpm));
        p.secs + segment_secs(self.ppq, p, next, tick - p.tick)
    }

    /// The tick at a time; the inverse of [`Self::seconds_at`], rounded to the nearest tick.
    pub fn tick_at(&self, secs: f64) -> i64 {
        if secs <= 0.0 {
            return (secs * self.pts[0].bpm * self.ppq / 60.0).round() as i64;
        }
        let i = self
            .pts
            .partition_point(|p| p.secs <= secs)
            .saturating_sub(1);
        let p = &self.pts[i];
        let rem = secs - p.secs;
        let dt = match self.pts.get(i + 1) {
            Some(n) if p.ramp && (n.bpm - p.bpm).abs() > 1e-9 => {
                let span = (n.tick - p.tick) as f64;
                let k = self.ppq * (n.bpm - p.bpm) / (60.0 * span);
                let bpm = p.bpm * (rem * k).exp();
                (bpm - p.bpm) * span / (n.bpm - p.bpm)
            }
            _ => rem * p.bpm * self.ppq / 60.0,
        };
        p.tick + dt.round() as i64
    }

    /// The frame a tick falls on at `rate` (never before the start of the song).
    pub fn frame_at(&self, tick: i64, rate: f64) -> u64 {
        (self.seconds_at(tick) * rate).round().max(0.0) as u64
    }

    /// The tick a frame falls on.
    pub fn tick_of_frame(&self, frame: u64, rate: f64) -> i64 {
        self.tick_at(frame as f64 / rate)
    }
}

/// Seconds from `p` to `dt` ticks later, `dt` not past `next`.
fn segment_secs(ppq: f64, p: &Pt, next: Option<(i64, f64)>, dt: i64) -> f64 {
    let dt = dt as f64;
    match next {
        Some((tick, bpm)) if p.ramp && tick > p.tick && (bpm - p.bpm).abs() > 1e-9 => {
            let span = (tick - p.tick) as f64;
            let at = p.bpm + (bpm - p.bpm) * dt / span;
            (60.0 / ppq) * (span / (bpm - p.bpm)) * (at / p.bpm).ln()
        }
        _ => dt * 60.0 / (p.bpm * ppq),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::TempoId;

    fn pt(n: u64, tick: i64, bpm: f64, ramp: bool) -> TempoPoint {
        TempoPoint {
            id: TempoId(n),
            tick,
            bpm,
            ramp,
        }
    }

    #[test]
    fn constant_tempo_is_half_a_second_per_beat_at_120() {
        let m = TempoMap::new(960, &[pt(1, 0, 120.0, false)]);
        assert!((m.seconds_at(960) - 0.5).abs() < 1e-12);
        assert_eq!(m.frame_at(960, 48_000.0), 24_000);
        assert_eq!(m.tick_at(0.5), 960);
    }

    #[test]
    fn a_jump_from_120_to_128_changes_the_length_of_a_beat_after_it() {
        // Two bars of 4/4 at 120 take 4 s; a beat after that at 128 takes 60/128 s.
        let m = TempoMap::new(960, &[pt(1, 0, 120.0, false), pt(2, 7680, 128.0, false)]);
        assert!((m.seconds_at(7680) - 4.0).abs() < 1e-12);
        assert!((m.seconds_at(7680 + 960) - (4.0 + 60.0 / 128.0)).abs() < 1e-12);
        // The same beat before the jump is 0.5 s.
        assert!((m.seconds_at(960) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_ramp_matches_numerical_integration() {
        let m = TempoMap::new(960, &[pt(1, 0, 120.0, true), pt(2, 3840, 240.0, false)]);
        let mut secs = 0.0;
        for t in 0..3840 {
            let bpm = 120.0 + 120.0 * (f64::from(t) + 0.5) / 3840.0;
            secs += 60.0 / (bpm * 960.0);
        }
        assert!(
            (m.seconds_at(3840) - secs).abs() < 1e-6,
            "{} vs {secs}",
            m.seconds_at(3840)
        );
        // Closed form: (60/ppq) * span/(b1-b0) * ln(b1/b0).
        let closed = (60.0 / 960.0) * (3840.0 / 120.0) * 2f64.ln();
        assert!((m.seconds_at(3840) - closed).abs() < 1e-12);
    }

    #[test]
    fn tick_at_is_the_inverse_of_seconds_at_across_jumps_and_ramps() {
        let m = TempoMap::new(
            960,
            &[
                pt(1, 0, 120.0, false),
                pt(2, 3840, 90.0, true),
                pt(3, 9600, 150.0, false),
            ],
        );
        for tick in [0, 1, 959, 3840, 5000, 7777, 9600, 9601, 20_000] {
            assert_eq!(m.tick_at(m.seconds_at(tick)), tick, "tick {tick}");
        }
    }

    #[test]
    fn a_missing_first_point_extends_the_first_tempo_backwards() {
        let m = TempoMap::new(960, &[pt(1, 960, 60.0, false)]);
        assert!((m.seconds_at(960) - 1.0).abs() < 1e-12);
        assert!((m.seconds_at(-960) + 1.0).abs() < 1e-12);
    }
}
