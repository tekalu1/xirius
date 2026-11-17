//! Page-backed POSIX shared memory for immutable ARA audio.

use std::ffi::{CString, c_char, c_int, c_void};
use std::io;

const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0x0200;
const O_EXCL: c_int = 0x0800;
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 1;
const SEEK_END: c_int = 2;

unsafe extern "C" {
    fn shm_open(name: *const c_char, flags: c_int, mode: u32) -> c_int;
    fn shm_unlink(name: *const c_char) -> c_int;
    fn ftruncate(fd: c_int, size: i64) -> c_int;
    fn lseek(fd: c_int, offset: i64, whence: c_int) -> i64;
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn close(fd: c_int) -> c_int;
}

pub(super) struct Mapping {
    view: *mut c_void,
    len: usize,
    owner: Option<CString>,
}

unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

fn name(text: &str) -> CString {
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    CString::new(format!("/xirius-ara-{hash:016x}")).unwrap()
}

impl Mapping {
    pub(super) fn create(text: &str, len: usize) -> io::Result<Self> {
        let name = name(text);
        // SAFETY: terminated private name and exclusive creation.
        let fd = unsafe { shm_open(name.as_ptr(), O_CREAT | O_EXCL | O_RDWR, 0o600) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return Err(io::Error::new(
                error.kind(),
                format!("ARA audio shm_open(create): {error}"),
            ));
        }
        if unsafe { ftruncate(fd, len as i64) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                close(fd);
                shm_unlink(name.as_ptr());
            }
            return Err(io::Error::new(
                error.kind(),
                format!("ARA audio ftruncate: {error}"),
            ));
        }
        Self::map(fd, len, PROT_READ | PROT_WRITE, Some(name))
    }

    pub(super) fn open(text: &str) -> io::Result<Self> {
        let name = name(text);
        // SAFETY: terminated name; the returned descriptor belongs here.
        let fd = unsafe { shm_open(name.as_ptr(), O_RDONLY, 0) };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return Err(io::Error::new(
                error.kind(),
                format!("ARA audio shm_open(open): {error}"),
            ));
        }
        let len = unsafe { lseek(fd, 0, SEEK_END) };
        if len < super::HEADER as i64 || len > isize::MAX as i64 {
            unsafe {
                close(fd);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "short ARA audio mapping",
            ));
        }
        Self::map(fd, len as usize, PROT_READ, None)
    }

    fn map(fd: c_int, len: usize, prot: c_int, owner: Option<CString>) -> io::Result<Self> {
        // SAFETY: the descriptor describes at least len bytes and this call owns the view.
        let view = unsafe { mmap(std::ptr::null_mut(), len, prot, MAP_SHARED, fd, 0) };
        let error = io::Error::last_os_error();
        unsafe {
            close(fd);
        }
        if view as isize == -1 {
            if let Some(name) = owner {
                unsafe {
                    shm_unlink(name.as_ptr());
                }
            }
            return Err(io::Error::new(
                error.kind(),
                format!("ARA audio mmap: {error}"),
            ));
        }
        Ok(Self { view, len, owner })
    }

    pub(super) fn pointer(&self) -> *mut u8 {
        self.view.cast()
    }
    pub(super) fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this object owns the view and, if present, the name.
        unsafe {
            munmap(self.view, self.len);
            if let Some(name) = &self.owner {
                shm_unlink(name.as_ptr());
            }
        }
    }
}
