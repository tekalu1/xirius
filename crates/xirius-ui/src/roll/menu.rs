//! The right press in the roll. The menus themselves are the shared ones
//! (`context/roll.rs`); this only settles what the press selects and where it landed.

use xirius_core::id::NoteId;
use xirius_core::time::Tick;

use super::lanes::{LaneKind, bar_hit, nearest_point, value_at};
use super::{Ctx, Roll};

/// What a right press found in the roll.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    /// A note of the grid (the selected notes, once the press is settled).
    Note,
    /// Empty grid.
    Grid,
    /// A key of the piano, by pitch.
    Keys(u8),
    /// A velocity bar (the selected notes).
    Bar,
    /// A controller point (the selected points).
    Point,
    /// Empty part of a controller lane: the tick and value under the press.
    Lane { tick: Tick, value: i32 },
    /// The tab (or the name column) of lane `n`.
    Tab(usize),
}

impl Roll {
    /// A right press at a point of the roll. A note, a bar or a point that is not selected
    /// is selected alone first (CTX-007); the pointer is kept as the place a paste goes to.
    pub fn context_spot(&mut self, ctx: &Ctx, x: f32, y: f32) -> Option<Spot> {
        self.sync(ctx);
        let frame = self.frame(ctx.body);
        let v = self.view(ctx);
        self.cancel();
        if frame.grid.contains(x, y) {
            let hit = self.hit_note(ctx, &v, x, y).map(|(id, _)| id);
            // A note that is selected keeps the points selected with it (a mixed selection).
            if hit.is_none_or(|id| !self.selected.contains(&id)) {
                self.points.clear();
            }
            if let Some(id) = hit {
                self.select_alone(id);
            }
            self.pointer = Some((x, y));
            return Some(if hit.is_some() {
                Spot::Note
            } else {
                Spot::Grid
            });
        }
        if frame.keys.contains(x, y) {
            let pitch = u8::try_from(v.pitch_at(y)).ok().filter(|p| *p <= 127)?;
            return Some(Spot::Keys(pitch));
        }
        if let Some(i) = frame.tabs.iter().position(|r| r.contains(x, y)) {
            return Some(Spot::Tab(i));
        }
        let lb = *frame
            .lanes
            .iter()
            .find(|l| l.plot.contains(x, y) || l.head.contains(x, y))?;
        if lb.head.contains(x, y) {
            return Some(Spot::Tab(lb.index));
        }
        if lb.folded {
            return None;
        }
        if self.active_lane != lb.index {
            self.points.clear();
        }
        self.active_lane = lb.index;
        self.pointer = Some((x, y));
        let kind = self.lanes[lb.index].kind;
        match kind {
            LaneKind::Velocity => {
                self.points.clear();
                let id = bar_hit(ctx, &v, x)?;
                self.select_alone(id);
                Some(Spot::Bar)
            }
            LaneKind::Ctrl(ck) => {
                let pts = self.song_points(ctx, ck);
                match nearest_point(&pts, kind, &lb.plot, &v, x, y) {
                    Some(i) => {
                        let t = pts[i].tick;
                        if !self.points.contains(&t) {
                            // Not selected: the point is the whole selection.
                            self.points = [t].into();
                            self.selected.clear();
                        }
                        Some(Spot::Point)
                    }
                    None => {
                        self.points.clear();
                        let tick = self.lane_tick(ctx, &v, x);
                        let value = kind.conform(value_at(kind, &lb.plot, y));
                        Some(Spot::Lane { tick, value })
                    }
                }
            }
        }
    }

    /// Whether notes and controller points are selected together (CTX-007).
    pub fn mixed(&self, ctx: &Ctx) -> bool {
        !self.points.is_empty() && !self.selected_notes(ctx).is_empty()
    }

    /// The note is the whole selection, unless it is selected already.
    fn select_alone(&mut self, id: NoteId) {
        if !self.selected.contains(&id) {
            self.selected.clear();
            self.selected.insert(id);
        }
    }

    /// Selects the notes of one pitch.
    pub fn select_pitch(&mut self, ctx: &Ctx, pitch: u8) {
        self.points.clear();
        self.selected = ctx
            .notes()
            .iter()
            .filter(|n| n.pitch == pitch)
            .map(|n| n.id)
            .collect();
    }

    /// Shows another controller in lane `index`. False when it is shown already.
    pub fn set_lane_kind(&mut self, index: usize, kind: LaneKind) -> bool {
        if self.lanes.iter().any(|l| l.kind == kind) || index >= self.lanes.len() {
            return false;
        }
        if let Some(old) = self.lanes[index].kind.ctrl() {
            self.hidden.insert(old);
        }
        if let Some(new) = kind.ctrl() {
            self.hidden.remove(&new);
        }
        self.lanes[index].kind = kind;
        self.active_lane = index;
        self.points.clear();
        true
    }

    /// Hides lane `index` (never the velocity lane, never the last one).
    pub fn hide_lane(&mut self, index: usize) -> bool {
        let Some(lane) = self.lanes.get(index) else {
            return false;
        };
        if lane.kind == LaneKind::Velocity || self.lanes.len() < 2 {
            return false;
        }
        if let Some(k) = lane.kind.ctrl() {
            self.hidden.insert(k);
        }
        self.lanes.remove(index);
        self.active_lane = self.active_lane.min(self.lanes.len() - 1);
        self.points.clear();
        true
    }
}
