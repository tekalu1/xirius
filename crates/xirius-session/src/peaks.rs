//! Peak files of audio files (UI-037): the min/max of every run of samples, at several
//! coarsenesses, so a waveform is drawn at any zoom without reading the sound.
//!
//! A file's peaks are made once, off the window's thread ([`request`]), and kept in the song
//! folder's `Peaks/` (or, for a song not saved yet, in a cache folder). The peak file is named
//! after the audio file and its size and time, so an audio file that changes gets new peaks.
//! Level 0 has one peak per [`BASE`] frames of the file's own rate; each level after it merges
//! [`STEP`] peaks of the one before.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Frames per peak of level 0.
pub const BASE: u32 = 128;
/// Peaks of a level merged into one of the next.
pub const STEP: usize = 4;
/// The folder of the song the peak files are kept in.
pub const FOLDER: &str = "Peaks";

const MAGIC: &[u8; 4] = b"XPK1";

/// Min and max of a run of samples, both channels together, in 1/127 of full scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Peak {
    pub min: i8,
    pub max: i8,
}

impl Peak {
    /// The peak of one run of samples of both channels: a run of level 0 is [`BASE`] frames.
    pub fn of(left: &[f32], right: &[f32]) -> Peak {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for v in left.iter().chain(right) {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        Peak {
            min: quantise(lo),
            max: quantise(hi),
        }
    }

    fn merge(self, o: Peak) -> Peak {
        Peak {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
}

/// The peaks of one audio file.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    /// The file's own rate.
    pub rate: u32,
    /// The file's length in frames.
    pub frames: u64,
    /// `levels[k]` has one peak per `BASE * STEP^k` frames.
    pub levels: Vec<Vec<Peak>>,
}

fn quantise(x: f32) -> i8 {
    (x.clamp(-1.0, 1.0) * 127.0).round() as i8
}

impl Peaks {
    /// The peaks of a decoded stereo signal.
    pub fn from_samples(rate: u32, left: &[f32], right: &[f32]) -> Peaks {
        let base: Vec<Peak> = left
            .chunks(BASE as usize)
            .zip(right.chunks(BASE as usize))
            .map(|(l, r)| Peak::of(l, r))
            .collect();
        Peaks::from_base(rate, left.len() as u64, base)
    }

    /// The peaks of `frames` frames whose level 0 is `base`: the levels after it are merged
    /// from it. A file read bit by bit adds to `base` and makes the rest again.
    pub fn from_base(rate: u32, frames: u64, base: Vec<Peak>) -> Peaks {
        let mut levels = vec![base];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let next = levels
                .last()
                .unwrap_or(&Vec::new())
                .chunks(STEP)
                .map(|c| c.iter().copied().reduce(Peak::merge).unwrap_or_default())
                .collect();
            levels.push(next);
        }
        Peaks {
            rate,
            frames,
            levels,
        }
    }

    /// Frames per peak at `level`.
    pub fn level_frames(level: usize) -> f64 {
        f64::from(BASE) * (STEP as f64).powi(level as i32)
    }

    /// The coarsest level with at least one peak for each `frames_per_px` frames: what a zoom
    /// that shows that many frames in a pixel reads.
    pub fn level_for(&self, frames_per_px: f64) -> usize {
        let mut level = 0;
        while level + 1 < self.levels.len() && Self::level_frames(level + 1) <= frames_per_px {
            level += 1;
        }
        level
    }

    /// Min and max of the frames `[start, end)` at `level`; `None` outside the file.
    pub fn reduce(&self, level: usize, start: f64, end: f64) -> Option<Peak> {
        let peaks = self.levels.get(level)?;
        let n = Self::level_frames(level);
        let first = (start / n).floor().max(0.0) as usize;
        let last = ((end / n).ceil().max(0.0) as usize)
            .max(first + 1)
            .min(peaks.len());
        if first >= last {
            return None;
        }
        peaks[first..last].iter().copied().reduce(Peak::merge)
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut b = MAGIC.to_vec();
        b.extend_from_slice(&self.rate.to_le_bytes());
        b.extend_from_slice(&self.frames.to_le_bytes());
        b.extend_from_slice(&BASE.to_le_bytes());
        b.extend_from_slice(&(STEP as u32).to_le_bytes());
        b.extend_from_slice(&(self.levels.len() as u32).to_le_bytes());
        for l in &self.levels {
            b.extend_from_slice(&(l.len() as u64).to_le_bytes());
            for p in l {
                b.push(p.min as u8);
                b.push(p.max as u8);
            }
        }
        b
    }

    fn from_bytes(b: &[u8]) -> Option<Peaks> {
        let mut at = 0;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(at..at + n)?;
            at += n;
            Some(s)
        };
        if take(4)? != MAGIC {
            return None;
        }
        let u32_of = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap_or([0; 4]));
        let rate = u32_of(take(4)?);
        let frames = u64::from_le_bytes(take(8)?.try_into().ok()?);
        if u32_of(take(4)?) != BASE || u32_of(take(4)?) as usize != STEP {
            return None;
        }
        let count = u32_of(take(4)?) as usize;
        let mut levels = Vec::with_capacity(count.min(64));
        for _ in 0..count {
            let n = u64::from_le_bytes(take(8)?.try_into().ok()?) as usize;
            let raw = take(n.checked_mul(2)?)?;
            levels.push(
                raw.chunks(2)
                    .map(|c| Peak {
                        min: c[0] as i8,
                        max: c[1] as i8,
                    })
                    .collect(),
            );
        }
        (!levels.is_empty()).then_some(Peaks {
            rate,
            frames,
            levels,
        })
    }
}

