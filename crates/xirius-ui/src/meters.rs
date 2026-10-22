//! Meters for the UI-024 spike.
//!
//! The "engine" side is a thread that publishes block peaks into atomics, the
//! way the real audio thread must (no locks, no allocation). The UI side reads
//! once per frame and applies the ballistics (fall-back and peak hold).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const METER_COUNT: usize = 64;

/// Block peaks (linear, >= 0) written by the engine and taken by the UI.
///
/// Non-negative `f32` bit patterns order like the numbers, so `fetch_max` on the
/// bits keeps the largest peak seen since the UI last looked.
pub struct MeterBus {
    peaks: [AtomicU32; METER_COUNT],
    blocks: AtomicU32,
}

impl Default for MeterBus {
    fn default() -> Self {
        MeterBus {
            peaks: std::array::from_fn(|_| AtomicU32::new(0)),
            blocks: AtomicU32::new(0),
        }
    }
}

impl MeterBus {
    /// Engine side.
    pub fn publish(&self, channel: usize, peak: f32) {
        debug_assert!(peak >= 0.0);
        self.peaks[channel].fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    /// Engine side: one block has been published.
    pub fn count_block(&self) {
        self.blocks.fetch_add(1, Ordering::Relaxed);
    }

    /// Blocks published since the bus was made.
    pub fn blocks(&self) -> u32 {
        self.blocks.load(Ordering::Relaxed)
    }

    /// UI side: the largest peak since the last call.
    pub fn take(&self, channel: usize) -> f32 {
        f32::from_bits(self.peaks[channel].swap(0, Ordering::Relaxed))
    }
}

/// A thread that stands in for the audio callback: one block every `period`.
pub struct EngineSim {
    run: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    bus: Arc<MeterBus>,
}

impl EngineSim {
    pub fn spawn(bus: Arc<MeterBus>, period: Duration) -> EngineSim {
        let run = Arc::new(AtomicBool::new(true));
        let handle = std::thread::Builder::new()
            .name("engine-sim".into())
            .spawn({
                let run = run.clone();
                let bus = bus.clone();
                move || {
                    let start = Instant::now();
                    let mut n = 0u32;
                    while run.load(Ordering::Relaxed) {
                        let t = start.elapsed().as_secs_f32();
                        for ch in 0..METER_COUNT {
                            bus.publish(ch, synth_peak(ch, t));
                        }
                        bus.count_block();
                        n += 1;
                        let due = start + period * n;
                        if let Some(wait) = due.checked_duration_since(Instant::now()) {
                            std::thread::sleep(wait);
                        }
                    }
                }
            })
            .expect("spawn engine-sim");
        EngineSim {
            run,
            handle: Some(handle),
            bus,
        }
    }

