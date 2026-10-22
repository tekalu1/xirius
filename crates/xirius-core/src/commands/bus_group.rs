//! Selected channels routed to a new bus as one history step.
use super::*;
use crate::id::TrackId;
use crate::model::Output as Route;

pub(super) fn bus(p: &mut Project, a: &Args) -> CmdResult<Output> {
    let sources = match a.0.get("sources") {
        Some(v) => Args(serde_json::Map::from_iter([("ids".into(), v.clone())]))
            .require_ids::<TrackId>("チャンネル")?,
        None => Vec::new(),
    };
    for id in &sources {
        if id.is_master() {
            return Err(CmdError::conflict("マスターはバスにまとめられません"));
        }
        p.track(*id)?;
    }
    let result = super::track::add_track(p, a, crate::model::TrackKind::Bus)?;
    let to: TrackId = result.data["id"]
        .as_str()
        .expect("id")
        .parse()
        .map_err(CmdError::invalid)?;
    if let Some(plugin) = a.opt_str("plugin") {
        let name = plugin.trim();
        if name.is_empty() {
            return Err(CmdError::invalid("--plugin が空です"));
        }
        super::track::place_new_plugin(
            p,
            to,
            crate::model::PluginKind::Effect,
            name,
            a.opt_str("format").unwrap_or("builtin"),
            a.opt_str("uid"),
            None,
        )?;
    }
    for id in sources {
        p.track_mut(id)?.output = Some(Route::Bus(to));
    }
    Ok(result)
}
