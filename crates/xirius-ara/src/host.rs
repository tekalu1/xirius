//! The host's side of ARA: the five controllers the plugin calls back (PLG-031).
//!
//! - Audio access: the plugin reads a source's samples, from any of its threads at once
//!   (PLG-034). A source's audio is an immutable [`SourceAudio`] behind an `Arc`, so a reader
//!   holds its own reference and reads without locks. Replacing the audio (a new sample rate,
//!   PLG-047) swaps the `Arc` while the plugin's access is off.
//! - Archiving: the bytes of an archive being stored or restored ([`ArchiveWriter`],
//!   [`ArchiveReader`]).
//! - Content access: the song's tempo and bar signature for the plugin's musical context.
//! - Model update: the plugin's notifications. They arrive on the thread that called
//!   `notifyModelUpdates`, the ARA thread ([`crate::service`]); they are queued here with the
//!   thread they came on and handled there.
//! - Playback: the plugin's requests to start, stop or move the transport, queued for the app.
//!
//! Every controller gets the same host reference, a pointer to the document's [`HostSide`].
//! The host references of sources, modifications, regions and musical contexts are the
//! host's keys (never zero), not pointers.

use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread::ThreadId;

use crate::sys::{self, Bool, ContentTimeRange, ContentType, FALSE, Ref, TRUE};

/// A source's samples at one rate, one `Vec` per channel.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceAudio {
    pub rate: f64,
    pub channels: Vec<Vec<f32>>,
}

impl SourceAudio {
    pub fn stereo(rate: f64, left: Vec<f32>, right: Vec<f32>) -> SourceAudio {
        SourceAudio {
            rate,
            channels: vec![left, right],
        }
    }

    pub fn frames(&self) -> usize {
        self.channels.iter().map(Vec::len).min().unwrap_or(0)
    }
}

/// The song's time as the plugin's musical context sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Musical {
    pub bpm: f64,
    pub numerator: i32,
    pub denominator: i32,
}

impl Default for Musical {
    fn default() -> Musical {
        Musical {
            bpm: 120.0,
            numerator: 4,
            denominator: 4,
        }
    }
}

/// A notification from the plugin (the model update controller).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// `state` is [`sys::ANALYSIS_STARTED`], `UPDATED` or `COMPLETED`.
    Analysis {
        source: usize,
        state: i32,
        value: f32,
    },
    SourceContent {
        source: usize,
        flags: i32,
    },
    ModificationContent {
        modification: usize,
        flags: i32,
    },
    RegionContent {
        region: usize,
        flags: i32,
    },
    DocumentData,
}

/// A request from the plugin to the app's transport (the playback controller).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    Start,
    Stop,
    Position(f64),
    CycleRange(f64, f64),
    Cycle(bool),
}

/// Which controllers the plugin has called, for the checks of PLG-031.
#[derive(Debug, Default)]
pub struct Calls {
    pub audio_access: AtomicU64,
    pub archiving: AtomicU64,
    pub content_access: AtomicU64,
    pub model_update: AtomicU64,
    pub playback: AtomicU64,
    /// `readAudioSamples` calls, and the most readers alive at once.
    pub reads: AtomicU64,
    pub readers: AtomicI64,
    pub most_readers: AtomicI64,
    /// Reads made on another thread than the ARA thread.
    pub reads_off_thread: AtomicU64,
}

impl Calls {
    /// Whether all five controllers were called.
    pub fn all_five(&self) -> bool {
        [
            &self.audio_access,
            &self.archiving,
            &self.content_access,
            &self.model_update,
            &self.playback,
        ]
        .iter()
        .all(|c| c.load(Ordering::Acquire) > 0)
    }
}

type Slot = Arc<RwLock<Arc<SourceAudio>>>;

