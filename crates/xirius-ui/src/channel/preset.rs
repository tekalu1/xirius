//! The channel editor's preset tag (MIX-042, PLG-065): «‹ ▾ name ● ›» in its bar, the
//! device window's parts (UI-332) over the track's whole chain: an audio or a bus track's FX
//! (`fx-chain`), an instrument track's instrument and FX (`instrument-fx`), a
//! multi-instrument's layers (`multi-instrument`). The list is the chain presets of that
//! kind the session keeps, by name; ‹ › load the one before or after the tag's (they stop at
//! the list's ends, as the mock's), a row loads it over the whole chain, 上書き保存 and
//! 名前を付けて保存… write the chain.
//!
//! The song keeps no chain preset's name, so the tag names the preset this screen last
//! loaded onto the track or saved from it while the song is open, and the mark says the
//! chain moved off what it was then. A track none was loaded on shows [`UNKNOWN`]; its › loads
//! the list's first.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::chain_preset::{Chain, ChainKind};
use xirius_core::id::TrackId;
use xirius_core::model::{Track, TrackKind};

use crate::instrument::menu::{LOAD, SAVE, SAVE_AS};
use crate::instrument::presets::UNKNOWN;
use crate::popup::Choice;
use crate::studio::Studio;

/// How often the list is read again while the window is open.
const EVERY: Duration = Duration::from_millis(500);

/// A chain preset of the session's, as the list names it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    pub name: String,
    pub file: String,
}

/// The preset a track's tag names and the chain as it was loaded or saved.
#[derive(Clone, Debug)]
struct Mark {
    name: String,
    kind: ChainKind,
    chain: Chain,
}

#[derive(Debug, Default)]
pub struct ChainPresets {
    marks: HashMap<TrackId, Mark>,
    /// The lists by kind, as last read.
    lists: HashMap<&'static str, Vec<Listed>>,
    read_at: Option<Instant>,
}

/// The kind of chain the tag of `t` loads and saves.
pub fn kind_of(t: &Track) -> ChainKind {
    if !super::keys::layers(t).is_empty() {
        ChainKind::Multi
    } else if t.kind == TrackKind::Instrument {
        ChainKind::Instrument
    } else {
        ChainKind::Fx
    }
}

/// The whole chain of `t` as a preset of `kind` holds it; none when it cannot be one.
fn chain_of(t: &Track, kind: ChainKind) -> Option<Chain> {
    Chain::capture(t, kind, None).ok().map(|(c, _)| c)
}

impl ChainPresets {
    /// The tag of `t`: the preset's name and whether the chain moved off it.
    pub fn tag(&self, t: &Track) -> (String, bool) {
        let kind = kind_of(t);
        match self.marks.get(&t.id).filter(|m| m.kind == kind) {
            Some(m) => (
                m.name.clone(),
                chain_of(t, kind).is_none_or(|c| c != m.chain),
            ),
            None => (UNKNOWN.into(), false),
        }
    }

    /// The presets of `kind` as last read.
    pub fn list(&self, kind: ChainKind) -> &[Listed] {
        self.lists.get(kind.name()).map_or(&[], Vec::as_slice)
    }

    /// Whether the ‹ and the › of `t` can step: none past the ends. A tag that names no preset
    /// of the list sits before its first, so › loads that (review 3 中5).
    pub fn steps(&self, t: &Track) -> (bool, bool) {
        let list = self.list(kind_of(t));
        let (name, _) = self.tag(t);
        match list.iter().position(|l| l.name == name) {
            Some(k) => (k > 0, k + 1 < list.len()),
            None => (false, !list.is_empty()),
        }
    }

    /// The song closed: its tracks' ids come again in the next.
    pub fn forget(&mut self) {
        self.marks.clear();
    }
}

