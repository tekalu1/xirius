//! The roll's horizontal zoom, as a part of the strip under the grid: a slider
//! on a log scale with a button at each end. The same zoom is
//! `view.zoom.time --to <px>` (and `--by <steps>`), so a script reaches the
//! same widths the slider does.

use xirius_design::{Layout, Radius};

use super::frame::Frame;
use super::{BEAT_PX_MAX, BEAT_PX_MIN, Ctx, Drag, Mode, Out, Roll};
use crate::draw::Pen;
use crate::studio::Rect;

/// The slider's thumb, in px.
pub const THUMB: f32 = 12.0;
/// How many steps of `view.zoom.time` one press of a button zooms.
pub const BUTTON_STEPS: f32 = 4.0;
/// How much one step of `view.zoom.time` scales the width.
pub const STEP: f32 = 1.18;

/// Where a width sits along the slider (0 at the narrowest, 1 at the widest).
pub fn pos_of(beat_px: f32) -> f32 {
    let span = (BEAT_PX_MAX / BEAT_PX_MIN).ln();
    ((beat_px.max(BEAT_PX_MIN) / BEAT_PX_MIN).ln() / span).clamp(0.0, 1.0)
}

/// The width at a place along the slider.
pub fn px_of(pos: f32) -> f32 {
    BEAT_PX_MIN * (BEAT_PX_MAX / BEAT_PX_MIN).powf(pos.clamp(0.0, 1.0))
}

/// The thumb's rectangle on a slider for the width `beat_px`.
pub fn thumb_of(track: Rect, beat_px: f32) -> Rect {
    let run = (track.w - THUMB).max(0.0);
    Rect::new(
        track.x + run * pos_of(beat_px),
        track.y + (track.h - THUMB) / 2.0,
        THUMB,
        THUMB,
    )
}

impl Roll {
    /// Sets the width of a beat, keeping the tick in the middle of the grid
    /// where it is.
    pub fn zoom_to(&mut self, ctx: &Ctx, beat_px: f32) {
        let g = self.frame(ctx.body).grid;
        let v = self.view(ctx);
        let mid = g.x + g.w / 2.0;
        let anchor = self.zoom_anchor.unwrap_or_else(|| v.tick_f(mid));
        self.beat_px = beat_px.clamp(BEAT_PX_MIN, BEAT_PX_MAX);
        self.scroll.0 = anchor as f32 / v.ppq * self.beat_px - (mid - g.x);
        self.clamp_scroll(ctx);
    }

    /// Zooms by `steps` of `view.zoom.time`.
    pub fn zoom_by(&mut self, ctx: &Ctx, steps: f32) {
        self.zoom_to(ctx, self.beat_px * STEP.powf(steps));
    }

    /// Puts the width where the pointer is along the slider.
    pub(super) fn zoom_track_to(&mut self, ctx: &Ctx, x: f32) {
        let track = self.frame(ctx.body).zoom_track;
        let run = (track.w - THUMB).max(1.0);
        self.zoom_to(ctx, px_of((x - track.x - THUMB / 2.0) / run));
    }

    /// A press in the zoom part of the strip, if it is on one of its parts.
    pub(super) fn zoom_down(&mut self, ctx: &Ctx, f: &Frame, x: f32, y: f32) -> Option<Out> {
        if f.zoom_out.contains(x, y) {
            self.zoom_by(ctx, -BUTTON_STEPS);
        } else if f.zoom_in.contains(x, y) {
            self.zoom_by(ctx, BUTTON_STEPS);
        } else if f.zoom_track.contains(x, y) {
            let g = f.grid;
            self.zoom_anchor = Some(self.view(ctx).tick_f(g.x + g.w / 2.0));
            self.zoom_track_to(ctx, x);
            self.drag = Some(Drag::new(Mode::Zoom, (x, y)));
        } else {
            return None;
        }
        Some(Out::used())
    }
}

/// The zoom part of the strip: the two buttons and the slider.
pub fn paint(pen: &mut Pen, roll: &Roll, f: &Frame) {
    let c = pen.c;
    let size = Layout::ICON_SIZE;
    for (r, icon) in [(f.zoom_out, "zoom-out"), (f.zoom_in, "zoom-in")] {
        let hot = roll.pointer.is_some_and(|(x, y)| r.contains(x, y));
        if hot {
            pen.round(r, c.hover, Radius::S);
        }
        pen.icon(
            icon,
            Rect::new(
                r.x + (r.w - size) / 2.0,
                r.y + (r.h - size) / 2.0,
                size,
                size,
            ),
            c.ink,
        );
    }
    let track = f.zoom_track;
    let held = matches!(roll.drag.as_ref().map(|d| d.mode), Some(Mode::Zoom));
    let hot = held || roll.pointer.is_some_and(|(x, y)| track.contains(x, y));
    let thumb = thumb_of(track, roll.beat_px);
    let mid = track.y + track.h / 2.0;
    pen.round(
        Rect::new(track.x, mid - 2.0, track.w, 4.0),
        c.surface_3,
        Radius::Pill,
    );
    let reach = thumb.x + THUMB / 2.0 - track.x;
    pen.round(
        Rect::new(track.x, mid - 2.0, reach, 4.0),
        c.fill.opacity(0.45),
        Radius::Pill,
    );
    pen.round(thumb, if hot { c.fill_hover } else { c.fill }, Radius::Pill);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slider_runs_from_the_narrowest_width_to_the_widest_on_a_log_scale() {
        assert_eq!(pos_of(BEAT_PX_MIN), 0.0);
        assert_eq!(pos_of(BEAT_PX_MAX), 1.0);
        assert!((px_of(0.0) - BEAT_PX_MIN).abs() < 1e-3);
        assert!((px_of(1.0) - BEAT_PX_MAX).abs() < 0.5);
        // The middle is the geometric mean, so every notch of the slider is the same ratio.
        let mid = px_of(0.5);
        assert!((mid - (BEAT_PX_MIN * BEAT_PX_MAX).sqrt()).abs() < 0.01);
        for px in [8.0, 20.0, 48.0, 300.0, 4096.0] {
            assert!((px_of(pos_of(px)) - px).abs() / px < 1e-3);
        }
    }

    #[test]
    fn the_thumb_stays_inside_the_track() {
        let track = Rect::new(100.0, 10.0, 120.0, 20.0);
        let (lo, hi) = (thumb_of(track, BEAT_PX_MIN), thumb_of(track, BEAT_PX_MAX));
        assert_eq!(lo.x, track.x);
        assert_eq!(hi.right(), track.right());
    }
}
