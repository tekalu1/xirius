//! The row picked on a strip's flow (MIX-045): its M, the bar of its level row, the ＋ after it
//! and its keys. ↑↓ pick the row above or below, Enter opens it (a splitter opens or shuts),
//! Space turns it off or on, M mutes the branch or layer it heads, Alt+←→ moves that level by
//! 1 dB, Alt+↑↓ moves the row within its chain, Insert or ＋ puts a plug-in after it, Delete
//! takes it away and Esc lets it go.

use serde_json::{Value, json};
use xirius_core::id::{NodeId, TrackId};
use xirius_core::model::PluginKind;
use xirius_core::rack::Source;

use super::flow::{self, Kind, Pick, Target};
use super::meters::{CEIL_DB, FLOOR_DB};
use super::{LevelDrag, MixerHit};
use crate::popup::PopupKind;
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;

/// One step of Alt+←→ on a level, in dB.
const LEVEL_STEP: f64 = 1.0;

impl Studio {
    /// The level the M and the level row of branch or layer `n` of `t` move.
    pub(crate) fn row_target(&self, t: TrackId, n: NodeId) -> Option<Target> {
        let p = self.project();
        let rack = p.tracks.get(&t)?.rack.as_ref()?;
        let (db, pan, mute) = rack.levels(n)?;
        let layer = matches!(&rack.source, Some(Source::Multi { layers, .. })
            if layers.iter().any(|l| l.id == n));
        Some(Target {
            id: n,
            layer,
            db,
            pan,
            mute,
        })
    }

    /// Sets a branch's or a layer's levels: one step of the history.
    pub(crate) fn set_target(&mut self, target: Target, mut args: Value) -> bool {
        args["id"] = json!(target.id.to_string());
        let command = if target.layer {
            "rack.layer.set"
        } else {
            "rack.branch.set"
        };
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.run_command(command, args)
    }

    /// The M of a row: the branch it heads or its layer mutes, or is heard again.
    pub(crate) fn row_mute(&mut self, t: TrackId, n: NodeId) -> bool {
        let Some(target) = self.row_target(t, n) else {
            return false;
        };
        let mute = if target.mute { "off" } else { "on" };
        self.set_target(target, json!({ "mute": mute }))
    }

    /// A press takes the keys from what had them, unless it lands on it: the picked row, a
    /// key send's bar, the pad, a device's control (UI-324).
    pub fn press_keys(&mut self, g: &Geometry, x: f32, y: f32) {
        // a press off the name being typed keeps the name, wherever it lands
        if self.input_map.name.is_some() && !self.name_rect(g).is_some_and(|r| r.contains(x, y)) {
            self.name_key("enter", Mods::NONE);
        }
        self.chosen_press(g, x, y);
        if self.popup.is_some() {
            return;
        }
        self.value_blur(g, x, y);
        if !matches!(self.hit(g, x, y), Hit::Mixer(MixerHit::KeyLevel(..))) {
            self.mixer.key_focus = None;
        }
        if !self.over_instrument(g, x, y) {
            self.device_blur();
        }
        self.head_fader_press_off(g, x, y);
    }

    /// The keys move to another part (F6, the palette, a menu of the arrangement): the picked
    /// row, the key send's bar and a branch's value let them go.
    pub(crate) fn mixer_let_go_keys(&mut self) {
        self.value_let_go();
        self.mixer.chosen = None;
        self.mixer.key_focus = None;
        self.mixer.heading = None;
    }

    /// Is one of the windows that float over the screen under the point?
    pub(crate) fn over_float(&self, g: &Geometry, x: f32, y: f32) -> bool {
        self.over_instrument(g, x, y)
    }

    /// A press let go: the rows take the height of what pressed (a touch, 32), so the press
    /// itself was hit where the rows were drawn.
    pub fn coarse_up(&mut self) {
        if let Some(c) = self.mixer.coarse_next.take() {
            self.mixer.coarse = c;
        }
    }

