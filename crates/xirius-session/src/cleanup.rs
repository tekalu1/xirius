//! `project.cleanup`: the recordings nothing uses any more go to the recycle bin (REC-016,
//! CTX-009).
//!
//! An undone recording leaves its WAV in `Recordings/`, and a redo brings it back, so the file
//! stays while any state the song can come back to names it: the song as it is, every state of
//! the history (undone ones included, as redo reaches them), the saved `project.json` and every
//! version, and the histories of other processes that had the song open (PRJ-023). A file is
//! put in the recycle bin only when none of them does. What cannot be read stops the cleanup
//! before anything is moved, as does an open recording, an AI work copy (whose states live only
//! in memory) or another process that has the song open now.
//!
//! Nor does a file go that another song open in this process has named, in any state it got to
//! (a recording of the closed song placed in the next one, REC-026), by its path or another way
//! of writing it (a link to a folder, a drive letter for a share). Each file is checked in memory
//! while no command runs ([`in_use`]), put aside in the folder of this process next to it
//! (`.xirius-moving/<process>/`, a rename), checked on the disk (another process that opened the
//! song or wrote to its history), checked in memory again, and only then moved to the recycle
//! bin: a command that named it meanwhile has it put back, and none waits for the disk. No file is
//! written over as one moves: one that took its name meanwhile (a take) keeps it, and the file
//! goes back as `<name> (2).wav`; a take is not given a name that is aside.
//!
//! A process holds its folder (a lock on `held.lock`) while a file is in it, and the OS lets it go
//! when the process dies: what is in a folder held is left to its process, and what one that went
//! down left is put back where it was the next time the files are cleaned up. One in a folder with
//! no `held.lock` was taken back from the recycle bin, which gives it back where it took it from:
//! it is put back where it was too, and is not moved that time.
//!
//! `xirius play` and `xirius mcp` run it when they close a saved song; a dry run lists the
//! files without moving them. The app runs it as a [`Closed`] on a thread of its own once the
//! next song is open (REC-026).
//!
//! It reads only what it has to, as every file may take seconds on a cloud drive (REC-026): a
//! song with no recording in its take folders reads nothing of its history, and the states are
//! read for the files of their clips only, not the blobs they point to.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::json;
use xirius_core::command::Output;
use xirius_core::in_use;
use xirius_core::model::{ClipBody, Project};
use xirius_core::session::{lock_files, others_open_among};
use xirius_core::store::{self, Stamp, Store};
use xirius_core::{CmdError, CmdResult};

use crate::record::take_folders;

/// Why it stops while another process has the song open.
pub const OTHERS_OPEN: &str =
    "ほかのプロセスがこの曲を開いている間は片付けできません（閉じてから実行してください）";
/// Why it stops while an AI's work copy is there.
pub const WORK_COPY: &str =
    "AI の作業コピーがある間は片付けできません（取り込むか破棄してから実行してください）";
/// What a [`Closed`] that was stopped gives.
pub const STOPPED: &str = "片付けを止めました";

/// Moves a file to the recycle bin ([`crate::os::trash`], or another place in tests).
pub type Trash = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// `XIRIUS_TRASH_DIR`: where the programs (`xirius`, the app) move files instead of the
/// recycle bin when `project.cleanup` or an export that replaces a file sends them there (the
/// tests, and scripts that keep the files themselves). A file on another drive is copied and
/// then removed.
pub fn trash_from_env() -> Option<Trash> {
    let dir = PathBuf::from(std::env::var_os("XIRIUS_TRASH_DIR")?);
    Some(Arc::new(move |path: &Path| {
        let fail = |e: std::io::Error| {
            format!(
                "{} を {} に移動できません: {e}",
                path.display(),
                dir.display()
            )
        };
        std::fs::create_dir_all(&dir).map_err(fail)?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut to = dir.join(&name);
        let mut n = 1;
        while to.exists() {
            n += 1;
            to = dir.join(format!("{n}-{name}"));
        }
        if std::fs::rename(path, &to).is_ok() {
            return Ok(());
        }
        std::fs::copy(path, &to).map_err(fail)?;
        std::fs::remove_file(path).map_err(fail)
    }))
}

/// The recordings no state of the song names.
#[derive(Debug, Default)]
pub struct Unused {
    /// Paths relative to the song folder, with `/`, in order.
    pub files: Vec<String>,
    pub bytes: u64,
    /// Recordings some state still names.
    pub kept: usize,
}

