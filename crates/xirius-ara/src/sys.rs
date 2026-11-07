//! The C API of ARA 2.3 (`third_party/ara_api/ARAInterface.h`), written out by hand for what
//! the host uses (LIC-004, PLG-031).
//!
//! The header packs every struct to 1 byte on x86 and x64 and to 8 bytes on ARM; [`ara_struct`]
//! does the same. Calls are `extern "C"` (`ARA_CALL` is `__cdecl`). Members added after the
//! first version of a struct are read only when the struct's `struct_size` covers them
//! ([`covers`]). Names follow the header, in Rust's case.
#![allow(non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_void};

pub type Bool = i32;
pub type Size = usize;
pub type Utf8String = *const c_char;
pub type PersistentId = *const c_char;
pub type ContentType = i32;
pub type ContentGrade = i32;
pub type ContentUpdateFlags = i32;
pub type PlaybackTransformationFlags = i32;
pub type RoleFlags = i32;
pub type ApiGeneration = i32;

pub const TRUE: Bool = 1;
pub const FALSE: Bool = 0;

pub const API_GENERATION_2_0_FINAL: ApiGeneration = 4;
pub const API_GENERATION_2_X_DRAFT: ApiGeneration = 5;
pub const API_GENERATION_2_3_FINAL: ApiGeneration = 6;

pub const CONTENT_NOTES: ContentType = 10;
pub const CONTENT_TEMPO_ENTRIES: ContentType = 20;
pub const CONTENT_BAR_SIGNATURES: ContentType = 21;
pub const CONTENT_STATIC_TUNING: ContentType = 31;
pub const CONTENT_KEY_SIGNATURES: ContentType = 42;
pub const CONTENT_SHEET_CHORDS: ContentType = 45;

pub const GRADE_INITIAL: ContentGrade = 0;
pub const GRADE_DETECTED: ContentGrade = 1;
pub const GRADE_ADJUSTED: ContentGrade = 2;
pub const GRADE_APPROVED: ContentGrade = 3;

pub const UPDATE_EVERYTHING: ContentUpdateFlags = 0;
pub const UPDATE_SIGNAL_SCOPE_REMAINS_UNCHANGED: ContentUpdateFlags = 1;
pub const UPDATE_NOTE_SCOPE_REMAINS_UNCHANGED: ContentUpdateFlags = 1 << 1;
pub const UPDATE_TUNING_SCOPE_REMAINS_UNCHANGED: ContentUpdateFlags = 1 << 3;
pub const UPDATE_HARMONIC_SCOPE_REMAINS_UNCHANGED: ContentUpdateFlags = 1 << 4;

pub const ANALYSIS_STARTED: i32 = 0;
pub const ANALYSIS_UPDATED: i32 = 1;
pub const ANALYSIS_COMPLETED: i32 = 2;

pub const TRANSFORMATION_NONE: PlaybackTransformationFlags = 0;
pub const TRANSFORMATION_TIMESTRETCH: PlaybackTransformationFlags = 1;
pub const TRANSFORMATION_TIMESTRETCH_REFLECTING_TEMPO: PlaybackTransformationFlags = 2;

pub const ROLE_PLAYBACK_RENDERER: RoleFlags = 1;
pub const ROLE_EDITOR_RENDERER: RoleFlags = 2;
pub const ROLE_EDITOR_VIEW: RoleFlags = 4;
pub const ROLES_ALL: RoleFlags = ROLE_PLAYBACK_RENDERER | ROLE_EDITOR_RENDERER | ROLE_EDITOR_VIEW;

/// An object of one side, as the other side sees it: an address it never reads through.
pub type Ref = *mut c_void;

