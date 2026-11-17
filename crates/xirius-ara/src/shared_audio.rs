//! Immutable stereo audio passed between the song and its ARA host (PLG-078).
//!
//! A mapping has one creator and any number of readers. The creator publishes the complete
//! contents before sending its name over the control channel. Dropping the creator removes the
//! name; an already-open reader may finish using its view.

use std::io;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP_ALL_ACCESS, FILE_MAP_READ, MEMORY_BASIC_INFORMATION,
    MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
    VirtualQuery,
};

const MAGIC: u32 = u32::from_le_bytes(*b"XARA");
const HEADER: usize = 24;

/// One planar stereo buffer in a named, page-backed mapping.
pub struct SharedAudio {
    #[cfg(windows)]
    handle: HANDLE,
    #[cfg(target_os = "macos")]
    handle: macos::Mapping,
    view: *mut u8,
    frames: usize,
    rate: f64,
}

// The buffer is immutable after creation. Readers have no write access.
unsafe impl Send for SharedAudio {}
unsafe impl Sync for SharedAudio {}

impl SharedAudio {
    /// Creates one buffer. `name` must be unique for its lifetime.
    pub fn create(name: &str, rate: f64, left: &[f32], right: &[f32]) -> io::Result<Self> {
        if left.len() != right.len() || !rate.is_finite() || rate <= 0.0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid audio"));
        }
        let size = left
            .len()
            .checked_mul(2 * size_of::<f32>())
            .and_then(|n| n.checked_add(HEADER))
            .filter(|n| *n <= isize::MAX as usize)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "audio is too large"))?;
        #[cfg(windows)]
        let audio = {
            let wide = wide(name);
            // SAFETY: pagefile-backed mapping with a valid size and terminated name.
            let handle = unsafe {
                CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    std::ptr::null(),
                    PAGE_READWRITE,
                    (size as u64 >> 32) as u32,
                    size as u32,
                    wide.as_ptr(),
                )
            };
            // CreateFileMappingW also opens an existing name. Never overwrite audio that a
            // previous document still owns.
            if !handle.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe { CloseHandle(handle) };
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "ARA audio mapping already exists",
                ));
            }
            Self::map_windows(handle, FILE_MAP_ALL_ACCESS, rate, left.len())?
        };
        #[cfg(target_os = "macos")]
        let audio = {
            let handle = macos::Mapping::create(name, size)?;
            SharedAudio {
                view: handle.pointer(),
                handle,
                frames: left.len(),
                rate,
            }
        };
        // SAFETY: the creator has exclusive access until it sends the name, and the mapped
        // range is at least HEADER + two channels of `frames` samples.
        unsafe {
            audio.view.cast::<u32>().write(MAGIC);
            audio.view.add(8).cast::<u64>().write(left.len() as u64);
            audio.view.add(16).cast::<f64>().write(rate);
            std::ptr::copy_nonoverlapping(left.as_ptr(), audio.channel_mut(0), left.len());
            std::ptr::copy_nonoverlapping(right.as_ptr(), audio.channel_mut(1), right.len());
        }
        Ok(audio)
    }

    /// Opens a published buffer, read-only. Its name is no longer usable after the creator
    /// is dropped, even while this view remains valid.
    pub fn open(name: &str) -> io::Result<Self> {
        #[cfg(windows)]
        let mut audio = {
            // SAFETY: terminated name; OpenFileMappingW returns a reader-owned handle.
            let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wide(name).as_ptr()) };
            Self::map_windows(handle, FILE_MAP_READ, 0.0, 0)?
        };
        #[cfg(target_os = "macos")]
        let mut audio = {
            let handle = macos::Mapping::open(name)?;
            SharedAudio {
                view: handle.pointer(),
                handle,
                frames: 0,
                rate: 0.0,
            }
        };
        // SAFETY: mapping is at least HEADER bytes; map_windows and macos::open check it.
        let (magic, frames, rate) = unsafe {
            (
                audio.view.cast::<u32>().read(),
                audio.view.add(8).cast::<u64>().read(),
                audio.view.add(16).cast::<f64>().read(),
            )
        };
        let size = usize::try_from(frames)
            .ok()
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(HEADER));
        if magic != MAGIC
            || !rate.is_finite()
            || rate <= 0.0
            || size.is_none_or(|n| n > audio.mapped_len())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid ARA audio mapping",
            ));
        }
        audio.frames = frames as usize;
        audio.rate = rate;
        Ok(audio)
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    pub fn channel(&self, channel: usize) -> &[f32] {
        assert!(channel < 2);
        // SAFETY: the header and mapping length were checked; the creator never writes again.
        unsafe { std::slice::from_raw_parts(self.channel_mut(channel), self.frames) }
    }

    fn channel_mut(&self, channel: usize) -> *mut f32 {
        // SAFETY: creation and open both guarantee the complete range is mapped.
        unsafe { self.view.add(HEADER + channel * self.frames * 4).cast() }
    }

    #[cfg(windows)]
    fn map_windows(handle: HANDLE, access: u32, rate: f64, frames: usize) -> io::Result<Self> {
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: valid mapping handle; map all bytes.
        let view = unsafe { MapViewOfFile(handle, access, 0, 0, 0) };
        if view.Value.is_null() {
            let error = io::Error::last_os_error();
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self {
            handle,
            view: view.Value.cast(),
            frames,
            rate,
        })
    }

    #[cfg(windows)]
    fn mapped_len(&self) -> usize {
        // SAFETY: the pointer belongs to an active mapped view.
        let mut info = std::mem::MaybeUninit::<MEMORY_BASIC_INFORMATION>::uninit();
        let written = unsafe {
            VirtualQuery(
                self.view.cast(),
                info.as_mut_ptr(),
                size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if written == 0 {
            return 0;
        }
        unsafe { info.assume_init() }.RegionSize
    }

    #[cfg(target_os = "macos")]
    fn mapped_len(&self) -> usize {
        self.handle.len()
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[cfg(windows)]
impl Drop for SharedAudio {
    fn drop(&mut self) {
        // SAFETY: this view and handle are owned here.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.view.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "shared_audio_macos.rs"]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn name() -> String {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        format!(
            "xirius-ara-audio-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn replacement_and_close_release_audio_names() {
        let old_name = name();
        let old = SharedAudio::create(&old_name, 48_000.0, &[1.0, 2.0], &[3.0, 4.0]).unwrap();
        assert_eq!(
            SharedAudio::create(&old_name, 48_000.0, &[9.0], &[9.0])
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        let reader = SharedAudio::open(&old_name).unwrap();
        assert_eq!(reader.channel(0), &[1.0, 2.0]);
        assert_eq!(reader.channel(1), &[3.0, 4.0]);
        assert_eq!(reader.rate(), 48_000.0);
        let new_name = name();
        let replacement = SharedAudio::create(&new_name, 44_100.0, &[5.0], &[6.0]).unwrap();
        drop(old);
        assert_eq!(reader.channel(0), &[1.0, 2.0]);
        drop(reader);
        assert!(SharedAudio::open(&old_name).is_err());
        drop(replacement);
        assert!(SharedAudio::open(&new_name).is_err());
    }
}
