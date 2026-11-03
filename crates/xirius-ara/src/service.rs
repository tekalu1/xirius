//! The ARA thread of one plugin: its document, its renderers, and the clips' sound
//! (PLG-032, PLG-034 to PLG-037, PLG-047, PLG-058).
//!
//! A [`Service`] owns one thread. Everything the plugin is called with runs there, marked as a
//! call into the plugin ([`crate::contain`]) and watched by a second thread: a contained fault
//! or a call that does not come back ends the ARA thread alone, and the service reports
//! itself crashed ([`Service::crashed`]). The song's clips then play dry until the service is
//! made again from the last archive.
//!
//! The song is handed over whole ([`Service::sync`]); the thread compares it with the model it
//! made and edits the document in one cycle: sources (one per file), modifications and
//! regions (one each per clip). After the plugin's analysis of a source completes, each clip
//! is rendered offline through its own playback renderer, and the result is handed back
//! ([`Service::state`]). Moving a clip keeps its rendered sound (PLG-036); a new length or a
//! new start in the source renders again. The plugin's notifications are pulled on the ARA
//! thread every 30 ms (PLG-034).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use crate::contain::{self, Seen, Watch};
use crate::document::{Document, FactoryInfo, Note, RegionProps, SourceProps};
use crate::host::{HostSide, Musical, Notice, Request, SourceAudio};
use crate::sys::{self, Ref, RoleFlags};
use crate::{Error, error};

/// A plugin binary with an ARA factory, used on the ARA thread only.
pub trait Binary {
    fn factory(&self) -> *const sys::Factory;
    /// A new plugin instance bound to the document controller `dc`.
    fn instance(
        &mut self,
        dc: Ref,
        known: RoleFlags,
        assigned: RoleFlags,
    ) -> Result<Box<dyn Instance>, Error>;
}

/// A plugin instance bound to the document.
pub trait Instance {
    fn extension(&self) -> sys::PlugInExtensionInstance;
    /// Sets the instance up to process offline at `rate`. Returns its latency in frames.
    fn prepare(&mut self, rate: f64, max_block: usize) -> Result<u32, Error>;
    fn set_transport(&mut self, playing: bool, position: i64);
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
}

/// Makes the binary, on the ARA thread.
pub type Loader = Box<dyn FnOnce() -> Result<Box<dyn Binary>, Error> + Send>;

/// The song as the plugin sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub rate: f64,
    pub bpm: f64,
    pub numerator: i32,
    pub denominator: i32,
    pub clips: Vec<ClipSpec>,
}

impl Default for Song {
    fn default() -> Song {
        Song {
            rate: 48_000.0,
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
            clips: Vec::new(),
        }
    }
}

/// One clip with ARA.
#[derive(Debug, Clone)]
pub struct ClipSpec {
    pub key: u64,
    /// The clip this one was duplicated from: its modification is cloned with its edits.
    pub clone_of: Option<u64>,
    /// The persistent id of the clip's audio source (one per file, see [`source_id`]).
    pub source: String,
    pub name: String,
    /// The file's samples at the song's rate.
    pub audio: Arc<SourceAudio>,
    /// Seconds on the timeline.
    pub start: f64,
    /// Seconds into the file.
    pub offset: f64,
    pub duration: f64,
}

impl PartialEq for ClipSpec {
    fn eq(&self, o: &ClipSpec) -> bool {
        self.key == o.key
            && self.clone_of == o.clone_of
            && self.source == o.source
            && self.name == o.name
            && Arc::ptr_eq(&self.audio, &o.audio)
            && self.start == o.start
            && self.offset == o.offset
            && self.duration == o.duration
    }
}

/// The persistent id of a file's audio source.
pub fn source_id(file: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in file.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("xirius.source.{h:016x}")
}

