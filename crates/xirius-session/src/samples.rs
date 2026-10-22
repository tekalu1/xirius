//! Stretches of an audio file's own samples (UI-310): what a waveform zoomed in past level 0 of
//! its peaks (a pixel narrower than [`crate::peaks::BASE`] frames) is drawn from.
//!
//! The window never reads the sound itself. [`request`] asks for a [`CHUNK`] of frames, which a
//! thread of this module's own reads, the newest ask first, and [`take`] hands over what it read.
//! WAV, AIFF and FLAC are read from where the chunk starts; MP3 and Ogg Vorbis are decoded from
//! the start of the file up to the chunk, so the frames are the ones [`crate::decode::decode`]
//! gives (and the peaks were made from).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// Frames in a chunk.
pub const CHUNK: u64 = 65_536;
/// Frames merged into one of a chunk's blocks, so a column of many frames reads few values.
pub const BLOCK: usize = 16;
/// Asks kept waiting at most; older ones are dropped (the window asks again for what it still
/// shows).
const QUEUE: usize = 32;

/// Min and max of both channels, in 1/32767 of full scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub min: i16,
    pub max: i16,
}

impl Span {
    fn merge(self, o: Span) -> Span {
        Span {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }

    /// The span in full scale (1.0 is 0 dBFS).
    pub fn scale(self) -> (f32, f32) {
        (f32::from(self.min) / 32767.0, f32::from(self.max) / 32767.0)
    }
}

fn quantise(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// One chunk of a file: each frame's min and max of its two channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Samples {
    /// The file's own rate.
    pub rate: u32,
    /// The first frame.
    pub start: u64,
    pub frames: Vec<Span>,
    /// One span per [`BLOCK`] frames.
    blocks: Vec<Span>,
}

impl Samples {
    pub fn from_channels(rate: u32, start: u64, left: &[f32], right: &[f32]) -> Samples {
        let frames: Vec<Span> = left
            .iter()
            .zip(right)
            .map(|(l, r)| Span {
                min: quantise(l.min(*r)),
                max: quantise(l.max(*r)),
            })
            .collect();
        let blocks = frames
            .chunks(BLOCK)
            .map(|c| c.iter().copied().reduce(Span::merge).unwrap_or_default())
            .collect();
        Samples {
            rate,
            start,
            frames,
            blocks,
        }
    }

    /// One past the last frame.
    pub fn end(&self) -> u64 {
        self.start + self.frames.len() as u64
    }

    /// The span of frame `f`, if the chunk has it.
    pub fn at(&self, f: u64) -> Option<Span> {
        self.frames
            .get(f.checked_sub(self.start)? as usize)
            .copied()
    }

    /// Min and max of the frames `[a, b)` this chunk has (a frame counts when any of it is in
    /// the range); `None` when it has none of them.
    pub fn reduce(&self, a: f64, b: f64) -> Option<Span> {
        let first = (a.floor().max(0.0) as u64).max(self.start);
        let last = (b.ceil().max(0.0) as u64)
            .max(a.floor().max(0.0) as u64 + 1)
            .min(self.end());
        if first >= last {
            return None;
        }
        let (i, j) = ((first - self.start) as usize, (last - self.start) as usize);
        let whole = (i.div_ceil(BLOCK), j / BLOCK);
        let edges = |r: std::ops::Range<usize>| self.frames[r].iter().copied().reduce(Span::merge);
        if whole.0 >= whole.1 {
            return edges(i..j);
        }
        let middle = self.blocks[whole.0..whole.1]
            .iter()
            .copied()
            .reduce(Span::merge);
        [edges(i..whole.0 * BLOCK), middle, edges(whole.1 * BLOCK..j)]
            .into_iter()
            .flatten()
            .reduce(Span::merge)
    }

    /// Bytes the chunk holds.
    pub fn bytes(&self) -> usize {
        (self.frames.len() + self.blocks.len()) * std::mem::size_of::<Span>()
    }
}

/// Whether frames of `path` are read from where they are (the others are decoded from the start).
fn seekable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "wav" | "wave" | "aif" | "aiff" | "aifc" | "flac"
        )
    })
}

/// Reads the frames `[start, start + len)` of `path` at its own rate (fewer at its end).
pub fn read(path: &Path, start: u64, len: u64) -> Result<Samples, String> {
    read_from(path, start, len, seekable(path))
}

