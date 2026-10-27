//! The recording area under a track (REC-002, REC-005, REC-006, REC-014, REC-015).
//!
//! An armed audio track shows the level of its input and its input gain, as the mock's track head
//! does. Clips that lie over one another are the takes of one passage: the clip that plays
//! carries their count, and pressing it opens below the track a Comp row that shows which take
//! plays where, and a row for each take (they start folded, UI-296). Pressing a take's row makes that take
//! play between the dividers (where the Comp row changes take) around the press; dragging along
//! it makes the take play over the stretch dragged, snapped to the grid (`take.comp`, one step
//! of the history either way). The stretch chosen stays selected: the up and down arrows switch
//! its take, left and right move it to the next stretch between dividers.
//!
//! The rows hang under the track's automation lanes ([`Row::rec_h`]). This module is their
//! geometry, what is under the pointer, and their painting.

use serde_json::json;
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::{Project, Track, TrackKind};
use xirius_core::takes::{self, TakeSlot, TakeStack};
use xirius_core::time::Tick;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use crate::arrange::{Drag, Plan, Preview, Spot};
use crate::draw::{Align, Pen, inset, track_color};
use crate::inmeter::{Fit, fit, zone};
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Mods;
use crate::tree::{Row, RowKind};

/// The input row of an armed audio track: its level and its gain.
pub const INPUT_H: f32 = 40.0;
/// The Comp row, the takes that play.
pub const COMP_H: f32 = 28.0;
/// One take.
pub const TAKE_H: f32 = 24.0;
/// One take when there are many: the rows shrink so that all of them show.
pub const MIN_TAKE_H: f32 = 14.0;
/// The height the take rows share before they begin to shrink.
const TAKES_H: f32 = 96.0;
/// The range of the input gain, in dB (`track.input.gain`).
pub const GAIN_RANGE: (f64, f64) = (-24.0, 24.0);

const METER_Y: f32 = 8.0;
const METER_H: f32 = 10.0;
const GAIN_Y: f32 = 24.0;
const GAIN_H: f32 = 10.0;
const THUMB: f32 = 10.0;
/// A take's bar is narrower than this: it is not worth a press.
const MIN_PIECE: Tick = 30;

/// The height of a take row when a stack has `rows` of them: all of them fit in [`TAKES_H`] down
/// to [`MIN_TAKE_H`] each.
pub fn take_h(rows: usize) -> f32 {
    (TAKES_H / rows.max(1) as f32).clamp(MIN_TAKE_H, TAKE_H)
}

/// The stretch of a take stack the last press or drag chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompSel {
    pub track: TrackId,
    pub from: Tick,
    pub to: Tick,
}

/// What is under the pointer in the recording area.
#[derive(Clone, Debug, PartialEq)]
pub enum RecPart {
    /// The input gain slider, the dB under the pointer.
    Gain(f64),
    /// The count of takes on a clip, or the Comp row's head: opens the Comp and take rows and
    /// folds them away again (UI-296).
    Fold,
    /// A take's row: pressing it plays the take over `from..to` (between the dividers), and a
    /// drag along it over the stretch dragged.
    Bar { take: ClipId, from: Tick, to: Tick },
}

/// What a track shows under its lanes.
#[derive(Clone, Debug)]
pub struct RecArea {
    /// The level and gain of the armed input.
    pub input: bool,
    pub stacks: Vec<TakeStack>,
    /// The Comp and take rows show; folded, nothing of the takes does (UI-296).
    pub open: bool,
}

impl RecArea {
    pub fn of(track: &Track, open: bool) -> RecArea {
        RecArea {
            input: track.kind == TrackKind::Audio && track.armed,
            stacks: takes::stacks(track),
            open,
        }
    }

    /// How many take rows the tallest stack needs.
    pub fn rows(&self) -> usize {
        self.stacks.iter().map(|s| s.rows).max().unwrap_or(0)
    }

    /// There are takes and their rows are open.
    pub fn has_takes(&self) -> bool {
        self.open && !self.stacks.is_empty()
    }