/// The persistent id of a clip's modification.
pub fn modification_id(key: u64) -> String {
    format!("xirius.mod.{key}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The plugin is being loaded or the clip is not synced yet.
    Waiting,
    Analysing,
    Rendering,
    Ready,
    Failed,
    Crashed,
}

/// A clip's sound through the plugin, starting at the clip's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub rate: f64,
    /// The clip's length when rendered, in seconds (the buffers may be longer: the tail).
    pub duration: f64,
    pub offset: f64,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipState {
    pub phase: Phase,
    /// The analysis's progress, 0 to 1.
    pub progress: f32,
    pub rendered: Option<Arc<Rendered>>,
}

impl Default for ClipState {
    fn default() -> ClipState {
        ClipState {
            phase: Phase::Waiting,
            progress: 0.0,
            rendered: None,
        }
    }
}

/// What the ARA thread saw, for checks (PLG-031, PLG-034, PLG-038).
#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub info: FactoryInfo,
    pub generation: sys::ApiGeneration,
    pub all_five: bool,
    pub calls: [u64; 5],
    pub reads: u64,
    pub reads_off_thread: u64,
    pub most_readers: i64,
    /// Notifications that came on another thread than the ARA thread.
    pub notices_off_thread: u64,
    pub notices: u64,
    pub asserts: u64,
    pub last_assert: String,
}

enum Msg {
    Sync(Song),
    Store(Sender<Option<(Vec<u8>, String)>>),
    Notes(u64, bool, Sender<Vec<Note>>),
    Probe(Sender<Probe>),
    Stop,
}

struct Shared {
    states: Mutex<HashMap<u64, ClipState>>,
    crashed: Mutex<Option<String>>,
    failed: Mutex<Option<String>>,
    dirty: AtomicBool,
    finished: AtomicBool,
    loaded: AtomicBool,
    generation: AtomicU64,
    requests: Mutex<Vec<Request>>,
    info: Mutex<Option<FactoryInfo>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn changed(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        (self.notify)();
    }
}

/// A call into the plugin may take this long before its thread is ended.
pub const CALL_LIMIT: Duration = Duration::from_secs(30);
/// Frames per render block.
pub const BLOCK: usize = 512;
/// The longest tail rendered after a clip.
pub const MAX_TAIL: f64 = 2.0;

/// One plugin's ARA document on its own thread.
pub struct Service {
    tx: Sender<Msg>,
    shared: Arc<Shared>,
    last: Mutex<Option<Song>>,
}

