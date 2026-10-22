//! OPS-006 / CTX-001: the screen-only rows of the menus as commands. A row that changes only the
//! screen (show a channel, open an effect's window, change a roll lane) has the name and the
//! arguments of a command in the table, so the menu, the palette, a key and the command line all
//! reach the same code: the menu builds `(name, args)` from its `Act` and `screen_command` runs it.
use std::str::FromStr;

use serde_json::{Map, Value, json};
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::CtrlKind;

use super::Act;
use super::browser::{BrowseAct, FxTarget};
use super::export::Row;
use crate::roll::LaneKind;
use crate::studio::{DrawerTab, Geometry, Studio};

/// The command a row of a menu stands for, with its arguments (targets are `ids`, positions `at`,
/// ranges `range`). Every row has one: a menu row is never more than a spelling of a command.
pub fn act_command(act: &Act) -> (&'static str, Value) {
    match act {
        Act::Tool(name) => (name, json!({})),
        Act::Seek(at) => ("transport.seek", json!({"at": at})),
        Act::PlayFrom(at) => ("transport.play", json!({"at": at})),
        Act::Lane(t, target) => (
            "auto.lane.show",
            json!({"track": t.to_string(), "target": target}),
        ),
        Act::HideLane(t, target) => (
            "auto.lane.hide",
            json!({"track": t.to_string(), "target": target.to_string()}),
        ),
        Act::SelectIn(f) => ("select.in_folder", json!({"ids": [f.to_string()]})),
        Act::Folders(open) => (
            "view.folders",
            json!({"state": if *open { "open" } else { "closed" }}),
        ),
        Act::Stems(tracks) => (
            "view.export",
            json!({"ids": tracks.iter().map(ToString::to_string).collect::<Vec<_>>()}),
        ),
        Act::FolderStem(f) => ("view.export", json!({"ids": [f.to_string()]})),
        Act::ExportLoop => ("view.export", json!({"range": "loop"})),
        Act::ExportSpan(from, to) => ("view.export", json!({"range": format!("{from}..{to}")})),
        Act::MidiExport(tracks, span) => (
            "view.export",
            json!({
                "kind": "midi",
                "ids": tracks.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "range": span.map_or_else(|| "song".to_string(), |(a, b)| format!("{a}..{b}")),
            }),
        ),
        Act::PointValue(t, target, at, ..) => (
            "auto.point.value",
            json!({"track": t.to_string(), "target": target.to_string(), "at": at}),
        ),
        Act::DeletePoints => ("clip.delete", json!({})),
        Act::Preset(id, name) => (
            "plugin.preset.choose",
            json!({"id": id.to_string(), "preset": name}),
        ),
        Act::PresetFile(id, save) => (
            "plugin.preset.file",
            json!({"id": id.to_string(), "save": save}),
        ),
        Act::RestartPlugin(id) => ("plugin.restart", json!({"id": id.to_string()})),
        Act::RangeFrame(place, from, to) => (
            "range.transform",
            match place {
                Some((t, target)) => json!({"track": t.to_string(), "target": target.to_string(),
                                            "from": from, "to": to}),
                None => json!({"tempo": true, "from": from, "to": to}),
            },
        ),
        Act::PickInsert(t) => ("plugin.pick", json!({"id": t.to_string()})),
        Act::InsertRecent(t, item) => ("plugin.insert", json!({"id": t.to_string(), "item": item})),
        Act::Browse(b) => match b {
            BrowseAct::UseNew(n) => ("browser.use", json!({"item": n, "into": "new"})),
            BrowseAct::UseHere(n) => ("browser.use", json!({"item": n, "into": "track"})),
            BrowseAct::Favourite(n, on) => ("browser.favorite", on_off(json!({"item": n}), *on)),
            BrowseAct::FavouriteFile(p, on) => (
                "browser.favorite",
                on_off(json!({"item": format!("file:{}", p.display())}), *on),
            ),
            BrowseAct::Hide(n, on) => ("browser.hide", on_off(json!({"item": n}), *on)),
            BrowseAct::Fx(target, n) => (
                "browser.fx",
                json!({"item": n, "into": match target {
                    FxTarget::Channel => "channel",
                    FxTarget::NewBus => "bus",
                    FxTarget::Master => "master",
                }}),
            ),
            BrowseAct::Groups(open) => (
                "browser.groups",
                json!({"state": if *open { "open" } else { "closed" }}),
            ),
            BrowseAct::ShowHidden(on) => ("view.browser.hidden", on_off(json!({}), *on)),
            BrowseAct::Rescan => ("plugin.rescan", json!({})),
            BrowseAct::PutNew(p) => (
                "browser.put",
                json!({"path": p.display().to_string(), "into": "new"}),
            ),
            BrowseAct::PutHere(p) => (
                "browser.put",
                json!({"path": p.display().to_string(), "into": "track"}),
            ),
            BrowseAct::AddPlace(p) => ("location.add", json!({"path": p.display().to_string()})),
            BrowseAct::RemovePlace(key) => ("location.remove", json!({"location": key})),
            BrowseAct::Reveal(p) => ("file.reveal_os", json!({"path": p.display().to_string()})),
            BrowseAct::SelectUsing(f) => ("select.using_file", json!({"file": f})),
            BrowseAct::Locate(f) => ("file.locate", json!({"path": f})),
            BrowseAct::LocateFolder => ("file.locate", json!({})),
            BrowseAct::Cleanup => ("view.cleanup", json!({})),
        },
        Act::Export(e) => (
            match e.row {
                Row::Listen => "export.preview",
                Row::Rewrite => "export.update",
                Row::Same => "export.open",
                Row::Import => "export.import",
                Row::Reveal => "export.reveal_os",
                Row::Trash => "export.delete",
            },
            match &e.file {
                Some(f) => json!({"job": e.job, "file": f}),
                None => json!({"job": e.job}),
            },
        ),
        Act::Open => ("clip.open", json!({})),
        Act::Reveal(t) => ("track.reveal", json!({"id": t.to_string(), "in": "tracks"})),
        Act::ShowStrip(t) => ("track.reveal", json!({"id": t.to_string(), "in": "mixer"})),
        Act::SelectSources(t) => ("select.sources", json!({"id": t.to_string()})),
        Act::RollPitch(p) => ("select.pitch", json!({"pitch": p})),
        Act::OpenPlugin(t, id) => (
            "plugin.editor.open",
            json!({"track": t.to_string(), "id": id.to_string()}),
        ),
        Act::RollLaneKind(index, kind) => (
            "cc.lane.set",
            json!({"index": index, "lane": lane_name(*kind)}),
        ),
        Act::RollLaneHide(index) => ("cc.lane.hide", json!({"index": index})),
        Act::MidiLearn(target) => ("midimap.learn", json!({"target": target})),
        Act::MidiLearnCancel => ("midimap.learn", json!({"cancel": true})),
        Act::MidiLearnConfirm => ("midimap.learn", json!({"confirm": true})),
    }
}

