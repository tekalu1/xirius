//! Callback timing and underrun counters, written by the audio thread with relaxed atomics and
//! read by any other thread.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

/// Width of one histogram bucket.
pub const BUCKET_US: u64 = 10;
/// Number of buckets; the last one also holds everything slower (10 ms and up).
pub const BUCKETS: usize = 1000;

pub struct Stats {
    callbacks: AtomicU64,
    frames: AtomicU64,
    min_frames: AtomicU64,
    max_frames: AtomicU64,
    busy_ns_sum: AtomicU64,
    busy_ns_max: AtomicU64,
    /// Processing took longer than the buffer it filled.
    deadline_misses: AtomicU64,
    /// The gap since the previous callback was more than 1.5 device periods.
    late_callbacks: AtomicU64,
    /// Underruns the device layer reported.
    device_xruns: AtomicU64,
    /// Callbacks that found the device buffer completely drained: the device played silence.
    starved: AtomicU64,
    /// Other errors the device layer reported.
    device_errors: AtomicU64,
    /// Device output latency, in ns: how long until what a callback writes is heard, as the
    /// device reports it ([`Steady`] over the last callbacks).
    output_latency_ns: AtomicU64,
    /// Samples the WAV tap could not queue.
    tap_dropped: AtomicU64,
    /// Frames of the count-in still to click (0: none runs). The control side sets it to the
    /// whole count before the count-in is sent; the audio thread counts it down and clears it.
    count_in_left: AtomicU64,
    /// Frames of the test tone still to play (0: none sounds).
    tone_left: AtomicU64,
    /// The loudest sample sent to the left and the right of the device since the control side
    /// last took them, as `f32` bits (a silent output still measures what it would send).
    output_peak: [AtomicU32; 2],
    histogram: Box<[AtomicU64]>,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            callbacks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            min_frames: AtomicU64::new(u64::MAX),
            max_frames: AtomicU64::new(0),
            busy_ns_sum: AtomicU64::new(0),
            busy_ns_max: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            late_callbacks: AtomicU64::new(0),
            device_xruns: AtomicU64::new(0),
            starved: AtomicU64::new(0),
            device_errors: AtomicU64::new(0),
            output_latency_ns: AtomicU64::new(0),
            tap_dropped: AtomicU64::new(0),
            count_in_left: AtomicU64::new(0),
            tone_left: AtomicU64::new(0),
            output_peak: [AtomicU32::new(0), AtomicU32::new(0)],
            histogram: (0..BUCKETS).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

impl Stats {
    /// Callbacks recorded so far.
    pub fn callbacks(&self) -> u64 {
        self.callbacks.load(Ordering::Relaxed)
    }

    /// Records one callback. `busy` is the time spent rendering, `budget` the duration of the
    /// frames rendered, and `late` whether it started later than the device period allows.
    pub fn record_callback(&self, frames: usize, busy: Duration, budget: Duration, late: bool) {
        let r = Ordering::Relaxed;
        let frames = frames as u64;
        let ns = busy.as_nanos().min(u64::MAX as u128) as u64;
        self.callbacks.fetch_add(1, r);
        self.frames.fetch_add(frames, r);
        self.min_frames.fetch_min(frames, r);
        self.max_frames.fetch_max(frames, r);
        self.busy_ns_sum.fetch_add(ns, r);
        self.busy_ns_max.fetch_max(ns, r);
        let bucket = ((ns / 1000 / BUCKET_US) as usize).min(BUCKETS - 1);
        self.histogram[bucket].fetch_add(1, r);
        if busy > budget {
            self.deadline_misses.fetch_add(1, r);
        }
        if late {
            self.late_callbacks.fetch_add(1, r);
        }
    }