impl Service {
    /// Starts the thread. `archive` (bytes and archive id) is restored into the first synced
    /// song. `notify` is called (on another thread) when a clip's sound or the document
    /// changed.
    pub fn start(
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Service {
        Service::start_with(loader, archive, notify, CALL_LIMIT)
    }

    pub fn start_with(
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
        limit: Duration,
    ) -> Service {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            states: Mutex::new(HashMap::new()),
            crashed: Mutex::new(None),
            failed: Mutex::new(None),
            dirty: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            requests: Mutex::new(Vec::new()),
            info: Mutex::new(None),
            notify,
        });
        let watch = Watch::new(limit);
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        {
            let mine = shared.clone();
            let watch = watch.clone();
            // Never joined: a contained fault ends this thread without unwinding.
            let spawned = std::thread::Builder::new()
                .name("ara".into())
                .spawn(move || {
                    watch.attach();
                    let _ = ready_tx.send(());
                    run(loader, archive, &mine, &rx);
                    mine.finished.store(true, Ordering::Release);
                });
            if spawned.is_err() {
                *lock(&shared.failed) = Some("could not start the ARA thread".into());
                shared.finished.store(true, Ordering::Release);
            }
        }
        let _ = ready_rx.recv_timeout(Duration::from_secs(5));
        {
            let shared = shared.clone();
            let _ = std::thread::Builder::new()
                .name("ara-watch".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(50));
                        let seen = watch.check();
                        if shared.finished.load(Ordering::Acquire) {
                            return;
                        }
                        let why = match seen {
                            Seen::Alive => continue,
                            Seen::Ended => format!(
                                "the plugin stopped with an {}",
                                contain::describe(contain::last_code())
                            ),
                            Seen::Hung => "the plugin stopped answering".to_string(),
                        };
                        *lock(&shared.crashed) = Some(why);
                        for s in lock(&shared.states).values_mut() {
                            s.phase = Phase::Crashed;
                            s.rendered = None;
                        }
                        shared.finished.store(true, Ordering::Release);
                        shared.changed();
                        return;
                    }
                });
        }
        Service {
            tx,
            shared,
            last: Mutex::new(None),
        }
    }

    /// Hands the song over. Nothing is sent when it did not change.
    pub fn sync(&self, song: &Song) {
        let mut last = lock(&self.last);
        if last.as_ref() == Some(song) {
            return;
        }
        *last = Some(song.clone());
        {
            // Every clip has a state from now on, so a crash before the first render still
            // shows on it.
            let mut states = lock(&self.shared.states);
            let crashed = self.crashed().is_some();
            for c in &song.clips {
                let state = states.entry(c.key).or_default();
                if crashed {
                    state.phase = Phase::Crashed;
                }
            }
        }
        let _ = self.tx.send(Msg::Sync(song.clone()));
    }

    /// The plugin's archive of the document and its archive id, or `None` when the plugin
    /// failed, crashed or did not answer within `limit`.
    pub fn store(&self, limit: Duration) -> Option<(Vec<u8>, String)> {
        if self.crashed().is_some() {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::Store(tx)).ok()?;
        rx.recv_timeout(limit).ok().flatten()
    }

    /// The notes of a clip's source (or of its modification, with the edits).
    pub fn notes(&self, key: u64, of_modification: bool, limit: Duration) -> Vec<Note> {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Msg::Notes(key, of_modification, tx)).is_err() {
            return Vec::new();
        }
        rx.recv_timeout(limit).unwrap_or_default()
    }

    pub fn probe(&self, limit: Duration) -> Option<Probe> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Msg::Probe(tx)).ok()?;
        rx.recv_timeout(limit).ok()
    }

    pub fn state(&self, key: u64) -> Option<ClipState> {
        lock(&self.shared.states).get(&key).cloned()
    }

    pub fn states(&self) -> HashMap<u64, ClipState> {
        lock(&self.shared.states).clone()
    }

    /// Waits until every synced clip is ready or failed, at most to `deadline` (an export
    /// waits for the analysis, ENG-025). Whether all were.
    pub fn wait_idle(&self, deadline: Instant) -> bool {
        loop {
            if self.idle() {
                return true;
            }
            if Instant::now() >= deadline || self.crashed().is_some() || self.failed().is_some() {
                return self.idle();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn idle(&self) -> bool {
        let Some(song) = lock(&self.last).clone() else {
            return true;
        };
        let states = lock(&self.shared.states);
        song.clips.iter().all(|c| {
            states
                .get(&c.key)
                .is_some_and(|s| matches!(s.phase, Phase::Ready | Phase::Failed | Phase::Crashed))
        })
    }

    /// Whether the plugin changed its document since the last call (an edit in its window).
    pub fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }

    /// Why the plugin crashed, if it did.
    pub fn crashed(&self) -> Option<String> {
        lock(&self.shared.crashed).clone()
    }

    /// Why the plugin could not be used (not loaded, no document), if so.
    pub fn failed(&self) -> Option<String> {
        lock(&self.shared.failed).clone()
    }

    pub fn loaded(&self) -> bool {
        self.shared.loaded.load(Ordering::Acquire)
    }

    pub fn info(&self) -> Option<FactoryInfo> {
        lock(&self.shared.info).clone()
    }

    /// Counts every change of the clips' sound.
    pub fn generation(&self) -> u64 {
        self.shared.generation.load(Ordering::Acquire)
    }

    /// The plugin's requests to the transport since the last call.
    pub fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut *lock(&self.shared.requests))
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        // The thread tears the document down; it is not joined (see `start`).
        let until = Instant::now() + Duration::from_secs(5);
        while !self.shared.finished.load(Ordering::Acquire) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

// The ARA thread -----------------------------------------------------------------------------

const CONTEXT_KEY: usize = 1;
const SEQUENCE_KEY: usize = 1;

struct Source {
    key: usize,
    r: Ref,
    props: SourceProps,
    audio: Arc<SourceAudio>,
    analysed: bool,
    progress: f32,
}

struct Clip {
    spec: ClipSpec,
    modification: Ref,
    region: Ref,
    instance: Option<Box<dyn Instance>>,
    prepared: f64,
    rendered: Option<Arc<Rendered>>,
    stale: bool,
    failed: bool,
}

struct Ara {
    doc: Document,
    binary: Box<dyn Binary>,
    side: Arc<HostSide>,
    shared: Arc<Shared>,
    kinds: Vec<sys::ContentType>,
    context: Ref,
    sequence: Ref,
    musical: Musical,
    rate: f64,
    sources: HashMap<String, Source>,
    next_source: usize,
    clips: HashMap<u64, Clip>,
    archive: Option<(Vec<u8>, String)>,
    /// Notifications of the document's data right after a restore are not edits.
    quiet: u32,
    ara_thread: Option<ThreadId>,
    notices: u64,
    notices_off_thread: u64,
}

fn run(
    loader: Loader,
    archive: Option<(Vec<u8>, String)>,
    shared: &Arc<Shared>,
    rx: &Receiver<Msg>,
) {
    let mut ara = match start(loader, archive, shared) {
        Ok(a) => a,
        Err(e) => {
            *lock(&shared.failed) = Some(e.0);
            for s in lock(&shared.states).values_mut() {
                s.phase = Phase::Failed;
            }
            shared.changed();
            // Answer until stopped, so callers do not wait for their limits.
            while let Ok(m) = rx.recv() {
                match m {
                    Msg::Stop => return,
                    Msg::Sync(song) => {
                        let mut states = lock(&shared.states);
                        for c in &song.clips {
                            states.entry(c.key).or_default().phase = Phase::Failed;
                        }
                    }
                    Msg::Store(tx) => drop(tx.send(None)),
                    Msg::Notes(_, _, tx) => drop(tx.send(Vec::new())),
                    Msg::Probe(tx) => drop(tx.send(Probe::default())),
                }
            }
            return;
        }
    };
    shared.loaded.store(true, Ordering::Release);
    loop {
        let first = match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(m) => Some(m),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Msg::Stop),
        };
        let mut msgs: Vec<Msg> = first.into_iter().collect();
        msgs.extend(rx.try_iter());
        // Only the newest song matters.
        let newest = msgs.iter().rposition(|m| matches!(m, Msg::Sync(_)));
        let mut stop = false;
        for (i, m) in msgs.into_iter().enumerate() {
            match m {
                Msg::Stop => stop = true,
                Msg::Sync(song) if Some(i) == newest => ara.sync(song),
                Msg::Sync(_) => {}
                Msg::Store(tx) => drop(tx.send(ara.store())),
                Msg::Notes(key, of_mod, tx) => drop(tx.send(ara.notes(key, of_mod))),
                Msg::Probe(tx) => drop(tx.send(ara.probe())),
            }
        }
        if stop {
            break;
        }
        ara.tick();
    }
    ara.teardown();
}

