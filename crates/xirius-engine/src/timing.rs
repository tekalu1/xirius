//! How long each plugin takes to process a block (ENG-008).
//!
//! A [`Timed`] wraps a processor and, around each `process`, reads the clock and writes the
//! block's time into the processor's [`Meter`]: two relaxed stores into a ring allocated when
//! the meter is made, so the audio thread allocates and locks nothing. Any other thread reads
//! the ring with [`Meter::read`]: the load over about the last second, and the slowest block in
//! it. A [`Timings`] is the list of meters of one song, by plugin id; only the thread that builds
//! a plan touches its lock.
//!
//! A plugin in `xirius-plugin-host` is timed there, around its own processing (ENG-060): the
//! engine's side only hands the audio over, and its clock would read the hand-over (or the
//! wait for it), not the plugin.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::processor::{AudioIo, NoteEvent, ProcessSetup, Processor};

/// Blocks the ring keeps: a second of 64-frame blocks at 48 kHz is 750, so the window is the
/// shorter of a second and this many blocks.
pub const RING: usize = 512;
/// How much audio a reading covers, in seconds.
pub const WINDOW_SECS: f64 = 1.0;

/// The times of one processor's last [`RING`] blocks.
pub struct Meter {
    rate: AtomicU64,
    /// Blocks written so far.
    written: AtomicU64,
    ns: Box<[AtomicU64]>,
    frames: Box<[AtomicU32]>,
    /// The slowest block since the meter was made, in ns.
    peak_ns: AtomicU64,
    /// The times are the plugin host's.
    child: AtomicBool,
}

/// What a [`Meter`] read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// Blocks the reading covers (0: nothing has been processed yet).
    pub blocks: u32,
    /// Processing time over the audio time of those blocks: 0.15 is 15% of the deadline.
    pub load: f64,
    /// The mean time of one block, in µs.
    pub mean_us: f64,
    /// The slowest block of the reading, in µs.
    pub max_us: f64,
    /// The slowest block of the reading over the time its audio lasts.
    pub max_load: f64,
    /// The slowest block since the meter was made, in µs.
    pub peak_us: f64,
    /// The times were taken in the plugin host's process, around the plugin's own work.
    pub child: bool,
}

impl Meter {
    pub fn new() -> Meter {
        Meter {
            rate: AtomicU64::new(0f64.to_bits()),
            written: AtomicU64::new(0),
            ns: (0..RING).map(|_| AtomicU64::new(0)).collect(),
            frames: (0..RING).map(|_| AtomicU32::new(0)).collect(),
            peak_ns: AtomicU64::new(0),
            child: AtomicBool::new(false),
        }
    }

    pub fn set_rate(&self, rate: f64) {
        self.rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    /// One block of `frames` that took `ns`. Called by the audio thread only.
    pub fn record(&self, frames: usize, ns: u64) {
        let r = Ordering::Relaxed;
        let at = self.written.load(r);
        let i = (at % RING as u64) as usize;
        self.ns[i].store(ns, r);
        self.frames[i].store(frames.min(u32::MAX as usize) as u32, r);
        self.written.store(at + 1, r);
        self.peak_ns.fetch_max(ns, r);
    }

    /// The last second or so of blocks.
    pub fn read(&self) -> Reading {
        let r = Ordering::Relaxed;
        let rate = f64::from_bits(self.rate.load(r));
        let written = self.written.load(r);
        let mut reading = Reading {
            peak_us: self.peak_ns.load(r) as f64 / 1000.0,
            child: self.child.load(r),
            ..Reading::default()
        };
        if rate <= 0.0 || written == 0 {
            return reading;
        }
        let (mut busy, mut audio, mut slowest, mut slowest_load) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut blocks = 0u32;
        for back in 1..=written.min(RING as u64) {
            let i = ((written - back) % RING as u64) as usize;
            let frames = f64::from(self.frames[i].load(r));
            let ns = self.ns[i].load(r) as f64;
            if frames == 0.0 {
                continue;
            }
            let budget = frames / rate * 1e9;
            busy += ns;
            audio += budget;
            slowest = slowest.max(ns);
            slowest_load = slowest_load.max(ns / budget);
            blocks += 1;
            if audio >= WINDOW_SECS * 1e9 {
                break;
            }
        }
        if blocks > 0 && audio > 0.0 {
            reading.blocks = blocks;
            reading.load = busy / audio;
            reading.mean_us = busy / f64::from(blocks) / 1000.0;
            reading.max_us = slowest / 1000.0;
            reading.max_load = slowest_load;
        }
        reading
    }
}

impl Default for Meter {
    fn default() -> Self {
        Meter::new()
    }
}

/// The meters of a song's plugins, by the id they are told apart with (the plugin's id in the
/// song, as a number). A new plan's meters replace the old ones; the old plan keeps writing to
/// its own until it is dropped.
#[derive(Default)]
pub struct Timings {
    meters: Mutex<Vec<(u64, Arc<Meter>)>>,
    /// How long each plugin took to open, the last time it was opened (ENG-064).
    opened: Mutex<Vec<(u64, Duration)>>,
}

impl Timings {
    pub fn new() -> Timings {
        Timings::default()
    }

