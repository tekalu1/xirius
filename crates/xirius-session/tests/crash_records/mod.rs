//! The crash records of the plugin hosts a test starts, in a folder of the test's own instead
//! of the person's `Crashes` (`XIRIUS_CRASH_DIR`, read by each host as it starts): a host makes
//! its records whether it goes down or not, and leaves the folder behind.

use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Made before the test starts a thread or a host. Removed when dropped, once the hosts still
/// closing have let go of their own records.
pub struct CrashRecords(PathBuf);

impl CrashRecords {
    pub fn new() -> CrashRecords {
        let dir = tempfile::Builder::new()
            .prefix("crash-records-")
            .tempdir_in(records_dir())
            .unwrap()
            .keep();
        // SAFETY: nextest runs each test in a process of its own (docs/testing.md), and this is
        // set before the test starts a thread.
        unsafe { std::env::set_var("XIRIUS_CRASH_DIR", &dir) };
        CrashRecords(dir)
    }
}

impl Drop for CrashRecords {
    fn drop(&mut self) {
        let end = Instant::now() + Duration::from_secs(10);
        while std::fs::remove_dir_all(&self.0).is_err() && self.0.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Under the test build directory, not the temporary folder: with many tests running, a host
/// may still hold its records after the wait, or a host started as the test ends may make the
/// folder again. The next test removes those older than a test may run (nextest stops one after
/// 5 minutes, `.config/nextest.toml`), as `xirius_test_vst3::copies_dir` does.
fn records_dir() -> PathBuf {
    let dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("xirius-test-crash-records");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(15 * 60));
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    dir
}
