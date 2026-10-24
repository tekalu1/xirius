//! Commands about the app rather than a song: the songs it had open (`song.recent`, UI-261) and
//! whether a newer version is out (`app.update.check`, UI-262). They need no song, so the CLI
//! runs them without one.

use std::path::PathBuf;

use serde_json::json;
use xirius_core::command::{Args, CommandDef, Handler, Output};
use xirius_core::{CmdError, CmdResult, ExtContext, Extension, Risk};

use crate::recent::{LastOpen, RecentList};
use crate::update::{CURRENT, Fetch, UpdateCheck};

pub const NAMES: &[&str] = &["song.recent", "app.update.check"];

/// The pref that turns asking at launch on and off; manual checks are always available.
pub const UPDATE_PREF: &str = "app.update.check";

pub fn definitions() -> Vec<CommandDef> {
    vec![
        CommandDef {
            name: "song.recent",
            summary: "最近開いた曲の一覧と、次の起動で開く曲を表示します",
            args: vec![],
            history: false,
            risk: Risk::Read,
            handler: Handler::Session,
            screen_only: false,
        },
        CommandDef {
            name: "app.update.check",
            summary: "GitHub のリリースから新しいバージョンがあるかを確認します",
            args: vec![],
            history: false,
            risk: Risk::Read,
            handler: Handler::Session,
            screen_only: false,
        },
    ]
}

/// Answers [`NAMES`] from the app's folder.
pub struct AppExt {
    /// The app's own folder ([`crate::recent::root`]).
    pub root: PathBuf,
    pub settings: xirius_prefs::Shared,
    pub fetch: Fetch,
}

impl AppExt {
    /// The app's folder and the system's `curl`.
    pub fn standard(settings: xirius_prefs::Shared) -> AppExt {
        AppExt {
            root: crate::recent::root(),
            settings,
            fetch: crate::update::curl(),
        }
    }

    fn recent(&self) -> Output {
        let list = RecentList::in_dir(&self.root);
        let songs = list.songs();
        let open = match list.last_open() {
            LastOpen::Empty => json!({ "kind": "empty" }),
            LastOpen::Sample => json!({ "kind": "sample" }),
            LastOpen::Song(dir) => json!({ "kind": "song", "dir": dir.to_string_lossy() }),
        };
        let message = if songs.is_empty() {
            "最近の曲なし".to_string()
        } else {
            songs
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        Output::with(
            message,
            json!({
                "songs": songs.iter().map(|s| json!({
                    "name": s.name,
                    "dir": s.dir.to_string_lossy(),
                    "at_ms": s.at_ms,
                    "draft": s.draft,
                })).collect::<Vec<_>>(),
                "open": open,
            }),
        )
    }

    fn update(&self, dry: bool) -> CmdResult<Output> {
        let beta = {
            let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            s.reload_if_changed();
            s.prefs.bool(crate::update::BETA_PREF)
        };
        if dry {
            return Ok(Output::msg(
                "app.update.check をドライランで実行しました（確認していません）",
            ));
        }
        let check = UpdateCheck::in_dir(&self.root, self.fetch.clone());
        let checked = check
            .ask(now_ms(), beta)
            .map_err(|e| CmdError::conflict(format!("更新を確認できません: {e}")))?;
        let newer = checked.newer_than(CURRENT).cloned();
        let latest = checked.latest.clone();
        let message = match &newer {
            Some(r) => format!("新しいバージョン {} があります", r.version),
            None => format!("{CURRENT} は最新です"),
        };
        Ok(Output::with(
            message,
            json!({
                "enabled": true,
                "beta": beta,
                "current": CURRENT,
                "latest": latest.as_ref().map(|r| r.version.clone()),
                "prerelease": latest.as_ref().is_some_and(|r| r.prerelease),
                "newer": newer.is_some(),
                "url": latest.as_ref().map(|r| r.url.clone()),
                "notes": latest.map(|r| r.notes).unwrap_or_default(),
                "checked_ms": checked.checked_ms,
            }),
        ))
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

impl Extension for AppExt {
    fn answers(&self, name: &str) -> bool {
        NAMES.contains(&name)
    }

    fn run(
        &mut self,
        _: &ExtContext<'_>,
        def: &CommandDef,
        _: &Args,
        dry: bool,
    ) -> CmdResult<Output> {
        match def.name {
            "song.recent" => Ok(self.recent()),
            "app.update.check" => self.update(dry),
            other => Err(CmdError::not_found(format!("{other} はありません"))),
        }
    }
}
