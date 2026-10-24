//! The song on disk: a folder you can read and diff.
//!
//! ```text
//! Song.xirius/
//!   manifest.toml        format version, name, AI permission, which history step was saved
//!   project.json         the song (pretty-printed): tracks, clips, notes, tempo ...
//!   history/index.json   the steps (who, what, when; a long argument as its length) and the cursor
//!   history/<n>.json     the song after step n (compact; long strings point to history/blobs/)
//!   history/blobs/<sha>  long strings (>= 4 KB, such as plugin states) stored once by SHA-256
//!   history/lanes/<k>/   the history of another process that has the song open at the same time
//!                        (index.json, <n>.json, lane.json, session.lock), sharing history/blobs/
//!   history/lanes/<k>.lock  held (an OS file lock) while a process uses history lane k
//!   versions/index.json  named versions and automatic save points
//!   versions/<id>.json   the song at that version
//!   backups/project-<ms>.json  project.json as it was before the AI wrote over it (the last 20)
//! ```
//!
//! Every file is written to a temporary name, flushed, then renamed over the
//! target, so a crash leaves either the old file or the new one, never half
//! of each. A state file is written before the index that mentions it, so an
//! interrupted step costs at most that one step. The blobs a state points to are
//! written before the state. A state whose blob is missing or does not match its
//! hash cannot be read: undoing to it fails with an error, and opening a song whose
//! last state is such falls back to `project.json` (the saved song never points to
//! blobs). The states an undone or dropped step left, and the blobs no kept state
//! points to, are removed by the next save ([`Store::prune_blobs`]).
//!
//! Two processes with the song open (the screen and the `xirius` program, PRJ-023) each keep
//! their own history: the first in `history/`, the others in `history/lanes/<k>/`. A process
//! removes only the state files of its own lane, and a blob only when no state file of any lane
//! points to it. Writing a state holds `history/lanes/blobs.lock` shared, and the blob pruning
//! holds it alone, so a blob written for a state that is not yet on disk is never removed.
//!
//! The AI writes over `project.json` only through [`Store::overwrite_project`] (AI-018): the file
//! has to still be the one the session knows by its SHA-256, a copy goes to `backups/` and is
//! read back first, and the new file is read back after.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{CmdError, CmdResult, ErrorCode};
use crate::model::Project;
use crate::session::{AiPermission, Step};

pub const FORMAT: &str = "xirius-project";
pub const FORMAT_VERSION: u32 = 1;
/// Format version for `history/<n>.json` with content-addressed `history/blobs/<sha256>` (PRJ-021).
pub const HISTORY_FORMAT_VERSION: u32 = 2;
/// Strings at least this long in `history/<n>.json` are stored in `history/blobs/<sha256>`.
pub const BLOB_MIN_BYTES: usize = 4096;
/// What [`Store::read_state_unresolved`] leaves in place of a blob's text: no path or name can
/// be it.
pub const UNREAD_BLOB: &str = "\0$blob";

