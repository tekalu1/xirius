//! The menus of an instrument: the preset list of its window (UI-098) and
//! the right press on its row at the head of the inserts (CTX-040), and the
//! right press on an instrument in the browser (CTX-048).
//!
//! A menu is a list of [`Choice`]s: `to` says what the row does, and a row
//! that cannot run now has `to` starting with [`OFF`]; [`GAP`] puts space
//! between two groups, and a row whose `to` starts with [`FOOT`] goes under
//! the list, where the query never filters it. A preset's row is [`LOAD`] and
//! its name, so no name can read as one of those.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, Project};

use super::current;
use super::presets::Presets;
use super::window::is_built_in;
use crate::popup::{Action, Choice, PopupKind};
use crate::studio::{Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

/// What a row that cannot run now starts with.
pub const OFF: &str = "off:";
/// The space between two groups.
pub const GAP: &str = "-";
/// What a row under the list starts with (the preset list's 上書き保存, UI-332).
pub const FOOT: &str = "@";
/// The preset list's rows under it (UI-332).
pub const SAVE: &str = "@save";
pub const SAVE_AS: &str = "@save-as";
pub const SHOW: &str = "@show";
/// What a preset's row starts with, before its name (a name is anything but empty and
/// Default: `-`, `@save` and `off:x` are names too).
pub const LOAD: &str = "load:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Destination(TrackId, PluginId),
    ChannelDestination(TrackId),
    /// The presets of a built-in instrument (the box at the foot of its window).
    Preset(TrackId, PluginId),
    /// The right press on an instrument track's instrument row (S30).
    Row(TrackId, PluginId),
    /// The parameters to show a lane of (the row's オートメーションを表示).
    Params(TrackId, PluginId),
    /// The chain presets of a track (the channel editor's tag, MIX-042).
    Chain(TrackId),
}

/// A row's icon and its key, from what it does.
fn look(menu: Menu, to: &str) -> (Option<&'static str>, &'static str) {
    let key = match (menu, to) {
        (Menu::Row(..), "open") => "Shift+F11",
        (Menu::Row(..), "auto") => "A",
        (Menu::Row(..), "copy") => "Ctrl+C",
        (Menu::Row(..), "remove") => "Del",
        _ => "",
    };
    (None, key)
}

/// The instrument `id` of track `t`, while it is still there.
fn plugin_of(p: &Project, t: TrackId, id: PluginId) -> Option<&Plugin> {
    p.tracks
        .get(&t)
        .and_then(|t| current(t))
        .filter(|x| x.id == id)
}

/// The built-in device `id` wherever it is, the instrument or an effect
/// (the one `load_preset` loads into).
fn device_of(p: &Project, id: PluginId) -> Option<&Plugin> {
    p.tracks
        .values()
        .flat_map(|t| &t.inserts)
        .find(|x| x.id == id)
}

/// The parameters of an instrument a lane can follow, as (id, name): the
/// built-in one's knobs, else what the plugin keeps.
pub fn params(plugin: &Plugin) -> Vec<(String, String)> {
    if let Some(def) = plugin.builtin() {
        return def
            .params
            .iter()
            .map(|p| (p.id.to_string(), p.label.to_string()))
            .collect();
    }
    plugin
        .params
        .keys()
        .map(|k| (k.clone(), k.clone()))
        .collect()
}

/// The menu as a list menu's rows (UI-320): [`GAP`] opens a new unnamed
/// group, a row that cannot run is dim.
pub fn pick(menu: Menu, choices: Vec<Choice>, current: &str) -> crate::pick_list::PickList {
    use crate::pick_list::{PickGroup, PickList, PickRow};
    let mut groups: Vec<PickGroup> = vec![PickGroup::new("", Vec::new())];
    let mut foot = Vec::new();
    for ch in choices {
        if ch.to == GAP {
            groups.push(PickGroup::new("", Vec::new()));
            continue;
        }
        let to = ch.to.trim_start_matches(OFF);
        let (_, key) = look(menu, to);
        let row = PickRow::new(ch.label).hint(key).on(ch.to == current);
        let row = match ch.to.strip_prefix(OFF) {
            Some(_) => row.disabled(),
            None => row.act(Action::Pick(ch.to.clone())),
        };
        if to.starts_with(FOOT) {
            foot.push(row);
        } else {
            groups.last_mut().unwrap().rows.push(row);
        }
    }
    groups.retain(|g| !g.rows.is_empty());
    let mut list = PickList::of(Vec::new()).footer(foot);
    list.groups = groups;
    list
}

