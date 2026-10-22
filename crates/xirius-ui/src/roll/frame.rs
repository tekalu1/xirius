//! Where the parts of the roll sit in the drawer, and the map between ticks /
//! pitches and pixels.

use xirius_core::time::Tick;
use xirius_design::{Layout, Space};

use super::{Ctx, Roll};
use crate::studio::Rect;

pub const STRIP_H: f32 = 22.0;
pub const LANE_H: f32 = 40.0;
pub const LANE_H_MIN: f32 = 24.0;
pub const LANE_FOLDED_H: f32 = 16.0;
pub const GRID_H_MIN: f32 = 84.0;
/// Room above and below a controller's plot, so a point at the end stays whole.
pub const LANE_PAD: f32 = 5.0;
pub const MENU_ITEM_H: f32 = 22.0;
pub const MENU_W: f32 = 168.0;
/// The zoom slider's length.
pub const ZOOM_W: f32 = 120.0;

/// The pitches the grid has a row for, from the top: every pitch of the keyboard, the highest on
/// top, or the rows of a drum map in the order they were put in (EDT-006). Kept in place, so the
/// map is cheap to copy around.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rows {
    keys: bool,
    len: u8,
    /// The pitches from the top, `len` of them.
    order: [u8; 128],
}

impl Rows {
    pub const KEYS: Rows = Rows {
        keys: true,
        len: 128,
        order: [0; 128],
    };

    /// The pitches, the highest on top.
    pub fn of(pitches: impl IntoIterator<Item = u8>) -> Rows {
        let mut all: Vec<u8> = pitches.into_iter().collect();
        all.sort_unstable_by(|a, b| b.cmp(a));
        Rows::ordered(all)
    }

    /// The pitches in the order given, the first on top; a pitch given twice keeps its first
    /// place.
    pub fn ordered(pitches: impl IntoIterator<Item = u8>) -> Rows {
        let mut rows = Rows {
            keys: false,
            len: 0,
            order: [0; 128],
        };
        for p in pitches {
            if p <= 127 && !rows.has(p) {
                rows.order[usize::from(rows.len)] = p;
                rows.len += 1;
            }
        }
        rows
    }

    pub fn is_keys(self) -> bool {
        self.keys
    }

    pub fn len(self) -> usize {
        usize::from(self.len)
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    fn shown(&self) -> &[u8] {
        &self.order[..self.len()]
    }

    pub fn has(self, pitch: u8) -> bool {
        pitch <= 127 && (self.keys || self.shown().contains(&pitch))
    }

    /// How many rows lie above the pitch's; for a pitch without a row, how many have a higher
    /// pitch.
    pub fn rank(self, pitch: u8) -> usize {
        if self.keys {
            return usize::from(127u8.saturating_sub(pitch));
        }
        match self.shown().iter().position(|p| *p == pitch) {
            Some(i) => i,
            None => self.shown().iter().filter(|p| **p > pitch).count(),
        }
    }

    /// The pitch of the `rank`th row from the top.
    pub fn pitch_of(self, rank: usize) -> Option<u8> {
        if self.keys {
            return u8::try_from(rank).ok().and_then(|r| 127u8.checked_sub(r));
        }
        self.shown().get(rank).copied()
    }

    /// The pitches from the top down.
    pub fn pitches(self) -> impl Iterator<Item = u8> {
        (0..self.len()).filter_map(move |i| self.pitch_of(i))
    }

    pub fn top(self) -> u8 {
        self.pitch_of(0).unwrap_or(127)
    }
}

/// The time / pitch ↔ pixel map of the note grid.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub grid: Rect,
    pub ppq: f32,
    pub beat_px: f32,
    pub row_h: f32,
    pub scroll: (f32, f32),
    pub rows: Rows,
}

impl View {
    /// Window x of a tick counted from the clip's head.
    pub fn x(&self, tick: Tick) -> f32 {
        self.grid.x + tick as f32 / self.ppq * self.beat_px - self.scroll.0
    }

    /// The tick (from the clip's head) at window x, not rounded.
    pub fn tick_f(&self, x: f32) -> f64 {
        f64::from((x - self.grid.x + self.scroll.0) / self.beat_px) * f64::from(self.ppq)
    }

    pub fn tick(&self, x: f32) -> Tick {
        self.tick_f(x).round() as Tick
    }

    /// Window y of the top of a pitch's row.
    pub fn y(&self, pitch: u8) -> f32 {
        self.grid.y + self.rows.rank(pitch) as f32 * self.row_h - self.scroll.1
    }

    /// Whether the grid shows a drum map instead of the keyboard.
    pub fn drums(&self) -> bool {
        !self.rows.is_keys()
    }

