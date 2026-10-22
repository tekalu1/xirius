//! Loading a VST3 module (the DLL inside a `.vst3` bundle) and listing its classes.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use vst3::Steinberg::{
    FIDString, FUnknown, IPluginFactory, IPluginFactory2, IPluginFactory2Trait, IPluginFactory3,
    IPluginFactory3Trait, IPluginFactoryTrait, PClassInfo, PClassInfo2, PFactoryInfo, kResultOk,
};
use vst3::{ComPtr, Interface};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::serial::{SETUP_LIMIT, turn, turn_within};
use crate::{Error, c_str, error};

type InitDll = unsafe extern "system" fn() -> bool;
type GetPluginFactory = unsafe extern "system" fn() -> *mut IPluginFactory;

/// The category of classes that process audio (effects and instruments).
pub const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

#[derive(Clone, Debug)]
pub struct ClassInfo {
    pub cid: [u8; 16],
    pub name: String,
    pub category: String,
    /// `|`-separated, e.g. `Fx|Reverb` or `Instrument|Synth`. Empty without `IPluginFactory2`.
    pub sub_categories: String,
    pub vendor: String,
    pub version: String,
}

impl ClassInfo {
    pub fn is_audio_processor(&self) -> bool {
        self.category == AUDIO_MODULE_CLASS
    }

    pub fn is_instrument(&self) -> bool {
        self.sub_categories.split('|').any(|s| s == "Instrument")
    }
}

/// A handle to a VST3 module, kept initialized until this process exits (ARC-043).
///
/// Loading a bundle that is already loaded in this process shares the loaded DLL, so
/// `InitDll` runs once. Neither `ExitDll` nor `FreeLibrary` is called: plugins may leave
/// timers, threads or window procedures after their last instance is released. Deinitializing
/// their globals or unmapping their code then would turn a later callback into a crash.
/// Windows reclaims the mappings at process exit. Factory calls and instance setup/teardown
/// still take turns across the process (see `serial`), so any thread may load.
pub struct Module {
    path: PathBuf,
    library: Arc<Library>,
    pub vendor: String,
}

/// The DLL behind every [`Module`] of one binary. A successful open stays in `LOADED`.
struct Library {
    binary: PathBuf,
    _handle: HMODULE,
    factory: Option<ComPtr<IPluginFactory>>,
    vendor: String,
}

// SAFETY: the module handle and the factory are process-wide, and every call into them is
// made during a turn.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

/// Process-lifetime owners, plus the number of current Module handles (which can be zero).
struct Resident {
    key: PathBuf,
    library: Result<Arc<Library>, String>,
    users: usize,
}
static LOADED: Mutex<Vec<Resident>> = Mutex::new(Vec::new());

/// `C:\Program Files\Common Files\VST3`, the standard 64-bit location on Windows.
pub fn default_vst3_dir() -> PathBuf {
    let base = std::env::var_os("CommonProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
    base.join("VST3")
}

/// Every `.vst3` bundle (directory or single file) under `dir`, sorted. Bundles are not
/// searched inside.
pub fn find_bundles(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let is_bundle = p
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("vst3"));
            if is_bundle {
                out.push(p);
            } else if p.is_dir() {
                stack.push(p);
            }
        }
    }
    out.sort();
    out
}

/// The DLL to load for a bundle: the path itself when it is a file, otherwise
/// `Contents/x86_64-win/<name>.vst3`.
fn binary_path(bundle: &Path) -> Result<PathBuf, Error> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let arch = bundle.join("Contents").join("x86_64-win");
    if let Some(name) = bundle.file_name() {
        let p = arch.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    std::fs::read_dir(&arch)
        .ok()
        .and_then(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .find(|p| p.is_file() && p.extension().is_some_and(|x| x == "vst3"))
        })
        .ok_or_else(|| error(format!("no x86_64-win binary in {}", bundle.display())))
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain([0]).collect()
}

impl Library {
    /// Loads the DLL, calls `InitDll` if present and takes the plugin factory.
    fn open(binary: PathBuf) -> Result<Library, Error> {
        let w = wide(&binary);
        // SAFETY: `w` is a NUL-terminated wide path. Loading runs the DLL's initialisers,
        // which is what hosting a plugin means.
        let handle = unsafe { LoadLibraryW(w.as_ptr()) };
        if handle.is_null() {
            return Err(error(format!(
                "LoadLibrary failed for {} ({})",
                binary.display(),
                std::io::Error::last_os_error()
            )));
        }
        let mut library = Library {
            binary,
            _handle: handle,
            factory: None,
            vendor: String::new(),
        };
        // SAFETY: the symbols are looked up by their documented names and called with the
        // documented signatures (`bool PLUGIN_API InitDll()` etc.).
        unsafe {
            if let Some(init) = GetProcAddress(handle, c"InitDll".as_ptr().cast()) {
                let init: InitDll = std::mem::transmute(init);
                if !init() {
                    return Err(error("InitDll returned false"));
                }
            }
            let get = GetProcAddress(handle, c"GetPluginFactory".as_ptr().cast())
                .ok_or_else(|| error("the module has no GetPluginFactory"))?;
            let get: GetPluginFactory = std::mem::transmute(get);
            // GetPluginFactory returns a reference the caller owns.
            library.factory = ComPtr::from_raw(get());
        }
        let factory = library
            .factory
            .as_ref()
            .ok_or_else(|| error("GetPluginFactory returned null"))?;
        // SAFETY: `PFactoryInfo` is plain data; the factory fills it.
        let mut info: PFactoryInfo = unsafe { std::mem::zeroed() };
        if unsafe { factory.getFactoryInfo(&mut info) } == kResultOk {
            library.vendor = c_str(&info.vendor);
        }
        Ok(library)
    }
}