    /// A new meter for plugin `id`, replacing the one of an earlier plan.
    pub fn meter(&self, id: u64) -> Arc<Meter> {
        let m = Arc::new(Meter::new());
        let mut all = self.meters.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|(i, _)| *i != id);
        all.push((id, m.clone()));
        m
    }

    /// Forgets the meters of plugins not in `keep`.
    pub fn retain(&self, keep: impl Fn(u64) -> bool) {
        let mut all = self.meters.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|(i, _)| keep(*i));
        let mut opened = self.opened.lock().unwrap_or_else(|e| e.into_inner());
        opened.retain(|(i, _)| keep(*i));
    }

    /// Every meter's reading, by id.
    pub fn read(&self) -> Vec<(u64, Reading)> {
        let all = self.meters.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().map(|(i, m)| (*i, m.read())).collect()
    }

    pub fn reading(&self, id: u64) -> Option<Reading> {
        let all = self.meters.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().find(|(i, _)| *i == id).map(|(_, m)| m.read())
    }

    /// Plugin `id` took `took` to open (ENG-064).
    pub fn set_opened(&self, id: u64, took: Duration) {
        let mut all = self.opened.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|(i, _)| *i != id);
        all.push((id, took));
    }

    /// How long plugin `id` took to open the last time.
    pub fn opened(&self, id: u64) -> Option<Duration> {
        let all = self.opened.lock().unwrap_or_else(|e| e.into_inner());
        all.iter().find(|(i, _)| *i == id).map(|(_, d)| *d)
    }
}

/// A processor that times its inner one, or takes the times its inner one's host took.
pub struct Timed {
    inner: Box<dyn Processor>,
    meter: Arc<Meter>,
    /// The host's frames and time last seen.
    child: Option<(u64, u64)>,
}

impl Timed {
    pub fn new(inner: Box<dyn Processor>, meter: Arc<Meter>) -> Timed {
        Timed {
            inner,
            meter,
            child: None,
        }
    }
}

impl Processor for Timed {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn prepare(&mut self, setup: &ProcessSetup) {
        self.meter.set_rate(setup.sample_rate);
        self.inner.prepare(setup);
        self.child = None;
    }

    fn process(&mut self, io: &mut AudioIo<'_>, events: &[NoteEvent]) {
        let frames = io.frames();
        let start = Instant::now();
        self.inner.process(io, events);
        let ns = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let Some((done, took)) = self.inner.child_time() else {
            self.meter.record(frames, ns);
            return;
        };
        // What the host processed since the last block, and how long that took there.
        self.meter.child.store(true, Ordering::Relaxed);
        match self.child {
            Some((f, t)) if done > f && took >= t => {
                self.meter.record((done - f) as usize, took - t);
            }
            Some((f, t)) if done == f && took == t => return,
            // The first look, or counts that started again: from here on.
            _ => {}
        }
        self.child = Some((done, took));
    }

    fn child_time(&self) -> Option<(u64, u64)> {
        self.inner.child_time()
    }

    fn latency(&self) -> u32 {
        self.inner.latency()
    }

    fn timeline_only(&self) -> bool {
        self.inner.timeline_only()
    }

