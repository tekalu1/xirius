//! The song's pool (BRW-004): `file.collect` copies files from outside the song into its media
//! folder's `Imports` and points the clips at the copies; `file.locate` points the clips of a
//! file that is not found at where it is now. Each is one step of the history (a
//! `clip.relink`); an undo points the clips back and leaves the copies.
//!
//! `file.locate --new_path` also points the other missing files of the same folder at the files of
//! the same name beside the one picked, as a folder that moved takes all of them along.
//! `file.locate --folder` looks for every missing file below a folder by its name.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::model::Media;
use xirius_core::session::{copy_into, song_name};
use xirius_core::{Args, CmdError, CmdResult, ErrorCode, ExtContext, Output};

use crate::file_ops::{Entry, pool};
use crate::import::Plan;
use crate::reveal::target;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 2] = ["file.collect", "file.locate"];

/// How many folders `file.locate --folder` looks through at most, so a drive picked by mistake
/// ends.
const MAX_FOLDERS: usize = 20_000;

pub fn run(cx: &ExtContext<'_>, name: &str, args: &Args, dry: bool) -> CmdResult<Plan> {
    let dir = match cx.dir {
        Some(d) => Some(std::path::absolute(d).map_err(|e| CmdError::io(e.to_string()))?),
        None => None,
    };
    let dir = dir.as_deref();
    let all = pool(cx.project, dir);
    match name {
        "file.collect" => collect(cx, dir, &all, args, dry),
        _ => locate(dir, &all, args, dry),
    }
}

