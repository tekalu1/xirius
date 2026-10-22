//! Portable preset files for built-ins and opaque VST3 states.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use xirius_core::command::Output;
use xirius_core::extension::ExtContext;
use xirius_core::id::PluginId;
use xirius_core::model::PluginKind;
use xirius_core::{Args, CmdError, CmdResult};

pub const NAMES: [&str; 2] = ["plugin.preset.save", "plugin.preset.open"];
type Edits = Vec<(String, Map<String, Value>)>;

#[derive(Serialize, Deserialize)]
struct Preset {
    version: u32,
    format: String,
    uid: String,
    kind: PluginKind,
    params: BTreeMap<String, f64>,
    state: Option<String>,
}

pub(crate) fn run(
    cx: &ExtContext<'_>,
    a: &Args,
    name: &str,
    dry: bool,
) -> CmdResult<(Output, Edits)> {
    let id: PluginId = a.id("id")?;
    let x = cx
        .project
        .tracks
        .values()
        .find_map(|t| t.inserts.iter().find(|x| x.id == id))
        .ok_or_else(|| CmdError::not_found("プラグインが見つかりません"))?;
    let path = PathBuf::from(a.str("file")?);
    let path = if path.is_absolute() {
        path
    } else {
        cx.dir.unwrap_or(Path::new(".")).join(path)
    };
    let io = |e: std::io::Error| CmdError::invalid(format!("プリセットを読み書きできません: {e}"));
    if name == NAMES[0] {
        let preset = Preset {
            version: 1,
            format: x.format.clone(),
            uid: x.uid.clone(),
            kind: x.kind,
            params: x.params.clone(),
            state: x.state.clone(),
        };
        let data =
            serde_json::to_vec_pretty(&preset).map_err(|e| CmdError::invalid(e.to_string()))?;
        if !dry {
            xirius_core::store::atomic_write(&path, &data)?;
        }
        return Ok((
            Output::with("プリセットを保存しました", json!({"file":path})),
            Vec::new(),
        ));
    }
    if std::fs::metadata(&path).map_err(io)?.len() > 64 * 1024 * 1024 {
        return Err(CmdError::invalid("プリセットのファイルが大きすぎます"));
    }
    let preset: Preset = serde_json::from_slice(&std::fs::read(&path).map_err(io)?)
        .map_err(|e| CmdError::invalid(format!("プリセットを読み込めません: {e}")))?;
    let same_uid = preset.uid == x.uid
        || (preset.format.eq_ignore_ascii_case("builtin")
            && x.builtin()
                .zip(xirius_params::find(&preset.uid))
                .is_some_and(|(a, b)| a.uid == b.uid));
    if preset.version != 1
        || !preset.format.eq_ignore_ascii_case(&x.format)
        || !same_uid
        || preset.kind != x.kind
    {
        return Err(CmdError::conflict(
            "このプラグインのプリセットではありません",
        ));
    }
    if preset
        .params
        .values()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(CmdError::range("プリセットの値が範囲外です"));
    }
    if let Some(state) = &preset.state {
        if state.len() % 4 != 0
            || !state
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        {
            return Err(CmdError::invalid("プリセットの状態を読み込めません"));
        }
        if preset.format.eq_ignore_ascii_case("vst3") {
            xirius_plugins::decode_state(state)
                .map_err(|e| CmdError::invalid(format!("プリセットの状態を読み込めません: {e}")))?;
        }
    }
    let values: Vec<_> = preset
        .params
        .iter()
        .map(|(k, v)| json!({"param":k,"value":v}))
        .collect();
    let mut args = json!({"id":id.to_string(),"values":values})
        .as_object()
        .unwrap()
        .clone();
    if let Some(state) = preset.state {
        args.insert("state".into(), json!(state));
    }
    Ok((
        Output::msg("プリセットを読み込みました"),
        if dry {
            Vec::new()
        } else {
            vec![("plugin.preset.apply".into(), args)]
        },
    ))
}
