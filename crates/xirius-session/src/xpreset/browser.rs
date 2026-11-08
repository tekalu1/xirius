//! File-backed operations for the shared track browser. Called by the song owner.
use super::*;
use std::collections::BTreeSet;
use xirius_core::track_browser::{Category, Entry as Indexed};
use xirius_core::track_preset::TrackPreset;

pub const NAMES: &[&str] = &[
    "browser.preset.apply",
    "browser.preset.list",
    "browser.preset.favourite",
    "browser.preset.save",
    "browser.preset.copy",
];

struct Item {
    entry: Indexed,
    value: Value,
}

fn describe(
    value: Value,
    id: String,
    file: Option<String>,
    source: &str,
    folder: Vec<String>,
    catalog: &Catalog,
) -> CmdResult<Item> {
    let (name, category, device, device_name, has_fx) = if let Some(kind) = kind_of_value(&value) {
        let (p, _) = ChainPreset::parse(&value, None)?;
        let instrument = p
            .chain
            .devices
            .iter()
            .find(|d| d.kind == PluginKind::Instrument);
        let cat = match kind {
            ChainKind::Instrument => Category::Instrument,
            ChainKind::Multi => Category::MultiInstrument,
            ChainKind::Fx => Category::FxChain,
        };
        (
            p.name,
            cat,
            instrument.map(|d| d.uid.clone()).unwrap_or_default(),
            instrument.map(|d| d.name.clone()).unwrap_or_default(),
            !p.chain.rack.fx.is_empty(),
        )
    } else if let Ok((p, _)) = DevicePreset::parse(&value, None) {
        (
            p.name,
            if p.kind == PluginKind::Instrument {
                Category::Instrument
            } else {
                Category::Fx
            },
            p.device.uid,
            p.device.name,
            false,
        )
    } else {
        let (p, _) = TrackPreset::parse(&value)?;
        (p.name, Category::Track, String::new(), String::new(), false)
    };
    let vendor = if xirius_params::find(&device).is_some() {
        "Xirius".into()
    } else {
        catalog
            .find(&device)
            .map(|e| e.vendor.clone())
            .unwrap_or_default()
    };
    Ok(Item {
        entry: Indexed {
            id,
            name,
            category,
            device,
            device_name,
            vendor,
            source: source.into(),
            folder,
            file,
            has_fx,
        },
        value,
    })
}

