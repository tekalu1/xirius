//! A small ARA plugin written in Rust, for the host's tests (PLG-031 to PLG-037, PLG-047,
//! PLG-058).
//!
//! It behaves like an ARA plugin where the host can see it:
//!
//! - its analysis reads a source on two threads of its own at once, each with its own reader,
//!   and reports its progress only while the host calls `notifyModelUpdates`;
//! - it finds one note per source, and per modification one note a beat long at the tempo it
//!   read from the musical context through the host's content access;
//! - an edit ([`edit`], like a user in the plugin's window) changes a modification's gain,
//!   tells the host the modification and the document changed, and asks the transport to move
//!   to the region;
//! - its archive is JSON with its archive id; it restores into objects with the same
//!   persistent ids;
//! - its playback renderer plays the regions added to it: the source times the gain.
//!
//! It checks the host against ARA's rules ([`violations`]): a model change outside editing, a
//! region added to a renderer that is set up to process, new source properties while the
//! samples may be read, objects left when the document is destroyed. It can fault or hang
//! ([`Config`]) to test PLG-058.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Barrier, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::service::{Binary, Instance, Loader};
use crate::sys::{self, Bool, ContentType, FALSE, Ref, RoleFlags, TRUE};
use crate::{Error, contain, error};

pub const ARCHIVE_ID: &str = "org.xirius.fake.document.2";
/// An archive id of an older version, which this one still reads.
pub const OLD_ARCHIVE_ID: &str = "org.xirius.fake.document.1";

/// How the plugin misbehaves.
#[derive(Debug, Clone, Copy, Default)]
pub struct Config {
    /// How long the analysis of a source takes, about.
    pub analysis_ms: u64,
    /// Faults on the host's thread when asked to analyse.
    pub fault_on_analysis: bool,
    /// Faults on one of its own analysis threads. The executable the fake is linked into is
    /// then registered as the plugin's module ([`contain::add_module`]): use it in a test
    /// binary that expects no other fault.
    pub fault_on_own_thread: bool,
    /// Never returns from rendering.
    pub hang_in_render: bool,
    /// The tail after each region, in seconds.
    pub tail: f64,
    /// Microseconds each block of rendering takes, spent spinning.
    pub render_spin_us: u64,
}

