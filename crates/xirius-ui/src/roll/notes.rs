//! Pointer work on the notes: pressing, dragging, releasing, hovering, the
//! wheel. Each drag keeps the notes as they were at the press and rebuilds
//! its [`Preview`] from them on every move and every change of the keys held
//! (EDT-053); the release turns the preview into one core command.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use xirius_core::id::NoteId;
use xirius_core::model::Note;
use xirius_core::time::Tick;

use super::frame::View;
use super::{
    BEAT_PX_MAX, BEAT_PX_MIN, Balloon, Ctx, Drag, Edit, Mode, NoteView, Out, Preview, ROW_H_MAX,
    ROW_H_MIN, Roll, SLOP, Then, VEL_PER_PX, Zone, ids_json, length_text, pitch_name,
};
use crate::studio::Rect;
use crate::tools::{Area, Mods, Tool, snap_round};

/// Wheel pixels that count as one notch of zoom.
pub const WHEEL_NOTCH: f32 = 60.0;

/// The rectangle a note is hit in. In the drum view a hit is a square centered on its start.
pub fn note_rect(v: &View, n: &NoteView) -> Rect {
    if v.drums() {
        return Rect::new(v.x(n.start) - v.row_h / 2.0, v.y(n.pitch), v.row_h, v.row_h);
    }
    Rect::new(v.x(n.start), v.y(n.pitch), v.px(n.length).max(2.0), v.row_h)
}

/// Which of the note's four places `(x, y)` is in.
pub fn zone_of(r: &Rect, row_h: f32, x: f32, y: f32) -> Zone {
    let edge = 6.0f32.min(r.w / 3.0);
    if x < r.x + edge {
        Zone::EdgeL
    } else if x >= r.right() - edge {
        Zone::EdgeR
    } else if row_h >= 12.0 && y < r.y + 4.0 {
        Zone::Velocity
    } else if row_h >= 12.0 && y >= r.bottom() - 4.0 {
        Zone::Split
    } else {
        Zone::Body
    }
}

/// The width of the column left of the grid for what the clip is shown as.
fn keys_width(ctx: &Ctx) -> f32 {
    if ctx.drum.is_some() {
        super::DRUM_KEYS_W
    } else {
        xirius_design::Layout::PIANO_KEYS_W
    }
}

fn balloon_at(x: f32, y: f32, text: String) -> Option<Balloon> {
    Some(Balloon {
        at: (x as i32 + 14, y as i32 - 28),
        text,
    })
}

impl Roll {
    /// Follows the song: a new clip starts a fresh view, the same clip only
    /// forgets notes that are gone.
    pub fn sync(&mut self, ctx: &Ctx) {
        if self.clip == Some(ctx.clip.id) && self.drum_seen != ctx.drum {
            self.drum_seen = ctx.drum.clone();
            self.keys_w = keys_width(ctx);
            self.center(ctx);
        }
        if self.clip == Some(ctx.clip.id)
            && self
                .synced_project
                .as_ref()
                .is_some_and(|p| std::sync::Arc::ptr_eq(p, &ctx.project))
        {
            self.clamp_scroll(ctx);
            return;
        }
        if self.clip != Some(ctx.clip.id) {
            let (row_h, beat_px, extended, scale, next_vel) = (
                self.row_h,
                self.beat_px,
                self.extended,
                self.scale,
                self.next_vel,
            );
            let clipboard = std::mem::take(&mut self.clipboard);
            *self = Roll {
                clipboard,
                row_h,
                beat_px,
                extended,
                scale,
                next_vel,
                clip: Some(ctx.clip.id),
                drum_seen: ctx.drum.clone(),
                keys_w: keys_width(ctx),
                ..Roll::default()
            };
            for l in &ctx.clip.controllers {
                self.lanes
                    .push(super::Lane::new(super::LaneKind::Ctrl(l.kind)));
            }
            self.center(ctx);
        } else {
            let mut alive: BTreeSet<NoteId> = ctx.notes().iter().map(|n| n.id).collect();
            alive.extend(ctx.part_notes().map(|(_, n)| n.id));
            self.selected.retain(|id| alive.contains(id));
            if let Some(d) = &self.drag
                && d.orig.iter().any(|(id, _)| !alive.contains(id))
            {
                self.drag = None;
                self.preview = Preview::default();
            }
            // Controllers added through another command source get a lane.
            for l in &ctx.clip.controllers {
                let kind = super::LaneKind::Ctrl(l.kind);
                if !self.hidden.contains(&l.kind) && !self.lanes.iter().any(|x| x.kind == kind) {
                    self.lanes.push(super::Lane::new(kind));
                }
            }
        }
        // Points that are gone (deleted, undone) leave the selection.
        if let Some(k) = self.active().and_then(|l| l.kind.ctrl()) {
            let alive: BTreeSet<Tick> = ctx
                .clip
                .controllers
                .iter()
                .find(|l| l.kind == k)
                .map(|l| l.points.iter().map(|p| p.tick).collect())
                .unwrap_or_default();
            self.points.retain(|t| alive.contains(t));
        }
        self.clamp_scroll(ctx);
        self.synced_project = Some(ctx.project.clone());
    }

    /// Puts the notes in the middle of the grid.
    pub fn center(&mut self, ctx: &Ctx) {
        let rows = (self.frame(ctx.body).grid.h / self.row_h).floor().max(1.0) as usize;
        if ctx.drum.is_some() {
            // The rows the hits are on in the middle; with none, the bottom of the map (the kick).
            let ranks: Vec<usize> = ctx
                .notes()
                .iter()
                .map(|n| ctx.rows().rank(n.pitch))
                .collect();
            let top = match (ranks.iter().min(), ranks.iter().max()) {
                (Some(lo), Some(hi)) => ((lo + hi) / 2).saturating_sub(rows / 2),
                _ => ctx.rows().len(),
            };
            self.scroll = (-self.lead(ctx), top as f32 * self.row_h);
            self.clamp_scroll(ctx);
            return;
        }
        let top = crate::studio::roll_top_pitch(ctx.notes(), rows);
        self.scroll.0 = 0.0;
        self.scroll_to_pitch(ctx, top);
    }

