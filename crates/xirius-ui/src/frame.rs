//! Frame-time hook (UI-029).
//!
//! The UI calls [`FrameLog::begin_frame`] once at the start of every frame
//! (before the view is rebuilt) and [`FrameLog::end_frame`] after the last
//! element has been painted. From that we get two series:
//!
//! * `interval`: time between two frame starts. With vsync this is the
//!   presentation cadence, so a missed vsync shows up as 2x the budget.
//! * `cpu`: time from frame start to the end of paint on the UI thread
//!   (layout + prepaint + paint). It does not include GPU time or present, but
//!   it shows how much of the budget the UI itself uses, which `interval`
//!   hides when vsync caps it.
//!
//! Panes can also add their own paint time so the slowest pane is visible.

use std::time::Instant;

/// 60 fps.
pub const BUDGET_60_MS: f64 = 1000.0 / 60.0;

/// What counts as a pass for one scenario.
#[derive(Clone, Copy, Debug)]
pub struct Criteria {
    /// Mean frame rate must be at least this.
    pub min_fps: f64,
    /// 99th percentile of the frame interval must be at most this.
    pub max_p99_ms: f64,
}

impl Criteria {
    /// 60 fps with a little slack for timer jitter: mean >= 59 fps and
    /// p99 <= 20 ms (1.2 frames).
    pub const FPS_60: Criteria = Criteria {
        min_fps: 59.0,
        max_p99_ms: 20.0,
    };
}

/// Summary of a series of durations in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Distribution {
    pub count: usize,
    pub mean: f64,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl Distribution {
    pub fn from_samples(samples: &[f64]) -> Distribution {
        if samples.is_empty() {
            return Distribution::default();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let sum: f64 = sorted.iter().sum();
        Distribution {
            count: sorted.len(),
            mean: sum / sorted.len() as f64,
            min: sorted[0],
            p50: percentile(&sorted, 50.0),
            p95: percentile(&sorted, 95.0),
            p99: percentile(&sorted, 99.0),
            max: sorted[sorted.len() - 1],
        }
    }
}

/// Nearest-rank percentile of an ascending slice. `p` is in 0..=100.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Panes whose own paint time is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Headers,
    Timeline,
    PianoRoll,
    Meters,
}

impl Pane {
    pub const ALL: [Pane; 4] = [Pane::Headers, Pane::Timeline, Pane::PianoRoll, Pane::Meters];

