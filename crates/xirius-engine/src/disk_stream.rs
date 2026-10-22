//! Native-rate WAV playback through a fixed page ring (ENG-021). The control side opens
//! the header; only the reader worker seeks and reads samples. Realtime readers never wait.
use std::cell::UnsafeCell;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const PAGE: usize = 8192;
const PAGES: usize = 8;

struct Data {
    at: u64,
    len: usize,
    left: Box<[f32; PAGE]>,
    right: Box<[f32; PAGE]>,
}
struct Slot {
    state: AtomicU8,
    at: AtomicU64,
    data: UnsafeCell<Data>,
}
struct Reading<'a>(&'a Slot);
impl Drop for Reading<'_> {
    fn drop(&mut self) {
        self.0.state.store(2, Ordering::Release);
    }
}
// SAFETY: 0/2 -> 1 grants the writer exclusive access; 2 -> 3 grants the reader exclusive
// access. Release to 2 publishes samples, and readers never touch a slot being written.
unsafe impl Sync for Slot {}
struct Shared {
    slots: Vec<Slot>,
    wanted: AtomicU64,
    stop: AtomicBool,
    reads: AtomicU64,
    misses: AtomicU64,
    failed: AtomicBool,
}

pub struct DiskAudio {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    frames: u64,
}
impl std::fmt::Debug for DiskAudio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskAudio")
            .field("frames", &self.frames)
            .finish()
    }
}

impl DiskAudio {
    /// Returns None for formats/rates that need the session decoder's conversion.
    pub fn open(path: &Path, rate: u32) -> Result<Option<Self>, String> {
        let Ok(mut reader) = hound::WavReader::open(path) else {
            return Ok(None);
        };
        let spec = reader.spec();
        if spec.sample_rate != rate || !(1..=2).contains(&spec.channels) {
            return Ok(None);
        }
        let frames = u64::from(reader.duration());
        let shared = Arc::new(Shared {
            slots: (0..PAGES)
                .map(|_| Slot {
                    state: AtomicU8::new(0),
                    at: AtomicU64::new(u64::MAX),
                    data: UnsafeCell::new(Data {
                        at: 0,
                        len: 0,
                        left: Box::new([0.0; PAGE]),
                        right: Box::new([0.0; PAGE]),
                    }),
                })
                .collect(),
            wanted: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            reads: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            failed: AtomicBool::new(false),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("xirius-disk-reader".into())
            .spawn(move || {
                while !worker.stop.load(Ordering::Acquire) {
                    let first = worker.wanted.load(Ordering::Acquire) / PAGE as u64;
                    let mut wrote = false;
                    for page in first..first + PAGES as u64 {
                        let at = page * PAGE as u64;
                        if at >= frames {
                            break;
                        }
                        let slot = &worker.slots[page as usize % PAGES];
                        let state = slot.state.load(Ordering::Acquire);
                        if state == 2 && slot.at.load(Ordering::Relaxed) == at {
                            continue;
                        }
                        if state != 0 && state != 2 {
                            continue;
                        }
                        if slot
                            .state
                            .compare_exchange(state, 1, Ordering::AcqRel, Ordering::Relaxed)
                            .is_err()
                        {
                            continue;
                        }
                        // SAFETY: state 1 exclusively owns this slot until published below.
                        let data = unsafe { &mut *slot.data.get() };
                        if state == 2 && data.at == at {
                            slot.state.store(2, Ordering::Release);
                            continue;
                        }
                        data.at = at;
                        data.len = (frames - at).min(PAGE as u64) as usize;
                        data.left.fill(0.0);
                        data.right.fill(0.0);
                        if reader.seek(at as u32).is_err() {
                            worker.failed.store(true, Ordering::Relaxed);
                        } else {
                            for i in 0..data.len {
                                let mut sample = || {
                                    let result = match spec.sample_format {
                                        hound::SampleFormat::Float => reader
                                            .samples::<f32>()
                                            .next()
                                            .unwrap_or(Err(hound::Error::FormatError("short WAV"))),
                                        hound::SampleFormat::Int => reader
                                            .samples::<i32>()
                                            .next()
                                            .unwrap_or(Err(hound::Error::FormatError("short WAV")))
                                            .map(|x| {
                                                x as f32
                                                    / (1u64 << (spec.bits_per_sample - 1)) as f32
                                            }),
                                    };
                                    match result {
                                        Ok(v) if v.is_finite() => v,
                                        _ => {
                                            worker.failed.store(true, Ordering::Relaxed);
                                            0.0
                                        }
                                    }
                                };
                                data.left[i] = sample();
                                data.right[i] = if spec.channels == 1 {
                                    data.left[i]
                                } else {
                                    sample()
                                };
                            }
                            worker.reads.fetch_add(1, Ordering::Relaxed);
                        }
                        slot.at.store(at, Ordering::Relaxed);
                        slot.state.store(2, Ordering::Release);
                        wrote = true;
                        if worker.wanted.load(Ordering::Acquire) / PAGE as u64 != first {
                            break;
                        }
                    }
                    if !wrote {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Some(Self {
            shared,
            thread: Some(thread),
            frames,
        }))
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }
    pub fn disk_reads(&self) -> u64 {
        self.shared.reads.load(Ordering::Relaxed)
    }
    pub fn missed_frames(&self) -> u64 {
        self.shared.misses.load(Ordering::Relaxed)
    }
    pub fn failed(&self) -> bool {
        self.shared.failed.load(Ordering::Relaxed)
    }

    /// Primes the first page before playback starts, on the control thread.
    pub fn prime(&self, frame: u64) {
        self.mix(frame, 1, true, |_, _, _| {});
    }

    pub fn mix(
        &self,
        mut frame: u64,
        frames: usize,
        offline: bool,
        mut sample: impl FnMut(usize, f32, f32),
    ) {
        let mut done = 0;
        self.shared.wanted.store(frame, Ordering::Release);
        while done < frames && frame < self.frames {
            let page = frame / PAGE as u64;
            let at = page * PAGE as u64;
            let offset = (frame - at) as usize;
            let n = (PAGE - offset)
                .min(frames - done)
                .min((self.frames - frame) as usize);
            let slot = &self.shared.slots[page as usize % PAGES];
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut copied = false;
            loop {
                if slot
                    .state
                    .compare_exchange(2, 3, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    let _reading = Reading(slot);
                    // SAFETY: state 3 excludes the writer; Release below returns the page.
                    let data = unsafe { &*slot.data.get() };
                    if data.at == at && offset + n <= data.len {
                        for i in 0..n {
                            sample(done + i, data.left[offset + i], data.right[offset + i]);
                        }
                        copied = true;
                    }
                }
                if copied || !offline || Instant::now() >= deadline {
                    break;
                }
                std::thread::yield_now();
            }
            if !copied {
                self.shared.misses.fetch_add(n as u64, Ordering::Relaxed);
            }
            frame += n as u64;
            done += n;
            self.shared.wanted.store(frame, Ordering::Release);
        }
    }
}

impl Drop for DiskAudio {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
