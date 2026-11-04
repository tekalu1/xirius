//! The channel editor's preset tag (MIX-042, PLG-065): «‹ ▾ name ● ›» in its bar, the
//! device window's parts (UI-332) over the track's whole chain: an audio or a bus track's FX
//! (`fx-chain`), an instrument track's instrument and FX (`instrument-fx`), a
//! multi-instrument's layers (`multi-instrument`). The list is the chain presets of that
//! kind the session keeps, by name; ‹ › load the one before or after the tag's (they stop at
//! the list's ends, as the mock's), a row loads it over the whole chain, 上書き保存 and
//! 名前を付けて保存… write the chain.
//!
//! The list starts with [`DEFAULT`], a chain of no effects (an audio or a bus track's empty
//! FX, an instrument track's instrument alone, a multi-instrument's layers and key ranges
//! alone), as a device window's (review 3 中5). The song keeps no chain preset's name, so the
//! tag names, as the device window's: the preset this screen last loaded onto the track or
//! saved from it while the song is open, while the chain is still that; else `Default` when
//! the chain is of no effects (every new track's); else the saved preset the chain is; else
//! the one loaded or saved last, marked; else [`UNKNOWN`], marked. So a song opened again
//! shows the names it showed. A tag that names no row sits before the list: its › loads
//! `Default`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use xirius_core::chain_preset::{Chain, ChainKind};
use xirius_core::id::{NodeId, PluginId, TrackId};
use xirius_core::model::{PluginKind, Track, TrackKind};
use xirius_core::rack::{Rack, RackNode, Source};
pub use xirius_core::xpreset::DEFAULT_PRESET as DEFAULT;

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
    /// Its chain as [`Chain::shape`] gives it, to tell a track's chain is it.
    pub shape: Option<Value>,
}

/// The preset a track's tag names and the chain as it was loaded or saved (none: no chain a
/// preset holds, as an audio track's empty FX after `Default`).
#[derive(Clone, Debug)]
struct Mark {
    name: String,
    kind: ChainKind,
    shape: Option<Value>,
}

#[derive(Debug, Default)]
pub struct ChainPresets {
    marks: HashMap<TrackId, Mark>,
    /// The lists by kind, as last read.
    lists: HashMap<&'static str, Vec<Listed>>,
    read_at: Option<Instant>,
    /// Each track's chain by [`Chain::shape`] as the tag last took it, kept while the track is
    /// the same: the tag is asked at each paint, the capture reads every knob.
    shapes: RefCell<HashMap<TrackId, Shape>>,
}

/// A track as the tag took it, the kind it took it as and its shape.
type Shape = (Arc<Track>, ChainKind, Option<Value>);

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

/// The whole chain of `t` as a preset of `kind` holds it, by [`Chain::shape`]; none when it
/// cannot be one.
fn shape_of(t: &Track, kind: ChainKind) -> Option<Value> {
    Chain::capture(t, kind, None).ok().map(|(c, _)| c.shape())
}

/// The rack of `t`, a serial track's as its order.
fn rack_of(t: &Track) -> Rack {
    t.rack.clone().unwrap_or_else(|| Rack::serial(t))
}

/// The chains of `rack` effects sit in: its FX and each layer's.
fn chains(rack: &Rack) -> Vec<&[RackNode]> {
    let mut v = vec![rack.fx.as_slice()];
    if let Some(Source::Multi { layers, .. }) = &rack.source {
        v.extend(layers.iter().map(|l| l.fx.as_slice()));
    }
    v
}

/// The splitters in `nodes`, the ones in their branches too.
fn splitters(nodes: &[RackNode], out: &mut Vec<NodeId>) {
    for n in nodes {
        if let RackNode::Split(x) = n {
            out.push(x.id);
            for b in &x.branches {
                splitters(&b.fx, out);
            }
        }
    }
}

/// Is the chain of `t` the `Default` one: no effect and no splitter, the instrument or the
/// layers and their key ranges as they are.
pub fn is_default(t: &Track) -> bool {
    t.inserts.iter().all(|x| x.kind != PluginKind::Effect)
        && chains(&rack_of(t)).iter().all(|c| c.is_empty())
}

impl ChainPresets {
    /// The tag of `t`: the preset's name and whether the chain moved off it (the module's
    /// order).
    pub fn tag(&self, t: &Arc<Track>) -> (String, bool) {
        let kind = kind_of(t);
        let mark = self.marks.get(&t.id).filter(|m| m.kind == kind);
        let now = self.shape(t, kind);
        if let Some(m) = mark.filter(|m| m.shape == now) {
            return (m.name.clone(), false);
        }
        if is_default(t) {
            return (DEFAULT.into(), false);
        }
        let saved = now.as_ref().and_then(|now| {
            self.list(kind)
                .iter()
                .find(|l| l.shape.as_ref() == Some(now))
        });
        match (saved, mark) {
            (Some(l), _) => (l.name.clone(), false),
            (None, Some(m)) => (m.name.clone(), true),
            (None, None) => (UNKNOWN.into(), true),
        }
    }

    /// [`shape_of`] `t`, taken again only when the track changed: a change makes a new one.
    fn shape(&self, t: &Arc<Track>, kind: ChainKind) -> Option<Value> {
        let mut shapes = self.shapes.borrow_mut();
        if let Some((was, k, shape)) = shapes.get(&t.id)
            && Arc::ptr_eq(was, t)
            && *k == kind
        {
            return shape.clone();
        }
        let shape = shape_of(t, kind);
        shapes.insert(t.id, (t.clone(), kind, shape.clone()));
        shape
    }

    /// The presets of `kind` as last read.
    pub fn list(&self, kind: ChainKind) -> &[Listed] {
        self.lists.get(kind.name()).map_or(&[], Vec::as_slice)
    }

