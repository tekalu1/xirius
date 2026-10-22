//! Applying the complete contents of a plugin preset as one edit.
use super::*;
use crate::id::{IdKind, PluginId};

pub(super) fn defs() -> Vec<CommandDef> {
    vec![edit(
        "plugin.preset.apply",
        "プリセットの状態を適用します",
        vec![
            ArgDef::id("id", IdKind::Plugin, "プラグイン").req(),
            ArgDef::text("state", "保存された状態"),
            ArgDef::objects("values", "パラメーターの値").req(),
        ],
        |p, _, a| {
            let id: PluginId = a.id("id")?;
            if let Some(state) = a.opt_str("state")
                && (state.len() % 4 != 0
                    || !state
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')))
            {
                return Err(CmdError::invalid("プリセットの状態を読み込めません"));
            }
            let t = p
                .tracks
                .iter()
                .find(|(_, t)| t.inserts.iter().any(|x| x.id == id))
                .map(|(t, _)| *t)
                .ok_or_else(|| CmdError::not_found("プラグインが見つかりません"))?;
            let mut params = std::collections::BTreeMap::new();
            for v in a.0["values"].as_array().expect("checked") {
                let k = v["param"]
                    .as_str()
                    .ok_or_else(|| CmdError::invalid("パラメーター名が必要です"))?;
                let value = v["value"]
                    .as_f64()
                    .filter(|x| x.is_finite() && (0.0..=1.0).contains(x))
                    .ok_or_else(|| CmdError::range("パラメーターの値は0〜1です"))?;
                params.insert(k.into(), value);
            }
            let x = p
                .track_mut(t)?
                .inserts
                .iter_mut()
                .find(|x| x.id == id)
                .expect("found");
            x.params = params;
            x.state = a
                .opt_str("state")
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            Ok(Output::msg("プリセットを読み込みました"))
        },
    )]
}
