//! The sound of audio clips played at another tempo, speed or pitch, or backwards (EDT-007,
//! EDT-016, LIC-013).
//!
//! The source file is never rewritten. The clip's sound is rendered from it with
//! [`xirius_stretch`] on a worker thread and kept here, one entry per file, rate and setting,
//! shared by every song in the process. Until it is ready the clip is left off the timeline
//! (it plays silent), and [`generation`] moves on so the host sends the timeline again
//! ([`crate::host`]). Nothing here runs on the audio thread. A render for the export or a
//! bounce is done on the caller's thread instead ([`get`] with `wait`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::SystemTime;

use xirius_core::model::{AudioProcess, Clip, ClipBody, Project, TrackKind};
use xirius_core::tempo::TempoMap;
use xirius_engine::AudioData;

/// How many renders run at once.
const WORKERS: usize = 2;
/// Ready sounds not wanted lately are let go above this many bytes.
const KEEP_BYTES: usize = 512 * 1024 * 1024;

/// What a clip's sound is rendered from and how.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: PathBuf,
    pub rate: u32,
    /// Output seconds per source second ([`AudioProcess::ratio`]), as bits.
    ratio: u64,
    /// The pitch multiplier ([`AudioProcess::transpose`]), as bits.
    transpose: u64,
    pub reverse: bool,
}

impl Key {
    /// The render a clip with `process` needs where the song plays at `song_bpm`; `None` when
    /// it plays its source as it is.
    pub fn of(path: &Path, rate: u32, process: &AudioProcess, song_bpm: f64) -> Option<Key> {
        let ratio = process.ratio(song_bpm);
        let transpose = process.transpose();
        // Rounded so that the same setting reached by different sums is one entry.
        let round = |v: f64| (v * 1e9).round() / 1e9;
        let (ratio, transpose) = (round(ratio), round(transpose));
        if ratio == 1.0 && transpose == 1.0 && !process.reverse {
            return None;
        }
        Some(Key {
            path: path.to_path_buf(),
            rate,
            ratio: ratio.to_bits(),
            transpose: transpose.to_bits(),
            reverse: process.reverse,
        })
    }

    pub fn ratio(&self) -> f64 {
        f64::from_bits(self.ratio)
    }

    pub fn transpose(&self) -> f64 {
        f64::from_bits(self.transpose)
    }

    /// Only played backwards: quick enough to do at once.
    fn quick(&self) -> bool {
        self.ratio() == 1.0 && self.transpose() == 1.0
    }
}

/// The render `clip` plays from, for a song in `dir` at `rate` (as the builder works it out).
pub fn clip_key(clip: &Clip, tmap: &TempoMap, rate: u32, dir: Option<&Path>) -> Option<Key> {
    let ClipBody::Audio { file, process, .. } = &clip.body else {
        return None;
    };
    if process.is_plain() {
        return None;
    }
    let path = Path::new(file);
    let path = match dir {
        Some(d) if !path.is_absolute() => d.join(path),
        _ => path.to_path_buf(),
    };
    Key::of(&path, rate, process, tmap.bpm_at(clip.start))
}

/// The renders the unmuted audio clips of `p` play from.
pub fn keys_of(p: &Project, tmap: &TempoMap, rate: u32, dir: Option<&Path>) -> Vec<Key> {
    p.tracks
        .values()
        .filter(|t| t.kind == TrackKind::Audio)
        .flat_map(|t| &t.clips)
        .filter(|c| !c.muted)
        .filter_map(|c| clip_key(c, tmap, rate, dir))
        .collect()
}

/// Where a clip's sound is.
#[derive(Clone, Debug)]
pub enum State {
    Ready(Arc<AudioData>),
    /// Being rendered; how far (0..=1).
    Working(f32),
    /// The sentence for the user.
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Option<Stamp> {
        std::fs::metadata(path).ok().map(|m| Stamp {
            len: m.len(),
            modified: m.modified().ok(),
        })
    }
}

enum Slot {
    Working {
        done: Arc<AtomicU32>,
        cancel: Arc<AtomicBool>,
        /// The builder that asked ([`sweep`] stops only its own).
        owner: u64,
    },
    Ready(Arc<AudioData>),
    Failed(String),
}

struct Entry {
    stamp: Option<Stamp>,
    slot: Slot,
    /// When it was last asked for (a count, not a time).
    used: u64,
}

