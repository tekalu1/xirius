//! The ARA thread of one plugin: its documents, their renderers, and the clips' sound
//! (PLG-032, PLG-034 to PLG-037, PLG-047, PLG-058).
//!
//! A [`Service`] is one document on an ARA thread: a thread of its own ([`Service::start`]),
//! or the plugin's one thread that all its documents in the process share
//! ([`Service::start_shared`]; Melodyne faults on a second document made on another thread
//! than its first). Everything the plugin is called with runs there, marked as a call into the
//! plugin ([`crate::contain`]) and watched by a second thread: a contained fault or a call
//! that does not come back ends the ARA thread alone, and its documents report themselves
//! crashed ([`Service::crashed`]). The song's clips then play dry until a service is made
//! again from the last archive.
//!
//! The song is handed over whole ([`Service::sync`]); the thread compares it with the model it
//! made and edits the document in one cycle: sources (one per file), modifications and
//! regions (one each per clip). After the plugin's analysis of a source completes, each clip
//! is rendered offline through its own playback renderer, a second of sound per turn, and the
//! result is handed back ([`Service::state`]). Moving a clip keeps its rendered sound
//! (PLG-036); a new length or a new start in the source renders again. The plugin's
//! notifications are pulled on the ARA thread every 30 ms (PLG-034).
//!
//! Sources are given at their files' own rates, and each clip renders at its source's rate
//! (PLG-047): the song's rate is not the plugin's business, so a new song rate changes
//! nothing in the document and the user's edits stay. The caller converts the sound to the
//! song's rate.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use crate::contain::{self, Seen, Watch};
use crate::document::{Document, FactoryInfo, Note, RegionProps, SourceProps};
use crate::host::{HostSide, Musical, Notice, Request, SourceAudio};
use crate::sys::{self, Ref, RoleFlags};
use crate::{Error, error};
pub use xirius_vst3::{Caption, Placement};

/// A plugin binary with an ARA factory, used on the ARA thread only.
pub trait Binary {
    fn factory(&self) -> *const sys::Factory;
    /// An address in the plugin's module, registered with [`contain::add_module`], so a fault
    /// on a thread of the plugin's own is noticed ([`contain::faults_in`]). 0 for none.
    fn module(&self) -> usize {
        0
    }
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
    /// Opens the instance's editor (its ARA editor view) in a window of its own, on the ARA
    /// thread, which answers its messages.
    fn open_editor(&mut self, _title: &str, _place: Placement) -> Result<Box<dyn Window>, Error> {
        Err(error("the plugin has no editor"))
    }
}

/// A plugin's editor window, on the ARA thread. Dropping it closes it.
pub trait Window {
    /// False once its close box hid it.
    fn is_visible(&self) -> bool;
    /// Brings it above the other windows, without taking the foreground.
    fn raise(&self);
    fn set_caption(&self, _caption: Caption) {}
}

/// The plugin's answer to [`Service::store_later`]: its archive and the archive id, or `None`.
pub type Stored = Receiver<Option<(Vec<u8>, String)>>;

/// Makes the binary, on the ARA thread.
pub type Loader = Box<dyn FnOnce() -> Result<Box<dyn Binary>, Error> + Send>;

/// The song as the plugin sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub bpm: f64,
    pub numerator: i32,
    pub denominator: i32,
    pub clips: Vec<ClipSpec>,
}

