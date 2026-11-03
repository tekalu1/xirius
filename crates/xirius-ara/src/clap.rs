//! ARA in a CLAP plugin (`ARACLAP.h`, PLG-033).
//!
//! The entry's `get_factory` gives, next to the plugin factory, a `clap_ara_factory` that
//! lists each plugin id's ARA factory. A plugin instance answers the extension
//! `org.ara-audio.ara.pluginextension/2`, which binds it to the document with roles.
//!
//! The plugins of `xirius-clap` run on their own main thread behind clack; an ARA renderer is
//! driven from the ARA thread with the document, so this module binds CLAP's C API itself.

// The ARA references passed in are the plugin's opaque handles: the host hands them back to
// the plugin and never reads through them.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::{
    CLAP_TRANSPORT_HAS_SECONDS_TIMELINE, CLAP_TRANSPORT_IS_PLAYING, clap_event_header,
    clap_event_transport, clap_input_events, clap_output_events,
};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::CLAP_SECTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::clap_process;
use clap_sys::version::CLAP_VERSION;

use crate::service::{Binary, Instance};
use crate::sys::{self, Ref, RoleFlags};
use crate::{Error, contain, error};

pub const ARA_FACTORY: &CStr = c"org.ara-audio.ara.factory/2";
pub const ARA_FACTORY_DRAFT: &CStr = c"org.ara-audio.ara.factory.draft/2";
pub const ARA_EXTENSION: &CStr = c"org.ara-audio.ara.pluginextension/2";
pub const ARA_EXTENSION_DRAFT: &CStr = c"org.ara-audio.ara.pluginextension.draft/2";
/// The plugin works with or without ARA.
pub const FEATURE_ARA_SUPPORTED: &str = "ara:supported";
/// The plugin works only with ARA.
pub const FEATURE_ARA_REQUIRED: &str = "ara:required";

#[repr(C)]
pub struct clap_ara_factory {
    pub get_factory_count: unsafe extern "C" fn(*const clap_ara_factory) -> u32,
    pub get_ara_factory: unsafe extern "C" fn(*const clap_ara_factory, u32) -> *const sys::Factory,
    pub get_plugin_id: unsafe extern "C" fn(*const clap_ara_factory, u32) -> *const c_char,
}

#[repr(C)]
pub struct clap_ara_plugin_extension {
    pub get_factory: unsafe extern "C" fn(*const clap_plugin) -> *const sys::Factory,
    pub bind_to_document_controller: unsafe extern "C" fn(
        *const clap_plugin,
        Ref,
        RoleFlags,
        RoleFlags,
    )
        -> *const sys::PlugInExtensionInstance,
}

/// Whether a descriptor's features say the plugin speaks ARA.
pub fn has_ara_feature(features: &[String]) -> bool {
    features
        .iter()
        .any(|f| f == FEATURE_ARA_SUPPORTED || f == FEATURE_ARA_REQUIRED)
}

/// The ARA factory an entry gives for `plugin_id`.
///
/// # Safety
/// `entry` is an initialized, live CLAP entry.
pub unsafe fn ara_factory(
    entry: *const clap_plugin_entry,
    plugin_id: &str,
) -> Option<*const sys::Factory> {
    // SAFETY: per the contract.
    let get = unsafe { (*entry).get_factory }?;
    for id in [ARA_FACTORY, ARA_FACTORY_DRAFT] {
        // SAFETY: a plain query of a live entry.
        let f = contain::call(|| unsafe { get(id.as_ptr()) }) as *const clap_ara_factory;
        if f.is_null() {
            continue;
        }
        // SAFETY: the entry returned a live ARA factory list.
        let found = contain::call(|| unsafe {
            let n = ((*f).get_factory_count)(f);
            (0..n).find_map(|i| {
                let id = sys::string(((*f).get_plugin_id)(f, i));
                (id == plugin_id).then(|| ((*f).get_ara_factory)(f, i))
            })
        });
        if let Some(found) = found.filter(|p| !p.is_null()) {
            return Some(found);
        }
    }
    None
}

