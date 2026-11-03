//! One plugin's document controller, driven by the host (PLG-032, PLG-035, PLG-037).
//!
//! The objects the host makes in the plugin's model are named by the host's keys (its host
//! references) and answered with the plugin's references ([`Ref`]). Changes to the model are
//! made between [`Document::begin_editing`] and [`Document::end_editing`]; the plugin's
//! notifications are pulled with [`Document::notify_model_updates`]. Every call is made on the
//! thread that made the document, marked as a call into the plugin ([`crate::contain`]).

// The ARA references passed in are the plugin's opaque handles: the host hands them back to
// the plugin and never reads through them.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::collections::HashSet;
use std::ffi::{CString, c_char};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::host::{self, ArchiveReader, ArchiveWriter, HostSide};
use crate::sys::{self, Bool, ContentType, FALSE, Ref, TRUE};
use crate::{Error, contain, error};

/// A note the plugin found in a source or modification (`ARAContentNote`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Hertz; 0 when the note has no pitch.
    pub frequency: f32,
    /// MIDI number, or -1 without pitch.
    pub pitch: i32,
    pub volume: f32,
    /// Seconds from the start of the source or modification.
    pub start: f64,
    pub duration: f64,
}

/// Assertions the plugin's ARA library reported (a mistake of the host or the plugin).
static ASSERTS: AtomicU64 = AtomicU64::new(0);
static LAST_ASSERT: Mutex<String> = Mutex::new(String::new());

/// How many assertions plugins reported in this process, and the last one's text.
pub fn asserts() -> (u64, String) {
    (
        ASSERTS.load(Ordering::Acquire),
        LAST_ASSERT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
    )
}

unsafe extern "C" fn on_assert(
    _category: i32,
    _problem: *const std::ffi::c_void,
    diagnosis: *const c_char,
) {
    ASSERTS.fetch_add(1, Ordering::AcqRel);
    // SAFETY: the plugin passes a NUL-terminated text or null.
    let text = unsafe { sys::string(diagnosis) };
    *LAST_ASSERT.lock().unwrap_or_else(PoisonError::into_inner) = text;
}

static ASSERT_FUNCTION: sys::AssertFunction = on_assert;

/// Factories already initialized in this process, with the generation they were given.
/// `initializeARAWithConfiguration` is called once per factory and process; the factory is
/// never uninitialized, because its module stays loaded to the end (ARC-043).
static INITIALIZED: Mutex<Vec<(usize, sys::ApiGeneration)>> = Mutex::new(Vec::new());

/// Initializes `factory` once and returns the API generation it runs at.
///
/// # Safety
/// `factory` points to a live factory whose struct stays valid for the process.
pub unsafe fn initialize(factory: *const sys::Factory) -> Result<sys::ApiGeneration, Error> {
    if factory.is_null() {
        return Err(error("the plugin has no ARA factory"));
    }
    let mut done = INITIALIZED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(&(_, g)) = done.iter().find(|(f, _)| *f == factory as usize) {
        return Ok(g);
    }
    // SAFETY: per the contract; fields are read by value from the packed struct.
    let (size, lowest, highest, init) = unsafe {
        let f = &*factory;
        (
            f.struct_size,
            f.lowest_supported_api_generation,
            f.highest_supported_api_generation,
            f.initialize_ara_with_configuration,
        )
    };
    if size < sys::FACTORY_MIN_SIZE {
        return Err(error(format!(
            "the ARA factory is too small ({size} bytes)"
        )));
    }
    let generation = highest.min(sys::API_GENERATION_2_3_FINAL);
    if generation < lowest.max(sys::API_GENERATION_2_0_FINAL) {
        return Err(error(format!(
            "the plugin needs ARA generation {lowest} to {highest}; the host speaks {} to {}",
            sys::API_GENERATION_2_0_FINAL,
            sys::API_GENERATION_2_3_FINAL
        )));
    }
    let config = sys::InterfaceConfiguration {
        struct_size: std::mem::size_of::<sys::InterfaceConfiguration>(),
        desired_api_generation: generation,
        assert_function_address: &ASSERT_FUNCTION,
    };
    contain::call(|| {
        // SAFETY: a configuration that lives through the call; the assert function is static.
        unsafe { init(&config) }
    });
    done.push((factory as usize, generation));
    Ok(generation)
}