    /// The topmost note under a point, and the place on it.
    pub fn hit_note(&self, ctx: &Ctx, v: &View, x: f32, y: f32) -> Option<(NoteId, Zone)> {
        if !v.grid.contains(x, y) {
            return None;
        }
        ctx.notes().iter().rev().find_map(|n| {
            let r = note_rect(v, &self.shown(n));
            r.contains(x, y).then(|| {
                if v.drums() {
                    return (n.id, Zone::Body);
                }
                (
                    n.id,
                    zone_of(&r, if self.extended { v.row_h } else { 0.0 }, x, y),
                )
            })
        })
    }

    fn snapshot(&self, ctx: &Ctx) -> Vec<(NoteId, NoteView)> {
        ctx.notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .map(|n| (n.id, NoteView::from(n)))
            .collect()
    }

    fn clamp_pitch(p: i32) -> u8 {
        p.clamp(0, 127) as u8
    }

    fn clamp_tick(ctx: &Ctx, t: Tick) -> Tick {
        t.clamp(0, (ctx.clip_len() - 1).max(0))
    }

    /// Where a click splits: the grid, or a quarter beat when the grid is off.
    fn cut_tick(&self, ctx: &Ctx, shift: bool, tick: Tick) -> Tick {
        match ctx.snap(self.beat_px, shift) {
            Some(_) => ctx.snapped(self.beat_px, shift, tick),
            None => snap_round(tick, (ctx.ppq() / 4).max(1)),
        }
    }

    /// Where a press with the cut tool would split the note under the pointer
    /// (ticks from the clip's head), if it would split one.
    pub fn cut_line(&self, ctx: &Ctx) -> Option<(NoteId, Tick)> {
        if self.drag.is_some() || ctx.tools.held(Area::Roll, self.mods) != Tool::Cut {
            return None;
        }
        let (x, _) = self.pointer?;
        let (id, _) = self.hover?;
        let rel = self.cut_tick(ctx, self.mods.shift, self.view(ctx).tick(x));
        let n = ctx.notes().iter().find(|n| n.id == id)?;
        (rel > n.start && rel < n.start + n.length).then_some((id, rel))
    }

    /// The band laid on the note's top (velocity) or bottom (split) edge while the arrow tool's
    /// pointer is on that place, in the extended mode only (2026-10-21): 4px tall inside the
    /// note's face, between the two edge places.
    pub fn zone_band(&self, ctx: &Ctx) -> Option<(Zone, Rect)> {
        if self.drag.is_some() || ctx.tools.held(Area::Roll, self.mods) != Tool::Select {
            return None;
        }
        let (id, zone) = self.hover?;
        if !matches!(zone, Zone::Velocity | Zone::Split) {
            return None;
        }
        let v = self.view(ctx);
        let n = ctx.notes().iter().find(|n| n.id == id)?;
        let r = note_rect(&v, &self.shown(n));
        let edge = 6.0f32.min(r.w / 3.0);
        let y = if zone == Zone::Velocity {
            r.y + 1.0
        } else {
            r.bottom() - 5.0
        };
        Some((zone, Rect::new(r.x + edge, y, r.w - edge * 2.0, 4.0)))
    }

    /// The note a press with the mute tool would flip, while the pointer is on it.
    pub fn mute_target(&self, ctx: &Ctx) -> Option<NoteId> {
        if self.drag.is_some() || ctx.tools.held(Area::Roll, self.mods) != Tool::Mute {
            return None;
        }
        self.hover.map(|(id, _)| id)
    }

    fn split_edit(&self, ctx: &Ctx, ids: Vec<NoteId>, rel: Tick) -> Option<Edit> {
        let crossing: Vec<NoteId> = ctx
            .notes()
            .iter()
            .filter(|n| ids.contains(&n.id) && rel > n.start && rel < n.start + n.length)
            .map(|n| n.id)
            .collect();
        (!crossing.is_empty()).then(|| {
            Edit::new(
                "note.split",
                json!({ "ids": ids_json(&crossing), "at": ctx.clip.start + rel }),
                Then::Select,
            )
        })
    }

    // ------------------------------------------------------------- press