    /// Blocks published so far.
    pub fn blocks(&self) -> u32 {
        self.bus.blocks()
    }
}

impl Drop for EngineSim {
    fn drop(&mut self) {
        self.run.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// A moving level for channel `ch` at time `t` seconds: -40 dB to about
/// +1 dB, with every channel at its own speed so the bars do not move in step.
pub fn synth_peak(ch: usize, t: f32) -> f32 {
    let speed = 0.7 + (ch % 7) as f32 * 0.31;
    let phase = ch as f32 * 0.9;
    let slow = 0.5 + 0.5 * (t * speed + phase).sin();
    let jitter = 0.5 + 0.5 * ((t * 37.0 + ch as f32 * 5.3).sin() * (t * 11.0).cos());
    let db = -40.0 + 41.0 * slow * (0.85 + 0.15 * jitter);
    10f32.powf(db / 20.0)
}

/// Meter scale: -60 dBFS at 0, +6 dBFS at 1.
pub const DB_FLOOR: f32 = -60.0;
pub const DB_CEIL: f32 = 6.0;

/// Where the bar changes colour (UI-009 asks for a smooth ramp; this spike
/// uses three flat zones because gradients are off the table).
pub const HOT_DB: f32 = -12.0;
pub const OVER_DB: f32 = 0.0;

pub fn linear_to_pos(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    ((20.0 * x.log10() - DB_FLOOR) / (DB_CEIL - DB_FLOOR)).clamp(0.0, 1.0)
}

pub fn db_to_pos(db: f32) -> f32 {
    ((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR)).clamp(0.0, 1.0)
}

/// UI-side state of all meters.
#[derive(Clone)]
pub struct MeterState {
    /// Bar position 0..=1.
    pub pos: [f32; METER_COUNT],
    /// Peak-hold position 0..=1.
    pub hold: [f32; METER_COUNT],
    hold_left: [f32; METER_COUNT],
    /// Seconds left of the "went over 0 dBFS" light.
    pub over: [f32; METER_COUNT],
}

/// Fall speed in meter-scale units per second (about 30 dB/s).
const FALL_PER_S: f32 = 30.0 / (DB_CEIL - DB_FLOOR);
const HOLD_S: f32 = 1.2;
const OVER_S: f32 = 1.0;

impl Default for MeterState {
    fn default() -> Self {
        MeterState {
            pos: [0.0; METER_COUNT],
            hold: [0.0; METER_COUNT],
            hold_left: [0.0; METER_COUNT],
            over: [0.0; METER_COUNT],
        }
    }
}

impl MeterState {
    /// Applies `dt` seconds of ballistics to the new peaks (linear).
    pub fn update(&mut self, peaks: impl Fn(usize) -> f32, dt: f32) {
        for ch in 0..METER_COUNT {
            let peak = peaks(ch);
            let target = linear_to_pos(peak);
            let fallen = self.pos[ch] - FALL_PER_S * dt;
            self.pos[ch] = target.max(fallen).max(0.0);
            if self.pos[ch] >= self.hold[ch] {
                self.hold[ch] = self.pos[ch];
                self.hold_left[ch] = HOLD_S;
            } else if self.hold_left[ch] > 0.0 {
                self.hold_left[ch] -= dt;
            } else {
                self.hold[ch] = (self.hold[ch] - FALL_PER_S * 0.5 * dt).max(self.pos[ch]);
            }
            if peak > 1.0 {
                self.over[ch] = OVER_S;
            } else {
                self.over[ch] = (self.over[ch] - dt).max(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_keeps_the_largest_peak_until_taken() {
        let bus = MeterBus::default();
        bus.publish(3, 0.2);
        bus.publish(3, 0.9);
        bus.publish(3, 0.4);
        assert_eq!(bus.take(3), 0.9);
        assert_eq!(bus.take(3), 0.0);
        assert_eq!(bus.take(4), 0.0);
    }

    #[test]
    fn scale_endpoints() {
        assert_eq!(linear_to_pos(0.0), 0.0);
        assert_eq!(linear_to_pos(1e-6), 0.0);
        assert!((linear_to_pos(1.0) - db_to_pos(0.0)).abs() < 1e-6);
        assert_eq!(linear_to_pos(100.0), 1.0);
        assert!(linear_to_pos(0.5) < linear_to_pos(0.6));
    }

    #[test]
    fn bar_rises_at_once_and_falls_slowly() {
        let mut m = MeterState::default();
        m.update(|_| 1.0, 1.0 / 60.0);
        let top = m.pos[0];
        assert!((top - db_to_pos(0.0)).abs() < 1e-6);
        m.update(|_| 0.0, 1.0 / 60.0);
        assert!(m.pos[0] < top);
        assert!(m.pos[0] > top - 0.02, "falls about 30 dB/s");
    }

    #[test]
    fn peak_hold_stays_then_decays() {
        let mut m = MeterState::default();
        m.update(|_| 1.0, 0.016);
        let held = m.hold[0];
        for _ in 0..30 {
            m.update(|_| 0.0, 0.016); // 0.48 s: still held
        }
        assert_eq!(m.hold[0], held);
        for _ in 0..300 {
            m.update(|_| 0.0, 0.016);
        }
        assert!(m.hold[0] < held);
    }

    #[test]
    fn over_lights_only_above_zero_dbfs() {
        let mut m = MeterState::default();
        m.update(|_| 0.99, 0.016);
        assert_eq!(m.over[0], 0.0);
        m.update(|_| 1.2, 0.016);
        assert!(m.over[0] > 0.9);
        for _ in 0..100 {
            m.update(|_| 0.1, 0.016);
        }
        assert_eq!(m.over[0], 0.0);
    }

    #[test]
    fn synth_peaks_stay_in_range_and_differ() {
        let mut distinct = std::collections::HashSet::new();
        for ch in 0..METER_COUNT {
            for step in 0..200 {
                let p = synth_peak(ch, step as f32 * 0.05);
                assert!((0.0..1.3).contains(&p));
            }
            distinct.insert(synth_peak(ch, 1.0).to_bits());
        }
        assert!(distinct.len() > 20);
    }

    #[test]
    fn engine_sim_publishes_until_dropped() {
        let bus = Arc::new(MeterBus::default());
        let sim = EngineSim::spawn(bus.clone(), Duration::from_millis(2));
        // Waits for the blocks, not a time: on a busy machine the thread starts late.
        let begun = Instant::now();
        while sim.blocks() < 5 {
            assert!(
                begun.elapsed() < Duration::from_secs(60),
                "{} blocks",
                sim.blocks()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(bus.take(0) > 0.0);
        drop(sim);
    }
}
