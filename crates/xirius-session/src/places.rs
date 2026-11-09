//! What the browser keeps between runs (BRW-001, BRW-005, BRW-012, BRW-015): the favourite and
//! hidden instruments and effects, whether hidden ones are listed, and the places the file tab
//! has a tab for. They are one JSON file of the user's (`XIRIUS_BROWSER_FILE`, otherwise
//! `browser.json` next to the plugin cache), written by the screen and by the commands
//! `browser.favorite`, `browser.hide`, `view.browser.hidden`, `location.list`, `location.add`,
//! `location.rename` and `location.remove`.
//!
//! The song's own folder is always the first tab, 「この曲」: it is not in the file, so it
//! cannot be renamed or taken away.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xirius_core::{Args, CmdError, CmdResult, Output};

/// The tab of the song's folder.
pub const SONG: &str = "この曲";

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 7] = [
    "browser.favorite",
    "browser.hide",
    "view.browser.hidden",
    "location.list",
    "location.add",
    "location.rename",
    "location.remove",
];

/// One read/modify/write boundary shared by command and browser writer owners.
/// Call only on a worker: a stopped transaction must never hold a UI callback.
pub fn transaction<T>(
    file: &Path,
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    type Lanes = std::collections::HashMap<PathBuf, Weak<Mutex<()>>>;
    static LANES: OnceLock<Mutex<Lanes>> = OnceLock::new();
    let key = std::path::absolute(file).unwrap_or_else(|_| file.to_owned());
    let lane = {
        let mut lanes = LANES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        lanes.retain(|_, lane| lane.strong_count() != 0);
        if let Some(lane) = lanes.get(&key).and_then(Weak::upgrade) {
            lane
        } else {
            let lane = Arc::new(Mutex::new(()));
            lanes.insert(key, Arc::downgrade(&lane));
            lane
        }
    };
    let _guard = lane.lock().unwrap_or_else(|e| e.into_inner());
    // Keep a stable lock file: replacing the JSON must not replace the lock held
    // by another process. The process-local lane also serializes shared handles.
    let lock = xirius_core::store::open_lock(&file.with_extension("json.lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    operation()
}

/// A folder with a tab of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub path: PathBuf,
}

/// What the browser keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// The obsolete preset-browser store has been imported exactly once.
    pub legacy_favourites_imported: bool,
    pub favourites: BTreeSet<String>,
    /// Left out of the lists; a search still finds them.
    pub hidden: BTreeSet<String>,
    /// The lists show the hidden ones too.
    pub show_hidden: bool,
    pub places: Vec<Place>,
}

impl Default for Prefs {
    /// What a first run starts with ([`Prefs::first_run`] in the user's home folder).
    fn default() -> Prefs {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        Prefs::first_run(home.as_deref())
    }
}

impl Prefs {
    /// Three favourites, and tabs for the downloads and the music in `home` when it has them.
    pub fn first_run(home: Option<&Path>) -> Prefs {
        let places = home
            .map(|h| {
                ["Downloads", "Music"]
                    .into_iter()
                    .map(|n| Place {
                        name: n.to_string(),
                        path: h.join(n),
                    })
                    .filter(|p| p.path.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        Prefs {
            legacy_favourites_imported: false,
            favourites: ["Sampler", "EQ", "Vital"].map(String::from).into(),
            hidden: BTreeSet::new(),
            show_hidden: false,
            places,
        }
    }
}

/// Where the browser's file is.
pub fn path() -> PathBuf {
    std::env::var_os("XIRIUS_BROWSER_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| xirius_plugins::default_cache_path().with_file_name("browser.json"))
}

fn names(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The file at `path`; what a first run starts with when there is none.
pub fn load(path: &Path) -> Result<Prefs, String> {
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("prefs_read", path).map_err(|e| e.to_string())?;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Prefs::default()),
        Err(e) => return Err(format!("{} を読めません: {e}", path.display())),
    };
    let v: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} を読み取れません: {e}", path.display()))?;
    let places = v["places"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(Place {
                        name: p["name"].as_str()?.to_string(),
                        path: PathBuf::from(p["path"].as_str()?),
                    })
                })
                .filter(|p| p.name != SONG)
                .collect()
        })
        .unwrap_or_default();
    Ok(Prefs {
        legacy_favourites_imported: v["legacy_favourites_imported"].as_bool().unwrap_or(false),
        favourites: names(&v["favourites"]),
        hidden: names(&v["hidden"]),
        show_hidden: v["show_hidden"].as_bool().unwrap_or(false),
        places,
    })
}

