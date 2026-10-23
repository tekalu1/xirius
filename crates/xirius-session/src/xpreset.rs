//! The device presets the user keeps (PLG-063): one `.xpreset` JSON file per named preset,
//! in `presets/<kind>/<device>/` of a folder of the user's (`XIRIUS_PRESETS`, otherwise
//! `presets` next to the plugin cache).
//!
//! `preset.save`, `load`, `list`, `remove`, `rename` and `factory.restore` are answered
//! here; what changes the song goes out as the edit `preset.put` of the command table, so
//! one load is one step of the history and a preset reads the same way from the screen,
//! the CLI and MCP.
//!
//! A preset is its device and its name, letter case not told apart (as Windows does not
//! tell file names apart): one device has one "Warm", whatever its file is called. Each
//! name makes its own file name ([`file_name`]), and nothing is written over a file that is
//! not the preset's own; a file is written whole to a temporary one and renamed over.
//!
//! The factory presets of the built-in effects are copied in when the app starts ([`seed`],
//! or the first save or load), and the mark `.factory-seeded` records the set's version and
//! each file copied with its hash. A copied factory preset is a plain preset file — it can
//! be overwritten, renamed and deleted. A newer set adds what it brings and takes away the
//! old files the user left as they were copied. `preset.factory.restore` adds back the
//! factory presets that are missing, and touches nothing that is there.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Map, Value, json};
use xirius_core::command::Output;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project};
use xirius_core::store::{atomic_write, sha256_hex};
use xirius_core::xpreset::{DEFAULT_PRESET, DevicePreset, PresetDevice, kind_name, kind_of};
use xirius_core::{Args, CmdError, CmdResult};
use xirius_params::{BuiltinDef, FACTORY_VERSION, factory_presets};
use xirius_plugins::Catalog;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 6] = [
    "preset.save",
    "preset.load",
    "preset.list",
    "preset.remove",
    "preset.rename",
    "preset.factory.restore",
];

type Edits = Vec<(String, Map<String, Value>)>;

/// Written once the factory presets were copied, so a preset the user deleted is not
/// copied again on the next run (only `preset.factory.restore` brings it back).
const SEED_MARK: &str = ".factory-seeded";

const MAX_PRESET_BYTES: u64 = 64 * 1024 * 1024;

/// The longest file name (without `.xpreset`) a preset's name makes.
const MAX_FILE_CHARS: usize = 100;

/// Where the presets are.
pub fn dir() -> PathBuf {
    std::env::var_os("XIRIUS_PRESETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| xirius_plugins::default_cache_path().with_file_name("presets"))
}

/// The file name a name makes, a different one for each name: what Windows does not allow
/// in a file name (and `%` and `~`) becomes `%` and its UTF-8 bytes in hex, so `a:b`, `a?b`
/// and `a_b` are three files. A name too long for one keeps its first characters, then `~`
/// and a hash of the whole name.
fn file_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    // Windows drops the dots and spaces a file name ends with.
    let tail = chars
        .iter()
        .rposition(|c| !matches!(c, '.' | ' '))
        .map_or(0, |i| i + 1);
    let mut s = String::new();
    for (i, c) in chars.iter().enumerate() {
        if c.is_control() || "\\/:*?\"<>|%~".contains(*c) || i >= tail {
            escape(&mut s, *c);
        } else {
            s.push(*c);
        }
    }
    if s.is_empty() {
        s.push('%');
    }
    let stem = s
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.chars().count() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_digit() || "¹²³".contains(c)));
    if reserved {
        let first = s.remove(0);
        let mut head = String::new();
        escape(&mut head, first);
        s.insert_str(0, &head);
    }
    if s.chars().count() > MAX_FILE_CHARS {
        let head: String = s.chars().take(MAX_FILE_CHARS - 17).collect();
        s = format!("{head}~{}", &sha256_hex(name.as_bytes())[..16]);
    }
    s
}

fn escape(s: &mut String, c: char) {
    for b in c.to_string().bytes() {
        let _ = write!(s, "%{b:02X}");
    }
}

/// A name as letter case does not tell it apart.
fn fold(s: &str) -> String {
    s.to_lowercase()
}

fn io(e: std::io::Error) -> CmdError {
    CmdError::conflict(format!("プリセットのファイルを読み書きできません: {e}"))
}

