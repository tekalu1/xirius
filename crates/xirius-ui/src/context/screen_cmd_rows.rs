//! OPS-006 / CTX-001 / CTX-002: the other screen rows of the menus as commands (the export panel,
//! the lanes of automation, the effect pickers, the preset file, the browser's rows).
//!
//! `screen_cmd` has the rows that only show something; these change the song's screen state or the
//! browser's own. Each name is a command of the table (`screen_ops`, or one the CLI has too) and
//! its arguments follow the one convention: targets are `--ids`, positions `--at`, ranges
//! `--range`. A menu row builds `(name, args)` from its `Act` (`act_command`) and the window
//! answers it here, so a typed command and the row do the same thing.

use std::path::PathBuf;

use serde_json::{Map, Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::id::{AnyId, PluginId, TrackId};
use xirius_core::time::Tick;

use super::browser::{BrowseAct, FxTarget};
use super::export::{ExportAct, Row};
use crate::autovalue::AutoRef;
use crate::browser::intern;
use crate::export::Range;
use crate::popup::PopupKind;
use crate::studio::{FileRequest, Geometry, Studio};

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}

fn id<T: std::str::FromStr>(args: &Map<String, Value>, key: &str) -> Option<T> {
    text(args, key)?.parse().ok()
}

fn flag(args: &Map<String, Value>, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true",
        _ => false,
    }
}

/// The ids of `--ids` (a list, or one id said as a text).
fn ids_of(args: &Map<String, Value>) -> Vec<AnyId> {
    match args.get("ids") {
        Some(Value::Array(xs)) => xs.iter().filter_map(|x| x.as_str()?.parse().ok()).collect(),
        Some(Value::String(s)) => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        _ => Vec::new(),
    }
}

/// What a flag pair `--on` / `--off` asks for; `default` when neither is said.
fn wanted(args: &Map<String, Value>, default: bool) -> bool {
    if flag(args, "off") {
        false
    } else if flag(args, "on") {
        true
    } else {
        default
    }
}

impl Studio {
    /// A position said as ticks or as `bar.beat.tick`.
    pub(crate) fn tick_arg(&self, v: Option<&Value>) -> Option<Tick> {
        match v? {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => self.project().time_map().parse(s).ok(),
            _ => None,
        }
    }

    fn said(&mut self, line: &str) -> Option<bool> {
        self.error = Some(line.to_string());
        Some(false)
    }

