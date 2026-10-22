//! Copies the master output to a WAV file on a writer thread (ENG-017, ENG-020).
//!
//! The audio thread pushes samples into a ring buffer and never waits; when the ring is full the
//! samples are dropped and counted in [`crate::stats::Stats`].

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Consumer, RingBuffer};

use crate::engine::TapProducer;

pub struct WavTap {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<u64, hound::Error>>>,
}

impl WavTap {
    /// Starts the writer. Give the returned producer to [`crate::AudioEngine::set_tap`].
    /// `seconds` sizes the ring: how long the writer may stall before samples are dropped.
    pub fn start(
        path: impl Into<PathBuf>,
        sample_rate: u32,
        seconds: f64,
    ) -> Result<(WavTap, TapProducer), hound::Error> {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let writer = hound::WavWriter::create(path.into(), spec)?;
        let capacity = ((sample_rate as f64 * seconds) as usize * 2).max(4096);
        let (tx, rx) = RingBuffer::new(capacity);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("xirius-wav-tap".into())
            .spawn(move || write_loop(writer, rx, &stop2))
            .map_err(hound::Error::IoError)?;
        Ok((
            WavTap {
                stop,
                thread: Some(thread),
            },
            tx,
        ))
    }

    /// Writes what is left in the ring and closes the file. Returns the samples written.
    pub fn finish(mut self) -> Result<u64, hound::Error> {
        self.stop.store(true, Ordering::Release);
        match self.thread.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err(hound::Error::IoError(std::io::Error::other(
                "the WAV writer thread panicked",
            ))),
            None => Ok(0),
        }
    }
}

impl Drop for WavTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn write_loop(
    mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    mut rx: Consumer<f32>,
    stop: &AtomicBool,
) -> Result<u64, hound::Error> {
    let mut written = 0u64;
    loop {
        // Read the flag before draining so samples pushed before `finish` are not lost.
        let stopping = stop.load(Ordering::Acquire);
        let n = rx.slots();
        if n > 0 {
            let chunk = rx.read_chunk(n).expect("slots() were available");
            let (a, b) = chunk.as_slices();
            for &s in a.iter().chain(b) {
                writer.write_sample(s)?;
            }
            chunk.commit_all();
            written += n as u64;
        } else if stopping {
            break;
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    writer.finalize()?;
    Ok(written)
}