thread_local! {
    static FILES_READ: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many files the stores have read on this thread: what a step opens, counted (the tests
/// of REC-026, where every file opened may take seconds on a slow drive).
pub fn files_read() -> u64 {
    FILES_READ.with(|n| n.get())
}

fn count_read() {
    FILES_READ.with(|n| n.set(n.get() + 1));
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Writes `bytes` to `path` so that `path` is never left half-written.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> CmdResult<()> {
    crate::session::saving::check()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let result = (|| -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(CmdError::from)
}

/// The SHA-256 of `bytes`, in lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Copies of `project.json` the AI's saves keep in `backups/` (AI-018).
pub const BACKUP_KEEP: usize = 20;

/// What a write over `project.json` kept aside and what it wrote (AI-018).
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct Overwrite {
    /// The copy of the old `project.json`, relative to the song folder.
    pub backup: String,
    /// SHA-256 of `project.json` before the write.
    pub before: String,
    /// SHA-256 of `project.json` after the write.
    pub after: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub name: String,
    pub saved_at_ms: u64,
    /// The history step (absolute number) `project.json` was written at.
    pub saved_step: u64,
    pub ai_permission: AiPermission,
    pub next_version: u64,
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format_version: u32,
    project: Project,
}

#[derive(Serialize, Deserialize)]
struct RawProjectFile {
    format_version: u32,
    project: Value,
}

fn project_bytes(p: &Project, pretty: bool) -> CmdResult<Vec<u8>> {
    let f = ProjectFile {
        format_version: FORMAT_VERSION,
        project: p.clone(),
    };
    if pretty {
        serde_json::to_vec_pretty(&f)
    } else {
        serde_json::to_vec(&f)
    }
    .map_err(|e| CmdError::io(e.to_string()))
}

fn extract_blobs(v: &mut Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::String(s) if s.len() >= BLOB_MIN_BYTES => {
            let hash = sha256_hex(s.as_bytes());
            let text = std::mem::take(s);
            out.push((hash.clone(), text));
            *v = json!({ "$blob": hash });
        }
        Value::Array(a) => {
            for item in a {
                extract_blobs(item, out);
            }
        }
        Value::Object(o) => {
            for val in o.values_mut() {
                extract_blobs(val, out);
            }
        }
        _ => {}
    }
}

fn collect_blob_refs(v: &Value, used: &mut HashSet<String>) {
    match v {
        Value::Object(o) => {
            if let Some(hash) = blob_ref(o) {
                used.insert(hash.to_string());
                return;
            }
            for val in o.values() {
                collect_blob_refs(val, used);
            }
        }
        Value::Array(a) => {
            for item in a {
                collect_blob_refs(item, used);
            }
        }
        _ => {}
    }
}

/// A version's ID is `v` and a number, so it can only name a file in `versions/`.
fn check_version_id(id: &str) -> CmdResult<()> {
    if !id.starts_with('v') || !id[1..].chars().all(|c| c.is_ascii_digit()) {
        return Err(CmdError::invalid(format!(
            "バージョンの ID `{id}` が正しくありません（v1・v2 の形式）"
        )));
    }
    Ok(())
}

/// Each `{"$blob": "<hash>"}` in `v` as [`UNREAD_BLOB`], for [`Store::read_state_unresolved`].
fn leave_blobs(v: &mut Value, rel: &str) -> CmdResult<()> {
    match v {
        Value::Object(o) => {
            if let Some(hash) = blob_ref(o) {
                if !is_valid_blob_hash(hash) {
                    return Err(Store::corrupt(rel, "不正な blob ハッシュ"));
                }
                *v = Value::String(UNREAD_BLOB.into());
                return Ok(());
            }
            for val in o.values_mut() {
                leave_blobs(val, rel)?;
            }
        }
        Value::Array(a) => {
            for item in a {
                leave_blobs(item, rel)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// A blob's name is the SHA-256 of its text in lowercase hex, so it can only name a file in
/// `history/blobs/`.
fn is_valid_blob_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// The hash in `{"$blob": "<hash>"}`, the one shape `extract_blobs` writes. A song's own map
/// may hold the key `$blob` (a parameter's name), but its value is never a string.
fn blob_ref(o: &serde_json::Map<String, Value>) -> Option<&str> {
    match o.get("$blob") {
        Some(Value::String(hash)) if o.len() == 1 => Some(hash),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionKind {
    /// Named by the user.
    Named,
    /// A save point made on a timer.
    Auto,
    /// The saved state kept aside before a crash recovery replaced it.
    BeforeCrash,
    /// Where another process that had the song open at the same time left it (PRJ-023).
    OtherProcess,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    pub name: String,
    pub kind: VersionKind,
    pub created_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryIndex {
    /// Absolute number of the oldest kept state.
    pub first: u64,
    /// Absolute number of the state the song is at.
    pub cursor: u64,
    /// `steps[i]` made state `first + i + 1`.
    pub steps: Vec<Step>,
    /// Highest id counter seen, so ids stay unique after an undo.
    #[serde(default)]
    pub id_floor: u64,
}

/// What [`Store::project_stamp`] reads of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
    /// The history lane this store writes steps to: 0 is `history/`, k is `history/lanes/<k>/`.
    pub lane: u32,
}

/// The folder holding the other processes' history lanes and their lock files (PRJ-023).
pub const LANES: &str = "history/lanes";
const BLOBS_LOCK: &str = "blobs.lock";

fn lane_rel(lane: u32) -> String {
    if lane == 0 {
        "history".to_string()
    } else {
        format!("{LANES}/{lane}")
    }
}

/// Opens (making it when missing) a file used only for its OS lock. Lock files are never
/// removed: removing one another process has open would let a third lock a new file.
pub(crate) fn open_lock(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// The numbered state files (`<n>.json`) in `dir`.
fn numbered_states(dir: &Path) -> Vec<(u64, PathBuf)> {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let n = name.strip_suffix(".json")?.parse::<u64>().ok()?;
            Some((n, entry.path()))
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
struct LaneFile {
    saved_step: u64,
}

impl Store {
    pub fn read_view(&self) -> CmdResult<crate::session::View> {
        if !self.p("view.json").is_file() {
            return Ok(crate::session::View::default());
        }
        serde_json::from_slice(&self.read_bytes("view.json")?)
            .map_err(|e| Self::corrupt("view.json", e))
    }

    pub fn write_view(&self, view: &crate::session::View) -> CmdResult<()> {
        let bytes = serde_json::to_vec_pretty(view).map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(&self.p("view.json"), &bytes)
    }
    pub fn new(dir: impl Into<PathBuf>) -> Store {
        Store::in_lane(dir, 0)
    }

    /// The song in `dir`, writing its history steps to lane `lane` (PRJ-023).
    pub fn in_lane(dir: impl Into<PathBuf>, lane: u32) -> Store {
        Store {
            dir: dir.into(),
            lane,
        }
    }

    pub fn is_song(dir: &Path) -> bool {
        dir.join("manifest.toml").is_file()
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn read_bytes(&self, rel: &str) -> CmdResult<Vec<u8>> {
        count_read();
        fs::read(self.p(rel))
            .map_err(|e| CmdError::from(e).with_context(format!("{rel} を読み込めません")))
    }

    fn corrupt(rel: &str, e: impl std::fmt::Display) -> CmdError {
        CmdError::new(ErrorCode::Corrupt, format!("{rel} が破損しています: {e}"))
    }

    // ---- manifest and project ------------------------------------------

    pub fn read_manifest(&self) -> CmdResult<Manifest> {
        let text = String::from_utf8(self.read_bytes("manifest.toml")?)
            .map_err(|e| Self::corrupt("manifest.toml", e))?;
        let m: Manifest = toml::from_str(&text).map_err(|e| Self::corrupt("manifest.toml", e))?;
        if m.format != FORMAT {
            return Err(Self::corrupt("manifest.toml", "Xirius の曲ではありません"));
        }
        if m.format_version > FORMAT_VERSION {
            return Err(CmdError::new(
                ErrorCode::Corrupt,
                format!(
                    "新しい形式（バージョン {}）の曲のため、このアプリでは開けません",
                    m.format_version
                ),
            ));
        }
        Ok(m)
    }

    pub fn write_manifest(&self, m: &Manifest) -> CmdResult<()> {
        let text = toml::to_string_pretty(m).map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(&self.p("manifest.toml"), text.as_bytes())
    }

    /// Writes `m` over a manifest another process may also write (PRJ-023): the version
    /// counter never goes back, and a process in a lane other than 0 keeps the saved step on
    /// disk (its own steps are not in `history/`).
    pub fn update_manifest(&self, m: &Manifest) -> CmdResult<()> {
        let _guard = self.lock_versions()?;
        let mut m = m.clone();
        if let Ok(disk) = self.read_manifest() {
            m.next_version = m.next_version.max(disk.next_version);
            if self.lane != 0 {
                m.saved_step = disk.saved_step;
            }
        }
        self.write_manifest(&m)
    }

    pub fn read_project(&self) -> CmdResult<Project> {
        Ok(self.read_project_report()?.0)
    }

    /// `project.json`, repaired the way a song is read (PRJ-026), with the
    /// rows of what the repair changed.
    pub fn read_project_report(&self) -> CmdResult<(Project, Vec<crate::track_preset::Adjust>)> {
        self.read_project_file("project.json")
    }

    /// `project.json` with the SHA-256 of the bytes it was read from, read once, so the two
    /// cannot come from different writes (UI-289).
    pub fn read_project_hashed(&self) -> CmdResult<(Project, String)> {
        let (p, _, hash) = self.read_project_hashed_report()?;
        Ok((p, hash))
    }

    /// [`Store::read_project_hashed`], also giving the repair rows.
    pub fn read_project_hashed_report(
        &self,
    ) -> CmdResult<(Project, Vec<crate::track_preset::Adjust>, String)> {
        let bytes = self.read_bytes("project.json")?;
        let (p, rows) = Self::parse_project("project.json", &bytes)?;
        Ok((p, rows, sha256_hex(&bytes)))
    }

    fn read_project_file(
        &self,
        rel: &str,
    ) -> CmdResult<(Project, Vec<crate::track_preset::Adjust>)> {
        Self::parse_project(rel, &self.read_bytes(rel)?)
    }

    fn parse_project(
        rel: &str,
        bytes: &[u8],
    ) -> CmdResult<(Project, Vec<crate::track_preset::Adjust>)> {
        let mut f: ProjectFile =
            serde_json::from_slice(bytes).map_err(|e| Self::corrupt(rel, e))?;
        if f.format_version > FORMAT_VERSION {
            return Err(Self::corrupt(rel, "新しい形式"));
        }
        let rows = f.project.fix_racks();
        f.project
            .validate()
            .map_err(|e| Self::corrupt(rel, e.message))?;
        Ok((f.project, rows))
    }

    pub fn write_project(&self, p: &Project) -> CmdResult<()> {
        self.write_project_file("project.json", p, true)
    }

    fn write_project_file(&self, rel: &str, p: &Project, pretty: bool) -> CmdResult<()> {
        atomic_write(&self.p(rel), &project_bytes(p, pretty)?)
    }

    /// The size and modification time of `project.json` (none when there is no such file):
    /// what tells, without reading it, that the file may have changed (PRJ-025).
    pub fn project_stamp(&self) -> CmdResult<Option<Stamp>> {
        match fs::metadata(self.p("project.json")) {
            Ok(m) => Ok(Some(Stamp {
                len: m.len(),
                modified: m.modified().ok(),
            })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CmdError::from(e).with_context("project.json を読み込めません")),
        }
    }

    /// The SHA-256 of `project.json` as it is on disk (none when there is no such file).
    pub fn project_hash(&self) -> CmdResult<Option<String>> {
        match fs::read(self.p("project.json")) {
            Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(CmdError::from(e).with_context("project.json を読み込めません")),
        }
    }

    /// `project.json` as it is on disk, when it is still the file `expected` (its SHA-256) says.
    pub fn check_project(&self, expected: &str) -> CmdResult<Vec<u8>> {
        let bytes = fs::read(self.p("project.json")).map_err(|e| {
            CmdError::conflict(format!(
                "project.json を読み込めないため、上書きを中止しました: {e}"
            ))
        })?;
        if sha256_hex(&bytes) != expected {
            return Err(CmdError::conflict(
                "project.json が開いた後にほかで書き換えられているため、上書きを中止しました",
            ));
        }
        Ok(bytes)
    }

    /// Writes `p` over `project.json` the way the AI may (AI-018): only when the file is still
    /// `expected`, after a copy of it is in `backups/` and reads back the same, and reading the
    /// new file back must give what was written, or the copy goes back in its place.
    pub fn overwrite_project(&self, p: &Project, expected: &str) -> CmdResult<Overwrite> {
        self.overwrite_project_with(p, expected, &mut |path, bytes| atomic_write(path, bytes))
    }

    fn overwrite_project_with(
        &self,
        p: &Project,
        expected: &str,
        write: &mut dyn FnMut(&Path, &[u8]) -> CmdResult<()>,
    ) -> CmdResult<Overwrite> {
        let old = self.check_project(expected)?;
        let before = sha256_hex(&old);
        let backup = self.backup_name();
        let backup_path = self.p(&backup);
        write(&backup_path, &old)?;
        if fs::read(&backup_path).map(|b| sha256_hex(&b)).ok() != Some(before.clone()) {
            let _ = fs::remove_file(&backup_path);
            return Err(CmdError::io(format!(
                "控え {backup} を読み戻すとハッシュが合わないため、上書きを中止しました"
            )));
        }
        let new = project_bytes(p, true)?;
        let after = sha256_hex(&new);
        let path = self.p("project.json");
        let written = write(&path, &new);
        if written.is_err() || fs::read(&path).map(|b| sha256_hex(&b)).ok() != Some(after.clone()) {
            let back = write(&path, &old);
            let restored = back.is_ok()
                && fs::read(&path).map(|b| sha256_hex(&b)).ok() == Some(before.clone());
            return Err(CmdError::io(if restored {
                format!(
                    "書いた project.json を読み戻すとハッシュが合わないため、元に戻しました（控え: {backup}）"
                )
            } else {
                format!(
                    "書いた project.json を読み戻すとハッシュが合わず、元にも戻せませんでした。控え {backup} から戻してください"
                )
            }));
        }
        self.prune_backups(BACKUP_KEEP);
        Ok(Overwrite {
            backup,
            before,
            after,
        })
    }

    /// `backups/project-<ms>.json`, a millisecond later when that name is taken.
    fn backup_name(&self) -> String {
        let mut ms = now_ms();
        loop {
            let rel = format!("backups/project-{ms:013}.json");
            if !self.p(&rel).exists() {
                return rel;
            }
            ms += 1;
        }
    }

    /// The copies in `backups/`, oldest first.
    pub fn backups(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.p("backups"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("project-") && n.ends_with(".json"))
            .map(|n| format!("backups/{n}"))
            .collect();
        names.sort();
        names
    }

    fn prune_backups(&self, keep: usize) {
        let names = self.backups();
        for old in &names[..names.len().saturating_sub(keep)] {
            let _ = fs::remove_file(self.p(old));
        }
    }

    // ---- history ---------------------------------------------------------

    /// The folder of this store's history lane, relative to the song folder.
    pub fn history_rel(&self) -> String {
        lane_rel(self.lane)
    }

    fn state_rel(&self, abs: u64) -> String {
        format!("{}/{abs}.json", self.history_rel())
    }

    fn index_rel(&self) -> String {
        format!("{}/index.json", self.history_rel())
    }

    pub fn write_state(&self, abs: u64, p: &Project) -> CmdResult<()> {
        let mut v = serde_json::to_value(p).map_err(|e| CmdError::io(e.to_string()))?;
        let mut blobs = Vec::new();
        extract_blobs(&mut v, &mut blobs);
        // Held from the first blob until the state that points to them is on disk, so that
        // another process's pruning (which holds it alone) does not take a blob away between.
        let _shared = if blobs.is_empty() {
            None
        } else {
            Some(self.lock(BLOBS_LOCK, false)?)
        };
        for (hash, text) in blobs {
            // Same name, same text: a blob already there is kept. One cut short or made over
            // by hand is written again (a full check would read every blob on every edit).
            let blob_path = self.p(&format!("history/blobs/{hash}"));
            if !fs::metadata(&blob_path).is_ok_and(|m| m.len() == text.len() as u64) {
                atomic_write(&blob_path, text.as_bytes())?;
            }
        }
        let f = RawProjectFile {
            format_version: HISTORY_FORMAT_VERSION,
            project: v,
        };
        let bytes = serde_json::to_vec(&f).map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(&self.p(&self.state_rel(abs)), &bytes)
    }

    pub fn read_state(&self, abs: u64) -> CmdResult<Project> {
        Ok(self.read_state_report(abs)?.0)
    }

    /// A history state, repaired the way a song is read (PRJ-026), with the
    /// rows of what the repair changed.
    pub fn read_state_report(
        &self,
        abs: u64,
    ) -> CmdResult<(Project, Vec<crate::track_preset::Adjust>)> {
        let rel = self.state_rel(abs);
        let bytes = self.read_bytes(&rel)?;
        let mut f: RawProjectFile =
            serde_json::from_slice(&bytes).map_err(|e| Self::corrupt(&rel, e))?;
        match f.format_version {
            FORMAT_VERSION => {}
            HISTORY_FORMAT_VERSION => self.resolve_blobs(&mut f.project, &rel)?,
            _ => return Err(Self::corrupt(&rel, "新しい形式")),
        }
        let mut p: Project =
            serde_json::from_value(f.project).map_err(|e| Self::corrupt(&rel, e))?;
        let rows = p.fix_racks();
        p.validate().map_err(|e| Self::corrupt(&rel, e.message))?;
        Ok((p, rows))
    }

    /// A history state with the blobs it points to left unread: each long text held in
    /// `history/blobs/` is [`UNREAD_BLOB`] (not read, nor its SHA-256 checked). Enough for what
    /// names the files of the clips (REC-026), whose paths are shorter than a blob's text; one
    /// that is `UNREAD_BLOB` was not, and is read with [`Store::read_state`].
    ///
    /// Use it only to collect the files the clips name. Nothing else in it holds: it is not
    /// repaired (the racks) nor checked as a song ([`Project::validate`]), and every long text
    /// in it is `UNREAD_BLOB`; a song made from it is not one that was.
    pub fn read_state_unresolved(&self, abs: u64) -> CmdResult<Project> {
        let rel = self.state_rel(abs);
        let bytes = self.read_bytes(&rel)?;
        let mut f: RawProjectFile =
            serde_json::from_slice(&bytes).map_err(|e| Self::corrupt(&rel, e))?;
        match f.format_version {
            FORMAT_VERSION => {}
            HISTORY_FORMAT_VERSION => leave_blobs(&mut f.project, &rel)?,
            _ => return Err(Self::corrupt(&rel, "新しい形式")),
        }
        serde_json::from_value(f.project).map_err(|e| Self::corrupt(&rel, e))
    }

    fn resolve_blobs(&self, v: &mut Value, rel: &str) -> CmdResult<()> {
        match v {
            Value::Object(o) => {
                if let Some(hash) = blob_ref(o) {
                    if !is_valid_blob_hash(hash) {
                        return Err(Self::corrupt(rel, "不正な blob ハッシュ"));
                    }
                    let blob_rel = format!("history/blobs/{hash}");
                    count_read();
                    let bytes = fs::read(self.p(&blob_rel)).map_err(|e| {
                        Self::corrupt(rel, format!("{blob_rel} を読み込めません: {e}"))
                    })?;
                    if sha256_hex(&bytes) != hash {
                        return Err(Self::corrupt(
                            rel,
                            format!("{blob_rel} のハッシュが一致しません"),
                        ));
                    }
                    let text = String::from_utf8(bytes).map_err(|e| Self::corrupt(rel, e))?;
                    *v = Value::String(text);
                    return Ok(());
                }
                for val in o.values_mut() {
                    self.resolve_blobs(val, rel)?;
                }
            }
            Value::Array(a) => {
                for item in a {
                    self.resolve_blobs(item, rel)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Removes this lane's state files outside `first..=last`, and the files in `history/blobs/`
    /// that neither a kept state of this lane nor any state file of another lane points to
    /// (PRJ-021, PRJ-023), with the temporary files a crash left there.
    pub fn prune_blobs(&self, first: u64, last: u64) -> CmdResult<usize> {
        for (n, path) in numbered_states(&self.p(&self.history_rel())) {
            if n < first || n > last {
                let _ = fs::remove_file(path);
            }
        }
        let blobs_dir = self.p("history/blobs");
        if !blobs_dir.is_dir() {
            return Ok(0);
        }
        let _alone = self.lock(BLOBS_LOCK, true)?;
        let mut used = HashSet::new();
        for abs in first..=last {
            let rel = self.state_rel(abs);
            Self::collect_state_refs(&rel, &self.read_bytes(&rel)?, &mut used)?;
        }
        for lane in self.lanes() {
            if lane == self.lane {
                continue;
            }
            for (_, path) in numbered_states(&self.p(&lane_rel(lane))) {
                match fs::read(&path) {
                    Ok(bytes) => {
                        Self::collect_state_refs(&path.display().to_string(), &bytes, &mut used)?
                    }
                    // The other process removed it (an undone step it dropped).
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(CmdError::io(format!(
                            "{} を読み込めません: {e}",
                            path.display()
                        )));
                    }
                }
            }
        }
        let mut removed = 0;
        for entry in fs::read_dir(&blobs_dir).map_err(|e| CmdError::io(e.to_string()))? {
            let entry = entry.map_err(|e| CmdError::io(e.to_string()))?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // A temporary file is what a write cut off by a crash left: one in the middle of
            // its write holds the lock shared, so none is now.
            if (name.contains(".tmp") || !used.contains(&name))
                && entry.file_type().is_ok_and(|t| t.is_file())
                && fs::remove_file(entry.path()).is_ok()
            {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn collect_state_refs(rel: &str, bytes: &[u8], used: &mut HashSet<String>) -> CmdResult<()> {
        let f: RawProjectFile = serde_json::from_slice(bytes).map_err(|e| Self::corrupt(rel, e))?;
        if f.format_version == HISTORY_FORMAT_VERSION {
            collect_blob_refs(&f.project, used);
        }
        Ok(())
    }

    /// The history lanes that have a folder: 0 (`history/`) and every `history/lanes/<k>/`.
    pub fn lanes(&self) -> Vec<u32> {
        let mut lanes: Vec<u32> = fs::read_dir(self.p(LANES))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok()?.parse::<u32>().ok())
            .filter(|&k| k != 0)
            .collect();
        lanes.sort_unstable();
        lanes.insert(0, 0);
        lanes
    }

    /// Takes the OS lock on `history/lanes/<name>`, waiting for it: alone, or shared with the
    /// others that take it shared. It is let go when the returned file is dropped.
    fn lock(&self, name: &str, alone: bool) -> CmdResult<File> {
        let path = self.p(&format!("{LANES}/{name}"));
        let f = open_lock(&path)?;
        if alone { f.lock() } else { f.lock_shared() }.map_err(CmdError::from)?;
        Ok(f)
    }

    /// The step `project.json` was last written at by this lane (lanes other than 0 keep it in
    /// `lane.json`; lane 0's is the manifest's `saved_step`).
    pub fn lane_saved_step(&self) -> Option<u64> {
        let bytes = fs::read(self.p(&format!("{}/lane.json", self.history_rel()))).ok()?;
        serde_json::from_slice::<LaneFile>(&bytes)
            .ok()
            .map(|f| f.saved_step)
    }

    pub fn write_lane_saved_step(&self, saved_step: u64) -> CmdResult<()> {
        let bytes = serde_json::to_vec(&LaneFile { saved_step })
            .map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(
            &self.p(&format!("{}/lane.json", self.history_rel())),
            &bytes,
        )
    }

    /// Removes the folder of history lane `lane` (not 0), which no process holds any more.
    pub fn remove_lane(&self, lane: u32) -> CmdResult<()> {
        if lane == 0 {
            return Ok(());
        }
        let path = self.p(&lane_rel(lane));
        match fs::remove_dir_all(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CmdError::io(format!(
                "{} を消せません: {e}",
                path.display()
            ))),
        }
    }

    /// The format of the state file `abs`, when it can be read.
    pub fn state_format(&self, abs: u64) -> Option<u32> {
        #[derive(Deserialize)]
        struct Format {
            format_version: u32,
        }
        let bytes = fs::read(self.p(&self.state_rel(abs))).ok()?;
        serde_json::from_slice::<Format>(&bytes)
            .ok()
            .map(|f| f.format_version)
    }

    /// Are there state files in this lane outside `first..=last`?
    pub fn has_states_outside(&self, first: u64, last: u64) -> bool {
        numbered_states(&self.p(&self.history_rel()))
            .iter()
            .any(|&(n, _)| n < first || n > last)
    }

    /// Does this lane have a history index?
    pub fn has_index(&self) -> bool {
        self.p(&self.index_rel()).is_file()
    }

    pub fn has_state(&self, abs: u64) -> bool {
        self.p(&self.state_rel(abs)).is_file()
    }

    pub fn remove_state(&self, abs: u64) {
        let _ = fs::remove_file(self.p(&self.state_rel(abs)));
    }

    pub fn write_index(&self, idx: &HistoryIndex) -> CmdResult<()> {
        let bytes = serde_json::to_vec(idx).map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(&self.p(&self.index_rel()), &bytes)
    }

    pub fn read_index(&self) -> CmdResult<Option<HistoryIndex>> {
        let rel = self.index_rel();
        if !self.p(&rel).is_file() {
            return Ok(None);
        }
        let bytes = self.read_bytes(&rel)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| Self::corrupt(&rel, e))
    }

    // ---- versions --------------------------------------------------------

    pub fn list_versions(&self) -> CmdResult<Vec<VersionInfo>> {
        let path = self.p("versions/index.json");
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let bytes = self.read_bytes("versions/index.json")?;
        serde_json::from_slice(&bytes).map_err(|e| Self::corrupt("versions/index.json", e))
    }

    fn write_versions(&self, v: &[VersionInfo]) -> CmdResult<()> {
        let bytes = serde_json::to_vec_pretty(v).map_err(|e| CmdError::io(e.to_string()))?;
        atomic_write(&self.p("versions/index.json"), &bytes)
    }

    /// Writes the song as a new version and adds it to the list. Its number is `number` (from
    /// `Manifest::next_version`), or the one after the highest in the list when another process
    /// took that number first (PRJ-023).
    pub fn add_version(
        &self,
        number: u64,
        name: &str,
        kind: VersionKind,
        p: &Project,
    ) -> CmdResult<VersionInfo> {
        let _guard = self.lock_versions()?;
        let mut all = self.list_versions()?;
        let number = all
            .iter()
            .filter_map(|v| v.id.strip_prefix('v')?.parse::<u64>().ok())
            .fold(number, |n, used| n.max(used + 1));
        let info = VersionInfo {
            id: format!("v{number}"),
            name: name.to_string(),
            kind,
            created_ms: now_ms(),
        };
        self.write_project_file(&format!("versions/{}.json", info.id), p, true)?;
        all.push(info.clone());
        self.write_versions(&all)?;
        Ok(info)
    }

    /// Held while the version list or the manifest is read and written back, so that two
    /// processes do not write over each other's change.
    fn lock_versions(&self) -> CmdResult<File> {
        let path = self.p("versions/index.lock");
        let f = open_lock(&path)?;
        f.lock()?;
        Ok(f)
    }

    pub fn read_version(&self, id: &str) -> CmdResult<Project> {
        check_version_id(id)?;
        let all = self.list_versions()?;
        let Some(info) = all.iter().find(|v| v.id == id) else {
            return Err(CmdError::not_found(format!(
                "バージョン {id} が見つかりません"
            )));
        };
        self.read_listed_version(info)
    }

    /// A version from [`Store::list_versions`], read without reading the list again (REC-026:
    /// the cleanup reads every version of the list it read once).
    pub fn read_listed_version(&self, info: &VersionInfo) -> CmdResult<Project> {
        check_version_id(&info.id)?;
        Ok(self
            .read_project_file(&format!("versions/{}.json", info.id))?
            .0)
    }

    /// Keeps only the newest `keep` automatic versions.
    pub fn prune_auto(&self, keep: usize) -> CmdResult<()> {
        let _guard = self.lock_versions()?;
        let all = self.list_versions()?;
        let autos: Vec<&VersionInfo> = all.iter().filter(|v| v.kind == VersionKind::Auto).collect();
        if autos.len() <= keep {
            return Ok(());
        }
        let drop: Vec<String> = autos[..autos.len() - keep]
            .iter()
            .map(|v| v.id.clone())
            .collect();
        let rest: Vec<VersionInfo> = all.into_iter().filter(|v| !drop.contains(&v.id)).collect();
        self.write_versions(&rest)?;
        for id in drop {
            let _ = fs::remove_file(self.p(&format!("versions/{id}.json")));
        }
        Ok(())
    }

    /// Copies every version into another song folder.
    pub fn copy_versions_to(&self, other: &Store) -> CmdResult<()> {
        let all = self.list_versions()?;
        for v in &all {
            let rel = format!("versions/{}.json", v.id);
            fs::create_dir_all(other.p("versions"))?;
            crate::session::saving::copy_file_atomic(&self.p(&rel), &other.p(&rel))?;
        }
        if !all.is_empty() {
            other.write_versions(&all)?;
        }
        Ok(())
    }

    pub fn new_manifest(name: &str, saved_step: u64, ai: AiPermission) -> Manifest {
        Manifest {
            format: FORMAT.to_string(),
            format_version: FORMAT_VERSION,
            name: name.to_string(),
            saved_at_ms: now_ms(),
            saved_step,
            ai_permission: ai,
            next_version: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A song folder holding `project.json` for `Project::new(name)`, and that file's hash.
    fn song(name: &str) -> (tempfile::TempDir, Store, String) {
        let tmp = tempfile::Builder::new()
            .prefix("xirius-store-test-")
            .tempdir()
            .unwrap();
        let store = Store::new(tmp.path());
        store.write_project(&Project::new(name)).unwrap();
        let hash = store.project_hash().unwrap().unwrap();
        (tmp, store, hash)
    }

    fn on_disk(store: &Store) -> Vec<u8> {
        fs::read(store.dir.join("project.json")).unwrap()
    }

    /// Writes as asked, except the `bad`th write (from 1), which loses its last byte.
    fn flaky(bad: usize) -> impl FnMut(&Path, &[u8]) -> CmdResult<()> {
        let mut n = 0;
        move |path, bytes| {
            n += 1;
            let keep = if n == bad {
                bytes.len() - 1
            } else {
                bytes.len()
            };
            atomic_write(path, &bytes[..keep])
        }
    }

    #[test]
    fn a_backup_that_reads_back_wrong_stops_the_write_before_it_starts() {
        let (_tmp, store, hash) = song("a");
        let old = on_disk(&store);
        let e = store
            .overwrite_project_with(&Project::new("b"), &hash, &mut flaky(1))
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::Io);
        assert!(e.message.contains("控え"), "{}", e.message);
        assert_eq!(on_disk(&store), old, "the song is as it was");
        assert!(store.backups().is_empty(), "the bad copy is not kept");
    }

    #[test]
    fn a_song_file_that_reads_back_wrong_is_put_back_from_the_backup() {
        let (_tmp, store, hash) = song("a");
        let old = on_disk(&store);
        let e = store
            .overwrite_project_with(&Project::new("b"), &hash, &mut flaky(2))
            .unwrap_err();
        assert!(e.message.contains("元に戻しました"), "{}", e.message);
        assert_eq!(on_disk(&store), old, "the old file is back");
        let backups = store.backups();
        assert_eq!(backups.len(), 1, "the copy stays");
        assert_eq!(fs::read(store.dir.join(&backups[0])).unwrap(), old);
    }

    #[test]
    fn the_last_twenty_backups_are_kept() {
        let (_tmp, store, mut hash) = song("a");
        let mut first = Vec::new();
        for i in 0..BACKUP_KEEP + 3 {
            let done = store
                .overwrite_project(&Project::new(format!("v{i}")), &hash)
                .unwrap();
            if i < 3 {
                first.push(done.backup.clone());
            }
            assert_eq!(Some(done.after.clone()), store.project_hash().unwrap());
            hash = done.after;
        }
        let kept = store.backups();
        assert_eq!(kept.len(), BACKUP_KEEP);
        for gone in &first {
            assert!(!kept.contains(gone), "{gone} should have gone");
        }
    }
}