impl Studio {
    /// Reads the presets of `kind` again; true when the list changed.
    pub(crate) fn read_chain_presets(&mut self, kind: ChainKind) -> bool {
        self.chain_presets.read_at = Some(Instant::now());
        // design-lint: allow(ui-blocking-io) MIX-042: on the window thread the session answers the list from its cache and reads the files on a background job.
        let Ok(data) = self.song.run("preset.list", json!({ "kind": kind.name() })) else {
            return false;
        };
        let list: Vec<Listed> = data["presets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(Listed {
                    name: p["name"].as_str()?.to_string(),
                    file: p["file"].as_str()?.to_string(),
                })
            })
            .collect();
        let had = self.chain_presets.lists.insert(kind.name(), list.clone());
        had.as_ref() != Some(&list)
    }

    /// Every half second while the channel editor is open, so a preset saved or removed
    /// elsewhere shows; true when the list changed.
    pub(crate) fn poll_chain_presets(&mut self) -> bool {
        let Some(t) = self.channel.as_ref().map(|w| w.track) else {
            return false;
        };
        if self
            .chain_presets
            .read_at
            .is_some_and(|at| at.elapsed() < EVERY)
        {
            return false;
        }
        let Some(kind) = self.project().tracks.get(&t).map(|x| kind_of(x)) else {
            return false;
        };
        self.read_chain_presets(kind)
    }

    /// Loads the preset in `file` over the whole chain of `t`.
    pub(crate) fn load_chain_preset(&mut self, t: TrackId, file: &str) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "preset.load",
            json!({ "track": t.to_string(), "preset": file, "at": "track", "replace": true }),
        )
    }

    /// The ‹ (`forward` false) or the › of the tag of `t`.
    pub(crate) fn step_chain_preset(&mut self, t: TrackId, forward: bool) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let (name, _) = self.chain_presets.tag(track);
        let list = self.chain_presets.list(kind_of(track));
        let to = match list.iter().position(|l| l.name == name) {
            Some(k) if forward => k + 1,
            Some(k) => k.wrapping_sub(1),
            // a tag before the list: › goes to its first
            None if forward => 0,
            None => return false,
        };
        let Some(file) = list.get(to).map(|l| l.file.clone()) else {
            return false;
        };
        self.load_chain_preset(t, &file)
    }

    /// Saves the whole chain of `t` as the preset `name` (`overwrite`: over its file).
    pub(crate) fn save_chain_preset(&mut self, t: TrackId, name: &str, overwrite: bool) -> bool {
        let Some(kind) = self.project().tracks.get(&t).map(|x| kind_of(x)) else {
            return false;
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            "preset.save",
            json!({
                "track": t.to_string(),
                "kind": kind.name(),
                "name": name,
                "overwrite": overwrite,
            }),
        )
    }

    /// The list's preset of the name, letter case aside (as the session finds it).
    fn chain_preset_named(&self, kind: ChainKind, name: &str) -> Option<String> {
        let fold = |s: &str| s.trim().to_lowercase();
        self.chain_presets
            .list(kind)
            .iter()
            .find(|l| fold(&l.name) == fold(name))
            .map(|l| l.name.clone())
    }

    /// 名前を付けて保存…: the name box, with the tag's name and a number not taken yet.
    pub(crate) fn save_chain_preset_as(&mut self, t: TrackId) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let kind = kind_of(track);
        let (name, _) = self.chain_presets.tag(track);
        let initial = if name == UNKNOWN {
            String::new()
        } else {
            (2..)
                .map(|k| format!("{name} {k}"))
                .find(|c| self.chain_preset_named(kind, c).is_none())
                .unwrap_or(name)
        };
        self.ask_chain_name("名前を付けて保存…", t, None, &initial);
        true
    }

    fn ask_chain_name(&mut self, label: &str, t: TrackId, over: Option<&str>, initial: &str) {
        let at = self.instruments.menu_at.unwrap_or_default();
        self.ask_name(
            label,
            "ui.chain.save",
            json!({ "track": t.to_string(), "over": over }),
            "name",
            initial,
            (at.x, at.bottom() + 4.0),
        );
    }

    /// Enter in the name box: a new name saves; a name the list has asks once more
    /// (上書きして保存…) and Enter on it again writes over that file.
    pub(crate) fn save_chain_preset_named(&mut self, t: TrackId, name: &str, over: Option<&str>) {
        let Some(kind) = self.project().tracks.get(&t).map(|x| kind_of(x)) else {
            return;
        };
        let fold = |s: &str| s.trim().to_lowercase();
        match self.chain_preset_named(kind, name) {
            Some(had) if over.is_none_or(|o| fold(o) != fold(&had)) => {
                self.ask_chain_name("上書きして保存…", t, Some(&had), &had);
            }
            had => {
                self.save_chain_preset(t, name, had.is_some());
            }
        }
    }

    /// The rows of the tag's list: the presets, the ✓ on the tag's, then 上書き保存 /
    /// 名前を付けて保存… / ブラウザーに表示 under them, as a device's.
    pub(crate) fn chain_menu_view(&self, t: TrackId) -> (Vec<Choice>, String) {
        let row = |to: &str, label: &str| Choice {
            to: to.into(),
            label: label.into(),
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return (Vec::new(), String::new());
        };
        let list = self.chain_presets.list(kind_of(track));
        let (name, off) = self.chain_presets.tag(track);
        let mut rows: Vec<Choice> = list
            .iter()
            .map(|l| row(&format!("{LOAD}{}", l.name), &l.name))
            .collect();
        if rows.is_empty() {
            rows.push(row("off:none", "プリセットがありません"));
        }
        let known = name != UNKNOWN;
        rows.push(row(
            if known && off { SAVE } else { "off:@save" },
            "上書き保存",
        ));
        rows.push(row(SAVE_AS, "名前を付けて保存…"));
        // The browser lists no chain presets yet.
        rows.push(row("off:@show", "ブラウザーに表示"));
        let now = if known {
            format!("{LOAD}{name}")
        } else {
            String::new()
        };
        (rows, now)
    }

    /// A row of the tag's list was picked (the list is shut already).
    pub(crate) fn chain_menu_pick(&mut self, t: TrackId, to: &str) {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let kind = kind_of(track);
        match to {
            SAVE => {
                let (name, _) = self.chain_presets.tag(track);
                if name != UNKNOWN {
                    self.save_chain_preset(t, &name, true);
                }
            }
            SAVE_AS => {
                self.save_chain_preset_as(t);
            }
            _ => {
                let Some(name) = to.strip_prefix(LOAD) else {
                    return;
                };
                let file = self
                    .chain_presets
                    .list(kind)
                    .iter()
                    .find(|l| l.name == name)
                    .map(|l| l.file.clone());
                if let Some(file) = file {
                    self.load_chain_preset(t, &file);
                }
            }
        }
    }

    /// After a command ran, on the window's thread or the worker: a whole chain loaded onto
    /// a track from the list, or saved from it, gives the track's tag that name.
    pub(crate) fn chain_preset_ran(&mut self, command: &str, args: &Value, data: &Value) {
        if !matches!(command, "preset.load" | "preset.save") {
            return;
        }
        let Some(t) = args["track"]
            .as_str()
            .and_then(|s| s.parse::<TrackId>().ok())
        else {
            return;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let kind = kind_of(track);
        let whole = args.get("of").is_none_or(Value::is_null);
        let name = match command {
            "preset.load" if whole && args["at"] == "track" && args["replace"] == true => {
                let file = args["preset"].as_str().unwrap_or_default();
                self.chain_presets
                    .list(kind)
                    .iter()
                    .find(|l| l.file == file)
                    .map(|l| l.name.clone())
            }
            "preset.save" if whole && args["kind"] == kind.name() => data["name"]
                .as_str()
                .or_else(|| args["name"].as_str())
                .map(|s| s.trim().to_string()),
            _ => None,
        };
        let (Some(name), Some(chain)) = (name, chain_of(track, kind)) else {
            return;
        };
        self.chain_presets
            .marks
            .insert(t, Mark { name, kind, chain });
        if command == "preset.save" {
            self.read_chain_presets(kind);
        }
    }
}