    pub fn height(&self) -> f32 {
        let mut h = 0.0;
        if self.input {
            h += INPUT_H;
        }
        if self.has_takes() {
            h += COMP_H + self.rows() as f32 * self.take_h();
        }
        h
    }

    /// Top of the input row, from the top of the area.
    pub fn input_y(&self) -> Option<f32> {
        self.input.then_some(0.0)
    }

    /// Top of the Comp row.
    pub fn comp_y(&self) -> Option<f32> {
        self.has_takes()
            .then_some(if self.input { INPUT_H } else { 0.0 })
    }

    /// The height of one take row.
    pub fn take_h(&self) -> f32 {
        take_h(self.rows())
    }

    /// Top of take row `j`.
    pub fn take_y(&self, j: usize) -> f32 {
        self.comp_y().unwrap_or(0.0) + COMP_H + j as f32 * self.take_h()
    }

    /// The take rows the area shows.
    pub fn shown_rows(&self) -> usize {
        if self.has_takes() { self.rows() } else { 0 }
    }
}

/// The heights of the areas of the tracks that have one.
pub fn heights(studio: &Studio, p: &Project) -> std::collections::HashMap<TrackId, f32> {
    p.tracks
        .values()
        .filter_map(|t| {
            let h = RecArea::of(t, studio.takes_open.contains(&t.id)).height();
            (h > 0.0).then_some((t.id, h))
        })
        .collect()
}

fn gain_rect(row: &Row) -> Rect {
    let x = row.content_x();
    Rect::new(
        x,
        GAIN_Y,
        (row.head_w - Space::X2.px() - x).max(0.0),
        GAIN_H,
    )
}

fn meter_rect(row: &Row) -> Rect {
    let x = row.content_x();
    Rect::new(
        x,
        METER_Y,
        (row.head_w - Space::X2.px() - x).max(0.0),
        METER_H,
    )
}

/// The gain at `x` of the slider (relative to the column's left edge).
fn gain_at(row: &Row, x: f32) -> f64 {
    let r = gain_rect(row);
    let t = ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0) as f64;
    let db = GAIN_RANGE.0 + t * (GAIN_RANGE.1 - GAIN_RANGE.0);
    // Whole decibels, and an exact 0 dB in the middle.
    db.round().clamp(GAIN_RANGE.0, GAIN_RANGE.1)
}

fn gain_x(row: &Row, db: f64) -> f32 {
    let r = gain_rect(row);
    let t = ((db - GAIN_RANGE.0) / (GAIN_RANGE.1 - GAIN_RANGE.0)).clamp(0.0, 1.0) as f32;
    r.x + t * r.w
}

/// The Comp row's head, where pressing folds the takes away.
fn fold_rect(row: &Row, area: &RecArea) -> Option<Rect> {
    area.comp_y().map(|y| Rect::new(0.0, y, row.head_w, COMP_H))
}

/// Where the Comp row of a stack changes take: its ends, and each place the take that plays
/// changes (or stops). Two pieces of one take in a row have no divider between them.
pub fn dividers(stack: &TakeStack) -> Vec<Tick> {
    let mut played: Vec<&TakeSlot> = stack.slots.iter().filter(|x| x.plays).collect();
    played.sort_by_key(|x| (x.start, x.row));
    let mut at = vec![stack.start, stack.end];
    for w in played.windows(2) {
        if w[0].row != w[1].row || w[0].end != w[1].start {
            at.push(w[0].end);
            at.push(w[1].start);
        }
    }
    at.sort_unstable();
    at.dedup();
    at
}

/// The stretch between the dividers around `tick`, cut to `slot`.
fn piece(stack: &TakeStack, slot: &TakeSlot, tick: Tick) -> Option<(Tick, Tick)> {
    let at = dividers(stack);
    let from = at.iter().rev().find(|d| **d <= tick).copied()?;
    let to = at.iter().find(|d| **d > tick).copied()?;
    let (from, to) = (from.max(slot.start), to.min(slot.end));
    (to - from >= MIN_PIECE).then_some((from, to))
}