fn start(
    loader: Loader,
    archive: Option<(Vec<u8>, String)>,
    shared: &Arc<Shared>,
) -> Result<Ara, Error> {
    let binary = contain::call(loader)?;
    let side = HostSide::new();
    // SAFETY: the binary keeps its module loaded and its factory valid.
    let doc = unsafe { Document::new(binary.factory(), side.clone(), "Xirius")? };
    let info = doc.info();
    *lock(&shared.info) = Some(info.clone());
    Ok(Ara {
        doc,
        binary,
        side,
        shared: shared.clone(),
        kinds: info.analyzeable,
        context: std::ptr::null_mut(),
        sequence: std::ptr::null_mut(),
        musical: Musical::default(),
        rate: 0.0,
        sources: HashMap::new(),
        next_source: 1,
        clips: HashMap::new(),
        archive,
        quiet: 0,
        ara_thread: Some(std::thread::current().id()),
        notices: 0,
        notices_off_thread: 0,
    })
}

fn region_props(c: &ClipSpec) -> RegionProps {
    RegionProps {
        start_in_modification: c.offset,
        duration_in_modification: c.duration,
        start_in_playback: c.start,
        duration_in_playback: c.duration,
    }
}

/// The host reference of a clip's modification and region (never zero).
fn clip_ref(key: u64) -> usize {
    key as usize + 1
}