/// Declares a struct with the header's packing.
macro_rules! ara_struct {
    ($(#[$m:meta])* pub struct $name:ident { $($(#[$fm:meta])* pub $field:ident : $ty:ty,)* }) => {
        $(#[$m])*
        #[cfg_attr(any(target_arch = "x86", target_arch = "x86_64"), repr(C, packed(1)))]
        #[cfg_attr(not(any(target_arch = "x86", target_arch = "x86_64")), repr(C, packed(8)))]
        #[derive(Clone, Copy)]
        pub struct $name { $($(#[$fm])* pub $field: $ty,)* }
    };
}

/// `ARA_IMPLEMENTED_STRUCT_SIZE`: the size up to and including `$field`.
macro_rules! implemented_size {
    ($ty:ty, $field:ident) => {
        std::mem::offset_of!($ty, $field) + {
            const fn size_of_return<T, U>(_: fn(&T) -> U) -> usize {
                std::mem::size_of::<U>()
            }
            size_of_return(|s: &$ty| s.$field)
        }
    };
}
pub(crate) use implemented_size;

ara_struct! {
    pub struct Color { pub r: f32, pub g: f32, pub b: f32, }
}

ara_struct! {
    pub struct DocumentProperties { pub struct_size: Size, pub name: Utf8String, }
}

ara_struct! {
    pub struct MusicalContextProperties {
        pub struct_size: Size,
        pub name: Utf8String,
        pub order_index: i32,
        pub color: *const Color,
    }
}

ara_struct! {
    pub struct RegionSequenceProperties {
        pub struct_size: Size,
        pub name: Utf8String,
        pub order_index: i32,
        pub musical_context_ref: Ref,
        pub color: *const Color,
    }
}

ara_struct! {
    pub struct AudioSourceProperties {
        pub struct_size: Size,
        pub name: Utf8String,
        pub persistent_id: PersistentId,
        pub sample_count: i64,
        pub sample_rate: f64,
        pub channel_count: i32,
        pub merits_64_bit_samples: Bool,
        pub channel_arrangement_data_type: i32,
        pub channel_arrangement: *const c_void,
    }
}

ara_struct! {
    pub struct AudioModificationProperties {
        pub struct_size: Size,
        pub name: Utf8String,
        pub persistent_id: PersistentId,
    }
}

ara_struct! {
    pub struct PlaybackRegionProperties {
        pub struct_size: Size,
        pub transformation_flags: PlaybackTransformationFlags,
        pub start_in_modification_time: f64,
        pub duration_in_modification_time: f64,
        pub start_in_playback_time: f64,
        pub duration_in_playback_time: f64,
        pub musical_context_ref: Ref,
        pub region_sequence_ref: Ref,
        pub name: Utf8String,
        pub color: *const Color,
    }
}

ara_struct! {
    pub struct ContentTimeRange { pub start: f64, pub duration: f64, }
}

ara_struct! {
    pub struct ContentTempoEntry { pub time_position: f64, pub quarter_position: f64, }
}

ara_struct! {
    pub struct ContentBarSignature { pub numerator: i32, pub denominator: i32, pub position: f64, }
}

ara_struct! {
    pub struct ContentNote {
        pub frequency: f32,
        pub pitch_number: i32,
        pub volume: f32,
        pub start_position: f64,
        pub attack_duration: f64,
        pub note_duration: f64,
        pub signal_duration: f64,
    }
}

// Host controllers -----------------------------------------------------------------------------

pub type AssertFunction =
    unsafe extern "C" fn(category: i32, problem: *const c_void, diagnosis: *const c_char);

ara_struct! {
    pub struct AudioAccessControllerInterface {
        pub struct_size: Size,
        pub create_audio_reader_for_source:
            unsafe extern "C" fn(ctrl: Ref, source: Ref, use_64_bit: Bool) -> Ref,
        pub read_audio_samples: unsafe extern "C" fn(
            ctrl: Ref,
            reader: Ref,
            position: i64,
            count: i64,
            buffers: *const *mut c_void,
        ) -> Bool,
        pub destroy_audio_reader: unsafe extern "C" fn(ctrl: Ref, reader: Ref),
    }
}

ara_struct! {
    pub struct ArchivingControllerInterface {
        pub struct_size: Size,
        pub get_archive_size: unsafe extern "C" fn(ctrl: Ref, reader: Ref) -> Size,
        pub read_bytes_from_archive:
            unsafe extern "C" fn(ctrl: Ref, reader: Ref, position: Size, length: Size, buffer: *mut u8) -> Bool,
        pub write_bytes_to_archive:
            unsafe extern "C" fn(ctrl: Ref, writer: Ref, position: Size, length: Size, buffer: *const u8) -> Bool,
        pub notify_document_archiving_progress: unsafe extern "C" fn(ctrl: Ref, value: f32),
        pub notify_document_unarchiving_progress: unsafe extern "C" fn(ctrl: Ref, value: f32),
        pub get_document_archive_id: unsafe extern "C" fn(ctrl: Ref, reader: Ref) -> PersistentId,
    }
}

ara_struct! {
    pub struct ContentAccessControllerInterface {
        pub struct_size: Size,
        pub is_musical_context_content_available:
            unsafe extern "C" fn(ctrl: Ref, context: Ref, kind: ContentType) -> Bool,
        pub get_musical_context_content_grade:
            unsafe extern "C" fn(ctrl: Ref, context: Ref, kind: ContentType) -> ContentGrade,
        pub create_musical_context_content_reader: unsafe extern "C" fn(
            ctrl: Ref,
            context: Ref,
            kind: ContentType,
            range: *const ContentTimeRange,
        ) -> Ref,
        pub is_audio_source_content_available:
            unsafe extern "C" fn(ctrl: Ref, source: Ref, kind: ContentType) -> Bool,
        pub get_audio_source_content_grade:
            unsafe extern "C" fn(ctrl: Ref, source: Ref, kind: ContentType) -> ContentGrade,
        pub create_audio_source_content_reader: unsafe extern "C" fn(
            ctrl: Ref,
            source: Ref,
            kind: ContentType,
            range: *const ContentTimeRange,
        ) -> Ref,
        pub get_content_reader_event_count: unsafe extern "C" fn(ctrl: Ref, reader: Ref) -> i32,
        pub get_content_reader_data_for_event:
            unsafe extern "C" fn(ctrl: Ref, reader: Ref, index: i32) -> *const c_void,
        pub destroy_content_reader: unsafe extern "C" fn(ctrl: Ref, reader: Ref),
    }
}

ara_struct! {
    pub struct ModelUpdateControllerInterface {
        pub struct_size: Size,
        pub notify_audio_source_analysis_progress:
            unsafe extern "C" fn(ctrl: Ref, source: Ref, state: i32, value: f32),
        pub notify_audio_source_content_changed: unsafe extern "C" fn(
            ctrl: Ref,
            source: Ref,
            range: *const ContentTimeRange,
            flags: ContentUpdateFlags,
        ),
        pub notify_audio_modification_content_changed: unsafe extern "C" fn(
            ctrl: Ref,
            modification: Ref,
            range: *const ContentTimeRange,
            flags: ContentUpdateFlags,
        ),
        pub notify_playback_region_content_changed: unsafe extern "C" fn(
            ctrl: Ref,
            region: Ref,
            range: *const ContentTimeRange,
            flags: ContentUpdateFlags,
        ),
        pub notify_document_data_changed: unsafe extern "C" fn(ctrl: Ref),
    }
}

ara_struct! {
    pub struct PlaybackControllerInterface {
        pub struct_size: Size,
        pub request_start_playback: unsafe extern "C" fn(ctrl: Ref),
        pub request_stop_playback: unsafe extern "C" fn(ctrl: Ref),
        pub request_set_playback_position: unsafe extern "C" fn(ctrl: Ref, time: f64),
        pub request_set_cycle_range: unsafe extern "C" fn(ctrl: Ref, start: f64, duration: f64),
        pub request_enable_cycle: unsafe extern "C" fn(ctrl: Ref, enable: Bool),
    }
}

ara_struct! {
    pub struct DocumentControllerHostInstance {
        pub struct_size: Size,
        pub audio_access_controller_host_ref: Ref,
        pub audio_access_controller_interface: *const AudioAccessControllerInterface,
        pub archiving_controller_host_ref: Ref,
        pub archiving_controller_interface: *const ArchivingControllerInterface,
        pub content_access_controller_host_ref: Ref,
        pub content_access_controller_interface: *const ContentAccessControllerInterface,
        pub model_update_controller_host_ref: Ref,
        pub model_update_controller_interface: *const ModelUpdateControllerInterface,
        pub playback_controller_host_ref: Ref,
        pub playback_controller_interface: *const PlaybackControllerInterface,
    }
}

// The plug-in's side ----------------------------------------------------------------------------

ara_struct! {
    pub struct RestoreObjectsFilter {
        pub struct_size: Size,
        pub document_data: Bool,
        pub audio_source_ids_count: Size,
        pub audio_source_archive_ids: *const PersistentId,
        pub audio_source_current_ids: *const PersistentId,
        pub audio_modification_ids_count: Size,
        pub audio_modification_archive_ids: *const PersistentId,
        pub audio_modification_current_ids: *const PersistentId,
    }
}

ara_struct! {
    pub struct StoreObjectsFilter {
        pub struct_size: Size,
        pub document_data: Bool,
        pub audio_source_refs_count: Size,
        pub audio_source_refs: *const Ref,
        pub audio_modification_refs_count: Size,
        pub audio_modification_refs: *const Ref,
    }
}

ara_struct! {
    pub struct ProcessingAlgorithmProperties {
        pub struct_size: Size,
        pub persistent_id: PersistentId,
        pub name: Utf8String,
    }
}

type Dc = Ref;

ara_struct! {
    pub struct DocumentControllerInterface {
        pub struct_size: Size,
        pub destroy_document_controller: unsafe extern "C" fn(Dc),
        pub get_factory: unsafe extern "C" fn(Dc) -> *const Factory,
        pub begin_editing: unsafe extern "C" fn(Dc),
        pub end_editing: unsafe extern "C" fn(Dc),
        pub notify_model_updates: unsafe extern "C" fn(Dc),
        pub begin_restoring_document_from_archive: Option<unsafe extern "C" fn(Dc, Ref) -> Bool>,
        pub end_restoring_document_from_archive: Option<unsafe extern "C" fn(Dc, Ref) -> Bool>,
        pub store_document_to_archive: Option<unsafe extern "C" fn(Dc, Ref) -> Bool>,
        pub update_document_properties: unsafe extern "C" fn(Dc, *const DocumentProperties),
        pub create_musical_context:
            unsafe extern "C" fn(Dc, Ref, *const MusicalContextProperties) -> Ref,
        pub update_musical_context_properties:
            unsafe extern "C" fn(Dc, Ref, *const MusicalContextProperties),
        pub update_musical_context_content:
            unsafe extern "C" fn(Dc, Ref, *const ContentTimeRange, ContentUpdateFlags),
        pub destroy_musical_context: unsafe extern "C" fn(Dc, Ref),
        pub create_audio_source: unsafe extern "C" fn(Dc, Ref, *const AudioSourceProperties) -> Ref,
        pub update_audio_source_properties: unsafe extern "C" fn(Dc, Ref, *const AudioSourceProperties),
        pub update_audio_source_content:
            unsafe extern "C" fn(Dc, Ref, *const ContentTimeRange, ContentUpdateFlags),
        pub enable_audio_source_samples_access: unsafe extern "C" fn(Dc, Ref, Bool),
        pub deactivate_audio_source_for_undo_history: unsafe extern "C" fn(Dc, Ref, Bool),
        pub destroy_audio_source: unsafe extern "C" fn(Dc, Ref),
        pub create_audio_modification:
            unsafe extern "C" fn(Dc, Ref, Ref, *const AudioModificationProperties) -> Ref,
        pub clone_audio_modification:
            unsafe extern "C" fn(Dc, Ref, Ref, *const AudioModificationProperties) -> Ref,
        pub update_audio_modification_properties:
            unsafe extern "C" fn(Dc, Ref, *const AudioModificationProperties),
        pub deactivate_audio_modification_for_undo_history: unsafe extern "C" fn(Dc, Ref, Bool),
        pub destroy_audio_modification: unsafe extern "C" fn(Dc, Ref),
        pub create_playback_region:
            unsafe extern "C" fn(Dc, Ref, Ref, *const PlaybackRegionProperties) -> Ref,
        pub update_playback_region_properties:
            unsafe extern "C" fn(Dc, Ref, *const PlaybackRegionProperties),
        pub destroy_playback_region: unsafe extern "C" fn(Dc, Ref),
        pub is_audio_source_content_available: unsafe extern "C" fn(Dc, Ref, ContentType) -> Bool,
        pub is_audio_source_content_analysis_incomplete:
            unsafe extern "C" fn(Dc, Ref, ContentType) -> Bool,
        pub request_audio_source_content_analysis:
            unsafe extern "C" fn(Dc, Ref, Size, *const ContentType),
        pub get_audio_source_content_grade: unsafe extern "C" fn(Dc, Ref, ContentType) -> ContentGrade,
        pub create_audio_source_content_reader:
            unsafe extern "C" fn(Dc, Ref, ContentType, *const ContentTimeRange) -> Ref,
        pub is_audio_modification_content_available:
            unsafe extern "C" fn(Dc, Ref, ContentType) -> Bool,
        pub get_audio_modification_content_grade:
            unsafe extern "C" fn(Dc, Ref, ContentType) -> ContentGrade,
        pub create_audio_modification_content_reader:
            unsafe extern "C" fn(Dc, Ref, ContentType, *const ContentTimeRange) -> Ref,
        pub is_playback_region_content_available: unsafe extern "C" fn(Dc, Ref, ContentType) -> Bool,
        pub get_playback_region_content_grade:
            unsafe extern "C" fn(Dc, Ref, ContentType) -> ContentGrade,
        pub create_playback_region_content_reader:
            unsafe extern "C" fn(Dc, Ref, ContentType, *const ContentTimeRange) -> Ref,
        pub get_content_reader_event_count: unsafe extern "C" fn(Dc, Ref) -> i32,
        pub get_content_reader_data_for_event: unsafe extern "C" fn(Dc, Ref, i32) -> *const c_void,
        pub destroy_content_reader: unsafe extern "C" fn(Dc, Ref),
        pub create_region_sequence:
            Option<unsafe extern "C" fn(Dc, Ref, *const RegionSequenceProperties) -> Ref>,
        pub update_region_sequence_properties:
            Option<unsafe extern "C" fn(Dc, Ref, *const RegionSequenceProperties)>,
        pub destroy_region_sequence: Option<unsafe extern "C" fn(Dc, Ref)>,
        pub get_playback_region_head_and_tail_time:
            Option<unsafe extern "C" fn(Dc, Ref, *mut f64, *mut f64)>,
        pub restore_objects_from_archive:
            Option<unsafe extern "C" fn(Dc, Ref, *const RestoreObjectsFilter) -> Bool>,
        pub store_objects_to_archive:
            Option<unsafe extern "C" fn(Dc, Ref, *const StoreObjectsFilter) -> Bool>,
        pub get_processing_algorithms_count: Option<unsafe extern "C" fn(Dc) -> i32>,
        pub get_processing_algorithm_properties:
            Option<unsafe extern "C" fn(Dc, i32) -> *const ProcessingAlgorithmProperties>,
        pub get_processing_algorithm_for_audio_source: Option<unsafe extern "C" fn(Dc, Ref) -> i32>,
        pub request_processing_algorithm_for_audio_source: Option<unsafe extern "C" fn(Dc, Ref, i32)>,
        pub is_licensed_for_capabilities: Option<
            unsafe extern "C" fn(Dc, Bool, Size, *const ContentType, PlaybackTransformationFlags) -> Bool,
        >,
        pub store_audio_source_to_audio_file_chunk:
            Option<unsafe extern "C" fn(Dc, Ref, Ref, *mut PersistentId, *mut Bool) -> Bool>,
        pub is_audio_modification_preserving_audio_source_signal:
            Option<unsafe extern "C" fn(Dc, Ref) -> Bool>,
    }
}

ara_struct! {
    pub struct DocumentControllerInstance {
        pub struct_size: Size,
        pub document_controller_ref: Ref,
        pub document_controller_interface: *const DocumentControllerInterface,
    }
}

ara_struct! {
    pub struct InterfaceConfiguration {
        pub struct_size: Size,
        pub desired_api_generation: ApiGeneration,
        pub assert_function_address: *mut AssertFunction,
    }
}

ara_struct! {
    pub struct Factory {
        pub struct_size: Size,
        pub lowest_supported_api_generation: ApiGeneration,
        pub highest_supported_api_generation: ApiGeneration,
        pub factory_id: PersistentId,
        pub initialize_ara_with_configuration: unsafe extern "C" fn(*const InterfaceConfiguration),
        pub uninitialize_ara: unsafe extern "C" fn(),
        pub plug_in_name: Utf8String,
        pub manufacturer_name: Utf8String,
        pub information_url: Utf8String,
        pub version: Utf8String,
        pub create_document_controller_with_document: unsafe extern "C" fn(
            *const DocumentControllerHostInstance,
            *const DocumentProperties,
        ) -> *const DocumentControllerInstance,
        pub document_archive_id: PersistentId,
        pub compatible_document_archive_ids_count: Size,
        pub compatible_document_archive_ids: *const PersistentId,
        pub analyzeable_content_types_count: Size,
        pub analyzeable_content_types: *const ContentType,
        pub supported_playback_transformation_flags: PlaybackTransformationFlags,
        pub supports_storing_audio_file_chunks: Bool,
    }
}

ara_struct! {
    pub struct PlaybackRendererInterface {
        pub struct_size: Size,
        pub add_playback_region: unsafe extern "C" fn(Ref, Ref),
        pub remove_playback_region: unsafe extern "C" fn(Ref, Ref),
    }
}

ara_struct! {
    pub struct EditorRendererInterface {
        pub struct_size: Size,
        pub add_playback_region: unsafe extern "C" fn(Ref, Ref),
        pub remove_playback_region: unsafe extern "C" fn(Ref, Ref),
        pub add_region_sequence: unsafe extern "C" fn(Ref, Ref),
        pub remove_region_sequence: unsafe extern "C" fn(Ref, Ref),
    }
}

ara_struct! {
    pub struct ViewSelection {
        pub struct_size: Size,
        pub playback_region_refs_count: Size,
        pub playback_region_refs: *const Ref,
        pub region_sequence_refs_count: Size,
        pub region_sequence_refs: *const Ref,
        pub time_range: *const ContentTimeRange,
    }
}

ara_struct! {
    pub struct EditorViewInterface {
        pub struct_size: Size,
        pub notify_selection: unsafe extern "C" fn(Ref, *const ViewSelection),
        pub notify_hide_region_sequences: unsafe extern "C" fn(Ref, Size, *const Ref),
    }
}

ara_struct! {
    pub struct PlugInExtensionInstance {
        pub struct_size: Size,
        pub plug_in_extension_ref: Ref,
        pub plug_in_extension_interface: *const c_void,
        pub playback_renderer_ref: Ref,
        pub playback_renderer_interface: *const PlaybackRendererInterface,
        pub editor_renderer_ref: Ref,
        pub editor_renderer_interface: *const EditorRendererInterface,
        pub editor_view_ref: Ref,
        pub editor_view_interface: *const EditorViewInterface,
    }
}

/// The smallest document controller the header allows (`kARADocumentControllerInterfaceMinSize`).
pub const DOCUMENT_CONTROLLER_MIN_SIZE: usize =
    implemented_size!(DocumentControllerInterface, destroy_content_reader);
/// The whole `ARAViewSelection`.
pub const VIEW_SELECTION_SIZE: usize = implemented_size!(ViewSelection, time_range);
/// `kARAFactoryMinSize`.
pub const FACTORY_MIN_SIZE: usize =
    implemented_size!(Factory, supported_playback_transformation_flags);

/// Reads a string the other side owns. Null reads as empty.
///
/// # Safety
/// `s` is null or a NUL-terminated string that stays valid during the call.
pub unsafe fn string(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    // SAFETY: per the contract.
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sizes the C compiler gives these structs with the header's packing (checked
    /// against `sizeof` on x64 with MSVC).
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn struct_sizes_match_the_packed_header() {
        use std::mem::size_of;
        assert_eq!(size_of::<Color>(), 12);
        assert_eq!(size_of::<DocumentProperties>(), 16);
        assert_eq!(size_of::<MusicalContextProperties>(), 28);
        assert_eq!(size_of::<RegionSequenceProperties>(), 36);
        assert_eq!(size_of::<AudioSourceProperties>(), 60);
        assert_eq!(size_of::<AudioModificationProperties>(), 24);
        assert_eq!(size_of::<PlaybackRegionProperties>(), 76);
        assert_eq!(size_of::<ContentTimeRange>(), 16);
        assert_eq!(size_of::<ContentTempoEntry>(), 16);
        assert_eq!(size_of::<ContentBarSignature>(), 16);
        assert_eq!(size_of::<ContentNote>(), 44);
        assert_eq!(size_of::<AudioAccessControllerInterface>(), 32);
        assert_eq!(size_of::<ArchivingControllerInterface>(), 56);
        assert_eq!(size_of::<ContentAccessControllerInterface>(), 80);
        assert_eq!(size_of::<ModelUpdateControllerInterface>(), 48);
        assert_eq!(size_of::<PlaybackControllerInterface>(), 48);
        assert_eq!(size_of::<DocumentControllerHostInstance>(), 88);
        assert_eq!(size_of::<RestoreObjectsFilter>(), 60);
        assert_eq!(size_of::<StoreObjectsFilter>(), 44);
        assert_eq!(size_of::<ProcessingAlgorithmProperties>(), 24);
        assert_eq!(size_of::<DocumentControllerInterface>(), 440);
        assert_eq!(size_of::<DocumentControllerInstance>(), 24);
        assert_eq!(size_of::<InterfaceConfiguration>(), 20);
        assert_eq!(size_of::<Factory>(), 128);
        assert_eq!(size_of::<PlaybackRendererInterface>(), 24);
        assert_eq!(size_of::<EditorRendererInterface>(), 40);
        assert_eq!(size_of::<ViewSelection>(), 48);
        assert_eq!(size_of::<EditorViewInterface>(), 24);
        assert_eq!(size_of::<PlugInExtensionInstance>(), 72);
        assert_eq!(DOCUMENT_CONTROLLER_MIN_SIZE, 336);
        assert_eq!(FACTORY_MIN_SIZE, 124);
    }
}