/// Whose cleanup it is.
#[derive(Clone, Copy, Debug, Default)]
pub struct Own {
    /// The session that has the song open ([`xirius_core::session::Session::uses_id`]): the
    /// files it named do not count, as its history on the disk says which it can come back to.
    pub uses: Option<u64>,
    /// The history lane that session holds in the song folder. None for a song closed, which
    /// another process may open while the files move: it is looked for before each one goes.
    pub lane: Option<u32>,
}

/// The recordings in the song's take folders (the media folder's `Recordings/` and the older
/// `Recordings/` of `dir`) that neither `project` nor any state saved in `dir` names, nor another
/// song open here. An error when a state cannot be read: then nothing is known to be unused.
/// Nothing is moved: a file left aside ([`MOVING`]) is listed where it is.
pub fn unused(project: &Project, dir: &Path, own: Own) -> CmdResult<Unused> {
    Ok(unused_until(project, dir, own, false, &AtomicBool::new(false))?.0)
}

/// [`unused`], with what the states name and what told, before they were read, that another
/// process had not changed them. With `back`, a file left aside is put back where it was first.
fn unused_until(
    project: &Project,
    dir: &Path,
    own: Own,
    back: bool,
    stop: &AtomicBool,
) -> CmdResult<(Unused, BTreeSet<String>, Watch)> {
    // The files first: one made after this is not looked at, and a song with none has nothing
    // whose names the history has to be read for.
    let mut list = Listing {
        back,
        stop,
        found: Vec::new(),
        restored: Vec::new(),
    };
    for root in take_folders(&project.media, dir) {
        if root.is_dir() {
            wavs(&root, &mut list)?;
        }
    }
    // Taken back from the recycle bin: not moved this time.
    let restored: HashSet<PathBuf> = list.restored.into_iter().collect();
    let mut found = list.found;
    found.retain(|p| !restored.contains(p));
    let mut out = Unused {
        kept: restored.len(),
        ..Unused::default()
    };
    if found.is_empty() {
        return Ok((out, BTreeSet::new(), Watch::default()));
    }
    found.sort();
    found.dedup();
    let watch = Watch::new(dir);
    let used = names_in_use(project, dir, stop)?;
    for path in found {
        let k = key(dir, &home_of(&path));
        if used.contains(&k) || in_use::named(&k, own.uses) {
            out.kept += 1;
            continue;
        }
        out.bytes += std::fs::metadata(&path).map_or(0, |m| m.len());
        out.files.push(shown(dir, &path));
    }
    Ok((out, used, watch))
}

/// What tells that another process opened the song or wrote to it: its lock files, and the
/// stamps of the files that list its states (each lane's history, `project.json`, the versions).
/// A lane made after this changes the folder `history/lanes`, whose stamp is kept too.
#[derive(Default)]
struct Watch {
    lanes_folder: Option<Stamp>,
    lanes: Vec<u32>,
    locks: Vec<u32>,
    files: Vec<Option<Stamp>>,
}

impl Watch {
    fn new(dir: &Path) -> Watch {
        let lanes = Store::new(dir).lanes();
        Watch {
            lanes_folder: stamp(&dir.join(store::LANES)),
            files: Watch::files(dir, &lanes),
            lanes,
            locks: lock_files(dir),
        }
    }

    fn files(dir: &Path, lanes: &[u32]) -> Vec<Option<Stamp>> {
        let lists = lanes
            .iter()
            .map(|&k| {
                dir.join(Store::in_lane(dir, k).history_rel())
                    .join("index.json")
            })
            .chain([dir.join("project.json"), dir.join("versions/index.json")]);
        lists.map(|p| stamp(&p)).collect()
    }

    /// Whether a list may name more now than when this was made.
    fn changed(&self, dir: &Path) -> bool {
        stamp(&dir.join(store::LANES)) != self.lanes_folder
            || Watch::files(dir, &self.lanes) != self.files
    }
}

fn stamp(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(|m| Stamp {
        len: m.len(),
        modified: m.modified().ok(),
    })
}

/// `project.cleanup` on the song in `dir`.
pub fn run(project: &Project, dir: &Path, dry: bool, trash: &Trash, own: Own) -> CmdResult<Output> {
    run_until(project, dir, dry, trash, own, &AtomicBool::new(false))
}