/// The host's side of one document.
pub struct HostSide {
    main: Mutex<Option<ThreadId>>,
    sources: RwLock<HashMap<usize, Slot>>,
    musical: Mutex<HashMap<usize, Musical>>,
    notices: Mutex<Vec<(Notice, ThreadId)>>,
    requests: Mutex<Vec<Request>>,
    pub calls: Calls,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl HostSide {
    pub fn new() -> Arc<HostSide> {
        Arc::new(HostSide {
            main: Mutex::new(None),
            sources: RwLock::new(HashMap::new()),
            musical: Mutex::new(HashMap::new()),
            notices: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
            calls: Calls::default(),
        })
    }

    /// Makes the calling thread the document's main thread (the ARA thread).
    pub fn set_main(&self) {
        *lock(&self.main) = Some(std::thread::current().id());
    }

    pub fn is_main(&self) -> bool {
        *lock(&self.main) == Some(std::thread::current().id())
    }

    pub fn set_source(&self, key: usize, audio: Arc<SourceAudio>) {
        let mut map = self.sources.write().unwrap_or_else(PoisonError::into_inner);
        match map.get(&key) {
            Some(slot) => *slot.write().unwrap_or_else(PoisonError::into_inner) = audio,
            None => {
                map.insert(key, Arc::new(RwLock::new(audio)));
            }
        }
    }

    pub fn remove_source(&self, key: usize) {
        self.sources
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key);
    }

    pub fn source(&self, key: usize) -> Option<Arc<SourceAudio>> {
        let map = self.sources.read().unwrap_or_else(PoisonError::into_inner);
        let slot = map.get(&key)?;
        Some(slot.read().unwrap_or_else(PoisonError::into_inner).clone())
    }

    pub fn set_musical(&self, key: usize, m: Musical) {
        lock(&self.musical).insert(key, m);
    }

    /// The notifications since the last call, each with the thread it came on.
    pub fn take_notices(&self) -> Vec<(Notice, ThreadId)> {
        std::mem::take(&mut *lock(&self.notices))
    }

    pub fn take_requests(&self) -> Vec<Request> {
        std::mem::take(&mut *lock(&self.requests))
    }

    fn notice(&self, n: Notice) {
        self.calls.model_update.fetch_add(1, Ordering::AcqRel);
        lock(&self.notices).push((n, std::thread::current().id()));
    }

    fn request(&self, r: Request) {
        self.calls.playback.fetch_add(1, Ordering::AcqRel);
        lock(&self.requests).push(r);
    }
}

/// # Safety
/// `r` is the host reference the document handed out: a live `HostSide`.
unsafe fn side<'a>(r: Ref) -> &'a HostSide {
    // SAFETY: per the contract; the document keeps the `HostSide` alive while the plugin
    // may call.
    unsafe { &*(r as *const HostSide) }
}

// Audio access --------------------------------------------------------------------------------

struct Reader {
    audio: Arc<SourceAudio>,
    use_64: bool,
}

unsafe extern "C" fn create_audio_reader(ctrl: Ref, source: Ref, use_64: Bool) -> Ref {
    // SAFETY: the plugin passes the controller reference it was given.
    let s = unsafe { side(ctrl) };
    s.calls.audio_access.fetch_add(1, Ordering::AcqRel);
    let Some(audio) = s.source(source as usize) else {
        return std::ptr::null_mut();
    };
    let n = s.calls.readers.fetch_add(1, Ordering::AcqRel) + 1;
    s.calls.most_readers.fetch_max(n, Ordering::AcqRel);
    Box::into_raw(Box::new(Reader {
        audio,
        use_64: use_64 != FALSE,
    })) as Ref
}

unsafe extern "C" fn read_audio_samples(
    ctrl: Ref,
    reader: Ref,
    position: i64,
    count: i64,
    buffers: *const *mut c_void,
) -> Bool {
    // SAFETY: as above.
    let s = unsafe { side(ctrl) };
    s.calls.reads.fetch_add(1, Ordering::AcqRel);
    if !s.is_main() {
        s.calls.reads_off_thread.fetch_add(1, Ordering::AcqRel);
    }
    if reader.is_null() || buffers.is_null() || count < 0 {
        return FALSE;
    }
    // SAFETY: a reader made by `create_audio_reader` and not yet destroyed.
    let r = unsafe { &*(reader as *const Reader) };
    let frames = r.audio.frames() as i64;
    let count = count as usize;
    let mut ok = TRUE;
    for (ch, data) in r.audio.channels.iter().enumerate() {
        // SAFETY: the plugin passes one buffer per channel of the source.
        let out = unsafe { *buffers.add(ch) };
        if out.is_null() {
            return FALSE;
        }
        for i in 0..count {
            let at = position + i as i64;
            let v = if (0..frames).contains(&at) {
                data[at as usize]
            } else {
                // Asking outside the source is the plugin's mistake; it gets silence.
                ok = FALSE;
                0.0
            };
            // SAFETY: each buffer holds `count` samples of the asked size.
            unsafe {
                if r.use_64 {
                    *(out as *mut f64).add(i) = f64::from(v);
                } else {
                    *(out as *mut f32).add(i) = v;
                }
            }
        }
    }
    ok
}

