//! A device preset (PLG-063): one device's settings kept as a `.xpreset` file, so every
//! device shows its own preset list: `Default` first (the device's own defaults, not a
//! file), then the named presets by name.
//!
//! The file is JSON (`format` `xirius-preset`, `version` 1) under `presets/<kind>/` beside
//! the track presets, with `kind` `fx` or `instrument`. It is read the way a song is
//! (PRJ-007): what is missing is filled, a key this version does not know is left out, a
//! value out of range is rounded, and every one of these comes back as an [`Adjust`] row.
//! The `plugin.preset` files written before (an object with `uid`/`kind`/`params` and no
//! `format` marker) read the same way; Studio Pro's preset files are not read.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::error::{CmdError, CmdResult};
use crate::model::{Plugin, PluginKind};
use crate::track_preset::{Adjust, AdjustKind};

/// The version this writes. Newer files are read as far as they are understood.
pub const VERSION: u32 = 1;

/// The `format` a `.xpreset` file names itself with.
pub const FORMAT: &str = "xirius-preset";

/// The name every device's list starts with: its own defaults, synthesised on read, so it
/// stays at the head even after every preset file is deleted.
pub const DEFAULT_PRESET: &str = "Default";

/// The preset kinds a `.xpreset` may hold (PLG-065 adds the chain kinds).
pub const KINDS: [&str; 2] = ["fx", "instrument"];

const KEYS: [&str; 6] = ["format", "version", "kind", "name", "saved_at", "devices"];
const DEVICE_KEYS: [&str; 8] = [
    "ref", "name", "kind", "format", "uid", "params", "state", "samples",
];

/// The folder (`presets/<kind>/`) a plugin kind's preset files live under.
pub fn kind_name(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Effect => "fx",
        PluginKind::Instrument => "instrument",
    }
}

/// The plugin kind a file's `kind` names (`fx` is the effects).
pub fn kind_of(k: &str) -> Option<PluginKind> {
    match k {
        "fx" => Some(PluginKind::Effect),
        "instrument" => Some(PluginKind::Instrument),
        _ => None,
    }
}

/// The device a preset belongs to. `format`/`uid`/`params`/`state` are kept the way the
/// song's plug-in row keeps them, so a preset round-trips what this build cannot run.
#[derive(Clone, PartialEq, Debug)]
pub struct PresetDevice {
    pub name: String,
    pub kind: PluginKind,
    pub format: String,
    pub uid: String,
    pub params: BTreeMap<String, f64>,
    pub state: Option<String>,
    /// `None`: the file said nothing about samples (an old `plugin.preset` file), so loading
    /// leaves the device's files alone. `Some(vec![])`: an `.xpreset` that means none.
    pub samples: Option<Vec<String>>,
}

/// One named device preset, for the `fx` and `instrument` kinds exactly one device.
#[derive(Clone, PartialEq, Debug)]
pub struct DevicePreset {
    pub name: String,
    pub kind: PluginKind,
    pub device: PresetDevice,
    pub saved_at: Option<u64>,
}

impl DevicePreset {
    /// The preset of what the plug-in `x` sounds like now, under `name`.
    pub fn capture(x: &Plugin, name: &str, saved_at: Option<u64>) -> DevicePreset {
        DevicePreset {
            name: name.to_string(),
            kind: x.kind,
            device: PresetDevice {
                name: x.name.clone(),
                kind: x.kind,
                format: x.format.clone(),
                uid: x.uid.clone(),
                params: x
                    .params
                    .iter()
                    .filter(|(k, _)| holds(x, k))
                    .map(|(k, v)| (k.clone(), *v))
                    .collect(),
                state: x.state.clone(),
                samples: Some(x.samples.clone()),
            },
            saved_at,
        }
    }

    /// The `Default` preset of `x`'s device: a built-in's own defaults of the knobs a preset
    /// holds, an empty state for anything else.
    pub fn factory_default(x: &Plugin) -> DevicePreset {
        let (params, samples) = match x.builtin() {
            Some(def) => (
                def.params
                    .iter()
                    .filter(|p| p.preset)
                    .map(|p| (p.id.to_string(), p.default_norm()))
                    .collect(),
                Some(Vec::new()),
            ),
            None => (BTreeMap::new(), None),
        };
        DevicePreset {
            name: DEFAULT_PRESET.to_string(),
            kind: x.kind,
            device: PresetDevice {
                name: x.name.clone(),
                kind: x.kind,
                format: x.format.clone(),
                uid: x.uid.clone(),
                params,
                state: None,
                samples,
            },
            saved_at: None,
        }
    }

