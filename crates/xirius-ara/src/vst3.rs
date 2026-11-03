//! ARA in a VST3 plugin (`ARAVST3.h`, PLG-033).
//!
//! The module exports an `ARA Main Factory Class` named like its audio processor; its
//! `IMainFactory` gives the ARA factory the document is made from. Each processor instance
//! answers `IPlugInEntryPoint2`, which binds it to the document with roles. Binding happens
//! after the instance is made and before it is set up for processing.

// The ARA references passed in are the plugin's opaque handles: the host hands them back to
// the plugin and never reads through them.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use vst3::Steinberg::{TUID, tresult};
use xirius_vst3::{AudioPart, Caption, ClassInfo, Editor, Module, Placement, Plugin};

use crate::service::{Binary, Instance, Window};
use crate::sys::{self, Ref, RoleFlags};
use crate::{Error, contain, error};

/// The category of the class that gives the ARA factory.
pub const MAIN_FACTORY_CLASS: &str = "ARA Main Factory Class";

const fn iid(t: TUID) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = t[i] as u8;
        i += 1;
    }
    out
}

pub const IID_MAIN_FACTORY: [u8; 16] =
    iid(vst3::uid(0xDB2A1669, 0xFAFD42A5, 0xA82F864F, 0x7B6872EA));
pub const IID_ENTRY_POINT: [u8; 16] =
    iid(vst3::uid(0x12814E54, 0xA1CE4076, 0x82B96813, 0x16950BD6));
pub const IID_ENTRY_POINT_2: [u8; 16] =
    iid(vst3::uid(0xCD9A5913, 0xC9EB46D7, 0x96CA53AD, 0xD1DB89F5));

#[repr(C)]
struct UnknownVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const TUID, *mut *mut c_void) -> tresult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct MainFactoryVtbl {
    unknown: UnknownVtbl,
    get_factory: unsafe extern "system" fn(*mut c_void) -> *const sys::Factory,
}

#[repr(C)]
struct EntryPointVtbl {
    unknown: UnknownVtbl,
    get_factory: unsafe extern "system" fn(*mut c_void) -> *const sys::Factory,
    bind_to_document_controller:
        unsafe extern "system" fn(*mut c_void, Ref) -> *const sys::PlugInExtensionInstance,
}

#[repr(C)]
struct EntryPoint2Vtbl {
    unknown: UnknownVtbl,
    bind_to_document_controller_with_roles:
        unsafe extern "system" fn(
            *mut c_void,
            Ref,
            RoleFlags,
            RoleFlags,
        ) -> *const sys::PlugInExtensionInstance,
}

/// # Safety
/// `obj` is a live COM object whose first field is a pointer to a `V`.
unsafe fn vtbl<'a, V>(obj: NonNull<c_void>) -> &'a V {
    // SAFETY: per the contract.
    unsafe { &**(obj.as_ptr() as *const *const V) }
}

/// # Safety
/// `obj` is a live COM object with a reference owned by the caller.
unsafe fn release(obj: NonNull<c_void>) {
    // SAFETY: per the contract.
    unsafe { (vtbl::<UnknownVtbl>(obj).release)(obj.as_ptr()) };
}

/// The main factory class matching `processor` (same name), or the only one.
pub fn main_factory_class<'a>(classes: &'a [ClassInfo], processor: &str) -> Option<&'a ClassInfo> {
    let mains: Vec<&ClassInfo> = classes
        .iter()
        .filter(|c| c.category == MAIN_FACTORY_CLASS)
        .collect();
    mains
        .iter()
        .find(|c| c.name == processor)
        .or_else(|| (mains.len() == 1).then(|| &mains[0]))
        .copied()
}

/// The ARA factory of `module` for its processor class `processor`, and an address in the
/// module's code (registered with [`contain::add_module`]). The main factory object is kept on
/// purpose (its reference is never released): the factory struct belongs to it, and the
/// module stays loaded to the end of the process anyway.
pub fn factory(module: &Module, processor: &str) -> Result<(*const sys::Factory, usize), Error> {
    let classes = module.classes();
    let class = main_factory_class(&classes, processor)
        .ok_or_else(|| error(format!("{processor} has no ARA main factory class")))?;
    let obj = module
        .create_raw(&class.cid, &IID_MAIN_FACTORY)
        .ok_or_else(|| error("the ARA main factory could not be made"))?;
    // SAFETY: `obj` is an `IMainFactory` we own a reference to.
    let f = contain::call(|| unsafe { (vtbl::<MainFactoryVtbl>(obj).get_factory)(obj.as_ptr()) });
    if f.is_null() {
        // SAFETY: our reference.
        unsafe { release(obj) };
        return Err(error("the ARA main factory gave no factory"));
    }
    // The plugin's own threads run in its module; a fault there is contained too.
    // SAFETY: as above.
    let code = unsafe { vtbl::<MainFactoryVtbl>(obj).get_factory } as usize;
    contain::add_module(code);
    Ok((f, code))
}