    /// The commands of the rows that are not only a look at the song. `None` for a name that is
    /// not one of these (or has not the arguments of one), `Some(false)` when it found nothing.
    pub(crate) fn screen_rows(
        &mut self,
        g: &Geometry,
        name: &str,
        args: &Map<String, Value>,
    ) -> Option<bool> {
        match name {
            "transport.play" if args.contains_key("at") => {
                let Some(at) = self.tick_arg(args.get("at")) else {
                    return self.said("--at の位置を読み取れません");
                };
                Some(self.play_from(at))
            }
            "view.folders" => {
                let open = match text(args, "state") {
                    Some("open") => true,
                    Some("closed") => false,
                    _ => return Some(false),
                };
                if self
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    .arrange_run(
                        "view.folders",
                        json!({"state": if open { "open" } else { "closed" }}),
                    )
                    .is_none()
                {
                    return Some(false);
                }
                Some(self.fold_all(g, !open))
            }
            "select.in_folder" => {
                let Some(AnyId::Folder(f)) = ids_of(args).first().copied() else {
                    return Some(false);
                };
                if self
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    .arrange_run("select.in_folder", json!({"ids": [f.to_string()]}))
                    .is_none()
                {
                    return Some(false);
                }
                Some(self.select_in(f))
            }
            "auto.lane.show" => {
                let (Some(t), Some(target)) = (id::<TrackId>(args, "track"), text(args, "target"))
                else {
                    return Some(false);
                };
                Some(self.show_lane(g, t, target))
            }
            "auto.lane.hide" => {
                let (Some(t), Some(target)) = (
                    id::<TrackId>(args, "track"),
                    text(args, "target").and_then(|s| s.parse::<AutoTarget>().ok()),
                ) else {
                    return Some(false);
                };
                Some(self.hide_lane(g, t, &target))
            }
            "auto.point.value" => {
                let (Some(t), Some(target), Some(at)) = (
                    id::<TrackId>(args, "track"),
                    text(args, "target").and_then(|s| s.parse::<AutoTarget>().ok()),
                    self.tick_arg(args.get("at")),
                ) else {
                    return Some(false);
                };
                let Some(point) = self.auto_ref(t, &target, at) else {
                    return Some(false);
                };
                let (x, y) = self.input_map.pointer;
                Some(self.auto_value_open(point, x, y))
            }
            "view.export" => Some(self.view_export(g, args)),
            "view.export.close" => Some(
                self.popup
                    .as_ref()
                    .is_some_and(|p| p.kind == PopupKind::Export)
                    && self.export_action(crate::export::Act::Close),
            ),
            "mixer.channel.next" => Some(self.step_channel(g, true)),
            "mixer.channel.prev" => Some(self.step_channel(g, false)),
            "plugin.preset.next" => Some(self.step_preset(true)),
            "plugin.preset.prev" => Some(self.step_preset(false)),
            "plugin.preset.choose" => {
                let (Some(p), Some(preset)) = (id::<PluginId>(args, "id"), text(args, "preset"))
                else {
                    return Some(false);
                };
                Some(self.load_preset(p, preset))
            }
            "plugin.preset.file" => {
                let Some(p) = id::<PluginId>(args, "id") else {
                    return Some(false);
                };
                self.file_request = Some(if flag(args, "save") {
                    FileRequest::PresetSave(p)
                } else {
                    FileRequest::PresetOpen(p)
                });
                Some(true)
            }
            "plugin.restart" => {
                match id::<PluginId>(args, "id") {
                    Some(p) => self.restart_plugin(p),
                    None => self.restart_plugins(),
                };
                Some(true)
            }
            "midimap.learn" => Some(self.midi_learn(args)),
            "plugin.pick" => {
                let Some(t) = id::<TrackId>(args, "id") else {
                    return Some(false);
                };
                Some(self.pick_insert(g, t))
            }
            "plugin.insert" => {
                let (Some(t), Some(item)) = (id::<TrackId>(args, "id"), text(args, "item")) else {
                    return Some(false);
                };
                Some(self.insert_recent(t, item))
            }
            "plugin.rescan" if !args.contains_key("job") => {
                Some(self.browse_run(BrowseAct::Rescan))
            }
            "browser.use" => {
                let Some(item) = text(args, "item").map(intern) else {
                    return Some(false);
                };
                Some(self.browse_run(match text(args, "into") {
                    Some("track") => BrowseAct::UseHere(item),
                    _ => BrowseAct::UseNew(item),
                }))
            }
            "browser.fx" => {
                let Some(item) = text(args, "item").map(intern) else {
                    return Some(false);
                };
                let target = match text(args, "into") {
                    Some("bus") => FxTarget::NewBus,
                    Some("master") => FxTarget::Master,
                    _ => FxTarget::Channel,
                };
                Some(self.browse_run(BrowseAct::Fx(target, item)))
            }
            "browser.groups" => {
                let open = match text(args, "state") {
                    Some("open") => true,
                    Some("closed") => false,
                    _ => return Some(false),
                };
                Some(self.browse_run(BrowseAct::Groups(open)))
            }
            "browser.put" => {
                let Some(path) = text(args, "path").map(PathBuf::from) else {
                    return Some(false);
                };
                Some(self.browse_run(match text(args, "into") {
                    Some("track") => BrowseAct::PutHere(path),
                    _ => BrowseAct::PutNew(path),
                }))
            }
            "browser.favorite" => {
                let Some(item) = text(args, "item") else {
                    return Some(false);
                };
                let on = wanted(args, true);
                Some(match item.strip_prefix("file:") {
                    Some(path) => {
                        self.browse_run(BrowseAct::FavouriteFile(PathBuf::from(path), on))
                    }
                    None => self.browse_run(BrowseAct::Favourite(intern(item), on)),
                })
            }
            "browser.hide" => {
                let Some(item) = text(args, "item").map(intern) else {
                    return Some(false);
                };
                Some(self.browse_run(BrowseAct::Hide(item, wanted(args, true))))
            }
            "view.browser.hidden" => {
                let on = wanted(args, !self.browser.show_hidden);
                Some(self.browse_run(BrowseAct::ShowHidden(on)))
            }
            "location.add" => {
                let Some(path) = text(args, "path").map(PathBuf::from) else {
                    return Some(false);
                };
                // A tab with a name of its own is the CLI's: the window names it after the folder.
                if args.contains_key("name") {
                    return None;
                }
                Some(self.browse_run(BrowseAct::AddPlace(path)))
            }
            "location.remove" => {
                let Some(key) = text(args, "location") else {
                    return Some(false);
                };
                Some(self.browse_run(BrowseAct::RemovePlace(key.to_string())))
            }
            "select.using_file" => {
                let file = text(args, "file")?.to_string();
                Some(self.browse_run(BrowseAct::SelectUsing(file)))
            }
            // Without the place, the window asks for it: a file for one, a folder for all.
            "file.locate" if text(args, "new_path").is_none() && text(args, "folder").is_none() => {
                Some(self.browse_run(match text(args, "path") {
                    Some(f) => BrowseAct::Locate(f.to_string()),
                    None => BrowseAct::LocateFolder,
                }))
            }
            "file.reveal_os" => {
                let Some(path) = text(args, "path").map(PathBuf::from) else {
                    return Some(false);
                };
                Some(self.browse_run(BrowseAct::Reveal(path)))
            }
            "export.preview" | "export.update" | "export.open" | "export.reveal_os"
            | "export.delete"
                if text(args, "job").is_some() && !flag(args, "stop") && !flag(args, "status") =>
            {
                let row = match name {
                    "export.preview" => Row::Listen,
                    "export.update" => Row::Rewrite,
                    "export.open" => Row::Same,
                    "export.reveal_os" => Row::Reveal,
                    _ => Row::Trash,
                };
                let job = text(args, "job")?.to_string();
                let anchor = self.export_anchor(g);
                if name == "export.delete" {
                    // Typed, the trash asks once as the card's button does; the menu's own
                    // confirming row is the one that goes straight to it.
                    self.popup = None;
                    self.open_export(anchor);
                    return Some(self.export_action(crate::export::Act::Delete(job)));
                }
                // The panel open already would be shut by opening it again; the row's own
                // menu has taken its place, so the typed command does the same.
                if self
                    .popup
                    .as_ref()
                    .is_some_and(|p| p.kind == PopupKind::Export)
                {
                    self.popup = None;
                }
                let file = text(args, "file").map(str::to_string);
                Some(self.export_run_row(ExportAct {
                    job,
                    row,
                    anchor,
                    file,
                }))
            }
            _ => None,
        }
    }