impl Ara {
    fn sync(&mut self, song: Song) {
        let musical = Musical {
            bpm: song.bpm,
            numerator: song.numerator,
            denominator: song.denominator,
        };
        if self.rate != 0.0 && self.rate != song.rate {
            self.change_rate(&song);
        }
        self.rate = song.rate;

        let wanted: HashMap<u64, &ClipSpec> = song.clips.iter().map(|c| (c.key, c)).collect();
        let wanted_sources: HashSet<&str> = song.clips.iter().map(|c| c.source.as_str()).collect();
        let mut new_sources = Vec::new();
        self.doc.begin_editing();

        if self.context.is_null() {
            self.side.set_musical(CONTEXT_KEY, musical);
            match self.doc.create_musical_context(CONTEXT_KEY, "Song") {
                Ok(r) => self.context = r,
                Err(e) => {
                    self.doc.end_editing();
                    self.fail_all(&song, &e.0);
                    return;
                }
            }
            self.sequence = self
                .doc
                .create_region_sequence(SEQUENCE_KEY, "Track", self.context)
                .unwrap_or(std::ptr::null_mut());
        } else if musical != self.musical {
            self.side.set_musical(CONTEXT_KEY, musical);
            self.doc.update_musical_context_content(self.context);
        }
        self.musical = musical;

        // Gone clips: the instance first (it renders the region), then region and modification.
        let gone: Vec<u64> = self
            .clips
            .keys()
            .copied()
            .filter(|k| !wanted.contains_key(k))
            .collect();
        for key in gone {
            if let Some(mut c) = self.clips.remove(&key) {
                c.instance = None;
                self.doc.destroy_playback_region(c.region);
                self.doc.destroy_audio_modification(c.modification);
            }
            lock(&self.shared.states).remove(&key);
        }
        let unused: Vec<String> = self
            .sources
            .keys()
            .filter(|s| !wanted_sources.contains(s.as_str()))
            .cloned()
            .collect();
        for id in unused {
            if let Some(s) = self.sources.remove(&id) {
                self.doc.enable_samples_access(s.r, false);
                self.doc.destroy_audio_source(s.r);
                self.side.remove_source(s.key);
            }
        }

        // New and changed clips. A clone's original is made first.
        let mut order: Vec<&ClipSpec> = song.clips.iter().collect();
        order.sort_by_key(|c| c.clone_of.is_some());
        for spec in order {
            if let Err(e) = self.ensure_source(spec, &mut new_sources) {
                self.fail_clip(spec.key, &e.0);
                continue;
            }
            match self.clips.get_mut(&spec.key) {
                Some(c) => {
                    if c.spec == *spec {
                        continue;
                    }
                    let moved_only = c.spec.offset == spec.offset
                        && c.spec.duration == spec.duration
                        && Arc::ptr_eq(&c.spec.audio, &spec.audio);
                    let p = region_props(spec);
                    self.doc
                        .update_playback_region(c.region, &p, self.context, self.sequence);
                    if !moved_only {
                        if c.spec.offset != spec.offset {
                            // The old sound is of another part of the file.
                            c.rendered = None;
                        }
                        c.stale = true;
                    }
                    c.spec = spec.clone();
                }
                None => {
                    if let Err(e) = self.make_clip(spec) {
                        self.fail_clip(spec.key, &e.0);
                    }
                }
            }
        }

        // The archive goes into the objects just made with the same persistent ids.
        if let Some((data, id)) = self.archive.take()
            && !self.clips.is_empty()
        {
            if self.doc.restore(&data, &id) {
                self.quiet = 3;
                for c in self.clips.values_mut() {
                    c.stale = true;
                }
            } else {
                *lock(&self.shared.failed) =
                    Some("the plugin could not restore its archive".into());
            }
        } else if let Some(a) = self.archive.take() {
            // Nothing to restore into yet.
            self.archive = Some(a);
        }
        self.doc.end_editing();

        for id in new_sources {
            if let Some(s) = self.sources.get(&id) {
                self.doc.enable_samples_access(s.r, true);
                self.doc.request_analysis(s.r, &self.kinds);
            }
        }
        self.publish();
    }

