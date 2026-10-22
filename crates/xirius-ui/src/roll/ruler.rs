//! The ruler over the roll: the bar numbers, the loop band and the seek. It is
//! the arrangement's ruler ([`crate::ruler`]) on the roll's pixels, so a press
//! and a drag here do what they do there, and the same parts are painted. Only
//! the map from song time to the window differs: the roll draws the clip from
//! its head, so its origin moves with the clip's start.

use xirius_core::time::Tick;
use xirius_design::Radius;

use super::frame::Frame;
use super::{Ctx, Drag, Mode, Out, Roll};
use crate::draw::Pen;
use crate::ruler::{self as shared, Band, LoopGrip, Scale};
use crate::studio::{Rect, Studio};

/// How close to the clip's end a press on the ruler grabs its handle.
const END_GRAB: f32 = 8.0;

impl Roll {
    /// Song ticks to window x, as the grid draws the clip.
    pub fn ruler_scale(&self, ctx: &Ctx) -> Scale {
        let v = self.view(ctx);
        Scale {
            origin: v.grid.x - self.scroll.0 - v.px(ctx.clip.start),
            beat_px: self.beat_px,
            ppq: v.ppq,
        }
    }

    /// The loop grip under a point on the band (`None` below the band).
    fn loop_grip(&self, ctx: &Ctx, f: &Frame, x: f32, y: f32) -> Option<LoopGrip> {
        if !f.ruler.contains(x, y) || !shared::on_band(f.ruler, y) {
            return None;
        }
        let scale = self.ruler_scale(ctx);
        let span = ctx
            .project
            .loop_range
            .map(|l| (scale.x(l.start), scale.x(l.end)));
        Some(shared::grip_at(span, x))
    }

    /// The grip the pointer is on, for the hover.
    pub(super) fn grip_under(&self, ctx: &Ctx, f: &Frame, x: f32, y: f32) -> Option<LoopGrip> {
        self.loop_grip(ctx, f, x, y)
    }

    /// Whether a point on the ruler's lower half is on the clip's end handle.
    pub fn end_grab(&self, ctx: &Ctx, f: &Frame, x: f32, y: f32) -> bool {
        if !f.ruler.contains(x, y) || shared::on_band(f.ruler, y) {
            return false;
        }
        let end = self.ruler_scale(ctx).x(ctx.clip.start + ctx.clip_len());
        (x - end).abs() <= END_GRAB
    }

    /// The clip's length a drag of its end handle leaves at window x: on the
    /// grid, and at least one step.
    pub(super) fn ruler_end(&self, ctx: &Ctx, x: f32) -> Tick {
        let at = self.ruler_scale(ctx).tick(x) - ctx.clip.start;
        let at = ctx.snapped(self.beat_px, self.mods.shift, at);
        at.max(ctx.step(self.beat_px).max(1))
    }

    /// A press on the ruler. Below the band it seeks. On the band a press on a
    /// grip or the body of the loop drags the loop; a press elsewhere on the
    /// band starts a new range and seeks too, as the arrangement does.
    pub(super) fn ruler_down(&mut self, ctx: &Ctx, f: &Frame, x: f32, y: f32) -> Out {
        let at = self.ruler_scale(ctx).tick(x).max(0);
        if self.end_grab(ctx, f, x, y) {
            self.drag = Some(Drag::new(Mode::ClipEnd, (x, y)));
            return Out::used();
        }
        let Some(grip) = self.loop_grip(ctx, f, x, y) else {
            return Out {
                seek: Some(at),
                ..Out::used()
            };
        };
        self.drag = Some(Drag::new(Mode::Loop(grip), (x, y)));
        Out {
            seek: (grip == LoopGrip::New).then_some(at),
            ..Out::used()
        }
    }

    /// The loop a drag on the band would leave now (song ticks), if it changes
    /// anything. Shift takes the snap off, as it does in the arrangement.
    pub(super) fn ruler_loop(
        &self,
        ctx: &Ctx,
        grip: LoopGrip,
        press_x: f32,
        x: f32,
    ) -> Option<(Tick, Tick)> {
        let scale = self.ruler_scale(ctx);
        let shift = self.mods.shift;
        let now = ctx.project.loop_range.map(|l| (l.start, l.end));
        shared::loop_drag(
            now,
            grip,
            scale.tick(press_x),
            scale.tick(x),
            scale.delta(x - press_x),
            |t| ctx.snapped(self.beat_px, shift, t - ctx.clip.start) + ctx.clip.start,
        )
    }

    /// The loop as the ruler shows it: the drag's preview, else the song's.
    fn shown_loop(&self, ctx: &Ctx) -> Option<(Tick, Tick)> {
        self.preview
            .loop_span
            .or_else(|| ctx.project.loop_range.map(|l| (l.start, l.end)))
    }

    /// The clip's length as the roll shows it (ticks from its head): the
    /// drag's preview as it is, else the song's, eased when it changes.
    pub fn shown_len(&self, studio: &Studio, ctx: &Ctx) -> f32 {
        if let Some(length) = self.preview.clip_end {
            return length as f32;
        }
        studio.motion.value(
            &format!("roll.end.{}", ctx.clip.id),
            ctx.clip_len() as f32,
            xirius_design::Duration::Base,
            studio.ai.motion,
            std::time::Instant::now(),
        )
    }