    /// The point of a lane at a song position, as the value box names it.
    pub(crate) fn auto_ref(
        &self,
        track: TrackId,
        target: &AutoTarget,
        at: Tick,
    ) -> Option<AutoRef> {
        let p = self.project();
        let (lane, l) = p
            .tracks
            .get(&track)?
            .automation
            .iter()
            .enumerate()
            .find(|(_, l)| l.target == *target)?;
        let index = l.points.iter().position(|pt| pt.tick == at)?;
        Some(AutoRef { track, lane, index })
    }

    /// `view.export`: opens the export panel for the tracks or folder of `--ids`, or for the
    /// `--range` (`song`, `loop`, `section:ID`, `a..b`).
    fn view_export(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        match text(args, "kind") {
            None => {}
            Some("midi") => return self.view_midi_export(g, args),
            Some(k) => {
                self.error = Some(format!("--kind に `{k}` は使えません"));
                return false;
            }
        }
        let ids = ids_of(args);
        if !ids.is_empty() {
            return match ids.as_slice() {
                [AnyId::Folder(f)] => self.open_folder_stem(g, *f),
                _ => {
                    let tracks: Vec<TrackId> = ids
                        .iter()
                        .filter_map(|i| match i {
                            AnyId::Track(t) => Some(*t),
                            _ => None,
                        })
                        .collect();
                    self.open_stems(g, tracks)
                }
            };
        }
        match text(args, "range").unwrap_or("song") {
            "song" => {
                self.load_export();
                self.export.set_stems(false);
                self.export.range = Range::Song;
                self.export.fixed_span = None;
                self.open_export(self.export_anchor(g));
                true
            }
            "loop" => self.open_loop_export(g),
            r => {
                let Some((from, to)) = self.range_span(r) else {
                    return false;
                };
                self.open_span_export(g, from, to);
                true
            }
        }
    }

