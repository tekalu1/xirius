//! POSIX shared memory with a bounded, polling auto-reset notification.
//! Audio-thread notification only stores an atomic; native waits are a stage 3 optimization.
use std::ffi::CString;
use std::io;
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

/// Cross-process auto-reset event. Repeated signals coalesce just as Win32 events do.
pub struct Event(Mapping);
impl Event {
    pub fn create(text: &str) -> io::Result<Self> {
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
            std::thread::sleep(Duration::from_millis(1));
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
