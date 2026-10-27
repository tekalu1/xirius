//! Selecting by rule and the clipboard, from the keys (CTX-028, KEY-003, OPS-015).
//!
//! Every key here runs a command of core's table (`select.*`, `edit.*`), so the CLI and MCP do
//! the same work with the same words. What the window keeps of its own is only what it shows:
//! the arrangement's selected clips ([`crate::arrange::Arrange::clips`]), the roll's selected
//! notes and points, and the selected points of one automation lane ([`PointSel`]). After a
//! command that changes the session's selection, the window takes that selection over.
//!
//! The arrangement has the focus (or there is no roll open): Ctrl+C / X / V work on the selected
//! clips, or on the selected points of an automation lane when those are what is selected.
//! Ctrl+V puts the first of them at the playhead; Ctrl+Shift+V puts them back where they were.
//! In the roll, the notes keep going through the roll's own keys (they also fill the session's
//! clipboard), and the points of a controller lane go through [`crate::roll`]'s `pointclip`.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use xirius_core::auto::AutoTarget;
use xirius_core::clipboard::Clipboard;
use xirius_core::id::{AnyId, ClipId, NoteId, TrackId};
use xirius_core::time::Tick;

use crate::arrange::{Preview, Spot};
use crate::studio::{Geometry, Studio};
use crate::tools::{Mods, Tool};
use crate::tree::RowKind;

/// The selected points of one automation lane.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointSel {
    lane: Option<(TrackId, AutoTarget)>,
    ticks: BTreeSet<Tick>,
}

impl PointSel {
    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    pub fn clear(&mut self) {
        self.lane = None;
        self.ticks.clear();
    }

    /// Whether the point at `tick` of this lane is selected.
    pub fn has(&self, track: TrackId, target: &AutoTarget, tick: Tick) -> bool {
        self.lane
            .as_ref()
            .is_some_and(|(t, g)| *t == track && g == target)
            && self.ticks.contains(&tick)
    }

    /// Selects the point at `tick`: alone, or with the others of the lane when `extend`.
    pub fn select(&mut self, track: TrackId, target: &AutoTarget, tick: Tick, extend: bool) {
        let same = self
            .lane
            .as_ref()
            .is_some_and(|(t, g)| *t == track && g == target);
        if !(extend && same) {
            self.ticks.clear();
        }
        self.lane = Some((track, target.clone()));
        if extend && same && self.ticks.contains(&tick) {
            self.ticks.remove(&tick);
        } else {
            self.ticks.insert(tick);
        }
    }

    pub fn lane(&self) -> Option<&(TrackId, AutoTarget)> {
        self.lane.as_ref()
    }

    pub fn ticks(&self) -> &BTreeSet<Tick> {
        &self.ticks
    }

    pub(crate) fn set_all(&mut self, track: TrackId, target: &AutoTarget, ticks: BTreeSet<Tick>) {
        self.lane = Some((track, target.clone()));
        self.ticks = ticks;
    }
}

impl Studio {
    /// A key of selecting or the clipboard. `Some(used)` when it was one of them; None when it
    /// is not, or is left to the roll's own keys.
    pub fn clip_key(&mut self, g: &Geometry, key: &str, mods: Mods) -> Option<bool> {
        let chord = xirius_prefs::Chord::from_event(key, crate::input_dispatch::bits(mods))?;
        let ctx = if !self.arrange.focus && self.roll_input(g).is_some() {
            "roll"
        } else {
            "arrange"
        };
        let (_, binding) = xirius_prefs::Keymap::default().lookup(ctx, &chord)?;
        let (command, args) = xirius_prefs::cmdline::parse(&binding, None).ok()?;
        self.clip_command(
            g,
            &command,
            args.get("at").and_then(serde_json::Value::as_str) == Some("original"),
        )
    }

