//! Device-channel choices not covered by prefs' existing catalogue.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Routes {
    pub device: String,
    pub input: Option<usize>,
    pub output: [usize; 2],
}

/// Restore the same physical output selection for UI, CLI and session playback.
pub fn apply(host: &mut crate::host::SessionHost) -> Result<(), String> {
    let Some(settings) = &host.io_settings else {
        return Ok(());
    };
    let settings = settings.lock().unwrap_or_else(|e| e.into_inner());
    let Some(dir) = &settings.dir else {
        return Ok(());
    };
    let Some(routes) = read(dir)? else {
        return Ok(());
    };
    let selected = settings.prefs.text("audio.device");
    if !selected.is_empty() && routes.device != selected {
        return Ok(());
    }
    drop(settings);
    host.controller()
        .send(xirius_engine::Command::OutputChannels(routes.output))
        .map_err(|_| "出力を設定できません".into())
}

pub fn read(dir: &Path) -> Result<Option<Routes>, String> {
    let path = dir.join("audio-routing.json");
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn write(dir: &Path, routes: &Routes) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("audio-routing.json");
    let temporary = dir.join(format!("audio-routing-{}.tmp", std::process::id()));
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(routes).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}