unsafe extern "C" fn destroy_audio_reader(ctrl: Ref, reader: Ref) {
    // SAFETY: as above.
    let s = unsafe { side(ctrl) };
    s.calls.audio_access.fetch_add(1, Ordering::AcqRel);
    if !reader.is_null() {
        s.calls.readers.fetch_sub(1, Ordering::AcqRel);
        // SAFETY: made by `create_audio_reader`; the plugin destroys it once.
        drop(unsafe { Box::from_raw(reader as *mut Reader) });
    }
}

pub static AUDIO_ACCESS: sys::AudioAccessControllerInterface =
    sys::AudioAccessControllerInterface {
        struct_size: std::mem::size_of::<sys::AudioAccessControllerInterface>(),
        create_audio_reader_for_source: create_audio_reader,
        read_audio_samples,
        destroy_audio_reader,
    };

// Archiving -----------------------------------------------------------------------------------

/// An archive the plugin restores from.
pub struct ArchiveReader {
    pub data: Vec<u8>,
    pub archive_id: CString,
}

/// An archive the plugin stores into.
#[derive(Default)]
pub struct ArchiveWriter {
    pub data: Mutex<Vec<u8>>,
}

unsafe extern "C" fn get_archive_size(ctrl: Ref, reader: Ref) -> usize {
    // SAFETY: as above; `reader` is an `ArchiveReader` the document passed.
    unsafe {
        side(ctrl).calls.archiving.fetch_add(1, Ordering::AcqRel);
        (*(reader as *const ArchiveReader)).data.len()
    }
}

unsafe extern "C" fn read_bytes_from_archive(
    ctrl: Ref,
    reader: Ref,
    position: usize,
    length: usize,
    buffer: *mut u8,
) -> Bool {
    // SAFETY: as above.
    let data = unsafe {
        side(ctrl).calls.archiving.fetch_add(1, Ordering::AcqRel);
        &(*(reader as *const ArchiveReader)).data
    };
    let Some(end) = position.checked_add(length).filter(|&e| e <= data.len()) else {
        return FALSE;
    };
    // SAFETY: the plugin's buffer holds `length` bytes.
    unsafe { std::ptr::copy_nonoverlapping(data[position..end].as_ptr(), buffer, length) };
    TRUE
}

unsafe extern "C" fn write_bytes_to_archive(
    ctrl: Ref,
    writer: Ref,
    position: usize,
    length: usize,
    buffer: *const u8,
) -> Bool {
    // SAFETY: as above; `writer` is an `ArchiveWriter` the document passed.
    let w = unsafe {
        side(ctrl).calls.archiving.fetch_add(1, Ordering::AcqRel);
        &*(writer as *const ArchiveWriter)
    };
    let mut data = lock(&w.data);
    let end = position + length;
    if data.len() < end {
        data.resize(end, 0);
    }
    // SAFETY: the plugin's buffer holds `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(buffer, length) };
    data[position..end].copy_from_slice(bytes);
    TRUE
}

unsafe extern "C" fn notify_archiving_progress(ctrl: Ref, _value: f32) {
    // SAFETY: as above.
    unsafe { side(ctrl) }
        .calls
        .archiving
        .fetch_add(1, Ordering::AcqRel);
}

unsafe extern "C" fn get_document_archive_id(ctrl: Ref, reader: Ref) -> sys::PersistentId {
    // SAFETY: as above.
    unsafe {
        side(ctrl).calls.archiving.fetch_add(1, Ordering::AcqRel);
        (*(reader as *const ArchiveReader)).archive_id.as_ptr()
    }
}

pub static ARCHIVING: sys::ArchivingControllerInterface = sys::ArchivingControllerInterface {
    struct_size: std::mem::size_of::<sys::ArchivingControllerInterface>(),
    get_archive_size,
    read_bytes_from_archive,
    write_bytes_to_archive,
    notify_document_archiving_progress: notify_archiving_progress,
    notify_document_unarchiving_progress: notify_archiving_progress,
    get_document_archive_id,
};

// Content access ------------------------------------------------------------------------------

enum Events {
    Tempo(Vec<sys::ContentTempoEntry>),
    Bars(Vec<sys::ContentBarSignature>),
}

