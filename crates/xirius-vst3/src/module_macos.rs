//! CoreFoundation resolves CFBundleExecutable, including names unlike the bundle name.
//! Loaded bundles stay resident, just as Windows modules do (ARC-043).
use super::{Error, GetPluginFactory, IPluginFactory, Path, PathBuf, c_void, error};
use std::ffi::CStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::sync::{Mutex, Once};
use vst3::ComPtr;

type Ref = *const c_void;
type Entry = unsafe extern "C" fn(Ref) -> bool;
type Exit = unsafe extern "C" fn() -> bool;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        a: Ref,
        bytes: *const u8,
        len: isize,
        dir: u8,
    ) -> Ref;
    fn CFBundleCreate(a: Ref, url: Ref) -> Ref;
    fn CFBundleCopyExecutableURL(bundle: Ref) -> Ref;
    fn CFURLGetFileSystemRepresentation(url: Ref, resolve: u8, bytes: *mut u8, len: isize) -> u8;
    fn CFBundleLoadExecutable(bundle: Ref) -> u8;
    fn CFBundleGetFunctionPointerForName(bundle: Ref, name: Ref) -> *mut c_void;
    fn CFStringCreateWithCString(a: Ref, bytes: *const std::ffi::c_char, encoding: u32) -> Ref;
    fn CFRelease(value: Ref);
    fn atexit(callback: extern "C" fn()) -> std::ffi::c_int;
}

struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: each Owned holds one Create/Copy reference.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn bundle(path: &Path) -> Result<Owned, Error> {
    let path = std::fs::canonicalize(path).map_err(|e| error(e.to_string()))?;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: valid path bytes and their length; CoreFoundation copies them.
    let url = Owned(unsafe {
        CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            1,
        )
    });
    if url.0.is_null() {
        return Err(error("cannot create VST3 bundle URL"));
    }
    let bundle = Owned(unsafe { CFBundleCreate(std::ptr::null(), url.0) });
    if bundle.0.is_null() {
        return Err(error("invalid macOS VST3 bundle"));
    }
    Ok(bundle)
}

pub(super) fn binary_path(path: &Path) -> Result<PathBuf, Error> {
    let bundle = bundle(path)?;
    let url = Owned(unsafe { CFBundleCopyExecutableURL(bundle.0) });
    if url.0.is_null() {
        return Err(error("VST3 bundle has no executable"));
    }
    let mut bytes = vec![0u8; 4096];
    // SAFETY: live URL and writable buffer of the given size.
    if unsafe {
        CFURLGetFileSystemRepresentation(url.0, 1, bytes.as_mut_ptr(), bytes.len() as isize)
    } == 0
    {
        return Err(error("cannot resolve VST3 bundle executable path"));
    }
    bytes.truncate(bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len()));
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if !path.is_file() {
        return Err(error("VST3 bundle executable does not exist"));
    }
    Ok(path)
}

fn symbol(bundle: Ref, name: &CStr) -> Result<*mut c_void, Error> {
    // SAFETY: static NUL-terminated UTF-8 symbol name (kCFStringEncodingUTF8).
    let name_ref =
        Owned(unsafe { CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), 0x08000100) });
    if name_ref.0.is_null() {
        return Err(error("cannot create symbol name"));
    }
    let address = unsafe { CFBundleGetFunctionPointerForName(bundle, name_ref.0) };
    if address.is_null() {
        return Err(error(format!(
            "VST3 bundle has no {}",
            name.to_string_lossy()
        )));
    }
    Ok(address)
}

static EXITS: Mutex<Vec<Exit>> = Mutex::new(Vec::new());
static REGISTER_EXIT: Once = Once::new();
extern "C" fn exit_bundles() {
    let exits = EXITS.lock().unwrap_or_else(|e| e.into_inner());
    for exit in exits.iter().rev() {
        // SAFETY: successfully initialized modules remain mapped until process exit.
        unsafe {
            exit();
        }
    }
}

pub(super) fn open(binary: &Path) -> Result<(*mut c_void, Option<ComPtr<IPluginFactory>>), Error> {
    let root = binary
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| error("expected Contents/MacOS executable in VST3 bundle"))?;
    let bundle = bundle(root)?;
    if unsafe { CFBundleLoadExecutable(bundle.0) } == 0 {
        return Err(error(format!("cannot load VST3 bundle {}", root.display())));
    }
    // Keep even failed initializations resident: they may already have registered callbacks.
    let handle = bundle.0;
    std::mem::forget(bundle);
    // SAFETY: VST3's documented macOS entry points and signatures.
    let entry: Entry = unsafe { std::mem::transmute(symbol(handle, c"bundleEntry")?) };
    let exit: Exit = unsafe { std::mem::transmute(symbol(handle, c"bundleExit")?) };
    let get: GetPluginFactory =
        unsafe { std::mem::transmute(symbol(handle, c"GetPluginFactory")?) };
    if !unsafe { entry(handle) } {
        return Err(error("bundleEntry returned false"));
    }
    REGISTER_EXIT.call_once(|| unsafe {
        atexit(exit_bundles);
    });
    EXITS.lock().unwrap_or_else(|e| e.into_inner()).push(exit);
    Ok((handle.cast_mut(), unsafe { ComPtr::from_raw(get()) }))
}