/// The file's name, for a message.
fn shown(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// A `.xpreset` on disk: the file's own text plus what was parsed far enough to know its
/// name and device.
#[derive(Clone)]
struct Entry {
    path: PathBuf,
    value: Value,
    name: String,
    kind: PluginKind,
    device_uid: String,
    device_name: String,
    device_format: String,
    saved_at: Option<u64>,
}

impl Entry {
    fn device(&self) -> String {
        canon_uid(&self.device_format, &self.device_uid)
    }

    /// Whether this is the preset `name` of the device `device` (a canonical uid).
    fn is(&self, kind: PluginKind, device: &str, name: &str) -> bool {
        self.kind == kind && self.device() == device && fold(&self.name) == fold(name)
    }
}

/// A `.xpreset` that could not be read, and why.
struct Skipped {
    path: PathBuf,
    reason: String,
}

/// The canonical key a device is grouped by: a built-in under either its uid or its name is
/// the same device; anything else is the uid itself, letter case ignored.
fn canon_uid(format: &str, uid: &str) -> String {
    if format.eq_ignore_ascii_case("builtin")
        && let Some(def) = xirius_params::find(uid)
    {
        return def.uid.to_string();
    }
    uid.to_lowercase()
}

/// Every `.xpreset` under `presets/<kind>/`, both the device folders and loose files left
/// at the kind folder itself, ordered by path: those that parse, and those that do not.
fn stored(dir: &Path) -> (Vec<Entry>, Vec<Skipped>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for kind in [PluginKind::Effect, PluginKind::Instrument] {
        let kd = dir.join(kind_name(kind));
        let mut dirs = Vec::new();
        let mut loose = Vec::new();
        for e in std::fs::read_dir(&kd).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                loose.push(p);
            }
        }
        let paths: Vec<PathBuf> = dirs.iter().flat_map(|d| files_in(d)).chain(loose).collect();
        for p in paths {
            if p.extension().and_then(|e| e.to_str()) != Some("xpreset") || p.is_dir() {
                continue;
            }
            match entry_at(&p) {
                Ok(e) => out.push(e),
                Err(reason) => skipped.push(Skipped { path: p, reason }),
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    (out, skipped)
}

fn files_in(d: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v
}

/// The preset the file `path` holds, or why it is not one.
fn entry_at(path: &Path) -> Result<Entry, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_PRESET_BYTES {
        return Err("ファイルが大きすぎます".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let v: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("JSON として読めません: {e}"))?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
    let (preset, _) = DevicePreset::parse(&v, stem.as_deref()).map_err(|e| e.message)?;
    Ok(Entry {
        path: path.to_path_buf(),
        value: v,
        name: preset.name,
        kind: preset.kind,
        device_uid: preset.device.uid,
        device_name: preset.device.name,
        device_format: preset.device.format,
        saved_at: preset.saved_at,
    })
}

/// The file the preset `name` of the device `uid` of kind `kind` makes.
fn path_of(dir: &Path, kind: PluginKind, uid: &str, name: &str) -> PathBuf {
    dir.join(kind_name(kind))
        .join(file_name(uid))
        .join(format!("{}.xpreset", file_name(name)))
}

/// Whether `a` and `b` are one file on the disk (the same path in another letter case, on
/// Windows).
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Refuses when a file other than `own` (the preset's own until now) is at `to`: another
/// preset's file, or one that cannot be read.
fn free(to: &Path, own: Option<&Path>) -> CmdResult<()> {
    if std::fs::symlink_metadata(to).is_err() || own.is_some_and(|o| same_file(o, to)) {
        return Ok(());
    }
    Err(CmdError::conflict(format!(
        "{} には別のファイルがあります（別の名前にするか、そのファイルを片付けてください）",
        shown(to)
    )))
}

/// Writes `data` as the file `to`; `own` is the preset's file until now (`None`: a new one).
/// The data goes to a temporary file and is renamed over, so a failed write leaves the old
/// file whole; a move to another name (a change of the letter case only, too) is one rename.
fn put_file(own: Option<&Path>, to: &Path, data: &[u8]) -> CmdResult<()> {
    match own {
        None => atomic_write(to, data),
        Some(o) => {
            atomic_write(o, data)?;
            if o != to {
                std::fs::rename(o, to).map_err(io)?;
            }
            Ok(())
        }
    }
}

fn to_bytes(v: &Value) -> CmdResult<Vec<u8>> {
    serde_json::to_vec_pretty(v).map_err(|e| CmdError::invalid(e.to_string()))
}

fn def_kind(def: &BuiltinDef) -> PluginKind {
    match def.kind {
        xirius_params::Kind::Instrument => PluginKind::Instrument,
        xirius_params::Kind::Effect => PluginKind::Effect,
    }
}

/// The factory set: the built-in effects' named presets, each with its file's bytes. The
/// Sampler and the Drum Kit have only their Default (PLG-064). A factory file carries the
/// values only — no samples, no time — so the same set makes the same bytes.
fn factory_set() -> Vec<(&'static BuiltinDef, &'static str, Vec<u8>)> {
    let mut out = Vec::new();
    for def in xirius_params::BUILTINS.iter().copied() {
        if def_kind(def) != PluginKind::Effect {
            continue;
        }
        for (name, values) in factory_presets(def) {
            let preset = DevicePreset {
                name: name.to_string(),
                kind: PluginKind::Effect,
                device: PresetDevice {
                    name: def.name.into(),
                    kind: PluginKind::Effect,
                    format: "builtin".into(),
                    uid: def.uid.into(),
                    params: def
                        .params
                        .iter()
                        .zip(values.iter().copied())
                        .map(|(p, v)| (p.id.to_string(), v))
                        .collect(),
                    state: None,
                    samples: None,
                },
                saved_at: None,
            };
            let data = serde_json::to_vec_pretty(&preset.to_value()).unwrap_or_default();
            out.push((def, name, data));
        }
    }
    out
}

/// What the mark says: its set's version and the files it records; `None` when there is
/// none. A mark that is not such a record (the first form held a time only) counts as the
/// current set, copied with nothing recorded.
fn mark(dir: &Path) -> Option<(u32, Vec<Value>)> {
    let text = std::fs::read(dir.join(SEED_MARK)).ok()?;
    let Ok(Value::Object(m)) = serde_json::from_slice::<Value>(&text) else {
        return Some((FACTORY_VERSION, Vec::new()));
    };
    let version = m.get("version").and_then(Value::as_u64).unwrap_or(0);
    let files = m
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some((u32::try_from(version).unwrap_or(u32::MAX), files))
}

fn write_mark(dir: &Path, version: u32, files: &[Value]) -> CmdResult<()> {
    let v = json!({ "version": version, "seeded_at": now_ms(), "files": files });
    atomic_write(&dir.join(SEED_MARK), &to_bytes(&v)?)
}

/// The mark's row of a copied file: which preset, where (from `dir`, `/` between the
/// folders) and the hash of what was written.
fn record(
    dir: &Path,
    kind: PluginKind,
    device: &str,
    name: &str,
    path: &Path,
    data: &[u8],
) -> Value {
    let file = path.strip_prefix(dir).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |r| {
            r.components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        },
    );
    json!({
        "kind": kind_name(kind),
        "device": device,
        "name": name,
        "file": file,
        "sha256": sha256_hex(data),
    })
}

fn recorded(r: &Value, kind: PluginKind, device: &str, name: &str) -> bool {
    r["kind"] == kind_name(kind)
        && r["device"].as_str() == Some(device)
        && r["name"].as_str().is_some_and(|n| fold(n) == fold(name))
}

/// The file a mark's row names, when it stays inside `dir`.
fn under(dir: &Path, rel: &str) -> Option<PathBuf> {
    let r = Path::new(rel);
    r.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| dir.join(r))
}

/// One seeding at a time, from the app's start and a command alike.
static SEEDING: Mutex<()> = Mutex::new(());

/// Copies the factory presets into `dir` the first time, and brings a folder copied from an
/// older factory set up to this one: what the set adds is copied, and an old file the set
/// no longer has is removed when its hash is still what was copied. Never writes over a
/// file (a preset of the same name in any file, or any file at the place, readable or not),
/// and never copies again what the mark records (the user may have deleted it).
pub fn seed(dir: &Path) -> CmdResult<()> {
    let _one = SEEDING.lock().unwrap_or_else(|e| e.into_inner());
    let before = mark(dir);
    if before.as_ref().is_some_and(|(v, _)| *v >= FACTORY_VERSION) {
        return Ok(());
    }
    let set = factory_set();
    let mut keep = Vec::new();
    for r in before.map(|(_, files)| files).unwrap_or_default() {
        let in_set = set
            .iter()
            .any(|(def, name, _)| recorded(&r, def_kind(def), def.uid, name));
        if in_set {
            keep.push(r);
            continue;
        }
        if let Some(p) = r["file"].as_str().and_then(|f| under(dir, f))
            && let Ok(bytes) = std::fs::read(&p)
            && r["sha256"].as_str() == Some(sha256_hex(&bytes).as_str())
        {
            std::fs::remove_file(&p).map_err(io)?;
        }
    }
    let (entries, _) = stored(dir);
    for (def, name, data) in &set {
        let kind = def_kind(def);
        if keep.iter().any(|r| recorded(r, kind, def.uid, name)) {
            continue;
        }
        if let Some(e) = entries.iter().find(|e| e.is(kind, def.uid, name)) {
            // Already there (restored by hand, say): recorded when it is the factory file.
            if std::fs::read(&e.path).is_ok_and(|b| b == *data) {
                keep.push(record(dir, kind, def.uid, name, &e.path, data));
            }
            continue;
        }
        let path = path_of(dir, kind, def.uid, name);
        if std::fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        atomic_write(&path, data)?;
        keep.push(record(dir, kind, def.uid, name, &path, data));
    }
    write_mark(dir, FACTORY_VERSION, &keep)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// The plug-in `id` on the track `tid`.
fn plugin_on(project: &Project, tid: TrackId, id: PluginId) -> CmdResult<&Plugin> {
    let t = project.track(tid)?;
    t.inserts
        .iter()
        .find(|x| x.id == id)
        .ok_or_else(|| CmdError::not_found(format!("このトラックにプラグイン {id} がありません")))
}

/// The device the `--device` text names: a built-in's uid under its uid or name, a
/// catalogue entry's uid, an entry's device, else the text itself.
fn device_key(device: &str, catalog: &Catalog, entries: &[Entry]) -> String {
    if let Some(def) = xirius_params::find(device) {
        return def.uid.to_string();
    }
    if let Some(e) = catalog.find(device) {
        return e.uid.to_lowercase();
    }
    if let Some(e) = entries.iter().find(|e| {
        e.device_uid.eq_ignore_ascii_case(device) || e.device_name.eq_ignore_ascii_case(device)
    }) {
        return e.device();
    }
    device.to_lowercase()
}

/// The entries named `name` (letter case aside), narrowed by `device` (a canonical uid).
fn find_in<'e>(entries: &'e [Entry], name: &str, device: Option<&str>) -> Vec<&'e Entry> {
    entries
        .iter()
        .filter(|e| fold(&e.name) == fold(name))
        .filter(|e| device.is_none_or(|d| e.device() == d))
        .collect()
}