/// The stack of `track` that holds `tick`.
fn stack_at(track: &Track, tick: Tick) -> Option<TakeStack> {
    takes::stacks(track)
        .into_iter()
        .find(|s| s.start <= tick && tick < s.end)
}

impl Studio {
    /// What is at (x, y) of the recording area of `row`, as an arrangement spot.
    pub(crate) fn rec_spot(
        &self,
        g: &Geometry,
        row: &Row,
        x: f32,
        y: f32,
        in_column: bool,
    ) -> Spot {
        let RowKind::Track(t) = row.kind else {
            return Spot::None;
        };
        let p = self.project();
        let Some(track) = p.tracks.get(&t) else {
            return Spot::None;
        };
        let area = RecArea::of(track, self.takes_open.contains(&t));
        let top = self.row_y(g, row) + row.h + row.lanes as f32 * crate::tree::LANE_H;
        let ry = y - top;
        let rec = |part: RecPart| Spot::Rec { track: t, part };
        if in_column {
            let lx = x - g.column.x;
            if let Some(iy) = area.input_y()
                && ry >= iy
                && ry < iy + INPUT_H
            {
                let r = gain_rect(row);
                // The slider takes the row a little above and below, to be easy to hit.
                if ry >= GAIN_Y - 4.0 && lx >= r.x - 4.0 && lx <= r.right() + 4.0 {
                    return rec(RecPart::Gain(gain_at(row, lx)));
                }
                return Spot::None;
            }
            if fold_rect(row, &area).is_some_and(|r| r.contains(lx, ry)) {
                return rec(RecPart::Fold);
            }
            return Spot::None;
        }
        let tick = self.tick_at(g, x);
        for j in 0..area.shown_rows() {
            let y0 = area.take_y(j);
            if ry < y0 || ry >= y0 + area.take_h() {
                continue;
            }
            for s in &area.stacks {
                if let Some(slot) = s.at(j, tick)
                    && let Some((from, to)) = piece(s, slot, tick)
                {
                    return rec(RecPart::Bar {
                        take: slot.clip,
                        from,
                        to,
                    });
                }
            }
        }
        Spot::None
    }