/// The rows of a menu as it is now, and the row that is on.
pub fn view(p: &Project, presets: &Presets, menu: Menu) -> (Vec<Choice>, String) {
    let row = |to: &str, label: &str| Choice {
        to: to.into(),
        label: label.into(),
    };
    match menu {
        Menu::Preset(_, id) => {
            // The device's presets, the ✓ on the one its tag names (the knobs
            // off it or not), then 上書き保存 / 名前を付けて保存… / ブラウザーに表示
            // under them.
            let Some((x, def)) = device_of(p, id).and_then(|x| Some((x, x.builtin()?))) else {
                return (Vec::new(), String::new());
            };
            let list = presets.names(def);
            let (name, off) = presets.tag_of(x);
            let mut rows: Vec<Choice> =
                list.iter().map(|n| row(&format!("{LOAD}{n}"), n)).collect();
            // Over a preset's file once the knobs moved off it, or again when its file is
            // gone; never over Default, nor while the tag knows no preset (—).
            let save = off && name != super::window::DEFAULT && presets.origin(x).is_some();
            rows.push(row(if save { SAVE } else { "off:@save" }, "上書き保存"));
            rows.push(row(SAVE_AS, "名前を付けて保存…"));
            rows.push(row(SHOW, "ブラウザーに表示"));
            let now = presets
                .origin(x)
                .map(|n| format!("{LOAD}{n}"))
                .unwrap_or_default();
            (rows, now)
        }
        Menu::Row(t, id) => {
            let built_in = plugin_of(p, t, id).is_some_and(is_built_in);
            let mut rows = vec![
                row("open", "画面を開く"),
                // Only the built-in instrument's presets are known here.
                row(if built_in { "preset" } else { "off:preset" }, "プリセット"),
                row("replace", "差し替える…"),
                row("reload", "再起動"),
                row(GAP, ""),
                row("auto", "オートメーションを表示"),
                row(GAP, ""),
                // Nothing takes a copied plugin yet.
                row("off:copy", "コピー"),
                row(GAP, ""),
                row("remove", "プラグインを削除"),
            ];
            if !built_in {
                rows.insert(4, row("process", "動かす場所"));
            }
            (rows, String::new())
        }
        // The studio keeps the chain presets' list ([`Studio::chain_menu_view`]).
        Menu::Chain(_) | Menu::Destination(..) | Menu::ChannelDestination(_) => {
            (Vec::new(), String::new())
        }
        Menu::Params(t, id) => {
            let names = plugin_of(p, t, id).map(params).unwrap_or_default();
            if names.is_empty() {
                return (
                    vec![row("off:none", "パラメーターがありません")],
                    String::new(),
                );
            }
            (
                names.iter().map(|(id, n)| row(id, n)).collect(),
                String::new(),
            )
        }
    }
}

impl Studio {
    pub(crate) fn open_inst_menu(&mut self, menu: Menu, anchor: Rect) {
        self.instruments.menu_at = Some(anchor);
        if matches!(menu, Menu::Row(..)) {
            self.refresh_presets();
            let items = self.instrument_context_items(menu, anchor);
            self.open_classified("other-59", items, anchor.x, anchor.bottom());
            return;
        }
        match menu {
            Menu::Preset(..) => {
                self.refresh_presets();
            }
            Menu::Chain(t) => {
                if let Some(kind) = self
                    .project()
                    .tracks
                    .get(&t)
                    .map(|x| crate::channel::preset::kind_of(x))
                {
                    self.read_chain_presets(kind);
                }
            }
            _ => {}
        }
        self.open_popup(PopupKind::InstMenu(menu), anchor);
    }

    fn instrument_context_items(&mut self, menu: Menu, anchor: Rect) -> Vec<crate::context::Item> {
        use crate::context::Item;
        let (choices, current) = self.inst_menu_view(menu);
        choices
            .into_iter()
            .filter(|c| c.to != GAP)
            .map(|choice| {
                let enabled = !choice.to.starts_with(OFF);
                let to = choice.to.strip_prefix(OFF).unwrap_or(&choice.to);
                let children = match (menu, to) {
                    (Menu::Row(t, id), "preset") => {
                        Some(self.instrument_context_items(Menu::Preset(t, id), anchor))
                    }
                    (Menu::Row(t, id), "auto") => {
                        Some(self.instrument_context_items(Menu::Params(t, id), anchor))
                    }
                    (Menu::Row(t, id), "process") => Some(self.plugin_process_items(t, id)),
                    _ => None,
                };
                let mut item = if let Some(children) = children {
                    Item::sub(&choice.label, children)
                } else {
                    Item::legacy(
                        &choice.label,
                        PopupKind::InstMenu(menu),
                        anchor,
                        Action::Pick(choice.to.clone()),
                    )
                    .on(choice.to == current)
                }
                .enabled(enabled);
                item.key = look(menu, to).1.into();
                if !enabled {
                    item.reason = crate::disabled_reason::label(&choice.label).into();
                }
                item
            })
            .collect()
    }