fn musical_events(m: Musical, kind: ContentType) -> Option<Events> {
    match kind {
        // Two entries are the least the API allows; a constant tempo is a straight line.
        sys::CONTENT_TEMPO_ENTRIES => Some(Events::Tempo(vec![
            sys::ContentTempoEntry {
                time_position: 0.0,
                quarter_position: 0.0,
            },
            sys::ContentTempoEntry {
                time_position: 60.0 / m.bpm,
                quarter_position: 1.0,
            },
        ])),
        sys::CONTENT_BAR_SIGNATURES => Some(Events::Bars(vec![sys::ContentBarSignature {
            numerator: m.numerator,
            denominator: m.denominator,
            position: 0.0,
        }])),
        _ => None,
    }
}

unsafe extern "C" fn is_musical_content_available(
    ctrl: Ref,
    context: Ref,
    kind: ContentType,
) -> Bool {
    // SAFETY: as above.
    let s = unsafe { side(ctrl) };
    s.calls.content_access.fetch_add(1, Ordering::AcqRel);
    let known = lock(&s.musical).contains_key(&(context as usize));
    (known && musical_events(Musical::default(), kind).is_some()) as Bool
}

unsafe extern "C" fn musical_content_grade(ctrl: Ref, _context: Ref, _kind: ContentType) -> i32 {
    // SAFETY: as above.
    unsafe { side(ctrl) }
        .calls
        .content_access
        .fetch_add(1, Ordering::AcqRel);
    // The song's tempo is set by its user.
    sys::GRADE_APPROVED
}

unsafe extern "C" fn create_musical_content_reader(
    ctrl: Ref,
    context: Ref,
    kind: ContentType,
    _range: *const ContentTimeRange,
) -> Ref {
    // SAFETY: as above.
    let s = unsafe { side(ctrl) };
    s.calls.content_access.fetch_add(1, Ordering::AcqRel);
    let Some(m) = lock(&s.musical).get(&(context as usize)).copied() else {
        return std::ptr::null_mut();
    };
    match musical_events(m, kind) {
        Some(e) => Box::into_raw(Box::new(e)) as Ref,
        None => std::ptr::null_mut(),
    }
}

unsafe extern "C" fn is_source_content_available(
    ctrl: Ref,
    _source: Ref,
    _kind: ContentType,
) -> Bool {
    // SAFETY: as above.
    unsafe { side(ctrl) }
        .calls
        .content_access
        .fetch_add(1, Ordering::AcqRel);
    // The host knows nothing about a source's notes or tempo of its own.
    FALSE
}

unsafe extern "C" fn source_content_grade(ctrl: Ref, _source: Ref, _kind: ContentType) -> i32 {
    // SAFETY: as above.
    unsafe { side(ctrl) }
        .calls
        .content_access
        .fetch_add(1, Ordering::AcqRel);
    sys::GRADE_INITIAL
}

unsafe extern "C" fn create_source_content_reader(
    ctrl: Ref,
    _source: Ref,
    _kind: ContentType,
    _range: *const ContentTimeRange,
) -> Ref {
    // SAFETY: as above.
    unsafe { side(ctrl) }
        .calls
        .content_access
        .fetch_add(1, Ordering::AcqRel);
    std::ptr::null_mut()
}

unsafe extern "C" fn content_event_count(ctrl: Ref, reader: Ref) -> i32 {
    // SAFETY: as above; `reader` came from `create_musical_content_reader`.
    unsafe {
        side(ctrl)
            .calls
            .content_access
            .fetch_add(1, Ordering::AcqRel);
        match &*(reader as *const Events) {
            Events::Tempo(v) => v.len() as i32,
            Events::Bars(v) => v.len() as i32,
        }
    }
}

unsafe extern "C" fn content_event(ctrl: Ref, reader: Ref, index: i32) -> *const c_void {
    // SAFETY: as above.
    unsafe {
        side(ctrl)
            .calls
            .content_access
            .fetch_add(1, Ordering::AcqRel);
        let i = index.max(0) as usize;
        match &*(reader as *const Events) {
            Events::Tempo(v) => v.get(i).map_or(std::ptr::null(), |e| e as *const _ as _),
            Events::Bars(v) => v.get(i).map_or(std::ptr::null(), |e| e as *const _ as _),
        }
    }
}

unsafe extern "C" fn destroy_content_reader(ctrl: Ref, reader: Ref) {
    // SAFETY: as above; the plugin destroys each reader once.
    unsafe {
        side(ctrl)
            .calls
            .content_access
            .fetch_add(1, Ordering::AcqRel);
        if !reader.is_null() {
            drop(Box::from_raw(reader as *mut Events));
        }
    }
}