    pub fn record_xrun(&self) {
        self.device_xruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_starved(&self) {
        self.starved.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_device_error(&self) {
        self.device_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_output_latency(&self, latency: Duration) {
        let ns = latency.as_nanos().min(u64::MAX as u128) as u64;
        self.output_latency_ns.store(ns, Ordering::Relaxed);
    }

    pub fn record_tap_dropped(&self, samples: u64) {
        self.tap_dropped.fetch_add(samples, Ordering::Relaxed);
    }

    /// Frames of the count-in still to click (0: none runs).
    pub fn count_in_left(&self) -> u64 {
        self.count_in_left.load(Ordering::Relaxed)
    }

    pub fn set_count_in_left(&self, frames: u64) {
        self.count_in_left.store(frames, Ordering::Relaxed);
    }

    /// Frames of the test tone still to play (0: none sounds).
    pub fn tone_left(&self) -> u64 {
        self.tone_left.load(Ordering::Relaxed)
    }

    pub fn set_tone_left(&self, frames: u64) {
        self.tone_left.store(frames, Ordering::Relaxed);
    }

    /// Keeps the louder of `peak` and what is kept, left and right.
    pub fn record_output_peak(&self, peak: [f32; 2]) {
        for (kept, p) in self.output_peak.iter().zip(peak) {
            // A non-negative float's bits order the same way as the float.
            kept.fetch_max(p.abs().to_bits(), Ordering::Relaxed);
        }
    }

    /// The loudest sample sent to the left and the right since the last call.
    pub fn take_output_peak(&self) -> [f32; 2] {
        [0, 1].map(|i| f32::from_bits(self.output_peak[i].swap(0, Ordering::Relaxed)))
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let r = Ordering::Relaxed;
        let histogram: Vec<u64> = self.histogram.iter().map(|b| b.load(r)).collect();
        let callbacks = self.callbacks.load(r);
        StatsSnapshot {
            callbacks,
            frames: self.frames.load(r),
            min_frames: if callbacks == 0 {
                0
            } else {
                self.min_frames.load(r)
            },
            max_frames: self.max_frames.load(r),
            busy_total: Duration::from_nanos(self.busy_ns_sum.load(r)),
            busy_mean: Duration::from_nanos(self.busy_ns_sum.load(r) / callbacks.max(1)),
            busy_max: Duration::from_nanos(self.busy_ns_max.load(r)),
            busy_p50: percentile(&histogram, 0.50),
            busy_p99: percentile(&histogram, 0.99),
            busy_p999: percentile(&histogram, 0.999),
            deadline_misses: self.deadline_misses.load(r),
            late_callbacks: self.late_callbacks.load(r),
            device_xruns: self.device_xruns.load(r),
            starved: self.starved.load(r),
            device_errors: self.device_errors.load(r),
            output_latency: Duration::from_nanos(self.output_latency_ns.load(r)),
            tap_dropped: self.tap_dropped.load(r),
            histogram,
        }
    }
}

/// Upper edge of the bucket holding the `q` quantile.
fn percentile(histogram: &[u64], q: f64) -> Duration {
    let total: u64 = histogram.iter().sum();
    if total == 0 {
        return Duration::ZERO;
    }
    let want = ((total as f64) * q).ceil() as u64;
    let mut seen = 0;
    for (i, &c) in histogram.iter().enumerate() {
        seen += c;
        if seen >= want {
            return Duration::from_micros((i as u64 + 1) * BUCKET_US);
        }
    }
    Duration::from_micros(BUCKETS as u64 * BUCKET_US)
}

/// Callbacks a [`Steady`] figure is the median of.
pub const STEADY_WINDOW: usize = 32;

/// A latency that holds still: the median of the last [`STEADY_WINDOW`] measurements. A
/// shared-mode device calls back with however much its buffer has room for, so one callback's
/// measurement swings by up to a period; the median of many does not. Kept on the audio thread:
/// it allocates nothing.
#[derive(Clone, Debug)]
pub struct Steady {
    ring: [u64; STEADY_WINDOW],
    len: usize,
    next: usize,
}

impl Default for Steady {
    fn default() -> Self {
        Self::new()
    }
}

impl Steady {
    pub const fn new() -> Steady {
        Steady {
            ring: [0; STEADY_WINDOW],
            len: 0,
            next: 0,
        }
    }

    /// Takes in one measurement and gives the median of those kept.
    pub fn push(&mut self, ns: u64) -> u64 {
        self.ring[self.next] = ns;
        self.next = (self.next + 1) % STEADY_WINDOW;
        self.len = (self.len + 1).min(STEADY_WINDOW);
        let mut sorted = self.ring;
        let kept = &mut sorted[..self.len];
        kept.sort_unstable();
        let mid = self.len / 2;
        if self.len % 2 == 1 {
            kept[mid]
        } else {
            (kept[mid - 1] + kept[mid]) / 2
        }
    }
}

#[derive(Clone, Debug)]
pub struct StatsSnapshot {
    pub callbacks: u64,
    pub frames: u64,
    pub min_frames: u64,
    pub max_frames: u64,
    /// The processing time of every callback so far.
    pub busy_total: Duration,
    pub busy_mean: Duration,
    pub busy_max: Duration,
    /// Quantiles are bucket upper edges, so they are rounded up to 10 µs.
    pub busy_p50: Duration,
    pub busy_p99: Duration,
    pub busy_p999: Duration,
    pub deadline_misses: u64,
    pub late_callbacks: u64,
    pub device_xruns: u64,
    pub starved: u64,
    pub device_errors: u64,
    pub output_latency: Duration,
    pub tap_dropped: u64,
    pub histogram: Vec<u64>,
}

impl StatsSnapshot {
    /// Non-empty histogram buckets as `(lower edge µs, count)`.
    pub fn buckets(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.histogram
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(i, c)| (i as u64 * BUCKET_US, *c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_latency_is_the_median_of_the_last_callbacks() {
        let mut s = Steady::new();
        assert_eq!(s.push(10), 10);
        assert_eq!(s.push(30), 20);
        assert_eq!(s.push(11), 11);
        // A shared-mode device: what is queued swings between none and a whole period from
        // one callback to the next; the figure shown does not.
        let mut s = Steady::new();
        let mut shown = Vec::new();
        for i in 0..200u64 {
            let queued = [0, 9_000_000, 3_000_000, 6_000_000][(i % 4) as usize];
            shown.push(s.push(20_000_000 + queued));
        }
        let last = &shown[STEADY_WINDOW..];
        assert!(last.iter().all(|&ns| ns == 24_500_000), "{last:?}");
    }

    #[test]
    fn counts_misses_and_percentiles() {
        let s = Stats::default();
        let budget = Duration::from_millis(10);
        for _ in 0..99 {
            s.record_callback(480, Duration::from_micros(15), budget, false);
        }
        s.record_callback(480, Duration::from_millis(12), budget, true);
        let snap = s.snapshot();
        assert_eq!(snap.callbacks, 100);
        assert_eq!(snap.frames, 48_000);
        assert_eq!(snap.deadline_misses, 1);
        assert_eq!(snap.late_callbacks, 1);
        assert_eq!(snap.busy_p50, Duration::from_micros(20));
        assert_eq!(snap.busy_max, Duration::from_millis(12));
        assert_eq!(snap.busy_p999, Duration::from_millis(10));
    }
}