/// The cleanup of a song that is closed (REC-016), run once it is: on a thread of its own while
/// the next song opens (REC-026). It holds the song as it was when it closed.
pub struct Closed {
    /// None for a song closed before, read when this runs ([`Closed::saved`]).
    project: Option<Arc<Project>>,
    dir: PathBuf,
    trash: Trash,
    own: Own,
}

impl Closed {
    /// `project` is the closed song as it was, saved in `dir`; nothing may be recording into it
    /// and no AI's work copy may be open on it (the closing session knows, this does not).
    pub fn new(project: Arc<Project>, dir: PathBuf, trash: Trash) -> Closed {
        Closed {
            project: Some(project),
            dir,
            trash,
            own: Own::default(),
        }
    }

    /// The song in `dir`, closed before (a cleanup that was stopped), as it is saved: read when
    /// this runs, off the thread that makes it. Nothing is done if `dir` holds no song any more.
    pub fn saved(dir: PathBuf, trash: Trash) -> Closed {
        Closed {
            project: None,
            dir,
            trash,
            own: Own::default(),
        }
    }

    /// The session that closed the song is `uses` ([`xirius_core::session::Session::uses_id`]),
    /// which may still be there when this runs: what it named does not count.
    pub fn closed_by(mut self, uses: u64) -> Closed {
        self.own.uses = Some(uses);
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `project.cleanup` on the closed song. Nothing moves while a process has it open (this
    /// one too, if it opened it again), nor once `stop` is set: then it ends with [`STOPPED`]
    /// unless a file had moved.
    pub fn run(&self, stop: &AtomicBool) -> CmdResult<Output> {
        // Stopped before it began: the song may be open again here.
        if stop.load(Ordering::Acquire) {
            return Err(CmdError::conflict(STOPPED));
        }
        if others_open_among(&self.dir, None, &lock_files(&self.dir)) {
            return Err(CmdError::conflict(OTHERS_OPEN));
        }
        let project = match &self.project {
            Some(p) => p.clone(),
            None if !self.dir.join("project.json").is_file() => {
                return Ok(Output::with(
                    format!("{} に曲はありません", self.dir.display()),
                    json!({ "files": [], "bytes": 0, "kept": 0, "moved": [] }),
                ));
            }
            None => Arc::new(Store::new(&self.dir).read_project()?),
        };
        run_until(&project, &self.dir, false, &self.trash, self.own, stop)
    }
}

fn run_until(
    project: &Project,
    dir: &Path,
    dry: bool,
    trash: &Trash,
    own: Own,
    stop: &AtomicBool,
) -> CmdResult<Output> {
    let (found, mut used, mut watch) = unused_until(project, dir, own, !dry, stop)?;
    let n = found.files.len();
    let size = megabytes(found.bytes);
    if n == 0 {
        return Ok(Output::with(
            format!(
                "使われていない録音のファイルはありません（使用中 {} 個）",
                found.kept
            ),
            json!({ "files": [], "bytes": 0, "kept": found.kept, "moved": [] }),
        ));
    }
    if dry {
        return Ok(Output::with(
            format!("使われていない録音のファイル {n} 個（{size}）をごみ箱に移動します"),
            json!({ "files": found.files, "bytes": found.bytes, "kept": found.kept, "moved": [] }),
        ));
    }
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    let mut kept = found.kept;
    let mut open = false;
    for f in &found.files {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let path = dir.join(f);
        let home = home_of(&path);
        let k = key(dir, &home);
        // In memory, while no command runs: whether a song open here named it.
        let alike = match look(dir, &k, &home, own, stop) {
            Look::Free(alike) => alike,
            Look::Named => {
                kept += 1;
                continue;
            }
            Look::Open => {
                open = true;
                break;
            }
            Look::Stopped => break,
        };
        // On the disk, while commands run: the same file written another way.
        if same_file(&path, &alike) {
            kept += 1;
            continue;
        }
        let aside = match Aside::put(&path) {
            Ok(aside) => aside,
            Err(e) => {
                failed.push(e);
                continue;
            }
        };
        // Another process that wrote to the song or opened it before the file was put aside.
        if watch.changed(dir) {
            watch = Watch::new(dir);
            match names_in_use(project, dir, stop) {
                Ok(now) => used = now,
                Err(e) => {
                    aside.back(&mut failed);
                    if !stop.load(Ordering::Acquire) {
                        failed.push(e.message);
                    }
                    break;
                }
            }
        }
        if used.contains(&k) {
            aside.back(&mut failed);
            kept += 1;
            continue;
        }
        if others_open_among(dir, own.lane, &watch.locks) {
            aside.back(&mut failed);
            open = true;
            break;
        }
        // In memory again: a command that named it before it was put aside is over now.
        match look(dir, &k, &home, own, stop) {
            Look::Free(now) if now == alike => {}
            Look::Free(_) | Look::Named => {
                aside.back(&mut failed);
                kept += 1;
                continue;
            }
            Look::Open => {
                aside.back(&mut failed);
                open = true;
                break;
            }
            Look::Stopped => {
                aside.back(&mut failed);
                break;
            }
        }
        match trash(&aside.path) {
            Ok(()) => moved.push(f.clone()),
            Err(e) => {
                aside.back(&mut failed);
                failed.push(e);
            }
        }
    }
    let data = json!({
        "files": found.files,
        "bytes": found.bytes,
        "kept": kept,
        "moved": moved,
        "errors": failed,
    });
    if moved.is_empty() && failed.is_empty() {
        if open {
            return Err(CmdError::conflict(OTHERS_OPEN));
        }
        if stop.load(Ordering::Acquire) {
            return Err(CmdError::conflict(STOPPED));
        }
        return Ok(Output::with(
            format!("使われていない録音のファイルはありません（使用中 {kept} 個）"),
            json!({ "files": [], "bytes": 0, "kept": kept, "moved": [] }),
        ));
    }
    if moved.is_empty() {
        return Err(CmdError::conflict(format!(
            "使われていない録音のファイルをごみ箱に移動できません: {}",
            failed.join("・")
        )));
    }
    let mut message = format!(
        "使われていない録音のファイル {} 個（{size}）をごみ箱に移動しました",
        moved.len()
    );
    if !failed.is_empty() {
        message.push_str(&format!(
            "（{} 個は移動できません: {}）",
            failed.len(),
            failed.join("・")
        ));
    }
    Ok(Output::with(message, data))
}

/// What [`look`] found of a file.
enum Look {
    /// No song open here named it, but maybe these (the same file written another way?).
    Free(Vec<PathBuf>),
    Named,
    /// The song is open here again.
    Open,
    Stopped,
}

/// Whether a song open here but `own`'s named the file `path` (of [`key`] `k`), looked at in
/// memory while no command runs.
fn look(dir: &Path, k: &str, path: &Path, own: Own, stop: &AtomicBool) -> Look {
    let Some(_alone) = in_use::alone() else {
        return Look::Open;
    };
    // Told while it waited: the song may be opening again here.
    if stop.load(Ordering::Acquire) {
        Look::Stopped
    } else if in_use::opened(dir, own.uses) {
        Look::Open
    } else if in_use::named(k, own.uses) {
        Look::Named
    } else {
        Look::Free(in_use::named_alike(k, path, own.uses))
    }
}

/// Whether one of `alike` is the file `path`, as the disk tells (a link to its folder, another
/// drive for the same share). Kept when that cannot be told.
fn same_file(path: &Path, alike: &[PathBuf]) -> bool {
    if alike.is_empty() {
        return false;
    }
    let Ok(real) = std::fs::canonicalize(path) else {
        return true;
    };
    alike.iter().any(|p| match std::fs::canonicalize(p) {
        Ok(other) => other == real,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    })
}

/// Where a file goes aside before it moves to the recycle bin, next to it: a rename into the
/// folder of the process that moves it (`<pid>-<start>`), which keeps its name.
const MOVING: &str = ".xirius-moving";
/// In each process's folder in [`MOVING`], locked while the process has a file there.
const HELD: &str = "held.lock";

/// This process's folder in [`MOVING`]: no other process, nor one before with its id, has it.
fn own_folder() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        let start = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        format!("{}-{start:x}", std::process::id())
    })
}