/// The error for a name more than one file holds: on one device, the files to pick from by
/// path; on several, how to name the device (`hint`).
fn several(name: &str, found: &[&Entry], hint: &str) -> CmdError {
    let one = found
        .iter()
        .all(|e| e.kind == found[0].kind && e.device() == found[0].device());
    if one {
        let files: Vec<String> = found.iter().map(|e| shown(&e.path)).collect();
        CmdError::conflict(format!(
            "「{name}」というプリセットがこの機器に {} 個あります（{}）。--preset にファイルのパスを指定してください",
            found.len(),
            files.join("、")
        ))
    } else {
        CmdError::conflict(format!(
            "「{name}」というプリセットがほかの機器にもあります（{hint}で機器を指定してください）"
        ))
    }
}

fn is_default(name: &str) -> bool {
    fold(name) == fold(DEFAULT_PRESET)
}

fn preset_name(a: &Args, key: &str) -> CmdResult<String> {
    let n = a.str(key)?.trim().to_string();
    if n.is_empty() {
        return Err(CmdError::invalid("名前を入力してください"));
    }
    if is_default(&n) {
        return Err(CmdError::conflict(format!(
            "「{n}」という名前は使えません（「{DEFAULT_PRESET}」は機器の既定の状態の名前です）"
        )));
    }
    Ok(n)
}