    /// Window x of the clip's end as the roll shows it.
    pub fn shown_end_x(&self, studio: &Studio, ctx: &Ctx) -> f32 {
        let v = self.view(ctx);
        v.x(0) + self.shown_len(studio, ctx) / v.ppq * v.beat_px
    }

    fn loop_dragging(&self) -> bool {
        matches!(self.drag.as_ref().map(|d| d.mode), Some(Mode::Loop(_)))
    }
}

/// Whether the loop shades the grid: it repeats, or a drag on it is under way.
fn lit(studio: &Studio, roll: &Roll) -> bool {
    studio.transport.looping || roll.loop_dragging()
}

/// The ruler's parts: the bar numbers, then the loop band.
pub fn paint_ruler(pen: &mut Pen, studio: &Studio, roll: &Roll, ctx: &Ctx, f: &Frame) {
    let scale = roll.ruler_scale(ctx);
    let ruler = f.ruler;
    pen.fill(ruler, pen.c.surface_2);
    let tm = ctx.project.time_map();
    let bar_px = (tm.bar_start(2) - tm.bar_start(1)) as f32 / scale.ppq * scale.beat_px;
    let (first, last) = (scale.tick(f.grid.x), scale.tick(f.grid.right()));
    let depth = shared::Depth::at(scale.beat_px);
    if depth == shared::Depth::Bars {
        let bars = shared::bars_in(&tm, first, last);
        shared::paint_bars(pen, &bars, shared::bar_every(bar_px), ruler, |t| scale.x(t));
    } else {
        let marks = shared::marks(&tm, ctx.ppq(), depth, first, last);
        shared::paint_marks(pen, &tm, &marks, ruler, |t| scale.x(t));
    }
    let (left, end) = (scale.x(ctx.clip.start), roll.shown_end_x(studio, ctx));
    let (left, end) = (left.max(f.grid.x), end.min(f.grid.right() + 8.0));
    if end > left {
        let band = Rect::new(left, ruler.bottom() - 7.0, end - left, 5.0);
        pen.round(band, pen.c.fill.opacity(0.45), Radius::Xs);
    }
    if end > f.grid.x && end < f.grid.right() + 8.0 {
        let held = matches!(roll.drag.as_ref().map(|d| d.mode), Some(Mode::ClipEnd));
        let face = if held || roll.end_hover {
            pen.c.fill_hover
        } else {
            pen.c.fill
        };
        pen.round(
            Rect::new(end - 4.0, ruler.bottom() - 10.0, 4.0, 10.0),
            face,
            Radius::Xs,
        );
    }
    if let Some((a, b)) = roll.shown_loop(ctx) {
        let band = Band {
            x0: scale.x(a),
            x1: scale.x(b),
            lit: lit(studio, roll),
            held: roll.loop_dragging(),
            hover: roll.ruler_hover,
            punch: false,
        };
        shared::paint_band(pen, &band, ruler, None);
    }
}

/// The grid outside the clip, dimmed: before its head and past its end.
pub fn paint_veil(pen: &mut Pen, studio: &Studio, roll: &Roll, ctx: &Ctx, f: &Frame) {
    let head = roll.ruler_scale(ctx).x(ctx.clip.start);
    let end = roll.shown_end_x(studio, ctx);
    let veil = pen.c.veil;
    if head > f.grid.x {
        pen.fill(
            Rect::new(f.grid.x, f.grid.y, head - f.grid.x, f.grid.h),
            veil,
        );
    }
    if end < f.grid.right() {
        let x = end.max(f.grid.x);
        pen.fill(Rect::new(x, f.grid.y, f.grid.right() - x, f.grid.h), veil);
    }
}

/// The loop's stretch over the grid, faintly, behind the notes.
pub fn paint_shade(pen: &mut Pen, studio: &Studio, roll: &Roll, ctx: &Ctx, f: &Frame) {
    let Some((a, b)) = roll.shown_loop(ctx) else {
        return;
    };
    if !lit(studio, roll) {
        return;
    }
    let scale = roll.ruler_scale(ctx);
    shared::shade(pen, scale.x(a), scale.x(b), f.grid);
}

/// Window x of the playhead in the roll: the map the bar numbers use.
pub fn playhead_x(studio: &Studio, roll: &Roll, ctx: &Ctx) -> f32 {
    roll.ruler_scale(ctx).x(studio.playhead)
}

/// The playhead over the grid and the ruler, at the song's position.
pub fn paint_playhead(pen: &mut Pen, studio: &Studio, roll: &Roll, ctx: &Ctx, f: &Frame) {
    let x = playhead_x(studio, roll, ctx);
    if x < f.grid.x || x > f.grid.right() {
        return;
    }
    let view = Rect::new(f.grid.x, f.ruler.y, f.grid.w, f.grid.bottom() - f.ruler.y);
    shared::paint_playhead(pen, studio, x, view);
}