    /// A press on the recording area. True when it was used.
    pub(crate) fn rec_down(&mut self, track: TrackId, part: RecPart) -> bool {
        match part {
            RecPart::Gain(db) => {
                let same = self
                    .project()
                    .tracks
                    .get(&track)
                    .is_some_and(|t| t.input_gain_db == db);
                if !same {
                    // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                    self.arrange_run(
                        "track.input.gain",
                        json!({ "ids": [track.to_string()], "db": db }),
                    );
                }
            }
            RecPart::Fold => {
                if !self.takes_open.remove(&track) {
                    self.takes_open.insert(track);
                } else if self.comp_sel.is_some_and(|c| c.track == track) {
                    // The stretch chosen went with its rows.
                    self.comp_sel = None;
                }
                self.after_edit();
            }
            RecPart::Bar { take, from, to } => self.comp_run(track, take, from, to),
        }
        true
    }

    /// `take` plays over `from..to`, which stays chosen for the arrow keys.
    /// A stretch the take already plays alone is only chosen: nothing is cut and nothing goes
    /// on the history.
    fn comp_run(&mut self, track: TrackId, take: ClipId, from: Tick, to: Tick) {
        let plays = self
            .project()
            .tracks
            .get(&track)
            .is_some_and(|t| takes::already_plays(t, take, from, to));
        let done = plays
            || self
                // design-lint: allow(ui-blocking-io) PRJ-031 review: guarded UI dispatcher; Session I/O is deferred and macros await the actual owner before continuing.
                .arrange_run(
                    "take.comp",
                    json!({ "take": take.to_string(), "from": from, "to": to }),
                )
                .is_some();
        if done {
            self.comp_sel = Some(CompSel { track, from, to });
            self.arrange.touched();
        }
    }

    /// What letting go of a drag along a take's row would do: that take plays over the stretch
    /// dragged, snapped and kept within the take's row.
    pub(crate) fn plan_comp(
        &self,
        g: &Geometry,
        d: &Drag,
        track: TrackId,
        take: ClipId,
    ) -> Option<Plan> {
        let p = self.project();
        let tr = p.tracks.get(&track)?;
        let stack = takes::stack_of(tr, take)?;
        let row = stack.slots.iter().find(|x| x.clip == take)?.row;
        let (lo, hi) = stack.row(row).fold((Tick::MAX, Tick::MIN), |(a, b), s| {
            (a.min(s.start), b.max(s.end))
        });
        let shift = d.mods.shift;
        let a = self.snap_tick(d.tick, shift);
        let b = self.snap_tick(self.tick_at(g, d.at.0).max(0), shift);
        let (from, to) = (a.min(b).max(lo), a.max(b).min(hi));
        if to <= from {
            return None;
        }
        let area = RecArea::of(tr, self.takes_open.contains(&track));
        let r = self.tree().row_of(RowKind::Track(track))?;
        let top = self.row_y(g, r) + r.h + r.lanes as f32 * crate::tree::LANE_H + area.take_y(row);
        let map = p.time_map();
        // Nothing to cut where the take already plays alone.
        let command = (!takes::already_plays(tr, take, from, to)).then(|| {
            (
                "take.comp",
                json!({ "take": take.to_string(), "from": from, "to": to }),
            )
        });
        Some(Plan {
            command,
            select: None,
            tip: format!("{} → {}", map.format(from), map.format(to)),
            preview: Preview::Range {
                start: from,
                end: to,
                top,
                bottom: top + area.take_h(),
            },
        })
    }

    /// A drag along a take's row is let go: its stretch is the one the arrow keys work on.
    pub(crate) fn comp_followed(&mut self, spot: &Spot, preview: &Preview) {
        if let Spot::Rec {
            track,
            part: RecPart::Bar { .. },
        } = spot
            && let Preview::Range { start, end, .. } = preview
        {
            self.comp_sel = Some(CompSel {
                track: *track,
                from: *start,
                to: *end,
            });
        }
    }

    /// The arrow keys on the stretch chosen: up and down switch its take; left and right move it
    /// to the next stretch between dividers, and with Shift widen it to that divider.
    /// True when the key was used.
    pub(crate) fn comp_key(&mut self, key: &str, mods: Mods) -> bool {
        let Some(sel) = self.comp_sel else {
            return false;
        };
        if key == "escape" {
            self.comp_sel = None;
            self.arrange.touched();
            return false;
        }
        let vertical = matches!(key, "up" | "down") && !mods.shift;
        let horizontal = matches!(key, "left" | "right");
        if !(vertical || horizontal)
            || mods.ctrl
            || mods.alt
            || self.settings.is_some()
            || self.palette.is_some()
            || self.popup.is_some()
        {
            return false;
        }
        let stack = {
            let p = self.project();
            p.tracks.get(&sel.track).and_then(|t| stack_at(t, sel.from))
        };
        let Some(stack) = stack else {
            self.comp_sel = None;
            return false;
        };
        if vertical {
            let Some(cur) = stack.playing_at(sel.from).map(|s| s.row).min() else {
                return true;
            };
            let row = if key == "up" {
                cur.checked_sub(1)
            } else {
                (cur + 1 < stack.rows).then_some(cur + 1)
            };
            if let Some(slot) = row.and_then(|r| stack.at(r, sel.from)) {
                let (from, to) = (sel.from.max(slot.start), sel.to.min(slot.end));
                if to > from {
                    self.comp_run(sel.track, slot.clip, from, to);
                }
            }
            return true;
        }
        let at = dividers(&stack);
        let next = |after: Tick| at.iter().find(|d| **d > after).copied();
        let prev = |before: Tick| at.iter().rev().find(|d| **d < before).copied();
        let (from, to) = match (key == "right", mods.shift) {
            (true, false) => next(sel.to).map(|b| (sel.to, b)),
            (false, false) => prev(sel.from).map(|a| (a, sel.from)),
            (true, true) => next(sel.to).map(|b| (sel.from, b)),
            (false, true) => prev(sel.from).map(|a| (a, sel.to)),
        }
        .unwrap_or((sel.from, sel.to));
        self.comp_sel = Some(CompSel {
            track: sel.track,
            from,
            to,
        });
        self.arrange.touched();
        true
    }
}

