//! Which history a session writes to when more than one process has the song open (PRJ-023).
//!
//! The screen and the `xirius` program (a command line, the AI's MCP server) may have one song
//! open at the same time. Each process takes a lane: an OS lock on `history/lanes/<k>.lock`,
//! held until the session closes and let go by the OS when the process dies. The first takes
//! lane 0, whose history is `history/`, as before; the others take the next free lane, whose
//! history is `history/lanes/<k>/`. So no process writes over another's steps, cuts them off
//! or clears up their states.
//!
//! A lane's marker (`session.lock` at the top for lane 0, in the lane's folder for the others)
//! is there while a process has the lane. Found by the one taking a lane no process holds, it
//! tells that the last one to have it went down without closing.
//!
//! The process that takes lane 0 closes the other lanes no process holds: where each got to is
//! kept as a version ([`VersionKind::OtherProcess`](crate::store::VersionKind)) when it differs
//! from the song as it opens, and the lane goes.

use super::opening::io::{self as fs, File, TryLockError};
use std::path::{Path, PathBuf};

use crate::error::{CmdError, CmdResult};
use crate::session::LOCK;
use crate::store::{LANES, open_lock};

/// Processes that may have one song open at the same time.
pub const MAX_LANES: u32 = 16;

/// A history lane this process holds (dropping it lets the lane go).
pub(crate) struct Lane {
    pub(crate) k: u32,
    /// `None` when the lock file could not be made (a folder that cannot be written to): the
    /// session then works as one alone.
    file: Option<File>,
}

fn lock_path(dir: &Path, k: u32) -> PathBuf {
    dir.join(LANES).join(format!("{k}.lock"))
}

/// The file that is there while a process has lane `k` (lane 0's is the song's
/// `session.lock`).
pub(crate) fn marker(dir: &Path, k: u32) -> PathBuf {
    if k == 0 {
        dir.join(LOCK)
    } else {
        dir.join(LANES).join(k.to_string()).join(LOCK)
    }
}

/// Lane `k`'s lock, when no process holds it.
pub(crate) fn try_hold(dir: &Path, k: u32) -> std::io::Result<Option<File>> {
    let f = open_lock(&lock_path(dir, k))?;
    super::opening::before("try_lock", &lock_path(dir, k))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

impl Lane {
    /// The first lane of the song in `dir` no process holds; with `first_only`, lane 0 or
    /// none (a new song folder).
    pub(crate) fn take(dir: &Path, first_only: bool) -> CmdResult<Lane> {
        for k in 0..MAX_LANES {
            match try_hold(dir, k) {
                Ok(Some(file)) => {
                    return Ok(Lane {
                        k,
                        file: Some(file),
                    });
                }
                Ok(None) => {}
                // No lock file can be made here: nothing else can have taken a lane either.
                Err(_) if k == 0 => return Ok(Lane { k, file: None }),
                Err(e) => {
                    return Err(CmdError::io(format!(
                        "{} を開けません: {e}",
                        lock_path(dir, k).display()
                    )));
                }
            }
            if first_only {
                break;
            }
        }
        Err(CmdError::conflict(format!(
            "ほかに {} のプロセスがこの曲を開いているため、開けません",
            if first_only { 1 } else { MAX_LANES }
        )))
    }

    /// Lets the lane go without removing its marker, as a process that went down would.
    pub(crate) fn drop_lock(&mut self) {
        self.file = None;
    }
}

/// Does a process other than the one holding lane `own` have the song in `dir` open? Only the
/// lock files there are tried ([`lock_files`]): each may take a while on a slow drive.
pub fn others_open(dir: &Path, own: Option<u32>) -> bool {
    others_open_among(dir, own, &lock_files(dir))
}

/// The lanes with a lock file in `dir`: a process holds a lane through its file, so those are
/// all [`others_open_among`] has to try. A lane taken first after this has a file this did not
/// see (and the folder `history/lanes` is changed).
pub fn lock_files(dir: &Path) -> Vec<u32> {
    let mut lanes: Vec<u32> = fs::read_dir(dir.join(LANES))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            name.strip_suffix(".lock")?.parse::<u32>().ok()
        })
        .filter(|&k| k < MAX_LANES)
        .collect();
    lanes.sort_unstable();
    lanes
}

/// [`others_open`], trying the lock files of `lanes` ([`lock_files`] read before).
pub fn others_open_among(dir: &Path, own: Option<u32>, lanes: &[u32]) -> bool {
    lanes.iter().filter(|&&k| Some(k) != own).any(|&k| {
        LOCKS_TRIED.with(|n| n.set(n.get() + 1));
        let path = lock_path(dir, k);
        let Ok(f) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
            return false;
        };
        matches!(f.try_lock(), Err(TryLockError::WouldBlock))
    })
}

thread_local! {
    static LOCKS_TRIED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many lock files [`others_open`] has tried on this thread: what looking for another
/// process opens, counted (the tests of REC-026, as [`crate::store::files_read`]).
pub fn locks_tried() -> u64 {
    LOCKS_TRIED.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_process_is_looked_for_in_the_lock_files_there_are_only() {
        let dir = std::env::temp_dir().join(format!("xirius-lane-locks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(lock_files(&dir).is_empty());
        let held = try_hold(&dir, 3).unwrap().unwrap();
        drop(try_hold(&dir, 0).unwrap().unwrap());
        fs::write(dir.join(LANES).join("blobs.lock"), b"").unwrap();
        let lanes = lock_files(&dir);
        assert_eq!(lanes, [0, 3]);
        let before = locks_tried();
        assert!(others_open_among(&dir, None, &lanes));
        assert!(!others_open_among(&dir, Some(3), &lanes));
        assert_eq!(
            locks_tried() - before,
            2 + 1,
            "the files there are, but its own"
        );
        let before = locks_tried();
        assert!(others_open(&dir, Some(0)));
        assert_eq!(locks_tried() - before, 1, "lane 3's");
        drop(held);
        assert!(!others_open_among(&dir, None, &lanes));
        let _ = fs::remove_dir_all(&dir);
    }
}