    pub(crate) fn inst_menu_view(&self, menu: Menu) -> (Vec<Choice>, String) {
        if let Menu::ChannelDestination(t) = menu {
            let detached = self
                .channel
                .as_ref()
                .is_some_and(|w| w.track == t && w.detached);
            return (
                vec![
                    Choice {
                        to: "internal".into(),
                        label: "画面内".into(),
                    },
                    Choice {
                        to: "external".into(),
                        label: "独立ウィンドウ".into(),
                    },
                ],
                if detached { "external" } else { "internal" }.into(),
            );
        }
        if let Menu::Destination(_, id) = menu {
            return self.device_destinations(id);
        }
        if let Menu::Chain(t) = menu {
            return self.chain_menu_view(t);
        }
        view(&self.project(), &self.instruments.presets, menu)
    }

    /// A row of an instrument's menu was picked (the menu is shut already).
    pub(crate) fn inst_menu_pick(&mut self, menu: Menu, to: &str) {
        match menu {
            Menu::ChannelDestination(t) => {
                if let Some(w) = self.channel.as_mut().filter(|w| w.track == t)
                    && matches!(to, "internal" | "external")
                {
                    w.detached = to == "external";
                }
            }
            Menu::Destination(t, id) => {
                if to == "channel" {
                    self.open_channel(t);
                } else {
                    self.move_device_view(id, to);
                }
            }
            Menu::Preset(_, id) => match to {
                SAVE => {
                    let p = self.project();
                    if let Some(name) =
                        device_of(&p, id).and_then(|x| self.instruments.presets.origin(x))
                    {
                        self.save_preset(id, &name, true);
                    }
                }
                SAVE_AS => {
                    self.save_preset_as(id);
                }
                SHOW => {
                    let p = self.project();
                    let Some(x) = device_of(&p, id) else {
                        return;
                    };
                    let Some(def) = x.builtin() else {
                        return;
                    };
                    let origin = self.instruments.presets.origin(x);
                    self.show_preset_in_browser(def.uid, origin.as_deref());
                }
                _ => {
                    if let Some(name) = to.strip_prefix(LOAD) {
                        self.load_preset(id, name);
                    }
                }
            },
            Menu::Row(t, id) => self.row_pick(t, id, to),
            Menu::Params(t, id) => {
                self.show_param_lane(t, id, to);
            }
            Menu::Chain(t) => self.chain_menu_pick(t, to),
        }
    }

    /// A row of the instrument row's menu (S30).
    fn row_pick(&mut self, t: TrackId, id: PluginId, to: &str) {
        let anchor = self.instruments.menu_at.unwrap_or_default();
        match to {
            "open" => {
                // Opens it; a window that is open stays open.
                if !self.instruments.is_open(id) && !self.mixer.is_open(id) {
                    self.open_plugin(t, id);
                }
            }
            "process" => self.open_plugin_menu(t, id, anchor),
            "preset" => self.open_inst_menu(Menu::Preset(t, id), anchor),
            "replace" => self.open_popup(PopupKind::Instrument(t), anchor),
            "reload" => {
                self.restart_plugins();
            }
            "auto" => self.open_inst_menu(Menu::Params(t, id), anchor),
            "remove" => {
                self.remove_instrument(t);
            }
            _ => {}
        }
    }

    /// Shows the track's lanes with one for the instrument's parameter
    /// `name`: one step to undo.
    pub fn show_param_lane(&mut self, t: TrackId, id: PluginId, name: &str) -> bool {
        let target = AutoTarget::Param(id, name.to_string());
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let known = plugin_of(&p, t, id).is_some_and(|x| params(x).iter().any(|(n, _)| n == name));
        if !known {
            return false;
        }
        let shown = track
            .automation
            .iter()
            .any(|l| l.target == target && !l.hidden);
        if !shown
            && self
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                .arrange_run(
                    "auto.lane.add",
                    json!({ "track": t.to_string(), "target": target.to_string() }),
                )
                .is_none()
        {
            return false;
        }
        self.arrange.open.insert(t);
        self.arrange.touched();
        true
    }

    /// Shift+F11: the window of the selected track's instrument (S30).
    pub fn instrument_key(&mut self, key: &str, mods: Mods) -> bool {
        if key != "f11" || !mods.shift || mods.ctrl || mods.alt {
            return false;
        }
        let Some(RowKind::Track(t)) = self.selected_row else {
            return false;
        };
        let Some(id) = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| current(x))
            .map(|x| x.id)
        else {
            return false;
        };
        self.open_plugin(t, id)
    }
}