    fn ensure_source(&mut self, spec: &ClipSpec, new: &mut Vec<String>) -> Result<(), Error> {
        if let Some(s) = self.sources.get_mut(&spec.source) {
            if !Arc::ptr_eq(&s.audio, &spec.audio) && spec.audio.rate == s.audio.rate {
                // The file changed on disk: the plugin reads it again.
                self.doc.enable_samples_access(s.r, false);
                s.audio = spec.audio.clone();
                s.props.frames = spec.audio.frames() as i64;
                self.side.set_source(s.key, s.audio.clone());
                self.doc.update_audio_source_properties(s.r, &s.props);
                self.doc.update_audio_source_content(s.r);
                self.doc.enable_samples_access(s.r, true);
                s.analysed = false;
                self.doc.request_analysis(s.r, &self.kinds);
            }
            return Ok(());
        }
        let key = self.next_source;
        self.next_source += 1;
        let props = SourceProps {
            name: spec.name.clone(),
            persistent_id: spec.source.clone(),
            frames: spec.audio.frames() as i64,
            rate: spec.audio.rate,
            channels: spec.audio.channels.len() as i32,
        };
        self.side.set_source(key, spec.audio.clone());
        let r = self.doc.create_audio_source(key, &props)?;
        self.sources.insert(
            spec.source.clone(),
            Source {
                key,
                r,
                props,
                audio: spec.audio.clone(),
                analysed: self.kinds.is_empty(),
                progress: 0.0,
            },
        );
        new.push(spec.source.clone());
        Ok(())
    }

    fn make_clip(&mut self, spec: &ClipSpec) -> Result<(), Error> {
        let source = self
            .sources
            .get(&spec.source)
            .ok_or_else(|| error("no source"))?
            .r;
        let key = clip_ref(spec.key);
        let id = modification_id(spec.key);
        let original = spec
            .clone_of
            .and_then(|k| self.clips.get(&k))
            .map(|c| c.modification);
        let modification = match original {
            Some(m) => self.doc.clone_audio_modification(m, key, &spec.name, &id)?,
            None => self
                .doc
                .create_audio_modification(source, key, &spec.name, &id)?,
        };
        let region = match self.doc.create_playback_region(
            modification,
            key,
            &region_props(spec),
            self.context,
            self.sequence,
        ) {
            Ok(r) => r,
            Err(e) => {
                self.doc.destroy_audio_modification(modification);
                return Err(e);
            }
        };
        self.clips.insert(
            spec.key,
            Clip {
                spec: spec.clone(),
                modification,
                region,
                instance: None,
                prepared: 0.0,
                rendered: None,
                stale: true,
                failed: false,
            },
        );
        Ok(())
    }

    /// A new song rate (PLG-047): the plugin's reading stops, each source gets its samples at
    /// the new rate, reading starts again, and the clips render again. They play dry
    /// meanwhile; the instances are set up again at the new rate when they render.
    fn change_rate(&mut self, song: &Song) {
        let audio: HashMap<&str, &Arc<SourceAudio>> = song
            .clips
            .iter()
            .map(|c| (c.source.as_str(), &c.audio))
            .collect();
        for s in self.sources.values() {
            self.doc.enable_samples_access(s.r, false);
        }
        self.doc.begin_editing();
        for (id, s) in self.sources.iter_mut() {
            let Some(a) = audio.get(id.as_str()) else {
                continue;
            };
            s.audio = (*a).clone();
            s.props.rate = a.rate;
            s.props.frames = a.frames() as i64;
            self.side.set_source(s.key, s.audio.clone());
            self.doc.update_audio_source_properties(s.r, &s.props);
        }
        self.doc.end_editing();
        for s in self.sources.values_mut() {
            self.doc.enable_samples_access(s.r, true);
            s.analysed = self.kinds.is_empty();
            self.doc.request_analysis(s.r, &self.kinds);
        }
        for c in self.clips.values_mut() {
            c.stale = true;
        }
    }