    /// The JSON a `.xpreset` file holds.
    pub fn to_value(&self) -> Value {
        let d = &self.device;
        let mut dev = Map::new();
        dev.insert("ref".into(), json!("p1"));
        dev.insert("name".into(), json!(d.name));
        dev.insert("kind".into(), json!(d.kind));
        dev.insert("format".into(), json!(d.format));
        dev.insert("uid".into(), json!(d.uid));
        if !d.params.is_empty() {
            dev.insert("params".into(), json!(d.params));
        }
        if let Some(state) = &d.state {
            dev.insert("state".into(), json!(state));
        }
        if let Some(samples) = &d.samples {
            dev.insert("samples".into(), json!(samples));
        }
        let mut o = Map::new();
        o.insert("format".into(), json!(FORMAT));
        o.insert("version".into(), json!(VERSION));
        o.insert("kind".into(), json!(kind_name(self.kind)));
        o.insert("name".into(), json!(self.name));
        if let Some(at) = self.saved_at {
            o.insert("saved_at".into(), json!(at));
        }
        o.insert("devices".into(), json!([Value::Object(dev)]));
        Value::Object(o)
    }

    /// Reads a `.xpreset` file, or an old `plugin.preset` file, and says what had to be
    /// adjusted to read it. Only a value that is not a preset at all is an error.
    /// `fallback` names a file whose `name` is absent (its stem).
    pub fn parse(v: &Value, fallback: Option<&str>) -> CmdResult<(DevicePreset, Vec<Adjust>)> {
        let o = v.as_object().ok_or_else(|| {
            CmdError::invalid("プリセットではありません（オブジェクトが必要です）")
        })?;
        if o.get("format").and_then(Value::as_str) == Some(FORMAT) {
            parse_xpreset(o, fallback)
        } else if o.contains_key("uid") {
            parse_legacy(o, fallback)
        } else {
            Err(CmdError::invalid(
                "プリセットではありません（format が xirius-preset ではありません）",
            ))
        }
    }
}

/// Whether a preset of the plug-in `x`'s device holds its parameter `id`: every one but a
/// built-in's knobs no preset sets ([`xirius_params::ParamSpec::preset`]; the Compressor's
/// sidechain, PLG-071). A preset is not saved with those, and loading one leaves them be.
pub fn holds(x: &Plugin, id: &str) -> bool {
    x.builtin()
        .and_then(|def| def.param(id))
        .is_none_or(|(_, p)| p.preset)
}

/// Whether the plug-in `x` is the device `format`/`uid` of a preset name: the same uid, or
/// the same built-in under its other name.
pub fn same_device(format: &str, uid: &str, x: &Plugin) -> bool {
    uid == x.uid
        || (format.eq_ignore_ascii_case("builtin")
            && x.builtin()
                .zip(xirius_params::find(uid))
                .is_some_and(|(a, b)| a.uid == b.uid))
}

fn parse_xpreset(
    o: &Map<String, Value>,
    fallback: Option<&str>,
) -> CmdResult<(DevicePreset, Vec<Adjust>)> {
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
    let name = match o
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(n) => n.to_string(),
        None => {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                "名前",
                "無いため、ファイルの名前にしました",
            ));
            fallback.unwrap_or("プリセット").to_string()
        }
    };
    let device = match o.get("devices") {
        Some(Value::Array(xs)) => {
            let mut kept = Vec::new();
            for (i, x) in xs.iter().enumerate() {
                if let Some(d) = parse_device(i + 1, x, &mut rows) {
                    kept.push(d);
                }
            }
            if kept.is_empty() {
                return Err(CmdError::invalid(
                    "プリセットを読み込めません（使える機器がありません）",
                ));
            }
            for d in kept.drain(1..) {
                rows.push(Adjust::new(
                    AdjustKind::Dropped,
                    d.name,
                    "1 機のプリセットに収まらないため、外しました",
                ));
            }
            kept.into_iter().next().expect("checked above")
        }
        _ => {
            return Err(CmdError::invalid(
                "プリセットを読み込めません（機器がありません）",
            ));
        }
    };
    let kind = match o.get("kind").and_then(Value::as_str).and_then(kind_of) {
        Some(k) => k,
        None => {
            rows.push(Adjust::new(
                AdjustKind::Filled,
                "種類",
                "読み込めないため、機器の種類に合わせました",
            ));
            device.kind
        }
    };
    let mut device = device;
    if device.kind != kind {
        rows.push(Adjust::new(
            AdjustKind::Filled,
            &device.name,
            "ファイルの種類と違うため、直しました",
        ));
        device.kind = kind;
    }
    Ok((
        DevicePreset {
            name,
            kind,
            device,
            saved_at: o.get("saved_at").and_then(Value::as_u64),
        },
        rows,
    ))
}