// ---------------------------------------------------------------- painting

/// The recording area's part of the track column: the input level and gain, and the labels of
/// the Comp and take rows.
pub fn paint_head(
    pen: &mut Pen,
    studio: &Studio,
    project: &Project,
    g: &Geometry,
    row: &Row,
    top: f32,
) {
    let RowKind::Track(t) = row.kind else {
        return;
    };
    let Some(track) = project.tracks.get(&t) else {
        return;
    };
    if row.rec_h <= 0.0 {
        return;
    }
    let c = pen.c;
    let area = RecArea::of(track, studio.takes_open.contains(&t));
    let base = top + row.h + row.lanes as f32 * crate::tree::LANE_H;
    let at = |r: Rect, y: f32| Rect::new(g.column.x + r.x, y + r.y, r.w, r.h);
    if let Some(iy) = area.input_y() {
        let y = base + iy;
        let m = studio.inputs.of(track, studio.transport.input_channels());
        let mut meter = at(meter_rect(row), y);
        // Recording with the input outside the frame: it is said beside the meter.
        if studio.input_outside_frame(t) {
            let w = (meter.w * 0.4).min(84.0);
            meter.w -= w + Space::X1.px();
            pen.text(
                "入力が枠の外",
                FontSize::Micro,
                Weight::Medium,
                c.ink_alert,
                Rect::new(meter.right() + Space::X1.px(), meter.y, w, meter.h),
                Align::Right,
            );
        }
        paint_meter(pen, meter, &m);
        // The gain slider: a groove and a thumb at the dB.
        let r = at(gain_rect(row), y);
        let groove = Rect::new(r.x, r.y + (r.h - 4.0) / 2.0, r.w, 4.0);
        pen.round(groove, c.well, Radius::Pill);
        let mid = gain_x(row, 0.0) + g.column.x;
        let x = gain_x(row, track.input_gain_db) + g.column.x;
        let (a, b) = (mid.min(x), mid.max(x));
        if b - a > 0.5 {
            pen.round(Rect::new(a, groove.y, b - a, 4.0), c.ink_blue, Radius::Pill);
        }
        let hot = matches!(
            studio.arrange.hover,
            Spot::Rec { track: h, part: RecPart::Gain(_) } if h == t
        );
        pen.round(
            Rect::new(x - THUMB / 2.0, r.y + (r.h - THUMB) / 2.0, THUMB, THUMB),
            if hot { c.ink_strong } else { c.thumb },
            Radius::Pill,
        );
    }
    if let Some(cy) = area.comp_y() {
        let y = base + cy;
        let x = g.column.x + row.content_x();
        let hot = matches!(
            studio.arrange.hover,
            Spot::Rec { track: h, part: RecPart::Fold } if h == t
        );
        let s = Layout::ICON_SIZE;
        // Open: the chevron points down and folds the rows away.
        pen.icon(
            "chevron-down",
            Rect::new(x, y + (COMP_H - s) / 2.0, s, s),
            if hot { c.ink_strong } else { c.ink },
        );
        pen.text(
            "Comp",
            FontSize::Micro,
            Weight::Medium,
            c.ink_strong,
            Rect::new(
                x + s + Space::X1.px(),
                y,
                (g.column.x + row.head_w - x - s).max(0.0),
                COMP_H,
            ),
            Align::Left,
        );
        for j in 0..area.shown_rows() {
            let ty = base + area.take_y(j);
            pen.text(
                &format!("T{}", j + 1),
                FontSize::Micro,
                Weight::Medium,
                c.ink,
                Rect::new(x + s + Space::X1.px(), ty, 40.0, area.take_h()),
                Align::Left,
            );
        }
    }
}