/// What this process has aside now.
struct Asides {
    /// The files: the listing of another cleanup leaves them there.
    files: BTreeSet<PathBuf>,
    /// The folders of this process that hold them, each with its [`HELD`] locked (none where the
    /// drive cannot lock a file).
    held: BTreeMap<PathBuf, Option<File>>,
}

static ASIDE: Mutex<Asides> = Mutex::new(Asides {
    files: BTreeSet::new(),
    held: BTreeMap::new(),
});

fn aside() -> std::sync::MutexGuard<'static, Asides> {
    ASIDE.lock().unwrap_or_else(|e| e.into_inner())
}

impl Asides {
    /// Marks `path` aside, in `folder` (this process's), held from now on.
    fn reserve(&mut self, folder: &Path, path: &Path) -> std::io::Result<()> {
        if !self.held.contains_key(folder) {
            // Twice: another cleanup may remove the folder as it empties it.
            let mut tried = 0;
            let file = loop {
                tried += 1;
                let opened = std::fs::create_dir_all(folder).and_then(|()| {
                    std::fs::OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(folder.join(HELD))
                });
                match opened {
                    Ok(f) => break f,
                    Err(e) if tried < 2 && e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            };
            let held = file.try_lock().is_ok().then_some(file);
            self.held.insert(folder.to_path_buf(), held);
        }
        self.files.insert(path.to_path_buf());
        Ok(())
    }

    /// `path` is no longer aside: the folder of this process it was in is let go once it holds
    /// none, and the folders left empty go.
    fn release(&mut self, path: &Path) {
        self.files.remove(path);
        let folder = self.held.keys().find(|f| path.starts_with(f)).cloned();
        if let Some(folder) = folder
            && !self.files.iter().any(|p| p.starts_with(&folder))
        {
            let _ = std::fs::remove_file(folder.join(HELD));
            self.held.remove(&folder);
        }
        let mut folder = path.parent();
        while let Some(f) = folder
            && f.components().any(|c| c.as_os_str() == MOVING)
            && std::fs::remove_dir(f).is_ok()
        {
            folder = f.parent();
        }
    }
}

/// A file put aside ([`MOVING`]); it is no longer marked once this goes.
struct Aside {
    from: PathBuf,
    path: PathBuf,
}

impl Aside {
    fn put(from: &Path) -> Result<Aside, String> {
        let fail = |e: std::io::Error| format!("{} を移動できません: {e}", from.display());
        // One left aside before and listed where it is: it is aside already.
        if from.components().any(|c| c.as_os_str() == MOVING) {
            aside().files.insert(from.to_path_buf());
            return Ok(Aside {
                from: from.to_path_buf(),
                path: from.to_path_buf(),
            });
        }
        let parent = from.parent().unwrap_or(Path::new(""));
        let name = from.file_name().unwrap_or_default();
        let folder = parent.join(MOVING).join(own_folder());
        let aside_of = |path: PathBuf, before: Option<&Path>| {
            let mut a = aside();
            if let Some(p) = before {
                a.files.remove(p);
            }
            a.reserve(&folder, &path).map(|()| path)
        };
        let path = aside_of(folder.join(name), None).map_err(fail)?;
        let mut aside = Aside {
            from: from.to_path_buf(),
            path,
        };
        let mut n = 1;
        let mut tried = 0;
        loop {
            let made = std::fs::create_dir_all(aside.path.parent().unwrap_or(&folder));
            match made.and_then(|()| move_new(from, &aside.path)) {
                Ok(()) => return Ok(aside),
                // One of its name in the folder already (it was taken back from the recycle bin).
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    n += 1;
                    let next = folder.join(n.to_string()).join(name);
                    aside.path = aside_of(next, Some(&aside.path)).map_err(fail)?;
                }
                // Another cleanup removed the folder as it emptied it.
                Err(e) if tried < 1 && e.kind() == std::io::ErrorKind::NotFound => tried += 1,
                Err(e) => return Err(fail(e)),
            }
        }
    }