/// The names `--paths` gives (`|` between them), or none.
fn names(args: &Args) -> Vec<String> {
    args.opt_str("paths")
        .map(|s| {
            s.split(['|', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The pool's entry the song names `file` (with `/` or `\`, in the case the disk ignores).
fn find<'a>(all: &'a [Entry], file: &str) -> Option<&'a Entry> {
    let norm = |s: &str| {
        let s = s.trim().replace('\\', "/");
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s
        }
    };
    let want = norm(file);
    all.iter()
        .find(|e| e.clips + e.frozen > 0 && norm(&e.file) == want)
}

fn relink(pairs: Vec<Value>) -> Vec<(String, Map<String, Value>)> {
    let mut m = Map::new();
    m.insert("files".into(), Value::Array(pairs));
    vec![("clip.relink".into(), m)]
}

fn nothing(message: impl Into<String>) -> CmdError {
    CmdError::new(ErrorCode::NothingToDo, message)
}

fn collect(
    cx: &ExtContext<'_>,
    dir: Option<&Path>,
    all: &[Entry],
    args: &Args,
    dry: bool,
) -> CmdResult<Plan> {
    let Some(dir) = dir else {
        return Err(CmdError::conflict(
            "保存前の曲のため、曲のフォルダーにコピーできません（先に保存が必要です）",
        ));
    };
    let asked = names(args);
    let mut picked: Vec<&Entry> = Vec::new();
    if asked.is_empty() {
        picked.extend(all.iter().filter(|e| e.external && e.clips + e.frozen > 0));
    } else {
        for name in &asked {
            let e = find(all, name).ok_or_else(|| {
                CmdError::not_found(format!("{name} は曲が使うファイルではありません"))
            })?;
            if !e.external {
                return Err(nothing(format!("{name} はすでに曲のフォルダーにあります")));
            }
            if !picked.contains(&e) {
                picked.push(e);
            }
        }
    }
    let missing: Vec<&str> = picked
        .iter()
        .filter(|e| !e.exists)
        .map(|e| e.file.as_str())
        .collect();
    if !asked.is_empty() && !missing.is_empty() {
        return Err(CmdError::not_found(format!(
            "{} が見つからないため、コピーできません",
            missing.join("、")
        )));
    }
    picked.retain(|e| e.exists);
    if picked.is_empty() {
        return Err(nothing("曲のフォルダーの外のファイルはありません"));
    }
    let imports = cx.project.media.place(dir, Media::IMPORTS);
    let n = picked.len();
    if dry {
        let files: Vec<&str> = picked.iter().map(|e| e.file.as_str()).collect();
        return Ok(Plan {
            output: Output::with(
                format!("ファイル {n} 個を曲のフォルダーにコピーします"),
                json!({ "files": files, "folder": imports.display().to_string(), "missing": missing }),
            ),
            edits: Vec::new(),
        });
    }
    let mut taken: Vec<PathBuf> = Vec::new();
    let mut pairs = Vec::new();
    for e in &picked {
        let dest = copy_into(&imports, &e.path, &taken)?;
        taken.push(dest.clone());
        pairs.push(json!({ "from": e.file, "to": song_name(&dest, dir) }));
    }
    let mut message = format!("ファイル {n} 個を曲のフォルダーにコピーしました");
    if !missing.is_empty() {
        message.push_str(&format!(
            "。見つからないファイル {} 個はそのままです（{}）",
            missing.len(),
            missing.join("、")
        ));
    }
    Ok(Plan {
        output: Output::with(
            message,
            json!({ "files": pairs, "folder": imports.display().to_string(), "missing": missing }),
        ),
        edits: relink(pairs),
    })
}

/// The last part of `p`, compared the way the disk compares it.
fn leaf(p: &Path) -> String {
    let n = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if cfg!(any(windows, target_os = "macos")) {
        n.to_lowercase()
    } else {
        n
    }
}

/// The files below `root` by their last part ([`leaf`]); the first found in name order wins.
fn files_below(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    let mut seen = 0;
    while let Some(d) = folders.pop() {
        seen += 1;
        if seen > MAX_FOLDERS {
            break;
        }
        let Ok(list) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut entries: Vec<PathBuf> = list.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        for p in entries.into_iter().rev() {
            if p.is_dir() {
                folders.push(p);
            } else {
                out.push((leaf(&p), p));
            }
        }
    }
    out
}

fn locate(dir: Option<&Path>, all: &[Entry], args: &Args, dry: bool) -> CmdResult<Plan> {
    let to = args
        .opt_str("new_path")
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let folder = args
        .opt_str("folder")
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let path = args
        .opt_str("path")
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let named = |e: &Entry, at: &Path| {
        json!({ "from": e.file, "to": match dir {
        Some(d) => song_name(at, d),
        None => at.display().to_string(),
    } })
    };
    let lost: Vec<&Entry> = all
        .iter()
        .filter(|e| !e.exists && e.clips + e.frozen > 0)
        .collect();
    let mut pairs = Vec::new();
    match (to, folder) {
        (Some(_), Some(_)) => {
            return Err(CmdError::invalid(
                "--new_path と --folder は同時に指定できません",
            ));
        }
        (None, None) => {
            return Err(CmdError::invalid(
                "--new_path か --folder が無いと場所を指定できません",
            ));
        }
        (Some(to), None) => {
            let path = path
                .ok_or_else(|| CmdError::invalid("--path が無いと --new_path を指定できません"))?;
            let e = find(all, path).ok_or_else(|| {
                CmdError::not_found(format!("{path} は曲が使うファイルではありません"))
            })?;
            let at =
                std::path::absolute(target(dir, to)).map_err(|e| CmdError::io(e.to_string()))?;
            if !at.is_file() {
                return Err(CmdError::not_found(format!(
                    "{} はありません",
                    at.display()
                )));
            }
            pairs.push(named(e, &at));
            // The other missing files of the same folder, beside the one picked.
            let old = e.path.parent().map(leafless);
            let new = at.parent().unwrap_or(Path::new(""));
            for other in &lost {
                if other.file == e.file || other.path.parent().map(leafless) != old {
                    continue;
                }
                let Some(name) = other.path.file_name() else {
                    continue;
                };
                let beside = new.join(name);
                if beside.is_file() {
                    pairs.push(named(other, &beside));
                }
            }
        }
        (None, Some(folder)) => {
            let root = std::path::absolute(target(dir, folder))
                .map_err(|e| CmdError::io(e.to_string()))?;
            if !root.is_dir() {
                return Err(CmdError::not_found(format!(
                    "{} はありません",
                    root.display()
                )));
            }
            let wanted: Vec<&Entry> = match path {
                Some(path) => vec![find(all, path).ok_or_else(|| {
                    CmdError::not_found(format!("{path} は曲が使うファイルではありません"))
                })?],
                None => lost.clone(),
            };
            if wanted.is_empty() {
                return Err(nothing("見つからないファイルはありません"));
            }
            let found = files_below(&root);
            for e in wanted {
                let name = leaf(&e.path);
                if let Some((_, at)) = found.iter().find(|(n, _)| *n == name) {
                    pairs.push(named(e, at));
                }
            }
        }
    }
    let n = pairs.len();
    let left = lost
        .iter()
        .filter(|e| !pairs.iter().any(|p| p["from"] == e.file.as_str()))
        .count();
    if n == 0 {
        return Err(CmdError::not_found(
            "見つからないファイルと同じ名前のファイルが、そのフォルダーにありません",
        ));
    }
    let mut message = if dry {
        format!("ファイル {n} 個の場所を付け替えます")
    } else {
        format!("ファイル {n} 個の場所を付け替えました")
    };
    if left > 0 {
        message.push_str(&format!("（見つからないファイルはあと {left} 個）"));
    }
    Ok(Plan {
        output: Output::with(message, json!({ "files": pairs, "missing": left })),
        edits: if dry { Vec::new() } else { relink(pairs) },
    })
}

/// `p` made comparable as a folder (in the case the disk ignores, `/` between the parts).
fn leafless(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if cfg!(any(windows, target_os = "macos")) {
        s.to_lowercase()
    } else {
        s
    }
}
