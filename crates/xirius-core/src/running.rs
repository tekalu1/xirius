//! The long jobs running now (EXP-047): bounces and freezes, in this process or in another
//! one. `job.cancel` stops them from the CLI, MCP and the window alike.
//!
//! A job of this process is a flag in a table. Every job also shows as files in [`folder`]:
//! `<id>.json` says what it is and `<id>.lock` is held while it runs, so a `xirius job.cancel`
//! started from a terminal finds a bounce the app or another `xirius` runs, and stops it with an
//! `<id>.cancel` file the job looks for.

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

/// A job as `job.list` and `job.cancel` show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Running {
    pub id: String,
    /// The command that runs (`clip.bounce`, `track.freeze`, ...).
    pub command: String,
    /// The song's folder (none for a song not saved yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song: Option<String>,
    pub pid: u32,
}

/// A job's place in the table while it runs; dropping it takes the job out.
pub struct Ticket {
    job: Running,
    flag: Arc<AtomicBool>,
    dir: PathBuf,
    lock: Option<File>,
}

static TABLE: Mutex<Vec<(Running, Arc<AtomicBool>)>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

fn table() -> MutexGuard<'static, Vec<(Running, Arc<AtomicBool>)>> {
    TABLE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Where the jobs of every process show.
pub fn folder() -> PathBuf {
    std::env::temp_dir().join("Xirius").join("jobs")
}

fn file(dir: &Path, id: &str, ext: &str) -> PathBuf {
    dir.join(format!("{id}.{ext}"))
}

/// Puts a job that runs `command` on the song in `song` into the table.
pub fn start(command: &str, song: Option<&Path>) -> Ticket {
    start_in(command, song, folder())
}

fn start_in(command: &str, song: Option<&Path>, dir: PathBuf) -> Ticket {
    let pid = std::process::id();
    let job = Running {
        id: format!("{pid}-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        command: command.to_string(),
        song: song.map(|p| p.display().to_string()),
        pid,
    };
    let flag = Arc::new(AtomicBool::new(false));
    table().push((job.clone(), flag.clone()));
    // Without the files the job still stops from this process.
    let lock = fs::create_dir_all(&dir).ok().and_then(|()| {
        let f = File::create(file(&dir, &job.id, "lock")).ok()?;
        f.try_lock().ok()?;
        let text = serde_json::to_vec(&job).ok()?;
        fs::write(file(&dir, &job.id, "json"), text).ok()?;
        Some(f)
    });
    Ticket {
        job,
        flag,
        dir,
        lock,
    }
}

impl Ticket {
    pub fn id(&self) -> &str {
        &self.job.id
    }

    /// Whether `job.cancel` has stopped the job.
    pub fn cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
            || (self.lock.is_some() && file(&self.dir, &self.job.id, "cancel").exists())
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        table().retain(|(j, _)| j.id != self.job.id);
        if let Some(lock) = self.lock.take() {
            // Publish completion before closing the handle and removing the sidecars.
            let _ = lock.unlock();
            drop(lock);
            cleanup(&self.dir, &self.job.id);
        }
    }
}

/// Keep the record discoverable until every sidecar is gone. A Windows reader may deny
/// deletion temporarily; the next list can then finish cleaning up without waiting here.
fn cleanup(dir: &Path, id: &str) {
    for ext in ["lock", "cancel", "json"] {
        if let Err(e) = fs::remove_file(file(dir, id, ext))
            && e.kind() != std::io::ErrorKind::NotFound
        {
            return;
        }
    }
}

/// Whether a process holds `id`'s lock. The look takes a shared lock, so two processes (or
/// threads) looking at once do not see each other's look as the job still running.
fn alive(dir: &Path, id: &str) -> bool {
    let Ok(f) = File::open(file(dir, id, "lock")) else {
        return false;
    };
    match f.try_lock_shared() {
        Ok(()) => {
            let _ = f.unlock();
            false
        }
        Err(TryLockError::WouldBlock) => true,
        Err(TryLockError::Error(_)) => false,
    }
}

/// The jobs running now: this process's first, then those of other processes. The files a
/// process left when it ended are removed.
pub fn list() -> Vec<Running> {
    list_in(&folder())
}

fn list_in(dir: &Path) -> Vec<Running> {
    let mut v: Vec<Running> = table().iter().map(|(j, _)| j.clone()).collect();
    let me = std::process::id();
    let Ok(entries) = fs::read_dir(dir) else {
        return v;
    };
    let mut others = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(job) = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Running>(&b).ok())
        else {
            continue;
        };
        if job.pid == me && v.iter().any(|live| live.id == job.id) {
            continue;
        }
        if alive(dir, &job.id) {
            others.push(job);
        } else {
            cleanup(dir, &job.id);
        }
    }
    others.sort_by(|a, b| a.id.cmp(&b.id));
    v.extend(others);
    v
}

