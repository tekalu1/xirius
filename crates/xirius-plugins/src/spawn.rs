//! Running the scan process with a time limit.
//!
//! On Windows the process starts on a desktop of its own that is never shown, so a plugin
//! that opens a window (a licence prompt, a splash) does not appear in front of the user; it
//! waits there until the time runs out. The process and anything it starts are in a job that
//! is killed when the time runs out, the scan is stopped or the scan ends.
//!
//! On Windows the time limit counts the process's own time: time it stood ready to run while
//! every processor it may use was busy with other work is left out, so a healthy plugin on a
//! busy machine is not taken for one that hangs (ARC-037). Elsewhere it counts wall time.
//! Until the process says it has started, and once it is ending, the machine rather than the
//! plugin holds it, so on a full machine only its processor time counts then (ARC-042).
//!
//! On Windows every scan has a hidden desktop of its own, and a process that ended before its
//! own code ran is started once more, so a bundle is not taken for a broken one when only the
//! process could not start (ARC-038).

#[cfg(not(windows))]
use std::path::Path;
#[cfg(not(windows))]
use std::time::Duration;

pub(crate) enum Exit {
    Code(u32),
    TimedOut,
    /// Ended because the scan was stopped.
    Stopped,
}

/// How often a running scan process is checked for a stop.
#[cfg(windows)]
const SLICE: std::time::Duration = std::time::Duration::from_millis(50);

#[cfg(windows)]
pub(crate) use windows::{Desktop, run, started};

/// Tells the process that started this one that its own code runs now (ARC-042).
#[cfg(not(windows))]
pub(crate) fn started() {}

#[cfg(not(windows))]
pub(crate) struct Desktop;

#[cfg(not(windows))]
impl Desktop {
    pub(crate) fn hidden() -> Option<Desktop> {
        None
    }
}

#[cfg(not(windows))]
pub(crate) fn run(
    program: &Path,
    args: &[String],
    timeout: Duration,
    _desktop: Option<&Desktop>,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Exit> {
    use std::time::Instant;
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Exit::Code(status.code().unwrap_or(-1) as u32));
        }
        let stopped = stop();
        if stopped || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(if stopped {
                Exit::Stopped
            } else {
                Exit::TimedOut
            });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(windows)]