unsafe extern "C" fn host_extension(_host: *const clap_host, _id: *const c_char) -> *const c_void {
    std::ptr::null()
}

unsafe extern "C" fn host_request(_host: *const clap_host) {}

struct HostStrings {
    name: CString,
    vendor: CString,
    url: CString,
    version: CString,
}

/// The host CLAP plugins of the ARA thread see: no extensions, requests ignored (the
/// renderer is driven block by block anyway).
fn host() -> Box<(clap_host, HostStrings)> {
    let strings = HostStrings {
        name: c"Xirius".into(),
        vendor: c"Xirius".into(),
        url: c"".into(),
        version: CString::new(env!("CARGO_PKG_VERSION")).unwrap_or_default(),
    };
    let mut b = Box::new((
        clap_host {
            clap_version: CLAP_VERSION,
            host_data: std::ptr::null_mut(),
            name: std::ptr::null(),
            vendor: std::ptr::null(),
            url: std::ptr::null(),
            version: std::ptr::null(),
            get_extension: Some(host_extension),
            request_restart: Some(host_request),
            request_process: Some(host_request),
            request_callback: Some(host_request),
        },
        strings,
    ));
    let (h, s) = &mut *b;
    h.name = s.name.as_ptr();
    h.vendor = s.vendor.as_ptr();
    h.url = s.url.as_ptr();
    h.version = s.version.as_ptr();
    b
}

/// A CLAP module with an ARA factory for one of its plugins.
pub struct ClapBinary {
    entry: *const clap_plugin_entry,
    plugin_id: CString,
    factory: *const sys::Factory,
    host: Box<(clap_host, HostStrings)>,
}

impl ClapBinary {
    /// Loads the `.clap` at `path` (kept loaded to the end, like VST3 modules) and finds the
    /// ARA factory of `plugin_id`.
    #[cfg(windows)]
    pub fn load(path: &Path, plugin_id: &str) -> Result<ClapBinary, Error> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: a NUL-terminated path; the library is never freed.
        let lib = contain::call(|| unsafe { LoadLibraryW(wide.as_ptr()) });
        if lib.is_null() {
            return Err(error(format!("could not load {}", path.display())));
        }
        // SAFETY: a symbol lookup in the loaded library.
        let sym = unsafe { GetProcAddress(lib, c"clap_entry".as_ptr().cast()) }
            .ok_or_else(|| error(format!("{} has no clap_entry", path.display())))?;
        contain::add_module(sym as usize);
        let entry = sym as *const clap_plugin_entry;
        let p = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        // SAFETY: `clap_entry` is the module's entry struct; `init` runs once per load.
        let ok = contain::call(|| unsafe { (*entry).init.is_some_and(|init| init(p.as_ptr())) });
        if !ok {
            return Err(error("the CLAP entry did not initialize"));
        }
        // SAFETY: initialized above.
        unsafe { ClapBinary::from_entry(entry, plugin_id) }
    }

    #[cfg(not(windows))]
    pub fn load(path: &Path, _plugin_id: &str) -> Result<ClapBinary, Error> {
        Err(error(format!(
            "loading {} is only built on Windows",
            path.display()
        )))
    }

    /// A binary over an entry that is already initialized (also a test's own entry).
    ///
    /// # Safety
    /// `entry` is initialized and stays valid for the process.
    pub unsafe fn from_entry(
        entry: *const clap_plugin_entry,
        plugin_id: &str,
    ) -> Result<ClapBinary, Error> {
        // SAFETY: per the contract.
        let factory = unsafe { ara_factory(entry, plugin_id) }
            .ok_or_else(|| error(format!("{plugin_id} has no ARA factory")))?;
        Ok(ClapBinary {
            entry,
            plugin_id: CString::new(plugin_id).unwrap_or_default(),
            factory,
            host: host(),
        })
    }
}