/// The plug-in kind (`fx`/`instrument`) an arg names, or the error for anything else.
fn kind_arg(a: &Args, key: &str) -> CmdResult<PluginKind> {
    kind_of(a.str(key)?)
        .ok_or_else(|| CmdError::invalid("kind には fx か instrument を指定してください"))
}

/// Whether a `--preset` that names no preset was meant as a file.
fn looks_like_path(spec: &str) -> bool {
    spec.contains(['/', '\\']) || spec.to_lowercase().ends_with(".xpreset")
}

/// The file a `--preset` path names: only a full path, which means the same from the
/// screen, the CLI and MCP.
fn path_arg(spec: &str) -> CmdResult<PathBuf> {
    let p = PathBuf::from(spec);
    if !p.is_absolute() {
        return Err(CmdError::invalid(
            "プリセットのファイルは絶対パスで指定してください",
        ));
    }
    if !p.is_file() {
        return Err(CmdError::not_found(format!(
            "プリセット「{spec}」が見つかりません"
        )));
    }
    Ok(p)
}

/// Refuses a file outside the presets folder (for a removal or a rename).
fn inside(dir: &Path, p: &Path) -> CmdResult<()> {
    let ok = match (dir.canonicalize(), p.canonicalize()) {
        (Ok(d), Ok(f)) => f.starts_with(&d) && f != d,
        _ => false,
    };
    if ok && p.extension().and_then(|e| e.to_str()) == Some("xpreset") {
        Ok(())
    } else {
        Err(CmdError::conflict(format!(
            "{} はプリセットの置き場の外のファイルです（{}）",
            p.to_string_lossy(),
            dir.to_string_lossy()
        )))
    }
}