/// `args` with the flag `--on` or `--off` for a switch.
fn on_off(mut args: Value, on: bool) -> Value {
    args[if on { "on" } else { "off" }] = json!(true);
    args
}

fn lane_name(kind: LaneKind) -> String {
    match kind {
        LaneKind::Velocity => "velocity".to_string(),
        LaneKind::Ctrl(k) => k.key(),
    }
}

fn lane_of(s: &str) -> Option<LaneKind> {
    if s == "velocity" {
        return Some(LaneKind::Velocity);
    }
    CtrlKind::parse(s).map(LaneKind::Ctrl)
}

fn id<T: FromStr>(args: &Map<String, Value>, key: &str) -> Option<T> {
    args.get(key)?.as_str()?.parse().ok()
}

fn index(args: &Map<String, Value>) -> Option<usize> {
    args.get("index")?.as_u64().map(|n| n as usize)
}

impl Studio {
    /// Runs a command that only changes the screen. `None` means the name is not one of these;
    /// `Some(false)` is a name that found nothing to do (a missing channel, a bad argument).
    pub(crate) fn screen_command(
        &mut self,
        g: &Geometry,
        name: &str,
        args: &Map<String, Value>,
    ) -> Option<bool> {
        Some(match name {
            "clip.open" => self.open_selected_clip(g, false),
            // Without a scale or a tilt it puts the frame round the range (AUT-010).
            "range.transform" if !args.contains_key("scale") && !args.contains_key("tilt") => {
                self.range_frame_command(g, args)
            }
            "view.dock" => {
                let tab = match args.get("tab").and_then(Value::as_str) {
                    Some("roll") => DrawerTab::Roll,
                    Some("mixer") => DrawerTab::Mixer,
                    _ => return Some(false),
                };
                let shown = self.drawer_open && self.tab == tab;
                let state = args.get("state").and_then(Value::as_str);
                // Closing a tab that is not the one shown leaves the dock as it is.
                self.drawer_open = match state {
                    Some("open") => true,
                    Some("closed") => self.drawer_open && !shown,
                    _ => !shown,
                };
                if self.drawer_open && state != Some("closed") {
                    self.tab = tab;
                }
                self.resized(g);
                true
            }
            "track.reveal" => {
                let Some(t) = id::<TrackId>(args, "id") else {
                    return Some(false);
                };
                if args.get("in").and_then(Value::as_str) == Some("mixer") {
                    self.show_strip(g, t)
                } else {
                    self.reveal_row(g, t)
                }
            }
            "mixer.focus" => match id::<TrackId>(args, "id") {
                Some(t) => self.focus_strip(g, t),
                None => false,
            },
            "select.sources" => match id::<TrackId>(args, "id") {
                Some(t) => self.select_sources(t),
                None => false,
            },
            "select.pitch" => {
                let Some(pitch) = args.get("pitch").and_then(Value::as_u64) else {
                    return Some(false);
                };
                let Some(ctx) = self.roll_ctx(g) else {
                    return Some(false);
                };
                self.roll.select_pitch(&ctx, pitch.min(127) as u8);
                true
            }
            "plugin.editor.open" => {
                let (Some(t), Some(p)) = (id::<TrackId>(args, "track"), id::<PluginId>(args, "id"))
                else {
                    return Some(false);
                };
                self.open_plugin(t, p);
                true
            }
            "cc.lane.set" => {
                let kind = args.get("lane").and_then(Value::as_str).and_then(lane_of);
                match (index(args), kind) {
                    (Some(i), Some(k)) => self.roll.set_lane_kind(i, k),
                    _ => false,
                }
            }
            "cc.lane.hide" => match index(args) {
                Some(i) => self.roll.hide_lane(i),
                None => false,
            },
            _ => return self.screen_rows(g, name, args),
        })
    }
}