/// What a factory says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FactoryInfo {
    pub factory_id: String,
    pub plugin_name: String,
    pub manufacturer: String,
    pub version: String,
    pub archive_id: String,
    pub compatible_archive_ids: Vec<String>,
    pub analyzeable: Vec<ContentType>,
}

/// # Safety
/// `factory` points to a live factory of at least [`sys::FACTORY_MIN_SIZE`] bytes.
pub unsafe fn factory_info(factory: *const sys::Factory) -> FactoryInfo {
    // SAFETY: per the contract; strings and arrays belong to the factory and stay valid.
    unsafe {
        let f = &*factory;
        let list = |n: usize, p: *const sys::PersistentId| -> Vec<String> {
            if p.is_null() {
                return Vec::new();
            }
            (0..n).map(|i| sys::string(*p.add(i))).collect()
        };
        let analyzeable = if f.analyzeable_content_types.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(
                f.analyzeable_content_types,
                f.analyzeable_content_types_count,
            )
            .to_vec()
        };
        FactoryInfo {
            factory_id: sys::string(f.factory_id),
            plugin_name: sys::string(f.plug_in_name),
            manufacturer: sys::string(f.manufacturer_name),
            version: sys::string(f.version),
            archive_id: sys::string(f.document_archive_id),
            compatible_archive_ids: list(
                f.compatible_document_archive_ids_count,
                f.compatible_document_archive_ids,
            ),
            analyzeable,
        }
    }
}

/// The properties of an audio source.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceProps {
    pub name: String,
    pub persistent_id: String,
    pub frames: i64,
    pub rate: f64,
    pub channels: i32,
}

/// The properties of a playback region, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionProps {
    pub start_in_modification: f64,
    pub duration_in_modification: f64,
    pub start_in_playback: f64,
    pub duration_in_playback: f64,
}

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', " ")).unwrap_or_default()
}

/// One plugin document. Dropping it destroys the plugin's document controller.
pub struct Document {
    factory: *const sys::Factory,
    side: Arc<HostSide>,
    // The plugin keeps a pointer to the host instance for the controller's lifetime.
    _host: Box<sys::DocumentControllerHostInstance>,
    dc: Ref,
    iface: *const sys::DocumentControllerInterface,
    iface_size: usize,
    generation: sys::ApiGeneration,
    editing: u32,
    /// Objects of the plugin made by this document and not destroyed (for checks).
    live: HashSet<usize>,
}

impl Document {
    /// Initializes the factory if needed and makes a document named `name`. The calling
    /// thread becomes the document's thread.
    ///
    /// # Safety
    /// `factory` points to a live factory that stays valid while the document lives.
    pub unsafe fn new(
        factory: *const sys::Factory,
        side: Arc<HostSide>,
        name: &str,
    ) -> Result<Document, Error> {
        // SAFETY: per the contract.
        let generation = unsafe { initialize(factory)? };
        side.set_main();
        let host = Box::new(host::host_instance(&side));
        let name = cstring(name);
        let props = sys::DocumentProperties {
            struct_size: std::mem::size_of::<sys::DocumentProperties>(),
            name: name.as_ptr(),
        };
        // SAFETY: per the contract; read by value.
        let create = unsafe { (*factory).create_document_controller_with_document };
        let instance = contain::call(|| {
            // SAFETY: the host instance lives in the box for the controller's lifetime.
            unsafe { create(&*host, &props) }
        });
        if instance.is_null() {
            return Err(error("the plugin did not make a document controller"));
        }
        // SAFETY: the plugin returned a live instance.
        let (dc, iface) = unsafe {
            (
                (*instance).document_controller_ref,
                (*instance).document_controller_interface,
            )
        };
        if iface.is_null() {
            return Err(error("the plugin's document controller has no interface"));
        }
        // SAFETY: as above.
        let iface_size = unsafe { (*iface).struct_size };
        if iface_size < sys::DOCUMENT_CONTROLLER_MIN_SIZE {
            return Err(error(format!(
                "the document controller is too small ({iface_size} bytes)"
            )));
        }
        Ok(Document {
            factory,
            side,
            _host: host,
            dc,
            iface,
            iface_size,
            generation,
            editing: 0,
            live: HashSet::new(),
        })
    }