/// Where a sample of a preset is: a full path itself, a name in the song from the song's
/// folder; `None` when it cannot be placed.
fn sample_path(s: &str, song: Option<&Path>) -> Option<PathBuf> {
    let p = Path::new(s);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    song.map(|d| {
        s.split(['/', '\\'])
            .filter(|c| !c.is_empty())
            .fold(d.to_path_buf(), |acc, c| acc.join(c))
    })
}

/// Runs one of [`NAMES`] on the presets in `dir`; `song` is the open song's folder.
pub(crate) fn run(
    dir: &Path,
    song: Option<&Path>,
    project: &Project,
    catalog: &Catalog,
    name: &str,
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    // The app copies the factory presets when it starts; a session with no app (the CLI,
    // MCP) does it on the first save or load. A command that only reads writes nothing, and
    // the save's own write says when the folder cannot be written.
    if !dry && matches!(name, "preset.save" | "preset.load") {
        let _ = seed(dir);
    }
    let (entries, skipped) = stored(dir);
    match name {
        "preset.save" => save(dir, song, project, &entries, a, dry),
        "preset.list" => list(dir, catalog, &entries, &skipped, a),
        "preset.load" => load(song, project, catalog, &entries, a, dry),
        "preset.remove" | "preset.rename" => {
            let spec = a.str("preset")?.trim();
            if is_default(spec) {
                return Err(CmdError::conflict(format!(
                    "「{DEFAULT_PRESET}」は機器の既定の状態で、プリセットのファイルではありません"
                )));
            }
            let device = a
                .opt_str("device")
                .map(|d| device_key(d, catalog, &entries));
            let found = find_in(&entries, spec, device.as_deref());
            let e = match found.as_slice() {
                [one] => (*one).clone(),
                [] if looks_like_path(spec) => {
                    let p = path_arg(spec)?;
                    inside(dir, &p)?;
                    if name == "preset.remove" {
                        // A file that cannot be read goes too, by its path.
                        if !dry {
                            std::fs::remove_file(&p).map_err(io)?;
                        }
                        return Ok((removed(None, &p, dry), Vec::new()));
                    }
                    entry_at(&p).map_err(|r| {
                        CmdError::invalid(format!("プリセットを読み込めません: {r}"))
                    })?
                }
                [] => {
                    return Err(CmdError::not_found(format!(
                        "プリセット「{spec}」が見つかりません"
                    )));
                }
                many => return Err(several(spec, many, "--device ")),
            };
            if name == "preset.remove" {
                if !dry {
                    std::fs::remove_file(&e.path).map_err(io)?;
                }
                Ok((removed(Some(&e.name), &e.path, dry), Vec::new()))
            } else {
                rename(&entries, &e, &preset_name(a, "name")?, dry)
            }
        }
        "preset.factory.restore" => restore(dir, catalog, &entries, a, dry),
        _ => Err(CmdError::not_found(format!("{name} は実装されていません"))),
    }
}

fn save(
    dir: &Path,
    song: Option<&Path>,
    project: &Project,
    entries: &[Entry],
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    let tid: TrackId = a.id("track")?;
    let kind = kind_arg(a, "kind")?;
    let name = preset_name(a, "name")?;
    let x = match a.opt_id::<PluginId>("of")? {
        Some(id) => plugin_on(project, tid, id)?,
        None if kind == PluginKind::Instrument => project
            .track(tid)?
            .inserts
            .iter()
            .find(|x| x.kind == PluginKind::Instrument)
            .ok_or_else(|| CmdError::not_found("このトラックにインストゥルメントがありません"))?,
        None => {
            return Err(CmdError::invalid(
                "--of にプリセットにするプラグインを指定してください",
            ));
        }
    };
    if x.kind != kind {
        return Err(CmdError::conflict(format!(
            "「{}」は{}ではありません",
            x.name,
            if kind == PluginKind::Effect {
                "エフェクト"
            } else {
                "インストゥルメント"
            },
        )));
    }
    let mut preset = DevicePreset::capture(x, &name, Some(now_ms()));
    // A sample is named from the song's folder; the preset keeps where it is, so another
    // song reads the same file.
    if let Some(samples) = preset.device.samples.as_mut() {
        for s in samples.iter_mut() {
            if let Some(p) = sample_path(s, song) {
                *s = p.to_string_lossy().into_owned();
            }
        }
    }
    let device = canon_uid(&x.format, &x.uid);
    let same: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.is(kind, &device, &name))
        .collect();
    let (own, to) = match same.as_slice() {
        [] => {
            let to = path_of(dir, kind, &device, &name);
            free(&to, None)?;
            (None, to)
        }
        [e] => {
            if !a.flag("overwrite") {
                return Err(CmdError::conflict(format!(
                    "プリセット「{}」はこの機器にもうあります（上書きするには --overwrite を付けてください）",
                    e.name
                )));
            }
            // At the file its name made, it follows the new spelling; a file the user named
            // keeps its name.
            let to = if e.path == path_of(dir, kind, &device, &e.name) {
                path_of(dir, kind, &device, &name)
            } else {
                e.path.clone()
            };
            free(&to, Some(&e.path))?;
            (Some(e.path.as_path()), to)
        }
        many => {
            // A save has no file to pick: one of them goes first, by its path.
            let files: Vec<String> = many.iter().map(|e| shown(&e.path)).collect();
            return Err(CmdError::conflict(format!(
                "「{name}」というプリセットがこの機器に {} 個あります（{}）。保存する前に、preset.remove か preset.rename の --preset にファイルのパスを指定して、1 つにしてください",
                many.len(),
                files.join("、")
            )));
        }
    };
    if !dry {
        put_file(own, &to, &to_bytes(&preset.to_value())?)?;
    }
    Ok((
        Output::with(
            format!(
                "プリセット「{name}」を保存{}",
                if dry { "します" } else { "しました" }
            ),
            json!({
                "name": name,
                "file": to.to_string_lossy(),
                "device": device,
                "kind": kind_name(kind),
            }),
        ),
        Vec::new(),
    ))
}

