//! Real-time scheduling for the audio thread (ENG-007).

/// Keeps the calling thread in the platform's audio scheduling class until dropped.
pub struct ProAudioThread {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(target_os = "macos")]
    thread: u32,
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

    #[cfg(target_os = "macos")]
    pub fn join() -> Option<Self> {
        #[repr(C)]
        struct Timebase {
            numer: u32,
            denom: u32,
        }
        #[repr(C)]
        struct TimeConstraint {
            period: u32,
            computation: u32,
            constraint: u32,
            preemptible: u32,
        }
        unsafe extern "C" {
            fn pthread_mach_thread_np(thread: libc::pthread_t) -> u32;
            fn mach_timebase_info(info: *mut Timebase) -> i32;
            fn thread_policy_set(thread: u32, flavor: i32, policy: *const i32, count: u32) -> i32;
        }
        let mut base = Timebase { numer: 0, denom: 0 };
        // SAFETY: the output points to a valid timebase value.
        if unsafe { mach_timebase_info(&mut base) } != 0 || base.numer == 0 {
            return None;
        }
        let ticks = |ns: u32| {
            ((u128::from(ns) * u128::from(base.denom)) / u128::from(base.numer))
                .min(u128::from(u32::MAX)) as u32
        };
        let policy = TimeConstraint {
            period: ticks(2_000_000),
            computation: ticks(500_000),
            constraint: ticks(1_500_000),
            preemptible: 1,
        };
        // SAFETY: this is the current thread and policy contains four 32-bit fields.
        let thread = unsafe { pthread_mach_thread_np(libc::pthread_self()) };
        if unsafe { thread_policy_set(thread, 2, (&policy as *const TimeConstraint).cast(), 4) }
            != 0
        {
            return None;
        }
        Some(Self { thread })
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn join() -> Option<Self> {
        None
    }
}

#[cfg(target_os = "macos")]
impl Drop for ProAudioThread {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn thread_policy_set(thread: u32, flavor: i32, policy: *const i32, count: u32) -> i32;
        }
        let standard = 0i32;
        // SAFETY: the guard is dropped by the thread that joined this policy.
        unsafe { thread_policy_set(self.thread, 1, &standard, 1) };
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
