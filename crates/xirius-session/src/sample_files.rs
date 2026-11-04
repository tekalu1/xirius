//! The commands of a Sampler's or a Drum Kit's files that read the disk (PLG-067, PLG-068):
//! `plugin.samples.load` reads a file or a folder's audio files, `plugin.samples.step` moves a
//! file to the one before or after it in its folder. Each leaves one `plugin.samples.*` edit,
//! so it is one step of the history.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::commands::samples::{kit_groups, kit_pads, used_pads};
use xirius_core::id::PluginId;
use xirius_core::model::Plugin;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};
use xirius_params::PAD_NAMES;

use crate::decode::is_audio;
use crate::import::{Plan, disk_path, song_path};

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 2] = ["plugin.samples.load", "plugin.samples.step"];

pub fn run(cx: &ExtContext<'_>, name: &str, args: &Args, dry: bool) -> CmdResult<Plan> {
    match name {
        "plugin.samples.load" => load(cx, args, dry),
        _ => step(cx, args, dry),
    }
}

fn plugin<'a>(cx: &'a ExtContext<'_>, args: &Args) -> CmdResult<&'a Plugin> {
    let id: PluginId = args.id("id")?;
    cx.project
        .tracks
        .values()
        .flat_map(|t| t.inserts.iter())
        .find(|x| x.id == id)
        .ok_or_else(|| CmdError::not_found(format!("プラグイン {id} が見つかりません")))
}

/// The audio files of a folder, by name (letter case aside).
pub fn audio_files(dir: &Path) -> CmdResult<Vec<PathBuf>> {
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("samples_list", dir).map_err(|e| CmdError::io(e.to_string()))?;
    let read = std::fs::read_dir(dir).map_err(|e| {
        CmdError::not_found(format!("フォルダーを読めません: {}（{e}）", dir.display()))
    })?;
    let mut files: Vec<PathBuf> = read
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_audio(p))
        .collect();
    files.sort_by_key(|p| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    });
    Ok(files)
}

/// What a drop of `names` on the Drum Kit `x` fills: the pads from `from` up to H16 (or the
/// free ones from A1); with `layers` and a pad, that one ([`kit_pads`]).
pub fn kit_estimate(x: &Plugin, names: &[String], from: Option<usize>, layers: bool) -> Vec<usize> {
    let Some(def) = x.builtin() else {
        return Vec::new();
    };
    let groups = kit_groups(names, layers);
    kit_pads(&used_pads(def, x), from, groups.len(), layers)
}

fn load(cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let x = plugin(cx, args)?;
    let mut given: Vec<&str> = args.opt_str("path").into_iter().collect();
    if let Some(list) = args.0.get("paths").and_then(Value::as_array) {
        for o in list {
            match o.get("path").and_then(Value::as_str) {
                Some(p) => given.push(p),
                None => return Err(CmdError::invalid("paths には [{path}] を指定してください")),
            }
        }
    }
    given.retain(|p| !p.trim().is_empty());
    if given.is_empty() {
        return Err(CmdError::invalid("--path か --paths を指定してください"));
    }
    // A file that is not audio among others is passed over and said so: the rest go in
    // (N-smp-5).
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    for given in &given {
        let path = disk_path(given, cx.dir)?;
        if path.is_dir() {
            files.extend(audio_files(&path)?);
        } else if path.is_file() {
            if !is_audio(&path) {
                skipped.push(
                    path.file_name()
                        .map_or_else(|| given.to_string(), |n| n.to_string_lossy().into_owned()),
                );
                continue;
            }
            files.push(path);
        } else {
            return Err(CmdError::not_found(format!(
                "ファイルが見つかりません: {}",
                path.display()
            )));
        }
    }
    if files.is_empty() && !skipped.is_empty() {
        return Err(CmdError::invalid(format!(
            "音声ファイルではありません: {}",
            skipped.join(", ")
        )));
    }
    if files.is_empty() {
        return Err(CmdError::not_found(format!(
            "音声ファイルがありません: {}",
            given.join(", ")
        )));
    }
    let names: Vec<String> = files.iter().map(|f| song_path(f, cx.dir)).collect();
    if let Some(index) = args.opt_i64("index").map(|i| i as usize) {
        return replace_one(x, index, &names[0], dry);
    }
    let mut add = Map::new();
    add.insert("id".into(), json!(x.id.to_string()));
    add.insert(
        "files".into(),
        json!(
            names
                .iter()
                .map(|f| json!({ "file": f }))
                .collect::<Vec<_>>()
        ),
    );
    for key in ["pad", "layers", "replace"] {
        if let Some(v) = args.0.get(key).filter(|v| !v.is_null()) {
            add.insert(key.into(), v.clone());
        }
    }
    let kit = x.builtin().is_some_and(|d| !d.per_pad.is_empty());
    let mut data = json!({ "plugin": x.id.to_string(), "count": names.len() });
    if !skipped.is_empty() {
        data["skipped"] = json!(skipped);
    }
    if kit {
        let from = args.opt_str("pad").and_then(xirius_params::pad_of);
        let mut x = x.clone();
        if args.flag("replace") {
            x.samples.clear();
        }
        let pads = kit_estimate(&x, &names, from, args.flag("layers"));
        if !pads.is_empty() {
            data["pads"] = json!(pads.iter().map(|p| PAD_NAMES[*p]).collect::<Vec<_>>());
        }
    }
    let mut message = format!(
        "{} に {} 個のサンプルを{}",
        x.name,
        names.len(),
        if dry {
            "読み込みます"
        } else {
            "読み込みました"
        }
    );
    if !skipped.is_empty() {
        message.push_str(&format!(
            "（音声ファイルでない {} は飛ばしました）",
            skipped.join("、")
        ));
    }
    Ok(Plan {
        output: Output::with(message, data),
        edits: if dry {
            Vec::new()
        } else {
            vec![("plugin.samples.add".into(), add)]
        },
    })
}