fn list(
    dir: &Path,
    catalog: &Catalog,
    entries: &[Entry],
    skipped: &[Skipped],
    a: &Args,
) -> CmdResult<(Output, Edits)> {
    let kind =
        match a.opt_str("kind") {
            Some(k) => Some(kind_of(k).ok_or_else(|| {
                CmdError::invalid("kind には fx か instrument を指定してください")
            })?),
            None => None,
        };
    let query = a.opt_str("query").unwrap_or_default().to_lowercase();
    let q = |n: &str| query.is_empty() || n.to_lowercase().contains(&query);
    // One list per device: `Default` first, then its files by name. Devices are the
    // built-ins plus whatever the files name; order is by the device's name.
    let mut devices: BTreeMap<String, (String, PluginKind, String)> = BTreeMap::new();
    for def in xirius_params::BUILTINS.iter().copied() {
        let dk = def_kind(def);
        if kind.is_some_and(|k| k != dk) {
            continue;
        }
        devices
            .entry(def.uid.to_string())
            .or_insert_with(|| (def.name.to_string(), dk, def.uid.to_string()));
    }
    for e in entries {
        if kind.is_some_and(|k| k != e.kind) {
            continue;
        }
        devices
            .entry(e.device())
            .or_insert_with(|| (e.device_name.clone(), e.kind, e.device_uid.clone()));
    }
    let key = a.opt_str("device").map(|d| device_key(d, catalog, entries));
    if let Some(key) = &key {
        devices.retain(|k, _| k == key);
        if devices.is_empty() {
            return Err(CmdError::not_found(format!(
                "機器「{}」が見つかりません",
                a.opt_str("device").unwrap_or_default()
            )));
        }
    }
    let mut presets = Vec::new();
    let mut names: Vec<(&String, &(String, PluginKind, String))> = devices.iter().collect();
    names.sort_by(|a, b| {
        a.1.0
            .to_lowercase()
            .cmp(&b.1.0.to_lowercase())
            .then_with(|| a.0.cmp(b.0))
    });
    for (uid, (dev_name, dk, dev_uid)) in names {
        if q(DEFAULT_PRESET) {
            presets.push(json!({
                "name": DEFAULT_PRESET,
                "kind": kind_name(*dk),
                "device": dev_uid,
                "device_name": dev_name,
                "default": true,
            }));
        }
        let mut es: Vec<&Entry> = entries
            .iter()
            .filter(|e| e.device() == *uid && e.kind == *dk)
            .filter(|e| q(&e.name))
            .collect();
        es.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.path.cmp(&b.path))
        });
        for e in es {
            let mut r = Map::new();
            r.insert("name".into(), json!(e.name));
            r.insert("kind".into(), json!(kind_name(e.kind)));
            r.insert("device".into(), json!(e.device_uid));
            r.insert("device_name".into(), json!(e.device_name));
            r.insert("file".into(), json!(e.path.to_string_lossy()));
            if let Some(at) = e.saved_at {
                r.insert("saved_at".into(), json!(at));
            }
            presets.push(Value::Object(r));
        }
    }
    // The files that could not be read, in the folders the list covers.
    let kinds: Vec<PluginKind> = kind.map_or_else(
        || vec![PluginKind::Effect, PluginKind::Instrument],
        |k| vec![k],
    );
    let bad: Vec<Value> = skipped
        .iter()
        .filter(|s| {
            kinds.iter().any(|k| {
                let kd = dir.join(kind_name(*k));
                match &key {
                    Some(key) => s.path.parent() == Some(kd.join(file_name(key)).as_path()),
                    None => s.path.starts_with(&kd),
                }
            })
        })
        .map(|s| json!({ "file": s.path.to_string_lossy(), "reason": s.reason }))
        .collect();
    let mut message = format!("プリセットは {} 個です", presets.len());
    if !bad.is_empty() {
        let _ = write!(message, "。読めないファイルが {} 個あります", bad.len());
    }
    let mut data = json!({ "presets": presets, "dir": dir.to_string_lossy() });
    if !bad.is_empty() {
        data["skipped"] = json!(bad);
    }
    Ok((Output::with(message, data), Vec::new()))
}

