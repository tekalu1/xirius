//! Crash logs and Windows minidumps (ENG-032), prepared before plugin code runs.
//!
//! The two files are opened at start, flat in the folder as `<pid>-<stamp>.log` and `.dmp`,
//! and marked to be deleted when the process lets go of them: a run that ends without a crash,
//! or is ended from outside, leaves nothing (ENG-062). A crash takes the mark off before it
//! writes, and `tidy_at` later moves the pair into its own `<pid>-<stamp>` folder.
use std::fs::File;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, EXCEPTION_ACCESS_VIOLATION, FILETIME, GENERIC_READ,
    GENERIC_WRITE, GetLastError, HANDLE, STILL_ACTIVE,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_BEGIN, FILE_DISPOSITION_INFO, FILE_SHARE_READ, FileDispositionInfo,
    FlushFileBuffers, SetEndOfFile, SetFileInformationByHandle, SetFilePointerEx, WriteFile,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS,
    MINIDUMP_EXCEPTION_INFORMATION, MiniDumpNormal, MiniDumpWriteDump, SEM_NOGPFAULTERRORBOX,
    SetErrorMode, SetUnhandledExceptionFilter,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, GetExitCodeProcess,
    GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

struct Writer {
    log: File,
    dump: File,
    writing: AtomicBool,
    log_failed: AtomicBool,
}
static WRITER: OnceLock<Writer> = OnceLock::new();
static EXCEPTION_FILTER: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
pub struct CrashReport {
    pub folder: PathBuf,
    pub log: PathBuf,
    pub dump: PathBuf,
}

pub fn directory() -> PathBuf {
    std::env::var_os("XIRIUS_CRASH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()))
                .join("Xirius")
                .join("Crashes")
        })
}

/// Initializes handles and the panic hook. A host with its own exception filter passes false
/// and forwards its exception to `record_exception`, preserving its plugin fault marker.
pub fn install(app: &str, exception_filter: bool) -> std::io::Result<()> {
    install_at(&directory(), app, exception_filter)
}

pub fn install_at(root: &Path, app: &str, exception_filter: bool) -> std::io::Result<()> {
    if WRITER.get().is_some() {
        return Ok(());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stem = format!("{}-{stamp}", std::process::id());
    std::fs::create_dir_all(root)?;
    let writer = Writer {
        log: open_marked(&root.join(format!("{stem}.log")))?,
        dump: open_marked(&root.join(format!("{stem}.dmp")))?,
        writing: AtomicBool::new(false),
        log_failed: AtomicBool::new(false),
    };
    let _ = WRITER.set(writer);
    note(&format!(
        "app={app} version={} pid={} executable={:?}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::current_exe()
    ));
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        note(&format!("panic: {info}"));
        // SAFETY: a panic has no Windows exception record; the dump still includes threads.
        unsafe {
            record_exception(std::ptr::null());
        }
        previous(info);
    }));
    if exception_filter {
        // SAFETY: process-wide exception configuration, with no interactive error dialog.
        unsafe {
            SetErrorMode(SEM_NOGPFAULTERRORBOX);
            SetUnhandledExceptionFilter(Some(on_exception));
            if AddVectoredExceptionHandler(1, Some(on_execution_fault)).is_null() {
                return Err(std::io::Error::last_os_error());
            }
        }
        EXCEPTION_FILTER.store(true, Ordering::Release);
    }
    Ok(())
}

/// Restores the app's filter after plugin initialization, including a failed setup. A host
/// that installs its own fault-marker filter (`install(..., false)`) is left alone.
pub fn restore_exception_filter() {
    if EXCEPTION_FILTER.load(Ordering::Acquire) {
        // SAFETY: reinstalling the process-wide handler whose writer is already prepared.
        unsafe {
            SetUnhandledExceptionFilter(Some(on_exception));
        }
    }
}

/// Restores the exception filter on return or unwind from a plugin's setup call.
pub fn guard_exception_filter() -> impl Drop {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            restore_exception_filter();
        }
    }
    Restore
}

/// Opens one of the pair, marked to go when the process closes it. Others may read it but not
/// delete or rename it while it is open, so `tidy_at` leaves the files of a live process alone.
fn open_marked(path: &Path) -> std::io::Result<File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
        .share_mode(FILE_SHARE_READ)
        .open(path)?;
    if !set_delete(&file, true) {
        return Err(std::io::Error::last_os_error());
    }
    Ok(file)
}

