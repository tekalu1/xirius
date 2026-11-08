//! Recording disk reservation (ENG-019). Allocation reserves disk blocks without extending
//! the readable file length, so a crash repair never mistakes reserved zeros for a take.
use std::fs::File;
use std::io;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALLOCATION_INFO, FileAllocationInfo, GetDiskFreeSpaceExW, SetFileInformationByHandle,
};

pub struct Reservation {
    directory: PathBuf,
    chunk: u64,
    allocated: u64,
    low: bool,
    unknown: bool,
    query: fn(&Path) -> io::Result<u64>,
}

impl Reservation {
    pub fn new(path: &Path, bytes_per_second: u64) -> Self {
        Self {
            directory: path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
            chunk: bytes_per_second.saturating_mul(60).max(65_536),
            allocated: 0,
            low: false,
            unknown: false,
            query: disk_space,
        }
    }

    pub fn ensure(&mut self, file: &File, needed: u64) -> io::Result<()> {
        if needed <= self.allocated {
            return Ok(());
        }
        let size = needed
            .div_ceil(self.chunk)
            .checked_mul(self.chunk)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or_else(|| io::Error::other("recording allocation is too large"))?;
        if let Err(error) = reserve(file, size, self.allocated) {
            self.low |= is_disk_full(&error);
            return Err(error);
        }
        self.allocated = size as u64;
        self.check_space();
        Ok(())
    }

    pub fn check_space(&mut self) {
        match (self.query)(&self.directory) {
            Ok(available) => {
                self.low = Self::is_low(available, self.chunk);
                self.unknown = false;
            }
            // A capacity query needs permissions that a writable file need not have.
            // It must not prevent flushing samples and updating the recoverable header.
            Err(_) => self.unknown = true,
        }
    }

    pub fn allocated(&self) -> u64 {
        self.allocated
    }
    pub fn low(&self) -> bool {
        self.low
    }
    pub fn unknown(&self) -> bool {
        self.unknown
    }
    fn is_low(available: u64, chunk: u64) -> bool {
        available < chunk.saturating_mul(2)
    }
}

pub(crate) fn is_disk_full(error: &io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(39 | 112))
    }
    #[cfg(target_os = "macos")]
    {
        matches!(error.raw_os_error(), Some(libc::ENOSPC | libc::EDQUOT))
    }
}

#[cfg(windows)]
fn disk_space(directory: &Path) -> io::Result<u64> {
    // UNC shares require the trailing directory separator too.
    let name: Vec<u16> = directory
        .as_os_str()
        .encode_wide()
        .chain(Some(b'\\' as u16))
        .chain(Some(0))
        .collect();
    let mut available = 0;
    // SAFETY: a terminated directory path and valid output pointer. Optional outputs null.
    if unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(available)
    }
}

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[cfg(windows)]
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_STANDARD_INFO, FileStandardInfo, GetFileInformationByHandleEx,
    };
    #[test]
    fn reservation_does_not_extend_the_recoverable_take() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-reservation-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("take.bin");
        let mut file = File::create(&path).unwrap();
        let mut reservation = Reservation::new(&path, 48_000 * 8);
        reservation.ensure(&file, 92).unwrap();
        assert!(reservation.allocated() >= 48_000 * 8 * 60);
        #[cfg(windows)]
        {
            let mut standard: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
            // SAFETY: valid handle and a writable SDK metadata record.
            assert_ne!(
                unsafe {
                    GetFileInformationByHandleEx(
                        file.as_raw_handle(),
                        FileStandardInfo,
                        (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                        size_of::<FILE_STANDARD_INFO>() as u32,
                    )
                },
                0
            );
            assert!(
                standard.AllocationSize >= 48_000 * 8 * 60,
                "disk blocks are actually reserved"
            );
        }
        assert_eq!(file.metadata().unwrap().len(), 0);
        file.write_all(b"take").unwrap();
        assert_eq!(file.metadata().unwrap().len(), 4);
        drop(file);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 4);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn an_unavailable_capacity_query_does_not_prevent_reserved_writes() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-query-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("take.bin");
        let mut file = File::create(&path).unwrap();
        let mut reservation = Reservation::new(&path, 48_000 * 8);
        reservation.directory = path.join("not-a-directory");
        reservation.ensure(&file, 92).unwrap();
        reservation.check_space();
        assert!(reservation.unknown());
        file.write_all(b"take").unwrap();
        file.sync_data().unwrap();
        drop(file);
        assert_eq!(std::fs::read(&path).unwrap(), b"take");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn low_capacity_is_reported_while_reserved_audio_can_still_be_written() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-low-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("take.bin");
        let mut file = File::create(&path).unwrap();
        let mut reservation = Reservation::new(&path, 1000);
        reservation.query = |_| Ok(32_768);
        reservation.ensure(&file, 92).unwrap();
        assert!(reservation.low());
        assert!(!reservation.unknown());
        assert!(reservation.allocated() >= 65_536);
        file.write_all(b"take").unwrap();
        file.sync_data().unwrap();
        drop(file);
        assert_eq!(std::fs::read(&path).unwrap(), b"take");
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(windows)]
fn reserve(file: &File, size: i64, _previous: u64) -> io::Result<()> {
    let info = FILE_ALLOCATION_INFO {
        AllocationSize: size,
    };
    // SAFETY: owned file and valid metadata structure.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileAllocationInfo,
            (&info as *const FILE_ALLOCATION_INFO).cast(),
            size_of::<FILE_ALLOCATION_INFO>() as u32,
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "macos")]
fn reserve(file: &File, size: i64, previous: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut store = libc::fstore_t {
        fst_flags: libc::F_ALLOCATEALL,
        fst_posmode: libc::F_PEOFPOSMODE,
        fst_offset: 0,
        fst_length: size - previous as i64,
        fst_bytesalloc: 0,
    };
    // SAFETY: owned fd and writable fstore_t. F_PREALLOCATE does not change logical EOF.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_PREALLOCATE, &mut store) } == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(target_os = "macos")]
fn disk_space(directory: &Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(directory.as_os_str().as_bytes())?;
    let mut info = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: NUL-terminated path and writable output structure.
    if unsafe { libc::statvfs(name.as_ptr(), info.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok(info.f_bavail.saturating_mul(info.f_frsize))
}