fn scan(root: &Path, at: &Path, catalog: &Catalog, out: &mut Vec<Item>, depth: usize) {
    if depth > 16 {
        return;
    }
    for e in std::fs::read_dir(at).into_iter().flatten().flatten() {
        let Ok(t) = e.file_type() else {
            continue;
        };
        let p = e.path();
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            scan(root, &p, catalog, out, depth + 1);
            continue;
        }
        if !p.extension().is_some_and(|s| s == "xpreset" || s == "json") {
            continue;
        }
        if !e.metadata().is_ok_and(|m| m.len() <= MAX_PRESET_BYTES) {
            continue;
        }
        let Some(v) = std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        else {
            continue;
        };
        let file = p.to_string_lossy().into_owned();
        let folder = p
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.iter().map(|s| s.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let folder = v
            .get("browser_folder")
            .and_then(Value::as_array)
            .map(|v| {
                v.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or(folder);
        if let Ok(item) = describe(
            v,
            format!("preset:{file}"),
            Some(file),
            "User",
            folder,
            catalog,
        ) {
            out.push(item);
        }
    }
}

fn index(dir: &Path, tracks: &Path, catalog: &Catalog) -> Vec<Item> {
    let mut out = Vec::new();
    scan(dir, dir, catalog, &mut out, 0);
    scan(tracks, tracks, catalog, &mut out, 0);
    for (def, name, bytes) in factory_set() {
        if let Ok(v) = serde_json::from_slice(&bytes)
            && let Ok(item) = describe(
                v,
                format!("factory:{}/{}", def.uid, name),
                Some(format!("factory:{}/{}", def.uid, name)),
                "Factory",
                Vec::new(),
                catalog,
            )
        {
            out.push(item);
        }
    }
    out
}

fn favourites(dir: &Path) -> BTreeSet<String> {
    std::fs::read(dir.join(".browser-favourites.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn destination(dir: &Path, a: &Args) -> CmdResult<PathBuf> {
    if a.opt_str("source").is_some_and(|s| s != "User") {
        return Err(CmdError::invalid("保存先は User を選んでください"));
    }
    let folder = a.opt_str("folder").unwrap_or_default();
    let rel = Path::new(folder);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(CmdError::invalid(
            "保存先は User 内のフォルダーを指定してください",
        ));
    }
    let path = dir.join("library").join(rel);
    // Reject junctions and symbolic links so a User save cannot escape its root.
    let mut check = dir.to_path_buf();
    for part in Path::new("library").components().chain(rel.components()) {
        check.push(part);
        if std::fs::symlink_metadata(&check).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(CmdError::invalid("リンク先には保存できません"));
        }
    }
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run(
    dir: &Path,
    tracks: &Path,
    song: Option<&Path>,
    project: &Project,
    catalog: &Catalog,
    command: &str,
    a: &Args,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    let items = index(dir, tracks, catalog);
    let mut fav = favourites(dir);
    if command == "browser.preset.apply" {
        let item = items
            .iter()
            .find(|i| i.entry.id == a.opt_str("entry").unwrap_or_default())
            .ok_or_else(|| CmdError::not_found("プリセットが見つかりません"))?;
        let target = a.opt_str("target").unwrap_or("track");
        let effect = item.entry.category.tab() == "effects";
        if (target == "instrument" && (effect || item.entry.category == Category::Track))
            || (matches!(target, "insert" | "effect") && !effect)
        {
            return Err(CmdError::invalid("この場所には適用できません"));
        }
        let mut value = item.value.clone();
        resolve_samples(&mut value, dir);
        let mut args = json!({"preset": value.to_string()});
        let edit = if target == "new" {
            let preset = as_track(&value)?;
            args["preset"] = json!(preset.to_value().to_string());
            "track.preset.make"
        } else if item.entry.category == Category::Track {
            args["ids"] = json!([a.str("track")?]);
            "track.preset.put"
        } else {
            args["track"] = json!(a.str("track")?);
            if effect {
                if target == "effect" {
                    args["at"] = json!(format!("replace:{}", a.str("of")?));
                } else {
                    args["at"] = json!(a.opt_str("at").unwrap_or("track"));
                }
            }
            "preset.put"
        };
        let edits = if dry {
            Vec::new()
        } else {
            vec![(edit.into(), args.as_object().unwrap().clone())]
        };
        return Ok((
            Output::with(
                if item.entry.has_fx {
                    "インストゥルメントと FX を置き換え"
                } else {
                    "プリセットを適用"
                },
                json!({"entry": item.entry.id}),
            ),
            edits,
        ));
    }
    if command == "browser.preset.list" {
        let mut entries: Vec<_> = items.into_iter().map(|i| i.entry).collect();
        for def in xirius_params::BUILTINS.iter().copied() {
            entries.push(Indexed {
                id: format!("device:{}", def.uid),
                name: def.name.into(),
                category: if def_kind(def) == PluginKind::Instrument {
                    Category::Instrument
                } else {
                    Category::Fx
                },
                device: def.uid.into(),
                device_name: def.name.into(),
                vendor: "Xirius".into(),
                source: String::new(),
                folder: Vec::new(),
                file: None,
                has_fx: false,
            });
        }
        for e in &catalog.entries {
            entries.push(Indexed {
                id: format!("device:{}", e.uid),
                name: e.name.clone(),
                category: if e.kind == PluginKind::Instrument {
                    Category::Instrument
                } else {
                    Category::Fx
                },
                device: e.uid.clone(),
                device_name: e.name.clone(),
                vendor: e.vendor.clone(),
                source: String::new(),
                folder: Vec::new(),
                file: None,
                has_fx: false,
            });
        }
        let query = a.opt_str("query").unwrap_or_default();
        let tab = a.opt_str("tab").unwrap_or("instruments");
        let rows = xirius_core::track_browser::tree(&entries, tab, query, &BTreeSet::new(), &fav);
        return Ok((
            Output::with(
                "トラックブラウザー",
                json!({"entries": entries, "rows": rows, "favourites": fav}),
            ),
            Vec::new(),
        ));
    }
    if command == "browser.preset.favourite" {
        let id = a.str("entry")?;
        if !fav.remove(id) {
            fav.insert(id.into());
        }
        if !dry {
            std::fs::create_dir_all(dir).map_err(io)?;
            atomic_write(
                &dir.join(".browser-favourites.json"),
                &to_bytes(&json!(fav))?,
            )?;
        }
        return Ok((
            Output::with("お気に入りを変更しました", json!({"favourites": fav})),
            Vec::new(),
        ));
    }
    let name = preset_name(a, "name")?;
    let mut value = if command == "browser.preset.copy" {
        items
            .iter()
            .find(|i| i.entry.id == a.opt_str("entry").unwrap_or_default())
            .ok_or_else(|| CmdError::not_found("プリセットが見つかりません"))?
            .value
            .clone()
    } else {
        let tid: TrackId = a.id("track")?;
        let kind = a.opt_str("kind").unwrap_or("instrument");
        if kind == "track" {
            TrackPreset::snapshot(project, tid, &name)?.to_value()
        } else if let Some(kind) = ChainKind::of(kind).or_else(|| {
            (kind == "instrument" && a.flag("include_fx")).then_some(ChainKind::Instrument)
        }) {
            let (mut chain, _) = Chain::capture(project.track(tid)?, kind, a.opt_id("of")?)?;
            for x in &mut chain.devices {
                for s in &mut x.samples {
                    if let Some(p) = sample_path(s, song).filter(|p| p.is_file()) {
                        *s = keep_sample(dir, &p, dry)?;
                    }
                }
            }
            ChainPreset {
                name: name.clone(),
                kind,
                saved_at: Some(now_ms()),
                chain,
            }
            .to_value()
        } else {
            let x = plugin_on(project, tid, a.id("of")?)?;
            let mut preset = DevicePreset::capture(x, &name, Some(now_ms()));
            if let Some(samples) = preset.device.samples.as_mut() {
                for s in samples {
                    if let Some(p) = sample_path(s, song).filter(|p| p.is_file()) {
                        *s = keep_sample(dir, &p, dry)?;
                    }
                }
            }
            preset.to_value()
        }
    };
    value["name"] = json!(name);
    value["browser_folder"] = json!(
        Path::new(a.opt_str("folder").unwrap_or_default())
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    );
    let item = describe(
        value.clone(),
        String::new(),
        None,
        "User",
        Vec::new(),
        catalog,
    )?;
    let at = destination(dir, a)?
        .join(format!("{:?}", item.entry.category))
        .join(file_name(&item.entry.device));
    let path = at.join(format!("{}.xpreset", file_name(&name)));
    if path.exists() && !a.flag("overwrite") {
        return Err(CmdError::conflict(
            "同じ名前があります。上書き保存を選んでください",
        ));
    }
    if !dry {
        std::fs::create_dir_all(&at).map_err(io)?;
        atomic_write(&path, &to_bytes(&value)?)?;
    }
    Ok((
        Output::with(
            "プリセットを保存しました",
            json!({"file": path.to_string_lossy(), "name": name, "has_fx": item.entry.has_fx}),
        ),
        Vec::new(),
    ))
}

fn as_track(value: &Value) -> CmdResult<TrackPreset> {
    if let Ok((p, _)) = TrackPreset::parse(value) {
        return Ok(p);
    }
    let (name, chain) = if kind_of_value(value).is_some() {
        let (p, _) = ChainPreset::parse(value, None)?;
        (p.name, p.chain)
    } else {
        let (p, _) = DevicePreset::parse(value, None)?;
        let d = p.device;
        let x = Plugin {
            id: PluginId(1),
            name: d.name,
            kind: d.kind,
            format: d.format,
            uid: d.uid,
            params: d.params,
            state: d.state,
            samples: d.samples.unwrap_or_default(),
            bypass: false,
            deactivated: false,
            isolation: Default::default(),
            sidechain: None,
            preset: Some(p.name.clone()),
        };
        let rack = xirius_core::rack::Rack {
            source: (x.kind == PluginKind::Instrument)
                .then_some(xirius_core::rack::Source::Single(x.id)),
            fx: if x.kind == PluginKind::Effect {
                vec![xirius_core::rack::RackNode::Plugin(x.id)]
            } else {
                Vec::new()
            },
        };
        (
            p.name,
            Chain {
                devices: vec![x],
                rack,
                controls: Default::default(),
            },
        )
    };
    Ok(TrackPreset {
        name,
        saved_at: None,
        kind: if chain.rack.source.is_some() {
            xirius_core::model::TrackKind::Instrument
        } else {
            xirius_core::model::TrackKind::Bus
        },
        color: None,
        volume_db: 0.0,
        pan: 0.0,
        inserts: chain.devices,
        sends: Vec::new(),
        rack: Some(chain.rack),
        controls: chain.controls,
    })
}

fn resolve_samples(value: &mut Value, dir: &Path) {
    match value {
        Value::Object(m) => {
            for (key, v) in m {
                if key == "samples" {
                    if let Some(samples) = v.as_array_mut() {
                        for s in samples {
                            if let Some(rel) = s.as_str().and_then(|s| s.strip_prefix(KEPT)) {
                                *s = json!(dir.join(rel).to_string_lossy());
                            }
                        }
                    }
                } else {
                    resolve_samples(v, dir);
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                resolve_samples(v, dir);
            }
        }
        _ => {}
    }
}
