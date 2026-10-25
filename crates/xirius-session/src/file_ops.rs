//! The browser's commands on files (BRW-013, BRW-014, CTX-059, BRW-004): `file.rename` renames
//! a file or a folder on the disk and points the song's clips and frozen tracks at the new
//! name, as one step of the history; `file.trash` moves a file or a folder to the OS's recycle
//! bin, but not one the song uses; `file.add.folder` makes a folder; `file.list` lists the files
//! the song uses, what made each (a recording, an import, a bounce, a freeze) and the recordings
//! no clip uses. A relative path is the song folder's. The song folder itself and what the song
//! keeps in it (its history, its versions, the peaks) are left alone.
//!
//! A rename of a file the song uses is in the history: the `clip.relink` step names the rename
//! (`disk`), so an undo takes the clips and the file back to the old name and a redo renames
//! both again; when the disk cannot follow, neither moves. A rename of a file the song does not
//! use leaves the history alone.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::model::{ClipBody, Media, Project};
use xirius_core::session::is_external;
use xirius_core::{Args, CmdError, CmdResult, ErrorCode, ExtContext, Output};

use crate::cleanup::Trash;
use crate::import::{Plan, disk_path, song_path};
use crate::reveal::target;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 4] = ["file.rename", "file.trash", "file.add.folder", "file.list"];

/// What the song keeps at the top of its folder.
const SONG_OWN: [&str; 6] = [
    "manifest.toml",
    "project.json",
    "view.json",
    "history",
    "versions",
    crate::peaks::FOLDER,
];

/// Characters a name of a file cannot have (on Windows; the others take the same).
const BAD: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

pub fn run(
    cx: &ExtContext<'_>,
    name: &str,
    args: &Args,
    dry: bool,
    trash: &Trash,
) -> CmdResult<Plan> {
    let dir = match cx.dir {
        Some(d) => Some(std::path::absolute(d).map_err(|e| CmdError::io(e.to_string()))?),
        None => None,
    };
    let dir = dir.as_deref();
    match name {
        "file.rename" => rename(cx.project, dir, args, dry),
        "file.trash" => to_trash(cx.project, dir, args, dry, trash),
        "file.add.folder" => add_folder(dir, args, dry),
        _ => Ok(Plan {
            output: list(cx.project, dir),
            edits: Vec::new(),
        }),
    }
}

/// A file the song uses, as its clips, frozen tracks and devices name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Use {
    /// As the song names it (relative to the song folder where possible).
    pub file: String,
    /// On the disk.
    pub path: PathBuf,
    pub clips: usize,
    pub frozen: usize,
    /// The devices that read it as a sample (ENG-077).
    pub devices: usize,
}

/// The files `p` uses, by the name the song gives them.
pub fn uses(p: &Project, dir: Option<&Path>) -> Vec<Use> {
    let mut out: BTreeMap<String, Use> = BTreeMap::new();
    let mut add = |file: &str, frozen: bool, device: bool| {
        let u = out.entry(file.to_string()).or_insert_with(|| Use {
            file: file.to_string(),
            path: disk_path(file, dir).unwrap_or_else(|_| PathBuf::from(file)),
            clips: 0,
            frozen: 0,
            devices: 0,
        });
        if frozen {
            u.frozen += 1;
        } else if device {
            u.devices += 1;
        } else {
            u.clips += 1;
        }
    };
    for t in p.tracks.values() {
        for c in &t.clips {
            if let ClipBody::Audio { file, .. } = &c.body {
                add(file, false, false);
            }
        }
        if let Some(z) = &t.frozen {
            add(&z.file, true, false);
        }
        for x in &t.inserts {
            for s in &x.samples {
                add(s, false, true);
            }
        }
    }
    out.into_values().collect()
}

/// A file of the song's pool (BRW-004): one the song uses, or a recording no clip uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub file: String,
    pub path: PathBuf,
    /// `record`, `import`, `bounce` or `freeze`.
    pub kind: &'static str,
    /// 0 when the file is not found.
    pub bytes: u64,
    pub exists: bool,
    /// Outside both the song's folder and its media folder.
    pub external: bool,
    pub clips: usize,
    pub frozen: usize,
    /// The devices that read it as a sample (ENG-077).
    pub devices: usize,
}