/// The level of two channels as two bars, with the zone a good level lies in drawn behind them.
fn paint_meter(pen: &mut Pen, r: Rect, m: &crate::mixer::meters::Meter) {
    let c = pen.c;
    pen.round(r, c.well, Radius::Clip);
    let (lo, hi) = zone();
    let zr = Rect::new(r.x + lo * r.w, r.y, (hi - lo) * r.w, r.h);
    // The zone is a frame around the bars: lit once the held peak is inside.
    let frame = match fit(m) {
        Fit::Good => c.ok,
        Fit::High => c.hot,
        _ => c.line[0],
    };
    pen.round(zr, frame, Radius::Xs);
    pen.round(inset(zr, 1.0), c.well, Radius::Xs);
    let bar_h = (r.h - 2.0) / 2.0 - 0.5;
    for ch in 0..2 {
        let y = r.y + 1.0 + ch as f32 * (bar_h + 1.0);
        let level = m.level[ch].clamp(0.0, 1.0);
        if level > 0.0 {
            let color = if level > hi { c.hot } else { c.ok };
            pen.round(
                Rect::new(r.x, y, (level * r.w).max(2.0), bar_h),
                color,
                Radius::Xs,
            );
        }
        let held = m.hold[ch].clamp(0.0, 1.0);
        if held > 0.0 {
            pen.round(
                Rect::new((r.x + held * r.w - 2.0).max(r.x), y, 2.0, bar_h),
                c.ink_strong,
                Radius::Xs,
            );
        }
    }
}