pub static CONTENT_ACCESS: sys::ContentAccessControllerInterface =
    sys::ContentAccessControllerInterface {
        struct_size: std::mem::size_of::<sys::ContentAccessControllerInterface>(),
        is_musical_context_content_available: is_musical_content_available,
        get_musical_context_content_grade: musical_content_grade,
        create_musical_context_content_reader: create_musical_content_reader,
        is_audio_source_content_available: is_source_content_available,
        get_audio_source_content_grade: source_content_grade,
        create_audio_source_content_reader: create_source_content_reader,
        get_content_reader_event_count: content_event_count,
        get_content_reader_data_for_event: content_event,
        destroy_content_reader,
    };

// Model update --------------------------------------------------------------------------------

unsafe extern "C" fn analysis_progress(ctrl: Ref, source: Ref, state: i32, value: f32) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.notice(Notice::Analysis {
        source: source as usize,
        state,
        value,
    });
}

unsafe extern "C" fn source_content_changed(
    ctrl: Ref,
    source: Ref,
    _range: *const ContentTimeRange,
    flags: i32,
) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.notice(Notice::SourceContent {
        source: source as usize,
        flags,
    });
}

unsafe extern "C" fn modification_content_changed(
    ctrl: Ref,
    modification: Ref,
    _range: *const ContentTimeRange,
    flags: i32,
) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.notice(Notice::ModificationContent {
        modification: modification as usize,
        flags,
    });
}

unsafe extern "C" fn region_content_changed(
    ctrl: Ref,
    region: Ref,
    _range: *const ContentTimeRange,
    flags: i32,
) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.notice(Notice::RegionContent {
        region: region as usize,
        flags,
    });
}

unsafe extern "C" fn document_data_changed(ctrl: Ref) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.notice(Notice::DocumentData);
}

pub static MODEL_UPDATE: sys::ModelUpdateControllerInterface =
    sys::ModelUpdateControllerInterface {
        struct_size: std::mem::size_of::<sys::ModelUpdateControllerInterface>(),
        notify_audio_source_analysis_progress: analysis_progress,
        notify_audio_source_content_changed: source_content_changed,
        notify_audio_modification_content_changed: modification_content_changed,
        notify_playback_region_content_changed: region_content_changed,
        notify_document_data_changed: document_data_changed,
    };

// Playback ------------------------------------------------------------------------------------

unsafe extern "C" fn request_start(ctrl: Ref) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.request(Request::Start);
}

unsafe extern "C" fn request_stop(ctrl: Ref) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.request(Request::Stop);
}

unsafe extern "C" fn request_position(ctrl: Ref, time: f64) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.request(Request::Position(time));
}

unsafe extern "C" fn request_cycle_range(ctrl: Ref, start: f64, duration: f64) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.request(Request::CycleRange(start, duration));
}

unsafe extern "C" fn request_cycle(ctrl: Ref, enable: Bool) {
    // SAFETY: as above.
    unsafe { side(ctrl) }.request(Request::Cycle(enable != FALSE));
}

pub static PLAYBACK: sys::PlaybackControllerInterface = sys::PlaybackControllerInterface {
    struct_size: std::mem::size_of::<sys::PlaybackControllerInterface>(),
    request_start_playback: request_start,
    request_stop_playback: request_stop,
    request_set_playback_position: request_position,
    request_set_cycle_range: request_cycle_range,
    request_enable_cycle: request_cycle,
};

/// The host instance for a document whose controllers all answer to `side`.
pub fn host_instance(side: &Arc<HostSide>) -> sys::DocumentControllerHostInstance {
    let r = Arc::as_ptr(side) as Ref;
    sys::DocumentControllerHostInstance {
        struct_size: std::mem::size_of::<sys::DocumentControllerHostInstance>(),
        audio_access_controller_host_ref: r,
        audio_access_controller_interface: &AUDIO_ACCESS,
        archiving_controller_host_ref: r,
        archiving_controller_interface: &ARCHIVING,
        content_access_controller_host_ref: r,
        content_access_controller_interface: &CONTENT_ACCESS,
        model_update_controller_host_ref: r,
        model_update_controller_interface: &MODEL_UPDATE,
        playback_controller_host_ref: r,
        playback_controller_interface: &PLAYBACK,
    }
}