    pub fn down(&mut self, ctx: &Ctx, x: f32, y: f32, mods: Mods, clicks: u32) -> Out {
        self.sync(ctx);
        self.mods = mods;
        self.pointer = Some((x, y));
        // The pointer holds the lit key from here, not the key that sounded it.
        self.audition_by_key = false;
        if self.menu.is_some() {
            return self.menu_down(ctx, x, y);
        }
        let frame = self.frame(ctx.body);
        if frame.strip.contains(x, y) {
            return self.strip_down(ctx, &frame, x, y);
        }
        if frame.ruler.contains(x, y) {
            return self.ruler_down(ctx, &frame, x, y);
        }
        if let Some(index) = frame.lane_at(x, y).map(|l| l.index) {
            return self.lane_down(ctx, &frame, index, x, y, clicks);
        }
        if frame.keys.contains(x, y) {
            let v = self.view(ctx);
            let pitch = Self::clamp_pitch(v.pitch_at(y));
            if mods.shift {
                self.selected = ctx
                    .notes()
                    .iter()
                    .filter(|n| n.pitch == pitch)
                    .map(|n| n.id)
                    .collect();
                self.audition = None;
                return Out::used();
            }
            self.audition = Some(pitch);
            self.audition_by_edit = false;
            self.drag = Some(Drag::new(Mode::Audition, (x, y)));
            return Out::used();
        }
        if !frame.grid.contains(x, y) {
            return Out::quiet();
        }
        self.points.clear();
        let v = self.view(ctx);
        let tool = if mods.ctrl && mods.alt {
            Tool::Select
        } else {
            ctx.tools.held(Area::Roll, mods)
        };
        if tool == Tool::Select
            && clicks == 1
            && let Some(out) = self.join_under(ctx, &v, x, y)
        {
            return out;
        }
        let hit = self.hit_note(ctx, &v, x, y);
        let zone = hit
            .map(|(_, z)| crate::mouse_dispatch::roll_zone(z))
            .unwrap_or("empty");
        let context = format!("roll.{}.{}.drag", tool.name(), zone);
        self.mods = crate::mouse_dispatch::modifiers(&ctx.mousemap, &context, mods, false);
        let click_context = context.replace(".drag", ".click");
        let click = crate::mouse_dispatch::action(&ctx.mousemap, &click_context, mods);
        let gesture = crate::mouse_dispatch::action(&ctx.mousemap, &context, mods);
        if gesture.as_deref() == Some("") && click.as_deref() == Some("") {
            return Out::used();
        }
        if let Some(action) = click.as_deref() {
            self.mods.shift = action == "select.toggle";
        }
        // Past the clip's end the pencil keeps drawing: the clip grows to take the note.
        let past_end = v.tick(x) >= ctx.clip_len();
        if hit.is_none()
            && !(0..ctx.clip_len()).contains(&v.tick(x))
            && clicks == 1
            && (tool == Tool::Select || (tool == Tool::Draw && !past_end))
        {
            self.drag = Some(Drag::new(Mode::Part, (x, y)));
            self.selected.clear();
            return Out::used();
        }
        let out = match tool {
            Tool::Select | Tool::Range if clicks >= 2 => self.double_click(ctx, &v, x, hit),
            Tool::Select => self.select_down(ctx, x, y, hit),
            Tool::Range => self.marquee_down(x, y),
            Tool::Draw => match hit {
                Some((id, _)) => {
                    let mut d = Drag::new(Mode::Erase, (x, y));
                    d.marked.insert(id);
                    self.drag = Some(d);
                    self.recompute(ctx);
                    Out::used()
                }
                None => self.draw_down(ctx, &v, x, y),
            },
            Tool::Eraser => {
                let mut d = Drag::new(Mode::Erase, (x, y));
                if let Some((id, _)) = hit {
                    d.marked.insert(id);
                }
                self.drag = Some(d);
                self.recompute(ctx);
                Out::used()
            }
            Tool::Cut => {
                let Some((id, _)) = hit else {
                    return Out::used();
                };
                let rel = self.cut_tick(ctx, mods.shift, v.tick(x));
                match self.split_edit(ctx, vec![id], rel) {
                    Some(e) => Out::edit(e),
                    None => Out::used(),
                }
            }
            Tool::Mute => match hit {
                Some((id, _)) => Out::edit(Edit::new(
                    "note.mute",
                    json!({ "ids": ids_json(&[id]) }),
                    Then::Keep,
                )),
                None => Out::used(),
            },
            Tool::Audition => {
                self.audition = Some(Self::clamp_pitch(v.pitch_at(y)));
                self.audition_by_edit = false;
                self.listen = Some(ctx.clip.start + v.tick(x).max(0));
                self.drag = Some(Drag::new(Mode::Audition, (x, y)));
                Out::used()
            }
        };
        // On the empty part the click takes every note from the pointer on; a drag from there
        // goes on to add what its rectangle holds.
        if hit.is_none() && tool == Tool::Select && click.as_deref() == Some("select.after") {
            let from = v.tick(x);
            let after: BTreeSet<NoteId> = ctx
                .notes()
                .iter()
                .filter(|n| n.start >= from)
                .map(|n| n.id)
                .collect();
            self.selected = after.clone();
            if let Some(drag) = &mut self.drag {
                drag.base = after;
            }
        }
        if let Some(drag) = &mut self.drag {
            drag.map_context = Some(context);
            drag.click_action = click;
            drag.drag_enabled = gesture.as_deref() != Some("");
            if let Some(action) = gesture.as_deref() {
                match action {
                    "note.velocity.set" => drag.mode = Mode::Velocity,
                    "note.move" | "note.duplicate" => drag.mode = Mode::Move,
                    "select.rect" | "select.rect --add" => drag.mode = Mode::Marquee,
                    "note.audition" => {
                        drag.mode = Mode::Audition;
                        self.audition = Some(Self::clamp_pitch(v.pitch_at(y)));
                        self.audition_by_edit = false;
                        self.listen = Some(ctx.clip.start + v.tick(x).max(0));
                    }
                    "note.resize" | "note.resize --adjacent" => {
                        drag.mode = if hit.is_some_and(|(_, z)| z == Zone::EdgeL) {
                            Mode::ResizeL
                        } else {
                            Mode::ResizeR
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }

    fn double_click(&mut self, ctx: &Ctx, v: &View, x: f32, hit: Option<(NoteId, Zone)>) -> Out {
        match hit {
            Some((id, _)) => {
                self.selected.remove(&id);
                Out::edit(Edit::new(
                    "note.delete",
                    json!({ "ids": ids_json(&[id]) }),
                    Then::Keep,
                ))
            }
            None => {
                let (_, y) = self.pointer.unwrap_or((x, 0.0));
                let at =
                    Self::clamp_tick(ctx, ctx.floored(self.beat_px, self.mods.shift, v.tick(x)));
                let pitch = Self::clamp_pitch(v.pitch_at(y));
                Out::edit(self.put_edit(
                    ctx,
                    &[NoteView {
                        start: at,
                        length: ctx.tools.snap.grid,
                        pitch,
                        velocity: self.next_vel,
                        muted: false,
                    }],
                ))
            }
        }
    }

    /// How long a note the pencil lays down with a bare click: the note last drawn, resized or
    /// selected when `last` (the setting 直前のノート) and there is one, else one grid step.
    pub fn draw_length(&self, last: bool, step: Tick) -> Tick {
        if last && self.next_length > 0 {
            self.next_length
        } else {
            step
        }
    }

    fn select_down(&mut self, ctx: &Ctx, x: f32, y: f32, hit: Option<(NoteId, Zone)>) -> Out {
        let shift = self.mods.shift;
        let Some((id, zone)) = hit else {
            return self.marquee_down(x, y);
        };
        let mut toggle_off = false;
        if shift {
            if self.selected.contains(&id) {
                toggle_off = true;
            } else {
                self.selected.insert(id);
            }
        } else if !self.selected.contains(&id) {
            self.selected.clear();
            self.selected.insert(id);
        }
        let mode = if self.mods.ctrl && self.mods.alt {
            Mode::Velocity
        } else {
            match zone {
                Zone::EdgeL => Mode::ResizeL,
                Zone::EdgeR => Mode::ResizeR,
                Zone::Velocity => Mode::Velocity,
                Zone::Body | Zone::Split => Mode::Move,
            }
        };
        let mut d = Drag::new(mode, (x, y));
        d.orig = self.snapshot(ctx);
        d.head = d.orig.iter().position(|(i, _)| *i == id).unwrap_or(0);
        // A note taken in hand is the "直前のノート" (the drum view has no lengths to carry).
        if ctx.drum.is_none()
            && !toggle_off
            && let Some((_, o)) = d.orig.get(d.head)
        {
            self.next_length = o.length;
        }
        d.zone = zone;
        d.toggle_off = toggle_off;
        d.clicked = Some(id);
        self.audition = (!toggle_off)
            .then(|| d.orig.get(d.head).map(|(_, o)| o.pitch))
            .flatten();
        self.audition_by_edit = self.audition.is_some();
        self.drag = Some(d);
        Out::used()
    }

    fn marquee_down(&mut self, x: f32, y: f32) -> Out {
        let mut d = Drag::new(Mode::Marquee, (x, y));
        if self.mods.shift {
            d.base = self.selected.clone();
        } else {
            self.selected.clear();
        }
        self.drag = Some(d);
        Out::used()
    }

    fn draw_down(&mut self, ctx: &Ctx, v: &View, x: f32, y: f32) -> Out {
        let floored = ctx.floored(self.beat_px, self.mods.shift, v.tick(x));
        let start = if v.tick(x) >= ctx.clip_len() {
            floored.max(0)
        } else {
            Self::clamp_tick(ctx, floored)
        };
        let pitch = Self::clamp_pitch(v.pitch_at(y));
        let mut d = Drag::new(Mode::Draw, (x, y));
        d.draw = Some((start, pitch, self.next_vel));
        self.audition = Some(pitch);
        self.audition_by_edit = true;
        self.drag = Some(d);
        self.recompute(ctx);
        Out::used()
    }

    /// The pitch the roll sounds: the lit key of the keyboard or the audition tool, and of a
    /// note picked or drawn only with `edit.audition`.
    pub(crate) fn sounding(&self) -> Option<u8> {
        self.audition
            .filter(|_| !self.audition_by_edit || self.audition_edits)
    }

    pub(super) fn put_edit(&self, ctx: &Ctx, notes: &[NoteView]) -> Edit {
        // A note that starts past the clip's end asks the clip to grow to hold it.
        let grow = notes.iter().any(|n| n.start >= ctx.clip_len());
        let list: Vec<Value> = notes
            .iter()
            .map(|n| {
                let at = if grow {
                    n.start.max(0)
                } else {
                    Self::clamp_tick(ctx, n.start)
                };
                json!({
                    "at": at,
                    "pitch": n.pitch,
                    "length": n.length.max(1),
                    "velocity": n.velocity,
                    "muted": n.muted,
                })
            })
            .collect();
        Edit::new(
            "note.put",
            if grow {
                json!({ "clip": ctx.clip.id.to_string(), "notes": list, "grow": true })
            } else {
                json!({ "clip": ctx.clip.id.to_string(), "notes": list })
            },
            Then::Select,
        )
    }

    // -------------------------------------------------------------- move

    pub fn moved(&mut self, ctx: &Ctx, x: f32, y: f32, mods: Mods) -> Out {
        let before = self.cursor(Some(ctx));
        let cut_before = self.cut_line(ctx);
        let keys = self.frame(ctx.body).keys;
        let v = self.view(ctx);
        let key_pitch = |p: Option<(f32, f32)>| {
            p.filter(|(x, y)| keys.contains(*x, *y))
                .map(|(_, y)| v.pitch_at(y))
        };
        let key_changed = key_pitch(self.pointer) != key_pitch(Some((x, y)));
        self.mods = self.mapped_mods(ctx, mods);
        self.pointer = Some((x, y));
        if self.drag.is_some() {
            self.recompute(ctx);
            return Out::used();
        }
        let frame = self.frame(ctx.body);
        let grip = self.grip_under(ctx, &frame, x, y);
        let grip_changed = grip != self.ruler_hover;
        self.ruler_hover = grip;
        let on_end = self.end_grab(ctx, &frame, x, y);
        let end_changed = on_end != self.end_hover;
        self.end_hover = on_end;
        let v = self.view(ctx);
        let hover = self.hit_note(ctx, &v, x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        Out {
            redraw: changed
                || grip_changed
                || end_changed
                || key_changed
                || cut_before != self.cut_line(ctx)
                || before != self.cursor(Some(ctx)),
            used: false,
            ..Out::default()
        }
    }

    /// A modifier key went down or up.
    pub fn mods_changed(&mut self, ctx: &Ctx, mods: Mods) -> Out {
        if mods == self.mods {
            return Out::quiet();
        }
        self.mods = self.mapped_mods(ctx, mods);
        if self.drag.is_some() {
            self.recompute(ctx);
        }
        Out {
            redraw: true,
            ..Out::default()
        }
    }

    /// Esc: drop the drag without changing the song.
    pub fn cancel(&mut self) -> Out {
        let drag = self.drag.take().is_some();
        self.zoom_anchor = None;
        let menu = self.menu.take().is_some();
        let was = drag || menu;
        self.preview = Preview::default();
        self.audition = None;
        self.listen = None;
        Out {
            redraw: was,
            used: was,
            ..Out::default()
        }
    }

    /// Works out what the drag would do now, from the notes as they were at
    /// the press, the pointer and the keys held.
    pub fn recompute(&mut self, ctx: &Ctx) {
        let Some(mut d) = self.drag.take() else {
            return;
        };
        let Some((px, py)) = self.pointer else {
            self.drag = Some(d);
            return;
        };
        let (dx, dy) = (px - d.press.0, py - d.press.1);
        if !d.moved && dx.hypot(dy) >= SLOP {
            d.moved = true;
        }
        if !d.drag_enabled && d.moved {
            self.preview = Preview::default();
            self.drag = Some(d);
            return;
        }
        let always = matches!(
            d.mode,
            Mode::Draw | Mode::Erase | Mode::Marquee | Mode::Lane | Mode::Zoom
        );
        if !d.moved && !always {
            self.drag = Some(d);
            return;
        }
        let v = self.view(ctx);
        let mut pv = std::mem::take(&mut self.preview);
        pv.changed.clear();
        pv.ghosts.clear();
        pv.balloon = None;
        pv.marquee = None;
        pv.erasing.clear();
        pv.lane = None;
        pv.band = None;
        pv.loop_span = None;
        pv.clip_end = None;
        pv.strum = None;
        match d.mode {
            Mode::ClipEnd => pv.clip_end = Some(self.ruler_end(ctx, px)),
            Mode::Zoom => self.zoom_track_to(ctx, px),
            Mode::Loop(grip) => pv.loop_span = self.ruler_loop(ctx, grip, d.press.0, px),
            Mode::Move => {
                if !(self.mods.ctrl
                    && self.mods.alt
                    && self.preview_strum(ctx, &v, &d, dx, &mut pv))
                {
                    self.preview_move(ctx, &v, &d, (dx, dy), &mut pv);
                    if let Some(pitch) = d
                        .orig
                        .get(d.head)
                        .and_then(|(id, _)| pv.changed.get(id))
                        .or_else(|| pv.ghosts.get(d.head))
                        .map(|n| n.pitch)
                    {
                        self.audition = Some(pitch);
                        self.audition_by_edit = true;
                    }
                }
            }
            Mode::ResizeR => self.preview_right(ctx, &v, &d, dx, &mut pv),
            Mode::ResizeL => self.preview_left(ctx, &v, &d, dx, &mut pv),
            Mode::Velocity => {
                let dv = (-dy * d.vel_per_px).round() as i32;
                for (id, o) in &d.orig {
                    let mut n = *o;
                    n.velocity = (i32::from(o.velocity) + dv).clamp(1, 127) as u8;
                    pv.changed.insert(*id, n);
                }
                if let Some(n) = d.orig.get(d.head).and_then(|(id, _)| pv.changed.get(id)) {
                    pv.balloon = balloon_at(px, py, format!("ベロシティ {}", n.velocity));
                }
            }
            Mode::Draw => self.preview_draw(ctx, &v, &d, (px, py), &mut pv),
            Mode::Erase => {
                let steps = ((px - d.last.0).hypot(py - d.last.1) / 4.0).ceil().max(1.0) as u32;
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let (sx, sy) = (
                        d.last.0 + (px - d.last.0) * t,
                        d.last.1 + (py - d.last.1) * t,
                    );
                    if let Some((id, _)) = self.hit_note(ctx, &v, sx, sy) {
                        d.marked.insert(id);
                    }
                }
                pv.erasing = d.marked.clone();
            }
            Mode::Marquee => {
                let (ax, ay) = v.world(d.press.0, d.press.1);
                let (bx, by) = v.world(px, py);
                let (x0, x1) = (ax.min(bx), ax.max(bx));
                let (y0, y1) = (ay.min(by), ay.max(by));
                let mut sel = d.base.clone();
                for n in ctx.notes() {
                    let r = note_rect(&v, &NoteView::from(n));
                    let (wx, wy) = v.world(r.x, r.y);
                    let (ww, wh) = (r.w, r.h);
                    if wx < x1 && wx + ww > x0 && wy < y1 && wy + wh > y0 {
                        sel.insert(n.id);
                    }
                }
                self.selected = sel;
                pv.marquee = Some(((x0, y0), (x1, y1)));
            }
            Mode::Audition => {
                self.audition = Some(Self::clamp_pitch(v.pitch_at(py)));
            }
            Mode::Part => {
                let (ax, ay) = v.world(d.press.0, v.grid.y);
                let (bx, by) = v.world(px, v.grid.bottom());
                pv.marquee = Some(((ax, ay), (bx, by)));
            }
            Mode::Lane => self.lane_update(ctx, &mut d, &mut pv),
        }
        d.last = (px, py);
        self.preview = pv;
        self.drag = Some(d);
    }

    /// Mod+Alt held while the top or bottom note of a selection is dragged sideways: the chords in
    /// the selection fan out, the way `note.strum` does it (EDT-054). Dragging the top note to the
    /// right starts the low notes first. False where the selection has no chord to spread.
    fn preview_strum(&self, ctx: &Ctx, v: &View, d: &Drag, dx: f32, pv: &mut Preview) -> bool {
        let head = d.orig[d.head].1;
        let (lo, hi) = d.orig.iter().fold((u8::MAX, 0), |(lo, hi), (_, o)| {
            (lo.min(o.pitch), hi.max(o.pitch))
        });
        if lo == hi || (head.pitch != lo && head.pitch != hi) {
            return false;
        }
        let mut chords: std::collections::BTreeMap<Tick, Vec<(u8, NoteId)>> = Default::default();
        for (id, o) in &d.orig {
            chords.entry(o.start).or_default().push((o.pitch, *id));
        }
        chords.retain(|_, c| c.len() > 1);
        if chords.is_empty() {
            return false;
        }
        let want = head.start + (dx / v.beat_px * v.ppq).round() as Tick;
        let by = ctx.snapped(self.beat_px, self.mods.shift, want) - head.start;
        let spread = if head.pitch == hi { by } else { -by };
        pv.strum = Some(spread);
        for chord in chords.values_mut() {
            chord.sort();
            if spread < 0 {
                chord.reverse();
            }
            let last = (chord.len() - 1) as Tick;
            for (i, (_, id)) in chord.iter().enumerate() {
                let o = d
                    .orig
                    .iter()
                    .find(|(x, _)| x == id)
                    .map_or(head, |(_, o)| *o);
                let shift = (spread.abs() * i as Tick / last).min(o.length - 1);
                pv.changed.insert(
                    *id,
                    NoteView {
                        start: o.start + shift,
                        length: o.length - shift,
                        ..o
                    },
                );
            }
        }
        let (px, py) = self.pointer.unwrap_or((0.0, 0.0));
        pv.balloon = balloon_at(px, py, format!("ストラム {spread:+} tick"));
        true
    }

    fn preview_move(&self, ctx: &Ctx, v: &View, d: &Drag, (dx, dy): (f32, f32), pv: &mut Preview) {
        let shift = self.mods.shift;
        let copy = self.mods.alt;
        let head = d.orig[d.head].1;
        let want = head.start + (dx / v.beat_px * v.ppq).round() as Tick;
        let min_start = d.orig.iter().map(|(_, o)| o.start).min().unwrap_or(0);
        let max_start = d.orig.iter().map(|(_, o)| o.start).max().unwrap_or(0);
        let hi = (ctx.clip_len() - 1 - max_start).max(-min_start);
        let ds = (ctx.snapped(self.beat_px, shift, want) - head.start).clamp(-min_start, hi);
        // Whole rows down (negative: up), kept on the grid for every note at once.
        let ranks = d.orig.iter().map(|(_, o)| v.rows.rank(o.pitch) as i32);
        let top = ranks.clone().min().unwrap_or(0);
        let bottom = ranks.max().unwrap_or(0);
        let last = v.rows.len() as i32 - 1;
        let drows = ((dy / v.row_h).round() as i32).clamp(-top, (last - bottom).max(-top));
        for (id, o) in &d.orig {
            let n = NoteView {
                start: o.start + ds,
                pitch: v.pitch_by(o.pitch, drows),
                ..*o
            };
            if copy {
                pv.ghosts.push(n);
            } else {
                pv.changed.insert(*id, n);
            }
        }
        let h = NoteView {
            start: head.start + ds,
            pitch: v.pitch_by(head.pitch, drows),
            ..head
        };
        let (px, py) = self.pointer.unwrap_or((0.0, 0.0));
        pv.balloon = balloon_at(
            px,
            py,
            format!(
                "{} · {}{}",
                if v.drums() {
                    ctx.drum_name(h.pitch)
                } else {
                    pitch_name(h.pitch)
                },
                ctx.bbt(h.start),
                if copy { " ＋" } else { "" }
            ),
        );
    }

    fn preview_right(&self, ctx: &Ctx, v: &View, d: &Drag, dx: f32, pv: &mut Preview) {
        let (shift, ctrl, alt) = (self.mods.shift, self.mods.ctrl, self.mods.alt);
        let min = ctx.min_len(self.beat_px, shift);
        let head = d.orig[d.head].1;
        let end = ctx.snapped(
            self.beat_px,
            shift,
            head.start + head.length + (dx / v.beat_px * v.ppq).round() as Tick,
        );
        let mut len = (end - head.start).max(min);
        let cap = ctx.clip_len() - head.start;
        if cap >= min {
            len = len.min(cap);
        }
        let dl = len - head.length;
        let s0 = d.orig.iter().map(|(_, o)| o.start).min().unwrap_or(0);
        let scale = len as f64 / head.length.max(1) as f64;
        for (id, o) in &d.orig {
            let mut n = *o;
            if d.orig.len() == 1 || (!ctrl && !alt) {
                n.length = (o.length + dl).max(min);
            } else if ctrl && alt {
                n.length = (head.start + len - o.start).max(min);
            } else if ctrl {
                n.length = len;
            } else {
                n.start = s0 + ((o.start - s0) as f64 * scale).round() as Tick;
                n.length = ((o.length as f64 * scale).round() as Tick).max(min);
            }
            pv.changed.insert(*id, n);
        }
        let (px, py) = self.pointer.unwrap_or((0.0, 0.0));
        let shown = pv.changed.get(&d.orig[d.head].0).map_or(len, |n| n.length);
        pv.balloon = balloon_at(px, py, format!("長さ {}", length_text(shown, ctx.ppq())));
    }

    fn preview_left(&self, ctx: &Ctx, v: &View, d: &Drag, dx: f32, pv: &mut Preview) {
        let shift = self.mods.shift;
        let min = ctx.min_len(self.beat_px, shift);
        let head = d.orig[d.head].1;
        let want = head.start + (dx / v.beat_px * v.ppq).round() as Tick;
        let min_start = d.orig.iter().map(|(_, o)| o.start).min().unwrap_or(0);
        let room = d.orig.iter().map(|(_, o)| o.length).min().unwrap_or(1) - min;
        let ds =
            (ctx.snapped(self.beat_px, shift, want) - head.start).clamp(-min_start, room.max(0));
        let start = head.start + ds;
        let length = head.length - ds;
        let end_anchor = d
            .orig
            .iter()
            .map(|(_, n)| n.start + n.length)
            .max()
            .unwrap_or(0);
        let scale = length as f64 / head.length.max(1) as f64;
        for (id, o) in &d.orig {
            let mut n = *o;
            let end = o.start + o.length;
            if d.orig.len() > 1 && self.mods.ctrl && self.mods.alt {
                n.start = start.min((end - min).max(0));
                n.length = end - n.start;
            } else if d.orig.len() > 1 && self.mods.ctrl {
                n.start = (end - length).max(0);
                n.length = end - n.start;
            } else if d.orig.len() > 1 && self.mods.alt {
                let scaled_end = end_anchor - ((end_anchor - end) as f64 * scale).round() as Tick;
                n.length = ((o.length as f64 * scale).round() as Tick).max(min);
                n.start = (scaled_end - n.length).max(0);
            } else {
                n.start = o.start + ds;
                n.length = (o.length - ds).max(1);
            }
            pv.changed.insert(*id, n);
        }
        let h = pv.changed[&d.orig[d.head].0];
        let (px, py) = self.pointer.unwrap_or((0.0, 0.0));
        pv.balloon = balloon_at(
            px,
            py,
            format!(
                "{} · {}",
                ctx.bbt(h.start),
                length_text(h.length, ctx.ppq())
            ),
        );
    }

    fn preview_draw(&self, ctx: &Ctx, v: &View, d: &Drag, (px, py): (f32, f32), pv: &mut Preview) {
        let Some((t0, p0, v0)) = d.draw else {
            return;
        };
        let shift = self.mods.shift;
        let step = ctx.step(self.beat_px);
        let t1 = ctx.snapped(self.beat_px, shift, v.tick(px));
        let grow = t0 >= ctx.clip_len();
        if v.drums() {
            self.preview_hits(ctx, v, d, (px, py), pv);
            return;
        }
        if self.mods.alt && d.moved {
            let p1 = i32::from(Self::clamp_pitch(v.pitch_at(py)));
            let n = (((t1 - t0).abs() as f64 / step as f64).round() as i64).max(1);
            let sign = if t1 >= t0 { 1 } else { -1 };
            for i in 0..=n {
                let start = t0 + sign * i * step;
                if start < 0 || (!grow && start >= ctx.clip_len()) {
                    continue;
                }
                let pitch = i32::from(p0)
                    + ((p1 - i32::from(p0)) as f64 * i as f64 / n as f64).round() as i32;
                pv.ghosts.push(NoteView {
                    start,
                    length: step,
                    pitch: Self::clamp_pitch(pitch),
                    velocity: v0,
                    muted: false,
                });
            }
            pv.balloon = balloon_at(px, py, format!("{} 個", n + 1));
            return;
        }
        let mut length = self.draw_length(ctx.prefs.text("edit.draw.length") == "last", step);
        let mut velocity = v0;
        if d.moved {
            length = (t1 - t0).max(step);
            let dv = (-(py - d.press.1) * VEL_PER_PX).round() as i32;
            velocity = (i32::from(v0) + dv).clamp(1, 127) as u8;
        }
        if !grow {
            length = length.min((ctx.clip_len() - t0).max(1));
        }
        pv.ghosts.push(NoteView {
            start: t0,
            length,
            pitch: p0,
            velocity,
            muted: false,
        });
        pv.balloon = balloon_at(
            px,
            py,
            format!(
                "{} · 長さ {} · ベロシティ {}",
                pitch_name(p0),
                length_text(length, ctx.ppq()),
                velocity
            ),
        );
    }

    /// The pencil in the drum view: a hit on the pressed row at every grid step the pointer is
    /// dragged across, each as long as the grid. Alt lays them along the line to the row the
    /// pointer is on (EDT-058).
    fn preview_hits(&self, ctx: &Ctx, v: &View, d: &Drag, (px, py): (f32, f32), pv: &mut Preview) {
        let Some((t0, p0, v0)) = d.draw else {
            return;
        };
        let step = ctx.step(self.beat_px);
        let t1 = ctx.snapped(self.beat_px, self.mods.shift, v.tick(px));
        let grow = t0 >= ctx.clip_len();
        let n = if d.moved {
            ((t1 - t0).abs() as f64 / step as f64).round() as i64
        } else {
            0
        };
        let sign = if t1 >= t0 { 1 } else { -1 };
        let line = self.mods.alt && d.moved;
        let (r0, r1) = (
            v.rows.rank(p0) as i64,
            v.rows.rank(Self::clamp_pitch(v.pitch_at(py))) as i64,
        );
        let mut velocity = v0;
        if d.moved && !line {
            let dv = (-(py - d.press.1) * VEL_PER_PX).round() as i32;
            velocity = (i32::from(v0) + dv).clamp(1, 127) as u8;
        }
        for i in 0..=n.max(if line { 1 } else { 0 }) {
            let start = t0 + sign * i * step;
            if start < 0 || (!grow && start >= ctx.clip_len()) {
                continue;
            }
            let pitch = if line {
                let rank = r0 + ((r1 - r0) as f64 * i as f64 / n.max(1) as f64).round() as i64;
                v.rows.pitch_of(rank as usize).unwrap_or(p0)
            } else {
                p0
            };
            pv.ghosts.push(NoteView {
                start,
                length: step,
                pitch,
                velocity,
                muted: false,
            });
        }
        let text = if pv.ghosts.len() > 1 {
            format!("{} 個", pv.ghosts.len())
        } else {
            format!("{} · ベロシティ {}", ctx.drum_name(p0), velocity)
        };
        pv.balloon = balloon_at(px, py, text);
    }

    // ----------------------------------------------------------- release

    pub fn up(&mut self, ctx: &Ctx, x: f32, y: f32, mods: Mods) -> Out {
        self.mods = self.mapped_mods(ctx, mods);
        self.pointer = Some((x, y));
        if self.drag.is_none() {
            return Out::quiet();
        }
        self.recompute(ctx);
        let Some(d) = self.drag.take() else {
            return Out::quiet();
        };
        let pv = std::mem::take(&mut self.preview);
        self.audition = None;
        self.listen = None;
        if d.moved && !d.drag_enabled {
            return Out::used();
        }
        let shift = self.mods.shift;
        let v = self.view(ctx);
        match d.mode {
            Mode::Part => self.part_up(ctx, &v, &d, x),
            Mode::Audition | Mode::Marquee => Out::used(),
            Mode::Zoom => {
                self.zoom_anchor = None;
                Out::used()
            }
            Mode::ClipEnd => match pv.clip_end {
                Some(length) if length != ctx.clip_len() => Out::edit(Edit::new(
                    "clip.resize",
                    json!({ "ids": [ctx.clip.id.to_string()], "length": length }),
                    Then::Keep,
                )),
                _ => Out::used(),
            },
            Mode::Loop(_) => match pv.loop_span {
                Some((start, end)) => Out::edit(Edit::new(
                    "loop.set",
                    json!({ "start": start, "end": end }),
                    Then::Keep,
                )),
                None => Out::used(),
            },
            Mode::Lane => self.lane_up(ctx, d, pv),
            Mode::Erase => {
                if d.marked.is_empty() {
                    return Out::used();
                }
                let ids: Vec<NoteId> = d.marked.iter().copied().collect();
                for id in &ids {
                    self.selected.remove(id);
                }
                Out::edit(Edit::new(
                    "note.delete",
                    json!({ "ids": ids_json(&ids) }),
                    Then::Keep,
                ))
            }
            Mode::Draw => {
                if pv.ghosts.is_empty() {
                    return Out::used();
                }
                if !v.drums() {
                    self.next_length = pv.ghosts[0].length;
                }
                if d.moved && !mods.alt {
                    self.next_vel = pv.ghosts[0].velocity;
                }
                Out::edit(self.put_edit(ctx, &pv.ghosts))
            }
            Mode::Move | Mode::ResizeL | Mode::ResizeR | Mode::Velocity => {
                if d.mode == Mode::Velocity
                    && let Some((id, _)) = d.orig.get(d.head)
                    && let Some(n) = pv.changed.get(id)
                {
                    self.next_vel = n.velocity;
                }
                if matches!(d.mode, Mode::ResizeL | Mode::ResizeR)
                    && d.moved
                    && let Some((id, _)) = d.orig.get(d.head)
                    && let Some(n) = pv.changed.get(id)
                {
                    self.next_length = n.length;
                }
                if !d.moved {
                    return self.click(ctx, &v, &d, x, shift);
                }
                if d.mode == Mode::Move
                    && let Some(spread) = pv.strum
                {
                    if spread == 0 {
                        return Out::used();
                    }
                    let ids: Vec<NoteId> = d.orig.iter().map(|(id, _)| *id).collect();
                    return Out::edit(Edit::new(
                        "note.strum",
                        json!({ "ids": ids_json(&ids), "spread": spread }),
                        Then::Keep,
                    ));
                }
                if d.mode == Mode::Move && !pv.ghosts.is_empty() {
                    let (o, g) = (d.orig[0].1, pv.ghosts[0]);
                    let (by, pitch_by) =
                        (g.start - o.start, i32::from(g.pitch) - i32::from(o.pitch));
                    if by == 0 && pitch_by == 0 {
                        return Out::used();
                    }
                    let ids: Vec<NoteId> = d.orig.iter().map(|(id, _)| *id).collect();
                    return Out::edit(Edit::new(
                        "note.duplicate",
                        json!({ "ids": ids_json(&ids), "by": by, "pitch_by": pitch_by }),
                        Then::Select,
                    ));
                }
                let mut out = match diff_set(&d.orig, &pv) {
                    Some(e) => Out::edit(e),
                    None => Out::used(),
                };
                if d.mode == Mode::Velocity {
                    out.next_velocity = Some(self.next_vel);
                }
                out
            }
        }
    }

    /// A release without a drag in between.
    fn click(&mut self, ctx: &Ctx, v: &View, d: &Drag, x: f32, shift: bool) -> Out {
        let Some(id) = d.clicked else {
            return Out::used();
        };
        if let Some(action) = d.click_action.as_deref() {
            match action {
                "" => return Out::used(),
                "select.none" => {
                    self.selected.clear();
                    return Out::used();
                }
                "select.set" => {
                    if !shift {
                        self.selected = BTreeSet::from([id]);
                    }
                    return Out::used();
                }
                "select.toggle" => {
                    if d.toggle_off {
                        self.selected.remove(&id);
                    }
                    return Out::used();
                }
                "note.mute" => {
                    return Out::edit(Edit::new(
                        "note.mute",
                        json!({"ids":ids_json(&[id])}),
                        Then::Keep,
                    ));
                }
                "note.split" | "note.split --deep" => {
                    let tick = self.cut_tick(ctx, shift, v.tick(x));
                    let ids = if action.ends_with("--deep") {
                        ctx.notes()
                            .iter()
                            .filter(|n| ctx.note(id).is_some_and(|hit| hit.pitch == n.pitch))
                            .map(|n| n.id)
                            .collect()
                    } else {
                        vec![id]
                    };
                    return self
                        .split_edit(ctx, ids, tick)
                        .map_or_else(Out::used, Out::edit);
                }
                _ => {}
            }
        }
        if d.toggle_off {
            self.selected.remove(&id);
            return Out::used();
        }
        if d.zone == Zone::Velocity && self.mods.alt {
            return Out::edit(Edit::new(
                "note.mute",
                json!({ "ids": ids_json(&[id]) }),
                Then::Keep,
            ));
        }
        if d.zone == Zone::Split && d.mode == Mode::Move {
            let rel = self.cut_tick(ctx, shift, v.tick(x));
            let ids: Vec<NoteId> = if self.mods.alt {
                let pitch = ctx.note(id).map(|n| n.pitch);
                ctx.notes()
                    .iter()
                    .filter(|n| Some(n.pitch) == pitch)
                    .map(|n| n.id)
                    .collect()
            } else {
                vec![id]
            };
            return match self.split_edit(ctx, ids, rel) {
                Some(e) => Out::edit(e),
                None => Out::used(),
            };
        }
        if !shift && self.selected.len() > 1 {
            self.selected.clear();
            self.selected.insert(id);
        }
        Out::used()
    }

    // ------------------------------------------------------------- wheel

    /// A wheel turn over the roll. `dy` is positive for a turn away from you.
    pub fn wheel(&mut self, ctx: &Ctx, x: f32, y: f32, dx: f32, dy: f32, mods: Mods) -> Out {
        let frame = self.frame(ctx.body);
        let g = frame.grid;
        let steps = dy / WHEEL_NOTCH;
        if mods.ctrl && mods.shift {
            // Windows sends the turn with Shift held sideways (EDT-136).
            let steps = (dy + dx) / WHEEL_NOTCH;
            let ax = x.clamp(g.x, g.right());
            let v = self.view(ctx);
            let anchor = v.tick_f(ax);
            self.beat_px = (self.beat_px * 1.15f32.powf(steps)).clamp(BEAT_PX_MIN, BEAT_PX_MAX);
            self.scroll.0 = anchor as f32 / v.ppq * self.beat_px - (ax - g.x);
        } else if mods.ctrl {
            let ay = y.clamp(g.y, g.bottom());
            let anchor = (ay - g.y + self.scroll.1) / self.row_h;
            self.row_h = (self.row_h + 2.0 * steps).clamp(ROW_H_MIN, ROW_H_MAX);
            self.scroll.1 = anchor * self.row_h - (ay - g.y);
        } else if mods.shift {
            self.scroll.0 -= dy + dx;
        } else {
            self.scroll.0 -= dx;
            self.scroll.1 -= dy;
        }
        self.clamp_scroll(ctx);
        Out::used()
    }
}

/// One `note.set` for every note the drag changed, naming only what differs.
pub(super) fn diff_set(orig: &[(NoteId, NoteView)], pv: &Preview) -> Option<Edit> {
    let mut list = Vec::new();
    for (id, o) in orig {
        let Some(n) = pv.changed.get(id) else {
            continue;
        };
        let mut m = Map::new();
        m.insert("id".into(), json!(id.to_string()));
        if n.start != o.start {
            m.insert("start".into(), json!(n.start));
        }
        if n.length != o.length {
            m.insert("length".into(), json!(n.length));
        }
        if n.pitch != o.pitch {
            m.insert("pitch".into(), json!(n.pitch));
        }
        if n.velocity != o.velocity {
            m.insert("velocity".into(), json!(n.velocity));
        }
        if m.len() > 1 {
            list.push(Value::Object(m));
        }
    }
    (!list.is_empty()).then(|| Edit::new("note.set", json!({ "notes": list }), Then::Keep))
}

impl Roll {
    fn mapped_mods(&self, ctx: &Ctx, mods: Mods) -> Mods {
        let Some(d) = &self.drag else {
            return mods;
        };
        let context = d.map_context.clone().unwrap_or_else(|| {
            format!(
                "roll.{}.{}.drag",
                ctx.tools.tool.name(),
                crate::mouse_dispatch::roll_zone(d.zone)
            )
        });
        let start = crate::mouse_dispatch::modifiers(&ctx.mousemap, &context, mods, false);
        crate::mouse_dispatch::during(&ctx.mousemap, &context, mods, start)
    }
    /// The notes of the clip that a key acts on: the selection, or all.
    pub fn target_ids(&self, ctx: &Ctx, all_if_none: bool) -> Vec<NoteId> {
        let mut sel: Vec<NoteId> = ctx
            .notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .map(|n| n.id)
            .collect();
        // The selected notes of the parts edited with the clip go with them (EDT-062).
        sel.extend(self.ids_elsewhere(ctx));
        if sel.is_empty() && all_if_none {
            ctx.notes().iter().map(|n| n.id).collect()
        } else {
            sel
        }
    }

    pub fn selected_notes<'a>(&self, ctx: &'a Ctx) -> Vec<&'a Note> {
        ctx.notes()
            .iter()
            .filter(|n| self.selected.contains(&n.id))
            .collect()
    }
}