/// Sets or clears the delete-on-close mark; one system call with no allocation, so a crash
/// filter can clear it.
fn set_delete(file: &File, delete: bool) -> bool {
    let info = FILE_DISPOSITION_INFO { DeleteFile: delete };
    // SAFETY: a valid handle opened with DELETE access and a structure alive for the call.
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        ) != 0
    }
}

/// Writes control-thread context; never call this from the audio thread.
pub fn note(message: &str) {
    let Some(writer) = WRITER.get() else { return };
    for bytes in [message.as_bytes(), b"\n"] {
        let mut written = 0;
        // SAFETY: valid preopened handle and an immutable buffer alive for this synchronous call.
        unsafe {
            let ok = WriteFile(
                writer.log.as_raw_handle(),
                bytes.as_ptr(),
                bytes.len().min(u32::MAX as usize) as u32,
                &mut written,
                std::ptr::null_mut(),
            );
            if ok == 0 || written as usize != bytes.len() {
                writer.log_failed.store(true, Ordering::Relaxed);
                return;
            }
        }
    }
}

pub fn log_failed() -> bool {
    WRITER
        .get()
        .is_some_and(|w| w.log_failed.load(Ordering::Relaxed))
}

/// # Safety
/// `exception` is null or the exception pointers supplied to this process's Windows filter.
pub unsafe fn record_exception(exception: *const EXCEPTION_POINTERS) {
    let Some(writer) = WRITER.get() else { return };
    if writer
        .writing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    // Kept from here on, even if writing the dump stops part way.
    set_delete(&writer.log, false);
    set_delete(&writer.dump, false);
    note("crash: writing minidump");
    let info = MINIDUMP_EXCEPTION_INFORMATION {
        ThreadId: unsafe { GetCurrentThreadId() },
        ExceptionPointers: exception.cast_mut(),
        ClientPointers: 0,
    };
    // SAFETY: process and output handles are valid; exception pointers remain valid throughout
    // the filter call. An atomic grants one writer, as DbgHelp requires serialized calls.
    unsafe {
        // A caught panic can leave the process alive. Replace its dump on a later fatal fault.
        SetFilePointerEx(
            writer.dump.as_raw_handle(),
            0,
            std::ptr::null_mut(),
            FILE_BEGIN,
        );
        SetEndOfFile(writer.dump.as_raw_handle());
        let ok = MiniDumpWriteDump(
            GetCurrentProcess(),
            GetCurrentProcessId(),
            writer.dump.as_raw_handle(),
            MiniDumpNormal,
            if exception.is_null() {
                std::ptr::null()
            } else {
                &info
            },
            std::ptr::null(),
            std::ptr::null(),
        );
        if ok == 0 {
            note("minidump: failed");
        } else {
            note("minidump: complete");
        }
        FlushFileBuffers(writer.dump.as_raw_handle());
        FlushFileBuffers(writer.log.as_raw_handle());
    }
    writer.writing.store(false, Ordering::Release);
}

unsafe extern "system" fn on_exception(exception: *const EXCEPTION_POINTERS) -> i32 {
    // SAFETY: forwarded directly from Windows's exception filter.
    unsafe {
        record_exception(exception);
    }
    EXCEPTION_CONTINUE_SEARCH
}

unsafe extern "system" fn on_execution_fault(exception: *mut EXCEPTION_POINTERS) -> i32 {
    // Vectored handlers see first-chance exceptions too. Do not dump C++ exceptions,
    // breakpoints, guard pages, or read/write AVs that a plugin may intentionally handle.
    // An execute AV (DEP / a stale DLL callback) is the narrow fallback when a DLL replaced
    // the top-level filter, or a Windows callback boundary bypasses it. This records evidence
    // without trying to recover or changing how Windows dispatches the exception.
    if !exception.is_null() {
        // SAFETY: Windows supplies a valid exception record for the duration of this call.
        unsafe {
            let record = (*exception).ExceptionRecord;
            if !record.is_null()
                && (*record).ExceptionCode == EXCEPTION_ACCESS_VIOLATION
                && (*record).NumberParameters >= 2
                && (*record).ExceptionInformation[0] == 8
            {
                note("crash: execution access violation (vectored)");
                record_exception(exception);
            }
        }
    }
    EXCEPTION_CONTINUE_SEARCH
}