    /// Puts the file back where it was, or by a name of its own next to it if another file has
    /// that name now (a take that began meanwhile): it writes over none.
    fn back(&self, failed: &mut Vec<String>) {
        if self.path != self.from
            && let Err(e) = put_back(&self.path, &self.from)
        {
            failed.push(format!(
                "{} を {} に戻せません: {e}",
                self.path.display(),
                self.from.display()
            ));
        }
    }
}

impl Drop for Aside {
    fn drop(&mut self) {
        aside().release(&self.path);
    }
}

/// Moves `from` to `to`, or to `<name> (2).wav` (`(3)`…) next to it if there is a file `to`:
/// where it went. It writes over no file, not one made with that name just before.
fn put_back(from: &Path, to: &Path) -> std::io::Result<PathBuf> {
    let mut at = to.to_path_buf();
    let mut n = 1;
    loop {
        match move_new(from, &at) {
            Ok(()) => return Ok(at),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => {
                n += 1;
                at = numbered(to, n);
            }
            Err(e) => return Err(e),
        }
    }
}

/// `<dir>/<stem> (n).<ext>` for `path` `<dir>/<stem>.<ext>`.
fn numbered(path: &Path, n: u32) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = match path.extension() {
        Some(ext) => format!("{stem} ({n}).{}", ext.to_string_lossy()),
        None => format!("{stem} ({n})"),
    };
    path.with_file_name(name)
}