    pub fn clip_command(&mut self, g: &Geometry, command: &str, original: bool) -> Option<bool> {
        let roll = !self.arrange.focus && self.roll_input(g).is_some();
        match command {
            "clip.join" if !roll => {
                let ids: Vec<_> = self
                    .arrange
                    .clips
                    .iter()
                    .map(|(_, id)| id.to_string())
                    .collect();
                if ids.len() < 2 {
                    return Some(true);
                }
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                let joined = self.arrange_run("clip.join", json!({"ids":ids}));
                if let Some(data) = &joined {
                    self.select_made(data);
                }
                self.rebuild(g);
                Some(joined.is_some())
            }
            "edit.copy" | "edit.cut" if !roll => Some(self.board_put(command == "edit.cut")),
            "edit.paste" => {
                if roll {
                    self.roll_paste(g, original)
                } else {
                    Some(self.board_paste(original, self.input_context(g) == "mixer"))
                }
            }
            "select.on-tracks" if roll => Some(self.roll_select_on_tracks(g)),
            "select.on-tracks" => Some(self.select_on_tracks()),
            "select.all" if !roll => Some(self.select_every()),
            "select.invert" => Some(self.select_flip(g, roll)),
            "select.clear" => Some(self.select_none()),
            "select.in-loop" => Some(self.select_loop(g, roll)),
            "clip.delete" if self.arrange.focus && self.clips_and_points_selected() => {
                Some(self.delete_points_and_clips())
            }
            "clip.duplicate" | "clip.delete"
                if self.arrange.focus && !self.arrange.points.is_empty() =>
            {
                Some(self.points_act(command == "clip.duplicate"))
            }
            _ => None,
        }
    }

    /// Whether Ctrl+C and Ctrl+X have something to take: points, clips, or the roll's notes.
    pub(crate) fn has_selection(&self, g: &Geometry) -> bool {
        if !self.arrange.points.is_empty() {
            return true;
        }
        if self.arrange.focus || self.roll_input(g).is_none() {
            return !self.picked_clips().is_empty();
        }
        !self.roll.points.is_empty() || !self.roll_notes(g).is_empty()
    }

    // -------------------------------------------------------------- the selection

    /// The selected clips that are still in the song.
    pub(crate) fn picked_clips(&self) -> Vec<ClipId> {
        let p = self.project();
        self.arrange
            .clips
            .iter()
            .filter(|(t, c)| {
                p.tracks
                    .get(t)
                    .is_some_and(|t| t.clips.iter().any(|k| k.id == *c))
            })
            .map(|(_, c)| *c)
            .collect()
    }

    fn ids_of(ids: &[ClipId]) -> Value {
        Value::Array(ids.iter().map(|c| json!(c.to_string())).collect())
    }

    /// Takes the session's selection over as the arrangement's selected clips.
    pub(crate) fn take_clips(&mut self) {
        let chosen: BTreeSet<ClipId> = self
            .song
            .session()
            .state
            .selection
            .iter()
            .filter_map(|i| match i {
                AnyId::Clip(c) => Some(*c),
                _ => None,
            })
            .collect();
        let p = self.project();
        self.arrange.clips = p
            .tracks
            .values()
            .flat_map(|t| t.clips.iter().map(move |c| (t.id, c.id)))
            .filter(|(_, c)| chosen.contains(c))
            .collect();
        if !self
            .selected_clip
            .is_some_and(|k| self.arrange.clips.contains(&k))
        {
            self.selected_clip = self.arrange.clips.iter().next().copied();
        }
        self.arrange.points.clear();
        self.arrange.touched();
    }

    /// Takes the session's selection over as the roll's selected notes.
    fn take_notes(&mut self) {
        self.roll.selected = self
            .song
            .session()
            .state
            .selection
            .iter()
            .filter_map(|i| match i {
                AnyId::Note(n) => Some(*n),
                _ => None,
            })
            .collect();
        self.roll.points.clear();
    }

    /// Puts what the window shows as selected into the session, so a rule starts from it.
    pub(crate) fn give_selection(&mut self, notes: &[NoteId], clips: &[ClipId]) {
        let ids: Vec<String> = if notes.is_empty() {
            clips.iter().map(ToString::to_string).collect()
        } else {
            notes.iter().map(ToString::to_string).collect()
        };
        if ids.is_empty() {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run("select.clear", json!({}));
        } else {
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            self.arrange_run("select.set", json!({ "ids": ids }));
        }
    }

    fn roll_notes(&self, g: &Geometry) -> Vec<NoteId> {
        let Some(ctx) = self.roll_ctx(g) else {
            return Vec::new();
        };
        self.roll
            .selected_notes(&ctx)
            .iter()
            .map(|n| n.id)
            .collect()
    }