impl Binary for ClapBinary {
    fn factory(&self) -> *const sys::Factory {
        self.factory
    }

    fn instance(
        &mut self,
        dc: Ref,
        known: RoleFlags,
        assigned: RoleFlags,
    ) -> Result<Box<dyn Instance>, Error> {
        // SAFETY: the entry is live; the plugin factory and the plugin follow CLAP's contract.
        let plugin = unsafe {
            let get = (*self.entry)
                .get_factory
                .ok_or_else(|| error("the CLAP entry has no factories"))?;
            let pf = contain::call(|| get(CLAP_PLUGIN_FACTORY_ID.as_ptr()))
                as *const clap_plugin_factory;
            let create = pf
                .as_ref()
                .and_then(|f| f.create_plugin)
                .ok_or_else(|| error("the CLAP entry has no plugin factory"))?;
            let p = contain::call(|| create(pf, &self.host.0, self.plugin_id.as_ptr()));
            if p.is_null() {
                return Err(error("the CLAP plugin could not be made"));
            }
            if !contain::call(|| (*p).init.is_some_and(|init| init(p))) {
                if let Some(destroy) = (*p).destroy {
                    contain::call(|| destroy(p));
                }
                return Err(error("the CLAP plugin did not initialize"));
            }
            p
        };
        // SAFETY: an initialized plugin; destroyed by the instance.
        unsafe { ClapInstance::bind(plugin, dc, known, assigned) }
            .map(|i| Box::new(i) as Box<dyn Instance>)
    }
}

/// One CLAP plugin instance bound to the document. Destroys the plugin when dropped.
pub struct ClapInstance {
    plugin: *const clap_plugin,
    ext: sys::PlugInExtensionInstance,
    active: bool,
    transport: clap_event_transport,
    steady: i64,
    rate: f64,
}

unsafe extern "C" fn no_events(_list: *const clap_input_events) -> u32 {
    0
}

unsafe extern "C" fn no_event(
    _list: *const clap_input_events,
    _i: u32,
) -> *const clap_event_header {
    std::ptr::null()
}

unsafe extern "C" fn drop_event(
    _list: *const clap_output_events,
    _e: *const clap_event_header,
) -> bool {
    true
}

static IN_EVENTS: clap_input_events = clap_input_events {
    ctx: std::ptr::null_mut(),
    size: Some(no_events),
    get: Some(no_event),
};

static OUT_EVENTS: clap_output_events = clap_output_events {
    ctx: std::ptr::null_mut(),
    try_push: Some(drop_event),
};

impl ClapInstance {
    /// Binds an initialized plugin to the document controller `dc`.
    ///
    /// # Safety
    /// `plugin` is an initialized CLAP plugin owned by the caller; the instance takes it.
    pub unsafe fn bind(
        plugin: *const clap_plugin,
        dc: Ref,
        known: RoleFlags,
        assigned: RoleFlags,
    ) -> Result<ClapInstance, Error> {
        // SAFETY: per the contract.
        let ext = unsafe {
            let get = (*plugin).get_extension;
            let e = [ARA_EXTENSION, ARA_EXTENSION_DRAFT].iter().find_map(|id| {
                let e = contain::call(|| get.map_or(std::ptr::null(), |g| g(plugin, id.as_ptr())));
                (!e.is_null()).then_some(e as *const clap_ara_plugin_extension)
            });
            let Some(e) = e else {
                if let Some(destroy) = (*plugin).destroy {
                    contain::call(|| destroy(plugin));
                }
                return Err(error("the CLAP plugin has no ARA extension"));
            };
            contain::call(|| ((*e).bind_to_document_controller)(plugin, dc, known, assigned))
        };
        let mut instance = ClapInstance {
            plugin,
            ext: sys::PlugInExtensionInstance {
                struct_size: 0,
                plug_in_extension_ref: std::ptr::null_mut(),
                plug_in_extension_interface: std::ptr::null(),
                playback_renderer_ref: std::ptr::null_mut(),
                playback_renderer_interface: std::ptr::null(),
                editor_renderer_ref: std::ptr::null_mut(),
                editor_renderer_interface: std::ptr::null(),
                editor_view_ref: std::ptr::null_mut(),
                editor_view_interface: std::ptr::null(),
            },
            active: false,
            // SAFETY: plain data; all-zero is valid.
            transport: unsafe { std::mem::zeroed() },
            steady: 0,
            rate: 0.0,
        };
        if ext.is_null() {
            return Err(error("the CLAP plugin did not bind to the document"));
        }
        // SAFETY: the plugin keeps the instance valid while it lives; copied by value. Set in
        // place: a copy made with `..instance` would leave `instance` to drop, destroying the
        // plugin.
        instance.ext = unsafe { *ext };
        Ok(instance)
    }