/// Renames `from` to `to` unless there is a file `to` ([`crate::os::rename_new`]).
fn move_new(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    MOVING_TO.with_borrow(|f| {
        if let Some(f) = f {
            f(to)
        }
    });
    crate::os::rename_new(from, to)
}

/// Run just before a file is moved to the path it is given (a take that begins then).
#[cfg(test)]
type MovingTo = Option<Box<dyn Fn(&Path)>>;

#[cfg(test)]
thread_local! {
    static MOVING_TO: std::cell::RefCell<MovingTo> = const { std::cell::RefCell::new(None) };
}

/// Where a file put aside ([`MOVING`]) was: the folder that holds [`MOVING`], with its name.
fn home_of(path: &Path) -> PathBuf {
    let mut home = PathBuf::new();
    for c in path.components() {
        if c.as_os_str() == MOVING {
            return home.join(path.file_name().unwrap_or_default());
        }
        home.push(c);
    }
    path.to_path_buf()
}

/// The names of the files aside in the take folder `folder` ([`MOVING`]), in lower case: a take
/// is given none of them, so each can go back by its name.
pub(crate) fn aside_names(folder: &Path) -> HashSet<String> {
    fn walk(dir: &Path, out: &mut HashSet<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|k| k.is_dir()) {
                walk(&e.path(), out);
            } else {
                out.insert(e.file_name().to_string_lossy().to_lowercase());
            }
        }
    }
    let mut out = HashSet::new();
    walk(&folder.join(MOVING), &mut out);
    out
}

/// Every audio file the song, its histories, `project.json` and its versions name, as [`key`]s.
/// Each state is read once, without the blobs it points to; each version once, from the list
/// read once. Ends with [`STOPPED`] once `stop` is set.
fn names_in_use(project: &Project, dir: &Path, stop: &AtomicBool) -> CmdResult<BTreeSet<String>> {
    let mut used = BTreeSet::new();
    let mut add = |p: &Project| {
        for t in p.tracks.values() {
            for c in &t.clips {
                if let ClipBody::Audio { file, .. } = &c.body {
                    used.insert(key(dir, Path::new(file)));
                }
            }
        }
    };
    add(project);
    let store = Store::new(dir);
    let fail = |e: CmdError| {
        CmdError::new(
            e.code,
            format!(
                "{}。使用中のファイルを確認できないため、何も移動しません",
                e.message
            ),
        )
    };
    let stopped = || {
        stop.load(Ordering::Acquire)
            .then(|| CmdError::conflict(STOPPED))
            .map_or(Ok(()), Err)
    };
    // The other processes' histories too (PRJ-023).
    for lane in store.lanes() {
        let history = Store::in_lane(dir, lane);
        if let Some(idx) = history.read_index().map_err(fail)? {
            let last = idx.first + idx.steps.len() as u64;
            for abs in idx.first..=last {
                stopped()?;
                let state = history.read_state_unresolved(abs).map_err(fail)?;
                if names_unread(&state) {
                    add(&history.read_state(abs).map_err(fail)?);
                } else {
                    add(&state);
                }
            }
        }
    }
    stopped()?;
    if dir.join("project.json").is_file() {
        add(&store.read_project().map_err(fail)?);
    }
    for v in store.list_versions().map_err(fail)? {
        stopped()?;
        add(&store.read_listed_version(&v).map_err(fail)?);
    }
    Ok(used)
}

/// Whether a clip of `p` names a file held in a blob that was not read.
fn names_unread(p: &Project) -> bool {
    p.tracks
        .values()
        .flat_map(|t| &t.clips)
        .any(|c| matches!(&c.body, ClipBody::Audio { file, .. } if file == store::UNREAD_BLOB))
}

/// The files [`wavs`] found.
struct Listing<'a> {
    /// Whether a file left aside goes back where it was ([`MOVING`]).
    back: bool,
    stop: &'a AtomicBool,
    found: Vec<PathBuf>,
    /// The files taken back from the recycle bin, where they are now.
    restored: Vec<PathBuf>,
}