impl Default for Song {
    fn default() -> Song {
        Song {
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
    /// The file's samples at its own rate.
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
    /// The source's rate, which the clip was rendered at.
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
    /// The song and its number (see [`Shared::synced`]).
    Sync(Song, u64),
    Store(Sender<Option<(Vec<u8>, String)>>),
    Notes(u64, bool, Sender<Vec<Note>>),
    Probe(Sender<Probe>),
    /// Opens the editor showing these clips, or raises it when it is open.
    Edit {
        title: String,
        place: Placement,
        clips: Vec<u64>,
        /// The answer; without one, a failure is kept for [`Service::take_editor_error`].
        tx: Option<Sender<Result<(), String>>>,
    },
    Caption(Caption),
    CloseEditor,
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
    /// The number of the last song the ARA thread took in. Until it is the last one sent, the
    /// states may be of an older song.
    synced: AtomicU64,
    requests: Mutex<Vec<Request>>,
    info: Mutex<Option<FactoryInfo>>,
    /// The plugin's editor window is open.
    editing: AtomicBool,
    /// Why the editor asked for without waiting did not open.
    editor_error: Mutex<Option<String>>,
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
/// Seconds of sound rendered per turn of the ARA thread, so its messages are answered between.
pub const RENDER_STEP: f64 = 1.0;
/// The longest tail rendered after a clip.
pub const MAX_TAIL: f64 = 2.0;

/// One plugin's ARA document, on an ARA thread.
pub struct Service {
    id: u64,
    tx: Sender<Job>,
    host: Arc<Host>,
    shared: Arc<Shared>,
    /// The last song sent and its number.
    last: Mutex<(Option<Song>, u64)>,
}

impl Service {
    /// Starts the document on a thread of its own. `archive` (bytes and archive id) is
    /// restored into the first synced song. `notify` is called (on another thread) when a
    /// clip's sound or the document changed.
    pub fn start(
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Service {
        Service::start_with(loader, archive, notify, CALL_LIMIT)
    }

    /// As [`Service::start`], with calls into the plugin limited to `limit`.
    pub fn start_with(
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
        limit: Duration,
    ) -> Service {
        Service::open(Host::spawn(None, limit), loader, archive, notify)
    }

    /// Starts the document on the ARA thread of `key` (the plugin), which every document of
    /// that plugin in the process shares: plugins (Melodyne among them) keep to the thread
    /// their first document was made on, and a second document on another thread faults.
    /// The thread stays to the end of the process, or until a fault or a hang ends it; then
    /// its documents are crashed and the next document starts a new thread.
    pub fn start_shared(
        key: &str,
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Service {
        Service::open(Host::shared(key), loader, archive, notify)
    }

    fn open(
        host: Result<(Arc<Host>, Sender<Job>), String>,
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        notify: Arc<dyn Fn() + Send + Sync>,
    ) -> Service {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let shared = Arc::new(Shared {
            states: Mutex::new(HashMap::new()),
            crashed: Mutex::new(None),
            failed: Mutex::new(None),
            dirty: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            loaded: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            synced: AtomicU64::new(0),
            requests: Mutex::new(Vec::new()),
            info: Mutex::new(None),
            editing: AtomicBool::new(false),
            editor_error: Mutex::new(None),
            notify,
        });
        let (host, tx) = match host {
            Ok(h) => h,
            Err(why) => {
                *lock(&shared.failed) = Some(why);
                shared.finished.store(true, Ordering::Release);
                // A host that is already ended: every call answers at once.
                let (tx, _) = mpsc::channel();
                (Host::ended(), tx)
            }
        };
        host.add(&shared);
        let sent = tx.send(Job::Open {
            id,
            loader,
            archive,
            shared: shared.clone(),
        });
        if sent.is_err() && !shared.finished.load(Ordering::Acquire) {
            *lock(&shared.failed) = Some("the ARA thread has ended".into());
            shared.finished.store(true, Ordering::Release);
        }
        Service {
            id,
            tx,
            host,
            shared,
            last: Mutex::new((None, 0)),
        }
    }

    fn send(&self, m: Msg) -> bool {
        self.tx.send(Job::To(self.id, m)).is_ok()
    }

    /// Hands the song over. Nothing is sent when it did not change.
    pub fn sync(&self, song: &Song) {
        let mut last = lock(&self.last);
        if last.0.as_ref() == Some(song) {
            return;
        }
        last.0 = Some(song.clone());
        last.1 += 1;
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
        self.send(Msg::Sync(song.clone(), last.1));
    }

    /// The plugin's archive of the document and its archive id, or `None` when the plugin
    /// failed, crashed or did not answer within `limit`.
    pub fn store(&self, limit: Duration) -> Option<(Vec<u8>, String)> {
        self.store_later()?.recv_timeout(limit).ok().flatten()
    }

    /// Asks the plugin for its archive without waiting: the answer of [`Service::store`] comes
    /// on the receiver when the plugin's thread has it. `None` when it crashed or is gone.
    pub fn store_later(&self) -> Option<Stored> {
        if self.crashed().is_some() {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Store(tx)).then_some(rx)
    }

    /// The notes of a clip's source (or of its modification, with the edits).
    pub fn notes(&self, key: u64, of_modification: bool, limit: Duration) -> Vec<Note> {
        let (tx, rx) = mpsc::channel();
        if !self.send(Msg::Notes(key, of_modification, tx)) {
            return Vec::new();
        }
        rx.recv_timeout(limit).unwrap_or_default()
    }

    pub fn probe(&self, limit: Duration) -> Option<Probe> {
        let (tx, rx) = mpsc::channel();
        if !self.send(Msg::Probe(tx)) {
            return None;
        }
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
        let (Some(song), sent) = lock(&self.last).clone() else {
            return true;
        };
        // The states of an older song say nothing about this one.
        if self.shared.synced.load(Ordering::Acquire) < sent {
            return false;
        }
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

    /// Opens the plugin's editor showing `clips` (by key), or raises it when it is open: one
    /// window per document. It closes by its own close box, by [`Service::close_editor`] or
    /// with the document; [`Service::editing`] tells. Waits for the ARA thread at most `limit`.
    pub fn open_editor(
        &self,
        title: &str,
        place: Placement,
        clips: &[u64],
        limit: Duration,
    ) -> Result<(), String> {
        if let Some(why) = self.crashed().or_else(|| self.failed()) {
            return Err(why);
        }
        let (tx, rx) = mpsc::channel();
        let sent = self.send(Msg::Edit {
            title: title.to_string(),
            place,
            clips: clips.to_vec(),
            tx: Some(tx),
        });
        if !sent {
            return Err("the ARA thread has ended".into());
        }
        rx.recv_timeout(limit)
            .unwrap_or_else(|_| Err("the plugin did not open its editor in time".into()))
    }

    /// As [`Service::open_editor`], without waiting (from the UI thread): a failure shows in
    /// [`Service::take_editor_error`].
    pub fn request_editor(
        &self,
        title: &str,
        place: Placement,
        clips: &[u64],
    ) -> Result<(), String> {
        if let Some(why) = self.crashed().or_else(|| self.failed()) {
            return Err(why);
        }
        let sent = self.send(Msg::Edit {
            title: title.to_string(),
            place,
            clips: clips.to_vec(),
            tx: None,
        });
        if sent {
            Ok(())
        } else {
            Err("the ARA thread has ended".into())
        }
    }

    /// Why the editor last asked for with [`Service::request_editor`] did not open, once.
    pub fn take_editor_error(&self) -> Option<String> {
        lock(&self.shared.editor_error).take()
    }

    /// Whether the plugin's editor window is open.
    pub fn editing(&self) -> bool {
        self.shared.editing.load(Ordering::Acquire)
    }

    /// The editor's caption colours, as the app's theme changed.
    pub fn set_editor_caption(&self, caption: Caption) {
        self.send(Msg::Caption(caption));
    }

    pub fn close_editor(&self) {
        self.send(Msg::CloseEditor);
    }

    /// The plugin's requests to the transport since the last call.
    pub fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut *lock(&self.shared.requests))
    }

    /// Closes the document and waits, at most to `deadline`, until the ARA thread took it
    /// down. Whether it did. Dropping a service closes it without waiting.
    pub fn close(self, deadline: Instant) -> bool {
        self.send(Msg::Stop);
        loop {
            if self.shared.finished.load(Ordering::Acquire) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        // The ARA thread tears the document down in its next turn; nothing here waits for
        // it (the UI thread drops services).
        self.send(Msg::Stop);
        self.host.forget(&self.shared);
    }
}

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
    /// The render under way, a step per turn.
    pass: Option<Pass>,
    stale: bool,
    failed: bool,
}

/// A render under way: its buffers, filled up to `at`.
struct Pass {
    rate: f64,
    start: i64,
    at: usize,
    duration: f64,
    offset: f64,
    left: Vec<f32>,
    right: Vec<f32>,
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
    sources: HashMap<String, Source>,
    next_source: usize,
    clips: HashMap<u64, Clip>,
    archive: Option<(Vec<u8>, String)>,
    /// Notifications of the document's data right after a restore are not edits.
    quiet: u32,
    ara_thread: Option<ThreadId>,
    notices: u64,
    notices_off_thread: u64,
    /// The plugin's module and the faults on its own threads when the document was made.
    module: usize,
    module_faults: u64,
    editor: Option<EditorView>,
}

/// The plugin's editor: its window and the instance in the editor view role behind it.
struct EditorView {
    // Dropped before the instance it shows.
    window: Box<dyn Window>,
    instance: Box<dyn Instance>,
}

// The ARA threads ------------------------------------------------------------------------------

/// What an ARA thread is asked.
enum Job {
    Open {
        id: u64,
        loader: Loader,
        archive: Option<(Vec<u8>, String)>,
        shared: Arc<Shared>,
    },
    To(u64, Msg),
}

/// An ARA thread and the documents on it, as seen from the other threads.
struct Host {
    services: Mutex<Vec<Weak<Shared>>>,
    ended: AtomicBool,
}

/// The shared ARA threads, by plugin. Their senders keep them running.
type SharedHost = (String, Arc<Host>, Sender<Job>);
static SHARED_HOSTS: Mutex<Vec<SharedHost>> = Mutex::new(Vec::new());

impl Host {
    fn shared(key: &str) -> Result<(Arc<Host>, Sender<Job>), String> {
        let mut hosts = lock(&SHARED_HOSTS);
        hosts.retain(|(_, h, _)| !h.ended.load(Ordering::Acquire));
        if let Some((_, h, tx)) = hosts.iter().find(|(k, _, _)| k == key) {
            return Ok((h.clone(), tx.clone()));
        }
        let (h, tx) = Host::spawn(Some(key), CALL_LIMIT)?;
        hosts.push((key.to_string(), h.clone(), tx.clone()));
        Ok((h, tx))
    }

    /// Starts an ARA thread and its watch. A thread of its own ends with its last sender.
    fn spawn(key: Option<&str>, limit: Duration) -> Result<(Arc<Host>, Sender<Job>), String> {
        let host = Arc::new(Host {
            services: Mutex::new(Vec::new()),
            ended: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::channel();
        let watch = Watch::new(limit);
        let (ready_tx, ready_rx) = mpsc::channel::<()>();
        {
            let watch = watch.clone();
            let host = host.clone();
            // Never joined: a contained fault ends this thread without unwinding.
            std::thread::Builder::new()
                .name(match key {
                    Some(k) => format!("ara {k}"),
                    None => "ara".into(),
                })
                .spawn(move || {
                    watch.attach();
                    thread_init();
                    let _ = ready_tx.send(());
                    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        host_loop(&rx, &host)
                    }));
                    if host.ended.swap(true, Ordering::AcqRel) {
                        return;
                    }
                    if run.is_err() {
                        // Not the plugin's fault: an error of the host's own on this thread.
                        host.crash("the ARA thread stopped on an error of Xirius's");
                    }
                })
                .map_err(|_| "could not start the ARA thread".to_string())?;
        }
        let _ = ready_rx.recv_timeout(Duration::from_secs(5));
        {
            let host = host.clone();
            let _ = std::thread::Builder::new()
                .name("ara-watch".into())
                .spawn(move || {
                    loop {
                        std::thread::sleep(Duration::from_millis(50));
                        let seen = watch.check();
                        if host.ended.load(Ordering::Acquire) {
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
                        host.ended.store(true, Ordering::Release);
                        host.crash(&why);
                        return;
                    }
                });
        }
        Ok((host, tx))
    }

    /// The host of a service that could not start.
    fn ended() -> Arc<Host> {
        Arc::new(Host {
            services: Mutex::new(Vec::new()),
            ended: AtomicBool::new(true),
        })
    }

    fn add(&self, shared: &Arc<Shared>) {
        let mut all = lock(&self.services);
        all.retain(|w| w.strong_count() > 0);
        all.push(Arc::downgrade(shared));
    }

    fn forget(&self, shared: &Arc<Shared>) {
        lock(&self.services)
            .retain(|w| w.strong_count() > 0 && !std::ptr::eq(w.as_ptr(), Arc::as_ptr(shared)));
    }

    /// The thread ended in the plugin: every document on it is crashed.
    fn crash(&self, why: &str) {
        let all: Vec<Arc<Shared>> = lock(&self.services)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for shared in all {
            if shared.finished.load(Ordering::Acquire) {
                continue;
            }
            *lock(&shared.crashed) = Some(why.to_string());
            for s in lock(&shared.states).values_mut() {
                s.phase = Phase::Crashed;
                s.rendered = None;
            }
            shared.finished.store(true, Ordering::Release);
            shared.changed();
        }
    }
}

/// The ARA thread is the plugin's UI thread too (as the VST3 owner's): OLE, and its window
/// messages answered.
fn thread_init() {
    #[cfg(windows)]
    // SAFETY: initializes this thread once; it is never uninitialized (the thread may end by a
    // contained fault).
    unsafe {
        windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null());
    }
}

fn pump() {
    #[cfg(windows)]
    // SAFETY: a standard PeekMessage loop on a zeroed MSG, bounded.
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
        };
        let mut msg: MSG = std::mem::zeroed();
        for _ in 0..64 {
            if PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) == 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// A document on the ARA thread, or the place of one that could not be made.
enum Slot {
    Open(Box<Ara>),
    Failed(Arc<Shared>),
}

impl Slot {
    fn shared(&self) -> &Arc<Shared> {
        match self {
            Slot::Open(a) => &a.shared,
            Slot::Failed(s) => s,
        }
    }

    /// Handles one cycle's messages. Whether the document was stopped.
    fn handle(&mut self, msgs: Vec<Msg>) -> bool {
        // Only the newest song matters.
        let newest = msgs.iter().rposition(|m| matches!(m, Msg::Sync(..)));
        let mut stop = false;
        for (i, m) in msgs.into_iter().enumerate() {
            match (&mut *self, m) {
                (_, Msg::Stop) => stop = true,
                (Slot::Open(ara), Msg::Sync(song, n)) if Some(i) == newest => {
                    ara.sync(song);
                    ara.shared.synced.fetch_max(n, Ordering::AcqRel);
                }
                (Slot::Failed(shared), Msg::Sync(song, n)) => {
                    let mut states = lock(&shared.states);
                    for c in &song.clips {
                        states.entry(c.key).or_default().phase = Phase::Failed;
                    }
                    shared.synced.fetch_max(n, Ordering::AcqRel);
                }
                (_, Msg::Sync(..)) => {}
                (Slot::Open(ara), Msg::Store(tx)) => drop(tx.send(ara.store())),
                (Slot::Open(ara), Msg::Notes(key, of_mod, tx)) => {
                    drop(tx.send(ara.notes(key, of_mod)))
                }
                (Slot::Open(ara), Msg::Probe(tx)) => drop(tx.send(ara.probe())),
                (
                    Slot::Open(ara),
                    Msg::Edit {
                        title,
                        place,
                        clips,
                        tx,
                    },
                ) => {
                    let opened = ara.edit(&title, place, &clips);
                    answer_edit(&ara.shared, tx, opened);
                }
                (Slot::Open(ara), Msg::Caption(c)) => {
                    if let Some(e) = &ara.editor {
                        e.window.set_caption(c);
                    }
                }
                (Slot::Open(ara), Msg::CloseEditor) => ara.close_editor(),
                (Slot::Failed(shared), Msg::Edit { tx, .. }) => {
                    let why = lock(&shared.failed).clone();
                    let why = why.unwrap_or_else(|| "the plugin failed".into());
                    answer_edit(shared, tx, Err(why));
                }
                (Slot::Failed(_), Msg::Caption(_) | Msg::CloseEditor) => {}
                (Slot::Failed(_), Msg::Store(tx)) => drop(tx.send(None)),
                (Slot::Failed(_), Msg::Notes(_, _, tx)) => drop(tx.send(Vec::new())),
                (Slot::Failed(_), Msg::Probe(tx)) => drop(tx.send(Probe::default())),
            }
        }
        stop
    }

    fn close(self) {
        let shared = self.shared().clone();
        if let Slot::Open(mut ara) = self {
            ara.teardown();
        }
        shared.finished.store(true, Ordering::Release);
    }
}

fn answer_edit(shared: &Shared, tx: Option<Sender<Result<(), String>>>, r: Result<(), String>) {
    match tx {
        Some(tx) => drop(tx.send(r)),
        None => {
            if let Err(why) = r {
                *lock(&shared.editor_error) = Some(why);
                shared.changed();
            }
        }
    }
}

fn open(loader: Loader, archive: Option<(Vec<u8>, String)>, shared: Arc<Shared>) -> Slot {
    match start(loader, archive, &shared) {
        Ok(a) => {
            shared.loaded.store(true, Ordering::Release);
            Slot::Open(Box::new(a))
        }
        Err(e) => {
            *lock(&shared.failed) = Some(e.0);
            for s in lock(&shared.states).values_mut() {
                s.phase = Phase::Failed;
            }
            shared.changed();
            Slot::Failed(shared)
        }
    }
}

/// The ARA thread: its documents, each edited, asked and rendered in turn, and the plugins'
/// window messages answered between.
fn host_loop(rx: &Receiver<Job>, host: &Host) {
    let mut slots: Vec<(u64, Slot)> = Vec::new();
    let mut busy = false;
    let mut windows = false;
    loop {
        pump();
        // While a render is under way, the next turn comes at once; while an editor is open,
        // soon, so its window answers.
        let wait = if busy {
            Duration::ZERO
        } else if windows {
            Duration::from_millis(5)
        } else {
            Duration::from_millis(30)
        };
        let first = match rx.recv_timeout(wait) {
            Ok(j) => Some(j),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let mut msgs: HashMap<u64, Vec<Msg>> = HashMap::new();
        for job in first.into_iter().chain(rx.try_iter()) {
            match job {
                Job::Open {
                    id,
                    loader,
                    archive,
                    shared,
                } => slots.push((id, open(loader, archive, shared))),
                Job::To(id, m) => msgs.entry(id).or_default().push(m),
            }
        }
        // Messages for documents already closed are dropped, and so are their answers.
        busy = false;
        windows = false;
        let mut i = 0;
        while i < slots.len() {
            let mine = msgs.remove(&slots[i].0).unwrap_or_default();
            if slots[i].1.handle(mine) {
                slots.remove(i).1.close();
                continue;
            }
            if let Slot::Open(ara) = &mut slots[i].1 {
                busy |= ara.tick();
                windows |= ara.editor.is_some();
            }
            i += 1;
        }
        if let Some(why) = own_thread_fault(&slots) {
            // A thread of the plugin's own ended in a fault (PLG-058): its state is not to be
            // trusted, so its documents are left as they are, not taken down.
            host.ended.store(true, Ordering::Release);
            host.crash(&why);
            std::mem::forget(slots);
            return;
        }
    }
    for (_, slot) in slots {
        slot.close();
    }
}

/// Why a document's plugin is crashed, when a thread of its own faulted since it was made.
fn own_thread_fault(slots: &[(u64, Slot)]) -> Option<String> {
    slots.iter().find_map(|(_, s)| match s {
        Slot::Open(a) if a.module != 0 && contain::faults_in(a.module) > a.module_faults => {
            Some(format!(
                "the plugin stopped with an {} on a thread of its own",
                contain::describe(contain::last_code())
            ))
        }
        _ => None,
    })
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
    let module = binary.module();
    Ok(Ara {
        doc,
        binary,
        side,
        shared: shared.clone(),
        kinds: info.analyzeable,
        context: std::ptr::null_mut(),
        sequence: std::ptr::null_mut(),
        musical: Musical::default(),
        sources: HashMap::new(),
        next_source: 1,
        clips: HashMap::new(),
        archive,
        quiet: 0,
        ara_thread: Some(std::thread::current().id()),
        notices: 0,
        notices_off_thread: 0,
        module,
        module_faults: contain::faults_in(module),
        editor: None,
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
                    let same_audio = Arc::ptr_eq(&c.spec.audio, &spec.audio);
                    let moved_only = c.spec.offset == spec.offset
                        && c.spec.duration == spec.duration
                        && same_audio;
                    let p = region_props(spec);
                    self.doc
                        .update_playback_region(c.region, &p, self.context, self.sequence);
                    if !moved_only {
                        if !same_audio {
                            // The old sound is of another file's content.
                            c.rendered = None;
                        }
                        // A new start or length in the file keeps the old sound until the new
                        // one is rendered: the caller plays what of it still fits (PLG-036).
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

        // The archive goes into the objects just made with the same persistent ids; kept while
        // there are none yet (a song whose files are still being read).
        if !self.clips.is_empty()
            && let Some((data, id)) = self.archive.take()
        {
            // Only an archive of this factory's own kind (or one it says it reads) is handed
            // over; a song's archive of another plugin never reaches it.
            let info = self.doc.info();
            let readable = id == info.archive_id || info.compatible_archive_ids.contains(&id);
            if !readable {
                *lock(&self.shared.failed) = Some(format!(
                    "the archive is of another kind ({id}) than the plugin reads"
                ));
            } else if self.doc.restore(&data, &id) {
                self.quiet = 3;
                for c in self.clips.values_mut() {
                    c.stale = true;
                }
            } else {
                *lock(&self.shared.failed) =
                    Some("the plugin could not restore its archive".into());
            }
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
            if !Arc::ptr_eq(&s.audio, &spec.audio) {
                // The file changed on disk: the plugin reads it again.
                self.doc.enable_samples_access(s.r, false);
                s.audio = spec.audio.clone();
                s.props.frames = spec.audio.frames() as i64;
                s.props.rate = spec.audio.rate;
                s.props.channels = spec.audio.channels.len() as i32;
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
                pass: None,
                stale: true,
                failed: false,
            },
        );
        Ok(())
    }

    /// One turn: the plugin's notifications, the analysis, and a step of a render. Whether a
    /// render is under way.
    fn tick(&mut self) -> bool {
        self.watch_editor();
        self.doc.notify_model_updates();
        let here = std::thread::current().id();
        let mut dirty = false;
        // A clip the plugin changed shows as rendering at once, so a wait for its sound does
        // not end on the old one.
        let mut restale = false;
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
                            restale = true;
                        }
                    }
                }
                Notice::ModificationContent {
                    modification: r, ..
                }
                | Notice::RegionContent { region: r, .. } => {
                    if let Some(c) = self.clips.values_mut().find(|c| clip_ref(c.spec.key) == r) {
                        c.stale = true;
                        restale = true;
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

        let mut changed = dirty || restale;
        for s in self.sources.values_mut() {
            if !s.analysed && self.doc.incomplete(s.r, &self.kinds).is_empty() {
                s.analysed = true;
                s.progress = 1.0;
                changed = true;
            }
        }
        // A render that a change made old starts again.
        for c in self.clips.values_mut() {
            if c.stale
                && let Some(p) = c.pass.take()
                && let Some(i) = c.instance.as_mut()
            {
                i.set_transport(false, p.start + p.at as i64);
                changed = true;
            }
        }
        // One clip at a time, a step per turn, so the messages are answered between.
        let next = self
            .clips
            .iter()
            .filter(|(_, c)| c.pass.is_some())
            .map(|(k, _)| *k)
            .min()
            .or_else(|| {
                self.clips
                    .iter()
                    .filter(|(_, c)| {
                        c.stale
                            && !c.failed
                            && self.sources.get(&c.spec.source).is_some_and(|s| s.analysed)
                    })
                    .map(|(k, _)| *k)
                    .min()
            });
        if let Some(key) = next {
            changed |= self.render(key);
        }
        if changed {
            self.publish();
        }
        self.clips.values().any(|c| c.pass.is_some())
    }

    /// Starts the clip's render, or goes on with it for a step. Whether the clip's state
    /// changed (its render ended or failed).
    fn render(&mut self, key: u64) -> bool {
        let dc = self.doc.controller_ref();
        let Some(c) = self.clips.get_mut(&key) else {
            return false;
        };
        if c.pass.is_none() {
            // The clip renders at its source's rate: the plugin never converts (PLG-047).
            let Some(rate) = self.sources.get(&c.spec.source).map(|s| s.audio.rate) else {
                c.failed = true;
                return true;
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
                            return true;
                        }
                        // SAFETY: the renderer of a live instance, before it is set up to
                        // process.
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
                        return true;
                    }
                }
            }
            let instance = c.instance.as_mut().expect("made above");
            if c.prepared != rate {
                if instance.prepare(rate, BLOCK).is_err() {
                    c.failed = true;
                    return true;
                }
                c.prepared = rate;
            }
            let (_, tail) = self.doc.head_and_tail(c.region);
            let spec = &c.spec;
            let frames = (spec.duration * rate).ceil().max(0.0) as usize
                + (tail.min(MAX_TAIL) * rate).ceil() as usize;
            let start = (spec.start * rate).round() as i64;
            instance.set_transport(true, start);
            c.pass = Some(Pass {
                rate,
                start,
                at: 0,
                duration: spec.duration,
                offset: spec.offset,
                left: vec![0.0; frames],
                right: vec![0.0; frames],
            });
        }
        let (Some(p), Some(instance)) = (c.pass.as_mut(), c.instance.as_mut()) else {
            return false;
        };
        let frames = p.left.len();
        let end = frames.min(p.at + (RENDER_STEP * p.rate).ceil() as usize);
        while p.at < end {
            let n = BLOCK.min(end - p.at);
            instance.process(&mut p.left[p.at..p.at + n], &mut p.right[p.at..p.at + n]);
            p.at += n;
        }
        if p.at < frames {
            return false;
        }
        instance.set_transport(false, p.start + frames as i64);
        let p = c.pass.take().expect("looked at above");
        c.rendered = Some(Arc::new(Rendered {
            rate: p.rate,
            duration: p.duration,
            offset: p.offset,
            left: p.left,
            right: p.right,
        }));
        true
    }

    fn edit(&mut self, title: &str, place: Placement, keys: &[u64]) -> Result<(), String> {
        if self.editor.is_none() {
            let dc = self.doc.controller_ref();
            let mut instance = self
                .binary
                .instance(dc, sys::ROLES_ALL, sys::ROLE_EDITOR_VIEW)
                .map_err(|e| e.0)?;
            let window = contain::call(|| instance.open_editor(title, place)).map_err(|e| e.0)?;
            self.editor = Some(EditorView { window, instance });
            self.shared.editing.store(true, Ordering::Release);
            self.shared.changed();
        }
        let e = self.editor.as_ref().expect("made above");
        let regions: Vec<Ref> = keys
            .iter()
            .filter_map(|k| self.clips.get(k).map(|c| c.region))
            .collect();
        let ext = e.instance.extension();
        if !regions.is_empty()
            && !ext.editor_view_interface.is_null()
            && !ext.editor_view_ref.is_null()
        {
            let selection = sys::ViewSelection {
                struct_size: sys::VIEW_SELECTION_SIZE,
                playback_region_refs_count: regions.len(),
                playback_region_refs: regions.as_ptr(),
                region_sequence_refs_count: 0,
                region_sequence_refs: std::ptr::null(),
                time_range: std::ptr::null(),
            };
            // SAFETY: the editor view of a live instance; the selection outlives the call.
            contain::call(|| unsafe {
                ((*ext.editor_view_interface).notify_selection)(ext.editor_view_ref, &selection)
            });
        }
        e.window.raise();
        Ok(())
    }

    /// Takes the editor down once its close box hid it.
    fn watch_editor(&mut self) {
        if self.editor.as_ref().is_some_and(|e| !e.window.is_visible()) {
            self.close_editor();
        }
    }

    fn close_editor(&mut self) {
        if let Some(e) = self.editor.take() {
            contain::call(|| drop(e));
            self.shared.editing.store(false, Ordering::Release);
            self.shared.changed();
        }
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
            } else if c.stale || c.pass.is_some() || c.rendered.is_none() {
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
        self.close_editor();
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