struct Job {
    key: Key,
    source: Arc<AudioData>,
    done: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
struct Cache {
    map: HashMap<Key, Entry>,
    queue: VecDeque<Job>,
    workers: usize,
    clock: u64,
}

fn cache() -> &'static (Mutex<Cache>, Condvar) {
    static CACHE: OnceLock<(Mutex<Cache>, Condvar)> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, Cache> {
    cache().0.lock().unwrap_or_else(|e| e.into_inner())
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static OWNERS: AtomicU64 = AtomicU64::new(1);

/// Moves on each time a render finishes or fails: the timelines that left a clip out are
/// built again.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// A new owner for [`get`] and [`sweep`].
pub fn owner() -> u64 {
    OWNERS.fetch_add(1, Ordering::Relaxed)
}

/// The sound for `key`. `source` reads the file at the key's rate; it is called only when a
/// render has to start. With `wait` the render is done here and the answer is never
/// [`State::Working`]; without, it is started on a worker.
pub fn get(
    key: &Key,
    owner: u64,
    wait: bool,
    source: impl FnOnce() -> Result<Arc<AudioData>, String>,
) -> State {
    let stamp = Stamp::of(&key.path);
    {
        let mut c = lock();
        c.clock += 1;
        let now = c.clock;
        if let Some(e) = c.map.get_mut(key)
            && e.stamp == stamp
        {
            e.used = now;
            match &e.slot {
                Slot::Ready(d) => return State::Ready(d.clone()),
                Slot::Failed(why) => return State::Failed(why.clone()),
                Slot::Working { done, .. } if !wait => {
                    return State::Working(permille(done));
                }
                Slot::Working { .. } => {}
            }
        }
    }
    let source = match source() {
        Ok(s) => s,
        Err(e) => {
            store(key, stamp, Slot::Failed(e.clone()));
            return State::Failed(e);
        }
    };
    if wait || key.quick() {
        let slot = match render(key, &source, &AtomicU32::new(0), &AtomicBool::new(false)) {
            Ok(d) => Slot::Ready(Arc::new(d)),
            Err(e) => Slot::Failed(e),
        };
        let state = match &slot {
            Slot::Ready(d) => State::Ready(d.clone()),
            Slot::Failed(e) => State::Failed(e.clone()),
            Slot::Working { .. } => unreachable!(),
        };
        store(key, stamp, slot);
        return state;
    }
    let done = Arc::new(AtomicU32::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut c = lock();
    if let Some(Entry {
        slot: Slot::Working { cancel, .. },
        ..
    }) = c.map.get(key)
    {
        cancel.store(true, Ordering::Relaxed);
    }
    c.clock += 1;
    let used = c.clock;
    c.map.insert(
        key.clone(),
        Entry {
            stamp,
            slot: Slot::Working {
                done: done.clone(),
                cancel: cancel.clone(),
                owner,
            },
            used,
        },
    );
    c.queue.push_back(Job {
        key: key.clone(),
        source,
        done,
        cancel,
    });
    if c.workers < WORKERS {
        c.workers += 1;
        let spawned = std::thread::Builder::new()
            .name("xirius-stretch".into())
            .spawn(work);
        if spawned.is_err() {
            c.workers -= 1;
        }
    }
    cache().1.notify_one();
    State::Working(0.0)
}

/// Where the sound for `key` is, without starting anything (for drawing the clip).
pub fn state(key: &Key) -> Option<State> {
    let c = lock();
    c.map.get(key).map(|e| match &e.slot {
        Slot::Ready(d) => State::Ready(d.clone()),
        Slot::Working { done, .. } => State::Working(permille(done)),
        Slot::Failed(why) => State::Failed(why.clone()),
    })
}

/// Forgets a failure, so the next build tries again.
pub fn retry(key: &Key) {
    let mut c = lock();
    if matches!(
        c.map.get(key),
        Some(Entry {
            slot: Slot::Failed(_),
            ..
        })
    ) {
        c.map.remove(key);
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// After `owner` built its timelines from `wanted`: stops its renders nobody wants any more,
/// and lets go of old sounds while the kept ones are over [`KEEP_BYTES`].
pub fn sweep(owner: u64, wanted: &[Key]) {
    let mut c = lock();
    let stale: Vec<Key> = c
        .map
        .iter()
        .filter(|(k, e)| {
            matches!(&e.slot, Slot::Working { owner: o, .. } if *o == owner) && !wanted.contains(k)
        })
        .map(|(k, _)| k.clone())
        .collect();
    for k in &stale {
        if let Some(Entry {
            slot: Slot::Working { cancel, .. },
            ..
        }) = c.map.remove(k)
        {
            cancel.store(true, Ordering::Relaxed);
        }
    }
    c.queue.retain(|j| !j.cancel.load(Ordering::Relaxed));
    let bytes = |e: &Entry| match &e.slot {
        Slot::Ready(d) => (d.left.len() + d.right.len()) * 4,
        _ => 0,
    };
    let mut total: usize = c.map.values().map(bytes).sum();
    if total <= KEEP_BYTES {
        return;
    }
    let mut old: Vec<(u64, Key, usize)> = c
        .map
        .iter()
        .filter(|(k, e)| bytes(e) > 0 && !wanted.contains(k))
        .map(|(k, e)| (e.used, k.clone(), bytes(e)))
        .collect();
    old.sort_by_key(|(used, _, _)| *used);
    for (_, k, n) in old {
        if total <= KEEP_BYTES {
            break;
        }
        c.map.remove(&k);
        total -= n;
    }
}

fn permille(done: &AtomicU32) -> f32 {
    done.load(Ordering::Relaxed) as f32 / 1000.0
}

fn store(key: &Key, stamp: Option<Stamp>, slot: Slot) {
    let mut c = lock();
    c.clock += 1;
    let used = c.clock;
    c.map.insert(key.clone(), Entry { stamp, slot, used });
}

fn work() {
    loop {
        let job = {
            let (m, cv) = cache();
            let mut c = m.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(j) = c.queue.pop_front() {
                    break j;
                }
                c = cv.wait(c).unwrap_or_else(|e| e.into_inner());
            }
        };
        if job.cancel.load(Ordering::Relaxed) {
            continue;
        }
        let out = render(&job.key, &job.source, &job.done, &job.cancel);
        let mut c = lock();
        // Only if this job is still the one the entry waits for.
        let current = matches!(
            c.map.get(&job.key),
            Some(Entry { slot: Slot::Working { cancel, .. }, .. }) if Arc::ptr_eq(cancel, &job.cancel)
        );
        if !current || job.cancel.load(Ordering::Relaxed) {
            continue;
        }
        if let Some(e) = c.map.get_mut(&job.key) {
            e.slot = match out {
                Ok(d) => Slot::Ready(Arc::new(d)),
                Err(why) => Slot::Failed(why),
            };
        }
        drop(c);
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Renders the clip's sound from the whole source.
fn render(
    key: &Key,
    source: &AudioData,
    done: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<AudioData, String> {
    let frames = source.frames();
    let mut left = source.left[..frames].to_vec();
    let mut right = source.right[..frames].to_vec();
    if key.reverse {
        left.reverse();
        right.reverse();
    }
    if key.quick() {
        return Ok(AudioData { left, right });
    }
    let out_len = (frames as f64 * key.ratio()).round().max(1.0) as usize;
    let transpose = key.transpose();
    let rate = key.rate as f32;
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        xirius_stretch::render(&[&left, &right], rate, out_len, transpose, &mut |d| {
            done.store((d.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
            !cancel.load(Ordering::Relaxed)
        })
    }));
    match run {
        Ok(Ok(mut out)) => {
            let right = out.pop().unwrap_or_default();
            let left = out.pop().unwrap_or_default();
            Ok(AudioData { left, right })
        }
        Ok(Err(xirius_stretch::Cancelled)) => Err("伸縮を止めました".into()),
        Err(_) => Err(format!("{} を伸縮できません", key.path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(reverse: bool, speed: f64) -> Key {
        let process = AudioProcess {
            speed,
            reverse,
            ..Default::default()
        };
        Key::of(Path::new("no-such-file.wav"), 48_000, &process, 120.0).unwrap()
    }

    fn source(n: usize) -> Arc<AudioData> {
        let left: Vec<f32> = (0..n).map(|i| i as f32).collect();
        Arc::new(AudioData {
            right: left.clone(),
            left,
        })
    }

    #[test]
    fn a_plain_clip_needs_nothing() {
        let p = AudioProcess::default();
        assert!(Key::of(Path::new("a.wav"), 48_000, &p, 120.0).is_none());
        // Following the song at its own tempo is plain too.
        let p = AudioProcess {
            tempo: Some(120.0),
            follow: true,
            ..Default::default()
        };
        assert!(Key::of(Path::new("a.wav"), 48_000, &p, 120.0).is_none());
        assert!(Key::of(Path::new("a.wav"), 48_000, &p, 90.0).is_some());
    }

    #[test]
    fn reversing_alone_is_done_at_once() {
        let k = key(true, 100.0);
        let State::Ready(d) = get(&k, owner(), false, || Ok(source(4))) else {
            panic!("not ready");
        };
        assert_eq!(d.left, vec![3.0, 2.0, 1.0, 0.0]);
    }

    #[test]
    fn a_stretch_works_in_the_background_and_then_is_ready() {
        let k = key(false, 50.0);
        let me = owner();
        let before = generation();
        let first = get(&k, me, false, || Ok(source(48_000)));
        assert!(matches!(first, State::Working(_)), "{first:?}");
        let t = std::time::Instant::now();
        while !matches!(state(&k), Some(State::Ready(_))) {
            assert!(t.elapsed().as_secs() < 30, "never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(generation() > before);
        let State::Ready(d) = get(&k, me, false, || panic!("read again")) else {
            panic!("not ready");
        };
        assert_eq!(d.frames(), 96_000);
    }

    #[test]
    fn a_source_that_cannot_be_read_fails_and_can_be_tried_again() {
        let k = key(false, 75.0);
        let s = get(&k, owner(), false, || Err("読めません".into()));
        assert!(matches!(s, State::Failed(ref e) if e == "読めません"));
        assert!(matches!(state(&k), Some(State::Failed(_))));
        retry(&k);
        assert!(state(&k).is_none());
    }

    #[test]
    fn a_render_nobody_wants_is_stopped() {
        let k = key(false, 60.0);
        let me = owner();
        let _ = get(&k, me, false, || Ok(source(48_000 * 30)));
        sweep(me, &[]);
        assert!(state(&k).is_none());
    }
}
