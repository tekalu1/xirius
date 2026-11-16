//! The crash records of the plugin hosts a test starts, in a folder of the test's own instead
//! of the person's `Crashes` (`XIRIUS_CRASH_DIR`, read by each host as it starts): a host makes
//! its records whether it goes down or not, and leaves the folder behind.

use std::time::{Duration, Instant};

/// Made before the test starts a thread or a host. Removed when dropped, once the hosts still
/// closing have let go of their own records.
pub struct CrashRecords(tempfile::TempDir);

impl CrashRecords {
    pub fn new() -> CrashRecords {
        let dir = tempfile::Builder::new()
            .prefix("xirius-crash-records-")
            .tempdir()
            .unwrap();
        // SAFETY: nextest runs each test in a process of its own (docs/testing.md), and this is
        // set before the test starts a thread.
        unsafe { std::env::set_var("XIRIUS_CRASH_DIR", dir.path()) };
        CrashRecords(dir)
    }
}

impl Drop for CrashRecords {
    fn drop(&mut self) {
        let end = Instant::now() + Duration::from_secs(10);
        while std::fs::remove_dir_all(self.0.path()).is_err()
            && self.0.path().exists()
            && Instant::now() < end
        {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
