//! Giving a macro knob its targets (MIX-040). The final mock has no list of a knob's targets,
//! so the knob's own right-click menu holds the three ways in: 「割り当てを追加…」 opens the
//! list of what the track can give (a [`PickList`] that searches), 学習 takes the parameter a
//! hand moved last (a device's knob, or the plug-in's own window), and 「割り当てを外す ›」
//! takes one back. The parts' menus give themselves with 「マクロコントロールに割り当て ›」.

use serde_json::json;
use xirius_core::auto::AutoTarget;
use xirius_core::controls::{ControlId, can_map};
use xirius_core::id::TrackId;
use xirius_prefs::midimap::Target;

use crate::automation_targets::lane_label;
use crate::context::{Act, Item};
use crate::pick_list::{PickList, PickRow};
use crate::popup::{Action, PopupKind};
use crate::studio::{Geometry, Studio};

/// The menu's rows that give and take back.
pub const ADD: &str = "割り当てを追加…";
pub const LEARN: &str = "学習";
pub const REMOVE: &str = "割り当てを外す";

impl Studio {
    /// The menu of macro knob `k` of track `t`, at the pointer.
    pub(crate) fn macro_knob_menu(&mut self, t: TrackId, k: u8, x: f32, y: f32) {
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let id = ControlId::Knob(k);
        let args = || json!({ "track": t.to_string(), "control": id.to_string() });
        // the parameter a hand moved last, when it is this track's and free for this knob
        let touched = self
            .song()
            .session()
            .touched()
            .and_then(|s| s.parse::<AutoTarget>().ok())
            .filter(|x| can_map(x) && x.current(track).is_some())
            .filter(|x| track.controls.owner_of(x).is_none_or(|o| o == id));
        let learn = match &touched {
            Some(x) => self.ctx_as(
                &format!("{LEARN}: {}", lane_label(&p, track, x)),
                "control.assign.touched",
                args(),
            ),
            None => self
                .ctx_as(LEARN, "control.assign.touched", args())
                .enabled(false),
        };
        let maps: Vec<Item> = track
            .controls
            .get(id)
            .maps
            .iter()
            .map(|m| {
                let mut a = args();
                a["target"] = json!(m.target.to_string());
                self.ctx_as(&lane_label(&p, track, &m.target), "control.unassign", a)
            })
            .collect();
        let any = !maps.is_empty();
        let rename = Item::ask(
            "名前を変更",
            "control.rename",
            args(),
            "name",
            &track.control_label(id),
        );
        let mut items = vec![
            Item::act(ADD, "", Act::PickMacro(t, k)),
            learn,
            Item::sub(REMOVE, maps).enabled(any),
            Item::gap(),
            rename,
            Item::gap(),
            self.lane_item(t, AutoTarget::Control(id)),
            Item::gap(),
        ];
        items.extend(self.midi_rows(Target::Control(t.to_string(), id.to_string())));
        self.open_classified("other-67", items, x, y);
    }

    /// Opens the list of targets under knob `k` wherever it shows: the macro window, else the
    /// channel editor's row.
    pub(crate) fn pick_macro(&mut self, g: &Geometry, t: TrackId, k: u8) -> bool {
        let i = usize::from(k);
        let anchor = self
            .macro_view(g)
            .filter(|v| v.track == t)
            .and_then(|v| v.knobs.get(i).copied())
            .or_else(|| {
                let v = self.channel_view(g)?;
                (self.channel.as_ref()?.track == t).then_some(())?;
                v.macros.and_then(|r| r.cells.get(i).copied())
            });
        let Some(anchor) = anchor else {
            return false;
        };
        self.open_popup(PopupKind::MacroTargets(t, k), anchor);
        true
    }

    /// The list of what track `t` can give knob `k`: its own on, another knob's waiting with
    /// that knob's name.
    pub(crate) fn macro_targets(&self, t: TrackId, k: u8) -> Option<PickList> {
        let p = self.project();
        let track = p.tracks.get(&t)?;
        let id = ControlId::Knob(k);
        let rows = self
            .auto_target_choices(t, None)
            .into_iter()
            .filter_map(|c| {
                let to = c.to.strip_prefix("off:").unwrap_or(&c.to).to_string();
                let target = to.parse::<AutoTarget>().ok()?;
                if !can_map(&target) || target.current(track).is_none() {
                    return None;
                }
                let row = PickRow::new(c.label);
                Some(match track.controls.owner_of(&target) {
                    Some(o) if o == id => row.on(true).act(Action::MacroTarget(t, k, to)),
                    Some(o) => row.disabled().hint(track.control_label(o)),
                    None => row.act(Action::MacroTarget(t, k, to)),
                })
            })
            .collect();
        Some(PickList::of(rows).named("一致する割り当て先はありません"))
    }

    /// A row of the list: the knob takes the target, or lets its own go.
    pub(crate) fn macro_target(&mut self, t: TrackId, k: u8, to: &str) -> bool {
        self.popup = None;
        let id = ControlId::Knob(k);
        let own = to.parse::<AutoTarget>().ok().is_some_and(|x| {
            self.project()
                .tracks
                .get(&t)
                .is_some_and(|tr| tr.controls.owner_of(&x) == Some(id))
        });
        let name = if own {
            "control.unassign"
        } else {
            "control.assign"
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(
            name,
            json!({ "track": t.to_string(), "control": id.to_string(), "target": to }),
        )
    }

    /// A right press over the macro window: a knob's menu. True over the window.
    pub(crate) fn macro_right(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        let Some(v) = self.macro_view(g) else {
            return false;
        };
        match super::part_at(&v, x, y) {
            None => false,
            Some(super::Part::Knob(k)) => {
                self.macro_knob_menu(v.track, k, x, y);
                true
            }
            Some(_) => true,
        }
    }
}
