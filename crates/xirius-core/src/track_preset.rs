//! A track preset (MIX-030): a channel's effects, sends, fader, pan, colour and instrument kept
//! under a name, to put on other tracks or to make a new track from.
//!
//! A preset file is read the way a song is (PRJ-007): what is missing is filled with the default,
//! a key this version does not know is left out, a value out of range is rounded to the nearest
//! one in range, and a part that points at nothing usable is turned down. Every one of these is
//! returned as an [`Adjust`] row, so a preset from another version or another PC never loads
//! without saying what became of it.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::error::{CmdError, CmdResult};
use crate::id::{PluginId, TrackId};
use crate::model::{MAX_DB, MIN_DB, Plugin, PluginIsolation, PluginKind, Project, TrackKind};

/// The version this writes. Newer files are read as far as they are understood.
pub const VERSION: u32 = 1;

const KEYS: [&str; 9] = [
    "version",
    "saved_at",
    "name",
    "kind",
    "color",
    "volume_db",
    "pan",
    "inserts",
    "sends",
];
const PLUGIN_KEYS: [&str; 10] = [
    "name",
    "kind",
    "format",
    "uid",
    "bypass",
    "deactivated",
    "params",
    "state",
    "samples",
    "isolation",
];
const SEND_KEYS: [&str; 4] = ["to", "to_name", "level_db", "pre"];

/// What became of one part of a preset on its way in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdjustKind {
    /// Missing, so the default was used.
    Filled,
    /// Not understood, so left out.
    Dropped,
    /// Out of range, so moved to the nearest value in range.
    Rounded,
    /// Pointing at something that is not there, so turned down.
    Rejected,
}

/// One row of what was adjusted: the part, and why.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Adjust {
    pub kind: AdjustKind,
    pub what: String,
    pub reason: String,
}

impl Adjust {
    pub fn new(kind: AdjustKind, what: impl Into<String>, reason: impl Into<String>) -> Adjust {
        Adjust {
            kind,
            what: what.into(),
            reason: reason.into(),
        }
    }
}

/// A send of a preset. The bus is found again by its id and name when the preset is put on a
/// track, because a preset outlives the song it was saved in.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetSend {
    pub to: Option<TrackId>,
    pub to_name: String,
    pub level_db: f64,
    pub pre: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackPreset {
    pub name: String,
    /// When it was first saved (ms), which is the order the presets are listed in. Files from before
    /// this was kept have none and come first.
    pub saved_at: Option<u64>,
    pub kind: TrackKind,
    pub color: Option<String>,
    pub volume_db: f64,
    pub pan: f64,
    /// The effects in their order; the instrument, if the track had one, comes first. The ids are
    /// placeholders: a track gets new ones.
    pub inserts: Vec<Plugin>,
    pub sends: Vec<PresetSend>,
}

impl TrackPreset {
    /// What `id` keeps of itself under `name`.
    pub fn snapshot(p: &Project, id: TrackId, name: &str) -> CmdResult<TrackPreset> {
        let t = p.track(id)?;
        if t.kind == TrackKind::Master {
            return Err(CmdError::conflict(
                "マスターはトラックのプリセットにできません",
            ));
        }
        let name = name.trim();
        if name.is_empty() {
            return Err(CmdError::invalid("プリセットの名前が空です"));
        }
        Ok(TrackPreset {
            name: name.to_string(),
            saved_at: None,
            kind: t.kind,
            color: t.color.clone(),
            volume_db: t.volume_db,
            pan: t.pan,
            inserts: t.inserts.clone(),
            sends: t
                .sends
                .iter()
                .map(|s| PresetSend {
                    to: Some(s.to),
                    to_name: p
                        .tracks
                        .get(&s.to)
                        .map(|b| b.name.clone())
                        .unwrap_or_default(),
                    // A muted send is saved at the level it comes back to.
                    level_db: s.muted_level.unwrap_or(s.level_db),
                    pre: s.pre,
                })
                .collect(),
        })
    }