/// Writes `prefs` to `path` (through a file beside it, so a crash leaves the old one).
pub fn save(path: &Path, prefs: &Prefs) -> Result<(), String> {
    #[cfg(feature = "test-io")]
    xirius_core::io_test::before("prefs_write", path).map_err(|e| e.to_string())?;
    let v = json!({
        "legacy_favourites_imported": prefs.legacy_favourites_imported,
        "favourites": prefs.favourites,
        "hidden": prefs.hidden,
        "show_hidden": prefs.show_hidden,
        "places": prefs.places.iter().map(|p| json!({
            "name": p.name,
            "path": p.path.to_string_lossy(),
        })).collect::<Vec<_>>(),
    });
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, serde_json::to_string_pretty(&v)? + "\n")?;
        std::fs::rename(&part, path)
    };
    write().map_err(|e| format!("{} に書けません: {e}", path.display()))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    #[cfg(feature = "test-io")]
    let _ = xirius_core::io_test::before("place_canonicalize", a);
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

impl Prefs {
    /// The place named `which` (its tab's name, or its folder).
    pub fn find(&self, which: &str) -> Option<usize> {
        let which = which.trim();
        self.places
            .iter()
            .position(|p| p.name == which)
            .or_else(|| {
                self.places
                    .iter()
                    .position(|p| same_dir(&p.path, Path::new(which)))
            })
    }

    fn free_name(&self, want: &str) -> String {
        let taken = |n: &str| n == SONG || self.places.iter().any(|p| p.name == n);
        if !taken(want) {
            return want.to_string();
        }
        (2..)
            .map(|i| format!("{want} {i}"))
            .find(|n| !taken(n))
            .expect("some number is free")
    }

    /// Adds a tab for the folder `dir` at the end; returns its name. A folder that has a tab
    /// already keeps it.
    pub fn add_place(&mut self, dir: &Path, name: Option<&str>) -> Result<String, String> {
        #[cfg(feature = "test-io")]
        xirius_core::io_test::before("place_validate", dir).map_err(|e| e.to_string())?;
        if !dir.is_dir() {
            return Err(format!("フォルダーが見つかりません: {}", dir.display()));
        }
        if let Some(p) = self.places.iter().find(|p| same_dir(&p.path, dir)) {
            return Err(format!("「{}」はもう場所のタブにあります", p.name));
        }
        let want = match name.map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.to_string_lossy().into_owned()),
        };
        let name = self.free_name(&want);
        self.places.push(Place {
            name: name.clone(),
            path: dir.to_path_buf(),
        });
        Ok(name)
    }

    pub fn rename_place(&mut self, which: &str, name: &str) -> Result<(), String> {
        let i = self.place_index(which)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("名前が空です".into());
        }
        if name == SONG
            || self
                .places
                .iter()
                .enumerate()
                .any(|(j, p)| j != i && p.name == name)
        {
            return Err(format!("「{name}」という場所はもうあります"));
        }
        self.places[i].name = name.to_string();
        Ok(())
    }

    pub fn remove_place(&mut self, which: &str) -> Result<Place, String> {
        let i = self.place_index(which)?;
        Ok(self.places.remove(i))
    }

    fn place_index(&self, which: &str) -> Result<usize, String> {
        if which.trim() == SONG {
            return Err("曲のフォルダー（この曲）は外したり名前を変えたりできません".into());
        }
        self.find(which)
            .ok_or_else(|| format!("場所「{which}」が見つかりません"))
    }
}