/// What made the file: the folder it is in says (`Recordings`, `Bounces`, `Freeze`), and a file
/// only a frozen track uses is a freeze; anything else was read in.
pub fn kind_of(u: &Use) -> &'static str {
    let folder = u
        .path
        .parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if same(&folder, Media::RECORDINGS) {
        "record"
    } else if same(&folder, Media::BOUNCES) {
        "bounce"
    } else if same(&folder, Media::FREEZE) || (u.clips + u.devices == 0 && u.frozen > 0) {
        "freeze"
    } else {
        "import"
    }
}

/// The song's pool: the files `p` uses and, in a saved song, the recordings of its take folders
/// that no clip uses (with no clip), in name order.
pub fn pool(p: &Project, dir: Option<&Path>) -> Vec<Entry> {
    let mut all = uses(p, dir);
    if let Some(dir) = dir {
        let mut takes = Vec::new();
        for root in crate::record::take_folders(&p.media, dir) {
            #[cfg(feature = "test-io")]
            let _ = xirius_core::io_test::before("pool_read_dir", &root);
            if let Ok(list) = std::fs::read_dir(&root) {
                takes.extend(list.filter_map(Result::ok).map(|e| e.path()).filter(|p| {
                    #[cfg(feature = "test-io")]
                    let _ = xirius_core::io_test::before("pool_file_type", p);
                    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
                }));
            }
        }
        for path in takes {
            if all
                .iter()
                .any(|u| below(&u.path, &path).is_some_and(|r| r.is_empty()))
            {
                continue;
            }
            all.push(Use {
                file: song_path(&path, Some(dir)),
                path,
                clips: 0,
                frozen: 0,
                devices: 0,
            });
        }
        all.sort_by(|a, b| a.file.cmp(&b.file));
    }
    all.into_iter()
        .map(|u| {
            #[cfg(feature = "test-io")]
            let _ = xirius_core::io_test::before("pool_metadata", &u.path);
            let meta = std::fs::metadata(&u.path).ok().filter(|m| m.is_file());
            Entry {
                kind: kind_of(&u),
                bytes: meta.as_ref().map_or(0, |m| m.len()),
                exists: meta.is_some(),
                external: dir.is_some_and(|d| is_external(&p.media, d, &u.path.to_string_lossy())),
                file: u.file,
                path: u.path,
                clips: u.clips,
                frozen: u.frozen,
                devices: u.devices,
            }
        })
        .collect()
}

/// The parts of `path` with `.` and `..` folded.
fn parts(path: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir | Component::RootDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    out
}

/// Whether two parts name the same entry (the file systems of Windows and the Mac ignore case).
fn same(a: &str, b: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

/// What is left of `path` below `root`, or `None` when `path` is not `root` nor under it.
fn below(path: &Path, root: &Path) -> Option<Vec<String>> {
    let (p, r) = (parts(path), parts(root));
    (p.len() >= r.len() && p.iter().zip(&r).all(|(a, b)| same(a, b))).then(|| p[r.len()..].to_vec())
}

/// The uses of `path` or of a file under it.
fn uses_under<'a>(all: &'a [Use], path: &Path) -> Vec<(&'a Use, Vec<String>)> {
    all.iter()
        .filter_map(|u| below(&u.path, path).map(|rest| (u, rest)))
        .collect()
}

/// The last part of `path`, for the messages.
fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The path asked for, on the disk. It has to be there.
fn existing(dir: Option<&Path>, args: &Args) -> CmdResult<PathBuf> {
    let given = args.str("path")?;
    if given.trim().is_empty() {
        return Err(CmdError::invalid("--path が空です"));
    }
    let path = std::path::absolute(target(dir, given)).map_err(|e| CmdError::io(e.to_string()))?;
    if std::fs::symlink_metadata(&path).is_err() {
        return Err(CmdError::not_found(format!(
            "{} はありません",
            path.display()
        )));
    }
    Ok(path)
}