    /// The preset as the file keeps it.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("version".into(), json!(VERSION));
        if let Some(at) = self.saved_at {
            m.insert("saved_at".into(), json!(at));
        }
        m.insert("name".into(), json!(self.name));
        m.insert("kind".into(), json!(self.kind));
        if let Some(c) = &self.color {
            m.insert("color".into(), json!(c));
        }
        m.insert("volume_db".into(), json!(self.volume_db));
        m.insert("pan".into(), json!(self.pan));
        m.insert(
            "inserts".into(),
            Value::Array(self.inserts.iter().map(plugin_value).collect()),
        );
        m.insert(
            "sends".into(),
            Value::Array(
                self.sends
                    .iter()
                    .map(|s| {
                        let mut m = Map::new();
                        if let Some(to) = s.to {
                            m.insert("to".into(), json!(to.to_string()));
                        }
                        m.insert("to_name".into(), json!(s.to_name));
                        m.insert("level_db".into(), json!(s.level_db));
                        m.insert("pre".into(), json!(s.pre));
                        Value::Object(m)
                    })
                    .collect(),
            ),
        );
        Value::Object(m)
    }

    /// Reads a preset, and says what had to be adjusted to read it. Only a value that is not a
    /// preset at all is an error.
    pub fn parse(v: &Value) -> CmdResult<(TrackPreset, Vec<Adjust>)> {
        let o = v.as_object().ok_or_else(|| {
            CmdError::invalid("トラックのプリセットではありません（オブジェクトが必要です）")
        })?;
        let mut rows = Vec::new();
        for k in o.keys().filter(|k| !KEYS.contains(&k.as_str())) {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                k.as_str(),
                "この版では読み込めないため、外しました",
            ));
        }
        if !matches!(o.get("version").and_then(Value::as_u64), Some(1..)) {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                "バージョン",
                format!("無いため {VERSION} として読み込みました"),
            ));
        }
        let name = match o.get("name").and_then(Value::as_str).map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => {
                rows.push(Adjust::new(
                    AdjustKind::Filled,
                    "名前",
                    "無いため「プリセット」にしました",
                ));
                "プリセット".to_string()
            }
        };
        let kind = match o.get("kind") {
            None | Some(Value::Null) => {
                rows.push(Adjust::new(
                    AdjustKind::Filled,
                    "種類",
                    "無いためオーディオトラックにしました",
                ));
                TrackKind::Audio
            }
            Some(k) => match serde_json::from_value::<TrackKind>(k.clone()) {
                Ok(TrackKind::Master) | Err(_) => {
                    rows.push(Adjust::new(
                        AdjustKind::Filled,
                        "種類",
                        "読み込めないためオーディオトラックにしました",
                    ));
                    TrackKind::Audio
                }
                Ok(k) => k,
            },
        };
        let color = match o.get("color") {
            None | Some(Value::Null) => None,
            Some(c) => match c.as_str().filter(|c| hex_color(c)) {
                Some(c) => Some(c.to_lowercase()),
                None => {
                    rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        "色",
                        "#rrggbb ではないため、外しました",
                    ));
                    None
                }
            },
        };
        let volume_db = number(o, "volume_db", "ボリューム", 0.0, MIN_DB, MAX_DB, &mut rows);
        let pan = number(o, "pan", "パン", 0.0, -1.0, 1.0, &mut rows);
        let mut inserts = Vec::new();
        match o.get("inserts") {
            Some(Value::Array(xs)) => {
                for (i, x) in xs.iter().enumerate() {
                    if let Some(x) = parse_plugin(i + 1, x, &mut rows) {
                        inserts.push(x);
                    }
                }
            }
            _ => rows.push(Adjust::new(
                AdjustKind::Filled,
                "エフェクト",
                "無いため、空にしました",
            )),
        }
        inserts = one_instrument_first(inserts, &mut rows);
        let mut sends = Vec::new();
        match o.get("sends") {
            Some(Value::Array(xs)) => {
                for (i, x) in xs.iter().enumerate() {
                    if let Some(x) = parse_send(i + 1, x, &mut rows) {
                        sends.push(x);
                    }
                }
            }
            _ => rows.push(Adjust::new(
                AdjustKind::Filled,
                "センド",
                "無いため、空にしました",
            )),
        }
        Ok((
            TrackPreset {
                name,
                saved_at: o.get("saved_at").and_then(Value::as_u64),
                kind,
                color,
                volume_db,
                pan,
                inserts,
                sends,
            },
            rows,
        ))
    }
}