/// Binds the instance `plugin` to the document controller `dc` with roles. Tries
/// `IPlugInEntryPoint2`, then the first version (which gives every role).
pub fn bind(
    plugin: &Plugin,
    dc: Ref,
    known: RoleFlags,
    assigned: RoleFlags,
) -> Result<sys::PlugInExtensionInstance, Error> {
    let ext = if let Some(e2) = plugin.query_component(&IID_ENTRY_POINT_2) {
        // SAFETY: an `IPlugInEntryPoint2` we own a reference to; released after the call (the
        // component keeps the binding).
        unsafe {
            let r = contain::call(|| {
                (vtbl::<EntryPoint2Vtbl>(e2).bind_to_document_controller_with_roles)(
                    e2.as_ptr(),
                    dc,
                    known,
                    assigned,
                )
            });
            release(e2);
            r
        }
    } else if let Some(e1) = plugin.query_component(&IID_ENTRY_POINT) {
        // SAFETY: as above, for an `IPlugInEntryPoint`.
        unsafe {
            let r = contain::call(|| {
                (vtbl::<EntryPointVtbl>(e1).bind_to_document_controller)(e1.as_ptr(), dc)
            });
            release(e1);
            r
        }
    } else {
        return Err(error(format!(
            "{} does not answer ARA's entry point",
            plugin.class().name
        )));
    };
    if ext.is_null() {
        return Err(error("the plugin did not bind to the document"));
    }
    // SAFETY: the plugin keeps the instance valid while the component lives; copied by value.
    Ok(unsafe { *ext })
}

/// A VST3 module with an ARA factory.
pub struct Vst3Binary {
    module: Arc<Module>,
    class: ClassInfo,
    factory: *const sys::Factory,
    code: usize,
}

impl Vst3Binary {
    /// Loads the bundle at `path` and finds its processor class named `class_name` (or the
    /// first audio processor) and its ARA factory.
    pub fn load(path: &std::path::Path, class_name: Option<&str>) -> Result<Vst3Binary, Error> {
        let module = Arc::new(Module::load(path).map_err(|e| error(e.to_string()))?);
        let classes = module.classes();
        let class = classes
            .iter()
            .filter(|c| c.is_audio_processor())
            .find(|c| class_name.is_none_or(|n| c.name == n))
            .cloned()
            .ok_or_else(|| error(format!("{} has no audio processor class", path.display())))?;
        let (factory, code) = factory(&module, &class.name)?;
        Ok(Vst3Binary {
            module,
            class,
            factory,
            code,
        })
    }
}

impl Binary for Vst3Binary {
    fn factory(&self) -> *const sys::Factory {
        self.factory
    }

    fn module(&self) -> usize {
        self.code
    }

    fn instance(
        &mut self,
        dc: Ref,
        known: RoleFlags,
        assigned: RoleFlags,
    ) -> Result<Box<dyn Instance>, Error> {
        let plugin = contain::call(|| Plugin::new(self.module.clone(), &self.class))
            .map_err(|e| error(e.to_string()))?;
        let ext = bind(&plugin, dc, known, assigned)?;
        Ok(Box::new(Vst3Instance {
            audio: None,
            plugin,
            ext,
        }))
    }
}

/// One processor instance bound to the document.
pub struct Vst3Instance {
    // Dropped before the plugin: the audio part holds the processor.
    audio: Option<AudioPart>,
    plugin: Plugin,
    ext: sys::PlugInExtensionInstance,
}

impl Instance for Vst3Instance {
    fn extension(&self) -> sys::PlugInExtensionInstance {
        self.ext
    }

    fn prepare(&mut self, rate: f64, max_block: usize) -> Result<u32, Error> {
        if self.audio.is_none() {
            self.audio = Some(self.plugin.audio().map_err(|e| error(e.to_string()))?);
        }
        let audio = self.audio.as_mut().expect("set above");
        contain::call(|| audio.prepare(rate, max_block, true)).map_err(|e| error(e.to_string()))
    }

    fn set_transport(&mut self, playing: bool, position: i64) {
        if let Some(a) = &mut self.audio {
            a.set_transport(playing, position);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if let Some(a) = &mut self.audio {
            contain::call(|| a.process(left, right, &[]));
        }
    }

    fn open_editor(&mut self, title: &str, place: Placement) -> Result<Box<dyn Window>, Error> {
        let editor = self
            .plugin
            .open_editor_at(title, place)
            .map_err(|e| error(e.to_string()))?;
        Ok(Box::new(editor))
    }
}

impl Window for Editor {
    fn is_visible(&self) -> bool {
        Editor::is_visible(self)
    }

    fn raise(&self) {
        Editor::raise(self);
    }

    fn set_caption(&self, caption: Caption) {
        Editor::set_caption(self, caption);
    }
}
