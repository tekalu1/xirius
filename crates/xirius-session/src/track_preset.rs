//! The track presets the user keeps (MIX-030): one JSON file for each, in a folder of the user's
//! (`XIRIUS_TRACK_PRESETS`, otherwise `track-presets` next to the plugin cache). The commands
//! `track.preset.save`, `apply`, `add`, `update` and `list` are answered here; what changes the
//! song goes out as the edits `track.preset.put`, `make` and `origin` of the command table, so one
//! apply is one step of the history and a preset opens the same way from the screen, the CLI and
//! MCP.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xirius_core::command::Output;
use xirius_core::id::TrackId;
use xirius_core::model::{Project, TrackKind};
use xirius_core::track_preset::TrackPreset;
use xirius_core::{Args, CmdError, CmdResult};
use xirius_plugins::Catalog;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 5] = [
    "track.preset.save",
    "track.preset.apply",
    "track.preset.add",
    "track.preset.update",
    "track.preset.list",
];

type Edits = Vec<(String, Map<String, Value>)>;

/// Where the presets are.
pub fn dir() -> PathBuf {
    std::env::var_os("XIRIUS_TRACK_PRESETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| xirius_plugins::default_cache_path().with_file_name("track-presets"))
}

/// The file name a preset's name makes: what Windows does not allow in a name becomes `_`.
fn file_name(name: &str) -> String {
    let mut s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || "\\/:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    s = s.trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s = "_".into();
    }
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let stem = s.split('.').next().unwrap_or_default().to_uppercase();
    if reserved.contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()))
    {
        s.insert(0, '_');
    }
    format!("{s}.json")
}

/// A preset on disk.
struct Stored {
    path: PathBuf,
    value: Value,
    name: String,
}

fn stored(dir: &Path) -> Vec<Stored> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Stored> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("json"))
        })
        .filter_map(|path| {
            let value: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
            let name = value
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))?;
            Some(Stored { path, value, name })
        })
        .collect();
    // The order they were saved in; files from before that was kept come first, by name.
    out.sort_by_key(|s| (saved_at(&s.value), s.name.to_lowercase()));
    out
}

fn saved_at(value: &Value) -> u64 {
    value.get("saved_at").and_then(Value::as_u64).unwrap_or(0)
}

fn find(dir: &Path, name: &str) -> CmdResult<Stored> {
    let name = name.trim();
    stored(dir)
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            CmdError::not_found(format!("トラックのプリセット「{name}」が見つかりません"))
        })
}

fn write(path: &Path, preset: &TrackPreset) -> CmdResult<()> {
    let mut data = serde_json::to_vec_pretty(&preset.to_value())
        .map_err(|e| CmdError::invalid(e.to_string()))?;
    data.push(b'\n');
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)
            .map_err(|e| CmdError::io(format!("{} を作れません: {e}", d.display())))?;
    }
    xirius_core::store::atomic_write(path, &data)
}

/// The uids of `preset`'s plugins that this PC does not have. With no scan to go by (an empty
/// catalogue), none are said to be missing.
fn missing(preset: &TrackPreset, catalog: &Catalog) -> Vec<String> {
    if catalog.entries.is_empty() {
        return Vec::new();
    }
    preset
        .inserts
        .iter()
        .filter(|x| !x.format.eq_ignore_ascii_case("builtin") && catalog.find(&x.uid).is_none())
        .map(|x| x.uid.clone())
        .collect()
}