/// The lanes side of the recording area: the Comp row and the takes, a bar of each.
pub fn paint_lanes(
    pen: &mut Pen,
    studio: &Studio,
    project: &Project,
    g: &Geometry,
    row: &Row,
    track: &Track,
    top: f32,
) {
    if row.rec_h <= 0.0 {
        return;
    }
    let c = pen.c;
    let area = RecArea::of(track, studio.takes_open.contains(&track.id));
    let base = top + row.h + row.lanes as f32 * crate::tree::LANE_H;
    let color = c.tracks[track_color(track) % c.tracks.len()];
    if let Some(iy) = area.input_y() {
        crate::record_loss::paint(pen, studio, project, g, row, track.id, base + iy);
    }
    let map = project.time_map();
    let hover = &studio.arrange.hover;
    let bar_rect = |s: &TakeSlot, y: f32, h: f32| {
        let x0 = studio.tick_x(g, s.start);
        let x1 = studio.tick_x(g, s.end);
        Rect::new(x0, y + 2.0, (x1 - x0).max(1.0), h - 4.0)
    };
    // The Comp row: every take that plays, where it plays.
    if let Some(cy) = area.comp_y() {
        let y = base + cy;
        for s in &area.stacks {
            for slot in s.slots.iter().filter(|x| x.plays) {
                let r = bar_rect(slot, y, COMP_H);
                if r.right() < g.lanes.x || r.x > g.lanes.right() {
                    continue;
                }
                pen.round(r, color, Radius::Clip);
                pen.mask(r, |pen| {
                    pen.text(
                        &format!("T{}", slot.row + 1),
                        FontSize::Micro,
                        Weight::Medium,
                        pen.c.ink_on_track,
                        Rect::new(r.x + Space::X1.px(), r.y, r.w, r.h),
                        Align::Left,
                    );
                });
            }
        }
    }
    let take_h = area.take_h();
    for j in 0..area.shown_rows() {
        let y = base + area.take_y(j);
        for s in &area.stacks {
            for slot in s.row(j) {
                let r = bar_rect(slot, y, take_h);
                if r.right() < g.lanes.x || r.x > g.lanes.right() {
                    continue;
                }
                // The part that plays is outlined in the track's color, with its waveform in the
                // color; the parts that do not play have a grey one.
                let face = if slot.plays {
                    pen.round(r, color, Radius::Clip);
                    let face = inset(r, 2.0);
                    pen.round(face, c.clip_sel, Radius::Clip);
                    face
                } else {
                    pen.round(r, c.clip, Radius::Clip);
                    r
                };
                if let Ok((_, clip)) = project.clip(slot.clip) {
                    let ink = if slot.plays {
                        color
                    } else {
                        c.ink.opacity(0.4)
                    };
                    pen.mask(face, |pen| {
                        crate::draw::paint_clip_body(pen, studio, clip, r, inset(face, 1.0), ink);
                    });
                }
                // The bar under the pointer, which a press would pick.
                if let Spot::Rec {
                    track: h,
                    part: RecPart::Bar { take, from, to },
                } = hover
                    && *h == track.id
                    && s.slots.iter().any(|x| x.clip == *take && x.row == j)
                {
                    let (a, b) = (studio.tick_x(g, *from), studio.tick_x(g, *to));
                    pen.round(
                        Rect::new(a, r.y, (b - a).max(1.0), r.h),
                        c.fill.opacity(0.12),
                        Radius::Clip,
                    );
                }
            }
        }
        // The bar lines, so a bar to press can be seen.
        for s in &area.stacks {
            let (b0, _, _) = map.to_bbt(s.start);
            let (b1, _, _) = map.to_bbt(s.end - 1);
            for bar in b0 + 1..=b1 {
                let x = studio.tick_x(g, map.bar_start(bar));
                if x < g.lanes.x || x > g.lanes.right() {
                    continue;
                }
                pen.fill(Rect::new(x, y + 2.0, 1.0, take_h - 4.0), c.surface_2);
            }
        }
    }
    // The stretch the arrow keys work on: a band down the Comp row and the takes.
    if let Some(sel) = studio.comp_sel
        && sel.track == track.id
        && let Some(cy) = area.comp_y()
    {
        let (a, b) = (studio.tick_x(g, sel.from), studio.tick_x(g, sel.to));
        let top = base + cy;
        let bottom = base + area.take_y(area.shown_rows());
        let band = Rect::new(a, top, (b - a).max(1.0), bottom - top);
        pen.mask(g.lanes, |pen| {
            // Seen through: the take rows under the band stay readable.
            pen.round(band, c.fill.opacity(0.16), Radius::Clip);
            pen.fill(Rect::new(a, top, 1.0, band.h), c.ink_strong);
            pen.fill(Rect::new(b - 1.0, top, 1.0, band.h), c.ink_strong);
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::id::TrackId;

    fn row() -> Row {
        let mut p = Project::new("t");
        let id = TrackId(1);
        p.tracks.insert(
            id,
            std::sync::Arc::new(Track::new(id, TrackKind::Audio, "A")),
        );
        p.root.push(xirius_core::id::Node::Track(id));
        *crate::tree::Tree::build(&p, &Default::default())
            .rows
            .first()
            .unwrap()
    }

    #[test]
    fn the_gain_slider_maps_its_ends_and_middle_to_the_range() {
        let r = row();
        let g = gain_rect(&r);
        assert_eq!(gain_at(&r, g.x - 10.0), GAIN_RANGE.0);
        assert_eq!(gain_at(&r, g.right() + 10.0), GAIN_RANGE.1);
        assert_eq!(gain_at(&r, g.x + g.w / 2.0), 0.0);
        let x = gain_x(&r, 6.0);
        assert_eq!(gain_at(&r, x), 6.0);
    }

    #[test]
    fn a_stack_is_a_comp_row_and_a_row_for_each_take() {
        let area = RecArea {
            input: true,
            stacks: vec![TakeStack {
                start: 0,
                end: 100,
                rows: 3,
                slots: Vec::new(),
            }],
            open: true,
        };
        assert_eq!(area.height(), INPUT_H + COMP_H + 3.0 * TAKE_H);
        assert_eq!(area.comp_y(), Some(INPUT_H));
        assert_eq!(area.take_y(1), INPUT_H + COMP_H + TAKE_H);
        // Folded, nothing of the takes shows: no Comp row either (UI-296).
        let folded = RecArea {
            open: false,
            ..area
        };
        assert_eq!(folded.height(), INPUT_H);
        assert_eq!(folded.comp_y(), None);
        assert_eq!(folded.shown_rows(), 0);
    }
}