    /// Ctrl+A in the arrangement: every clip; with points selected, every point of their lane.
    fn select_every(&mut self) -> bool {
        if let Some((t, target)) = self.arrange.points.lane().cloned() {
            let all: BTreeSet<Tick> = self
                .project()
                .tracks
                .get(&t)
                .and_then(|tr| tr.automation.iter().find(|l| l.target == target))
                .map(|l| l.points.iter().map(|p| p.tick).collect())
                .unwrap_or_default();
            self.arrange.points.set_all(t, &target, all);
            self.arrange.touched();
            return true;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run("select.clear", json!({}));
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if self.arrange_run("select.all", json!({})).is_none() {
            return true;
        }
        self.take_clips();
        true
    }

    /// Ctrl+Shift+A: every clip of the tracks of the selected clips (or the selected track).
    fn select_on_tracks(&mut self) -> bool {
        let mut ids: Vec<String> = self
            .picked_clips()
            .iter()
            .map(ToString::to_string)
            .collect();
        if ids.is_empty()
            && let Some(RowKind::Track(t)) = self.selected_row
        {
            ids.push(t.to_string());
        }
        if ids.is_empty() {
            return true;
        }
        if self
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            .arrange_run("select.on-tracks", json!({ "ids": ids }))
            .is_some()
        {
            self.take_clips();
        }
        true
    }

    /// Ctrl+I: what is not selected, among the notes of the clip in the roll, or among the clips.
    fn select_flip(&mut self, g: &Geometry, roll: bool) -> bool {
        if roll {
            let Some(clip) = self.roll_ctx(g).map(|c| c.clip.id) else {
                return false;
            };
            let notes = self.roll_notes(g);
            self.give_selection(&notes, &[]);
            let args = json!({ "clip": clip.to_string() });
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            if self.arrange_run("select.invert", args).is_some() {
                self.take_notes();
            }
            return true;
        }
        let clips = self.picked_clips();
        self.give_selection(&[], &clips);
        // With none selected the rule has nothing to flip from: every clip is "not selected".
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if self.arrange_run("select.invert", json!({})).is_some() {
            self.take_clips();
        }
        true
    }

    /// Ctrl+D: nothing selected.
    fn select_none(&mut self) -> bool {
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        self.arrange_run("select.clear", json!({}));
        self.arrange.clips.clear();
        self.arrange.points.clear();
        self.roll.selected.clear();
        self.roll.points.clear();
        self.arrange.touched();
        true
    }

    /// Alt+L: what lies in the loop range.
    fn select_loop(&mut self, g: &Geometry, roll: bool) -> bool {
        if roll {
            let Some(clip) = self.roll_ctx(g).map(|c| c.clip.id) else {
                return false;
            };
            if self
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                .arrange_run("select.in-loop", json!({ "clip": clip.to_string() }))
                .is_some()
            {
                self.take_notes();
            }
            return true;
        }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        if self.arrange_run("select.in-loop", json!({})).is_some() {
            self.take_clips();
        }
        true
    }

    // -------------------------------------------------------------- the clipboard

    /// Ctrl+C and Ctrl+X in the arrangement.
    fn board_put(&mut self, cut: bool) -> bool {
        let command = if cut { "edit.cut" } else { "edit.copy" };
        if let Some((t, target)) = self.arrange.points.lane().cloned()
            && !self.arrange.points.is_empty()
        {
            let ticks = join(self.arrange.points.ticks());
            let args = json!({ "track": t.to_string(), "target": target.to_string(),
                               "ticks": ticks });
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            if self.arrange_run(command, args).is_some() && cut {
                self.arrange.points.clear();
            }
            return true;
        }
        let clips = self.picked_clips();
        if clips.is_empty() {
            return false;
        }
        if self
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            .arrange_run(command, json!({ "ids": Self::ids_of(&clips) }))
            .is_some()
            && cut
        {
            self.after_clips_gone();
        }
        true
    }

    /// The selected clips went (cut, deleted): nothing of them stays selected.
    pub(crate) fn after_clips_gone(&mut self) {
        self.arrange.clips.clear();
        let p = self.project();
        if self.selected_clip.is_some_and(|(t, c)| {
            !p.tracks
                .get(&t)
                .is_some_and(|t| t.clips.iter().any(|k| k.id == c))
        }) {
            self.selected_clip = None;
        }
        self.arrange.touched();
    }

    fn board(&self) -> Option<Clipboard> {
        self.song.session().state.clipboard.clone()
    }

    /// Ctrl+V and Ctrl+Shift+V in the arrangement: at the playhead, or `original`.
    fn board_paste(&mut self, original: bool, mixer: bool) -> bool {
        let Some(board) = self.board() else {
            self.tell("貼り付けるものがありません".to_string());
            return true;
        };
        let at = if original {
            json!("original")
        } else {
            json!(self.playhead)
        };
        match &board {
            Clipboard::Tracks { .. } => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.arrange_run("edit.paste", json!({}));
            }
            // After the selected section, else at the playhead.
            Clipboard::Section { .. } => {
                let at = match self.selected_section() {
                    Some(s) if !original => json!(s.start + s.length),
                    _ => at,
                };
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                if let Some(data) = self.arrange_run("section.paste", json!({ "at": at })) {
                    self.select_section_made(&data);
                }
            }
            Clipboard::Clips(_) => {
                // On the selected track when that takes the clips; else where they came from.
                let mut data = None;
                if !original && let Some(RowKind::Track(t)) = self.selected_row {
                    let args = json!({ "at": at, "track": t.to_string() });
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: Song::ensure_inline refuses unreviewed Session handlers; model edits enqueue history, and reviewed status/list calls use window caches.
                    data = self.song.run("edit.paste", args).ok();
                    if data.is_some() {
                        self.error = None;
                        self.after_edit();
                    }
                }
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                let data = data.or_else(|| self.arrange_run("edit.paste", json!({ "at": at })));
                if let Some(data) = data {
                    self.select_made(&data);
                    self.arrange.points.clear();
                }
            }
            Clipboard::Automation { points, .. } => {
                let mut args = json!({ "at": at });
                let mut over = None;
                if !original && let Some((t, target)) = self.lane_under_pointer() {
                    args["track"] = json!(t.to_string());
                    args["target"] = json!(target.to_string());
                    over = Some((t, target));
                }
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                if self.arrange_run("edit.paste", args).is_some() {
                    let first = board.first_tick();
                    let shift = if original { 0 } else { self.playhead - first };
                    let ticks: BTreeSet<Tick> = points.iter().map(|p| p.tick + shift).collect();
                    if let Some((t, target)) = over.or_else(|| match &board {
                        Clipboard::Automation { track, target, .. } => {
                            Some((*track, target.clone()))
                        }
                        _ => None,
                    }) {
                        self.arrange.points.set_all(t, &target, ticks);
                    }
                    self.arrange.clips.clear();
                    self.arrange.touched();
                }
            }
            Clipboard::Plugins(_) => {
                // On the selected channel; its end of the inserts.
                if let Some(RowKind::Track(t)) = self.selected_row {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.arrange_run("edit.paste", json!({ "track": t.to_string() }));
                } else {
                    self.tell(format!("{}は、ミキサーで貼り付けます", board.describe()));
                }
            }
            Clipboard::Channel(_) => {
                // On the selected channels (the master takes it too).
                let ids: Vec<String> = self
                    .arrange
                    .tracks
                    .iter()
                    .filter_map(|r| match r {
                        RowKind::Track(t) => Some(t.to_string()),
                        _ => None,
                    })
                    .collect();
                let ids = match (ids.is_empty(), self.selected_row) {
                    _ if mixer && self.mixer.master_picked => {
                        vec![TrackId::MASTER.to_string()]
                    }
                    (true, Some(RowKind::Track(t))) => vec![t.to_string()],
                    _ => ids,
                };
                if ids.is_empty() {
                    self.tell(format!("{}は、ミキサーで貼り付けます", board.describe()));
                } else {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.arrange_run("channel.settings.paste", json!({ "ids": ids }));
                }
            }
            Clipboard::Notes(_) | Clipboard::Controller { .. } => {
                self.tell(format!(
                    "{}は、ピアノロールで貼り付けます",
                    board.describe()
                ));
            }
        }
        true
    }

