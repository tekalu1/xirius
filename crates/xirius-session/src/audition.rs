//! The player of the file browser's audition (BRW-020, BRW-021, BRW-022): a small player of
//! its own, kept open, that reads a file on a thread of its own and plays it on the kept plan.
//!
//! [`Audition::play`] hands a file over and returns at once; the window's thread never opens a
//! file. A thread reads the file once, start to end, while another decodes what has come in,
//! so the length, the sound and the peaks all come from that one read, and the peaks grow as
//! the file is read ([`Now::peaks`]). The newest file asked for is the one that plays: the
//! reading of one asked for before it stops, unless it is one of those read ahead.
//!
//! Each press and stop is an ask with a number of its own ([`Now::ask`]); what comes of an
//! older ask is not said of a newer one. Only the readers wait on a file: the lead takes the
//! asks and hands the files round, and the voice holds the engine and the device, so a stop or
//! a press is taken at once, however slow the place.
//!
//! What was decoded is kept ([`Limits`]), so the same file again plays without being opened,
//! and [`Audition::prefetch`] reads the rows next to the chosen one before they are asked for.
//! The engine runs on the device the song plays on, in shared mode, so the two sound together.
//! It is closed when nothing has played for a while, and when the song's player wants the
//! device alone ([`Audition::release`]). Exclusive settings instead feed a bounded PCM queue
//! into the already prepared song engine ([`Sink::Mix`]); that path opens no device.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use xirius_engine::audition::{self as cue, RATE};
use xirius_engine::engine::{self, EngineConfig};
use xirius_engine::{AudioData, Command, Controller, EngineEvent, ProcessSetup, RenderMode};

use crate::decode::{self, Decoded};
use crate::output::{Drivers, Output};
use crate::peaks::{self, BASE, Peak, Peaks, Stamp};

/// How a file is looked at and read. [`Disk`] is the system's; a test puts in a slow place
/// that counts what it is asked.
pub trait Reader: Send + Sync {
    /// The file's size and time, without opening it.
    fn stat(&self, path: &Path) -> Result<Stamp, String>;
    /// Opens the file to read it from the start.
    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>, String>;
}

/// The files on the disk.
pub struct Disk;

impl Reader for Disk {
    fn stat(&self, path: &Path) -> Result<Stamp, String> {
        std::fs::metadata(path)
            .map(|m| Stamp::from_meta(&m))
            .map_err(|e| format!("{} を開けません: {e}", path.display()))
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Read + Send>, String> {
        std::fs::File::open(path)
            .map(|f| Box::new(f) as Box<dyn Read + Send>)
            .map_err(|e| format!("{} を開けません: {e}", path.display()))
    }
}

/// How much decoded sound is kept, in bytes (BRW-021). Not a setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// All the files together; the one played longest ago goes first.
    pub total: usize,
    /// One file; a longer one plays but is not kept.
    pub one: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            total: 256 << 20,
            one: 64 << 20,
        }
    }
}

/// Where the audition's sound goes.
#[derive(Clone)]
pub enum Sink {
    /// The existing song output, used only with exclusive device settings.
    Mix(cue::bus::Port),
    /// Nowhere (tests, machines without a device).
    Silent { paced: bool },
    /// The device the song's settings choose, opened in shared mode, and the pair of its
    /// outputs the song plays on.
    Device {
        prefs: xirius_prefs::Prefs,
        drivers: Drivers,
        pair: [usize; 2],
    },
}

impl Sink {
    /// The same device, opened the same way (the pair of outputs changes without opening it).
    fn same(&self, other: &Sink) -> bool {
        match (self, other) {
            (Sink::Silent { paced: a }, Sink::Silent { paced: b }) => a == b,
            (Sink::Device { prefs: a, .. }, Sink::Device { prefs: b, .. }) => {
                ["audio.driver", "audio.device"]
                    .iter()
                    .all(|k| a.text(k) == b.text(k))
            }
            _ => false,
        }
    }

    fn pair(&self) -> [usize; 2] {
        match self {
            Sink::Device { pair, .. } => *pair,
            Sink::Silent { .. } | Sink::Mix(_) => [0, 1],
        }
    }

    fn open(&self, audio: xirius_engine::AudioEngine) -> Result<Output, String> {
        match self {
            Sink::Mix(_) => Err("試聴は曲の出力へ送ります".into()),
            Sink::Silent { paced } => Ok(Output::Silent(crate::output::SilentOutput::start(
                audio, *paced,
            ))),
            Sink::Device { prefs, drivers, .. } => {
                let device = crate::audio_settings::configured_device(prefs, &(drivers.devices)());
                (drivers.shared_out)(audio, device, None)
            }
        }
    }
}

/// Where the file asked for last is.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Phase {
    /// Nothing is asked for.
    #[default]
    Idle,
    /// Being read; nothing is heard yet.
    Loading,
    Playing,
    /// Played to its end.
    Ended,
    /// Could not be read or played; the sentence says why.
    Failed(String),
}