thread_local! {
    /// The configuration of the next document made on this thread.
    static NEXT: Cell<Config> = const { Cell::new(Config { analysis_ms: 0, fault_on_analysis: false, fault_on_own_thread: false, hang_in_render: false, tail: 0.0, render_spin_us: 0 }) };
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

static VIOLATIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LIVE_DOCUMENTS: AtomicI64 = AtomicI64::new(0);
static EDITS: Mutex<Vec<(String, f32)>> = Mutex::new(Vec::new());

fn violation(what: impl Into<String>) {
    lock(&VIOLATIONS).push(what.into());
}

/// The rules of ARA the host broke, in every document of this process so far.
pub fn violations() -> Vec<String> {
    lock(&VIOLATIONS).clone()
}

/// Documents made and not destroyed.
pub fn live_documents() -> i64 {
    LIVE_DOCUMENTS.load(Ordering::Acquire)
}

/// Sets the gain of the modification with persistent id `modification`, as a user would in
/// the plugin's window. Applied at the next `notifyModelUpdates`.
pub fn edit(modification: &str, gain: f32) {
    lock(&EDITS).push((modification.to_string(), gain));
}

/// The loader of a fake plugin binary for [`crate::Service`].
pub fn loader(config: Config) -> Loader {
    Box::new(move || Ok(Box::new(FakeBinary { config }) as Box<dyn Binary>))
}

// The factory ---------------------------------------------------------------------------------

struct Shared<T>(T);
// SAFETY: the wrapped structs hold pointers to static, immutable data only.
unsafe impl<T> Sync for Shared<T> {}

static ANALYZEABLE: [ContentType; 1] = [sys::CONTENT_NOTES];
static COMPATIBLE: Shared<[sys::PersistentId; 1]> =
    Shared([c"org.xirius.fake.document.1".as_ptr()]);

unsafe extern "C" fn initialize(_config: *const sys::InterfaceConfiguration) {}
unsafe extern "C" fn uninitialize() {}

static FACTORY: Shared<sys::Factory> = Shared(sys::Factory {
    struct_size: std::mem::size_of::<sys::Factory>(),
    lowest_supported_api_generation: sys::API_GENERATION_2_0_FINAL,
    highest_supported_api_generation: sys::API_GENERATION_2_3_FINAL,
    factory_id: c"org.xirius.fake".as_ptr(),
    initialize_ara_with_configuration: initialize,
    uninitialize_ara: uninitialize,
    plug_in_name: c"Fake ARA".as_ptr(),
    manufacturer_name: c"Xirius".as_ptr(),
    information_url: c"".as_ptr(),
    version: c"2.0.0".as_ptr(),
    create_document_controller_with_document: create_document_controller,
    document_archive_id: c"org.xirius.fake.document.2".as_ptr(),
    compatible_document_archive_ids_count: 1,
    compatible_document_archive_ids: &COMPATIBLE.0 as *const sys::PersistentId,
    analyzeable_content_types_count: 1,
    analyzeable_content_types: &ANALYZEABLE as *const ContentType,
    supported_playback_transformation_flags: sys::TRANSFORMATION_NONE,
    supports_storing_audio_file_chunks: FALSE,
});

/// The fake's ARA factory.
pub fn factory() -> *const sys::Factory {
    &FACTORY.0
}

// The document --------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Host(sys::DocumentControllerHostInstance);
// SAFETY: the host's controllers may be called from any of the plugin's threads.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}

impl Host {
    fn audio(&self) -> (Ref, sys::AudioAccessControllerInterface) {
        // SAFETY: the host keeps its interfaces valid for the document's lifetime.
        (self.0.audio_access_controller_host_ref, unsafe {
            *self.0.audio_access_controller_interface
        })
    }
    fn archiving(&self) -> (Ref, sys::ArchivingControllerInterface) {
        // SAFETY: as above.
        (self.0.archiving_controller_host_ref, unsafe {
            *self.0.archiving_controller_interface
        })
    }
    fn content(&self) -> (Ref, sys::ContentAccessControllerInterface) {
        // SAFETY: as above.
        (self.0.content_access_controller_host_ref, unsafe {
            *self.0.content_access_controller_interface
        })
    }
    fn model(&self) -> (Ref, sys::ModelUpdateControllerInterface) {
        // SAFETY: as above.
        (self.0.model_update_controller_host_ref, unsafe {
            *self.0.model_update_controller_interface
        })
    }
    fn playback(&self) -> (Ref, sys::PlaybackControllerInterface) {
        // SAFETY: as above.
        (self.0.playback_controller_host_ref, unsafe {
            *self.0.playback_controller_interface
        })
    }
}

/// One analysis of a source, shared with its two threads.
struct Job {
    cancel: AtomicBool,
    read: AtomicU64,
    total: u64,
    data: Mutex<Vec<Vec<f32>>>,
}

struct Context {
    host: Ref,
    bpm: f64,
}

struct Source {
    host: Ref,
    frames: i64,
    rate: f64,
    channels: i32,
    access: bool,
    wanted: bool,
    job: Option<Arc<Job>>,
    threads: Vec<JoinHandle<()>>,
    started: bool,
    analysed: bool,
    cache: Arc<Vec<Vec<f32>>>,
    peak: f32,
}

struct Modification {
    host: Ref,
    source: usize,
    id: String,
    gain: f32,
}

struct Region {
    host: Ref,
    modification: usize,
    context: usize,
    start_in_modification: f64,
    duration_in_modification: f64,
    start_in_playback: f64,
    duration_in_playback: f64,
    /// Renderers it was added to.
    bound: u32,
}

enum Pending {
    Progress(Ref, i32, f32),
    SourceContent(Ref),
    ModificationContent(Ref),
    RegionContent(Ref),
    DocumentData,
    Position(f64),
}

#[derive(Default)]
struct State {
    editing: bool,
    next: usize,
    contexts: HashMap<usize, Context>,
    sequences: HashMap<usize, ()>,
    sources: HashMap<usize, Source>,
    modifications: HashMap<usize, Modification>,
    regions: HashMap<usize, Region>,
    pending: Vec<Pending>,
}

impl State {
    fn id(&mut self) -> usize {
        self.next += 16;
        0x1000 + self.next
    }

    fn check_editing(&self, what: &str) {
        if !self.editing {
            violation(format!("{what} outside editing"));
        }
    }
}

struct Dc {
    host: Host,
    config: Config,
    instance: sys::DocumentControllerInstance,
    state: Mutex<State>,
}

// SAFETY: `instance` points to the static interface and to the `Dc` itself.
unsafe impl Send for Dc {}
unsafe impl Sync for Dc {}

/// # Safety
/// `r` is a document controller reference this module handed out and not destroyed.
unsafe fn dc<'a>(r: Ref) -> &'a Dc {
    // SAFETY: per the contract; the reference is an `Arc<Dc>` turned into a pointer.
    unsafe { &*(r as *const Dc) }
}

unsafe extern "C" fn create_document_controller(
    host: *const sys::DocumentControllerHostInstance,
    _props: *const sys::DocumentProperties,
) -> *const sys::DocumentControllerInstance {
    // SAFETY: the host passes a live instance; copied by value.
    let host = Host(unsafe { *host });
    let d = Arc::new(Dc {
        host,
        config: NEXT.with(Cell::get),
        instance: sys::DocumentControllerInstance {
            struct_size: std::mem::size_of::<sys::DocumentControllerInstance>(),
            document_controller_ref: std::ptr::null_mut(),
            document_controller_interface: &CONTROLLER.0,
        },
        state: Mutex::new(State::default()),
    });
    let r = Arc::into_raw(d) as *mut Dc;
    // SAFETY: just made; nothing else sees it yet.
    unsafe { (*r).instance.document_controller_ref = r as Ref };
    LIVE_DOCUMENTS.fetch_add(1, Ordering::AcqRel);
    // SAFETY: as above.
    unsafe { &(*r).instance }
}

unsafe extern "C" fn destroy_document_controller(r: Ref) {
    // SAFETY: the host destroys its controller once.
    let d = unsafe { Arc::from_raw(r as *const Dc) };
    {
        let mut s = lock(&d.state);
        let left = s.sources.len()
            + s.modifications.len()
            + s.regions.len()
            + s.contexts.len()
            + s.sequences.len();
        if left > 0 {
            violation(format!(
                "{left} objects left when the document was destroyed"
            ));
        }
        for src in s.sources.values_mut() {
            stop_job(src);
        }
    }
    LIVE_DOCUMENTS.fetch_sub(1, Ordering::AcqRel);
}

unsafe extern "C" fn get_factory(_r: Ref) -> *const sys::Factory {
    factory()
}

unsafe extern "C" fn begin_editing(r: Ref) {
    // SAFETY: a live controller (as in every function below).
    let mut s = lock(&unsafe { dc(r) }.state);
    if s.editing {
        violation("beginEditing while editing");
    }
    s.editing = true;
}

unsafe extern "C" fn end_editing(r: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    if !s.editing {
        violation("endEditing without beginEditing");
    }
    s.editing = false;
}

unsafe extern "C" fn notify_model_updates(r: Ref) {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    let pending = {
        let mut s = lock(&d.state);
        let State {
            sources,
            modifications,
            regions,
            pending,
            ..
        } = &mut *s;
        for src in sources.values_mut() {
            let Some(job) = src.job.clone() else { continue };
            if !src.started {
                src.started = true;
                pending.push(Pending::Progress(src.host, sys::ANALYSIS_STARTED, 0.0));
            }
            let read = job.read.load(Ordering::Acquire);
            if read >= job.total {
                for t in src.threads.drain(..) {
                    let _ = t.join();
                }
                let data = std::mem::take(&mut *lock(&job.data));
                src.peak = data.iter().flatten().fold(0.0f32, |p, v| p.max(v.abs()));
                src.cache = Arc::new(data);
                src.analysed = true;
                src.job = None;
                pending.push(Pending::Progress(src.host, sys::ANALYSIS_COMPLETED, 1.0));
                pending.push(Pending::SourceContent(src.host));
            } else {
                let value = read as f32 / job.total.max(1) as f32;
                pending.push(Pending::Progress(src.host, sys::ANALYSIS_UPDATED, value));
            }
        }
        // Edits made in the plugin's window.
        let mut edits = lock(&EDITS);
        edits.retain(|(id, gain)| {
            let Some((&key, m)) = modifications.iter_mut().find(|(_, m)| m.id == *id) else {
                return true;
            };
            m.gain = *gain;
            pending.push(Pending::ModificationContent(m.host));
            for reg in regions.values().filter(|reg| reg.modification == key) {
                pending.push(Pending::RegionContent(reg.host));
                pending.push(Pending::Position(reg.start_in_playback));
            }
            pending.push(Pending::DocumentData);
            false
        });
        std::mem::take(pending)
    };
    let (mref, model) = d.host.model();
    let (pref, playback) = d.host.playback();
    for p in pending {
        // SAFETY: the host's controllers, called from the thread that called us.
        unsafe {
            match p {
                Pending::Progress(src, state, value) => {
                    (model.notify_audio_source_analysis_progress)(mref, src, state, value)
                }
                Pending::SourceContent(src) => (model.notify_audio_source_content_changed)(
                    mref,
                    src,
                    std::ptr::null(),
                    sys::UPDATE_EVERYTHING,
                ),
                Pending::ModificationContent(m) => (model
                    .notify_audio_modification_content_changed)(
                    mref,
                    m,
                    std::ptr::null(),
                    sys::UPDATE_EVERYTHING,
                ),
                Pending::RegionContent(reg) => (model.notify_playback_region_content_changed)(
                    mref,
                    reg,
                    std::ptr::null(),
                    sys::UPDATE_EVERYTHING,
                ),
                Pending::DocumentData => (model.notify_document_data_changed)(mref),
                Pending::Position(t) => (playback.request_set_playback_position)(pref, t),
            }
        }
    }
}

unsafe extern "C" fn update_document_properties(_r: Ref, _p: *const sys::DocumentProperties) {}

/// Reads the context's tempo through the host's content access.
fn read_tempo(d: &Dc, context: Ref) -> f64 {
    let (cref, c) = d.host.content();
    // SAFETY: the host's controller with a context it made.
    unsafe {
        if (c.is_musical_context_content_available)(cref, context, sys::CONTENT_TEMPO_ENTRIES)
            == FALSE
        {
            return 120.0;
        }
        let _grade =
            (c.get_musical_context_content_grade)(cref, context, sys::CONTENT_TEMPO_ENTRIES);
        let reader = (c.create_musical_context_content_reader)(
            cref,
            context,
            sys::CONTENT_TEMPO_ENTRIES,
            std::ptr::null(),
        );
        if reader.is_null() {
            return 120.0;
        }
        let mut bpm = 120.0;
        if (c.get_content_reader_event_count)(cref, reader) >= 2 {
            let a = *((c.get_content_reader_data_for_event)(cref, reader, 0)
                as *const sys::ContentTempoEntry);
            let b = *((c.get_content_reader_data_for_event)(cref, reader, 1)
                as *const sys::ContentTempoEntry);
            let (dt, dq) = (
                b.time_position - a.time_position,
                b.quarter_position - a.quarter_position,
            );
            if dt > 0.0 {
                bpm = 60.0 * dq / dt;
            }
        }
        (c.destroy_content_reader)(cref, reader);
        bpm
    }
}

unsafe extern "C" fn create_musical_context(
    r: Ref,
    host: Ref,
    _p: *const sys::MusicalContextProperties,
) -> Ref {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    let bpm = read_tempo(d, host);
    let mut s = lock(&d.state);
    s.check_editing("createMusicalContext");
    let id = s.id();
    s.contexts.insert(id, Context { host, bpm });
    id as Ref
}

unsafe extern "C" fn update_musical_context_properties(
    r: Ref,
    _c: Ref,
    _p: *const sys::MusicalContextProperties,
) {
    // SAFETY: as above.
    lock(&unsafe { dc(r) }.state).check_editing("updateMusicalContextProperties");
}

unsafe extern "C" fn update_musical_context_content(
    r: Ref,
    c: Ref,
    _range: *const sys::ContentTimeRange,
    _f: i32,
) {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    let host = {
        let s = lock(&d.state);
        s.check_editing("updateMusicalContextContent");
        s.contexts.get(&(c as usize)).map(|x| x.host)
    };
    if let Some(host) = host {
        let bpm = read_tempo(d, host);
        let mut s = lock(&d.state);
        if let Some(x) = s.contexts.get_mut(&(c as usize)) {
            x.bpm = bpm;
        }
        let changed: Vec<Ref> = s
            .regions
            .values()
            .filter(|reg| reg.context == c as usize)
            .map(|reg| s.modifications[&reg.modification].host)
            .collect();
        for m in changed {
            s.pending.push(Pending::ModificationContent(m));
        }
    }
}

unsafe extern "C" fn destroy_musical_context(r: Ref, c: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("destroyMusicalContext");
    s.contexts.remove(&(c as usize));
}

unsafe extern "C" fn create_audio_source(
    r: Ref,
    host: Ref,
    p: *const sys::AudioSourceProperties,
) -> Ref {
    // SAFETY: as above; the properties live through the call.
    let (d, p) = unsafe { (dc(r), *p) };
    let mut s = lock(&d.state);
    s.check_editing("createAudioSource");
    let id = s.id();
    s.sources.insert(
        id,
        Source {
            host,
            frames: p.sample_count,
            rate: p.sample_rate,
            channels: p.channel_count,
            access: false,
            wanted: false,
            job: None,
            threads: Vec::new(),
            started: false,
            analysed: false,
            cache: Arc::new(Vec::new()),
            peak: 0.0,
        },
    );
    id as Ref
}

unsafe extern "C" fn update_audio_source_properties(
    r: Ref,
    src: Ref,
    p: *const sys::AudioSourceProperties,
) {
    // SAFETY: as above.
    let (d, p) = unsafe { (dc(r), *p) };
    let mut s = lock(&d.state);
    s.check_editing("updateAudioSourceProperties");
    if let Some(src) = s.sources.get_mut(&(src as usize)) {
        if src.access && (src.frames != p.sample_count || src.rate != p.sample_rate) {
            violation("the samples' count or rate changed while they could be read");
        }
        src.frames = p.sample_count;
        src.rate = p.sample_rate;
        src.channels = p.channel_count;
        src.analysed = false;
        src.cache = Arc::new(Vec::new());
    }
}

unsafe extern "C" fn update_audio_source_content(
    r: Ref,
    src: Ref,
    _range: *const sys::ContentTimeRange,
    _f: i32,
) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("updateAudioSourceContent");
    if let Some(src) = s.sources.get_mut(&(src as usize)) {
        src.analysed = false;
    }
}

