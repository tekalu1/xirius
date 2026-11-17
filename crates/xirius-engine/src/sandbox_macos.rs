//! POSIX shared memory with a bounded cross-process auto-reset notification.
use std::ffi::CString;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(super) struct Mapping {
    view: *mut libc::c_void,
    size: usize,
    owner: Option<CString>,
}
// SAFETY: mapping lifetime is owned here; the protocol synchronizes all accesses.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

fn name(text: &str) -> CString {
    // Stable across processes, under macOS's short POSIX shared-memory name limit.
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    CString::new(format!("/xirius-{hash:016x}")).unwrap()
}
impl Mapping {
    pub(super) fn create(text: &str, size: usize) -> io::Result<Self> {
        let name = name(text);
        // SAFETY: NUL-terminated name, private permissions, exclusive creation.
        let fd = unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(name.as_ptr());
            }
            return Err(error);
        }
        Self::map(fd, size, Some(name))
    }
    pub(super) fn open(text: &str) -> io::Result<Self> {
        let name = name(text);
        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::close(fd);
            }
            return Err(error);
        }
        let size = unsafe { stat.assume_init() }.st_size;
        if size <= 0 || size as u64 > isize::MAX as u64 {
            unsafe {
                libc::close(fd);
            }
            return Err(io::Error::other("invalid shared memory size"));
        }
        Self::map(fd, size as usize, None)
    }
    fn map(fd: libc::c_int, size: usize, owner: Option<CString>) -> io::Result<Self> {
        // SAFETY: fd describes at least size bytes; mmap owns a separate lifetime.
        let view = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        let error = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        if view == libc::MAP_FAILED {
            if let Some(name) = owner {
                unsafe {
                    libc::shm_unlink(name.as_ptr());
                }
            }
            return Err(error);
        }
        Ok(Self { view, size, owner })
    }
    pub(super) fn pointer(&self) -> *mut u8 {
        self.view.cast()
    }
    pub(super) fn len(&self) -> usize {
        self.size
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this object owns this mapping and, if created here, its name.
        unsafe {
            libc::munmap(self.view, self.size);
            if let Some(name) = &self.owner {
                libc::shm_unlink(name.as_ptr());
            }
        }
    }
}

type WaitAddress = unsafe extern "C" fn(*mut libc::c_void, u64, usize, u32, u32, u64) -> i32;
type WakeAddress = unsafe extern "C" fn(*mut libc::c_void, usize, u32) -> i32;

struct AddressSync {
    wait: WaitAddress,
    wake: WakeAddress,
}

fn address_sync() -> Option<&'static AddressSync> {
    static SYNC: OnceLock<Option<AddressSync>> = OnceLock::new();
    SYNC.get_or_init(|| {
        // Resolve once during event creation/opening, outside the audio callback. These
        // functions are available from the system on macOS 14.4 and newer.
        let wait = unsafe {
            libc::dlsym(
                libc::RTLD_DEFAULT,
                c"os_sync_wait_on_address_with_timeout".as_ptr(),
            )
        };
        let wake =
            unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"os_sync_wake_by_address_any".as_ptr()) };
        if wait.is_null() || wake.is_null() {
            return None;
        }
        // SAFETY: both symbols have the signatures declared by the system headers.
        Some(AddressSync {
            wait: unsafe { std::mem::transmute::<*mut libc::c_void, WaitAddress>(wait) },
            wake: unsafe { std::mem::transmute::<*mut libc::c_void, WakeAddress>(wake) },
        })
    })
    .as_ref()
}

/// Cross-process auto-reset event. Repeated signals coalesce.
pub struct Event(Mapping);
impl Event {
    pub fn create(text: &str) -> io::Result<Self> {
        let _ = address_sync();
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let text = if text.is_empty() {
            format!(
                "event-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )
        } else {
            format!("event-{text}")
        };
        Mapping::create(&text, std::mem::size_of::<AtomicU32>()).map(Self)
    }
    pub fn open(text: &str) -> io::Result<Self> {
        let _ = address_sync();
        let mapping = Mapping::open(&format!("event-{text}"))?;
        // Darwin reports the page-rounded size through fstat.
        if mapping.len() < std::mem::size_of::<AtomicU32>() {
            return Err(io::Error::other("invalid event mapping"));
        }
        Ok(Self(mapping))
    }
    fn flag(&self) -> &AtomicU32 {
        // SAFETY: page-aligned zero-initialized memory, exactly one shared atomic.
        unsafe { &*self.0.pointer().cast::<AtomicU32>() }
    }
    pub fn set(&self) {
        self.flag().store(1, Ordering::Release);
        if let Some(sync) = address_sync() {
            // SAFETY: the address is aligned shared memory and remains mapped for this call.
            unsafe { (sync.wake)(self.0.pointer().cast(), 4, 1) };
        }
    }
    pub fn wait(&self, ms: u32) -> bool {
        let deadline = Instant::now() + Duration::from_millis(u64::from(ms));
        loop {
            if self.flag().swap(0, Ordering::AcqRel) != 0 {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if let Some(sync) = address_sync() {
                // A compare-and-wait closes the gap between checking and sleeping.
                // The short bound lets the caller detect a crashed host promptly.
                let remaining = deadline.saturating_duration_since(Instant::now());
                let ns = remaining.as_nanos().clamp(1, 1_000_000) as u64;
                // SAFETY: the shared atomic is aligned and mapped for this call.
                unsafe { (sync.wait)(self.0.pointer().cast(), 0, 4, 1, 32, ns) };
            } else {
                // Older systems lack address waits; retain bounded polling.
                std::thread::sleep(Duration::from_micros(100));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_signals_coalesce_and_the_owner_removes_the_name() {
        let name = format!("event-test-{}", std::process::id());
        let owner = Event::create(&name).unwrap();
        let reader = Event::open(&name).unwrap();
        assert!(!reader.wait(0));
        owner.set();
        owner.set();
        assert!(reader.wait(0));
        assert!(!reader.wait(0));
        drop(owner);
        assert!(Event::open(&name).is_err());
        // Unlinking the name does not invalidate an already-open mapping.
        reader.set();
        assert!(reader.wait(0));
    }
}