    fn stop(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // SAFETY: the plugin is active and processing; undone in order.
        unsafe {
            if let Some(stop) = (*self.plugin).stop_processing {
                contain::call(|| stop(self.plugin));
            }
            if let Some(deactivate) = (*self.plugin).deactivate {
                contain::call(|| deactivate(self.plugin));
            }
        }
    }
}

impl Instance for ClapInstance {
    fn extension(&self) -> sys::PlugInExtensionInstance {
        self.ext
    }

    fn prepare(&mut self, rate: f64, max_block: usize) -> Result<u32, Error> {
        self.stop();
        // SAFETY: an initialized plugin, activated from the thread that made it.
        let ok = unsafe {
            let activate = (*self.plugin).activate;
            contain::call(|| activate.is_some_and(|a| a(self.plugin, rate, 1, max_block as u32)))
                && contain::call(|| {
                    (*self.plugin)
                        .start_processing
                        .is_none_or(|s| s(self.plugin))
                })
        };
        if !ok {
            return Err(error("the CLAP plugin did not activate"));
        }
        self.active = true;
        self.rate = rate;
        Ok(0)
    }

    fn set_transport(&mut self, playing: bool, position: i64) {
        let t = &mut self.transport;
        t.header.size = std::mem::size_of::<clap_event_transport>() as u32;
        t.flags = CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
        if playing {
            t.flags |= CLAP_TRANSPORT_IS_PLAYING;
        }
        let seconds = if self.rate > 0.0 {
            position as f64 / self.rate
        } else {
            0.0
        };
        t.song_pos_seconds = (seconds * CLAP_SECTIME_FACTOR as f64).round() as i64;
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.active {
            return;
        }
        let frames = left.len().min(right.len());
        let mut inputs = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut out_l = vec![0.0f32; frames];
        let mut out_r = vec![0.0f32; frames];
        let mut outputs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let input = clap_audio_buffer {
            data32: inputs.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut output = clap_audio_buffer {
            data32: outputs.as_mut_ptr(),
            ..input
        };
        let process = clap_process {
            steady_time: self.steady,
            frames_count: frames as u32,
            transport: &self.transport,
            audio_inputs: &input,
            audio_outputs: &mut output,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &IN_EVENTS,
            out_events: &OUT_EVENTS,
        };
        // SAFETY: an active plugin; every buffer lives through the call.
        unsafe {
            if let Some(p) = (*self.plugin).process {
                contain::call(|| p(self.plugin, &process));
            }
        }
        self.steady += frames as i64;
        let seconds = frames as f64 / self.rate.max(1.0);
        self.transport.song_pos_seconds += (seconds * CLAP_SECTIME_FACTOR as f64).round() as i64;
        left[..frames].copy_from_slice(&out_l);
        right[..frames].copy_from_slice(&out_r);
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: the plugin is destroyed once, on its thread.
        unsafe {
            if let Some(destroy) = (*self.plugin).destroy {
                contain::call(|| destroy(self.plugin));
            }
        }
    }
}