fn stop_job(src: &mut Source) {
    if let Some(job) = src.job.take() {
        job.cancel.store(true, Ordering::Release);
    }
    // The readers are destroyed before access ends.
    for t in src.threads.drain(..) {
        let _ = t.join();
    }
    src.started = false;
}

fn start_job(d: &Dc, src: &mut Source) {
    let channels = src.channels.max(1) as usize;
    let frames = src.frames.max(0) as u64;
    let job = Arc::new(Job {
        cancel: AtomicBool::new(false),
        read: AtomicU64::new(0),
        total: frames,
        data: Mutex::new(vec![vec![0.0; frames as usize]; channels]),
    });
    src.job = Some(job.clone());
    src.started = false;
    src.wanted = false;
    let barrier = Arc::new(Barrier::new(2));
    let half = frames / 2;
    let ms = d.config.analysis_ms;
    if d.config.fault_on_own_thread {
        // A thread of its own that nothing joins: a thread that ended this way has no result.
        drop(std::thread::spawn(own_fault));
    }
    for (from, to) in [(0, half), (half, frames)] {
        let job = job.clone();
        let barrier = barrier.clone();
        let host = d.host;
        let source = src.host as usize;
        src.threads.push(std::thread::spawn(move || {
            analyse(host, source as Ref, &job, &barrier, from, to, channels, ms);
        }));
    }
}