fn load(
    song: Option<&Path>,
    project: &Project,
    catalog: &Catalog,
    entries: &[Entry],
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    let tid: TrackId = a.id("track")?;
    project.track(tid)?;
    let mut of: Option<PluginId> = a.opt_id("of")?;
    let spec = a.str("preset")?.trim();
    let ofx = match of {
        Some(id) => Some(plugin_on(project, tid, id)?),
        None => None,
    };
    let device = match ofx {
        Some(x) => Some(canon_uid(&x.format, &x.uid)),
        None => a.opt_str("device").map(|d| device_key(d, catalog, entries)),
    };
    let (mut value, preset, mut rows) = if is_default(spec) {
        let x = match (ofx, &device) {
            (Some(x), _) => x,
            (None, Some(d)) => project
                .track(tid)?
                .inserts
                .iter()
                .find(|x| canon_uid(&x.format, &x.uid) == *d)
                .ok_or_else(|| {
                    CmdError::not_found(format!(
                        "このトラックに機器「{}」がありません",
                        a.opt_str("device").unwrap_or(d)
                    ))
                })?,
            (None, None) => {
                return Err(CmdError::invalid(
                    "「Default」を読み込む機器を --of か --device で指定してください",
                ));
            }
        };
        of = Some(x.id);
        let p = DevicePreset::factory_default(x);
        (p.to_value(), p, Vec::new())
    } else {
        let mut found = find_in(entries, spec, device.as_deref());
        // A name every device has (a factory preset's, say) is still clear when the track
        // holds one of those devices: that device's file is meant.
        if device.is_none() && found.len() > 1 {
            let on_track: Vec<String> = project
                .track(tid)?
                .inserts
                .iter()
                .map(|x| canon_uid(&x.format, &x.uid))
                .collect();
            let here: Vec<&Entry> = found
                .iter()
                .copied()
                .filter(|e| on_track.contains(&e.device()))
                .collect();
            if here.len() == 1 {
                found = here;
            }
        }
        let (path, fallback) = match found.as_slice() {
            [one] => (one.path.clone(), one.name.clone()),
            [] if looks_like_path(spec) => {
                let p = path_arg(spec)?;
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "プリセット".into());
                (p, stem)
            }
            [] => {
                return Err(CmdError::not_found(format!(
                    "プリセット「{spec}」が見つかりません"
                )));
            }
            many => return Err(several(spec, many, "--of か --device ")),
        };
        if std::fs::metadata(&path).map_err(io)?.len() > MAX_PRESET_BYTES {
            return Err(CmdError::invalid("プリセットのファイルが大きすぎます"));
        }
        let v: Value = serde_json::from_slice(&std::fs::read(&path).map_err(io)?)
            .map_err(|e| CmdError::invalid(format!("プリセットを読み込めません: {e}")))?;
        let (preset, rows) = DevicePreset::parse(&v, Some(&fallback))?;
        (v, preset, rows)
    };
    // A sample that is not on the disk any more is left out, and said so.
    let lost: Vec<String> = preset
        .device
        .samples
        .iter()
        .flatten()
        .filter(|s| !sample_path(s, song).is_some_and(|p| p.is_file()))
        .cloned()
        .collect();
    if !lost.is_empty()
        && let Some(list) = value
            .pointer_mut("/devices/0/samples")
            .and_then(Value::as_array_mut)
    {
        list.retain(|s| !s.as_str().is_some_and(|s| lost.iter().any(|l| l == s)));
    }
    let missed = !preset.device.format.eq_ignore_ascii_case("builtin")
        && !catalog.entries.is_empty()
        && catalog.find(&preset.device.uid).is_none();
    let pname = preset.name.clone();
    let mut args = json!({
        "track": tid.to_string(),
        "preset": value.to_string(),
        "name": pname,
        "missing": if missed { preset.device.uid.clone() } else { String::new() },
    });
    if !lost.is_empty() {
        args["lost"] = json!(json!(lost).to_string());
    }
    if let Some(id) = of {
        args["of"] = json!(id.to_string());
    } else if device.is_some() {
        // A device named by --device and on the track takes it, as with --of.
        if let Some(x) = project
            .track(tid)?
            .inserts
            .iter()
            .find(|x| x.kind == preset.kind && Some(canon_uid(&x.format, &x.uid)) == device)
        {
            args["of"] = json!(x.id.to_string());
        }
    }
    let out = if dry {
        for s in &lost {
            rows.push(xirius_core::track_preset::Adjust::new(
                xirius_core::track_preset::AdjustKind::Rejected,
                s.as_str(),
                "サンプルが見つからないため外しました",
            ));
        }
        Output::with(
            format!("プリセット「{pname}」を読み込みます"),
            json!({ "adjusted": rows }),
        )
    } else {
        Output::msg("")
    };
    let edits = if dry {
        Vec::new()
    } else {
        vec![("preset.put".into(), args.as_object().unwrap().clone())]
    };
    Ok((out, edits))
}