/// Stops the job `id` (every job when `None`) at its next block and returns the jobs stopped.
pub fn cancel(id: Option<&str>) -> Vec<Running> {
    cancel_in(&folder(), id)
}

fn cancel_in(dir: &Path, id: Option<&str>) -> Vec<Running> {
    let chosen: Vec<Running> = list_in(dir)
        .into_iter()
        .filter(|j| id.is_none_or(|id| j.id == id))
        .collect();
    let me = std::process::id();
    for j in &chosen {
        if j.pid == me {
            if let Some((_, flag)) = table().iter().find(|(x, _)| x.id == j.id) {
                flag.store(true, Ordering::Relaxed);
            }
        } else {
            let _ = fs::write(file(dir, &j.id, "cancel"), b"");
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_shows_while_it_runs_and_stops_when_cancelled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let a = start_in("clip.bounce", None, dir.into());
        let b = start_in("track.freeze", Some(Path::new("song")), dir.into());
        let ids: Vec<String> = list_in(dir).into_iter().map(|j| j.id).collect();
        assert!(ids.contains(&a.id().to_string()) && ids.contains(&b.id().to_string()));
        assert!(file(dir, a.id(), "json").exists(), "other processes see it");

        let stopped = cancel_in(dir, Some(b.id()));
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].command, "track.freeze");
        assert_eq!(stopped[0].song.as_deref(), Some("song"));
        assert!(b.cancelled() && !a.cancelled());
        assert!(cancel_in(dir, Some("0-0")).is_empty());

        let id = a.id().to_string();
        drop(a);
        assert!(!list_in(dir).iter().any(|j| j.id == id));
        assert!(!file(dir, &id, "json").exists());
        assert!(!file(dir, &id, "lock").exists());
    }

    #[test]
    fn looks_at_the_same_time_do_not_keep_an_ended_job_alive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir).unwrap();
        let id = format!("{}-looks-{}", u32::MAX - 1, std::process::id());
        File::create(file(dir, &id, "lock")).unwrap();
        let wrong = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    for _ in 0..2000 {
                        if alive(dir, &id) {
                            wrong.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let _ = fs::remove_file(file(dir, &id, "lock"));
        assert_eq!(wrong.into_inner(), 0);
    }

    #[test]
    fn a_job_of_another_process_stops_with_a_file_and_one_that_ended_goes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        fs::create_dir_all(dir).unwrap();
        // Another process: the pid is not ours, the lock is held by a handle of its own.
        let other = Running {
            id: format!("{}-other-{}", u32::MAX, std::process::id()),
            command: "clip.bounce".into(),
            song: None,
            pid: u32::MAX,
        };
        let held = File::create(file(dir, &other.id, "lock")).unwrap();
        held.try_lock().unwrap();
        fs::write(
            file(dir, &other.id, "json"),
            serde_json::to_vec(&other).unwrap(),
        )
        .unwrap();
        assert!(list_in(dir).contains(&other));
        assert_eq!(cancel_in(dir, Some(&other.id)), vec![other.clone()]);
        assert!(file(dir, &other.id, "cancel").exists());

        // It ends without cleaning up: the next look removes what it left.
        held.unlock().unwrap();
        drop(held);
        assert!(!list_in(dir).contains(&other));
        for ext in ["json", "cancel", "lock"] {
            assert!(!file(dir, &other.id, ext).exists(), "{ext}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_cleanup_keeps_the_record_for_the_next_look() {
        use std::os::windows::fs::OpenOptionsExt;
        for pid in [u32::MAX, std::process::id()] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path();
            let job = Running {
                id: "ended".into(),
                command: "clip.bounce".into(),
                song: None,
                pid,
            };
            let path = file(dir, &job.id, "lock");
            File::create(&path).unwrap();
            fs::write(
                file(dir, &job.id, "json"),
                serde_json::to_vec(&job).unwrap(),
            )
            .unwrap();
            // A reader that does not share deletion (e.g. a file indexer) delays removal.
            let reader = fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(&path)
                .unwrap();
            assert!(!list_in(dir).contains(&job));
            assert!(path.exists());
            assert!(
                file(dir, &job.id, "json").exists(),
                "retry must remain discoverable"
            );
            drop(reader);
            assert!(!list_in(dir).contains(&job));
            assert!(!path.exists());
            assert!(!file(dir, &job.id, "json").exists());
        }
    }
}