    /// The automation lane the pointer is over, if any.
    fn lane_under_pointer(&self) -> Option<(TrackId, AutoTarget)> {
        match &self.arrange.hover {
            Spot::Lane { track, target }
            | Spot::Point { track, target, .. }
            | Spot::Line { track, target, .. }
            | Spot::LaneHead { track, target, .. } => Some((*track, target.clone())),
            _ => None,
        }
    }

    /// Ctrl+V in the roll: points go into the open controller lane; clips and automation points
    /// do not belong here; notes (and a clipboard the session does not hold) are the roll's own.
    fn roll_paste(&mut self, g: &Geometry, original: bool) -> Option<bool> {
        let board = self.board()?;
        match board {
            Clipboard::Tracks { .. } => {
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                self.arrange_run("edit.paste", json!({}))?;
                self.rebuild(g);
                Some(true)
            }
            Clipboard::Controller { .. } => {
                let ctx = self.roll_input(g)?;
                let at = if original {
                    board.first_tick()
                } else {
                    self.roll.paste_tick(&ctx).unwrap_or(self.playhead)
                };
                let out = self.roll.paste_points(&ctx, &board, at);
                Some(self.apply_roll(g, out))
            }
            Clipboard::Plugins(_) | Clipboard::Channel(_) => {
                self.tell(format!("{}は、ミキサーで貼り付けます", board.describe()));
                Some(true)
            }
            Clipboard::Clips(_) | Clipboard::Automation { .. } | Clipboard::Section { .. } => {
                self.tell(format!("{}は、アレンジで貼り付けます", board.describe()));
                Some(true)
            }
            Clipboard::Notes(_) => None,
        }
    }

