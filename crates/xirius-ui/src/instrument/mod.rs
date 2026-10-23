//! The instrument of an instrument track (PLG-019): its row at the top of the
//! mixer's insert list, the list it is picked from, and swapping or taking
//! it out. A track whose instrument was taken out keeps the row: it becomes
//! the ＋ row that picks one again.

use serde_json::json;
use xirius_core::id::{PluginId, TrackId};
use xirius_core::model::{Plugin, PluginKind, Project, Track, TrackKind};
use xirius_design::Radius;
use xirius_plugins::Catalog;

use crate::browser::{LIB, OWN};
use crate::draw::Pen;
use crate::popup::Choice;
use crate::studio::{Rect, Studio};

pub mod keys;
pub mod menu;
pub mod shots;
pub mod window;

/// What a built-in instrument's choice starts with; the rest is its name.
const BUILT_IN: &str = "builtin:";

/// The instrument a new instrument track gets (UI-074, UI-076). The mock's
/// Grand Piano corresponds to its default piano multisamples (`sound = 0`).
pub const DEFAULT: &str = "Sampler";

/// What the ＋ row of a track without an instrument is called (its balloon and its accessible
/// name; the row itself shows only an icon).
pub const PICK_LABEL: &str = "インストゥルメントを選ぶ";

/// The side of the icon the ＋ row shows.
const ICON: f32 = 14.0;

/// The instrument of a track: the first insert, when it is one.
pub fn current(track: &Track) -> Option<&Plugin> {
    track
        .inserts
        .first()
        .filter(|x| x.kind == PluginKind::Instrument)
}

/// Is `id` the instrument of track `t`?
pub fn is_instrument_of(p: &Project, t: TrackId, id: PluginId) -> bool {
    p.tracks
        .get(&t)
        .and_then(|t| current(t))
        .is_some_and(|x| x.id == id)
}

/// The rows the insert list shows before the first insert: the ＋ row of an
/// instrument track that has no instrument.
pub fn gap(track: &Track) -> usize {
    usize::from(track.kind == TrackKind::Instrument && current(track).is_none())
}

/// A plugin to put in, as `plugin.replace` takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub name: String,
    pub format: String,
    pub uid: String,
}

impl Pick {
    pub fn built_in(name: &str) -> Pick {
        Pick {
            name: name.to_string(),
            format: "builtin".into(),
            uid: name.to_string(),
        }
    }
}

/// The built-in instruments, then what the scan found.
pub fn choices(catalog: &Catalog) -> Vec<Choice> {
    LIB.iter()
        .filter(|x| !x.effect && x.vendor == OWN)
        .map(|x| Choice {
            to: format!("{BUILT_IN}{}", x.name),
            label: x.name.to_string(),
        })
        .chain(
            catalog
                .search("", Some(PluginKind::Instrument))
                .into_iter()
                .map(|e| Choice {
                    to: e.uid.clone(),
                    label: if e.vendor.is_empty() {
                        e.name.clone()
                    } else {
                        format!("{} — {}", e.name, e.vendor)
                    },
                }),
        )
        .collect()
}

/// The plugin a choice of [`choices`] names, or an instrument of the browser by name.
pub fn resolve(catalog: &Catalog, to: &str) -> Option<Pick> {
    if let Some(name) = to.strip_prefix(BUILT_IN) {
        return Some(Pick::built_in(name));
    }
    if let Some(e) = catalog.find(to) {
        return Some(Pick {
            name: e.name.clone(),
            format: e.format.clone(),
            uid: e.uid.clone(),
        });
    }
    // A name from the browser's list.
    if LIB
        .iter()
        .any(|x| !x.effect && x.vendor == OWN && x.name == to)
    {
        return Some(Pick::built_in(to));
    }
    catalog
        .search("", Some(PluginKind::Instrument))
        .into_iter()
        .find(|e| e.name == to)
        .map(|e| Pick {
            name: e.name.clone(),
            format: e.format.clone(),
            uid: e.uid.clone(),
        })
}

/// The ＋ row of a track without an instrument (UI-305): the instrument's icon alone, the same
/// as the rail's tab, in the middle. The strip is too narrow for [`PICK_LABEL`], which the
/// balloon and the screen reader say instead.
pub fn paint_empty(pen: &mut Pen, row: Rect, hot: bool) {
    let c = pen.c;
    pen.round(row, if hot { c.surface_3 } else { c.surface_1 }, Radius::S);
    let (name, icon) = empty_icon(row);
    pen.icon(name, icon, c.ink);
}

/// The icon of the ＋ row (the rail's tab for the instruments) and where it sits in `row`.
pub fn empty_icon(row: Rect) -> (&'static str, Rect) {
    let icon = Rect::new(
        row.x + (row.w - ICON) / 2.0,
        row.y + (row.h - ICON) / 2.0,
        ICON,
        ICON,
    );
    (crate::browser::RailTab::Instruments.icon(), icon)
}

impl Studio {
    /// The list a track's instrument is picked from (UI-320):
    /// インストゥルメント, プラグイン, with the scan's rows under it.
    pub(crate) fn instrument_pick(&mut self) -> crate::pick_list::PickList {
        let all = choices(&self.plugin_catalog());
        let (builtin, plugins): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|c| c.to.starts_with(BUILT_IN));
        let mut list = crate::pick_list::PickList::grouped(
            vec![("インストゥルメント", builtin), ("プラグイン", plugins)],
            "keys",
            |to| crate::popup::Action::Pick(to.to_string()),
        )
        .named("一致するインストゥルメントはありません");
        list.footer = self.scan_footer();
        list
    }

    /// A choice picked in the instrument list of track `t`.
    pub(crate) fn pick_instrument(&mut self, t: TrackId, to: &str) -> bool {
        let catalog = self.plugin_catalog();
        match resolve(&catalog, to) {
            Some(pick) => self.set_instrument(t, &pick),
            None => {
                self.error = Some("そのインストゥルメントはすでに削除されています".into());
                false
            }
        }
    }

    /// Puts `pick` in as the instrument of `t`, in place of the one it has (one step).
    pub fn set_instrument(&mut self, t: TrackId, pick: &Pick) -> bool {
        self.run_command(
            "plugin.replace",
            json!({
                "slot": "instrument",
                "track": t.to_string(),
                "name": pick.name,
                "format": pick.format,
                "uid": pick.uid,
            }),
        )
    }

    /// Takes the instrument of `t` out; its row stays as the ＋ row.
    pub fn remove_instrument(&mut self, t: TrackId) -> bool {
        let Some(id) = self
            .project()
            .tracks
            .get(&t)
            .and_then(|x| current(x))
            .map(|x| x.id)
        else {
            return false;
        };
        self.close_editor(id);
        self.run_command("plugin.remove", json!({ "ids": [id.to_string()] }))
    }
}

#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod window_tests;