/// [`read`], going to `start` first when `seek` (else decoding from the file's start).
fn read_from(path: &Path, start: u64, len: u64, seek: bool) -> Result<Samples, String> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::{SeekMode, SeekTo};
    use symphonia::core::units::Timestamp;

    let mut head = [0u8; 4];
    if std::fs::File::open(path)
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head))
        .is_ok()
        && &head == b"RF64"
    {
        // Takes past 4 GiB are drawn from their peaks only.
        return Err(format!("{} は途中から読めません", path.display()));
    }
    let (mut format, id, params, _) = crate::decode::open(path)?;
    let rate = params.sample_rate.unwrap_or(0);
    let frames_tb = format
        .tracks()
        .iter()
        .find(|t| t.id == id)
        .and_then(|t| t.time_base)
        .is_some_and(|tb| tb.numer.get() == 1 && tb.denom.get() == rate);
    // Where a packet starts: its time stamp when the reader went there, else the frames before.
    let seeked = start > 0
        && seek
        && frames_tb
        && format
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: Timestamp::new(start as i64),
                    track_id: id,
                },
            )
            .is_ok();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
        .map_err(|_| format!("{} をデコードできません", path.display()))?;
    let end = start + len;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    let mut at: u64 = 0;
    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut rate_seen = rate;
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(format!("{} を読めません: {e}", path.display())),
        };
        if packet.track_id != id {
            continue;
        }
        if seeked {
            at = packet.pts.get().max(0) as u64;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                if buf.spec().rate() != 0 {
                    rate_seen = buf.spec().rate();
                }
                buf.copy_to_vecs_planar::<f32>(&mut planes);
                let (l, r) = match planes.as_slice() {
                    [mono] => (mono, mono),
                    [l, r, ..] => (l, r),
                    [] => continue,
                };
                let n = l.len() as u64;
                if at + n > start && at < end {
                    let from = start.saturating_sub(at) as usize;
                    let to = (end - at).min(n) as usize;
                    left.extend_from_slice(&l[from..to]);
                    right.extend_from_slice(&r[from..to]);
                }
                at += n;
                if at >= end {
                    break;
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("{} をデコードできません: {e}", path.display())),
        }
    }
    if rate_seen == 0 {
        return Err(format!("{} のサンプルレートが分かりません", path.display()));
    }
    Ok(Samples::from_channels(rate_seen, start, &left, &right))
}

// ---- in the background -------------------------------------------------------------------

/// What a background read left, by the file and the chunk's index.
pub type Got = Result<Arc<Samples>, String>;

#[derive(Default)]
struct Queue {
    /// The newest ask at the back.
    wanted: VecDeque<(PathBuf, u64)>,
    running: Option<(PathBuf, u64)>,
    done: HashMap<(PathBuf, u64), Got>,
    worker: bool,
}

fn queue() -> &'static Mutex<Queue> {
    static QUEUE_: OnceLock<Mutex<Queue>> = OnceLock::new();
    QUEUE_.get_or_init(Default::default)
}

fn idle() -> &'static Condvar {
    static IDLE: Condvar = Condvar::new();
    &IDLE
}

static FINISHED: AtomicU64 = AtomicU64::new(0);

/// Counts the chunks read so far; a window redraws when it moves.
pub fn finished() -> u64 {
    FINISHED.load(Ordering::Acquire)
}

/// Asks for chunk `index` of `path` (its frames from `index * CHUNK`). An ask already waiting
/// moves to the front; one being read or read is not asked again.
pub fn request(path: &Path, index: u64) {
    let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
    let key = (path.to_path_buf(), index);
    if q.running.as_ref() == Some(&key) || q.done.contains_key(&key) {
        return;
    }
    if let Some(i) = q.wanted.iter().position(|k| *k == key) {
        q.wanted.remove(i);
    }
    q.wanted.push_back(key);
    while q.wanted.len() > QUEUE {
        q.wanted.pop_front();
    }
    if q.worker {
        return;
    }
    q.worker = true;
    let spawned = std::thread::Builder::new()
        .name("xirius-samples".into())
        .spawn(work);
    if spawned.is_err() {
        q.worker = false;
        q.wanted.clear();
    }
}

/// Reads the asks, newest first, until there are none.
fn work() {
    loop {
        let key = {
            let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
            let Some(key) = q.wanted.pop_back() else {
                q.worker = false;
                drop(q);
                idle().notify_all();
                return;
            };
            q.running = Some(key.clone());
            key
        };
        let (path, index) = &key;
        let got = std::panic::catch_unwind(|| read(path, index * CHUNK, CHUNK))
            .unwrap_or_else(|_| Err(format!("{} を読めません", path.display())))
            .map(Arc::new);
        let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
        q.running = None;
        q.done.insert(key, got);
        FINISHED.fetch_add(1, Ordering::AcqRel);
    }
}

/// The chunks read since the last call.
pub fn take() -> Vec<((PathBuf, u64), Got)> {
    queue()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .done
        .drain()
        .collect()
}

/// Waits until every ask is read (a test, before it looks).
pub fn wait() {
    let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
    while q.worker {
        q = idle().wait(q).unwrap_or_else(|e| e.into_inner());
    }
}

#[cfg(test)]
mod tests;