    /// D and Delete with automation points selected.
    pub(crate) fn points_act(&mut self, duplicate: bool) -> bool {
        let Some((t, target)) = self.arrange.points.lane().cloned() else {
            return false;
        };
        let ticks = self.arrange.points.ticks().clone();
        let args = json!({ "track": t.to_string(), "target": target.to_string(),
                           "ticks": join(&ticks) });
        if duplicate {
            // As far to the right as they are wide; one beat when they are a single point.
            let span = ticks.last().copied().unwrap_or(0) - ticks.first().copied().unwrap_or(0);
            let by = if span > 0 {
                span
            } else {
                Tick::from(self.project().ppq)
            };
            let mut args = args;
            args["by"] = json!(by);
            // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
            if self.arrange_run("edit.duplicate", args).is_some() {
                let moved = ticks.iter().map(|k| k + by).collect();
                self.arrange.points.set_all(t, &target, moved);
            }
        // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
        } else if self.arrange_run("edit.delete", args).is_some() {
            self.arrange.points.clear();
        }
        true
    }

    // -------------------------------------------------------------- automation points

    /// A press in the arrangement: a point is selected (with Shift, added), anything else lets
    /// the points go. Called with the spot the press found.
    pub(crate) fn point_press(&mut self, spot: &Spot, tool: Tool, mods: Mods) {
        match spot {
            Spot::Point {
                track,
                target,
                index,
            } if tool != Tool::Eraser && !mods.alt => {
                let tick = self
                    .project()
                    .tracks
                    .get(track)
                    .and_then(|t| t.automation.iter().find(|l| l.target == *target))
                    .and_then(|l| l.points.get(*index))
                    .map(|p| p.tick);
                if let Some(tick) = tick {
                    self.arrange.points.select(*track, target, tick, mods.shift);
                    if !mods.shift {
                        self.arrange.clips.clear();
                    }
                    self.arrange.touched();
                }
            }
            _ if !mods.shift && !self.arrange.points.is_empty() => {
                self.arrange.points.clear();
                self.arrange.touched();
            }
            _ => {}
        }
    }

    /// The point a drag moved (or a press drew) is the selected one.
    pub(crate) fn point_followed(&mut self, spot: &Spot, preview: &Preview) {
        if let (Spot::Point { track, target, .. }, Preview::Point { tick, .. }) = (spot, preview) {
            self.arrange.points.select(*track, target, *tick, false);
        }
    }

    /// A press on an empty stretch of a lane drew a point at `tick`: it is the selected one.
    pub(crate) fn point_drawn(&mut self, track: TrackId, target: &AutoTarget, tick: Tick) {
        self.arrange.points.select(track, target, tick, false);
        self.arrange.clips.clear();
    }
}

/// `1,2,3`: the ticks as the commands take them.
fn join(ticks: &BTreeSet<Tick>) -> String {
    ticks
        .iter()
        .map(Tick::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests;
