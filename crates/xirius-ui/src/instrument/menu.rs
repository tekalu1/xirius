//! The menus of an instrument: the preset list of its window (UI-098) and
//! the right press on its row at the head of the inserts (CTX-040), and the
//! right press on an instrument in the browser (CTX-048).
//!
//! A menu is a list of [`Choice`]s: `to` says what the row does, and a row
//! that cannot run now has `to` starting with [`OFF`]; [`GAP`] puts space
//! between two groups.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, Project};

use super::current;
use super::window::{PRESETS, is_built_in, preset_of};
use crate::popup::{Action, Choice, PopupKind};
use crate::studio::{Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

/// What a row that cannot run now starts with.
pub const OFF: &str = "off:";
/// The space between two groups.
pub const GAP: &str = "-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// The presets of a built-in instrument (the box at the foot of its window).
    Preset(TrackId, PluginId),
    /// The right press on an instrument track's instrument row (S30).
    Row(TrackId, PluginId),
    /// The parameters to show a lane of (the row's オートメーションを表示).
    Params(TrackId, PluginId),
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
    for ch in choices {
        if ch.to == GAP {
            groups.push(PickGroup::new("", Vec::new()));
            continue;
        }
        let (_, key) = look(menu, ch.to.trim_start_matches(OFF));
        let row = PickRow::new(ch.label).hint(key).on(ch.to == current);
        let row = match ch.to.strip_prefix(OFF) {
            Some(_) => row.disabled(),
            None => row.act(Action::Pick(ch.to.clone())),
        };
        groups.last_mut().unwrap().rows.push(row);
    }
    groups.retain(|g| !g.rows.is_empty());
    let mut list = PickList::of(Vec::new());
    list.groups = groups;
    list
}

/// The rows of a menu as it is now, and the row that is on.
pub fn view(p: &Project, menu: Menu) -> (Vec<Choice>, String) {
    let row = |to: &str, label: &str| Choice {
        to: to.into(),
        label: label.into(),
    };
    match menu {
        Menu::Preset(t, id) => {
            let now = plugin_of(p, t, id).map_or(PRESETS[0], preset_of);
            (PRESETS.iter().map(|n| row(n, n)).collect(), now.into())
        }
        Menu::Row(t, id) => {
            let built_in = plugin_of(p, t, id).is_some_and(is_built_in);
            let rows = vec![
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
            (rows, String::new())
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
        self.open_popup(PopupKind::InstMenu(menu), anchor);
    }

    pub(crate) fn inst_menu_view(&self, menu: Menu) -> (Vec<Choice>, String) {
        view(&self.project(), menu)
    }

    /// A row of an instrument's menu was picked (the menu is shut already).
    pub(crate) fn inst_menu_pick(&mut self, menu: Menu, to: &str) {
        match menu {
            Menu::Preset(_, id) => {
                self.load_preset(id, to);
            }
            Menu::Row(t, id) => self.row_pick(t, id, to),
            Menu::Params(t, id) => {
                self.show_param_lane(t, id, to);
            }
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