fn listed(prefs: &Prefs, song: Option<&Path>) -> Value {
    let mut out = vec![json!({
        "name": SONG,
        "path": song.map(|d| d.to_string_lossy().into_owned()),
        "song": true,
    })];
    out.extend(prefs.places.iter().map(|p| {
        json!({
            "name": p.name,
            "path": p.path.to_string_lossy(),
            "song": false,
            "missing": !p.path.is_dir(),
        })
    }));
    Value::Array(out)
}

/// Runs one of [`NAMES`] on the file at `file`; `song` is the song's folder.
pub fn run(
    file: &Path,
    song: Option<&Path>,
    name: &str,
    args: &Args,
    dry: bool,
) -> CmdResult<Output> {
    run_with_legacy(file, None, song, name, args, dry)
}

/// Run a browser command against its canonical store, importing legacy data first.
pub fn run_with_legacy(
    file: &Path,
    legacy: Option<&Path>,
    song: Option<&Path>,
    name: &str,
    args: &Args,
    dry: bool,
) -> CmdResult<Output> {
    if dry {
        let prefs = load_migrated(file, legacy, false).map_err(CmdError::io)?;
        return run_locked(file, prefs, song, name, args, true);
    }
    transaction(file, || {
        let prefs = load_migrated(file, legacy, !dry)?;
        Ok(run_locked(file, prefs, song, name, args, dry))
    })
    .map_err(CmdError::io)?
}

/// Unambiguous legacy display names across built-in and scanned devices.
pub fn device_names(
    catalog: &xirius_plugins::Catalog,
) -> std::collections::BTreeMap<String, String> {
    let mut candidates = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
    for (name, uid) in xirius_params::BUILTINS
        .iter()
        .map(|d| (d.name, d.uid))
        .chain(
            catalog
                .entries
                .iter()
                .map(|e| (e.name.as_str(), e.uid.as_str())),
        )
    {
        candidates
            .entry(name.into())
            .or_default()
            .insert(format!("device:{uid}"));
    }
    candidates
        .into_iter()
        .filter(|(_, ids)| ids.len() == 1)
        .map(|(name, ids)| (name, ids.into_iter().next().unwrap()))
        .collect()
}

/// Resolve legacy names only when the complete device catalog makes them unique.
pub(crate) fn device_args(args: &Args, catalog: &xirius_plugins::Catalog) -> Args {
    let mut resolved = args.clone();
    let Some(item) = args.opt_str("item") else {
        return resolved;
    };
    let names = device_names(catalog);
    let id = names
        .get(item)
        .or_else(|| xirius_params::find(item).and_then(|def| names.get(def.name)))
        .map(String::as_str)
        .unwrap_or(item);
    let aliases: Vec<_> = names
        .iter()
        .filter(|(_, key)| key.as_str() == id)
        .map(|(name, _)| name)
        .collect();
    if args.opt_str("label").is_none() {
        let label = xirius_params::BUILTINS
            .iter()
            .find(|d| format!("device:{}", d.uid) == id)
            .map(|d| d.name)
            .or_else(|| {
                catalog
                    .entries
                    .iter()
                    .find(|e| format!("device:{}", e.uid) == id)
                    .map(|e| e.name.as_str())
            });
        resolved.0.insert("label".into(), json!(label));
    }
    resolved.0.insert("item".into(), json!(id));
    resolved.0.insert("aliases".into(), json!(aliases));
    resolved
}

/// Import the former store before applying any edit. Call inside a transaction.
pub fn load_migrated(file: &Path, legacy: Option<&Path>, persist: bool) -> Result<Prefs, String> {
    let mut prefs = load(file)?;
    if prefs.legacy_favourites_imported {
        return Ok(prefs);
    }
    let Some(legacy) = legacy else {
        return Ok(prefs);
    };
    let imported = match std::fs::read(legacy) {
        Ok(bytes) => {
            let old: BTreeSet<String> =
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", legacy.display()))?;
            prefs.favourites.extend(old);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("{}: {e}", legacy.display())),
    };
    prefs.legacy_favourites_imported = true;
    if persist && (imported || file.try_exists().map_err(|e| e.to_string())?) {
        save(file, &prefs)?;
    }
    Ok(prefs)
}