impl Listing<'_> {
    fn stopped(&self) -> CmdResult<()> {
        if self.stop.load(Ordering::Acquire) {
            Err(CmdError::conflict(STOPPED))
        } else {
            Ok(())
        }
    }

    /// `path`, a file left aside by a process that went down: back where it was, or listed where
    /// it is.
    fn left(&mut self, path: PathBuf) {
        if !self.back {
            self.found.push(path);
            return;
        }
        let at = put_back(&path, &home_of(&path));
        self.found.push(at.unwrap_or(path));
    }

    /// `path`, a file taken back from the recycle bin: back where it was.
    fn restore(&mut self, path: PathBuf) {
        if !self.back {
            self.restored.push(path);
            return;
        }
        let at = put_back(&path, &home_of(&path));
        self.restored.push(at.unwrap_or(path));
    }
}

/// The WAV files under `root`, in its subfolders too. Ends with [`STOPPED`] once `stop` is set:
/// each entry may take a while on a slow drive. Those left aside ([`MOVING`]) as [`left_aside`]
/// tells.
fn wavs(root: &Path, list: &mut Listing) -> CmdResult<()> {
    let entries = std::fs::read_dir(root)
        .map_err(|e| CmdError::io(format!("{} を読み込めません: {e}", root.display())))?;
    for entry in entries {
        list.stopped()?;
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() && entry.file_name() == MOVING {
            left_aside(&path, list)?;
        } else if kind.is_dir() {
            wavs(&path, list)?;
        } else if kind.is_file() && is_wav(&path) {
            list.found.push(path);
        }
    }
    Ok(())
}

fn is_wav(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// The files in `moving` (a [`MOVING`] folder) no process moves now: in a folder of this process
/// and not aside ([`Asides`]), or in the folder of a process that went down (its [`HELD`] not
/// locked), listed as [`Listing::left`]; in a folder with no [`HELD`], or none, taken back from
/// the recycle bin ([`Listing::restore`]). A folder another process holds is left to it.
fn left_aside(moving: &Path, list: &mut Listing) -> CmdResult<()> {
    let entries = std::fs::read_dir(moving)
        .map_err(|e| CmdError::io(format!("{} を読み込めません: {e}", moving.display())))?;
    for entry in entries {
        list.stopped()?;
        let entry = entry?;
        let folder = entry.path();
        if !entry.file_type()?.is_dir() {
            if is_wav(&folder) {
                list.restore(folder);
            }
            continue;
        }
        let mut plain = Listing {
            back: false,
            stop: list.stop,
            found: Vec::new(),
            restored: Vec::new(),
        };
        if entry.file_name() == own_folder() {
            wavs(&folder, &mut plain)?;
            let left: Vec<PathBuf> = {
                let mine = aside();
                let found = plain.found.into_iter();
                found.filter(|p| !mine.files.contains(p)).collect()
            };
            for p in left {
                list.left(p);
            }
            continue;
        }
        match holder(&folder) {
            Holder::Runs => {}
            Holder::Gone(lock) => {
                wavs(&folder, &mut plain)?;
                for p in plain.found {
                    list.left(p);
                }
                if list.back {
                    let _ = std::fs::remove_file(folder.join(HELD));
                    drop(lock);
                    remove_empty(&folder);
                }
            }
            Holder::None => {
                wavs(&folder, &mut plain)?;
                for p in plain.found {
                    list.restore(p);
                }
                if list.back {
                    remove_empty(&folder);
                }
            }
        }
    }
    if list.back {
        let _ = std::fs::remove_dir(moving);
    }
    Ok(())
}

/// Who has a process's folder in [`MOVING`].
enum Holder {
    /// The process, or one that cannot be told.
    Runs,
    /// No process: it went down. Held by this one while the lock is.
    Gone(File),
    /// No [`HELD`] there.
    None,
}

fn holder(folder: &Path) -> Holder {
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(folder.join(HELD));
    match opened {
        Ok(f) if f.try_lock().is_ok() => Holder::Gone(f),
        Ok(_) => Holder::Runs,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Holder::None,
        Err(_) => Holder::Runs,
    }
}

/// `folder` and the folders in it, those that are empty.
fn remove_empty(folder: &Path) {
    for e in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|k| k.is_dir()) {
            remove_empty(&e.path());
        }
    }
    let _ = std::fs::remove_dir(folder);
}

/// A path as the clips name it, made comparable ([`in_use::key`]).
fn key(dir: &Path, file: &Path) -> String {
    in_use::key(Some(dir), file)
}

