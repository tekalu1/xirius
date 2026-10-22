//! Real-time scheduling for the audio thread (ENG-007).

/// Keeps the calling thread in the MMCSS "Pro Audio" class until dropped.
pub struct ProAudioThread {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// The handle is only used to revert the thread it was taken on, which is where the guard lives.
unsafe impl Send for ProAudioThread {}

impl ProAudioThread {
    /// Joins the calling thread to MMCSS "Pro Audio". Returns `None` if Windows refused.
    #[cfg(windows)]
    pub fn join() -> Option<Self> {
        use windows_sys::Win32::System::Threading::{
            AVRT_PRIORITY_HIGH, AvSetMmThreadCharacteristicsW, AvSetMmThreadPriority,
        };
        let name: Vec<u16> = "Pro Audio".encode_utf16().chain([0]).collect();
        let mut index = 0u32;
        // SAFETY: `name` is a NUL-terminated UTF-16 string and `index` outlives the call.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(name.as_ptr(), &mut index) };
        if handle.is_null() {
            return None;
        }
        // SAFETY: `handle` was just returned by AvSetMmThreadCharacteristicsW.
        unsafe { AvSetMmThreadPriority(handle, AVRT_PRIORITY_HIGH) };
        Some(Self { handle })
    }

    #[cfg(not(windows))]
    pub fn join() -> Option<Self> {
        None
    }
}

#[cfg(windows)]
impl Drop for ProAudioThread {
    fn drop(&mut self) {
        // SAFETY: the handle came from AvSetMmThreadCharacteristicsW on this thread.
        unsafe {
            windows_sys::Win32::System::Threading::AvRevertMmThreadCharacteristics(self.handle)
        };
    }
}