/// Stops a change of the song folder, of a folder it is in, and of what the song keeps in it.
fn guard(dir: Option<&Path>, path: &Path, cannot: &str) -> CmdResult<()> {
    let Some(dir) = dir else {
        return Ok(());
    };
    if below(dir, path).is_some() {
        return Err(CmdError::conflict(format!(
            "{} は開いている曲のフォルダーのため、{cannot}",
            name_of(path)
        )));
    }
    if let Some(rest) = below(path, dir)
        && let Some(first) = rest.first()
        && SONG_OWN.iter().any(|own| same(own, first))
    {
        return Err(CmdError::conflict(format!(
            "{} は曲のデータのため、{cannot}",
            name_of(path)
        )));
    }
    Ok(())
}

/// The new name of `--name`, checked: one part, nothing a file name cannot have.
fn new_name(args: &Args) -> CmdResult<String> {
    let name = args.str("name")?.trim();
    if name.is_empty() {
        return Err(CmdError::invalid("--name が空です"));
    }
    if name == "." || name == ".." || name.ends_with('.') {
        return Err(CmdError::invalid(format!("{name} は名前に使用できません")));
    }
    if let Some(c) = name.chars().find(|c| BAD.contains(c) || c.is_control()) {
        return Err(CmdError::invalid(format!("名前に「{c}」は使用できません")));
    }
    Ok(name.to_string())
}

fn rename(p: &Project, dir: Option<&Path>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let from = existing(dir, args)?;
    guard(dir, &from, "名前を変更できません")?;
    let mut name = new_name(args)?;
    // A file keeps its kind: a name without an extension takes the old one.
    if from.is_file()
        && Path::new(&name).extension().is_none()
        && let Some(ext) = from.extension()
    {
        name = format!("{name}.{}", ext.to_string_lossy());
    }
    let to = from.with_file_name(&name);
    let old = name_of(&from);
    if old == name {
        return Err(CmdError::new(
            ErrorCode::NothingToDo,
            format!("{old} は既にその名前です"),
        ));
    }
    // Only the case differs: the same entry on Windows and the Mac, which a rename may change.
    if std::fs::symlink_metadata(&to).is_ok() && !same(&old, &name) {
        return Err(CmdError::conflict(format!("{name} は既にあります")));
    }
    let all = uses(p, dir);
    let mut files = Vec::new();
    let mut refs = 0;
    for (u, rest) in uses_under(&all, &from) {
        let moved = rest.iter().fold(to.clone(), |acc, part| acc.join(part));
        let new = song_path(&moved, dir);
        refs += u.clips + u.frozen + u.devices;
        files.push(json!({ "from": u.file, "to": new }));
    }
    if !dry {
        std::fs::rename(&from, &to)
            .map_err(|e| CmdError::io(format!("{old} の名前を変更できません: {e}")))?;
    }
    let mut message = if dry {
        format!("{old} の名前を {name} に変更します")
    } else {
        format!("{old} の名前を {name} に変更しました")
    };
    if refs > 0 {
        message.push_str(&format!("（曲の中の参照 {refs} 件を更新）"));
    }
    let output = Output::with(
        message,
        json!({
            "from": from.display().to_string(),
            "to": to.display().to_string(),
            "files": files,
            "references": refs,
        }),
    );
    let edits = if dry || files.is_empty() {
        Vec::new()
    } else {
        let mut a = Map::new();
        a.insert("files".into(), Value::Array(files));
        a.insert(
            "disk".into(),
            json!([{ "from": song_path(&from, dir), "to": song_path(&to, dir) }]),
        );
        vec![("clip.relink".to_string(), a)]
    };
    Ok(Plan { output, edits })
}

fn to_trash(
    p: &Project,
    dir: Option<&Path>,
    args: &Args,
    dry: bool,
    trash: &Trash,
) -> CmdResult<Plan> {
    let path = existing(dir, args)?;
    let cannot = "ごみ箱に移動できません";
    guard(dir, &path, cannot)?;
    let name = name_of(&path);
    let all = uses(p, dir);
    let used = uses_under(&all, &path);
    if !used.is_empty() {
        let message = if used.iter().any(|(_, rest)| rest.is_empty()) {
            format!("{name} は曲で使用中のため、{cannot}")
        } else {
            format!("{name} の中のファイルを曲で使用中のため、{cannot}")
        };
        return Err(CmdError::conflict(message));
    }
    if !dry {
        trash(&path).map_err(CmdError::conflict)?;
    }
    let message = if dry {
        format!("{name} をごみ箱に移動します")
    } else {
        format!("{name} をごみ箱に移動しました")
    };
    Ok(Plan {
        output: Output::with(message, json!({ "path": path.display().to_string() })),
        edits: Vec::new(),
    })
}