pub fn reports() -> std::io::Result<Vec<CrashReport>> {
    reports_at(&directory())
}
/// The records, newest first. Pairs left by a crash since the last look are moved into their
/// folders first.
pub fn reports_at(root: &Path) -> std::io::Result<Vec<CrashReport>> {
    tidy_in(root, false)?;
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut reports = Vec::new();
    for entry in entries {
        let folder = entry?.path();
        let log = folder.join("crash.log");
        let dump = folder.join("crash.dmp");
        if log.is_file() && dump.metadata().is_ok_and(|m| m.len() > 0) {
            reports.push(CrashReport { folder, log, dump });
        }
    }
    reports.sort_by_key(|r| std::cmp::Reverse(name_key(&r.folder)));
    Ok(reports)
}

/// What `tidy_at` did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tidied {
    /// Pairs a crash left, moved into their folders.
    pub moved: usize,
    /// Records with nothing of a crash in them, removed.
    pub removed: usize,
}

pub fn tidy() -> std::io::Result<Tidied> {
    tidy_at(&directory())
}

/// Clears what runs without a crash left, oldest first: the pairs a power cut kept from being
/// deleted, and the empty folders that every start made before ENG-062. A record with anything
/// of a crash in it (a dump, or a panic or crash line in the log) is kept, and so is anything a
/// live process still has open.
pub fn tidy_at(root: &Path) -> std::io::Result<Tidied> {
    tidy_in(root, true)
}

fn tidy_in(root: &Path, folders: bool) -> std::io::Result<Tidied> {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Tidied::default()),
        Err(e) => return Err(e),
    };
    let mut pairs = Vec::new();
    let mut old = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some(key) = name_key(&path) else { continue };
        if path.is_dir() {
            old.push((key, path));
        } else if path.extension().is_some_and(|e| e == "dmp") {
            pairs.push((key, path.with_extension("")));
        }
    }
    pairs.sort();
    old.sort();
    let mut done = Tidied::default();
    for (_, stem) in pairs {
        let (log, dump) = (stem.with_extension("log"), stem.with_extension("dmp"));
        if crashed(&log, &dump) {
            // The dump first: a live process holds it without letting it move.
            let folder = stem;
            if std::fs::create_dir(&folder).is_err() {
                continue;
            }
            if std::fs::rename(&dump, folder.join("crash.dmp")).is_err() {
                let _ = std::fs::remove_dir(&folder);
                continue;
            }
            let _ = std::fs::rename(&log, folder.join("crash.log"));
            done.moved += 1;
        } else if std::fs::remove_file(&dump).is_ok() {
            let _ = std::fs::remove_file(&log);
            done.removed += 1;
        }
    }
    if folders {
        for ((pid, stamp), folder) in old {
            let (log, dump) = (folder.join("crash.log"), folder.join("crash.dmp"));
            // Builds before ENG-062 kept their files open with no lock against deleting.
            if crashed(&log, &dump) || alive_since(pid, stamp) {
                continue;
            }
            for file in [&dump, &log] {
                match std::fs::remove_file(file) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => continue,
                    _ => {}
                }
            }
            // Only when nothing else is in it.
            if std::fs::remove_dir(&folder).is_ok() {
                done.removed += 1;
            }
        }
    }
    Ok(done)
}

/// `<pid>-<stamp>` (the stamp in nanoseconds since 1970), with or without an extension.
fn name_key(path: &Path) -> Option<(u32, u128)> {
    if path.extension().is_some_and(|e| e != "log" && e != "dmp") {
        return None;
    }
    let (pid, stamp) = path.file_stem()?.to_str()?.split_once('-')?;
    Some((pid.parse().ok()?, stamp.parse().ok()?))
}

fn crashed(log: &Path, dump: &Path) -> bool {
    dump.metadata().is_ok_and(|m| m.len() > 0)
        || std::fs::read(log).is_ok_and(|text| {
            String::from_utf8_lossy(&text).lines().any(|line| {
                ["panic:", "crash:", "minidump:"]
                    .iter()
                    .any(|mark| line.starts_with(mark))
            })
        })
}

/// Whether process `pid` is running and started no later than `stamp`, so it may be the one
/// that wrote the record (and not a later process given the same number).
fn alive_since(pid: u32, stamp: u128) -> bool {
    // SAFETY: plain queries on a handle this function opens and closes.
    unsafe {
        let process: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            // A process that is not ours to look at is taken as running.
            return GetLastError() == ERROR_ACCESS_DENIED;
        }
        let mut code = 0;
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        let running = GetExitCodeProcess(process, &mut code) != 0
            && code == STILL_ACTIVE as u32
            && GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) != 0;
        CloseHandle(process);
        let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        // FILETIME counts 100 ns from 1601.
        let started = u128::from(ticks.saturating_sub(116_444_736_000_000_000)) * 100;
        running && started <= stamp
    }
}