    fn tick(&mut self) {
        self.doc.notify_model_updates();
        let here = std::thread::current().id();
        let mut dirty = false;
        for (n, thread) in self.side.take_notices() {
            self.notices += 1;
            if Some(thread) != self.ara_thread || thread != here {
                self.notices_off_thread += 1;
            }
            match n {
                Notice::Analysis {
                    source,
                    state,
                    value,
                } => {
                    if let Some(s) = self.sources.values_mut().find(|s| s.key == source) {
                        s.progress = if state == sys::ANALYSIS_COMPLETED {
                            1.0
                        } else {
                            value.clamp(0.0, 1.0)
                        };
                    }
                }
                Notice::SourceContent { source, .. } => {
                    let id = self
                        .sources
                        .iter()
                        .find(|(_, s)| s.key == source)
                        .map(|(id, _)| id.clone());
                    for c in self.clips.values_mut() {
                        if Some(&c.spec.source) == id.as_ref() {
                            c.stale = true;
                        }
                    }
                }
                Notice::ModificationContent {
                    modification: r, ..
                }
                | Notice::RegionContent { region: r, .. } => {
                    if let Some(c) = self.clips.values_mut().find(|c| clip_ref(c.spec.key) == r) {
                        c.stale = true;
                    }
                }
                Notice::DocumentData => dirty |= self.quiet == 0,
            }
        }
        self.quiet = self.quiet.saturating_sub(1);
        if dirty {
            self.shared.dirty.store(true, Ordering::Release);
        }
        lock(&self.shared.requests).extend(self.side.take_requests());

        let mut changed = dirty;
        for s in self.sources.values_mut() {
            if !s.analysed && self.doc.incomplete(s.r, &self.kinds).is_empty() {
                s.analysed = true;
                s.progress = 1.0;
                changed = true;
            }
        }
        // One clip per turn, so the messages are answered between renders.
        let next = self
            .clips
            .iter()
            .filter(|(_, c)| {
                c.stale && !c.failed && self.sources.get(&c.spec.source).is_some_and(|s| s.analysed)
            })
            .map(|(k, _)| *k)
            .min();
        if let Some(key) = next {
            self.render(key);
            changed = true;
        }
        if changed {
            self.publish();
        }
    }

    fn render(&mut self, key: u64) {
        let rate = self.rate;
        let dc = self.doc.controller_ref();
        let Some(c) = self.clips.get_mut(&key) else {
            return;
        };
        c.stale = false;
        if c.instance.is_none() {
            match self
                .binary
                .instance(dc, sys::ROLES_ALL, sys::ROLE_PLAYBACK_RENDERER)
            {
                Ok(i) => {
                    let ext = i.extension();
                    if ext.playback_renderer_interface.is_null()
                        || ext.playback_renderer_ref.is_null()
                    {
                        c.failed = true;
                        return;
                    }
                    // SAFETY: the renderer of a live instance, before it is set up to process.
                    contain::call(|| unsafe {
                        ((*ext.playback_renderer_interface).add_playback_region)(
                            ext.playback_renderer_ref,
                            c.region,
                        )
                    });
                    c.instance = Some(i);
                    c.prepared = 0.0;
                }
                Err(_) => {
                    c.failed = true;
                    return;
                }
            }
        }
        let instance = c.instance.as_mut().expect("made above");
        if c.prepared != rate {
            if instance.prepare(rate, BLOCK).is_err() {
                c.failed = true;
                return;
            }
            c.prepared = rate;
        }
        let (_, tail) = self.doc.head_and_tail(c.region);
        let spec = &c.spec;
        let frames = (spec.duration * rate).ceil().max(0.0) as usize
            + (tail.min(MAX_TAIL) * rate).ceil() as usize;
        let start = (spec.start * rate).round() as i64;
        let mut left = vec![0.0f32; frames];
        let mut right = vec![0.0f32; frames];
        instance.set_transport(true, start);
        let mut at = 0;
        while at < frames {
            let n = BLOCK.min(frames - at);
            instance.process(&mut left[at..at + n], &mut right[at..at + n]);
            at += n;
        }
        instance.set_transport(false, start + frames as i64);
        c.rendered = Some(Arc::new(Rendered {
            rate,
            duration: spec.duration,
            offset: spec.offset,
            left,
            right,
        }));
    }

