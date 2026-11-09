//! KEY-012: the next and the previous channel (Studio Pro's Console › Next Channel), and the next
//! and the previous preset of the plugin window on top (Presets › Next). They take no arguments:
//! they step from what is selected and shown.

use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::Plugin;

use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tree::RowKind;

impl Studio {
    /// Selects the channel after (`forward`) or before the selected one, in the mixer's order
    /// without the master; with none selected, the first or the last. It stops at the ends. The
    /// mixer, when it is shown, scrolls to the strip.
    pub(super) fn step_channel(&mut self, g: &Geometry, forward: bool) -> bool {
        let order: Vec<TrackId> = crate::mixer::strip_order(&self.project())
            .into_iter()
            .filter(|t| !t.is_master())
            .collect();
        let now = match self.selected_row {
            Some(RowKind::Track(t)) => order.iter().position(|x| *x == t),
            _ => None,
        };
        let next = match (now, forward) {
            (None, true) => Some(0),
            (None, false) => order.len().checked_sub(1),
            (Some(i), true) => Some(i + 1),
            (Some(i), false) => i.checked_sub(1),
        };
        let Some(&t) = next.and_then(|i| order.get(i)) else {
            return false;
        };
        if self.drawer_open && self.tab == DrawerTab::Mixer {
            self.show_strip(g, t);
        }
        // A channel in a folded folder has no row to scroll to; it is selected all the same.
        if !self.reveal_row(g, t) {
            self.pick_row(RowKind::Track(t), crate::tools::Mods::NONE);
        }
        true
    }

    /// Loads the preset after (`forward`) or before the one the tag of the plugin window on top
    /// names (its knobs moved off it or not), round the list; from a device whose preset is not
    /// known, the first or the last (UI-332). Only a built-in plugin's window has them.
    /// The tag's ‹ › of the window just pressed call it too (UI-322): the press brings the window
    /// to the top first.
    pub(crate) fn step_preset(&mut self, forward: bool) -> bool {
        let p = self.project();
        let Some(id) = self.instruments.open.iter().rev().find_map(|w| {
            let plugin = p
                .tracks
                .values()
                .flat_map(|t| &t.inserts)
                .find(|x| x.id == w.plugin)?;
            Plugin::builtin(plugin)?;
            Some(w.plugin)
        }) else {
            return false;
        };
        self.step_device_preset(id, forward)
    }

    /// Resolve a frame's direction against the current owner state, not its asynchronous label.
    pub(crate) fn step_device_preset(&mut self, id: PluginId, forward: bool) -> bool {
        self.refresh_presets();
        let p = self.project();
        let Some(plugin) = p
            .tracks
            .values()
            .flat_map(|t| &t.inserts)
            .find(|p| p.id == id)
        else {
            return false;
        };
        let presets = &self.instruments.presets;
        let list = plugin.builtin().map_or_else(
            || {
                presets
                    .external
                    .get(&plugin.uid.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_default()
            },
            |def| presets.names(def),
        );
        let now = presets.origin(plugin);
        let n = list.len();
        if n == 0 {
            return false;
        }
        let i = match (list.iter().position(|x| Some(x) == now.as_ref()), forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        };
        self.load_preset(id, &list[i])
    }
}
