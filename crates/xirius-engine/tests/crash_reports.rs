//! Real panic and access-violation children leave discoverable logs and minidumps.
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xirius_engine::crash;

#[test]
fn crash_child() {
    let Ok(kind) = std::env::var("XIRIUS_CRASH_TEST") else {
        return;
    };
    crash::install("test engine", kind != "host-filter").unwrap();
    crash::note("project=test sample_rate=48000 max_block=512");
    if kind == "clean" {
        return;
    }
    if kind == "wait" {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if kind == "panic" {
        panic!("deliberate test failure");
    }
    if kind == "caught-then-exception" {
        let _ = std::panic::catch_unwind(|| panic!("caught first failure"));
        assert!(!crash::log_failed());
    }
    if kind == "handled-read" || kind == "handled-cpp" {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            AddVectoredExceptionHandler, RaiseException, RemoveVectoredExceptionHandler,
        };
        // SAFETY: software exceptions handled by this child's last vectored handler.
        unsafe {
            let handler = AddVectoredExceptionHandler(0, Some(handle_probe));
            assert!(!handler.is_null());
            let code = if kind == "handled-read" {
                0xc0000005
            } else {
                0xe06d7363
            };
            let params = [0usize, 0];
            RaiseException(code, 0, params.len() as u32, params.as_ptr());
            RemoveVectoredExceptionHandler(handler);
        }
        return;
    }
    if kind == "host-filter" {
        use windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter;
        // SAFETY: only this child owns the process filter, which the app must not replace.
        unsafe {
            SetUnhandledExceptionFilter(Some(host_filter));
            {
                let _filter = crash::guard_exception_filter();
            }
            assert_eq!(
                SetUnhandledExceptionFilter(None).map(|f| f as usize),
                Some(host_filter as *const () as usize)
            );
        }
        return;
    }
    if kind == "rearmed-filter-read" {
        let _filter = crash::guard_exception_filter();
        // SAFETY: simulates initialization replacing the filter and then returning.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(None);
        }
    }
    if kind == "replaced-filter-execute" {
        // SAFETY: this child simulates a plugin replacing the process-wide filter.
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(None);
        }
    }
    if kind == "execute" || kind == "replaced-filter-execute" {
        use windows_sys::Win32::System::Memory::{
            MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc,
        };
        // SAFETY: intentional DEP fault in this child only. The allocation has no execute
        // permission; this is a real CPU exception, not a synthetic RaiseException record.
        unsafe {
            let address = VirtualAlloc(
                std::ptr::null(),
                4096,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );
            assert!(!address.is_null());
            *(address as *mut u8) = 0xc3;
            let callback: unsafe extern "system" fn() = std::mem::transmute(address);
            callback();
        }
        unreachable!();
    }
    // SAFETY: deliberate, process-scoped fault in this test's child, with dialogs disabled.
    unsafe {
        windows_sys::Win32::System::Diagnostics::Debug::RaiseException(
            0xC0000005,
            1,
            0,
            std::ptr::null(),
        );
    }
}

unsafe extern "system" fn host_filter(
    _: *const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH
}

unsafe extern "system" fn handle_probe(
    _: *mut windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_EXECUTION
}

#[test]
fn handled_read_and_cpp_exceptions_leave_no_crash_report() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["handled-read", "handled-cpp", "host-filter"] {
        let directory = root.path().join(kind);
        assert!(finish(child(kind, &directory)).success());
        assert_eq!(names(&directory), Vec::<String>::new());
    }
}

#[test]
fn an_execute_fault_is_recorded_even_after_a_plugin_replaces_the_filter() {
    let root = tempfile::tempdir().unwrap();
    for kind in ["execute", "replaced-filter-execute"] {
        let directory = root.path().join(kind);
        let status = finish(child(kind, &directory));
        assert!(!status.success());
        let reports = crash::reports_at(&directory).unwrap();
        assert_eq!(reports.len(), 1, "{kind}");
        let log = std::fs::read_to_string(&reports[0].log).unwrap();
        assert!(log.contains("minidump: complete"));
        let dump = std::fs::read(&reports[0].dump).unwrap();
        let u32_at = |at| u32::from_le_bytes(dump[at..at + 4].try_into().unwrap());
        let directory = u32_at(12) as usize;
        let entry = (0..u32_at(8) as usize)
            .map(|i| directory + i * 12)
            .find(|&entry| u32_at(entry) == 6)
            .unwrap();
        let offset = u32_at(entry + 8) as usize;
        assert_eq!(u32_at(offset + 8), 0xc0000005);
        let access = u64::from_le_bytes(dump[offset + 40..offset + 48].try_into().unwrap());
        assert_eq!(access, 8, "an execution/DEP access violation");
    }
}

