//! macOS panic logs. Native crash reports remain the responsibility of macOS.
//! Mach exception capture and minidumps are not implemented in this stage.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
static CONTEXT: OnceLock<(PathBuf, Mutex<String>)> = OnceLock::new();
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
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Logs/Xirius/Crashes")
        })
}
pub fn install(app: &str, filter: bool) -> std::io::Result<()> {
    install_at(&directory(), app, filter)
}
pub fn install_at(root: &Path, app: &str, _filter: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    if CONTEXT
        .set((root.to_path_buf(), Mutex::new(format!("app={app}\n"))))
        .is_err()
    {
        return Ok(());
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some((root, context)) = CONTEXT.get() {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let folder = root.join(format!("{}-{stamp}", std::process::id()));
            if std::fs::create_dir_all(&folder).is_ok() {
                let context = context.try_lock().map(|s| s.clone()).unwrap_or_default();
                let _ = std::fs::write(
                    folder.join("crash.log"),
                    format!("{context}panic: {info}\n"),
                );
            }
        }
        previous(info);
    }));
    Ok(())
}
pub fn note(message: &str) {
    if let Some((_, text)) = CONTEXT.get() {
        let mut text = text.lock().unwrap_or_else(|e| e.into_inner());
        // Bound diagnostic context in long-running sessions.
        if text.len() > 65536 {
            text.clear();
        }
        text.push_str(message);
        text.push('\n');
    }
}
pub fn exception_filter_installed() -> bool {
    false
}
pub fn restore_exception_filter() {}
pub fn guard_exception_filter() -> impl Drop {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {}
    }
    Guard
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
        if log.is_file() {
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

fn alive_since(pid: u32, _stamp: u128) -> bool {
    // Conservative when a PID has been reused: never remove a live process's report.
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}