    pub fn side(&self) -> &Arc<HostSide> {
        &self.side
    }

    pub fn factory(&self) -> *const sys::Factory {
        self.factory
    }

    pub fn generation(&self) -> sys::ApiGeneration {
        self.generation
    }

    /// The plugin's reference to its document controller, for binding instances.
    pub fn controller_ref(&self) -> Ref {
        self.dc
    }

    pub fn info(&self) -> FactoryInfo {
        // SAFETY: the factory outlives the document.
        unsafe { factory_info(self.factory) }
    }

    fn f(&self) -> sys::DocumentControllerInterface {
        // SAFETY: the interface stays valid while the controller lives; read by value.
        unsafe { *self.iface }
    }

    /// Whether the interface has the member ending at `end` bytes (a later addition).
    fn has(&self, end: usize) -> bool {
        self.iface_size >= end
    }

    pub fn is_editing(&self) -> bool {
        self.editing > 0
    }

    pub fn begin_editing(&mut self) {
        if self.editing == 0 {
            let f = self.f().begin_editing;
            // SAFETY: a call on the live controller from its thread.
            contain::call(|| unsafe { f(self.dc) });
        }
        self.editing += 1;
    }

    pub fn end_editing(&mut self) {
        if self.editing == 0 {
            return;
        }
        self.editing -= 1;
        if self.editing == 0 {
            let f = self.f().end_editing;
            // SAFETY: as above.
            contain::call(|| unsafe { f(self.dc) });
        }
    }

