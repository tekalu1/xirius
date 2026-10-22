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
//! `xirius play` and `xirius mcp` run it when they close a saved song; a dry run lists the
//! files without moving them.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde_json::json;
use xirius_core::command::Output;
use xirius_core::model::{ClipBody, Project};
use xirius_core::store::Store;
use xirius_core::{CmdError, CmdResult};

use crate::record::take_folders;

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

/// The recordings in the song's take folders (the media folder's `Recordings/` and the older
/// `Recordings/` of `dir`) that neither `project` nor any state saved in `dir` names. An error
/// when a state cannot be read: then nothing is known to be unused.
pub fn unused(project: &Project, dir: &Path) -> CmdResult<Unused> {
    let used = names_in_use(project, dir)?;
    let mut out = Unused::default();
    let mut found = Vec::new();
    for root in take_folders(&project.media, dir) {
        if root.is_dir() {
            wavs(&root, &mut found)?;
        }
    }
    found.sort();
    found.dedup();
    for path in found {
        if used.contains(&key(dir, &path)) {
            out.kept += 1;
            continue;
        }
        out.bytes += std::fs::metadata(&path).map_or(0, |m| m.len());
        out.files.push(shown(dir, &path));
    }
    Ok(out)
}

/// `project.cleanup` on the song in `dir`.
pub fn run(project: &Project, dir: &Path, dry: bool, trash: &Trash) -> CmdResult<Output> {
    let found = unused(project, dir)?;
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
    for f in &found.files {
        match trash(&dir.join(f)) {
            Ok(()) => moved.push(f.clone()),
            Err(e) => failed.push(e),
        }
    }
    let data = json!({
        "files": found.files,
        "bytes": found.bytes,
        "kept": found.kept,
        "moved": moved,
        "errors": failed,
    });
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

/// Every audio file the song, its histories, `project.json` and its versions name, as [`key`]s.
fn names_in_use(project: &Project, dir: &Path) -> CmdResult<BTreeSet<String>> {
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
    let stop = |e: CmdError| {
        CmdError::new(
            e.code,
            format!(
                "{}。使用中のファイルを確認できないため、何も移動しません",
                e.message
            ),
        )
    };
    // The other processes' histories too (PRJ-023).
    for lane in store.lanes() {
        let history = Store::in_lane(dir, lane);
        if let Some(idx) = history.read_index().map_err(stop)? {
            let last = idx.first + idx.steps.len() as u64;
            for abs in idx.first..=last {
                add(&history.read_state(abs).map_err(stop)?);
            }
        }
    }
    if dir.join("project.json").is_file() {
        add(&store.read_project().map_err(stop)?);
    }
    for v in store.list_versions().map_err(stop)? {
        add(&store.read_version(&v.id).map_err(stop)?);
    }
    Ok(used)
}

/// The WAV files under `root`, in its subfolders too.
fn wavs(root: &Path, out: &mut Vec<PathBuf>) -> CmdResult<()> {
    let list = std::fs::read_dir(root)
        .map_err(|e| CmdError::io(format!("{} を読み込めません: {e}", root.display())))?;
    for entry in list {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            wavs(&path, out)?;
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        {
            out.push(path);
        }
    }
    Ok(())
}

/// A path as the clips name it, made comparable: absolute under `dir`, `.` and `..` folded,
/// `/` between the parts, and in one case where the file system ignores case.
fn key(dir: &Path, file: &Path) -> String {
    let full = if file.is_absolute() {
        file.to_path_buf()
    } else {
        dir.join(file)
    };
    let mut parts: Vec<String> = Vec::new();
    for c in full.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop();
            }
            Component::RootDir => {}
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    let joined = parts.join("/");
    if cfg!(any(windows, target_os = "macos")) {
        joined.to_lowercase()
    } else {
        joined
    }
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
}