    /// A press anywhere but on a strip's flow lets the picked row go.
    pub fn chosen_press(&mut self, g: &Geometry, x: f32, y: f32) {
        if self.mixer.chosen.is_none() || self.popup.is_some() {
            return;
        }
        let keep = matches!(
            self.hit(g, x, y),
            Hit::Mixer(
                MixerHit::Open(..)
                    | MixerHit::Bypass(..)
                    | MixerHit::Fold(..)
                    | MixerHit::RowMute(..)
                    | MixerHit::Level(..)
                    | MixerHit::BranchPan(..)
                    | MixerHit::AddBranch(..)
                    | MixerHit::After(..)
                    | MixerHit::Editor(..)
                    | MixerHit::Widen(..)
            )
        );
        if !keep {
            self.mixer.chosen = None;
        }
    }

    /// The ＋ after a row: the list of plug-ins, whose pick lands right after it. After an
    /// instrument it is another layer.
    pub(crate) fn insert_after(&mut self, t: TrackId, pick: Pick, anchor: Rect) {
        let instrument = matches!(pick, Pick::Plugin(id) if self.project().tracks.get(&t)
            .is_some_and(|x| x.inserts.iter().any(|p| p.id == id && p.kind == PluginKind::Instrument)));
        if instrument {
            self.mixer.insert_after = None;
            self.mixer.insert_into = None;
            self.open_popup(PopupKind::Layer(t), anchor);
            return;
        }
        if let Pick::Branch(id) = pick {
            // an empty branch: the plug-in goes in it
            self.mixer.insert_after = None;
            self.mixer.insert_into = Some((t, crate::channel::flow::Chain::Branch(id), 0));
            self.open_popup(PopupKind::Insert(t), anchor);
            if self.popup.is_none() {
                self.mixer.insert_into = None;
            }
            return;
        }
        self.mixer.insert_into = None;
        self.mixer.insert_after = Some((t, pick));
        self.open_popup(PopupKind::Insert(t), anchor);
        if self.popup.is_none() {
            self.mixer.insert_after = None;
        }
    }

    /// A press on the bar of the level row: the level stays, and moves by the way the pointer
    /// goes (MIX-045, approved 2026-11-06).
    pub fn level_down(&mut self, g: &Geometry, hit: Hit, x: f32) -> bool {
        if self.key_down(g, hit, x) {
            return true;
        }
        let Hit::Mixer(h @ MixerHit::Level(t, n)) = hit else {
            return false;
        };
        let (Some(bar), Some(target)) = (self.mixer_rect(g, h), self.row_target(t, n)) else {
            return false;
        };
        self.mixer.level_drag = Some(LevelDrag::taken(t, target, bar, x));
        self.level_move(x, false);
        true
    }

    /// The pointer moved with the bar held; `fine` (Shift) moves a quarter as far.
    pub fn level_move(&mut self, x: f32, fine: bool) -> bool {
        if self.mixer.key_drag.is_some() {
            return self.key_move(x, fine);
        }
        let Some(mut d) = self.mixer.level_drag else {
            return false;
        };
        let at = d.at(x, fine);
        self.mixer.level_drag = Some(d);
        let Some(v) = at.filter(|v| *v != d.value) else {
            return false;
        };
        d.value = v;
        self.mixer.level_drag = Some(d);
        if self.transport.is_live() {
            let mut p = (*self.project()).clone();
            if let Ok(track) = p.track_mut(d.track)
                && let Some(l) = track.rack.as_mut().and_then(|r| r.levels_mut(d.node))
            {
                if d.pan {
                    *l.pan = v;
                } else {
                    *l.gain_db = v;
                }
            }
            let heard = self.auto_heard(&p);
            self.transport.sync(&heard, self.song().dir());
        }
        true
    }

    /// The bar let go: one step when the level moved.
    pub fn level_up(&mut self) -> bool {
        if self.mixer.key_drag.is_some() {
            return self.key_up();
        }
        let Some(d) = self.mixer.level_drag.take() else {
            return false;
        };
        if d.value == d.start {
            let p = self.project();
            self.transport.sync(&p, self.song().dir());
            return true;
        }
        let target = Target {
            id: d.node,
            layer: d.layer,
            db: d.start,
            pan: d.start,
            mute: false,
        };
        let key = if d.pan { "pan" } else { "gain" };
        self.set_target(target, json!({ key: d.value }));
        true
    }