/// An old `plugin.preset` file (`plugin.preset.save` wrote it): `uid`/`kind`/`format`/
/// `params`/`state` on top, no `format` marker.
fn parse_legacy(
    o: &Map<String, Value>,
    fallback: Option<&str>,
) -> CmdResult<(DevicePreset, Vec<Adjust>)> {
    const LEGACY_KEYS: [&str; 6] = ["version", "format", "uid", "kind", "params", "state"];
    let mut rows = Vec::new();
    for k in o.keys().filter(|k| !LEGACY_KEYS.contains(&k.as_str())) {
        rows.push(Adjust::new(
            AdjustKind::Dropped,
            k.as_str(),
            "この版では読み込めないため、外しました",
        ));
    }
    // `version` is the file's own marker and `name` a key the format never had: reading
    // them as the device's unknown keys would report noise for a file written as intended.
    let mut body = o.clone();
    body.remove("version");
    if !body.contains_key("name")
        && let Some(uid) = body.get("uid").cloned()
    {
        body.insert("name".into(), uid);
    }
    let Some(device) = parse_device(1, &Value::Object(body), &mut rows) else {
        return Err(CmdError::invalid(
            "プリセットを読み込めません（どのプラグインか分かりません）",
        ));
    };
    let name = match o
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        Some(n) => n.to_string(),
        None => fallback.unwrap_or("プリセット").to_string(),
    };
    Ok((
        DevicePreset {
            name,
            kind: device.kind,
            device: PresetDevice {
                samples: None,
                ..device
            },
            saved_at: None,
        },
        rows,
    ))
}

/// One entry of `devices`, or the body of an old `plugin.preset` file (which carries the
/// same fields on top).
fn parse_device(n: usize, v: &Value, rows: &mut Vec<Adjust>) -> Option<PresetDevice> {
    let label = format!("機器 {n}");
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
    for k in o.keys().filter(|k| !DEVICE_KEYS.contains(&k.as_str())) {
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
    let samples = match o.get("samples") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
        ),
        Some(_) => {
            rows.push(Adjust::new(
                AdjustKind::Dropped,
                format!("{name} のサンプル"),
                "読み込めないため、外しました",
            ));
            None
        }
    };
    Some(PresetDevice {
        name,
        kind,
        format,
        uid,
        params,
        state,
        samples,
    })
}

fn base64_like(s: &str) -> bool {
    !s.is_empty()
        && s.len().is_multiple_of(4)
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::PluginId;

    fn plugin(format: &str, uid: &str) -> Plugin {
        Plugin {
            samples: Vec::new(),
            id: PluginId(1),
            name: "EQ".into(),
            kind: PluginKind::Effect,
            format: format.into(),
            uid: uid.into(),
            bypass: false,
            deactivated: false,
            params: BTreeMap::from([("gain".into(), 0.5)]),
            state: None,
            isolation: Default::default(),
            sidechain: None,
            preset: None,
        }
    }

    #[test]
    fn writes_and_reads_a_preset_back() {
        let mut x = plugin("builtin", "eq");
        x.preset = Some("古い".into());
        x.state = Some("QUJD".into());
        x.samples = vec!["kick.wav".into()];
        let p = DevicePreset::capture(&x, "ぬくもり", Some(42));
        let v = p.to_value();
        assert_eq!(v["format"], json!(FORMAT));
        assert_eq!(v["version"], json!(1));
        assert_eq!(v["kind"], json!("fx"));
        let (back, rows) = DevicePreset::parse(&v, None).unwrap();
        assert!(rows.is_empty(), "{rows:?}");
        assert_eq!(back.name, "ぬくもり");
        assert_eq!(back.kind, PluginKind::Effect);
        assert_eq!(back.device.uid, "eq");
        assert_eq!(back.device.params["gain"], 0.5);
        assert_eq!(back.device.state.as_deref(), Some("QUJD"));
        assert_eq!(back.device.samples, Some(vec!["kick.wav".to_string()]));
    }

    #[test]
    fn reads_a_legacy_plugin_preset() {
        let v = json!({
            "version": 1, "format": "vst3", "uid": "deadbeef", "kind": "effect",
            "params": {"mix": 0.25}, "state": "AAAA",
        });
        let (p, rows) = DevicePreset::parse(&v, Some("Warm")).unwrap();
        assert!(rows.is_empty(), "{rows:?}");
        assert_eq!(p.name, "Warm");
        assert_eq!(p.device.uid, "deadbeef");
        assert_eq!(p.device.samples, None);
        assert_eq!(p.device.params["mix"], 0.25);
    }

    #[test]
    fn rounds_and_drops_with_notes() {
        let v = json!({
            "format": "xirius-preset", "version": 1, "kind": "fx", "name": "x",
            "extra": true,
            "devices": [{
                "name": "EQ", "kind": "effect", "format": "builtin", "uid": "eq",
                "params": {"a": 1.5, "b": "loud", "c": 0.4},
                "state": "not base64!!",
                "unknown": 1,
            }],
        });
        let (p, rows) = DevicePreset::parse(&v, None).unwrap();
        assert_eq!(p.device.params["a"], 1.0);
        assert_eq!(p.device.params["c"], 0.4);
        assert!(!p.device.params.contains_key("b"));
        assert_eq!(p.device.state, None);
        assert_eq!(rows.len(), 5, "{rows:?}");
    }

    #[test]
    fn refuses_a_non_preset() {
        assert!(DevicePreset::parse(&json!({"hello": 1}), None).is_err());
        assert!(
            DevicePreset::parse(
                &json!({"format": "xirius-preset", "version": 1, "kind": "fx", "devices": []}),
                None,
            )
            .is_err()
        );
    }
}