// No Library destructor releases the LoadLibrary reference, including on a failed open:
// even a partial initialization can have registered callbacks in the DLL.

impl Module {
    /// Loads the bundle's DLL, calls `InitDll` if present and takes the plugin factory. When
    /// the DLL is already loaded, uses it again.
    pub fn load(bundle: &Path) -> Result<Module, Error> {
        let binary = binary_path(bundle)?;
        let key = std::fs::canonicalize(&binary).unwrap_or_else(|_| binary.clone());
        let _turn = turn_within("load", bundle, SETUP_LIMIT)?;
        let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
        let library = match loaded.iter_mut().find(|l| l.key == key) {
            Some(resident) => {
                let library = resident.library.as_ref().map_err(|e| error(e.clone()))?;
                resident.users += 1;
                library.clone()
            }
            None => {
                let result = Library::open(binary).map(Arc::new).map_err(|e| e.0);
                loaded.push(Resident {
                    key,
                    users: usize::from(result.is_ok()),
                    library: result.clone(),
                });
                result.map_err(error)?
            }
        };
        Ok(Module {
            path: bundle.to_path_buf(),
            vendor: library.vendor.clone(),
            library,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn binary(&self) -> &Path {
        &self.library.binary
    }

    /// How many [`Module`] handles use this DLL now. Zero does not mean it was unmapped.
    pub fn users(bundle: &Path) -> usize {
        let Ok(binary) = binary_path(bundle) else {
            return 0;
        };
        let key = std::fs::canonicalize(&binary).unwrap_or(binary);
        LOADED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|l| l.key == key)
            .map_or(0, |l| l.users)
    }

    fn factory(&self) -> &ComPtr<IPluginFactory> {
        self.library
            .factory
            .as_ref()
            .expect("the factory is set in open")
    }

    /// All classes the factory exports, with sub-categories when it implements
    /// `IPluginFactory2`.
    pub fn classes(&self) -> Vec<ClassInfo> {
        let _turn = turn("list the classes of", &self.path);
        let f = self.factory();
        let f2 = f.cast::<IPluginFactory2>();
        // SAFETY: plain factory queries with out-structs owned here.
        let count = unsafe { f.countClasses() };
        let mut out = Vec::new();
        for i in 0..count {
            if let Some(f2) = &f2 {
                let mut c: PClassInfo2 = unsafe { std::mem::zeroed() };
                if unsafe { f2.getClassInfo2(i, &mut c) } == kResultOk {
                    out.push(ClassInfo {
                        cid: c.cid.map(|b| b as u8),
                        name: c_str(&c.name),
                        category: c_str(&c.category),
                        sub_categories: c_str(&c.subCategories),
                        vendor: c_str(&c.vendor),
                        version: c_str(&c.version),
                    });
                    continue;
                }
            }
            let mut c: PClassInfo = unsafe { std::mem::zeroed() };
            if unsafe { f.getClassInfo(i, &mut c) } == kResultOk {
                out.push(ClassInfo {
                    cid: c.cid.map(|b| b as u8),
                    name: c_str(&c.name),
                    category: c_str(&c.category),
                    sub_categories: String::new(),
                    vendor: self.vendor.clone(),
                    version: String::new(),
                });
            }
        }
        out
    }

    /// Hands the host context to factories that ask for it (`IPluginFactory3`). Call it
    /// during a turn.
    pub(crate) fn set_host_context(&self, context: *mut FUnknown) {
        if let Some(f3) = self.factory().cast::<IPluginFactory3>() {
            // SAFETY: `context` is a live host object; the factory adds its own reference.
            unsafe { f3.setHostContext(context) };
        }
    }

    /// Creates an instance of class `cid` and asks it for interface `I`. Call it during a
    /// turn.
    pub(crate) fn create<I: Interface>(&self, cid: &[u8; 16]) -> Option<ComPtr<I>> {
        let mut obj: *mut c_void = std::ptr::null_mut();
        // SAFETY: both ids are 16-byte arrays; on success `obj` holds a reference we own.
        let r = unsafe {
            self.factory().createInstance(
                cid.as_ptr() as FIDString,
                I::IID.as_ptr() as FIDString,
                &mut obj,
            )
        };
        if r != kResultOk {
            return None;
        }
        // SAFETY: the factory returned an `I` with one reference for us.
        unsafe { ComPtr::from_raw(obj as *mut I) }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        let _turn = turn("release a module handle for", &self.path);
        let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
        let i = loaded
            .iter()
            .position(|l| {
                l.library
                    .as_ref()
                    .is_ok_and(|l| Arc::ptr_eq(l, &self.library))
            })
            .expect("a loaded module is listed");
        loaded[i].users -= 1;
    }
}