    /// The keys of the picked row (see the module).
    pub(crate) fn flow_key(&mut self, g: &Geometry, key: &str, ch: Option<&str>, m: Mods) -> bool {
        let Some((t, pick)) = self.mixer.chosen else {
            return false;
        };
        // the keys are the mixer's only while nothing else holds them
        if !self.mixer_shown()
            || self.popup.is_some()
            || m.ctrl
            || self.input_map.browser_focus
            || self.instruments.focus.is_some()
        {
            return false;
        }
        // Esc while a part waits for a knob ends the wait first (the keymap's)
        let assign = &self.live_midi.assign;
        if key == "escape" && (assign.learning.is_some() || assign.clash.is_some()) {
            return false;
        }
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return false;
        };
        let wide = self.width_of(g, t) >= super::WIDE_W;
        let lines = self.mixer.look().at(wide).lines(track);
        let picks: Vec<Pick> = lines
            .iter()
            .filter(|l| l.kind != Kind::Level)
            .filter_map(flow::Line::pick)
            .collect();
        let Some(i) = picks.iter().position(|x| *x == pick) else {
            return false;
        };
        let line = lines.iter().find(|l| l.pick() == Some(pick)).copied();
        let target = line.and_then(|l| l.target);
        let frozen = track.frozen.is_some();
        let instrument = matches!(line.map(|l| l.kind), Some(Kind::Instrument { .. }));
        match key {
            "escape" => self.mixer.chosen = None,
            "up" | "down" if m.alt => {
                if frozen || instrument {
                    return true;
                }
                let Some((at, k, n)) = flow::spot(track, pick) else {
                    return true;
                };
                let to = if key == "up" {
                    k.checked_sub(1)
                } else {
                    Some(k + 1).filter(|x| *x < n)
                };
                let Some(to) = to else {
                    return true;
                };
                let id = match pick {
                    Pick::Plugin(id) => id.to_string(),
                    Pick::Split(id) | Pick::Branch(id) => id.to_string(),
                };
                if track.rack.is_none() {
                    // the instrument stays first; the effects are after it
                    let gap = track.inserts.len() - n;
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command(
                        "plugin.move",
                        json!({ "id": id, "track": t.to_string(), "index": to + gap }),
                    );
                } else {
                    let into = at.map_or(t.to_string(), |n| n.to_string());
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.run_command(
                        "rack.node.move",
                        json!({ "id": id, "to": into, "index": to }),
                    );
                }
                self.reveal_pick(g, t, pick);
            }
            "up" | "down" => {
                let next = if key == "up" {
                    i.checked_sub(1)
                } else {
                    Some(i + 1).filter(|x| *x < picks.len())
                };
                if let Some(next) = next {
                    self.mixer.chosen = Some((t, picks[next]));
                    self.follow_pick();
                    self.reveal_pick(g, t, picks[next]);
                }
            }
            "left" | "right" if m.alt => {
                if let Some(target) = target.filter(|_| !frozen) {
                    let by = if key == "left" {
                        -LEVEL_STEP
                    } else {
                        LEVEL_STEP
                    };
                    let db = (target.db.max(f64::from(FLOOR_DB)) + by)
                        .clamp(f64::from(FLOOR_DB), f64::from(CEIL_DB));
                    if db != target.db {
                        self.set_target(target, json!({ "gain": db }));
                    }
                }
            }
            "enter" => match pick {
                Pick::Plugin(id) if self.health.crashed(id) => {
                    if let Some(r) = self.mixer_rect(g, MixerHit::Open(t, id)) {
                        self.open_popup(PopupKind::CrashedPlugin(t, id), r);
                    }
                }
                Pick::Plugin(id) => {
                    self.open_plugin(t, id);
                }
                Pick::Split(id) => self.fold_split(g, t, id),
                // an empty branch: the list of plug-ins, to put one in it
                Pick::Branch(_) => self.after_key(g, t, pick, frozen),
            },
            "space" => {
                let Pick::Plugin(id) = pick else {
                    return true;
                };
                if !frozen {
                    self.mixer_press(g, MixerHit::Bypass(t, id));
                    self.mixer.chosen = Some((t, pick));
                }
            }
            "m" => {
                if let Some(target) = target.filter(|_| !frozen) {
                    self.row_mute(t, target.id);
                }
            }
            "insert" if m.alt => {
                if !frozen && !instrument {
                    self.before_key(g, t, pick);
                }
            }
            "insert" => self.after_key(g, t, pick, frozen),
            _ if ch == Some("+") => self.after_key(g, t, pick, frozen),
            "delete" => match pick {
                Pick::Plugin(id) if !frozen => {
                    // the next row is picked in its place
                    let next = picks
                        .get(i + 1)
                        .or(i.checked_sub(1).and_then(|k| picks.get(k)));
                    self.close_editor(id);
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    if self.run_command("plugin.remove", json!({ "ids": [id.to_string()] })) {
                        self.mixer.chosen = next.map(|x| (t, *x));
                        self.follow_pick();
                    }
                }
                Pick::Plugin(_) => {}
                // an empty branch goes with its ✖ (one undo with its notice)
                Pick::Branch(_) => {}
                // a splitter goes with rack.flatten or its branches' own removal
                Pick::Split(_) => {
                    self.error = Some("スプリッターはキーでは削除できません".into());
                }
            },
            _ => return false,
        }
        true
    }

    fn after_key(&mut self, g: &Geometry, t: TrackId, pick: Pick, frozen: bool) {
        if frozen {
            return;
        }
        let anchor = self
            .mixer_rect(g, MixerHit::After(t, pick))
            .or_else(|| self.mixer_rect(g, MixerHit::Strip(t)))
            .unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
        self.insert_after(t, pick, anchor);
    }

    /// Alt+Insert: the list of plug-ins, whose pick goes right before the picked row, in its
    /// chain. The row heading a branch or a layer's effects adds at its head (MIX-042).
    fn before_key(&mut self, g: &Geometry, t: TrackId, pick: Pick) {
        use crate::channel::flow::Chain;
        if let Pick::Branch(_) = pick {
            return self.after_key(g, t, pick, false);
        }
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return;
        };
        let Some((at, k, _)) = flow::spot(track, pick) else {
            return;
        };
        let layer = |n| {
            crate::channel::keys::layers(track)
                .iter()
                .any(|l| l.id == n)
        };
        let chain = match at {
            None => Chain::Top,
            Some(n) if layer(n) => Chain::Layer(n),
            Some(n) => Chain::Branch(n),
        };
        let anchor = self
            .mixer_rect(g, MixerHit::After(t, pick))
            .or_else(|| self.mixer_rect(g, MixerHit::Strip(t)))
            .unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
        self.mixer.insert_after = None;
        self.mixer.insert_into = Some((t, chain, k));
        self.open_popup(PopupKind::Insert(t), anchor);
        if self.popup.is_none() {
            self.mixer.insert_into = None;
        }
    }
}

#[cfg(test)]
#[path = "choose_tests.rs"]
mod tests;

impl Studio {
    /// The row picked on a strip opens its window, when it is a built-in device: it has a
    /// window of its own (UI-098).
    pub(crate) fn follow_pick(&mut self) {
        if !self.mixer_shown() {
            return;
        }
        let Some((t, Pick::Plugin(id))) = self.mixer.chosen else {
            return;
        };
        let p = self.project();
        let Some(plugin) = p
            .tracks
            .get(&t)
            .and_then(|x| x.inserts.iter().find(|x| x.id == id))
        else {
            return;
        };
        if !crate::instrument::window::is_built_in(plugin) || self.health.crashed(id) {
            return;
        }
        if !self.instruments.is_open(id) {
            self.show_instrument(t, id);
        }
    }
}
