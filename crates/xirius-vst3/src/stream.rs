//! An in-memory `IBStream` for saving and restoring plugin state.

use std::ffi::c_void;
use std::sync::Mutex;

use vst3::Steinberg::IBStream_::IStreamSeekMode_::{kIBSeekCur, kIBSeekEnd, kIBSeekSet};
use vst3::Steinberg::{
    IBStream, IBStreamTrait, int32, int64, kInvalidArgument, kResultFalse, kResultOk, tresult,
};
use vst3::{Class, ComWrapper};

#[derive(Default)]
struct Inner {
    data: Vec<u8>,
    pos: usize,
}

#[derive(Default)]
pub struct MemoryStream {
    inner: Mutex<Inner>,
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl MemoryStream {
    /// A stream positioned at the start of `data`.
    pub fn with_bytes(data: Vec<u8>) -> ComWrapper<MemoryStream> {
        ComWrapper::new(MemoryStream {
            inner: Mutex::new(Inner { data, pos: 0 }),
        })
    }

    pub fn empty() -> ComWrapper<MemoryStream> {
        Self::with_bytes(Vec::new())
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.inner.lock().unwrap().data.clone()
    }

    pub fn rewind(&self) {
        self.inner.lock().unwrap().pos = 0;
    }
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(&self, buffer: *mut c_void, num_bytes: int32, read: *mut int32) -> tresult {
        if num_bytes < 0 || (buffer.is_null() && num_bytes > 0) {
            return kInvalidArgument;
        }
        let mut s = self.inner.lock().unwrap();
        let n = (num_bytes as usize).min(s.data.len().saturating_sub(s.pos));
        // SAFETY: the plugin's buffer holds `num_bytes` >= n bytes.
        unsafe { std::ptr::copy_nonoverlapping(s.data.as_ptr().add(s.pos), buffer as *mut u8, n) };
        s.pos += n;
        if !read.is_null() {
            // SAFETY: optional out-pointer, checked for null.
            unsafe { *read = n as int32 };
        }
        // Reading past the end is not an error: it returns fewer bytes.
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, num_bytes: int32, written: *mut int32) -> tresult {
        if num_bytes < 0 || (buffer.is_null() && num_bytes > 0) {
            return kInvalidArgument;
        }
        let n = num_bytes as usize;
        let mut s = self.inner.lock().unwrap();
        let end = s.pos + n;
        if s.data.len() < end {
            s.data.resize(end, 0);
        }
        let pos = s.pos;
        // SAFETY: the plugin passes `n` readable bytes; the destination was sized above.
        unsafe {
            std::ptr::copy_nonoverlapping(buffer as *const u8, s.data.as_mut_ptr().add(pos), n)
        };
        s.pos = end;
        if !written.is_null() {
            // SAFETY: optional out-pointer, checked for null.
            unsafe { *written = n as int32 };
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let mut s = self.inner.lock().unwrap();
        let base = match mode {
            m if m == kIBSeekSet => 0,
            m if m == kIBSeekCur => s.pos as i64,
            m if m == kIBSeekEnd => s.data.len() as i64,
            _ => return kInvalidArgument,
        };
        let target = base + pos;
        if target < 0 {
            return kResultFalse;
        }
        s.pos = target as usize;
        if !result.is_null() {
            // SAFETY: optional out-pointer, checked for null.
            unsafe { *result = target };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: checked for null above.
        unsafe { *pos = self.inner.lock().unwrap().pos as int64 };
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_seek_read_round_trip() {
        let s = MemoryStream::empty();
        let mut data = *b"hello world";
        let mut n = 0;
        unsafe {
            assert_eq!(s.write(data.as_mut_ptr().cast(), 11, &mut n), kResultOk);
            assert_eq!(n, 11);
            let mut at = 0;
            s.seek(6, kIBSeekSet, &mut at);
            assert_eq!(at, 6);
            let mut out = [0u8; 16];
            s.read(out.as_mut_ptr().cast(), 16, &mut n);
            assert_eq!(&out[..n as usize], b"world");
            s.seek(-5, kIBSeekEnd, &mut at);
            s.write(b"WORLD".as_ptr() as *mut c_void, 5, &mut n);
            s.seek(-11, kIBSeekCur, &mut at);
            assert_eq!(at, 0);
            s.tell(&mut at);
            assert_eq!(at, 0);
            assert_eq!(s.seek(-1, kIBSeekSet, &mut at), kResultFalse);
        }
        assert_eq!(s.bytes(), b"hello WORLD");
        s.rewind();
    }
}