    fn preparing(&self) -> bool {
        self.inner.preparing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn ready() -> Meter {
        let m = Meter::new();
        m.set_rate(RATE);
        m
    }

    #[test]
    fn nothing_processed_reads_as_nothing() {
        let r = ready().read();
        assert_eq!(r.blocks, 0);
        assert_eq!(r.load, 0.0);
    }

    #[test]
    fn the_load_is_the_time_over_the_time_of_the_audio() {
        let m = ready();
        // 480 frames last 10 ms; 1.5 ms of work is 15%.
        for _ in 0..50 {
            m.record(480, 1_500_000);
        }
        let r = m.read();
        assert_eq!(r.blocks, 50);
        assert!((r.load - 0.15).abs() < 1e-9, "{}", r.load);
        assert!((r.mean_us - 1500.0).abs() < 1e-6);
        assert!((r.max_us - 1500.0).abs() < 1e-6);
    }

    #[test]
    fn the_reading_covers_about_the_last_second() {
        let m = ready();
        // 200 slow blocks (a second at 10 ms is 100), then 100 quick ones: only the quick ones
        // are in the window.
        for _ in 0..200 {
            m.record(480, 8_000_000);
        }
        for _ in 0..100 {
            m.record(480, 500_000);
        }
        let r = m.read();
        assert_eq!(r.blocks, 100);
        assert!((r.load - 0.05).abs() < 1e-9, "{}", r.load);
        assert!((r.max_us - 500.0).abs() < 1e-6);
        // The slowest block ever is kept apart.
        assert!((r.peak_us - 8000.0).abs() < 1e-6);
    }

    #[test]
    fn the_ring_keeps_only_its_size_and_the_newest_win() {
        let m = ready();
        for i in 0..(RING as u64 * 3) {
            m.record(16, 1000 + i);
        }
        let r = m.read();
        assert_eq!(r.blocks as usize, RING);
        let newest = 1000 + RING as u64 * 3 - 1;
        assert!((r.max_us - newest as f64 / 1000.0).abs() < 1e-9);
    }

    #[test]
    fn the_slowest_block_is_told_by_its_own_budget() {
        let m = ready();
        m.record(480, 1_000_000);
        m.record(48, 1_000_000);
        let r = m.read();
        // 1 ms of a 1 ms block is the whole deadline.
        assert!((r.max_load - 1.0).abs() < 1e-9);
    }

    struct Burn(u64);
    impl Processor for Burn {
        fn name(&self) -> &str {
            "burn"
        }
        fn prepare(&mut self, _: &ProcessSetup) {}
        fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
            let until = Instant::now() + std::time::Duration::from_micros(self.0);
            while Instant::now() < until {}
            io.left.fill(0.5);
        }
        fn latency(&self) -> u32 {
            7
        }
    }

    /// Waits as the engine does for a plugin in another process, which reports a tenth of a
    /// millisecond of its own per block.
    struct Remote {
        frames: u64,
        ns: u64,
        wait_us: u64,
    }
    impl Processor for Remote {
        fn name(&self) -> &str {
            "remote"
        }
        fn prepare(&mut self, _: &ProcessSetup) {}
        fn process(&mut self, io: &mut AudioIo<'_>, _: &[NoteEvent]) {
            std::thread::sleep(std::time::Duration::from_micros(self.wait_us));
            self.frames += io.frames() as u64;
            self.ns += 100_000;
        }
        fn child_time(&self) -> Option<(u64, u64)> {
            Some((self.frames, self.ns))
        }
    }

    #[test]
    fn a_plugin_in_another_process_reads_as_the_time_it_took_there_not_the_wait() {
        let t = Timings::new();
        let mut p = Timed::new(
            Box::new(Remote {
                frames: 0,
                ns: 0,
                wait_us: 2_000,
            }),
            t.meter(9),
        );
        p.prepare(&ProcessSetup {
            sample_rate: RATE,
            max_block: 512,
            mode: crate::processor::RenderMode::Offline,
        });
        let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
        for _ in 0..5 {
            p.process(
                &mut AudioIo {
                    left: &mut l,
                    right: &mut r,
                },
                &[],
            );
        }
        let read = t.reading(9).unwrap();
        assert!(read.child);
        // The first block only starts the count.
        assert_eq!(read.blocks, 4);
        assert!((read.mean_us - 100.0).abs() < 1e-6, "{}", read.mean_us);
        // 0.1 ms of 10 ms, though each block waited 2 ms.
        assert!((read.load - 0.01).abs() < 1e-9, "{}", read.load);
        assert_eq!(p.child_time(), Some((2400, 500_000)));
    }

    #[test]
    fn a_timed_processor_passes_everything_through_and_records_its_blocks() {
        let t = Timings::new();
        let meter = t.meter(5);
        let mut p = Timed::new(Box::new(Burn(300)), meter);
        let setup = ProcessSetup {
            sample_rate: RATE,
            max_block: 512,
            mode: crate::processor::RenderMode::Offline,
        };
        p.prepare(&setup);
        assert_eq!(p.name(), "burn");
        assert_eq!(p.latency(), 7);
        let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
        for _ in 0..4 {
            p.process(
                &mut AudioIo {
                    left: &mut l,
                    right: &mut r,
                },
                &[],
            );
        }
        assert_eq!(l[0], 0.5);
        let read = t.reading(5).expect("the meter is listed");
        assert_eq!(read.blocks, 4);
        assert!(read.mean_us >= 300.0, "{}", read.mean_us);
        assert!(!read.child);
        assert!(t.reading(6).is_none());
    }

    #[test]
    fn a_new_meter_replaces_the_old_one_of_the_same_plugin_and_retain_forgets() {
        let t = Timings::new();
        let a = t.meter(1);
        a.set_rate(RATE);
        a.record(480, 1000);
        let _b = t.meter(1);
        let _c = t.meter(2);
        assert_eq!(t.read().len(), 2);
        assert_eq!(
            t.reading(1).unwrap().blocks,
            0,
            "the old plan's meter is gone"
        );
        t.retain(|id| id == 2);
        assert_eq!(t.read().len(), 1);
    }
}