/// Where the peak files of a song are kept: its `Peaks/` folder, or for a song not saved yet
/// the cache (`XIRIUS_PEAK_CACHE`, else `%LOCALAPPDATA%\Xirius\Peaks`).
pub fn store_dir(song_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = song_dir {
        return dir.join(FOLDER);
    }
    if let Some(dir) = std::env::var_os("XIRIUS_PEAK_CACHE") {
        return PathBuf::from(dir);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Xirius")
        .join(FOLDER)
}

/// FNV-1a.
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The size of a file and when it was last written, in ns from 1970: what tells one version
/// of a file from the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stamp {
    pub size: u64,
    pub time: u128,
}

impl Stamp {
    /// The stamp of `source` now; zero for a file that is not there.
    pub fn of(source: &Path) -> Stamp {
        std::fs::metadata(source)
            .map(|m| Stamp::from_meta(&m))
            .unwrap_or_default()
    }

    pub fn from_meta(meta: &std::fs::Metadata) -> Stamp {
        Stamp {
            size: meta.len(),
            time: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos()),
        }
    }
}

/// The peak file of `source` in `store`: its name, then a hash of its path, size and time.
pub fn file_for(source: &Path, store: &Path) -> PathBuf {
    file_stamped(source, store, Stamp::of(source))
}

/// [`file_for`] with the stamp the caller has read, so the file is not looked at again.
pub fn file_stamped(source: &Path, store: &Path, stamp: Stamp) -> PathBuf {
    let Stamp { size, time } = stamp;
    let key = format!(
        "{}|{size}|{time}",
        source.to_string_lossy().replace('\\', "/").to_lowercase()
    );
    let stem: String = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    store.join(format!("{stem}-{:016x}.xpk", hash(key.as_bytes())))
}

/// The peaks kept for `source` in `store`, if they are there and readable.
pub fn load(source: &Path, store: &Path) -> Option<Peaks> {
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("peaks_load", source).ok()?;
    load_stamped(source, store, Stamp::of(source))
}

/// [`load`] for the version of `source` that `stamp` says.
pub fn load_stamped(source: &Path, store: &Path, stamp: Stamp) -> Option<Peaks> {
    Peaks::from_bytes(&std::fs::read(file_stamped(source, store, stamp)).ok()?)
}

/// Reads `source` and makes its peaks.
pub fn build(source: &Path) -> Result<Peaks, String> {
    let d = crate::decode::decode(source)?;
    Ok(Peaks::from_samples(d.rate, &d.left, &d.right))
}

/// The peaks of `source`: those kept in `store`, else made and kept there.
pub fn ensure(source: &Path, store: &Path) -> Result<Peaks, String> {
    if let Some(p) = load(source, store) {
        return Ok(p);
    }
    let peaks = build(source)?;
    keep(source, store, Stamp::of(source), &peaks)?;
    Ok(peaks)
}

/// Keeps `peaks`, made from the version of `source` that `stamp` says, in `store`.
pub fn keep(source: &Path, store: &Path, stamp: Stamp, peaks: &Peaks) -> Result<(), String> {
    let path = file_stamped(source, store, stamp);
    std::fs::create_dir_all(store).map_err(|e| format!("{} を作れません: {e}", store.display()))?;
    // Written beside it and renamed, so a half-written file is never read.
    let tmp = path.with_extension("xpk.part");
    let mut f =
        std::fs::File::create(&tmp).map_err(|e| format!("{} に書けません: {e}", tmp.display()))?;
    f.write_all(&peaks.to_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("{} に書けません: {e}", tmp.display()))?;
    drop(f);
    std::fs::rename(&tmp, &path).map_err(|e| format!("{} に書けません: {e}", path.display()))
}