/// What the audition is doing, for the window to draw.
#[derive(Clone, Debug, Default)]
pub struct Now {
    /// The generation of a press, stop, or retry after a stale cache entry.
    pub ask: u64,
    /// The file asked for last; `None` once stopped.
    pub path: Option<PathBuf>,
    pub phase: Phase,
    /// When it was asked for.
    pub asked: Option<Instant>,
    /// How far it has played, 0 to 1.
    pub played: f32,
    /// The peaks of what has been read so far, at the file's rate.
    pub peaks: Option<Arc<Peaks>>,
    /// The frames of the file the peaks cover.
    pub covered: u64,
    /// The file's length in frames at its own rate, once the file has said.
    pub whole: Option<u64>,
    /// It played from what was kept: it was never read.
    pub kept: bool,
    /// Moves whenever any of this changes, so the window knows to draw again.
    pub serial: u64,
}

impl Now {
    /// The file `path` is asked for and is being read.
    pub fn loading(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path) && self.phase == Phase::Loading
    }

    /// The peaks of `path` cover the whole file.
    pub fn complete(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path)
            && self.peaks.is_some()
            && self.whole.is_some_and(|w| self.covered >= w)
    }
}

/// How often the peaks of a file being read are made again.
const GROW_EVERY: Duration = Duration::from_millis(80);
/// Bytes per read of the file.
const CHUNK: usize = 256 << 10;
/// Files read ahead of being asked for at once.
const AHEAD: usize = 2;
/// How long a cued file that has never said it plays is waited for before it counts as ended.
const START_WAIT: Duration = Duration::from_secs(10);
/// How long the device stays open with nothing playing.
const IDLE: Duration = Duration::from_secs(30);
/// How long [`Audition::release`] waits for the device to close.
const RELEASE_WAIT: Duration = Duration::from_millis(300);

/// What the lead, the thread that takes the asks, is told.
enum Msg {
    #[cfg(test)]
    Barrier(Sender<()>),
    Play {
        ask: u64,
        path: PathBuf,
        store: PathBuf,
        sink: Sink,
    },
    Stop,
    Prefetch(Vec<(PathBuf, PathBuf)>),
    Stale {
        id: u64,
    },
    Loaded {
        id: u64,
        got: Result<Got, String>,
    },
    Close,
}

/// What the voice, the thread that holds the engine and the device, is told.
enum Call {
    Cue {
        ask: u64,
        audio: Arc<AudioData>,
        sink: Sink,
    },
    Halt {
        ask: u64,
    },
    Release(Sender<()>),
    Close,
}

/// What a reader comes back with.
enum Got {
    Read(Loaded),
    /// The file is the same as one kept: it was not opened.
    Kept(Stamp),
}

struct Loaded {
    stamp: Stamp,
    audio: Arc<AudioData>,
    peaks: Arc<Peaks>,
}

struct Shared {
    now: Mutex<Now>,
    halted: AtomicU64,
    /// The files kept, the one played longest ago first, and their bytes in all.
    kept: Mutex<(Vec<(PathBuf, Stamp)>, usize)>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn is_kept(&self, path: &Path, stamp: Stamp) -> bool {
        lock(&self.kept)
            .0
            .iter()
            .any(|(p, s)| p == path && *s == stamp)
    }

    /// The number of the last ask.
    fn last(&self) -> u64 {
        lock(&self.now).ask
    }

    /// Changes what is said, if `ask` is still the last ask.
    fn change_for(&self, ask: u64, f: impl FnOnce(&mut Now)) {
        let mut n = lock(&self.now);
        if n.ask == ask {
            f(&mut n);
            n.serial += 1;
        }
    }

    /// Peaks of `path` read so far, while the ask `ask` is reading it for playing.
    fn grow(&self, ask: u64, path: &Path, peaks: Arc<Peaks>, whole: Option<u64>) {
        let mut n = lock(&self.now);
        if n.ask == ask && n.loading(path) && peaks.frames > n.covered {
            n.covered = peaks.frames;
            n.whole = whole.or(n.whole);
            n.peaks = Some(peaks);
            n.serial += 1;
        }
    }
}

