//! `clip.ara.attach` (EDT-023): which ARA plugin the clips go through is the plugin list's to
//! say, so the session works it out and leaves one `clip.ara.attach.apply` for the history.

use serde_json::{Map, Value, json};
use xirius_core::commands::clip_ara::attachable;
use xirius_core::id::ClipId;
use xirius_core::{Args, CmdError, CmdResult, ExtContext, Output};
use xirius_plugins::Catalog;

use crate::import::Plan;

/// Commands answered here (through [`crate::ext::SessionExt`]).
pub const NAMES: [&str; 1] = ["clip.ara.attach"];

pub fn run(cx: &ExtContext<'_>, catalog: &Catalog, args: &Args, dry: bool) -> CmdResult<Plan> {
    let p = cx.project;
    let ids = args.require_ids::<ClipId>("クリップ")?;
    for id in &ids {
        attachable(p, *id)?;
    }
    let (plugin, name) = choose(p, catalog, args.opt_str("plugin"))?;
    let mut a = Map::new();
    a.insert(
        "ids".into(),
        Value::Array(ids.iter().map(|i| json!(i.to_string())).collect()),
    );
    a.insert("plugin".into(), json!(plugin));
    a.insert("name".into(), json!(name));
    Ok(Plan {
        output: Output::with(
            format!(
                "クリップ {} 件を ARA のプラグイン {name} に付けました",
                ids.len()
            ),
            json!({ "ids": ids.iter().map(|i| i.to_string()).collect::<Vec<_>>(), "plugin": plugin, "name": name }),
        ),
        edits: if dry {
            Vec::new()
        } else {
            vec![("clip.ara.attach.apply".into(), a)]
        },
    })
}

/// The plugin asked for; else the one the song's clips already use; else the only one there is.
fn choose(
    p: &xirius_core::model::Project,
    catalog: &Catalog,
    asked: Option<&str>,
) -> CmdResult<(String, String)> {
    if let Some(uid) = asked {
        return crate::ara::find(catalog, uid).ok_or_else(|| {
            CmdError::not_found(format!(
                "ARA のプラグイン {uid} が見つかりません（plugin.list で ARA と出るものの id を指定してください）"
            ))
        });
    }
    let used: std::collections::BTreeSet<(&str, &str)> = p
        .ara
        .clips
        .values()
        .map(|r| (r.plugin.as_str(), r.name.as_str()))
        .collect();
    if let [(plugin, name)] = used.iter().collect::<Vec<_>>()[..] {
        return Ok((plugin.to_string(), name.to_string()));
    }
    let all = crate::ara::plugins(catalog);
    match &all[..] {
        [one] => Ok(one.clone()),
        [] => Err(CmdError::not_found(
            "ARA のプラグインが入っていません（plugin.scan で探せます）",
        )),
        many => Err(CmdError::invalid(format!(
            "ARA のプラグインが {} 個あります。--plugin で指定してください: {}",
            many.len(),
            many.iter()
                .map(|(id, n)| format!("{n}（{id}）"))
                .collect::<Vec<_>>()
                .join("、")
        ))),
    }
}