    /// Lets the plugin send its notifications ([`HostSide::take_notices`]).
    pub fn notify_model_updates(&self) {
        let f = self.f().notify_model_updates;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc) });
    }

    pub fn update_name(&mut self, name: &str) {
        let name = cstring(name);
        let props = sys::DocumentProperties {
            struct_size: std::mem::size_of::<sys::DocumentProperties>(),
            name: name.as_ptr(),
        };
        let f = self.f().update_document_properties;
        // SAFETY: as above; the properties live through the call.
        contain::call(|| unsafe { f(self.dc, &props) });
    }

    // Musical context -------------------------------------------------------------------------

    pub fn create_musical_context(&mut self, key: usize, name: &str) -> Result<Ref, Error> {
        let name = cstring(name);
        let props = sys::MusicalContextProperties {
            struct_size: std::mem::size_of::<sys::MusicalContextProperties>(),
            name: name.as_ptr(),
            order_index: 0,
            color: std::ptr::null(),
        };
        let f = self.f().create_musical_context;
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, key as Ref, &props) });
        self.made(r, "musical context")
    }

    /// Tells the plugin the song's tempo or bar signature changed.
    pub fn update_musical_context_content(&mut self, context: Ref) {
        let f = self.f().update_musical_context_content;
        // SAFETY: as above; a null range is the whole context.
        contain::call(|| unsafe { f(self.dc, context, std::ptr::null(), sys::UPDATE_EVERYTHING) });
    }

    pub fn destroy_musical_context(&mut self, context: Ref) {
        let f = self.f().destroy_musical_context;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, context) });
        self.live.remove(&(context as usize));
    }

    // Region sequence -------------------------------------------------------------------------

    /// A region sequence for a track (ARA 2). `None` when the plugin is ARA 1.
    pub fn create_region_sequence(&mut self, key: usize, name: &str, context: Ref) -> Option<Ref> {
        let f = self.f().create_region_sequence.filter(|_| {
            self.has(sys::implemented_size!(
                sys::DocumentControllerInterface,
                create_region_sequence
            ))
        })?;
        let name = cstring(name);
        let props = sys::RegionSequenceProperties {
            struct_size: std::mem::size_of::<sys::RegionSequenceProperties>(),
            name: name.as_ptr(),
            order_index: 0,
            musical_context_ref: context,
            color: std::ptr::null(),
        };
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, key as Ref, &props) });
        self.made(r, "region sequence").ok()
    }

    pub fn destroy_region_sequence(&mut self, sequence: Ref) {
        if let Some(f) = self.f().destroy_region_sequence {
            // SAFETY: as above.
            contain::call(|| unsafe { f(self.dc, sequence) });
        }
        self.live.remove(&(sequence as usize));
    }

    // Audio source ----------------------------------------------------------------------------

    fn source_props<'a>(
        p: &SourceProps,
        name: &'a CString,
        id: &'a CString,
    ) -> sys::AudioSourceProperties {
        sys::AudioSourceProperties {
            struct_size: std::mem::size_of::<sys::AudioSourceProperties>(),
            name: name.as_ptr(),
            persistent_id: id.as_ptr(),
            sample_count: p.frames,
            sample_rate: p.rate,
            channel_count: p.channels,
            merits_64_bit_samples: FALSE,
            channel_arrangement_data_type: 0,
            channel_arrangement: std::ptr::null(),
        }
    }

    pub fn create_audio_source(&mut self, key: usize, p: &SourceProps) -> Result<Ref, Error> {
        let (name, id) = (cstring(&p.name), cstring(&p.persistent_id));
        let props = Self::source_props(p, &name, &id);
        let f = self.f().create_audio_source;
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, key as Ref, &props) });
        self.made(r, "audio source")
    }

    /// New properties for a source (its rate after the song's rate changed, PLG-047). Its
    /// samples access must be off.
    pub fn update_audio_source_properties(&mut self, source: Ref, p: &SourceProps) {
        let (name, id) = (cstring(&p.name), cstring(&p.persistent_id));
        let props = Self::source_props(p, &name, &id);
        let f = self.f().update_audio_source_properties;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, source, &props) });
    }

    /// Tells the plugin the source's samples changed.
    pub fn update_audio_source_content(&mut self, source: Ref) {
        let f = self.f().update_audio_source_content;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, source, std::ptr::null(), sys::UPDATE_EVERYTHING) });
    }

    /// Lets the plugin read the source's samples, or stops it. The plugin destroys its readers
    /// before this returns when access is turned off.
    pub fn enable_samples_access(&mut self, source: Ref, on: bool) {
        let f = self.f().enable_audio_source_samples_access;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, source, if on { TRUE } else { FALSE }) });
    }

    pub fn destroy_audio_source(&mut self, source: Ref) {
        let f = self.f().destroy_audio_source;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, source) });
        self.live.remove(&(source as usize));
    }

    // Audio modification ----------------------------------------------------------------------

    pub fn create_audio_modification(
        &mut self,
        source: Ref,
        key: usize,
        name: &str,
        id: &str,
    ) -> Result<Ref, Error> {
        let (name, id) = (cstring(name), cstring(id));
        let props = sys::AudioModificationProperties {
            struct_size: std::mem::size_of::<sys::AudioModificationProperties>(),
            name: name.as_ptr(),
            persistent_id: id.as_ptr(),
        };
        let f = self.f().create_audio_modification;
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, source, key as Ref, &props) });
        self.made(r, "audio modification")
    }

    /// A copy of `modification` with its edits, for a duplicated clip (PLG-037).
    pub fn clone_audio_modification(
        &mut self,
        modification: Ref,
        key: usize,
        name: &str,
        id: &str,
    ) -> Result<Ref, Error> {
        let (name, id) = (cstring(name), cstring(id));
        let props = sys::AudioModificationProperties {
            struct_size: std::mem::size_of::<sys::AudioModificationProperties>(),
            name: name.as_ptr(),
            persistent_id: id.as_ptr(),
        };
        let f = self.f().clone_audio_modification;
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, modification, key as Ref, &props) });
        self.made(r, "audio modification copy")
    }

    pub fn destroy_audio_modification(&mut self, modification: Ref) {
        let f = self.f().destroy_audio_modification;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, modification) });
        self.live.remove(&(modification as usize));
    }

    // Playback region -------------------------------------------------------------------------

    fn region_props(p: &RegionProps, context: Ref, sequence: Ref) -> sys::PlaybackRegionProperties {
        sys::PlaybackRegionProperties {
            struct_size: std::mem::size_of::<sys::PlaybackRegionProperties>(),
            transformation_flags: sys::TRANSFORMATION_NONE,
            start_in_modification_time: p.start_in_modification,
            duration_in_modification_time: p.duration_in_modification,
            start_in_playback_time: p.start_in_playback,
            duration_in_playback_time: p.duration_in_playback,
            musical_context_ref: context,
            region_sequence_ref: sequence,
            name: std::ptr::null(),
            color: std::ptr::null(),
        }
    }

    pub fn create_playback_region(
        &mut self,
        modification: Ref,
        key: usize,
        p: &RegionProps,
        context: Ref,
        sequence: Ref,
    ) -> Result<Ref, Error> {
        let props = Self::region_props(p, context, sequence);
        let f = self.f().create_playback_region;
        // SAFETY: as above.
        let r = contain::call(|| unsafe { f(self.dc, modification, key as Ref, &props) });
        self.made(r, "playback region")
    }

    pub fn update_playback_region(
        &mut self,
        region: Ref,
        p: &RegionProps,
        context: Ref,
        sequence: Ref,
    ) {
        let props = Self::region_props(p, context, sequence);
        let f = self.f().update_playback_region_properties;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, region, &props) });
    }

    pub fn destroy_playback_region(&mut self, region: Ref) {
        let f = self.f().destroy_playback_region;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, region) });
        self.live.remove(&(region as usize));
    }

    /// The time the region's sound starts before and ends after it (reverb, a pre-attack).
    pub fn head_and_tail(&self, region: Ref) -> (f64, f64) {
        let Some(f) = self.f().get_playback_region_head_and_tail_time.filter(|_| {
            self.has(sys::implemented_size!(
                sys::DocumentControllerInterface,
                get_playback_region_head_and_tail_time
            ))
        }) else {
            return (0.0, 0.0);
        };
        let (mut head, mut tail) = (0.0, 0.0);
        // SAFETY: as above; both outs live through the call.
        contain::call(|| unsafe { f(self.dc, region, &mut head, &mut tail) });
        (head.max(0.0), tail.max(0.0))
    }

    // Analysis and content --------------------------------------------------------------------

    /// Asks the plugin to analyse `kinds` of the source (PLG-035).
    pub fn request_analysis(&mut self, source: Ref, kinds: &[ContentType]) {
        if kinds.is_empty() {
            return;
        }
        let f = self.f().request_audio_source_content_analysis;
        // SAFETY: as above; the list lives through the call.
        contain::call(|| unsafe { f(self.dc, source, kinds.len(), kinds.as_ptr()) });
    }

    /// The kinds among `kinds` whose analysis has not finished.
    pub fn incomplete(&self, source: Ref, kinds: &[ContentType]) -> Vec<ContentType> {
        let f = self.f().is_audio_source_content_analysis_incomplete;
        kinds
            .iter()
            .copied()
            // SAFETY: as above.
            .filter(|&k| contain::call(|| unsafe { f(self.dc, source, k) }) != FALSE)
            .collect()
    }

    pub fn source_content_available(&self, source: Ref, kind: ContentType) -> bool {
        let f = self.f().is_audio_source_content_available;
        // SAFETY: as above.
        contain::call(|| unsafe { f(self.dc, source, kind) }) != FALSE
    }

    /// The notes of a source (or of a modification, with its edits), empty when the plugin
    /// has none.
    pub fn notes(&self, object: Ref, of_modification: bool) -> Vec<Note> {
        let f = self.f();
        let (available, create): (
            unsafe extern "C" fn(Ref, Ref, ContentType) -> Bool,
            unsafe extern "C" fn(Ref, Ref, ContentType, *const sys::ContentTimeRange) -> Ref,
        ) = if of_modification {
            (
                f.is_audio_modification_content_available,
                f.create_audio_modification_content_reader,
            )
        } else {
            (
                f.is_audio_source_content_available,
                f.create_audio_source_content_reader,
            )
        };
        // SAFETY: as above.
        if contain::call(|| unsafe { available(self.dc, object, sys::CONTENT_NOTES) }) == FALSE {
            return Vec::new();
        }
        // SAFETY: as above.
        let reader = contain::call(|| unsafe {
            create(self.dc, object, sys::CONTENT_NOTES, std::ptr::null())
        });
        if reader.is_null() {
            return Vec::new();
        }
        let mut out = Vec::new();
        contain::call(|| {
            // SAFETY: a live reader of notes; each event is an `ARAContentNote` the plugin
            // keeps valid until the next call on the reader.
            unsafe {
                let n = (f.get_content_reader_event_count)(self.dc, reader).max(0);
                for i in 0..n {
                    let e = (f.get_content_reader_data_for_event)(self.dc, reader, i)
                        as *const sys::ContentNote;
                    if e.is_null() {
                        continue;
                    }
                    let e = *e;
                    out.push(Note {
                        frequency: e.frequency,
                        pitch: e.pitch_number,
                        volume: e.volume,
                        start: e.start_position,
                        duration: e.note_duration,
                    });
                }
                (f.destroy_content_reader)(self.dc, reader);
            }
        });
        out
    }

    // Archives --------------------------------------------------------------------------------

    /// Stores the whole document (PLG-037). The bytes, or `None` when the plugin failed.
    pub fn store(&mut self) -> Option<Vec<u8>> {
        let writer = ArchiveWriter::default();
        let w = &writer as *const ArchiveWriter as Ref;
        let f = self.f();
        let ok = if let Some(store) = f.store_objects_to_archive.filter(|_| {
            self.has(sys::implemented_size!(
                sys::DocumentControllerInterface,
                store_objects_to_archive
            ))
        }) {
            // SAFETY: as above; a null filter stores everything.
            contain::call(|| unsafe { store(self.dc, w, std::ptr::null()) })
        } else if let Some(store) = f.store_document_to_archive {
            // SAFETY: as above.
            contain::call(|| unsafe { store(self.dc, w) })
        } else {
            FALSE
        };
        if ok == FALSE {
            return None;
        }
        Some(
            writer
                .data
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// Restores an archive into the objects already made with the same persistent ids. Call
    /// it while editing (ARA 2), after making the objects.
    pub fn restore(&mut self, data: &[u8], archive_id: &str) -> bool {
        let reader = ArchiveReader {
            data: data.to_vec(),
            archive_id: cstring(archive_id),
        };
        let r = &reader as *const ArchiveReader as Ref;
        let f = self.f();
        let ok = if let Some(restore) = f.restore_objects_from_archive.filter(|_| {
            self.has(sys::implemented_size!(
                sys::DocumentControllerInterface,
                restore_objects_from_archive
            ))
        }) {
            debug_assert!(self.is_editing());
            // SAFETY: as above; a null filter restores everything.
            contain::call(|| unsafe { restore(self.dc, r, std::ptr::null()) })
        } else if let (Some(begin), Some(end)) = (
            f.begin_restoring_document_from_archive,
            f.end_restoring_document_from_archive,
        ) {
            // SAFETY: as above.
            contain::call(|| unsafe {
                let a = begin(self.dc, r);
                let b = end(self.dc, r);
                if a != FALSE && b != FALSE {
                    TRUE
                } else {
                    FALSE
                }
            })
        } else {
            FALSE
        };
        ok != FALSE
    }

    fn made(&mut self, r: Ref, what: &str) -> Result<Ref, Error> {
        if r.is_null() {
            return Err(error(format!("the plugin did not make the {what}")));
        }
        self.live.insert(r as usize);
        Ok(r)
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        while self.editing > 0 {
            self.end_editing();
        }
        let f = self.f().destroy_document_controller;
        // SAFETY: the controller is destroyed once, on its thread.
        contain::call(|| unsafe { f(self.dc) });
    }
}
