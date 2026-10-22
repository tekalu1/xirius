//! The rows of the menus that copy a channel's settings and that keep, apply and add track
//! presets (MIX-007, MIX-030). Every row runs a command of the table, so the CLI and MCP do the
//! same under the same names.

use serde_json::{Value, json};
use xirius_core::clipboard::Clipboard;
use xirius_core::id::TrackId;
use xirius_core::model::TrackKind;

use super::{Item, Run};
use crate::studio::Studio;
use crate::tree::RowKind;

/// A preset as the list says it.
struct Listed {
    name: String,
    kind: String,
}

impl Studio {
    /// The presets in the order they were saved in.
    fn listed_presets(&mut self) -> Vec<Listed> {
        let Ok(data) = self.song.run("track.preset.list", json!({})) else {
            return Vec::new();
        };
        data.get("presets")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        Some(Listed {
                            name: r.get("name")?.as_str()?.to_string(),
                            kind: r
                                .get("kind")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The channels a menu on `t` acts on: the selected ones when `t` is among them.
    pub(super) fn channels_of(&self, t: TrackId) -> Vec<TrackId> {
        let selected: Vec<TrackId> = self
            .arrange
            .tracks
            .iter()
            .filter_map(|r| match r {
                RowKind::Track(x) => Some(*x),
                _ => None,
            })
            .collect();
        if selected.len() > 1 && selected.contains(&t) {
            selected
        } else {
            vec![t]
        }
    }

    /// "チャンネルの設定をコピー" and "チャンネルの設定を貼り付け", and a gap. The paste is dim
    /// while nothing is copied, or when the only channel it would go to is the one copied.
    pub(super) fn channel_copy_items(&mut self, t: TrackId) -> Vec<Item> {
        let copied_from = match &self.song.session().state.clipboard {
            Some(Clipboard::Channel(c)) => Some(c.source),
            _ => None,
        };
        let targets = self.channels_of(t);
        let ids: Vec<String> = targets.iter().map(ToString::to_string).collect();
        let copy = Item {
            key: self.menu_key("channel.settings.copy"),
            ..Item::new(
                "チャンネルの設定をコピー",
                Run::Cmd("channel.settings.copy", json!({ "id": t.to_string() })),
            )
        };
        let usable = copied_from.is_some_and(|from| targets.iter().any(|x| *x != from));
        let paste = Item {
            key: self.menu_key("channel.settings.paste"),
            enabled: usable,
            ..Item::new(
                "チャンネルの設定を貼り付け",
                Run::Cmd("channel.settings.paste", json!({ "ids": ids })),
            )
        };
        vec![copy, paste, Item::gap()]
    }

    /// "トラックのプリセットとして保存…", "トラックのプリセットを適用 ›" and, on a track made from a
    /// preset, "トラックのプリセットを更新"; then a gap. Nothing for a bus or the master.
    pub(super) fn preset_items(&mut self, t: TrackId) -> Vec<Item> {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Vec::new();
        };
        if !matches!(track.kind, TrackKind::Audio | TrackKind::Instrument) {
            return Vec::new();
        }
        let kind = serde_json::to_value(track.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default();
        let current = track.preset.clone();
        let name = track.name.clone();
        let ids = self.channels_of(t);
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|x| {
                p.tracks
                    .get(x)
                    .is_some_and(|k| matches!(k.kind, TrackKind::Audio | TrackKind::Instrument))
            })
            .map(|x| x.to_string())
            .collect();
        let presets = self.listed_presets();
        let apply: Vec<Item> = presets
            .iter()
            .map(|pr| {
                Item::new(
                    &pr.name,
                    Run::Cmd(
                        "track.preset.apply",
                        json!({ "ids": ids, "preset": pr.name }),
                    ),
                )
                .on(current.as_deref() == Some(pr.name.as_str()))
                .enabled(pr.kind == kind || pr.kind.is_empty())
            })
            .collect();
        let save = Item::ask(
            "トラックのプリセットとして保存…",
            "track.preset.save",
            json!({ "track": t.to_string() }),
            "name",
            &name,
        );
        let mut rows = vec![
            save,
            Item::sub("トラックのプリセットを適用", apply).enabled(!presets.is_empty()),
        ];
        if current
            .as_deref()
            .is_some_and(|c| presets.iter().any(|pr| pr.name == c))
        {
            rows.push(Item::new(
                "トラックのプリセットを更新",
                Run::Cmd("track.preset.update", json!({ "track": t.to_string() })),
            ));
        }
        rows.push(Item::gap());
        rows
    }

    /// The presets as rows under "トラックを追加": each makes a track of its own, at `at`.
    pub(super) fn preset_add_rows(&mut self, at: &Value) -> Vec<Item> {
        let presets = self.listed_presets();
        if presets.is_empty() {
            return Vec::new();
        }
        let mut rows = vec![Item::gap()];
        rows.extend(presets.iter().map(|pr| {
            let mut args = json!({ "preset": pr.name });
            if let Some(o) = at.as_object() {
                for (k, v) in o {
                    args[k] = v.clone();
                }
            }
            Item::new(&pr.name, Run::Cmd("track.preset.add", args))
        }));
        rows
    }
}
