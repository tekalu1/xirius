//! Export jobs that outlast the process that ran them (AI-002, AI-010, OPEN-11).
//!
//! An export of a saved song leaves a record in the song folder, `exports/<job>.json`: what
//! was asked, where it went, how far it got and how it ended. The folder moves with the song,
//! and the app, `xirius mcp` and a one-shot `xirius` all find it from the song they open, so
//! the list of what was exported is there again when the song is opened again (OPEN-11). A
//! song not saved yet has no folder; its jobs are kept in memory only.
//!
//! While a job runs, its process holds a lock on `exports/<job>.lock`. The OS lets go of the
//! lock when the process ends, however it ends, so a record that still says `running` with
//! no one holding its lock was cut off: it reads as `interrupted`. The record is written
//! after the lock is taken and its last write comes before the lock is let go.
//!
//! Another process cancels a job by leaving `exports/<job>.cancel`, which the running process
//! looks for while it renders.

use std::fs::{self, File, TryLockError};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::CmdResult;
use xirius_core::store::atomic_write;

/// The folder in the song folder that holds the records.
pub const FOLDER: &str = "exports";
/// Finished records kept; the oldest beyond this are removed.
pub const KEEP: usize = 100;
/// The remembered file name patterns, one for the mix and one for stems (EXP-015).
const SETTINGS: &str = "settings.json";
/// What an interrupted job says.
pub const INTERRUPTED: &str = "書き出しの途中でプロセスが終了しました（クラッシュまたは強制終了）。ファイルは書き出されていません";

/// The records of one song folder.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn of_song(song_dir: &Path) -> Store {
        Store {
            dir: song_dir.join(FOLDER),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, id: &str, ext: &str) -> PathBuf {
        self.dir.join(format!("{id}.{ext}"))
    }

    /// Writes a job's record whole.
    pub fn write(&self, record: &Value) -> CmdResult<()> {
        let id = record["job"].as_str().unwrap_or("export-unknown");
        let bytes = serde_json::to_vec_pretty(record).unwrap_or_default();
        atomic_write(&self.file(id, "json"), &bytes)
    }

    /// Takes the lock that says `id` runs in this process; it is held until the file is
    /// dropped. `None` when the lock cannot be made (the job then reads as interrupted from
    /// elsewhere while it runs, and as what it ended with afterwards).
    pub fn lock(&self, id: &str) -> Option<File> {
        fs::create_dir_all(&self.dir).ok()?;
        let f = File::create(self.file(id, "lock")).ok()?;
        f.try_lock().ok()?;
        Some(f)
    }

    /// Lets go of the lock and clears a cancel that was asked for.
    pub fn release(&self, id: &str, lock: Option<File>) {
        drop(lock);
        let _ = fs::remove_file(self.file(id, "lock"));
        let _ = fs::remove_file(self.file(id, "cancel"));
    }

    /// Whether a process holds `id`'s lock.
    fn alive(&self, id: &str) -> bool {
        let Ok(f) = File::open(self.file(id, "lock")) else {
            return false;
        };
        match f.try_lock() {
            Ok(()) => {
                let _ = f.unlock();
                false
            }
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Error(_)) => false,
        }
    }

    fn read_raw(&self, id: &str) -> Option<Value> {
        let bytes = fs::read(self.file(id, "json")).ok()?;
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(Value::is_object)
    }

    /// A record as it stands: a running one whose process is gone reads as `interrupted`.
    pub fn read(&self, id: &str) -> Option<Value> {
        let v = self.read_raw(id)?;
        Some(self.settle(id, v))
    }

    fn settle(&self, id: &str, mut v: Value) -> Value {
        if v["state"] != "running" || self.alive(id) {
            return v;
        }
        // It may have ended between the read and the look at the lock.
        if let Some(again) = self.read_raw(id)
            && again["state"] != "running"
        {
            return again;
        }
        v["state"] = json!("interrupted");
        v["error"] = json!(INTERRUPTED);
        v
    }

    fn ids(&self) -> Vec<String> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        rd.filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let id = name.strip_suffix(".json")?;
                id.starts_with("export-").then(|| id.to_string())
            })
            .collect()
    }

    /// Every record, oldest first.
    pub fn list(&self) -> Vec<Value> {
        let mut all: Vec<Value> = self.ids().iter().filter_map(|id| self.read(id)).collect();
        all.sort_by_key(|v| v["started_ms"].as_u64().unwrap_or(0));
        all
    }

    /// Asks the process that runs `id` to stop it.
    pub fn ask_cancel(&self, id: &str) -> CmdResult<()> {
        atomic_write(&self.file(id, "cancel"), b"cancel")
    }

    pub fn cancel_asked(&self, id: &str) -> bool {
        self.file(id, "cancel").exists()
    }

    pub fn remove(&self, id: &str) {
        let _ = fs::remove_file(self.file(id, "json"));
        let _ = fs::remove_file(self.file(id, "cancel"));
    }

    /// Removes the oldest finished records beyond [`KEEP`].
    pub fn prune(&self) {
        let finished: Vec<Value> = self
            .list()
            .into_iter()
            .filter(|v| v["state"] != "running")
            .collect();
        for v in finished.iter().take(finished.len().saturating_sub(KEEP)) {
            if let Some(id) = v["job"].as_str() {
                self.remove(id);
            }
        }
    }

    /// The remembered settings (`{"mix_name": ..., "stems_name": ...}`).
    pub fn settings(&self) -> Map<String, Value> {
        fs::read(self.dir.join(SETTINGS))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    pub fn remember(&self, key: &str, value: Value) -> CmdResult<()> {
        let mut s = self.settings();
        if s.get(key) == Some(&value) {
            return Ok(());
        }
        s.insert(key.to_string(), value);
        let bytes = serde_json::to_vec_pretty(&Value::Object(s)).unwrap_or_default();
        atomic_write(&self.dir.join(SETTINGS), &bytes)
    }
}