// ---- in the background -------------------------------------------------------------------

/// What a background build left.
pub type Built = Result<Arc<Peaks>, String>;

type Key = (PathBuf, PathBuf);
#[derive(Clone)]
struct Tagged {
    generation: u64,
    built: Built,
}
#[derive(Clone)]
pub struct Request {
    key: Key,
    id: Arc<AtomicU64>,
    generation: u64,
    wanted: Arc<AtomicU64>,
}
impl Request {
    pub fn accepted(&self) -> bool {
        self.id.load(Ordering::Acquire) != 0
    }
}
fn jobs() -> &'static crate::read_jobs::ReadJobs<Key, Tagged> {
    static JOBS: OnceLock<crate::read_jobs::ReadJobs<Key, Tagged>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}
fn generations() -> &'static Mutex<HashMap<Key, Arc<AtomicU64>>> {
    static WANTED: OnceLock<Mutex<HashMap<Key, Arc<AtomicU64>>>> = OnceLock::new();
    WANTED.get_or_init(Default::default)
}
fn latest() -> &'static Mutex<HashMap<PathBuf, Request>> {
    static LATEST: OnceLock<Mutex<HashMap<PathBuf, Request>>> = OnceLock::new();
    LATEST.get_or_init(Default::default)
}
static FINISHED: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(1);
pub fn finished() -> u64 {
    FINISHED.load(Ordering::Acquire)
}
fn launch(key: &Key, wanted: Arc<AtomicU64>) -> Option<u64> {
    let (path, store) = key.clone();
    jobs().request(key.clone(), move || {
        let generation = wanted.load(Ordering::Acquire);
        let built = std::panic::catch_unwind(|| {
            #[cfg(feature = "test-io")]
            xirius_core::io_test::before("peaks_source", &path).map_err(|e| e.to_string())?;
            if !path.is_file() {
                return Err("波形の元のファイルが見つかりません".into());
            }
            let peaks = ensure(&path, &store)?;
            if !path.is_file() {
                return Err("波形の元のファイルが見つかりません".into());
            }
            Ok(peaks)
        })
        .unwrap_or_else(|_| Err("ピークを作れません".into()))
        .map(Arc::new);
        FINISHED.fetch_add(1, Ordering::AcqRel);
        Tagged { generation, built }
    })
}
fn ask(source: &Path, store: &Path) -> Request {
    let key = (source.to_path_buf(), store.to_path_buf());
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let wanted = generations()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key.clone())
        .or_default()
        .clone();
    wanted.fetch_max(generation, Ordering::AcqRel);
    let id = launch(&key, wanted.clone()).unwrap_or(0);
    Request {
        key,
        id: Arc::new(AtomicU64::new(id)),
        generation,
        wanted,
    }
}
/// Outstanding reads of the same source/store share one physical job. A later
/// request never consumes a completed result validated before that request.
pub fn request(source: &Path, store: &Path) -> Request {
    let request = ask(source, store);
    let old = latest()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(source.to_path_buf(), request.clone());
    if let Some(old) = old
        && old.key != request.key
    {
        discard(&old);
    }
    request
}
pub fn request_fresh(source: &Path, store: &Path) -> Request {
    ask(source, store)
}
pub fn discard(request: &Request) {
    let id = request.id.load(Ordering::Acquire);
    if id != 0 {
        jobs().discard(&request.key, id);
    }
}
pub fn take_request(request: &Request) -> Option<Built> {
    let current = jobs().current_id(&request.key).unwrap_or(0);
    request.id.store(current, Ordering::Release);
    if current != 0 {
        match jobs().peek(&request.key, current) {
            Some(Ok(tag)) if tag.generation >= request.generation => return Some(tag.built),
            Some(Err(())) => return Some(Err("ピークの読み込みが止まりました".into())),
            None => return None,
            _ => {}
        }
    }
    // The prior physical read finished with an older generation, or was
    // cancelled by a departed view. Revalidate on a worker, coalescing retries.
    let id = launch(&request.key, request.wanted.clone()).unwrap_or(0);
    request.id.store(id, Ordering::Release);
    None
}
pub fn take(source: &Path) -> Option<Built> {
    let request = latest()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(source)
        .cloned()?;
    let result = take_request(&request)?;
    latest()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(source);
    Some(result)
}
/// CLI/tests only; windows poll request handles and never join disk I/O.
pub fn wait() {
    loop {
        let wanted: Vec<_> = generations()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, generation) in wanted {
            if let Some(id) = jobs().current_id(&key)
                && let Some(Ok(tag)) = jobs().peek(&key, id)
                && tag.generation < generation.load(Ordering::Acquire)
            {
                launch(&key, generation);
            }
        }
        if !jobs().pending() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of the test's own, removed when dropped (also when the test fails).
    fn dir(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("xirius-peaks-{tag}-"))
            .tempdir()
            .unwrap()
    }

    #[test]
    fn levels_merge_the_one_before_and_zoom_picks_one() {
        let n: usize = 48_000 * 3;
        let x: Vec<f32> = (0..n)
            .map(|i| if i < 48_000 { 0.5 } else { -0.25 } * ((i % 7) as f32 / 6.0))
            .collect();
        let p = Peaks::from_samples(48_000, &x, &x);
        assert_eq!(p.levels[0].len(), n.div_ceil(BASE as usize));
        for k in 1..p.levels.len() {
            for (i, peak) in p.levels[k].iter().enumerate() {
                let kids = &p.levels[k - 1][i * STEP..((i + 1) * STEP).min(p.levels[k - 1].len())];
                assert_eq!(*peak, kids.iter().copied().reduce(Peak::merge).unwrap());
            }
        }
        assert_eq!(p.levels.last().unwrap().len(), 1);
        assert_eq!(p.level_for(1.0), 0);
        assert_eq!(p.level_for(511.0), 0);
        assert_eq!(p.level_for(512.0), 1);
        assert_eq!(p.level_for(8_000.0), 2);
        assert_eq!(p.level_for(10_000.0), 3);
        // The first second is loud and positive, the rest quieter and negative.
        let first = p.reduce(p.level_for(4800.0), 0.0, 40_960.0).unwrap();
        assert_eq!((first.min, first.max), (0, 64));
        let rest = p.reduce(0, 50_000.0, 140_000.0).unwrap();
        assert_eq!((rest.min, rest.max), (-32, 0));
        assert!(p.reduce(0, 1e9, 1e9 + 1.0).is_none());
    }

    #[test]
    fn a_peak_file_is_written_once_read_back_and_renewed_when_the_audio_changes() {
        let tmp = dir("file");
        let d = tmp.path();
        let wav = d.join("Loop.wav");
        let x: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.01).sin() * 0.8).collect();
        crate::export::write_wav(&wav, &x, &x, 48_000, 16).unwrap();
        let store = store_dir(Some(d));
        assert_eq!(store, d.join("Peaks"));
        let made = ensure(&wav, &store).unwrap();
        let file = file_for(&wav, &store);
        assert!(file.is_file());
        assert!(
            file.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Loop-")
        );
        assert_eq!(load(&wav, &store).unwrap(), made);
        assert_eq!(made.frames, 24_000);
        assert_eq!(made.levels[0].iter().map(|p| p.max).max(), Some(102));
        // Another file in its place is another peak file.
        let y = vec![0.1f32; 12_000];
        crate::export::write_wav(&wav, &y, &y, 48_000, 16).unwrap();
        std::fs::File::options()
            .append(true)
            .open(&wav)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert_ne!(file_for(&wav, &store), file);
        assert_eq!(ensure(&wav, &store).unwrap().frames, 12_000);
        // A broken peak file is made again.
        std::fs::write(file_for(&wav, &store), b"XPK1junk").unwrap();
        assert!(load(&wav, &store).is_none());
        assert_eq!(ensure(&wav, &store).unwrap().frames, 12_000);
    }

    #[test]
    fn a_request_builds_in_the_background_and_hands_over_once() {
        let tmp = dir("bg");
        let d = tmp.path();
        let wav = d.join("Pad.wav");
        let x = vec![0.5f32; 9_600];
        crate::export::write_wav(&wav, &x, &x, 48_000, 16).unwrap();
        let store = d.join("cache");
        let before = finished();
        request(&wav, &store);
        request(&wav, &store);
        let start = std::time::Instant::now();
        let got = loop {
            if let Some(b) = take(&wav) {
                break b;
            }
            assert!(start.elapsed().as_secs() < 20, "the build never ended");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert_eq!(got.unwrap().frames, 9_600);
        assert!(finished() > before);
        assert!(take(&wav).is_none());
        // A file that is not audio says why.
        let txt = d.join("x.txt");
        std::fs::write(&txt, "x").unwrap();
        request(&txt, &store);
        wait();
        assert!(take(&txt).unwrap().unwrap_err().contains("WAV"));
    }
}