/// `plugin.samples.load` with `index`: that sample becomes the file `name`, its values as
/// they were (the window's 「サンプルを置き換え…」).
fn replace_one(x: &Plugin, index: usize, name: &str, dry: bool) -> CmdResult<Plan> {
    if index == 0 || index > x.samples.len() {
        return Err(CmdError::not_found(format!(
            "{} にサンプル {index} はありません（{} 個）",
            x.name,
            x.samples.len()
        )));
    }
    let mut set = Map::new();
    set.insert("id".into(), json!(x.id.to_string()));
    set.insert("index".into(), json!(index));
    set.insert("file".into(), json!(name));
    let shown = Path::new(name)
        .file_name()
        .map_or_else(|| name.to_string(), |n| n.to_string_lossy().into_owned());
    Ok(Plan {
        output: Output::with(
            format!("{} のサンプル {index} を {shown} にしました", x.name),
            json!({ "plugin": x.id.to_string(), "index": index, "file": name }),
        ),
        edits: if dry || x.samples[index - 1] == name {
            Vec::new()
        } else {
            vec![("plugin.samples.set".into(), set)]
        },
    })
}

fn step(cx: &ExtContext<'_>, args: &Args, dry: bool) -> CmdResult<Plan> {
    let x = plugin(cx, args)?;
    let index = args.i64("index")? as usize;
    let by = args.opt_i64("by").unwrap_or(1);
    let Some(name) = x.samples.get(index.saturating_sub(1)) else {
        return Err(CmdError::not_found(format!(
            "{} にサンプル {index} はありません",
            x.name
        )));
    };
    let path = disk_path(name, cx.dir)?;
    let dir = path
        .parent()
        .ok_or_else(|| CmdError::not_found(format!("フォルダーがありません: {name}")))?;
    let files = audio_files(dir)?;
    if files.is_empty() {
        return Err(CmdError::not_found(format!(
            "音声ファイルがありません: {}",
            dir.display()
        )));
    }
    let leaf = |p: &Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let here = leaf(&path);
    // Where the file is among its neighbours: a file gone from the folder is where its name
    // would sort.
    let at = files.iter().position(|f| leaf(f) == here).map_or_else(
        || {
            let after = files
                .iter()
                .position(|f| leaf(f) > here)
                .unwrap_or(files.len());
            after as i64 - i64::from(by > 0)
        },
        |i| i as i64,
    );
    let n = files.len() as i64;
    let next = &files[(at + by).rem_euclid(n) as usize];
    let file = song_path(next, cx.dir);
    let mut set = Map::new();
    set.insert("id".into(), json!(x.id.to_string()));
    set.insert("index".into(), json!(index));
    set.insert("file".into(), Value::String(file.clone()));
    let shown = next
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.clone());
    Ok(Plan {
        output: Output::with(
            format!("{} のサンプル {index} を {shown} にしました", x.name),
            json!({ "plugin": x.id.to_string(), "index": index, "file": file }),
        ),
        edits: if dry || file == *name {
            Vec::new()
        } else {
            vec![("plugin.samples.set".into(), set)]
        },
    })
}