fn run_locked(
    file: &Path,
    mut prefs: Prefs,
    song: Option<&Path>,
    name: &str,
    args: &Args,
    dry: bool,
) -> CmdResult<Output> {
    let message = match name {
        "location.list" => {
            let n = prefs.places.len() + 1;
            return Ok(Output::with(
                format!("場所のタブは {n} 個です"),
                json!({ "locations": listed(&prefs, song) }),
            ));
        }
        "browser.favorite" | "browser.hide" => {
            let mut item = args.str("item")?.trim().to_string();
            if item.is_empty() {
                return Err(CmdError::invalid("--item が空です"));
            }
            let fav = name == "browser.favorite";
            let label = args
                .opt_str("label")
                .or_else(|| {
                    xirius_params::find(item.strip_prefix("device:").unwrap_or(&item))
                        .map(|def| def.name)
                })
                .or_else(|| (!fav).then_some(item.as_str()));
            let subject = label.map_or_else(String::new, |label| format!("「{label}」を"));
            let mut aliases = if fav {
                names(args.0.get("aliases").unwrap_or(&Value::Null))
            } else {
                BTreeSet::new()
            };
            if fav
                && !args.0.contains_key("aliases")
                && let Some(def) =
                    xirius_params::find(item.strip_prefix("device:").unwrap_or(&item))
            {
                aliases.insert(def.name.to_string());
                item = format!("device:{}", def.uid);
            }
            let set = if fav {
                &mut prefs.favourites
            } else {
                &mut prefs.hidden
            };
            let on = args
                .toggle()?
                .unwrap_or(!(set.contains(&item) || aliases.iter().any(|key| set.contains(key))));
            for alias in aliases {
                set.remove(&alias);
            }
            if on {
                set.insert(item.clone());
            } else {
                set.remove(&item);
            }
            match (fav, on) {
                (true, true) => format!("{subject}お気に入りにしました"),
                (true, false) => format!("{subject}お気に入りから外しました"),
                (false, true) => format!("{subject}隠しました（検索では見つかります）"),
                (false, false) => format!("{subject}再び表示しました"),
            }
        }
        "view.browser.hidden" => {
            let on = args.toggle()?.unwrap_or(!prefs.show_hidden);
            prefs.show_hidden = on;
            if on {
                format!("隠したもの（{} 個）も一覧に表示します", prefs.hidden.len())
            } else {
                "隠したものを一覧に表示しません".to_string()
            }
        }
        "location.add" => {
            let dir = PathBuf::from(args.str("path")?.trim());
            let added = prefs
                .add_place(&dir, args.opt_str("name"))
                .map_err(CmdError::invalid)?;
            format!("場所のタブ「{added}」を足しました")
        }
        "location.rename" => {
            let which = args.str("location")?;
            let to = args.str("name")?;
            prefs.rename_place(which, to).map_err(CmdError::invalid)?;
            format!("場所「{which}」の名前を「{}」にしました", to.trim())
        }
        "location.remove" => {
            let gone = prefs
                .remove_place(args.str("location")?)
                .map_err(CmdError::invalid)?;
            format!(
                "場所のタブ「{}」を外しました（フォルダーはそのままです）",
                gone.name
            )
        }
        other => return Err(CmdError::invalid(format!("{other} はここでは扱えません"))),
    };
    let data = json!({
        "favourites": prefs.favourites,
        "hidden": prefs.hidden,
        "show_hidden": prefs.show_hidden,
        "locations": listed(&prefs, song),
        "dry_run": dry,
    });
    if dry {
        return Ok(Output::with(format!("（試行）{message}"), data));
    }
    save(file, &prefs).map_err(CmdError::io)?;
    Ok(Output::with(message, data))
}

#[cfg(test)]
mod tests;