    pub fn name(self) -> &'static str {
        match self {
            Pane::Headers => "headers",
            Pane::Timeline => "timeline",
            Pane::PianoRoll => "piano-roll",
            Pane::Meters => "meters",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Raw samples for one scenario.
#[derive(Clone, Debug, Default)]
pub struct FrameLog {
    /// Frames to ignore at the start (shader / glyph / atlas warm-up).
    skip: usize,
    seen: usize,
    last_begin: Option<Instant>,
    cur_begin: Option<Instant>,
    cur_panes: [f64; 4],
    intervals: Vec<f64>,
    cpu: Vec<f64>,
    panes: [Vec<f64>; 4],
}

impl FrameLog {
    pub fn new(skip: usize) -> FrameLog {
        FrameLog {
            skip,
            ..FrameLog::default()
        }
    }

    pub fn begin_frame(&mut self, now: Instant) {
        self.cur_begin = Some(now);
        self.cur_panes = [0.0; 4];
    }

    /// Adds paint time for a pane in the current frame.
    pub fn add_pane(&mut self, pane: Pane, ms: f64) {
        self.cur_panes[pane.index()] += ms;
    }

    pub fn end_frame(&mut self, now: Instant) {
        let Some(begin) = self.cur_begin.take() else {
            return;
        };
        self.seen += 1;
        let interval = self.last_begin.map(|prev| ms_between(prev, begin));
        self.last_begin = Some(begin);
        if self.seen <= self.skip {
            return;
        }
        if let Some(interval) = interval {
            self.intervals.push(interval);
        }
        self.cpu.push(ms_between(begin, now));
        for (series, value) in self.panes.iter_mut().zip(self.cur_panes) {
            series.push(value);
        }
    }

    pub fn frames(&self) -> usize {
        self.cpu.len()
    }

    pub fn summarize(&self, criteria: Criteria) -> FrameSummary {
        let interval = Distribution::from_samples(&self.intervals);
        let dropped = self
            .intervals
            .iter()
            .filter(|ms| **ms > BUDGET_60_MS * 1.5)
            .count();
        let fps = if interval.mean > 0.0 {
            1000.0 / interval.mean
        } else {
            0.0
        };
        let pass =
            interval.count > 0 && fps >= criteria.min_fps && interval.p99 <= criteria.max_p99_ms;
        FrameSummary {
            frames: self.cpu.len(),
            interval,
            cpu: Distribution::from_samples(&self.cpu),
            panes: Pane::ALL.map(|pane| Distribution::from_samples(&self.panes[pane.index()])),
            fps,
            dropped,
            pass,
        }
    }

    /// Mean / p95 / p99 of the most recent `n` frame intervals, for the HUD.
    pub fn recent(&self, n: usize) -> Distribution {
        let start = self.intervals.len().saturating_sub(n);
        Distribution::from_samples(&self.intervals[start..])
    }
}

fn ms_between(a: Instant, b: Instant) -> f64 {
    b.duration_since(a).as_secs_f64() * 1000.0
}

#[derive(Clone, Debug)]
pub struct FrameSummary {
    pub frames: usize,
    pub interval: Distribution,
    pub cpu: Distribution,
    pub panes: [Distribution; 4],
    /// Mean frame rate from the mean interval.
    pub fps: f64,
    /// Frames whose interval was over 1.5 budgets (at least one missed vsync).
    pub dropped: usize,
    pub pass: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&v, 99.0), 99.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&[], 99.0), 0.0);
    }

    #[test]
    fn distribution_of_known_samples() {
        let d = Distribution::from_samples(&[4.0, 1.0, 3.0, 2.0]);
        assert_eq!(d.count, 4);
        assert_eq!(d.mean, 2.5);
        assert_eq!((d.min, d.max), (1.0, 4.0));
        assert_eq!(d.p50, 2.0);
        assert_eq!(Distribution::from_samples(&[]), Distribution::default());
    }

    fn feed(log: &mut FrameLog, intervals_ms: &[u64], cpu_ms: u64) {
        let origin = Instant::now();
        let mut t = origin;
        log.begin_frame(t);
        log.end_frame(t + Duration::from_millis(cpu_ms));
        for ms in intervals_ms {
            t += Duration::from_millis(*ms);
            log.begin_frame(t);
            log.end_frame(t + Duration::from_millis(cpu_ms));
        }
    }

    #[test]
    fn steady_60fps_passes() {
        let mut log = FrameLog::new(0);
        // 16.7 ms is not exact in whole ms; 17 ms is 58.8 fps, so use 16/17 mix.
        feed(&mut log, &[16, 17].repeat(100), 3);
        let s = log.summarize(Criteria::FPS_60);
        assert_eq!(s.frames, 201);
        assert_eq!(s.interval.count, 200);
        assert!(s.fps > 59.0, "{}", s.fps);
        assert_eq!(s.dropped, 0);
        assert!(s.pass);
        assert!((s.cpu.mean - 3.0).abs() < 0.5);
    }

    #[test]
    fn missed_vsyncs_fail() {
        let mut log = FrameLog::new(0);
        let mut intervals = vec![16u64; 90];
        intervals.extend([33; 10]);
        feed(&mut log, &intervals, 3);
        let s = log.summarize(Criteria::FPS_60);
        assert_eq!(s.dropped, 10);
        assert!(s.interval.p99 >= 33.0);
        assert!(!s.pass);
    }

    #[test]
    fn warmup_frames_are_skipped() {
        let mut log = FrameLog::new(5);
        // A 200 ms stall in the warm-up must not show up.
        let mut intervals = vec![200u64, 16, 16, 16, 16];
        intervals.extend([16u64; 50]);
        feed(&mut log, &intervals, 2);
        let s = log.summarize(Criteria::FPS_60);
        assert!(s.interval.max < 20.0, "{:?}", s.interval);
    }

    #[test]
    fn pane_time_is_summed_per_frame() {
        let mut log = FrameLog::new(0);
        let t = Instant::now();
        log.begin_frame(t);
        log.add_pane(Pane::Timeline, 1.0);
        log.add_pane(Pane::Timeline, 0.5);
        log.end_frame(t + Duration::from_millis(2));
        let s = log.summarize(Criteria::FPS_60);
        assert_eq!(s.panes[1].mean, 1.5);
        assert_eq!(s.panes[0].mean, 0.0);
    }

    #[test]
    fn recent_window_uses_only_the_tail() {
        let mut log = FrameLog::new(0);
        feed(&mut log, &[100, 100, 10, 10], 1);
        assert_eq!(log.recent(2).mean, 10.0);
    }
}