fn add_folder(dir: Option<&Path>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let parent = existing(dir, args)?;
    if !parent.is_dir() {
        return Err(CmdError::invalid(format!(
            "{} はフォルダーではありません",
            name_of(&parent)
        )));
    }
    let name = new_name(args)?;
    let path = parent.join(&name);
    guard(dir, &path, "フォルダーを作成できません")?;
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(CmdError::conflict(format!("{name} は既にあります")));
    }
    if !dry {
        std::fs::create_dir(&path)
            .map_err(|e| CmdError::io(format!("{name} を作成できません: {e}")))?;
    }
    let message = if dry {
        format!("フォルダー {name} を作成します")
    } else {
        format!("フォルダー {name} を作成しました")
    };
    Ok(Plan {
        output: Output::with(message, json!({ "path": path.display().to_string() })),
        edits: Vec::new(),
    })
}

fn list(p: &Project, dir: Option<&Path>) -> Output {
    let all = pool(p, dir);
    if all.is_empty() {
        return Output::with(
            "曲が使うファイルはありません",
            json!({ "files": [], "missing": 0 }),
        );
    }
    let mut lines = Vec::new();
    let mut files = Vec::new();
    let mut missing = 0;
    for u in &all {
        let exists = u.exists;
        let inside = dir.is_some_and(|d| below(&u.path, d).is_some());
        if !exists {
            missing += 1;
        }
        let mut count = Vec::new();
        if u.clips > 0 {
            count.push(format!("クリップ {} 個", u.clips));
        }
        if u.frozen > 0 {
            count.push(format!("フリーズ {} トラック", u.frozen));
        }
        if u.devices > 0 {
            count.push(format!("機器 {} 個", u.devices));
        }
        if count.is_empty() {
            count.push("使われていない録音".to_string());
        }
        let mut line = format!("{}（{}）", u.file, count.join("、"));
        if !exists {
            line.push_str(" 見つかりません");
        }
        lines.push(line);
        files.push(json!({
            "file": u.file,
            "path": u.path.display().to_string(),
            "exists": exists,
            "inside": inside,
            "external": u.external,
            "kind": u.kind,
            "bytes": u.bytes,
            "clips": u.clips,
            "frozen": u.frozen,
            "devices": u.devices,
        }));
    }
    let unused = all
        .iter()
        .filter(|u| u.clips + u.frozen + u.devices == 0)
        .count();
    let mut head = format!("曲が使うファイル {} 個", all.len() - unused);
    if missing > 0 {
        head.push_str(&format!("（見つからないもの {missing} 個）"));
    }
    if unused > 0 {
        head.push_str(&format!("・使われていない録音 {unused} 個"));
    }
    Output::with(
        format!("{head}\n{}", lines.join("\n")),
        json!({ "files": files, "missing": missing }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_below_itself_and_its_folders_in_any_case_the_os_ignores() {
        let root = Path::new("C:/song/Audio");
        assert_eq!(
            below(Path::new("C:/song/Audio/a.wav"), root),
            Some(vec!["a.wav".into()])
        );
        assert_eq!(below(root, root), Some(vec![]));
        assert_eq!(below(Path::new("C:/song/Audio2/a.wav"), root), None);
        assert_eq!(
            below(Path::new("C:/song/x/../Audio/b.wav"), root),
            Some(vec!["b.wav".into()])
        );
        if cfg!(windows) {
            assert!(below(Path::new("c:/SONG/audio/a.wav"), root).is_some());
        }
    }

    #[test]
    fn a_name_with_a_folder_or_a_mark_windows_refuses_is_refused() {
        let named = |n: &str| new_name(&Args::from_json(json!({ "name": n })));
        assert_eq!(named(" kick 2 ").unwrap(), "kick 2");
        for bad in ["", "a/b", "a\\b", "a:b", "what?", "..", "end."] {
            assert!(named(bad).is_err(), "{bad}");
        }
    }
}