mod windows {
    use std::collections::HashMap;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use windows_sys::Wdk::System::SystemInformation::{
        NtQuerySystemInformation, SystemProcessInformation, SystemProcessorPerformanceInformation,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, GENERIC_ALL, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
        JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation, QueryInformationJobObject,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, CreateDesktopW, HDESK};
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateEventW, CreateProcessW, EVENT_MODIFY_STATE,
        GetCurrentProcessId, GetExitCodeProcess, GetProcessAffinityMask, OpenEventW,
        PROCESS_INFORMATION, ResetEvent, ResumeThread, STARTF_USESHOWWINDOW, STARTUPINFOW,
        SetEvent, TerminateProcess, WaitForSingleObject,
    };
    use windows_sys::Win32::System::WindowsProgramming::{
        SYSTEM_PROCESS_INFORMATION, SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION,
        SYSTEM_THREAD_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    use super::{Exit, SLICE};

    /// A desktop of this window station that is never switched to.
    pub(crate) struct Desktop {
        handle: HDESK,
        /// `WinSta0\<name>`, NUL-terminated, as `STARTUPINFOW::lpDesktop` wants it.
        full_name: Vec<u16>,
    }

    // SAFETY: a desktop handle is a kernel handle; it is only closed in `drop`.
    unsafe impl Send for Desktop {}
    unsafe impl Sync for Desktop {}

    /// Numbers the hidden desktops of this process.
    static NEXT_DESKTOP: AtomicUsize = AtomicUsize::new(0);

    impl Desktop {
        /// A desktop of its own, so a scan never opens by name one that another scan of this
        /// process is closing (ARC-038).
        pub(crate) fn hidden() -> Option<Desktop> {
            let name = format!(
                "xirius-plugin-scan-{}-{}",
                std::process::id(),
                NEXT_DESKTOP.fetch_add(1, Ordering::Relaxed)
            );
            let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
            // SAFETY: `wide` is NUL-terminated; the rest are the documented defaults.
            let handle = unsafe {
                CreateDesktopW(
                    wide.as_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    GENERIC_ALL,
                    std::ptr::null(),
                )
            };
            if handle.is_null() {
                return None;
            }
            let full_name = format!("WinSta0\\{name}")
                .encode_utf16()
                .chain([0])
                .collect();
            Some(Desktop { handle, full_name })
        }
    }

    impl Drop for Desktop {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateDesktopW and is closed once.
            unsafe { CloseDesktop(self.handle) };
        }
    }

    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: an open handle owned here, closed once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// The exit code of a process that ended before its own code ran: a DLL of Windows could
    /// not start in it (STATUS_DLL_INIT_FAILED). A plugin is loaded later, so it is not the
    /// plugin's doing.
    const NOT_STARTED: u32 = 0xC000_0142;

    /// The name of the event a scan process sets once its own code runs.
    fn started_name(pid: u32) -> Vec<u16> {
        format!("Local\\xirius-scan-started-{pid}")
            .encode_utf16()
            .chain([0])
            .collect()
    }

    /// An unset event the process `pid` sets once its own code runs, or none if it cannot be
    /// made; then the process is taken as started.
    fn start_signal(pid: u32) -> Option<Owned> {
        let name = started_name(pid);
        // SAFETY: `name` is NUL-terminated; no attributes; manual reset, so it stays set.
        let event = Owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) });
        if event.0.is_null() {
            return None;
        }
        // An event of the same name a process of the same id left behind may be set already.
        // SAFETY: an open event handle.
        unsafe { ResetEvent(event.0) };
        Some(event)
    }

    /// Tells the process that started this one that its own code runs now, so from here on
    /// its time is counted as the plugin's (ARC-042). Nothing if no one waits for it.
    pub(crate) fn started() {
        // SAFETY: no arguments.
        let name = started_name(unsafe { GetCurrentProcessId() });
        // SAFETY: `name` is NUL-terminated.
        let event = Owned(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) });
        if !event.0.is_null() {
            // SAFETY: an open event handle with the right to set it.
            unsafe { SetEvent(event.0) };
        }
    }

    pub(crate) fn run(
        program: &Path,
        args: &[String],
        timeout: Duration,
        desktop: Option<&Desktop>,
        stop: &dyn Fn() -> bool,
    ) -> std::io::Result<Exit> {
        let exit = run_once(program, args, timeout, desktop, stop)?;
        if !matches!(exit, Exit::Code(NOT_STARTED)) {
            return Ok(exit);
        }
        // Once more, on a desktop of its own, before the bundle is taken for a broken one: on a
        // busy machine a process now and then cannot start (ARC-038).
        let fresh = Desktop::hidden();
        run_once(program, args, timeout, fresh.as_ref().or(desktop), stop)
    }

    fn run_once(
        program: &Path,
        args: &[String],
        timeout: Duration,
        desktop: Option<&Desktop>,
        stop: &dyn Fn() -> bool,
    ) -> std::io::Result<Exit> {
        let mut line = String::new();
        quote(&program.to_string_lossy(), &mut line);
        for a in args {
            line.push(' ');
            quote(a, &mut line);
        }
        let mut line: Vec<u16> = line.encode_utf16().chain([0]).collect();
        let app: Vec<u16> = program.as_os_str().encode_wide().chain([0]).collect();
        let mut desk_name = desktop.map(|d| d.full_name.clone());

        // SAFETY: a zeroed STARTUPINFOW with `cb` set is the documented empty value.
        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = SW_HIDE as u16;
        if let Some(d) = desk_name.as_mut() {
            si.lpDesktop = d.as_mut_ptr();
        }

        // SAFETY: no attributes; the job is configured below before the process runs.
        let job = Owned(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
        if job.0.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: plain data, zeroed then filled.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        // SAFETY: the struct and its size match the information class.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }

        let mut pi = PROCESS_INFORMATION::default();
        // SAFETY: `app` and `line` are NUL-terminated and `line` is writable, as
        // CreateProcessW requires. No handles are inherited.
        let ok = unsafe {
            CreateProcessW(
                app.as_ptr(),
                line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW | CREATE_SUSPENDED,
                std::ptr::null(),
                std::ptr::null(),
                &si,
                &mut pi,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let process = Owned(pi.hProcess);
        let thread = Owned(pi.hThread);
        // Made before the process runs, so it cannot set the event before there is one.
        let signal = start_signal(pi.dwProcessId);
        // SAFETY: both handles are open; the process is still suspended.
        unsafe {
            if AssignProcessToJobObject(job.0, process.0) == 0 {
                let e = std::io::Error::last_os_error();
                TerminateProcess(process.0, 1);
                return Err(e);
            }
            ResumeThread(thread.0);
        }
        // In slices, so a stopped scan ends its processes at once rather than at the timeout.
        let mut used = OwnTime::new(job.0, process.0, signal);
        loop {
            let left = timeout.saturating_sub(used.so_far());
            let ms = (left.min(SLICE).as_millis() as u32).max(1);
            // SAFETY: an open process handle.
            if unsafe { WaitForSingleObject(process.0, ms) } != WAIT_TIMEOUT {
                break;
            }
            let stopped = stop();
            if stopped || used.tick() >= timeout {
                // SAFETY: the job holds the process and whatever it started.
                unsafe {
                    TerminateJobObject(job.0, 1);
                    WaitForSingleObject(process.0, 5_000);
                }
                return Ok(if stopped {
                    Exit::Stopped
                } else {
                    Exit::TimedOut
                });
            }
        }
        let mut code = 0u32;
        // SAFETY: the process has ended; `code` is written.
        unsafe { GetExitCodeProcess(process.0, &mut code) };
        Ok(Exit::Code(code))
    }

    /// How much of its time limit a scan process has used (ARC-037).
    ///
    /// Wall time while a processor it may use is free or it waits on something of its own (a
    /// dialog on the hidden desktop, a lock, a sleep), so a hanging plugin still runs out of
    /// it. While every processor it may use is busy and it wants one, or waits on the machine
    /// for memory or a lock inside the kernel, or has not started its own code yet, or is
    /// ending (ARC-042), only the processor time it gets: a healthy plugin on a busy machine
    /// takes longer but is not taken for one that hangs.
    struct OwnTime {
        job: HANDLE,
        process: HANDLE,
        /// Set by the process once its own code runs; none once it is seen set.
        signal: Option<Owned>,
        last: Instant,
        idle: Option<Vec<i64>>,
        cpu: Duration,
        used: Duration,
    }

    impl OwnTime {
        fn new(job: HANDLE, process: HANDLE, signal: Option<Owned>) -> OwnTime {
            OwnTime {
                job,
                process,
                signal,
                last: Instant::now(),
                idle: idle_times(),
                cpu: Duration::ZERO,
                used: Duration::ZERO,
            }
        }

        fn so_far(&self) -> Duration {
            self.used
        }

        /// Whether the process has said its own code runs.
        fn started(&mut self) -> bool {
            if let Some(signal) = &self.signal {
                // SAFETY: an open event handle; not waited on.
                if unsafe { WaitForSingleObject(signal.0, 0) } != WAIT_OBJECT_0 {
                    return false;
                }
                self.signal = None;
            }
            true
        }

        /// Adds the time since the last call, or only the processor time the process had in it
        /// if it was kept waiting for one, and returns the time used so far.
        fn tick(&mut self) -> Duration {
            let now = Instant::now();
            let slice = now - self.last;
            self.last = now;
            let idle = idle_times();
            let free = match (&self.idle, &idle) {
                (Some(before), Some(after)) => Some(idle_on(before, after, affinity(self.process))),
                _ => None,
            };
            self.idle = idle;
            let cpu = job_cpu(self.job);
            let ran = cpu.saturating_sub(self.cpu);
            self.cpu = cpu;
            // A processor it may use stood idle for a good part of the slice, so it was not kept
            // waiting; only on a full machine are its threads looked at, which costs more. Until
            // its own code runs, Windows starts it (maps it, has it scanned, starts its console),
            // and on a full machine that waits on other processes kept waiting themselves.
            let had_room = free.is_some_and(|f| f >= slice / 4);
            let own = had_room || (self.started() && !wants_cpu(self.job));
            self.used += if own { slice } else { ran.min(slice) };
            self.used
        }
    }

    /// Each processor's idle time so far, in 100 ns units.
    fn idle_times() -> Option<Vec<i64>> {
        let mut all = [SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION::default(); 64];
        let mut len = 0u32;
        // SAFETY: the buffer and its size in bytes match; `len` is written.
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessorPerformanceInformation,
                all.as_mut_ptr().cast(),
                std::mem::size_of_val(&all) as u32,
                &mut len,
            )
        };
        if status < 0 {
            return None;
        }
        let n = len as usize / std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>();
        Some(all[..n.min(all.len())].iter().map(|p| p.IdleTime).collect())
    }

    /// The processors `process` may run on.
    fn affinity(process: HANDLE) -> usize {
        let (mut mine, mut system) = (0usize, 0usize);
        // SAFETY: an open process handle; both masks are written.
        if unsafe { GetProcessAffinityMask(process, &mut mine, &mut system) } == 0 {
            return usize::MAX;
        }
        mine
    }

    /// The idle time between two readings, summed over the processors in `mask`.
    fn idle_on(before: &[i64], after: &[i64], mask: usize) -> Duration {
        let ticks: i64 = before
            .iter()
            .zip(after)
            .enumerate()
            .filter(|(i, _)| *i < usize::BITS as usize && mask >> i & 1 == 1)
            .map(|(_, (b, a))| (a - b).max(0))
            .sum();
        Duration::from_nanos(ticks as u64 * 100)
    }

    /// How old a look at every thread may be before it is taken again. Taking it costs about
    /// 15 ms of a processor, so the scan's watchers share one.
    const FRESH: Duration = Duration::from_millis(200);

    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;

    /// Thread states (`KTHREAD_STATE`) of a thread that wants a processor: initialized, ready,
    /// standby, transition and deferred ready.
    const WANTS_CPU: [u32; 5] = [0, 1, 3, 6, 7];
    const RUNNING: u32 = 2;
    const WAITING: u32 = 5;

    /// Wait reasons (`KWAIT_REASON`) of a thread the machine keeps waiting rather than its own
    /// code: for memory or a page (FreePage, PageIn, PoolAllocation and their Wr forms,
    /// WrVirtualMemory, WrPageOut) or for a lock inside the kernel (WrResource, WrPushLock,
    /// WrFastMutex, WrGuardedMutex). On a full machine a process that is starting waits on
    /// these for hundreds of milliseconds (ARC-042). A plugin that hangs waits on something of
    /// its own instead: an event, a sleep, a message, a reply.
    const HELD_BY_MACHINE: [u32; 12] = [1, 2, 3, 8, 9, 10, 18, 19, 27, 28, 34, 35];

    /// Whether a waiting thread waits on the machine rather than on something of its own.
    fn held(state: u32, reason: u32) -> bool {
        state == WAITING && HELD_BY_MACHINE.contains(&reason)
    }

    #[derive(Clone, Copy, Default)]
    struct Threads {
        running: bool,
        wanting: bool,
        held: bool,
    }

    struct Seen {
        taken: Instant,
        /// By process id.
        processes: HashMap<usize, Threads>,
    }

    static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

    /// The processor time the job's processes have had.
    fn job_cpu(job: HANDLE) -> Duration {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: the struct and its size match the information class.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Duration::ZERO;
        }
        Duration::from_nanos((info.TotalUserTime + info.TotalKernelTime).max(0) as u64 * 100)
    }

    /// Whether a thread of the job's processes was, when last seen, running, ready to run or
    /// held by the machine, rather than all of them waiting on something of their own.
    fn wants_cpu(job: HANDLE) -> bool {
        let ids = job_processes(job);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if seen.as_ref().is_none_or(|s| s.taken.elapsed() >= FRESH) {
            *seen = Some(look());
        }
        seen.as_ref()
            .is_some_and(|seen| any_wants_cpu(&ids, &seen.processes))
    }

    /// Whether one of the processes `ids` wants a processor or is held by the machine, by a
    /// look at every process. One the look missed is still being made (on a full machine a scan
    /// process was missing for over ten seconds before its own code ran), was made after the
    /// look, or is ending: Windows holds it rather than its own code (ARC-042). An empty look,
    /// one that could not be taken, says nothing, so the time is counted as wall time.
    fn any_wants_cpu(ids: &[usize], processes: &HashMap<usize, Threads>) -> bool {
        !processes.is_empty()
            && ids.iter().any(|id| {
                processes
                    .get(id)
                    .is_none_or(|t| t.running || t.wanting || t.held)
            })
    }

    /// The ids of the processes in `job`.
    fn job_processes(job: HANDLE) -> Vec<usize> {
        #[repr(C)]
        struct List {
            assigned: u32,
            listed: u32,
            ids: [usize; 64],
        }
        let mut list = List {
            assigned: 0,
            listed: 0,
            ids: [0; 64],
        };
        // SAFETY: `List` has the layout of JOBOBJECT_BASIC_PROCESS_ID_LIST with room for 64
        // ids, and its size is passed; a job of more processes lists the first 64.
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicProcessIdList,
                (&mut list as *mut List).cast(),
                std::mem::size_of::<List>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        list.ids[..(list.listed as usize).min(64)].to_vec()
    }

    /// Takes the state of every thread on the machine. An empty look if it cannot be taken,
    /// so the time is counted as wall time.
    fn look() -> Seen {
        let mut processes = HashMap::new();
        let mut filled = None;
        // In u64s, so the entries are aligned.
        let mut buf = vec![0u64; (4 << 20) / 8];
        for _ in 0..4 {
            let mut len = 0u32;
            // SAFETY: the buffer and its size in bytes match; `len` is written.
            let status = unsafe {
                NtQuerySystemInformation(
                    SystemProcessInformation,
                    buf.as_mut_ptr().cast(),
                    (buf.len() * 8) as u32,
                    &mut len,
                )
            };
            if status == STATUS_INFO_LENGTH_MISMATCH {
                buf = vec![0; (len as usize + (1 << 20)) / 8];
                continue;
            }
            if status >= 0 {
                filled = Some((len as usize).min(buf.len() * 8));
            }
            break;
        }
        if let Some(len) = filled {
            let header = std::mem::size_of::<SYSTEM_PROCESS_INFORMATION>();
            let thread = std::mem::size_of::<SYSTEM_THREAD_INFORMATION>();
            let base = buf.as_ptr().cast::<u8>();
            let mut at = 0usize;
            while at + header <= len {
                // SAFETY: inside the part the kernel filled, at an entry's start, which is
                // 8-byte aligned like the buffer.
                let p = unsafe { &*base.add(at).cast::<SYSTEM_PROCESS_INFORMATION>() };
                let n = p.NumberOfThreads as usize;
                if at + header + n * thread > len {
                    break;
                }
                // SAFETY: the entry's threads follow it, and fit in the filled part (checked).
                let threads = unsafe {
                    std::slice::from_raw_parts(
                        base.add(at + header).cast::<SYSTEM_THREAD_INFORMATION>(),
                        n,
                    )
                };
                let mut t = Threads::default();
                for th in threads {
                    t.running |= th.ThreadState == RUNNING;
                    t.wanting |= WANTS_CPU.contains(&th.ThreadState);
                    t.held |= held(th.ThreadState, th.WaitReason);
                }
                processes.insert(p.UniqueProcessId as usize, t);
                if p.NextEntryOffset == 0 {
                    break;
                }
                at += p.NextEntryOffset as usize;
            }
        }
        Seen {
            taken: Instant::now(),
            processes,
        }
    }

    /// Appends `arg` quoted the way the C runtime splits a command line.
    fn quote(arg: &str, out: &mut String) {
        if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
            out.push_str(arg);
            return;
        }
        out.push('"');
        let mut slashes = 0usize;
        for c in arg.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            if c == '"' {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            } else {
                out.extend(std::iter::repeat_n('\\', slashes));
            }
            out.push(c);
            slashes = 0;
        }
        out.extend(std::iter::repeat_n('\\', slashes * 2));
        out.push('"');
    }

    #[cfg(test)]
    mod tests {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, GetProcessAffinityMask,
            GetThreadTimes, SetProcessAffinityMask, SetThreadAffinityMask,
        };

        use super::{
            Desktop, Exit, NOT_STARTED, Threads, WAIT_OBJECT_0, WAITING, any_wants_cpu, held, look,
            quote, run, start_signal, started,
        };

        /// The argument that makes this test program a scan process for the tests below:
        /// `<what>:<processor>`.
        const CHILD: &str = "xirius-own-time-child=";
        /// The processor time the working scan process needs.
        const WORK: Duration = Duration::from_millis(300);
        const LIMIT: Duration = Duration::from_secs(1);
        /// One crowded processor at a time.
        static ONE: Mutex<()> = Mutex::new(());

        /// Run as a scan process by the tests below; does nothing otherwise. Moves to the given
        /// processor, then does some work and ends, sleeps or spins.
        #[test]
        fn child() {
            let Some(arg) = std::env::args().find_map(|a| a.strip_prefix(CHILD).map(str::to_owned))
            else {
                return;
            };
            // Ends as a process that could not start, the first time it sees no `<mark>` file.
            if let Some(mark) = arg.strip_prefix("not-started-once:") {
                if std::fs::File::create_new(mark).is_ok() {
                    std::process::exit(NOT_STARTED as i32);
                }
                return;
            }
            started();
            let mut parts = arg.splitn(3, ':');
            let (what, cpu, mark) = (
                parts.next().unwrap(),
                parts.next().unwrap(),
                parts.next().unwrap(),
            );
            let cpu: u32 = cpu.parse().unwrap();
            // SAFETY: the pseudo handle of this process; the processor is one it may use.
            unsafe { SetProcessAffinityMask(GetCurrentProcess(), 1 << cpu) };
            // When it begins to sleep or spin, in microseconds since the Unix epoch. The working
            // one writes nothing: a file waits on the disk and the virus scan, which is counted
            // as its own time.
            if what != "work" {
                let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                std::fs::write(mark, now.as_micros().to_string()).unwrap();
            }
            match what {
                "work" => {
                    while own_cpu() < WORK {
                        spin_a_little();
                    }
                }
                "sleep" => loop {
                    std::thread::sleep(Duration::from_secs(3600));
                },
                _ => loop {
                    spin_a_little();
                },
            }
        }

        fn spin_a_little() {
            for i in 0..10_000u32 {
                std::hint::black_box(i);
            }
        }

        /// The processor time this thread has had.
        fn own_cpu() -> Duration {
            let mut t = [FILETIME::default(); 4];
            let [created, exited, kernel, user] = &mut t;
            // SAFETY: the pseudo handle of this thread; all four times are written.
            unsafe { GetThreadTimes(GetCurrentThread(), created, exited, kernel, user) };
            let ticks =
                |f: &FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
            Duration::from_nanos((ticks(&t[2]) + ticks(&t[3])) * 100)
        }

        /// The last processor this process may use.
        fn last_cpu() -> u32 {
            let (mut mine, mut system) = (0usize, 0usize);
            // SAFETY: the pseudo handle of this process; both masks are written.
            unsafe { GetProcessAffinityMask(GetCurrentProcess(), &mut mine, &mut system) };
            usize::BITS - 1 - mine.leading_zeros()
        }

        /// Threads that keep one processor busy until dropped.
        struct Crowd {
            stop: Arc<AtomicBool>,
            threads: Vec<std::thread::JoinHandle<()>>,
        }

        impl Crowd {
            fn on(cpu: u32, n: usize) -> Crowd {
                let stop = Arc::new(AtomicBool::new(false));
                let ready = Arc::new(AtomicUsize::new(0));
                let threads = (0..n)
                    .map(|_| {
                        let stop = stop.clone();
                        let ready = ready.clone();
                        std::thread::spawn(move || {
                            // SAFETY: the pseudo handle of this thread; a processor it may use.
                            unsafe { SetThreadAffinityMask(GetCurrentThread(), 1 << cpu) };
                            ready.fetch_add(1, Ordering::Release);
                            while !stop.load(Ordering::Relaxed) {
                                spin_a_little();
                            }
                        })
                    })
                    .collect();
                while ready.load(Ordering::Acquire) < n {
                    std::hint::spin_loop();
                }
                Crowd { stop, threads }
            }
        }

        impl Drop for Crowd {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
                for t in self.threads.drain(..) {
                    let _ = t.join();
                }
            }
        }

        /// Runs this test program as a scan process that does `what` on processor `cpu`, with
        /// `crowd` threads keeping that processor busy. Returns how it ended, how long that took,
        /// and how long it took from when the process began to sleep or spin, if it did.
        fn scan_process(what: &str, cpu: u32, crowd: usize) -> (Exit, Duration, Option<Duration>) {
            let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
            let _crowd = Crowd::on(cpu, crowd);
            let mark =
                std::env::temp_dir().join(format!("xirius-own-time-{}-{what}", std::process::id()));
            let _ = std::fs::remove_file(&mark);
            let args = [
                "spawn::windows::tests::child".to_owned(),
                format!("{CHILD}{what}:{cpu}:{}", mark.display()),
                "--exact".to_owned(),
                "--test-threads=1".to_owned(),
                "--quiet".to_owned(),
            ];
            let exe = std::env::current_exe().unwrap();
            let start = Instant::now();
            let exit = run(&exe, &args, LIMIT, None, &|| false).unwrap();
            let (took, ended) = (start.elapsed(), SystemTime::now());
            let began = std::fs::read_to_string(&mark)
                .ok()
                .and_then(|s| s.parse().ok())
                .map(|us| UNIX_EPOCH + Duration::from_micros(us));
            let _ = std::fs::remove_file(&mark);
            let since = began.map(|b| ended.duration_since(b).unwrap_or_default());
            (exit, took, since)
        }

        #[test]
        fn a_scan_process_kept_waiting_for_a_processor_is_not_timed_out() {
            // Counted in wall time it would have been stopped: it shares its processor with 24
            // other threads, so its work takes longer than the limit. Windows now and then gives
            // it more than its share and it ends within the limit, which does not try the case;
            // then it is run again (ARC-042, once in 152 runs on a full machine).
            let mut tries = Vec::new();
            for _ in 0..6 {
                let (exit, took, _) = scan_process("work", last_cpu(), 24);
                assert!(matches!(exit, Exit::Code(0)), "took {took:?}");
                tries.push(took);
                if took > LIMIT {
                    return;
                }
            }
            panic!("it never waited past the limit: {tries:?}");
        }

        #[test]
        fn a_scan_process_that_waits_on_nothing_still_runs_out_of_time() {
            let (exit, took, since) = scan_process("sleep", last_cpu(), 8);
            assert!(matches!(exit, Exit::TimedOut));
            // Until its own code runs, Windows starts it, which on a full machine is not counted
            // and has taken over ten seconds (ARC-042); from then on it is wall time.
            let since = since.expect("it began to sleep");
            assert!(
                took >= LIMIT && since < LIMIT * 5,
                "took {took:?}, {since:?} of it after it began to sleep"
            );
        }

        #[test]
        fn a_scan_process_that_spins_still_runs_out_of_time() {
            // With three others on its processor it runs a quarter of the time.
            let (exit, took, since) = scan_process("spin", last_cpu(), 3);
            assert!(matches!(exit, Exit::TimedOut));
            let since = since.expect("it began to spin");
            assert!(
                took >= LIMIT && since < LIMIT * 30,
                "took {took:?}, {since:?} of it after it began to spin"
            );
        }

        #[test]
        fn a_scan_process_that_could_not_start_is_started_once_more() {
            let mark =
                std::env::temp_dir().join(format!("xirius-not-started-{}", std::process::id()));
            let _ = std::fs::remove_file(&mark);
            let args = [
                "spawn::windows::tests::child".to_owned(),
                format!("{CHILD}not-started-once:{}", mark.display()),
                "--exact".to_owned(),
                "--test-threads=1".to_owned(),
                "--quiet".to_owned(),
            ];
            let exe = std::env::current_exe().unwrap();
            let exit = run(&exe, &args, Duration::from_secs(60), None, &|| false).unwrap();
            let _ = std::fs::remove_file(&mark);
            assert!(matches!(exit, Exit::Code(0)));
        }

        #[test]
        fn every_hidden_desktop_has_a_name_of_its_own() {
            let a = Desktop::hidden().unwrap();
            let b = Desktop::hidden().unwrap();
            assert_ne!(a.full_name, b.full_name);
        }

        #[test]
        fn the_look_at_every_thread_finds_this_one_running() {
            // SAFETY: no arguments.
            let me = unsafe { GetCurrentProcessId() } as usize;
            let seen = look();
            assert!(seen.processes.len() > 1);
            assert!(seen.processes.get(&me).is_some_and(|t| t.running));
        }

        #[test]
        fn a_thread_waiting_on_the_kernel_is_held_by_the_machine_and_one_waiting_on_its_own_is_not()
        {
            // WrPushLock, WrPageIn, WrResource: what a starting process waits on, on a full machine.
            assert!(held(WAITING, 28));
            assert!(held(WAITING, 9));
            assert!(held(WAITING, 27));
            // Executive, UserRequest, WrUserRequest, DelayExecution, WrQueue, WrLpcReply,
            // WrAlertByThreadId: what a hanging plugin waits on.
            for reason in [0, 6, 13, 4, 15, 17, 37] {
                assert!(!held(WAITING, reason), "{reason}");
            }
            // Only a waiting thread: the reason of one that is ready is left from its last wait.
            assert!(!held(1, 28));
        }

        #[test]
        fn a_process_the_look_missed_is_held_by_the_machine_unless_nothing_was_seen() {
            let own = Threads::default();
            let busy = Threads {
                wanting: true,
                ..Threads::default()
            };
            let seen = std::collections::HashMap::from([(10, own), (11, busy)]);
            // Waits on something of its own.
            assert!(!any_wants_cpu(&[10], &seen));
            assert!(any_wants_cpu(&[10, 11], &seen));
            // Ending, or started after the look.
            assert!(any_wants_cpu(&[10, 12], &seen));
            // A look that could not be taken.
            assert!(!any_wants_cpu(&[12], &std::collections::HashMap::new()));
            assert!(!any_wants_cpu(&[], &seen));
        }

        #[test]
        fn a_scan_process_says_when_its_own_code_runs() {
            // SAFETY: no arguments.
            let me = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };
            let signal = start_signal(me).unwrap();
            let set = || {
                // SAFETY: an open event handle; not waited on.
                unsafe {
                    windows_sys::Win32::System::Threading::WaitForSingleObject(signal.0, 0)
                        == WAIT_OBJECT_0
                }
            };
            assert!(!set());
            started();
            assert!(set());
            // Made again for a process of the same id, it starts unset.
            let again = start_signal(me).unwrap();
            assert!(!set());
            drop(again);
        }

        #[test]
        fn arguments_are_quoted_for_the_c_runtime() {
            let q = |a: &str| {
                let mut s = String::new();
                quote(a, &mut s);
                s
            };
            assert_eq!(q("plain"), "plain");
            assert_eq!(q(""), "\"\"");
            assert_eq!(
                q(r"C:\Program Files\VST3\a.vst3"),
                r#""C:\Program Files\VST3\a.vst3""#
            );
            assert_eq!(q(r"C:\dir with space\"), r#""C:\dir with space\\""#);
            assert_eq!(q(r#"say "hi""#), r#""say \"hi\"""#);
        }
    }
}