fn plugin_value(x: &Plugin) -> Value {
    let mut v = serde_json::to_value(x).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("id");
        // Sidechain sources name this song's channels; a preset carries the
        // plug-in's settings, not the wiring around it (MIX-047).
        o.remove("sidechain");
    }
    v
}

fn hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())
}

/// A number of `o`, in `lo..=hi`: the default when it is not there, the nearest end when it is
/// outside.
fn number(
    o: &Map<String, Value>,
    key: &str,
    what: &str,
    default: f64,
    lo: f64,
    hi: f64,
    rows: &mut Vec<Adjust>,
) -> f64 {
    match o.get(key).and_then(Value::as_f64).filter(|n| n.is_finite()) {
        None => {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                what,
                format!("無いため {default} にしました"),
            ));
            default
        }
        Some(n) if n < lo || n > hi => {
            let r = n.clamp(lo, hi);
            rows.push(Adjust::new(
                AdjustKind::Rounded,
                what,
                format!("{n} は範囲外のため {r} にしました"),
            ));
            r
        }
        Some(n) => n,
    }
}

fn parse_plugin(n: usize, v: &Value, rows: &mut Vec<Adjust>) -> Option<Plugin> {
    let label = format!("エフェクト {n}");
    let Some(o) = v.as_object() else {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            label,
            "読み込めないため、外しました",
        ));
        return None;
    };
    let text = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let (Some(format), Some(uid)) = (text("format"), text("uid")) else {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            label,
            "どのプラグインか分からないため、外しました",
        ));
        return None;
    };
    let name = text("name").unwrap_or_else(|| {
        rows.push(Adjust::new(
            AdjustKind::Filled,
            &label,
            "名前が無いため、uid を名前にしました",
        ));
        uid.clone()
    });
    for k in o.keys().filter(|k| !PLUGIN_KEYS.contains(&k.as_str())) {
        rows.push(Adjust::new(
            AdjustKind::Dropped,
            format!("{name} の {k}"),
            "この版では読み込めないため、外しました",
        ));
    }
    let kind = match o
        .get("kind")
        .and_then(|k| serde_json::from_value::<PluginKind>(k.clone()).ok())
    {
        Some(k) => k,
        None => {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                &name,
                "種類が無いためエフェクトにしました",
            ));
            PluginKind::Effect
        }
    };
    let bypass = match o.get("bypass").and_then(Value::as_bool) {
        Some(b) => b,
        None => {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                format!("{name} のバイパス"),
                "無いためオフにしました",
            ));
            false
        }
    };
    // Not written when off, so a preset without it is not one that needed adjusting.
    let deactivated = o
        .get("deactivated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut params = BTreeMap::new();
    match o.get("params") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            for (k, x) in m {
                let what = format!("{name} の {k}");
                match x.as_f64().filter(|x| x.is_finite()) {
                    None => rows.push(Adjust::new(
                        AdjustKind::Dropped,
                        what,
                        "数ではないため、外しました",
                    )),
                    Some(x) if !(0.0..=1.0).contains(&x) => {
                        let r = x.clamp(0.0, 1.0);
                        rows.push(Adjust::new(
                            AdjustKind::Rounded,
                            what,
                            format!("{x} は範囲外のため {r} にしました"),
                        ));
                        params.insert(k.clone(), r);
                    }
                    Some(x) => {
                        params.insert(k.clone(), x);
                    }
                }
            }
        }
        Some(_) => rows.push(Adjust::new(
            AdjustKind::Dropped,
            format!("{name} の値"),
            "読み込めないため、外しました",
        )),
    }
    let state = match o.get("state") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if base64_like(s) => Some(s.clone()),
        Some(_) => {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{name} の状態"),
                "読み込めないため、外しました",
            ));
            None
        }
    };
    let isolation = match o.get("isolation") {
        None | Some(Value::Null) => PluginIsolation::default(),
        Some(i) => serde_json::from_value(i.clone()).unwrap_or_else(|_| {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{name} の分離"),
                "読み込めないため、既定にしました",
            ));
            PluginIsolation::default()
        }),
    };
    // The files the device reads, as the song names them (ENG-077). The paths travel with
    // the preset; the files themselves follow the song's collect/relink rules.
    let samples = match o.get("samples") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(_) => {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{name} のサンプル"),
                "読み込めないため、外しました",
            ));
            Vec::new()
        }
    };
    Some(Plugin {
        samples,
        id: PluginId(0),
        name,
        kind,
        format,
        uid,
        bypass,
        deactivated,
        params,
        state,
        isolation,
        sidechain: None,
    })
}