/// Runs one of [`NAMES`] on the presets in `dir`.
pub(crate) fn run(
    dir: &Path,
    project: &Project,
    catalog: &Catalog,
    name: &str,
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    match name {
        "track.preset.list" => {
            let list: Vec<Value> = stored(dir)
                .iter()
                .map(|s| {
                    let n = |k: &str| s.value.get(k).and_then(Value::as_array).map_or(0, Vec::len);
                    json!({
                        "name": s.name,
                        "kind": s.value.get("kind").cloned().unwrap_or(Value::Null),
                        "inserts": n("inserts"),
                        "sends": n("sends"),
                        "file": s.path.to_string_lossy(),
                    })
                })
                .collect();
            Ok((
                Output::with(
                    format!("トラックのプリセットは {} 個です", list.len()),
                    json!({ "presets": list, "dir": dir.to_string_lossy() }),
                ),
                Vec::new(),
            ))
        }
        "track.preset.save" | "track.preset.update" => {
            let id: TrackId = a.id("track")?;
            let t = project.track(id)?;
            let (name, path, kept_at) = if name == "track.preset.save" {
                let n = a.str("name")?.trim().to_string();
                if n.is_empty() {
                    return Err(CmdError::invalid("プリセットの名前が空です"));
                }
                if stored(dir).iter().any(|s| s.name == n) {
                    return Err(CmdError::conflict(format!(
                        "「{n}」というトラックのプリセットはもうあります"
                    )));
                }
                let path = dir.join(file_name(&n));
                let last = stored(dir).iter().map(|s| saved_at(&s.value)).max();
                let at = xirius_core::store::now_ms().max(last.map_or(0, |l| l + 1));
                (n, path, Some(at))
            } else {
                let origin = t.preset.clone().ok_or_else(|| {
                    CmdError::conflict("プリセットから作ったトラックではありません")
                })?;
                let found = find(dir, &origin)?;
                let at = found.value.get("saved_at").and_then(Value::as_u64);
                (origin, found.path, at)
            };
            let mut preset = TrackPreset::snapshot(project, id, &name)?;
            preset.saved_at = kept_at;
            if !dry {
                write(&path, &preset)?;
            }
            let said = if name == t.name {
                format!("プリセット「{name}」")
            } else {
                format!("{}をプリセット「{name}」", t.name)
            };
            let verb = if dry {
                "として保存します"
            } else {
                "として保存しました"
            };
            // A preset keeps one chain, so a splitter's branches go in one after another (ENG-074).
            let flattened = t.rack.as_ref().is_some_and(|r| !r.is_serial());
            let note = match (flattened, dry) {
                (false, _) => "",
                (true, true) => "。分岐はプリセットに入らないため、直列にして保存します",
                (true, false) => "。分岐はプリセットに入らないため、直列にして保存しました",
            };
            let edits = if dry || t.preset.as_deref() == Some(name.as_str()) {
                Vec::new()
            } else {
                vec![edit(
                    "track.preset.origin",
                    json!({ "ids": [id.to_string()], "name": name }),
                )]
            };
            Ok((
                Output::with(
                    format!("{said}{verb}{note}"),
                    json!({
                        "name": name,
                        "file": path.to_string_lossy(),
                        "id": id.to_string(),
                        "flattened": flattened,
                    }),
                ),
                edits,
            ))
        }
        _ => {
            let found = find(dir, a.str("preset")?)?;
            let (preset, rows) = TrackPreset::parse(&found.value)?;
            let missing = missing(&preset, catalog).join(",");
            let mut args = json!({ "preset": found.value.to_string(), "missing": missing });
            let (edit_name, said) = if name == "track.preset.apply" {
                let ids = a.require_ids::<TrackId>("トラック")?;
                for id in &ids {
                    if project.track(*id)?.kind == TrackKind::Master {
                        return Err(CmdError::conflict("マスターには適用できません"));
                    }
                }
                args["ids"] = json!(ids.iter().map(ToString::to_string).collect::<Vec<_>>());
                (
                    "track.preset.put",
                    format!(
                        "{} トラックにプリセット「{}」を適用します",
                        ids.len(),
                        found.name
                    ),
                )
            } else {
                for k in ["into", "index"] {
                    if let Some(v) = a.0.get(k) {
                        args[k] = v.clone();
                    }
                }
                (
                    "track.preset.make",
                    format!("プリセット「{}」からトラックを作ります", found.name),
                )
            };
            let out = if dry {
                Output::with(said, json!({ "adjusted": rows }))
            } else {
                Output::msg("")
            };
            let edits = if dry {
                Vec::new()
            } else {
                vec![edit(edit_name, args)]
            };
            Ok((out, edits))
        }
    }
}

fn edit(name: &str, args: Value) -> (String, Map<String, Value>) {
    (
        name.to_string(),
        args.as_object().cloned().unwrap_or_default(),
    )
}