    /// The span of a `--range` of `section:ID` or `a..b`, or the error.
    fn range_span(&mut self, r: &str) -> Option<(Tick, Tick)> {
        let span = match r.strip_prefix("section:") {
            Some(key) => {
                let p = self.project();
                p.sections
                    .iter()
                    .find(|s| s.id.to_string() == key.trim() || s.name == key.trim())
                    .map(|s| (s.start, s.start + s.length))
            }
            None => r.split_once("..").and_then(|(a, b)| {
                Some((
                    self.tick_arg(Some(&json!(a.trim())))?,
                    self.tick_arg(Some(&json!(b.trim())))?,
                ))
            }),
        };
        let got = span.filter(|(a, b)| a < b);
        if got.is_none() {
            self.error = Some(format!("--range に `{r}` は使えません"));
        }
        got
    }

    /// `view.export --kind midi`: the MIDI files of the instrument tracks of `--ids` (all of
    /// them when none) over the `--range`.
    fn view_midi_export(&mut self, g: &Geometry, args: &Map<String, Value>) -> bool {
        let tracks: Vec<TrackId> = ids_of(args)
            .iter()
            .filter_map(|i| match i {
                AnyId::Track(t) => Some(*t),
                _ => None,
            })
            .collect();
        let (range, span) = match text(args, "range").unwrap_or("song") {
            "song" => (Range::Song, None),
            "loop" => (Range::Loop, None),
            r => match self.range_span(r) {
                Some(s) => (Range::Selection, Some(s)),
                None => return false,
            },
        };
        self.open_midi(g, tracks, range, span);
        true
    }

    /// Opens the export panel on the MIDI files of these tracks (every instrument track when
    /// none), over `range`; a span is fixed to this song (UI-280).
    pub(super) fn open_midi(
        &mut self,
        g: &Geometry,
        tracks: Vec<TrackId>,
        range: Range,
        span: Option<(Tick, Tick)>,
    ) {
        self.load_export();
        let x = &mut self.export;
        x.set_stems(false);
        x.midi = true;
        x.each = false;
        x.midi_picked = (!tracks.is_empty()).then_some(tracks);
        x.range = range;
        x.fixed_span = span;
        x.fixed_song = span.and(self.song.dir());
        self.open_export(self.export_anchor(g));
    }

    /// Opens the export panel for a fixed span of this song.
    pub(super) fn open_span_export(&mut self, g: &Geometry, from: Tick, to: Tick) {
        self.load_export();
        self.export.set_stems(false);
        self.export.range = Range::Selection;
        self.export.fixed_span = Some((from, to));
        self.export.fixed_song = self.song.dir();
        self.open_export(self.export_anchor(g));
    }
}