    /// The row whose band holds window y, counted from the top; it may lie off the grid.
    pub fn row_at(&self, y: f32) -> i32 {
        ((y - self.grid.y + self.scroll.1) / self.row_h).floor() as i32
    }

    /// The pitch whose row holds window y. On the keyboard it may lie off the keyboard; with a
    /// drum map it is the nearest row's.
    pub fn pitch_at(&self, y: f32) -> i32 {
        let row = self.row_at(y);
        if !self.drums() {
            return 127 - row;
        }
        let last = self.rows.len().saturating_sub(1) as i32;
        let row = row.clamp(0, last) as usize;
        self.rows.pitch_of(row).map_or(0, i32::from)
    }

    /// The pitch `rows` rows below `pitch`'s (above when negative), kept on the grid.
    pub fn pitch_by(&self, pitch: u8, rows: i32) -> u8 {
        let last = self.rows.len().saturating_sub(1) as i32;
        let at = (self.rows.rank(pitch) as i32 + rows).clamp(0, last) as usize;
        self.rows.pitch_of(at).unwrap_or(pitch)
    }

    /// World x (from the head, ignoring the scroll) and world y (from pitch 127).
    pub fn world(&self, x: f32, y: f32) -> (f32, f32) {
        (
            x - self.grid.x + self.scroll.0,
            y - self.grid.y + self.scroll.1,
        )
    }

    pub fn screen_point(&self, wx: f32, wy: f32) -> (f32, f32) {
        (
            wx + self.grid.x - self.scroll.0,
            wy + self.grid.y - self.scroll.1,
        )
    }

    pub fn px(&self, ticks: Tick) -> f32 {
        ticks as f32 / self.ppq * self.beat_px
    }
}

/// One controller lane on screen.
#[derive(Clone, Copy, Debug)]
pub struct LaneBox {
    pub index: usize,
    pub folded: bool,
    /// The name column, under the piano keys.
    pub head: Rect,
    /// The chevron that folds this lane.
    pub fold: Rect,
    /// Where the values are drawn (full height, padded inside by [`LANE_PAD`]).
    pub plot: Rect,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub area: Rect,
    /// The ruler above the grid: bar numbers, the loop band, the seek (see [`super::ruler`]).
    pub ruler: Rect,
    pub keys: Rect,
    pub grid: Rect,
    pub strip: Rect,
    pub plus: Rect,
    pub fold: Rect,
    /// The autoscroll switch, after the tools (or after the chevron when the lanes are folded).
    pub follow: Rect,
    /// The horizontal zoom at the strip's right end: out, the slider, in.
    pub zoom_out: Rect,
    pub zoom_track: Rect,
    pub zoom_in: Rect,
    pub name: Rect,
    pub tabs: Vec<Rect>,
    pub lanes: Vec<LaneBox>,
}

impl Frame {
    pub fn lane_at(&self, x: f32, y: f32) -> Option<&LaneBox> {
        self.lanes
            .iter()
            .find(|l| l.plot.contains(x, y) || l.head.contains(x, y))
    }
}

impl Roll {
    pub fn frame(&self, body: Rect) -> Frame {
        let pad = Space::X2.px();
        let area = Rect::new(body.x, body.y, body.w.max(0.0), (body.h - pad).max(0.0));
        let keys_w = self.keys_w;
        let n = self.lanes.len();
        let folded = |i: usize| self.lanes_folded || self.lanes[i].folded;
        let avail = area.h - STRIP_H - Layout::RULER_H;
        let lane_h = if n == 0 {
            LANE_H
        } else {
            ((avail - GRID_H_MIN) / n as f32).clamp(LANE_H_MIN, LANE_H)
        };
        let lanes_total: f32 = (0..n)
            .map(|i| {
                if self.lanes_folded {
                    0.0
                } else if folded(i) {
                    LANE_FOLDED_H
                } else {
                    lane_h
                }
            })
            .sum();
        let grid_h = (avail - lanes_total).max(24.0);
        let ruler = Rect::new(area.x + keys_w, area.y, area.w - keys_w, Layout::RULER_H);
        let top = area.y + Layout::RULER_H;
        let grid = Rect::new(area.x + keys_w, top, area.w - keys_w, grid_h);
        let keys = Rect::new(area.x, top, keys_w, grid_h);
        let strip = Rect::new(area.x, grid.bottom(), area.w, STRIP_H);
        let b = 20.0;
        let by = strip.y + (STRIP_H - b) / 2.0;
        let plus = Rect::new(strip.x + 2.0, by, b, b);
        let fold = Rect::new(plus.right() + 2.0, by, b, b);
        let follow_x = fold.right() + 10.0;
        let follow = Rect::new(follow_x, by, b, b);
        let zoom_in = Rect::new(strip.right() - b - 6.0, by, b, b);
        let zoom_track = Rect::new(zoom_in.x - 4.0 - ZOOM_W, by, ZOOM_W, b);
        let zoom_out = Rect::new(zoom_track.x - 4.0 - b, by, b, b);
        let x = follow.right() + 2.0;
        let name = Rect::new(x + 6.0, strip.y, (zoom_out.x - x - 18.0).max(0.0), STRIP_H);
        let tab_w = (name.w / n.max(1) as f32).min(90.0);
        let tabs = (0..n)
            .map(|i| Rect::new(name.x + i as f32 * tab_w, name.y, tab_w, name.h))
            .collect();
        let mut lanes = Vec::new();
        if !self.lanes_folded {
            let mut y = strip.bottom();
            for i in 0..n {
                let h = if folded(i) { LANE_FOLDED_H } else { lane_h };
                let head = Rect::new(area.x, y, keys_w, h);
                let plot = Rect::new(grid.x, y, grid.w, h);
                let c = 14.0;
                let fold = Rect::new(head.x + 2.0, y + (h.min(LANE_FOLDED_H) - c) / 2.0, c, c);
                lanes.push(LaneBox {
                    index: i,
                    folded: folded(i),
                    head,
                    fold,
                    plot,
                });
                y += h;
            }
        }
        Frame {
            area,
            ruler,
            keys,
            grid,
            strip,
            plus,
            fold,
            follow,
            zoom_out,
            zoom_track,
            zoom_in,
            name,
            tabs,
            lanes,
        }
    }