/// The fault of [`Config::fault_on_own_thread`], on a thread the host never called into.
#[inline(never)]
fn own_fault() {
    // SAFETY: none; the fault PLG-058 is about.
    unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
}

/// One of the two analysis threads: reads `[from, to)` of the source through its own reader.
#[allow(clippy::too_many_arguments)]
fn analyse(
    host: Host,
    source: Ref,
    job: &Job,
    barrier: &Barrier,
    from: u64,
    to: u64,
    channels: usize,
    ms: u64,
) {
    const CHUNK: u64 = 4096;
    let (aref, a) = host.audio();
    // SAFETY: the host's audio access, from a thread of the plugin.
    let reader = unsafe { (a.create_audio_reader_for_source)(aref, source, FALSE) };
    // Both readers live at once.
    barrier.wait();
    if reader.is_null() {
        job.read.fetch_add(to - from, Ordering::AcqRel);
        return;
    }
    let chunks = (to - from).div_ceil(CHUNK).max(1);
    let pause = Duration::from_micros(ms * 1000 / 2 / chunks);
    let mut at = from;
    let mut buffers = vec![vec![0.0f32; CHUNK as usize]; channels];
    while at < to && !job.cancel.load(Ordering::Acquire) {
        let n = CHUNK.min(to - at);
        let ptrs: Vec<*mut c_void> = buffers.iter_mut().map(|b| b.as_mut_ptr().cast()).collect();
        // SAFETY: one buffer per channel, each holding `n` samples.
        unsafe { (a.read_audio_samples)(aref, reader, at as i64, n as i64, ptrs.as_ptr()) };
        {
            let mut data = lock(&job.data);
            for (ch, b) in buffers.iter().enumerate() {
                data[ch][at as usize..(at + n) as usize].copy_from_slice(&b[..n as usize]);
            }
        }
        at += n;
        job.read.fetch_add(n, Ordering::AcqRel);
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
    // SAFETY: the reader made above, destroyed once.
    unsafe { (a.destroy_audio_reader)(aref, reader) };
}

unsafe extern "C" fn enable_audio_source_samples_access(r: Ref, src: Ref, on: Bool) {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    let mut s = lock(&d.state);
    let Some(src) = s.sources.get_mut(&(src as usize)) else {
        return;
    };
    src.access = on != FALSE;
    if src.access {
        if src.wanted && !src.analysed {
            start_job(d, src);
        }
    } else if src.job.is_some() {
        // Analysis is aborted; the host asks again.
        stop_job(src);
    }
}

unsafe extern "C" fn deactivate_for_undo(_r: Ref, _o: Ref, _d: Bool) {}

unsafe extern "C" fn destroy_audio_source(r: Ref, src: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("destroyAudioSource");
    if let Some(mut src) = s.sources.remove(&(src as usize)) {
        if src.access {
            violation("a source destroyed while its samples could be read");
        }
        stop_job(&mut src);
    }
    if s.modifications.values().any(|m| m.source == src as usize) {
        violation("a source destroyed before its modifications");
    }
}

fn add_modification(
    r: Ref,
    source: usize,
    host: Ref,
    p: *const sys::AudioModificationProperties,
    gain: f32,
) -> Ref {
    // SAFETY: as above.
    let (d, p) = unsafe { (dc(r), *p) };
    let mut s = lock(&d.state);
    s.check_editing("createAudioModification");
    let id = s.id();
    s.modifications.insert(
        id,
        Modification {
            host,
            source,
            // SAFETY: the host's string, valid through the call.
            id: unsafe { sys::string(p.persistent_id) },
            gain,
        },
    );
    id as Ref
}

unsafe extern "C" fn create_audio_modification(
    r: Ref,
    src: Ref,
    host: Ref,
    p: *const sys::AudioModificationProperties,
) -> Ref {
    add_modification(r, src as usize, host, p, 1.0)
}

unsafe extern "C" fn clone_audio_modification(
    r: Ref,
    m: Ref,
    host: Ref,
    p: *const sys::AudioModificationProperties,
) -> Ref {
    // SAFETY: as above.
    let original = lock(&unsafe { dc(r) }.state)
        .modifications
        .get(&(m as usize))
        .map(|m| (m.source, m.gain));
    match original {
        Some((source, gain)) => add_modification(r, source, host, p, gain),
        None => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn update_audio_modification_properties(
    r: Ref,
    _m: Ref,
    _p: *const sys::AudioModificationProperties,
) {
    // SAFETY: as above.
    lock(&unsafe { dc(r) }.state).check_editing("updateAudioModificationProperties");
}

unsafe extern "C" fn destroy_audio_modification(r: Ref, m: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("destroyAudioModification");
    s.modifications.remove(&(m as usize));
    if s.regions.values().any(|reg| reg.modification == m as usize) {
        violation("a modification destroyed before its regions");
    }
}

fn region_from(host: Ref, modification: usize, p: &sys::PlaybackRegionProperties) -> Region {
    Region {
        host,
        modification,
        context: p.musical_context_ref as usize,
        start_in_modification: p.start_in_modification_time,
        duration_in_modification: p.duration_in_modification_time,
        start_in_playback: p.start_in_playback_time,
        duration_in_playback: p.duration_in_playback_time,
        bound: 0,
    }
}

unsafe extern "C" fn create_playback_region(
    r: Ref,
    m: Ref,
    host: Ref,
    p: *const sys::PlaybackRegionProperties,
) -> Ref {
    // SAFETY: as above.
    let (d, p) = unsafe { (dc(r), *p) };
    let mut s = lock(&d.state);
    s.check_editing("createPlaybackRegion");
    let id = s.id();
    s.regions.insert(id, region_from(host, m as usize, &p));
    id as Ref
}

unsafe extern "C" fn update_playback_region_properties(
    r: Ref,
    reg: Ref,
    p: *const sys::PlaybackRegionProperties,
) {
    // SAFETY: as above.
    let (d, p) = unsafe { (dc(r), *p) };
    let mut s = lock(&d.state);
    s.check_editing("updatePlaybackRegionProperties");
    if let Some(old) = s.regions.get_mut(&(reg as usize)) {
        let bound = old.bound;
        *old = region_from(old.host, old.modification, &p);
        old.bound = bound;
    }
}

unsafe extern "C" fn destroy_playback_region(r: Ref, reg: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("destroyPlaybackRegion");
    if let Some(old) = s.regions.remove(&(reg as usize))
        && old.bound > 0
    {
        violation("a region destroyed while a renderer plays it");
    }
}

unsafe extern "C" fn is_source_content_available(r: Ref, src: Ref, kind: ContentType) -> Bool {
    // SAFETY: as above.
    let s = lock(&unsafe { dc(r) }.state);
    (kind == sys::CONTENT_NOTES && s.sources.get(&(src as usize)).is_some_and(|s| s.analysed))
        as Bool
}

unsafe extern "C" fn is_source_analysis_incomplete(r: Ref, src: Ref, kind: ContentType) -> Bool {
    // SAFETY: as above.
    let s = lock(&unsafe { dc(r) }.state);
    (kind == sys::CONTENT_NOTES && s.sources.get(&(src as usize)).is_some_and(|s| !s.analysed))
        as Bool
}

unsafe extern "C" fn request_analysis(r: Ref, src: Ref, n: usize, kinds: *const ContentType) {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    if d.config.fault_on_analysis {
        // SAFETY: none; the fault PLG-058 is about.
        unsafe { std::ptr::write_volatile(std::ptr::null_mut::<u8>(), 1) };
    }
    // SAFETY: the host's list of `n` kinds.
    let kinds = unsafe { std::slice::from_raw_parts(kinds, n) };
    if !kinds.contains(&sys::CONTENT_NOTES) {
        return;
    }
    let mut s = lock(&d.state);
    let Some(src) = s.sources.get_mut(&(src as usize)) else {
        return;
    };
    if src.analysed || src.job.is_some() {
        return;
    }
    src.wanted = true;
    if src.access {
        start_job(d, src);
    }
}

unsafe extern "C" fn grade(_r: Ref, _o: Ref, _k: ContentType) -> sys::ContentGrade {
    sys::GRADE_DETECTED
}

fn note_reader(notes: Vec<sys::ContentNote>) -> Ref {
    Box::into_raw(Box::new(notes)) as Ref
}

fn note(start: f64, duration: f64, volume: f32) -> sys::ContentNote {
    sys::ContentNote {
        frequency: 261.625_57,
        pitch_number: 60,
        volume,
        start_position: start,
        attack_duration: 0.0,
        note_duration: duration,
        signal_duration: duration,
    }
}

unsafe extern "C" fn create_source_reader(
    r: Ref,
    src: Ref,
    kind: ContentType,
    _t: *const sys::ContentTimeRange,
) -> Ref {
    // SAFETY: as above.
    let s = lock(&unsafe { dc(r) }.state);
    match s.sources.get(&(src as usize)) {
        Some(src) if kind == sys::CONTENT_NOTES && src.analysed => {
            note_reader(vec![note(0.0, src.frames as f64 / src.rate, src.peak)])
        }
        _ => std::ptr::null_mut(),
    }
}

/// The note of a modification: one beat at the tempo of its regions' context.
fn modification_note(s: &State, key: usize) -> Option<sys::ContentNote> {
    let m = s.modifications.get(&key)?;
    let src = s.sources.get(&m.source).filter(|s| s.analysed)?;
    let bpm = s
        .regions
        .values()
        .find(|reg| reg.modification == key)
        .and_then(|reg| s.contexts.get(&reg.context))
        .map_or(120.0, |c| c.bpm);
    Some(note(0.0, 60.0 / bpm, src.peak * m.gain))
}

unsafe extern "C" fn is_modification_content_available(r: Ref, m: Ref, kind: ContentType) -> Bool {
    // SAFETY: as above.
    let s = lock(&unsafe { dc(r) }.state);
    (kind == sys::CONTENT_NOTES && modification_note(&s, m as usize).is_some()) as Bool
}

unsafe extern "C" fn create_modification_reader(
    r: Ref,
    m: Ref,
    kind: ContentType,
    _t: *const sys::ContentTimeRange,
) -> Ref {
    // SAFETY: as above.
    let s = lock(&unsafe { dc(r) }.state);
    match modification_note(&s, m as usize) {
        Some(n) if kind == sys::CONTENT_NOTES => note_reader(vec![n]),
        _ => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn is_region_content_available(_r: Ref, _reg: Ref, _k: ContentType) -> Bool {
    FALSE
}

unsafe extern "C" fn create_region_reader(
    _r: Ref,
    _reg: Ref,
    _k: ContentType,
    _t: *const sys::ContentTimeRange,
) -> Ref {
    std::ptr::null_mut()
}

unsafe extern "C" fn reader_count(_r: Ref, reader: Ref) -> i32 {
    // SAFETY: a reader made by `note_reader`.
    unsafe { (*(reader as *const Vec<sys::ContentNote>)).len() as i32 }
}

unsafe extern "C" fn reader_event(_r: Ref, reader: Ref, i: i32) -> *const c_void {
    // SAFETY: as above.
    let v = unsafe { &*(reader as *const Vec<sys::ContentNote>) };
    v.get(i.max(0) as usize)
        .map_or(std::ptr::null(), |n| n as *const _ as *const c_void)
}

unsafe extern "C" fn destroy_reader(_r: Ref, reader: Ref) {
    // SAFETY: as above; destroyed once.
    drop(unsafe { Box::from_raw(reader as *mut Vec<sys::ContentNote>) });
}

unsafe extern "C" fn create_region_sequence(
    r: Ref,
    _host: Ref,
    _p: *const sys::RegionSequenceProperties,
) -> Ref {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("createRegionSequence");
    let id = s.id();
    s.sequences.insert(id, ());
    id as Ref
}

unsafe extern "C" fn update_region_sequence_properties(
    r: Ref,
    _q: Ref,
    _p: *const sys::RegionSequenceProperties,
) {
    // SAFETY: as above.
    lock(&unsafe { dc(r) }.state).check_editing("updateRegionSequenceProperties");
}

unsafe extern "C" fn destroy_region_sequence(r: Ref, q: Ref) {
    // SAFETY: as above.
    let mut s = lock(&unsafe { dc(r) }.state);
    s.check_editing("destroyRegionSequence");
    s.sequences.remove(&(q as usize));
}

unsafe extern "C" fn head_and_tail(r: Ref, _reg: Ref, head: *mut f64, tail: *mut f64) {
    // SAFETY: as above; the host's outs.
    unsafe {
        *head = 0.0;
        *tail = dc(r).config.tail;
    }
}

fn archive_json(s: &State) -> Vec<u8> {
    let mods: serde_json::Map<String, serde_json::Value> = s
        .modifications
        .values()
        .map(|m| (m.id.clone(), serde_json::json!(m.gain)))
        .collect();
    serde_json::to_vec(&serde_json::json!({ "archive": ARCHIVE_ID, "modifications": mods }))
        .unwrap_or_default()
}

fn write_archive(d: &Dc, writer: Ref) -> Bool {
    let bytes = archive_json(&lock(&d.state));
    let (aref, a) = d.host.archiving();
    // SAFETY: the host's writer, written from the start.
    unsafe {
        (a.notify_document_archiving_progress)(aref, 0.0);
        let ok = (a.write_bytes_to_archive)(aref, writer, 0, bytes.len(), bytes.as_ptr());
        (a.notify_document_archiving_progress)(aref, 1.0);
        ok
    }
}

fn read_archive(d: &Dc, reader: Ref) -> Bool {
    let (aref, a) = d.host.archiving();
    // SAFETY: the host's reader.
    let (bytes, id) = unsafe {
        let n = (a.get_archive_size)(aref, reader);
        let mut bytes = vec![0u8; n];
        if (a.read_bytes_from_archive)(aref, reader, 0, n, bytes.as_mut_ptr()) == FALSE {
            return FALSE;
        }
        (
            bytes,
            sys::string((a.get_document_archive_id)(aref, reader)),
        )
    };
    if id != ARCHIVE_ID && id != OLD_ARCHIVE_ID {
        return FALSE;
    }
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return FALSE;
    };
    let mut s = lock(&d.state);
    let State {
        modifications,
        pending,
        ..
    } = &mut *s;
    if let Some(mods) = v.get("modifications").and_then(|m| m.as_object()) {
        for m in modifications.values_mut() {
            if let Some(g) = mods.get(&m.id).and_then(serde_json::Value::as_f64) {
                m.gain = g as f32;
                pending.push(Pending::ModificationContent(m.host));
            }
        }
    }
    // Like many plugins, it says its data changed after a restore.
    pending.push(Pending::DocumentData);
    TRUE
}

unsafe extern "C" fn begin_restoring(r: Ref, reader: Ref) -> Bool {
    // SAFETY: as above.
    read_archive(unsafe { dc(r) }, reader)
}

unsafe extern "C" fn end_restoring(_r: Ref, _reader: Ref) -> Bool {
    TRUE
}

unsafe extern "C" fn store_document(r: Ref, writer: Ref) -> Bool {
    // SAFETY: as above.
    write_archive(unsafe { dc(r) }, writer)
}

unsafe extern "C" fn restore_objects(
    r: Ref,
    reader: Ref,
    _f: *const sys::RestoreObjectsFilter,
) -> Bool {
    // SAFETY: as above.
    let d = unsafe { dc(r) };
    lock(&d.state).check_editing("restoreObjectsFromArchive");
    read_archive(d, reader)
}

unsafe extern "C" fn store_objects(
    r: Ref,
    writer: Ref,
    _f: *const sys::StoreObjectsFilter,
) -> Bool {
    // SAFETY: as above.
    write_archive(unsafe { dc(r) }, writer)
}

static CONTROLLER: Shared<sys::DocumentControllerInterface> =
    Shared(sys::DocumentControllerInterface {
        struct_size: sys::implemented_size!(
            sys::DocumentControllerInterface,
            store_objects_to_archive
        ),
        destroy_document_controller,
        get_factory,
        begin_editing,
        end_editing,
        notify_model_updates,
        begin_restoring_document_from_archive: Some(begin_restoring),
        end_restoring_document_from_archive: Some(end_restoring),
        store_document_to_archive: Some(store_document),
        update_document_properties,
        create_musical_context,
        update_musical_context_properties,
        update_musical_context_content,
        destroy_musical_context,
        create_audio_source,
        update_audio_source_properties,
        update_audio_source_content,
        enable_audio_source_samples_access,
        deactivate_audio_source_for_undo_history: deactivate_for_undo,
        destroy_audio_source,
        create_audio_modification,
        clone_audio_modification,
        update_audio_modification_properties,
        deactivate_audio_modification_for_undo_history: deactivate_for_undo,
        destroy_audio_modification,
        create_playback_region,
        update_playback_region_properties,
        destroy_playback_region,
        is_audio_source_content_available: is_source_content_available,
        is_audio_source_content_analysis_incomplete: is_source_analysis_incomplete,
        request_audio_source_content_analysis: request_analysis,
        get_audio_source_content_grade: grade,
        create_audio_source_content_reader: create_source_reader,
        is_audio_modification_content_available: is_modification_content_available,
        get_audio_modification_content_grade: grade,
        create_audio_modification_content_reader: create_modification_reader,
        is_playback_region_content_available: is_region_content_available,
        get_playback_region_content_grade: grade,
        create_playback_region_content_reader: create_region_reader,
        get_content_reader_event_count: reader_count,
        get_content_reader_data_for_event: reader_event,
        destroy_content_reader: destroy_reader,
        create_region_sequence: Some(create_region_sequence),
        update_region_sequence_properties: Some(update_region_sequence_properties),
        destroy_region_sequence: Some(destroy_region_sequence),
        get_playback_region_head_and_tail_time: Some(head_and_tail),
        restore_objects_from_archive: Some(restore_objects),
        store_objects_to_archive: Some(store_objects),
        get_processing_algorithms_count: None,
        get_processing_algorithm_properties: None,
        get_processing_algorithm_for_audio_source: None,
        request_processing_algorithm_for_audio_source: None,
        is_licensed_for_capabilities: None,
        store_audio_source_to_audio_file_chunk: None,
        is_audio_modification_preserving_audio_source_signal: None,
    });

// The playback renderer -----------------------------------------------------------------------

struct Renderer {
    dc: Arc<Dc>,
    regions: Mutex<Vec<usize>>,
    active: AtomicBool,
}

unsafe extern "C" fn add_region(r: Ref, reg: Ref) {
    // SAFETY: a renderer of a live instance.
    let renderer = unsafe { &*(r as *const Renderer) };
    if renderer.active.load(Ordering::Acquire) {
        violation("a region added to a renderer set up to process");
    }
    if let Some(region) = lock(&renderer.dc.state).regions.get_mut(&(reg as usize)) {
        region.bound += 1;
    }
    lock(&renderer.regions).push(reg as usize);
}

unsafe extern "C" fn remove_region(r: Ref, reg: Ref) {
    // SAFETY: as above.
    let renderer = unsafe { &*(r as *const Renderer) };
    if renderer.active.load(Ordering::Acquire) {
        violation("a region removed from a renderer set up to process");
    }
    let mut regions = lock(&renderer.regions);
    if let Some(i) = regions.iter().position(|x| *x == reg as usize) {
        regions.remove(i);
        if let Some(region) = lock(&renderer.dc.state).regions.get_mut(&(reg as usize)) {
            region.bound = region.bound.saturating_sub(1);
        }
    }
}

static RENDERER: Shared<sys::PlaybackRendererInterface> = Shared(sys::PlaybackRendererInterface {
    struct_size: std::mem::size_of::<sys::PlaybackRendererInterface>(),
    add_playback_region: add_region,
    remove_playback_region: remove_region,
});

struct FakeBinary {
    config: Config,
}

impl Binary for FakeBinary {
    fn factory(&self) -> *const sys::Factory {
        // The document made next on this thread misbehaves like this binary.
        NEXT.with(|n| n.set(self.config));
        factory()
    }

    fn module(&self) -> usize {
        if !self.config.fault_on_own_thread {
            return 0;
        }
        contain::add_module(own_fault as *const () as usize);
        own_fault as *const () as usize
    }

    fn instance(
        &mut self,
        dc: Ref,
        _known: RoleFlags,
        assigned: RoleFlags,
    ) -> Result<Box<dyn Instance>, Error> {
        if dc.is_null() {
            return Err(error("no document controller"));
        }
        // SAFETY: a live controller made by this module from an `Arc`; one more owner.
        let dc = unsafe {
            Arc::increment_strong_count(dc as *const Dc);
            Arc::from_raw(dc as *const Dc)
        };
        let renderer = Box::new(Renderer {
            dc,
            regions: Mutex::new(Vec::new()),
            active: AtomicBool::new(false),
        });
        let plays = assigned & sys::ROLE_PLAYBACK_RENDERER != 0;
        let ext = sys::PlugInExtensionInstance {
            struct_size: std::mem::size_of::<sys::PlugInExtensionInstance>(),
            plug_in_extension_ref: std::ptr::null_mut(),
            plug_in_extension_interface: std::ptr::null(),
            playback_renderer_ref: if plays {
                &*renderer as *const Renderer as Ref
            } else {
                std::ptr::null_mut()
            },
            playback_renderer_interface: if plays { &RENDERER.0 } else { std::ptr::null() },
            editor_renderer_ref: std::ptr::null_mut(),
            editor_renderer_interface: std::ptr::null(),
            editor_view_ref: std::ptr::null_mut(),
            editor_view_interface: std::ptr::null(),
        };
        Ok(Box::new(FakeInstance {
            renderer,
            ext,
            rate: 0.0,
            position: 0,
        }))
    }
}

struct FakeInstance {
    renderer: Box<Renderer>,
    ext: sys::PlugInExtensionInstance,
    rate: f64,
    position: i64,
}

impl Instance for FakeInstance {
    fn extension(&self) -> sys::PlugInExtensionInstance {
        self.ext
    }

    fn prepare(&mut self, rate: f64, _max_block: usize) -> Result<u32, Error> {
        self.rate = rate;
        self.renderer.active.store(true, Ordering::Release);
        Ok(0)
    }

    fn set_transport(&mut self, _playing: bool, position: i64) {
        self.position = position;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.renderer.dc.config.hang_in_render {
            contain::call(|| {
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        }
        let spin = Duration::from_micros(self.renderer.dc.config.render_spin_us);
        let begun = Instant::now();
        while begun.elapsed() < spin {
            std::hint::spin_loop();
        }
        let n = left.len().min(right.len());
        left[..n].fill(0.0);
        right[..n].fill(0.0);
        let s = lock(&self.renderer.dc.state);
        for reg in lock(&self.renderer.regions).iter() {
            let Some(reg) = s.regions.get(reg) else {
                continue;
            };
            let Some(m) = s.modifications.get(&reg.modification) else {
                continue;
            };
            let Some(src) = s.sources.get(&m.source) else {
                continue;
            };
            if src.cache.is_empty() {
                continue;
            }
            let (l, r) = (&src.cache[0], src.cache.get(1).unwrap_or(&src.cache[0]));
            for i in 0..n {
                let t = (self.position + i as i64) as f64 / self.rate;
                let into = t - reg.start_in_playback;
                if !(0.0..reg.duration_in_playback).contains(&into)
                    || into >= reg.duration_in_modification
                {
                    continue;
                }
                let f = ((reg.start_in_modification + into) * src.rate) as usize;
                if f < l.len() {
                    left[i] += l[f] * m.gain;
                    right[i] += r.get(f).copied().unwrap_or(0.0) * m.gain;
                }
            }
        }
        drop(s);
        self.position += n as i64;
    }
}

impl Drop for FakeInstance {
    fn drop(&mut self) {
        self.renderer.active.store(false, Ordering::Release);
        let mut s = lock(&self.renderer.dc.state);
        for reg in lock(&self.renderer.regions).drain(..) {
            if let Some(region) = s.regions.get_mut(&reg) {
                region.bound = region.bound.saturating_sub(1);
            }
        }
    }
}