#[test]
fn crashes_leave_a_log_and_dump_that_a_later_process_can_find() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("crash-test-{}", std::process::id()));
    for kind in [
        "panic",
        "exception",
        "caught-then-exception",
        "rearmed-filter-read",
    ] {
        let directory = root.join(kind);
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--nocapture"])
            .env("XIRIUS_CRASH_TEST", kind)
            .env("XIRIUS_CRASH_DIR", &directory)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let status = finish(child);
        assert!(!status.success());
        let reports = crash::reports_at(&directory).unwrap();
        assert_eq!(reports.len(), 1);
        let log = std::fs::read_to_string(&reports[0].log).unwrap();
        assert!(log.contains("sample_rate=48000"));
        assert!(log.contains("minidump: complete"));
        let dump = std::fs::read(&reports[0].dump).unwrap();
        assert_eq!(&dump[..4], b"MDMP");
        if kind == "panic" {
            assert!(log.contains("deliberate test failure"));
        }
        if kind == "caught-then-exception" {
            assert_eq!(log.matches("minidump: complete").count(), 2);
        }
        if kind != "panic" {
            let u32_at = |at| u32::from_le_bytes(dump[at..at + 4].try_into().unwrap());
            let count = u32_at(8) as usize;
            let directory = u32_at(12) as usize;
            let exception = (0..count)
                .map(|i| directory + i * 12)
                .find(|&entry| u32_at(entry) == 6)
                .expect("a native exception stream");
            let offset = u32_at(exception + 8) as usize;
            assert_eq!(
                u32_at(offset + 8),
                0xC0000005,
                "the last native fault replaced the caught panic dump"
            );
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn child(kind: &str, directory: &std::path::Path) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_child", "--nocapture"])
        .env("XIRIUS_CRASH_TEST", kind)
        .env("XIRIUS_CRASH_DIR", directory)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn finish(mut child: std::process::Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("the crash reporter did not finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn names(directory: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_run_that_ends_without_a_crash_or_is_ended_from_outside_leaves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("clean");
    let status = finish(child("clean", &directory));
    assert!(status.success());
    assert_eq!(names(&directory), Vec::<String>::new());

    let directory = root.path().join("wait");
    let mut waiting = child("wait", &directory);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !directory.exists() || names(&directory).len() < 2 {
        assert!(
            Instant::now() < deadline,
            "the child never opened its records"
        );
        assert!(waiting.try_wait().unwrap().is_none(), "the child ended");
        std::thread::sleep(Duration::from_millis(10));
    }
    let open = names(&directory);
    assert!(
        open[0].ends_with(".dmp") && open[1].ends_with(".log"),
        "{open:?}"
    );
    // What a live process has open is not tidied away.
    assert_eq!(
        crash::tidy_at(&directory).unwrap(),
        crash::Tidied::default()
    );
    assert_eq!(crash::reports_at(&directory).unwrap().len(), 0);
    assert_eq!(names(&directory), open);
    waiting.kill().unwrap();
    waiting.wait().unwrap();
    assert_eq!(names(&directory), Vec::<String>::new());
}

#[test]
fn tidying_keeps_every_crash_and_clears_what_runs_without_one_left() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut ended = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_child"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let gone = ended.id();
    ended.wait().unwrap();
    let me = std::process::id();
    let folder = |name: &str, log: &str, dump: &[u8]| {
        std::fs::create_dir(directory.join(name)).unwrap();
        std::fs::write(directory.join(name).join("crash.log"), log).unwrap();
        std::fs::write(directory.join(name).join("crash.dmp"), dump).unwrap();
    };
    let pair = |name: &str, log: &str, dump: &[u8]| {
        std::fs::write(directory.join(format!("{name}.log")), log).unwrap();
        std::fs::write(directory.join(format!("{name}.dmp")), dump).unwrap();
    };
    let start = "app=engine version=0.0.0 pid=1\nslot=0 bundle=x\n";
    // Empty folders that every start made before the fix: one of an ended process, and one
    // whose number a later process (this one) was given.
    folder(&format!("{gone}-{}", now - 2), start, b"");
    folder(&format!("{me}-1"), start, b"");
    // Still in use by a running process of an earlier build.
    folder(&format!("{me}-{now}"), start, b"");
    // Crashes, in folders and as pairs.
    folder(
        &format!("{gone}-{}", now - 4),
        &format!("{start}panic: x\n"),
        b"",
    );
    folder(&format!("{gone}-{}", now - 3), start, b"MDMP");
    pair(
        &format!("{gone}-{}", now - 1),
        &format!("{start}crash: writing minidump\n"),
        b"MDMP",
    );
    // A pair a power cut kept from being deleted.
    pair(&format!("{gone}-{}", now - 5), start, b"");
    std::fs::write(directory.join("notes.txt"), "mine").unwrap();

    let done = crash::tidy_at(directory).unwrap();
    assert_eq!(
        done,
        crash::Tidied {
            moved: 1,
            removed: 3
        }
    );
    let mut kept = vec![
        format!("{gone}-{}", now - 1),
        format!("{gone}-{}", now - 3),
        format!("{gone}-{}", now - 4),
        format!("{me}-{now}"),
        "notes.txt".into(),
    ];
    kept.sort();
    assert_eq!(names(directory), kept);
    let reports = crash::reports_at(directory).unwrap();
    let listed: Vec<_> = reports
        .iter()
        .map(|r| r.folder.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    // Newest first; the panic without a dump has no dump to open, as before.
    assert_eq!(
        listed,
        [format!("{gone}-{}", now - 1), format!("{gone}-{}", now - 3)]
    );
    assert_eq!(std::fs::read(&reports[0].dump).unwrap(), b"MDMP");
    assert!(
        std::fs::read_to_string(&reports[0].log)
            .unwrap()
            .contains("crash: writing minidump")
    );
    assert_eq!(crash::tidy_at(directory).unwrap(), crash::Tidied::default());
}
