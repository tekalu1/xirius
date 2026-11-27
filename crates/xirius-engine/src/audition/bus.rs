//! A bounded, device-only preview bus (BRW-020). Only the feeder locks; the audio
//! callback reads plain frames from an SPSC queue and publishes progress atomically.

use crate::rt_lock::Mutex;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Copy)]
struct Frame {
    serial: u64,
    pair: [f32; 2],
}

#[derive(Default)]
struct State {
    alive: AtomicBool,
    wanted: AtomicU64,
    seen: AtomicU64,
    played: AtomicU64,
}

/// The control side, shared with the file reader's voice thread. Samples are at
/// `rate`; the queue holds at most 8192 stereo frames, regardless of file size.
#[derive(Clone)]
pub struct Port {
    tx: Arc<Mutex<Producer<Frame>>>,
    state: Arc<State>,
    pub rate: u32,
}

pub(crate) struct Bus {
    rx: Consumer<Frame>,
    state: Arc<State>,
    serial: u64,
    played: u64,
}

pub(crate) fn create(rate: u32) -> (Bus, Port) {
    let (tx, rx) = RingBuffer::new(8192);
    let state = Arc::new(State::default());
    state.alive.store(true, Ordering::Release);
    (
        Bus {
            rx,
            state: state.clone(),
            serial: 0,
            played: 0,
        },
        Port {
            tx: Arc::new(Mutex::new(tx)),
            state,
            rate,
        },
    )
}

impl Port {
    pub fn alive(&self) -> bool {
        self.state.alive.load(Ordering::Acquire)
    }
    pub fn start(&self, serial: u64) {
        if self.alive() {
            self.state.wanted.store(serial, Ordering::Release);
        }
    }

    /// Detach immediately; stopping and dropping the old device may finish later.
    pub fn close(&self) {
        self.state.alive.store(false, Ordering::Release);
        self.state.wanted.store(0, Ordering::Release);
    }

    pub fn stop(&self) {
        self.start(0);
    }

    /// Feeds as much as fits; never waits for the callback to consume it.
    pub fn feed(&self, serial: u64, left: &[f32], right: &[f32]) -> usize {
        if !self.alive() {
            return 0;
        }
        let mut tx = self.tx.lock().unwrap();
        let n = tx.slots().min(left.len()).min(right.len());
        for i in 0..n {
            let _ = tx.push(Frame {
                serial,
                pair: [left[i], right[i]],
            });
        }
        n
    }

    pub fn played(&self, serial: u64) -> Option<u64> {
        (self.state.seen.load(Ordering::Acquire) == serial)
            .then(|| self.state.played.load(Ordering::Acquire))
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        self.state.alive.store(false, Ordering::Release);
        self.stop();
    }
}

impl Bus {
    pub fn stop(&self) {
        self.state.wanted.store(0, Ordering::Release);
    }
    pub fn next_frame(&mut self) -> [f32; 2] {
        let wanted = self.wanted();
        self.pop(wanted)
    }

    fn wanted(&self) -> u64 {
        if self.state.alive.load(Ordering::Acquire) {
            self.state.wanted.load(Ordering::Acquire)
        } else {
            0
        }
    }

    fn follow(&mut self, wanted: u64) {
        if wanted != self.serial {
            self.serial = wanted;
            self.played = 0;
            self.state.played.store(0, Ordering::Release);
            self.state.seen.store(wanted, Ordering::Release);
        }
    }

    /// `wanted` was read before popping, so a cue may have started and fed since.
    fn pop(&mut self, wanted: u64) -> [f32; 2] {
        self.follow(wanted);
        while let Ok(frame) = self.rx.pop() {
            if frame.serial != self.serial {
                // `start` is stored before the feed, so a frame's own serial is wanted by now
                // unless a later start or stop replaced it: only then is the frame stale.
                self.follow(self.wanted());
            }
            if self.serial != 0 && frame.serial == self.serial {
                self.played += 1;
                self.state.played.store(self.played, Ordering::Release);
                return frame.pair;
            }
        }
        [0.0; 2]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn detach_silences_a_live_bus_and_rejects_old_feeders_before_device_drop() {
        let (mut bus, port) = super::create(48_000);
        port.start(1);
        assert_eq!(port.feed(1, &[0.5, 0.5], &[0.5, 0.5]), 2);
        assert_eq!(bus.next_frame(), [0.5; 2]);
        port.close();
        port.start(2);
        assert!(!port.alive());
        assert_eq!(port.feed(2, &[1.0], &[1.0]), 0);
        assert_eq!(bus.next_frame(), [0.0; 2]);
    }

    #[test]
    fn a_cue_fed_after_the_callback_read_the_stopped_serial_loses_no_frame() {
        let (mut bus, port) = super::create(48_000);
        port.start(1);
        assert_eq!(port.feed(1, &[0.25], &[0.25]), 1);
        assert_eq!(bus.next_frame(), [0.25; 2]);
        port.stop();
        // The callback reads the stopped serial; the next cue starts and feeds before it pops.
        let read = bus.wanted();
        port.start(2);
        assert_eq!(port.feed(2, &[0.5, 0.75], &[0.5, 0.75]), 2);
        assert_eq!(bus.pop(read), [0.5; 2]);
        assert_eq!(bus.next_frame(), [0.75; 2]);
        assert_eq!(port.played(2), Some(2));
    }
}