    fn publish(&self) {
        let mut states = lock(&self.shared.states);
        for (key, c) in &self.clips {
            let source = self.sources.get(&c.spec.source);
            let analysed = source.is_some_and(|s| s.analysed);
            let phase = if c.failed {
                Phase::Failed
            } else if !analysed {
                Phase::Analysing
            } else if c.stale || c.rendered.is_none() {
                Phase::Rendering
            } else {
                Phase::Ready
            };
            states.insert(
                *key,
                ClipState {
                    phase,
                    progress: source.map_or(0.0, |s| s.progress),
                    rendered: c.rendered.clone(),
                },
            );
        }
        drop(states);
        self.shared.changed();
    }

    fn fail_clip(&self, key: u64, _why: &str) {
        lock(&self.shared.states).entry(key).or_default().phase = Phase::Failed;
    }

    fn fail_all(&self, song: &Song, why: &str) {
        *lock(&self.shared.failed) = Some(why.to_string());
        for c in &song.clips {
            self.fail_clip(c.key, why);
        }
        self.shared.changed();
    }

    fn store(&mut self) -> Option<(Vec<u8>, String)> {
        if let Some(a) = &self.archive {
            // Never synced with clips: the archive is still the one given.
            return Some(a.clone());
        }
        let bytes = self.doc.store()?;
        Some((bytes, self.doc.info().archive_id))
    }

    fn notes(&self, key: u64, of_modification: bool) -> Vec<Note> {
        let Some(c) = self.clips.get(&key) else {
            return Vec::new();
        };
        if of_modification {
            self.doc.notes(c.modification, true)
        } else {
            self.sources
                .get(&c.spec.source)
                .map(|s| self.doc.notes(s.r, false))
                .unwrap_or_default()
        }
    }

    fn probe(&self) -> Probe {
        let calls = &self.side.calls;
        let (asserts, last_assert) = crate::document::asserts();
        Probe {
            info: self.doc.info(),
            generation: self.doc.generation(),
            all_five: calls.all_five(),
            calls: [
                calls.audio_access.load(Ordering::Acquire),
                calls.archiving.load(Ordering::Acquire),
                calls.content_access.load(Ordering::Acquire),
                calls.model_update.load(Ordering::Acquire),
                calls.playback.load(Ordering::Acquire),
            ],
            reads: calls.reads.load(Ordering::Acquire),
            reads_off_thread: calls.reads_off_thread.load(Ordering::Acquire),
            most_readers: calls.most_readers.load(Ordering::Acquire),
            notices_off_thread: self.notices_off_thread,
            notices: self.notices,
            asserts,
            last_assert,
        }
    }

    /// Takes the model down in the order ARA asks: instances, then regions, modifications
    /// and sources, then the context; the document last (when `self` drops).
    fn teardown(&mut self) {
        for c in self.clips.values_mut() {
            c.instance = None;
        }
        for s in self.sources.values() {
            self.doc.enable_samples_access(s.r, false);
        }
        self.doc.begin_editing();
        for (_, c) in self.clips.drain() {
            self.doc.destroy_playback_region(c.region);
            self.doc.destroy_audio_modification(c.modification);
        }
        for (_, s) in self.sources.drain() {
            self.doc.destroy_audio_source(s.r);
        }
        if !self.sequence.is_null() {
            self.doc.destroy_region_sequence(self.sequence);
        }
        if !self.context.is_null() {
            self.doc.destroy_musical_context(self.context);
        }
        self.doc.end_editing();
    }
}