    /// The names of the tag's rows: `Default`, then the presets of `kind`.
    pub fn names(&self, kind: ChainKind) -> Vec<String> {
        std::iter::once(DEFAULT.to_string())
            .chain(self.list(kind).iter().map(|l| l.name.clone()))
            .collect()
    }

    /// Whether the ‹ and the › of `t` can step: none past the ends. A tag that names no row
    /// sits before the first, so › loads `Default` (review 3 中5).
    pub fn steps(&self, t: &Arc<Track>) -> (bool, bool) {
        let names = self.names(kind_of(t));
        let (name, _) = self.tag(t);
        match names.iter().position(|n| *n == name) {
            Some(k) => (k > 0, k + 1 < names.len()),
            None => (false, true),
        }
    }

    /// The song closed: its tracks' ids come again in the next.
    pub fn forget(&mut self) {
        self.marks.clear();
        self.shapes.get_mut().clear();
    }
}

impl Studio {
    /// Reads the presets of `kind` again; true when the list changed.
    pub(crate) fn read_chain_presets(&mut self, kind: ChainKind) -> bool {
        self.chain_presets.read_at = Some(Instant::now());
        let args = json!({ "kind": kind.name(), "values": true });
        // design-lint: allow(ui-blocking-io) MIX-042: on the window thread the session answers the list from its cache and reads the files on a background job.
        let Ok(data) = self.song.run("preset.list", args) else {
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
                    shape: p.get("chain").cloned(),
                })
            })
            // `Default` is the list's own row; a file of the name cannot be saved.
            .filter(|l| !l.name.trim().eq_ignore_ascii_case(DEFAULT))
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

    /// `Default` over the whole chain of `t`, in one step to take back: its splitters and
    /// effects go, the instrument or the layers and their key ranges stay.
    pub(crate) fn load_default_chain(&mut self, t: TrackId) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let kind = kind_of(track);
        let effects: Vec<PluginId> = track
            .inserts
            .iter()
            .filter(|x| x.kind == PluginKind::Effect)
            .map(|x| x.id)
            .collect();
        let mut splits = Vec::new();
        for c in chains(&rack_of(track)) {
            splitters(c, &mut splits);
        }
        self.song.session_mut().begin_group();
        let mut done = true;
        if !effects.is_empty() {
            let ids: Vec<String> = effects.iter().map(ToString::to_string).collect();
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            done &= self.run_command("plugin.remove", json!({ "ids": ids }));
        }
        // Emptied, a splitter lays its (empty) branches in its place.
        for id in splits {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            done &= self.run_command("rack.split.remove", json!({ "id": id.to_string() }));
        }
        self.song.session_mut().end_group();
        // A refused step leaves effects on: the tag names what is there, not Default.
        if done && let Some(track) = self.project().tracks.get(&t) {
            let shape = shape_of(track, kind);
            self.chain_presets.marks.insert(
                t,
                Mark {
                    name: DEFAULT.into(),
                    kind,
                    shape,
                },
            );
        }
        done
    }

    /// Loads the row `name` of the tag's list over the whole chain of `t`.
    fn load_chain_row(&mut self, t: TrackId, kind: ChainKind, name: &str) -> bool {
        if name == DEFAULT {
            return self.load_default_chain(t);
        }
        let file = self
            .chain_presets
            .list(kind)
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.file.clone());
        file.is_some_and(|f| self.load_chain_preset(t, &f))
    }

    /// The ‹ (`forward` false) or the › of the tag of `t`.
    pub(crate) fn step_chain_preset(&mut self, t: TrackId, forward: bool) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let kind = kind_of(track);
        let (name, _) = self.chain_presets.tag(track);
        let names = self.chain_presets.names(kind);
        let to = match names.iter().position(|n| *n == name) {
            Some(k) if forward => k + 1,
            Some(k) => k.wrapping_sub(1),
            // a tag before the list: › goes to its first
            None if forward => 0,
            None => return false,
        };
        let Some(name) = names.get(to) else {
            return false;
        };
        self.load_chain_row(t, kind, name)
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

    /// 名前を付けて保存…: the name box, with the tag's name and a number not taken yet
    /// (empty at `Default`, as a device's).
    pub(crate) fn save_chain_preset_as(&mut self, t: TrackId) -> bool {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let kind = kind_of(track);
        let (name, _) = self.chain_presets.tag(track);
        let initial = if name == UNKNOWN || name == DEFAULT {
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

    /// The rows of the tag's list: `Default` and the presets, the ✓ on the tag's, then
    /// 上書き保存 / 名前を付けて保存… / ブラウザーに表示 under them, as a device's.
    pub(crate) fn chain_menu_view(&self, t: TrackId) -> (Vec<Choice>, String) {
        let row = |to: &str, label: &str| Choice {
            to: to.into(),
            label: label.into(),
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return (Vec::new(), String::new());
        };
        let names = self.chain_presets.names(kind_of(track));
        let (name, off) = self.chain_presets.tag(track);
        let mut rows: Vec<Choice> = names
            .iter()
            .map(|n| row(&format!("{LOAD}{n}"), n))
            .collect();
        let known = name != UNKNOWN;
        // `Default` is no file to write over.
        let over = known && off && name != DEFAULT;
        rows.push(row(if over { SAVE } else { "off:@save" }, "上書き保存"));
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
                if name != UNKNOWN && name != DEFAULT {
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
                self.load_chain_row(t, kind, name);
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
        let (Some(name), Some(shape)) = (name, shape_of(track, kind)) else {
            return;
        };
        let shape = Some(shape);
        self.chain_presets
            .marks
            .insert(t, Mark { name, kind, shape });
        if command == "preset.save" {
            self.read_chain_presets(kind);
        }
    }
}