/// The audition's player. Dropping it stops the sound and closes the device.
pub struct Audition {
    tx: Sender<Msg>,
    voice: Sender<Call>,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Audition {
    /// A player that reads the disk and keeps [`Limits::default`].
    pub fn new() -> Audition {
        Audition::with(Arc::new(Disk), Limits::default())
    }

    pub fn with(reader: Arc<dyn Reader>, limits: Limits) -> Audition {
        let shared = Arc::new(Shared {
            now: Mutex::new(Now::default()),
            halted: AtomicU64::new(0),
            kept: Mutex::new((Vec::new(), 0)),
        });
        let (tx, rx) = mpsc::channel();
        let (voice_tx, voice_rx) = mpsc::channel();
        let mut lead = Lead {
            rx,
            tx: tx.clone(),
            voice: voice_tx.clone(),
            shared: shared.clone(),
            reader,
            limits,
            kept: VecDeque::new(),
            bytes: 0,
            reading: Vec::new(),
            ahead: Vec::new(),
            tried: Vec::new(),
            last: None,
            want: None,
            readers: 0,
        };
        let mut voice = Voice {
            rx: voice_rx,
            shared: shared.clone(),
            player: None,
            mixed: None,
            on: None,
            serial: 0,
            idle: Instant::now(),
        };
        let threads = [
            std::thread::Builder::new()
                .name("xirius-audition".into())
                .spawn(move || lead.run()),
            std::thread::Builder::new()
                .name("xirius-audition-voice".into())
                .spawn(move || voice.run()),
        ];
        Audition {
            tx,
            voice: voice_tx,
            shared,
            threads: threads.into_iter().flatten().collect(),
        }
    }

    /// Makes the next ask, saying `now` of it, and gives its number.
    fn ask(&self, now: Now) -> u64 {
        let mut n = lock(&self.shared.now);
        let ask = n.ask + 1;
        *n = Now {
            ask,
            serial: n.serial + 1,
            ..now
        };
        ask
    }

    /// Plays `path`, whose peaks are kept in `store`, on `sink`. Returns at once.
    pub fn play(&self, path: &Path, store: &Path, sink: Sink) {
        let ask = self.ask(Now {
            path: Some(path.to_path_buf()),
            phase: Phase::Loading,
            asked: Some(Instant::now()),
            ..Now::default()
        });
        // What played before goes quiet now, however long this one takes to read.
        let _ = self.voice.send(Call::Halt { ask });
        let _ = self.tx.send(Msg::Play {
            ask,
            path: path.to_path_buf(),
            store: store.to_path_buf(),
            sink,
        });
    }

    /// Stops what plays. Returns at once.
    pub fn stop(&self) {
        let ask = self.ask(Now::default());
        let _ = self.voice.send(Call::Halt { ask });
        let _ = self.tx.send(Msg::Stop);
    }

    /// Reads these files (and the stores of their peaks) ahead, in this order, so they play at
    /// once when they are asked for. A list replaces the one before.
    pub fn prefetch(&self, files: Vec<(PathBuf, PathBuf)>) {
        let _ = self.tx.send(Msg::Prefetch(files));
    }

    /// Stops what plays and closes the device, so the song's player can have it alone (in
    /// exclusive mode). Waits a little for it to close: the voice never waits on a file, so it
    /// is quick. The next file opens it again.
    pub fn release(&self) {
        let _ = self.release_async().recv_timeout(RELEASE_WAIT);
    }

    /// Requests a close on the voice thread. Opening a conflicting device must wait
    /// for this acknowledgement, without blocking the window thread.
    pub fn release_async(&self) -> Receiver<()> {
        self.stop();
        let (tx, rx) = mpsc::channel();
        let _ = self.voice.send(Call::Release(tx));
        rx
    }

    pub fn halted(&self) -> bool {
        let n = lock(&self.shared.now);
        matches!(n.phase, Phase::Ended | Phase::Failed(_))
            || (n.phase == Phase::Idle && self.shared.halted.load(Ordering::Acquire) >= n.ask)
    }

    pub fn now(&self) -> Now {
        lock(&self.shared.now).clone()
    }

    /// The files kept, the one played longest ago first, and their bytes in all.
    pub fn kept(&self) -> (Vec<PathBuf>, usize) {
        let k = lock(&self.shared.kept);
        (k.0.iter().map(|(p, _)| p.clone()).collect(), k.1)
    }
}

impl Default for Audition {
    fn default() -> Audition {
        Audition::new()
    }
}

impl Drop for Audition {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Close);
        let _ = self.voice.send(Call::Close);
        // The lead owns at most three readers. It reaps them after uncancellable OS
        // calls return; closing a window never waits for those calls.
        if !self.threads.is_empty() {
            drop(self.threads.remove(0));
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

// ---- the lead: the asks, the readers and what is kept --------------------------------------

struct Item {
    path: PathBuf,
    stamp: Stamp,
    audio: Arc<AudioData>,
    peaks: Arc<Peaks>,
    bytes: usize,
}

/// A file being read, and the ask it is read for (0 while no ask wants it).
struct Reading {
    id: u64,
    path: PathBuf,
    store: PathBuf,
    cancel: Arc<AtomicBool>,
    ask: Arc<AtomicU64>,
    thread: JoinHandle<()>,
}

/// The file asked for last, until it is cued.
struct Want {
    ask: u64,
    path: PathBuf,
    sink: Sink,
    /// The reading it waits for.
    reading: Option<u64>,
    store: PathBuf,
    cued: bool,
}

/// The thread that takes the asks: it starts the readers, keeps what they read and hands it to
/// the voice. It never waits on a file.
struct Lead {
    rx: Receiver<Msg>,
    tx: Sender<Msg>,
    voice: Sender<Call>,
    shared: Arc<Shared>,
    reader: Arc<dyn Reader>,
    limits: Limits,
    kept: VecDeque<Item>,
    bytes: usize,
    reading: Vec<Reading>,
    /// Files to read ahead.
    ahead: Vec<(PathBuf, PathBuf)>,
    /// Files of those ahead the readers came back for, kept or not (too long, unreadable), so
    /// they are not read again while they stay in the list.
    tried: Vec<PathBuf>,
    /// The file asked for last.
    last: Option<PathBuf>,
    want: Option<Want>,
    /// The number of the last reading started.
    readers: u64,
}

impl Lead {
    fn run(&mut self) {
        let mut closing = false;
        while let Ok(m) = self.rx.recv() {
            match m {
                #[cfg(test)]
                Msg::Barrier(done) => {
                    let _ = done.send(());
                }
                Msg::Close => {
                    closing = true;
                    self.want = None;
                    self.ahead.clear();
                    self.drop_unwanted();
                }
                Msg::Loaded { id, got } => {
                    if closing {
                        if let Some(at) = self.reading.iter().position(|r| r.id == id) {
                            let _ = self.reading.remove(at).thread.join();
                        }
                    } else {
                        self.loaded(id, got);
                    }
                }
                Msg::Play {
                    ask,
                    path,
                    store,
                    sink,
                } if !closing => self.play(ask, path, store, sink),
                Msg::Stop if !closing => {
                    self.want = None;
                    self.drop_unwanted();
                }
                Msg::Prefetch(files) if !closing => {
                    self.tried.retain(|t| files.iter().any(|(p, _)| p == t));
                    self.ahead = files;
                    self.drop_unwanted();
                    self.schedule();
                }
                Msg::Stale { id } if !closing => self.stale(id),
                _ => {}
            }
            if closing && self.reading.is_empty() {
                break;
            }
        }
    }

    fn stale(&mut self, id: u64) {
        if let Some(w) = &mut self.want
            && w.reading == Some(id)
            && w.cued
        {
            let mut n = lock(&self.shared.now);
            if n.ask != w.ask {
                return;
            }
            if !matches!(n.phase, Phase::Loading | Phase::Playing) {
                // Refresh the cache, without reviving an ended or failed preview.
                let path = w.path.clone();
                drop(n);
                if let Some(r) = self.reading.iter().find(|r| r.id == id) {
                    r.ask.store(0, Ordering::Relaxed);
                }
                self.want = None;
                self.kept.retain(|i| i.path != path);
                self.bytes = self.kept.iter().map(|i| i.bytes).sum();
                self.show_kept();
                return;
            }
            // A new attempt invalidates every status update from the optimistic cue.
            n.ask += 1;
            w.ask = n.ask;
            w.cued = false;
            n.phase = Phase::Loading;
            n.played = 0.0;
            n.kept = false;
            n.peaks = None;
            n.covered = 0;
            n.whole = None;
            n.asked = Some(Instant::now());
            n.serial += 1;
            drop(n);
            if let Some(r) = self.reading.iter().find(|r| r.id == id) {
                r.ask.store(w.ask, Ordering::Relaxed);
            }
            let _ = self.voice.send(Call::Halt { ask: w.ask });
            self.kept.retain(|i| i.path != w.path);
            self.bytes = self.kept.iter().map(|i| i.bytes).sum();
            self.show_kept();
        }
    }

    fn fail(&mut self, ask: u64, why: String) {
        self.want = None;
        let mut n = lock(&self.shared.now);
        if n.ask != ask {
            return;
        }
        n.ask += 1;
        n.phase = Phase::Failed(why);
        n.serial += 1;
        let _ = self.voice.send(Call::Halt { ask: n.ask });
    }

    fn play(&mut self, ask: u64, path: PathBuf, store: PathBuf, sink: Sink) {
        self.last = Some(path.clone());
        self.want = Some(Want {
            ask,
            path: path.clone(),
            sink,
            reading: None,
            store,
            cued: false,
        });
        self.drop_unwanted();
        // A cache hit cues from memory now. Its reader validates the stamp in parallel.
        if let Some(stamp) = self.kept.iter().find(|i| i.path == path).map(|i| i.stamp)
            && let Some((audio, peaks)) = self.take_kept(&path, stamp)
        {
            self.play_loaded(ask, audio, peaks, true);
        }
        self.schedule();
    }

    fn schedule(&mut self) {
        if let Some(w) = &self.want
            && w.reading.is_none()
        {
            let (ask, path, store) = (w.ask, w.path.clone(), w.store.clone());
            let id = if let Some(r) = self
                .reading
                .iter()
                .find(|r| r.path == path && !r.cancel.load(Ordering::Relaxed))
            {
                r.ask.store(ask, Ordering::Relaxed);
                Some(r.id)
            } else if self.reading.len() < AHEAD + 1 {
                let id = self.read(path, store, ask, true);
                if id.is_none() {
                    self.fail(ask, "試聴を開始できませんでした".into());
                }
                id
            } else {
                None
            };
            if let Some(w) = &mut self.want {
                w.reading = id;
            }
        }
        self.read_ahead();
    }

    /// Hands the file asked for to the voice.
    fn cue(&mut self, audio: Arc<AudioData>) {
        if let Some(w) = &self.want {
            let _ = self.voice.send(Call::Cue {
                ask: w.ask,
                audio,
                sink: w.sink.clone(),
            });
        }
    }

    /// Lets go of the readers of files no longer asked for nor read ahead.
    fn drop_unwanted(&mut self) {
        let want = self.want.as_ref().map(|w| w.path.clone());
        let ahead = &self.ahead;
        for r in &self.reading {
            let keep = want.as_ref() == Some(&r.path) || ahead.iter().any(|(p, _)| *p == r.path);
            if !keep {
                r.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    /// Starts reading the files ahead, a few at a time. A file kept is only looked at, by the
    /// reader, so one changed since it was kept is read again.
    fn read_ahead(&mut self) {
        let mut i = 0;
        while i < self.ahead.len() && self.reading.len() < AHEAD {
            let (path, store) = self.ahead[i].clone();
            i += 1;
            if self.reading.iter().any(|r| r.path == path) || self.tried.contains(&path) {
                continue;
            }
            if self.read(path.clone(), store, 0, true).is_none() {
                self.tried.push(path);
            }
        }
    }

    /// Starts reading `path` on a thread of its own for the ask `ask`, and gives the reading's
    /// number. The reader first looks whether the file is the same as one kept, if `look`.
    fn read(&mut self, path: PathBuf, store: PathBuf, ask: u64, look: bool) -> Option<u64> {
        self.readers += 1;
        let id = self.readers;
        let cancel = Arc::new(AtomicBool::new(false));
        let ask = Arc::new(AtomicU64::new(ask));
        let (tx, shared, reader) = (self.tx.clone(), self.shared.clone(), self.reader.clone());
        let (p, c, k, at) = (path.clone(), cancel.clone(), ask.clone(), store.clone());
        let thread = std::thread::Builder::new()
            .name("xirius-audition-read".into())
            .spawn(move || {
                let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    load((&p, &at), &c, &k, &shared, &*reader, look, &|| {
                        let _ = tx.send(Msg::Stale { id });
                    })
                }))
                .unwrap_or_else(|_| Err(format!("{} を読めません", p.display())));
                let _ = tx.send(Msg::Loaded { id, got });
            })
            .ok()?;
        self.reading.push(Reading {
            id,
            path,
            store,
            cancel,
            ask,
            thread,
        });
        Some(id)
    }

    fn loaded(&mut self, id: u64, got: Result<Got, String>) {
        let Some(at) = self.reading.iter().position(|r| r.id == id) else {
            return;
        };
        let r = self.reading.remove(at);
        let _ = r.thread.join();
        if self.ahead.iter().any(|(p, _)| *p == r.path) && !self.tried.contains(&r.path) {
            self.tried.push(r.path.clone());
        }
        let wanted = self
            .want
            .as_ref()
            .filter(|w| w.reading == Some(id))
            .map(|w| w.ask);
        if !r.cancel.load(Ordering::Relaxed)
            && let Ok(Got::Read(l)) = &got
        {
            self.keep(&r.path, l, wanted.is_none());
        }
        let Some(ask) = wanted else {
            self.schedule();
            return;
        };
        if matches!(got, Ok(Got::Kept(_))) && self.want.as_ref().is_some_and(|w| w.cued) {
            self.want = None;
            self.schedule();
            return;
        }
        match got {
            Ok(Got::Read(l)) => self.play_loaded(ask, l.audio, l.peaks, false),
            Ok(Got::Kept(stamp)) => match self.take_kept(&r.path, stamp) {
                Some((audio, peaks)) => self.play_loaded(ask, audio, peaks, true),
                // Let go of since the reader looked: read it after all.
                None => match self.read(r.path, r.store, ask, false) {
                    Some(id) => {
                        if let Some(w) = &mut self.want {
                            w.reading = Some(id);
                        }
                    }
                    None => self.fail(ask, "試聴を開始できませんでした".into()),
                },
            },
            Err(e) => self.fail(ask, e),
        }
        if self.want.as_ref().is_some_and(|w| w.cued) {
            self.want = None;
        }
        self.schedule();
    }

    fn play_loaded(&mut self, ask: u64, audio: Arc<AudioData>, peaks: Arc<Peaks>, kept: bool) {
        self.shared.change_for(ask, |n| {
            n.kept = kept;
            n.covered = peaks.frames;
            n.whole = Some(peaks.frames);
            n.peaks = Some(peaks);
        });
        if let Some(w) = &mut self.want {
            w.cued = true;
        }
        self.cue(audio);
    }

    /// What is kept of `path` as it is at `stamp`, made the one played last.
    fn take_kept(&mut self, path: &Path, stamp: Stamp) -> Option<(Arc<AudioData>, Arc<Peaks>)> {
        let at = self
            .kept
            .iter()
            .position(|i| i.path == path && i.stamp == stamp)?;
        let item = self.kept.remove(at)?;
        let got = (item.audio.clone(), item.peaks.clone());
        self.kept.push_back(item);
        self.show_kept();
        Some(got)
    }

    /// Keeps what was read of `path`, as the one played last, or if it was read `ahead`, just
    /// before the file asked for last, so it goes before that one.
    fn keep(&mut self, path: &Path, l: &Loaded, ahead: bool) {
        let bytes = (l.audio.left.capacity() + l.audio.right.capacity())
            * std::mem::size_of::<f32>()
            + l.peaks.levels.capacity() * std::mem::size_of::<Vec<peaks::Peak>>()
            + l.peaks
                .levels
                .iter()
                .map(|v| v.capacity() * std::mem::size_of::<peaks::Peak>())
                .sum::<usize>()
            + std::mem::size_of::<AudioData>()
            + std::mem::size_of::<Peaks>();
        if bytes > self.limits.one {
            return;
        }
        self.kept.retain(|i| i.path != path);
        self.bytes = self.kept.iter().map(|i| i.bytes).sum();
        while self.bytes + bytes > self.limits.total {
            let Some(old) = self.kept.pop_front() else {
                break;
            };
            self.bytes -= old.bytes;
        }
        self.bytes += bytes;
        let item = Item {
            path: path.to_path_buf(),
            stamp: l.stamp,
            audio: l.audio.clone(),
            peaks: l.peaks.clone(),
            bytes,
        };
        let last = self.last.as_ref();
        match self
            .kept
            .iter()
            .position(|i| ahead && Some(&i.path) == last)
        {
            Some(at) => self.kept.insert(at, item),
            None => self.kept.push_back(item),
        }
        self.show_kept();
    }

    fn show_kept(&self) {
        *lock(&self.shared.kept) = (
            self.kept
                .iter()
                .map(|i| (i.path.clone(), i.stamp))
                .collect(),
            self.bytes,
        );
    }
}

// ---- the voice: the engine and the device --------------------------------------------------

struct Player {
    control: Controller,
    output: Output,
    sink: Sink,
    pair: [usize; 2],
}

/// The file cued, and what the engine has said of it.
struct On {
    ask: u64,
    serial: u64,
    len: u64,
    cued: Instant,
    heard: bool,
}

/// The thread that holds the engine and the device: it opens the device for the first file and
/// closes it when nothing has played for [`IDLE`].
struct Mixed {
    port: cue::bus::Port,
    audio: Arc<AudioData>,
    sent: usize,
    advanced: Instant,
    played: u64,
}

struct Voice {
    mixed: Option<Mixed>,
    rx: Receiver<Call>,
    shared: Arc<Shared>,
    player: Option<Player>,
    on: Option<On>,
    serial: u64,
    /// Since when nothing has played.
    idle: Instant,
}

impl Voice {
    fn run(&mut self) {
        loop {
            let call = match (&self.on, &self.player) {
                (Some(_), _) => self.rx.recv_timeout(Duration::from_millis(10)),
                (None, Some(_)) => self.rx.recv_timeout(Duration::from_millis(100)),
                (None, None) => self.rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match call {
                Ok(Call::Cue { ask, audio, sink }) => self.cue(ask, audio, &sink),
                Ok(Call::Halt { ask }) => {
                    self.halt();
                    self.shared.halted.fetch_max(ask, Ordering::Release);
                }
                Ok(Call::Release(done)) => {
                    self.halt();
                    self.player = None;
                    let _ = done.send(());
                }
                Ok(Call::Close) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.poll();
            if self.on.is_none() && self.player.is_some() && self.idle.elapsed() >= IDLE {
                self.player = None;
            }
        }
        self.halt();
    }

    /// Stops what plays, and lets go of its sound.
    fn halt(&mut self) {
        if let Some(m) = self.mixed.take() {
            m.port.stop();
        }
        if self.on.take().is_some() {
            self.idle = Instant::now();
            if let Some(p) = &mut self.player {
                let _ = cue::halt(&mut p.control);
            }
        }
    }

    fn cue(&mut self, ask: u64, audio: Arc<AudioData>, sink: &Sink) {
        // A newer press or a stop came while the file was read.
        if self.shared.last() != ask {
            return;
        }
        self.halt();
        if let Sink::Mix(port) = sink {
            self.player = None;
            self.serial += 1;
            let audio = if port.rate == RATE {
                audio
            } else {
                match decode::convert(
                    audio.left.clone(),
                    audio.right.clone(),
                    false,
                    RATE,
                    port.rate,
                ) {
                    Ok((left, right)) => Arc::new(AudioData { left, right }),
                    Err(e) => return self.shared.change_for(ask, |n| n.phase = Phase::Failed(e)),
                }
            };
            if self.shared.last() != ask {
                return;
            }
            let len = audio.left.len() as u64;
            port.start(self.serial);
            self.mixed = Some(Mixed {
                port: port.clone(),
                audio,
                sent: 0,
                advanced: Instant::now(),
                played: 0,
            });
            self.on = Some(On {
                ask,
                serial: self.serial,
                len,
                cued: Instant::now(),
                heard: false,
            });
            self.poll();
            return;
        }
        if let Err(e) = self.ready(sink) {
            return self.shared.change_for(ask, |n| n.phase = Phase::Failed(e));
        }
        if self.shared.last() != ask {
            self.halt();
            return;
        }
        let Some(p) = &mut self.player else {
            return;
        };
        self.serial += 1;
        let len = audio.left.len() as u64;
        if !cue::cue(&mut p.control, audio, self.serial) {
            return self.shared.change_for(ask, |n| {
                n.phase = Phase::Failed("試聴の指示を送れませんでした".into())
            });
        }
        self.on = Some(On {
            ask,
            serial: self.serial,
            len,
            cued: Instant::now(),
            heard: false,
        });
        self.shared.change_for(ask, |n| n.phase = Phase::Playing);
    }

    /// The engine and the device for `sink`, opened unless they are open already.
    fn ready(&mut self, sink: &Sink) -> Result<(), String> {
        let pair = sink.pair();
        if let Some(p) = &mut self.player
            && p.sink.same(sink)
            && p.output.lost().is_none()
        {
            if p.pair != pair && p.control.send(Command::OutputChannels(pair)).is_ok() {
                p.pair = pair;
            }
            return Ok(());
        }
        self.player = None;
        let setup = ProcessSetup {
            sample_rate: f64::from(RATE),
            max_block: cue::MAX_BLOCK,
            mode: RenderMode::Realtime,
        };
        let plan = cue::plan(setup).map_err(|e| format!("試聴を開始できませんでした: {e}"))?;
        let (audio, mut control) = engine::create(plan, EngineConfig::default());
        control
            .send(Command::OutputChannels(pair))
            .map_err(|_| "試聴の出力を設定できません".to_string())?;
        let output = sink.open(audio)?;
        self.player = Some(Player {
            control,
            output,
            sink: sink.clone(),
            pair,
        });
        Ok(())
    }

    /// Reads what the engine says of the file cued: how far it has played, and its end.
    fn poll(&mut self) {
        if self
            .on
            .as_ref()
            .is_some_and(|o| self.shared.last() != o.ask)
        {
            self.halt();
            return;
        }
        if self.mixed.as_ref().is_some_and(|m| !m.port.alive()) {
            if let Some(o) = &self.on {
                self.shared.change_for(o.ask, |n| {
                    n.path = None;
                    n.phase = Phase::Idle;
                });
            }
            self.halt();
            return;
        }
        if let (Some(m), Some(o)) = (&mut self.mixed, &mut self.on) {
            m.sent += m
                .port
                .feed(o.serial, &m.audio.left[m.sent..], &m.audio.right[m.sent..]);
            if let Some(position) = m.port.played(o.serial) {
                if position != m.played {
                    m.played = position;
                    m.advanced = Instant::now();
                }
                o.heard |= position > 0;
                self.shared.change_for(o.ask, |n| {
                    n.played = (position as f64 / o.len.max(1) as f64).min(1.0) as f32;
                    if position > 0 {
                        n.phase = Phase::Playing;
                    }
                });
                if position >= o.len {
                    let ask = o.ask;
                    self.halt();
                    self.shared.change_for(ask, |n| {
                        n.played = 1.0;
                        n.phase = Phase::Ended;
                    });
                    return;
                }
            }
            if m.advanced.elapsed() > START_WAIT {
                let ask = o.ask;
                self.halt();
                self.shared.change_for(ask, |n| {
                    n.phase = Phase::Failed("曲のオーディオ出力が停止しています".into())
                });
            }
            return;
        }
        let Some(p) = &mut self.player else {
            return;
        };
        let mut said: Option<(bool, u64)> = None;
        let serial = self.on.as_ref().map(|o| o.serial);
        p.control.poll(|ev| {
            if let EngineEvent::Transport {
                playing,
                position,
                serial: s,
            } = ev
                && Some(s) == serial
            {
                said = Some((playing, position));
            }
        });
        if let Some(e) = p.output.lost() {
            self.player = None;
            if let Some(o) = self.on.take() {
                self.idle = Instant::now();
                let why = crate::output::unavailable(&e);
                self.shared
                    .change_for(o.ask, |n| n.phase = Phase::Failed(why));
            }
            return;
        }
        let Some(o) = &mut self.on else {
            return;
        };
        let mut ended = false;
        if let Some((playing, position)) = said {
            o.heard |= playing;
            ended = position >= o.len || (!playing && o.heard);
            let played = (position as f64 / o.len.max(1) as f64).clamp(0.0, 1.0) as f32;
            self.shared.change_for(o.ask, |n| n.played = played);
        }
        if ended || (!o.heard && o.cued.elapsed() > START_WAIT) {
            let ask = o.ask;
            // The engine lets go of the file's sound: nothing of it stays on its timeline.
            self.halt();
            self.shared.change_for(ask, |n| {
                n.played = 1.0;
                n.phase = Phase::Ended;
            });
        }
    }
}

// ---- reading a file --------------------------------------------------------------------------

/// The bytes of a file read so far, and how the reading ended once it has.
type Bytes = (Vec<u8>, Option<Result<(), String>>);

/// The bytes of a file shared by the thread that reads it and the one that decodes.
struct Growing {
    bytes: Mutex<Bytes>,
    more: Condvar,
}

impl Growing {
    /// Room for a file of `size` bytes, so the bytes are not moved as they grow.
    fn new(size: u64) -> Growing {
        Growing {
            bytes: Mutex::new((Vec::with_capacity(usize::try_from(size).unwrap_or(0)), None)),
            more: Condvar::new(),
        }
    }

    fn push(&self, b: &[u8]) {
        lock(&self.bytes).0.extend_from_slice(b);
        self.more.notify_all();
    }

    fn end(&self, how: Result<(), String>) {
        lock(&self.bytes).1 = Some(how);
        self.more.notify_all();
    }
}

/// What the decoder reads: the bytes as they come, waiting for those not read yet.
struct Source {
    of: Arc<Growing>,
    pos: u64,
    len: u64,
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut g = lock(&self.of.bytes);
        loop {
            let have = g.0.len() as u64;
            if self.pos < have {
                let n = (have - self.pos).min(buf.len() as u64) as usize;
                let at = self.pos as usize;
                buf[..n].copy_from_slice(&g.0[at..at + n]);
                self.pos += n as u64;
                return Ok(n);
            }
            match &g.1 {
                Some(Ok(())) => return Ok(0),
                Some(Err(e)) => return Err(std::io::Error::other(e.clone())),
                None => g = self.of.more.wait(g).unwrap_or_else(|e| e.into_inner()),
            }
        }
    }
}

impl Seek for Source {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = at.ok_or_else(|| std::io::Error::other("seek before the start"))?;
        Ok(self.pos)
    }
}

impl symphonia::core::io::MediaSource for Source {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// Looks at `path`, reads it once and makes its sound at [`RATE`] and its peaks, showing the
/// peaks as they grow while the ask in `ask` wants it. The peaks are kept in `store` for the
/// song's lanes. A file the same as one kept is not opened, if `look`.
fn load(
    place: (&Path, &Path),
    cancel: &AtomicBool,
    ask: &AtomicU64,
    shared: &Shared,
    reader: &dyn Reader,
    look: bool,
    stale: &dyn Fn(),
) -> Result<Got, String> {
    let (path, store) = place;
    let stamp = reader.stat(path)?;
    if look && shared.is_kept(path, stamp) {
        return Ok(Got::Kept(stamp));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("読むのをやめました".into());
    }
    if look && lock(&shared.kept).0.iter().any(|(p, _)| p == path) {
        stale();
    }
    let stored = peaks::load_stamped(path, store, stamp).map(Arc::new);
    let show = |p: Arc<Peaks>, whole: Option<u64>| {
        shared.grow(ask.load(Ordering::Relaxed), path, p, whole);
    };
    if let Some(p) = &stored {
        show(p.clone(), Some(p.frames));
    }
    let mut file = reader.open(path)?;
    if cancel.load(Ordering::Relaxed) {
        return Err("読むのをやめました".into());
    }
    let bytes = Arc::new(Growing::new(stamp.size));
    let done = AtomicBool::new(false);
    let decoded = std::thread::scope(|s| {
        let source = Source {
            of: bytes.clone(),
            pos: 0,
            len: stamp.size,
        };
        let decoder = s.spawn(|| {
            let d = decode_growing(path, source, cancel, &show, stored.clone());
            done.store(true, Ordering::Relaxed);
            d
        });
        let mut buf = vec![0u8; CHUNK];
        let end = loop {
            if cancel.load(Ordering::Relaxed) {
                break Err("読むのをやめました".to_string());
            }
            // The decoder has what it wants of the file, or could not make it out.
            if done.load(Ordering::Relaxed) {
                break Ok(());
            }
            match file.read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(n) => bytes.push(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => break Err(format!("{} を読めません: {e}", path.display())),
            }
        };
        drop(file);
        bytes.end(end.clone());
        let decoded = decoder
            .join()
            .unwrap_or_else(|_| Err(format!("{} を読めません", path.display())));
        end.and(decoded)
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err("読むのをやめました".into());
    }
    let peaks = Arc::new(Peaks::from_samples(
        decoded.rate,
        &decoded.left,
        &decoded.right,
    ));
    if stored.is_none() {
        let _ = peaks::keep(path, store, stamp, &peaks);
    }
    let (left, right) = decode::convert(
        decoded.left,
        decoded.right,
        decoded.mono,
        decoded.rate,
        RATE,
    )?;
    Ok(Got::Read(Loaded {
        stamp,
        audio: Arc::new(AudioData { left, right }),
        peaks,
    }))
}

/// Decodes the bytes as they come, showing every [`GROW_EVERY`] the peaks of what is decoded
/// so far, or `stored`, the peaks made before.
fn decode_growing(
    path: &Path,
    mut source: Source,
    cancel: &AtomicBool,
    show: &(dyn Fn(Arc<Peaks>, Option<u64>) + Sync),
    stored: Option<Arc<Peaks>>,
) -> Result<Decoded, String> {
    let mut head = [0u8; 4];
    let rf64 = source.read_exact(&mut head).is_ok() && &head == b"RF64";
    source.pos = 0;
    if rf64 {
        // A take past 4 GiB, which symphonia does not read: the engine's reader reads the same
        // bytes, without opening the file again.
        let len = source.len;
        return decode::decode_rf64_source(path, source, len)?
            .ok_or_else(|| format!("{} を読めません", path.display()));
    }
    let mut base: Vec<Peak> = Vec::new();
    let mut last = Instant::now();
    decode::decode_source(path, Box::new(source), &mut |d, whole| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if base.is_empty() || last.elapsed() >= GROW_EVERY {
            last = Instant::now();
            if let Some(p) = &stored {
                // For an ask that took over the reading after it began.
                show(p.clone(), Some(p.frames));
                return true;
            }
            let run = BASE as usize;
            while (base.len() + 1) * run <= d.left.len() {
                let at = base.len() * run;
                base.push(Peak::of(&d.left[at..at + run], &d.right[at..at + run]));
            }
            let frames = (base.len() * run) as u64;
            show(
                Arc::new(Peaks::from_base(d.rate, frames, base.clone())),
                whole,
            );
        }
        true
    })
}

#[cfg(test)]
mod tests;