/// The answer of a removal: of the preset `name`, or of a file by its path.
fn removed(name: Option<&str>, path: &Path, dry: bool) -> Output {
    let verb = if dry { "します" } else { "しました" };
    Output::with(
        match name {
            Some(n) => format!("プリセット「{n}」を削除{verb}"),
            None => format!("プリセットのファイル {} を削除{verb}", shown(path)),
        },
        json!({ "file": path.to_string_lossy() }),
    )
}

/// Renames the preset `e` to `new`, in its folder: refused when the device has a preset of
/// that name already, or another file is at the place the name makes.
fn rename(entries: &[Entry], e: &Entry, new: &str, dry: bool) -> CmdResult<(Output, Edits)> {
    let device = e.device();
    if entries
        .iter()
        .any(|x| x.is(e.kind, &device, new) && !same_file(&x.path, &e.path))
    {
        return Err(CmdError::conflict(format!(
            "プリセット「{new}」はこの機器にもうあります"
        )));
    }
    let to = e.path.with_file_name(format!("{}.xpreset", file_name(new)));
    free(&to, Some(&e.path))?;
    if !dry {
        let mut v = e.value.clone();
        v["name"] = json!(new);
        put_file(Some(&e.path), &to, &to_bytes(&v)?)?;
    }
    Ok((
        Output::with(
            format!(
                "プリセット「{}」を「{new}」に名前を変え{}",
                e.name,
                if dry { "ます" } else { "ました" }
            ),
            json!({ "file": to.to_string_lossy(), "name": new }),
        ),
        Vec::new(),
    ))
}

fn restore(
    dir: &Path,
    catalog: &Catalog,
    entries: &[Entry],
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    let device = a.opt_str("device").map(|d| device_key(d, catalog, entries));
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    let mut records = Vec::new();
    for (def, name, data) in factory_set() {
        if device.as_ref().is_some_and(|d| *d != def.uid) {
            continue;
        }
        let kind = def_kind(def);
        if entries.iter().any(|e| e.is(kind, def.uid, name)) {
            continue;
        }
        let path = path_of(dir, kind, def.uid, name);
        if std::fs::symlink_metadata(&path).is_ok() {
            skipped.push(json!({
                "file": path.to_string_lossy(),
                "reason": "別のプリセットか読めないファイルがあるため、足しませんでした",
            }));
            continue;
        }
        added.push(format!("{}: {name}", def.name));
        if !dry {
            atomic_write(&path, &data)?;
            records.push(record(dir, kind, def.uid, name, &path, &data));
        }
    }
    // What was copied goes into the mark, so a newer set can tell it from the user's.
    if !records.is_empty()
        && let Some((version, mut files)) = mark(dir)
    {
        files.retain(|r| {
            !records.iter().any(|n| {
                r["kind"] == n["kind"] && r["device"] == n["device"] && {
                    let a = r["name"].as_str().map(fold);
                    a.is_some() && a == n["name"].as_str().map(fold)
                }
            })
        });
        files.extend(records);
        write_mark(dir, version, &files)?;
    }
    let mut message = if added.is_empty() {
        "足すプリセットはありませんでした".to_string()
    } else {
        format!(
            "内蔵のプリセットを {} 件足し{}",
            added.len(),
            if dry { "ます" } else { "ました" }
        )
    };
    if !skipped.is_empty() {
        let _ = write!(
            message,
            "。{} 件は同じ名前のファイルがあるため足しませんでした",
            skipped.len()
        );
    }
    Ok((
        Output::with(message, json!({ "added": added, "skipped": skipped })),
        Vec::new(),
    ))
}