fn base64_like(s: &str) -> bool {
    s.len().is_multiple_of(4)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

/// A channel has one instrument and it comes first; a preset with more keeps the first.
fn one_instrument_first(inserts: Vec<Plugin>, rows: &mut Vec<Adjust>) -> Vec<Plugin> {
    let mut instrument = None;
    let mut effects = Vec::new();
    for x in inserts {
        if x.kind != PluginKind::Instrument {
            effects.push(x);
        } else if instrument.is_none() {
            instrument = Some(x);
        } else {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                x.name,
                "インストゥルメントは 1 つだけのため、外しました",
            ));
        }
    }
    instrument.into_iter().chain(effects).collect()
}

fn parse_send(n: usize, v: &Value, rows: &mut Vec<Adjust>) -> Option<PresetSend> {
    let label = format!("センド {n}");
    let Some(o) = v.as_object() else {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            label,
            "読み込めないため、外しました",
        ));
        return None;
    };
    for k in o.keys().filter(|k| !SEND_KEYS.contains(&k.as_str())) {
        rows.push(Adjust::new(
            AdjustKind::Dropped,
            format!("{label} の {k}"),
            "この版では読み込めないため、外しました",
        ));
    }
    let to = o
        .get("to")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<TrackId>().ok());
    let to_name = o
        .get("to_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if to.is_none() && to_name.is_empty() {
        rows.push(Adjust::new(
            AdjustKind::Rejected,
            label,
            "送り先が分からないため、外しました",
        ));
        return None;
    }
    let level_db = number(o, "level_db", &label, 0.0, MIN_DB, MAX_DB, rows);
    Some(PresetSend {
        to,
        to_name,
        level_db,
        pre: o.get("pre").and_then(Value::as_bool).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{PluginIsolation, PluginKind};

    /// The files a device reads travel with the preset (ENG-077).
    #[test]
    fn a_plugins_samples_round_trip_through_a_preset() {
        let x = Plugin {
            id: PluginId(0),
            name: "Sampler".into(),
            kind: PluginKind::Instrument,
            format: "builtin".into(),
            uid: "sampler".into(),
            bypass: false,
            deactivated: false,
            params: Default::default(),
            state: None,
            sidechain: None,
            samples: vec![
                "Media/Imports/Kick.wav".into(),
                "Media/Imports/Snare.wav".into(),
            ],
            isolation: PluginIsolation::default(),
        };
        let mut rows = Vec::new();
        let got = parse_plugin(0, &plugin_value(&x), &mut rows).unwrap();
        assert_eq!(got.samples, x.samples, "rows: {rows:?}");
        // A preset that never listed them parses with none.
        let mut bare = serde_json::Map::new();
        for k in ["name", "kind", "format", "uid"] {
            bare.insert(k.into(), plugin_value(&x)[k].clone());
        }
        let mut rows = Vec::new();
        let no = parse_plugin(0, &Value::Object(bare), &mut rows).unwrap();
        assert!(no.samples.is_empty());
    }
}