    pub fn view(&self, ctx: &Ctx) -> View {
        View {
            grid: self.frame(ctx.body).grid,
            ppq: ctx.project.ppq as f32,
            beat_px: self.beat_px,
            row_h: self.row_h,
            scroll: self.scroll,
            rows: ctx.rows(),
        }
    }

    /// The farthest the view can scroll: down to the last row, right to the clip's end.
    pub fn max_scroll(&self, ctx: &Ctx) -> (f32, f32) {
        let v = self.view(ctx);
        (
            (v.px(ctx.clip_len() + ctx.ppq() * 16) - v.grid.w).max(0.0),
            (v.rows.len() as f32 * self.row_h - v.grid.h).max(0.0),
        )
    }

    /// How far left of the song's start the grid can scroll: in the drum view half a hit, so a
    /// hit on the first tick (its diamond centred there) shows whole.
    pub fn lead(&self, ctx: &Ctx) -> f32 {
        if ctx.drum.is_some() {
            (self.row_h / 2.0).ceil() + 1.0
        } else {
            0.0
        }
    }

    pub fn clamp_scroll(&mut self, ctx: &Ctx) {
        let (mx, my) = self.max_scroll(ctx);
        let least = -self.view(ctx).px(ctx.clip.start) - self.lead(ctx);
        self.scroll.0 = self.scroll.0.clamp(least, mx);
        self.scroll.1 = self.scroll.1.clamp(0.0, my);
    }

    /// The scroll that puts `top` at the top of the grid.
    pub fn scroll_to_pitch(&mut self, ctx: &Ctx, top: u8) {
        self.scroll.1 = ctx.rows().rank(top) as f32 * self.row_h;
        self.clamp_scroll(ctx);
    }

    /// The menu of the "+" button, popping upward from it.
    pub fn menu_rect(&self, frame: &Frame, items: usize) -> Rect {
        let h = items as f32 * MENU_ITEM_H + 8.0;
        Rect::new(frame.plus.x, frame.plus.y - h - 2.0, MENU_W, h)
    }

    pub fn menu_item_rect(&self, menu: Rect, i: usize) -> Rect {
        Rect::new(
            menu.x + 4.0,
            menu.y + 4.0 + i as f32 * MENU_ITEM_H,
            menu.w - 8.0,
            MENU_ITEM_H,
        )
    }
}

#[cfg(test)]
mod fold_tests {
    use super::*;
    use crate::roll::{Lane, LaneKind};
    use xirius_core::model::CtrlKind;

    #[test]
    fn folding_one_lane_preserves_the_height_of_every_other_lane() {
        let mut roll = Roll::default();
        roll.lanes.push(Lane::new(LaneKind::Ctrl(CtrlKind::Cc(1))));
        roll.lanes
            .push(Lane::new(LaneKind::Ctrl(CtrlKind::PitchBend)));
        let body = Rect::new(0.0, 0.0, 800.0, 220.0);
        let before = roll.frame(body);
        assert!(before.lanes[1].plot.h < LANE_H);
        roll.lanes[0].folded = true;
        let after = roll.frame(body);
        assert_eq!(after.lanes[0].plot.h, LANE_FOLDED_H);
        for i in 1..3 {
            assert_eq!(before.lanes[i].plot.h, after.lanes[i].plot.h);
        }
        assert!(after.grid.h > before.grid.h);
    }
}
