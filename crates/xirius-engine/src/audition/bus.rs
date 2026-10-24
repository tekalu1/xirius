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
        self.state.wanted.store(serial, Ordering::Release);
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
        let wanted = self.state.wanted.load(Ordering::Acquire);
        if wanted != self.serial {
            self.serial = wanted;
            self.played = 0;
            self.state.played.store(0, Ordering::Release);
            self.state.seen.store(wanted, Ordering::Release);
        }
        while let Ok(frame) = self.rx.pop() {
            if wanted != 0 && frame.serial == wanted {
                self.played += 1;
                self.state.played.store(self.played, Ordering::Release);
                return frame.pair;
            }
        }
        [0.0; 2]
    }
}