/// `path` relative to the song folder, with `/`.
fn shown(dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(dir).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A size as the cleanup says it: `12.3 MB`.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_compare_however_the_clip_names_them() {
        let dir = Path::new("/songs/a");
        let k = key(dir, Path::new("Recordings/Vox-1.wav"));
        assert_eq!(k, key(dir, Path::new("./Recordings/x/../Vox-1.wav")));
        assert_eq!(k, key(dir, Path::new("/songs/a/Recordings/Vox-1.wav")));
        assert_ne!(k, key(dir, Path::new("Recordings/Vox-2.wav")));
        assert_eq!(
            shown(dir, &dir.join("Recordings").join("Vox-1.wav")),
            "Recordings/Vox-1.wav"
        );
    }

    #[test]
    fn the_listing_of_the_take_folders_stops_once_told() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-cleanup-wavs-")
            .tempdir()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/take.wav"), b"RIFF").unwrap();
        let list = |stop: &AtomicBool| {
            let mut list = Listing {
                back: true,
                stop,
                found: Vec::new(),
                restored: Vec::new(),
            };
            wavs(dir.path(), &mut list).map(|()| list.found)
        };
        assert_eq!(list(&AtomicBool::new(false)).unwrap().len(), 1);
        let e = list(&AtomicBool::new(true)).unwrap_err();
        assert!(e.message.contains(STOPPED), "{e:?}");
    }

    #[test]
    fn changing_alias_candidates_finishes_without_calling_the_bin() {
        use xirius_core::id::{ClipId, TrackId};
        use xirius_core::model::{Clip, Track, TrackKind};

        let dir = tempfile::tempdir().unwrap();
        let p = Project::new("closed");
        let rec = dir.path().join("Media/Recordings");
        std::fs::create_dir_all(&rec).unwrap();
        let file = rec.join("alias-change-only.wav");
        std::fs::write(&file, b"old take").unwrap();

        let mut other = Project::new("other");
        let t = TrackId(other.alloc());
        let mut track = Track::new(t, TrackKind::Audio, "other");
        track.clips.push(Clip {
            id: ClipId(other.alloc()),
            name: "alias".into(),
            color: None,
            start: 0,
            length: 960,
            muted: false,
            take: None,
            controllers: Vec::new(),
            body: ClipBody::Audio {
                file: dir
                    .path()
                    .join("other/alias-change-only.wav")
                    .to_string_lossy()
                    .into_owned(),
                offset: 0,
                gain_db: 0.0,
                process: Default::default(),
            },
        });
        other.tracks.insert(t, Arc::new(track));
        let uses = in_use::Uses::default();
        // Exactly between the two in-memory checks, another song notes a possible alias.
        // Cleanup must put the file back and finish, without ever entering the bin callback.
        MOVING_TO.set(Some(Box::new(move |_| uses.note(&other, None))));
        let bin: Trash = Arc::new(|_| panic!("a changing alias candidate must keep the file"));
        let got =
            Closed::new(Arc::new(p), dir.path().to_path_buf(), bin).run(&AtomicBool::new(false));
        MOVING_TO.set(None);
        let got = got.unwrap();
        assert_eq!(got.data["moved"], json!([]));
        assert_eq!(std::fs::read(file).unwrap(), b"old take");
    }

    #[test]
    fn a_file_put_back_replaces_none_that_took_its_name_meanwhile() {
        let dir = tempfile::Builder::new()
            .prefix("xirius-cleanup-back-")
            .tempdir()
            .unwrap();
        let home = dir.path().join("Vox-3.wav");
        std::fs::write(&home, b"old").unwrap();
        let aside = Aside::put(&home).unwrap();
        // A take begins with the name just as the file goes back.
        let take = home.clone();
        MOVING_TO.set(Some(Box::new(move |to: &Path| {
            if to == take {
                std::fs::write(&take, b"new take").unwrap();
            }
        })));
        let mut failed = Vec::new();
        aside.back(&mut failed);
        MOVING_TO.set(None);
        drop(aside);
        assert_eq!(std::fs::read(&home).unwrap(), b"new take");
        let old = dir.path().join("Vox-3 (2).wav");
        assert_eq!(
            std::fs::read(&old).ok().as_deref(),
            Some(&b"old"[..]),
            "{failed:?}"
        );
        assert!(failed.is_empty(), "{failed:?}");
    }
}
