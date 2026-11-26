//! The lower half of a clip's right edge lengthens it filled with its loop (EDT-041, UI-388): a
//! clip without one loops the whole of what it was, and the drag is one step. With Alt the drag
//! moves where the loop starts instead, and the clip keeps its length. The clip then plays up to
//! the loop's end and the loop again until it ends; it is drawn that way, with a line where each
//! pass starts and a bar under the loop, and the loop's mark among the marks at its top right.

use xirius_core::clipfx::{ClipLoop, pieces};
use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::Clip;
use xirius_core::time::Tick;
use xirius_design::Radius;

use super::{Drag, Ghost, Grip, Plan, Preview, Spot};
use crate::draw::Pen;
use crate::studio::{Rect, Studio};
use crate::tools::Tool;

/// The bar under the loop.
const BAR_H: f32 = 3.0;

impl Studio {
    /// The lower half of the right edge of `clip` dragged: the clip's end (or with Alt its loop's
    /// start) under the pointer.
    pub(super) fn plan_clip_loop(&self, d: &Drag, track: TrackId, clip: &Clip) -> Option<Plan> {
        let p = self.project();
        let shift = d.mods.shift;
        let min_len = self
            .tools()
            .snap
            .active(&self.meter(), shift)
            .unwrap_or(i64::from(p.ppq) / 16)
            .max(1)
            .min(clip.length);
        let was = p.clip_loop(clip.id);
        let base = was.unwrap_or(ClipLoop {
            start: 0,
            length: clip.length,
        });
        let dt = self.px_to_ticks(d.at.0 - d.from.0);
        if !d.mods.alt {
            // The clip's end follows the pointer, and its loop fills it (UI-388).
            let end = self.snap_tick(clip.start + clip.length + dt, shift);
            let length = (end - clip.start).max(min_len);
            if length == clip.length {
                return None;
            }
            return Some(Plan {
                command: Some((
                    "clip.loop.set",
                    serde_json::json!({ "ids": [clip.id.to_string()], "fill": length }),
                )),
                select: None,
                tip: format!(
                    "ループで {:.2} 小節",
                    length as f64 / self.bar_ticks() as f64
                ),
                preview: Preview::ClipFill {
                    ghost: Ghost {
                        track,
                        start: clip.start,
                        length,
                        color: p
                            .tracks
                            .get(&track)
                            .map_or(0, |tr| crate::draw::track_color(tr)),
                    },
                    looped: base,
                },
            });
        }
        let end = base.end().min(clip.length);
        let start = self.snap_tick(clip.start + base.start + dt, shift) - clip.start;
        let start = start.clamp(0, (end - min_len).max(0));
        let looped = ClipLoop {
            start,
            length: end - start,
        };
        if Some(looped) == was || (was.is_none() && start == 0 && end == clip.length) {
            return None;
        }
        let tm = p.time_map();
        Some(Plan {
            command: Some((
                "clip.loop.set",
                serde_json::json!({
                    "ids": [clip.id.to_string()],
                    "start": start,
                    "length": looped.length,
                }),
            )),
            select: None,
            tip: format!(
                "ループの頭 {}",
                crate::topbar::bar_beat_tick(&tm, clip.start + start)
            ),
            preview: Preview::ClipLoop {
                clip: clip.id,
                looped,
            },
        })
    }

    /// The loop of `clip` as it is drawn: the one its head is being dragged to, or its own.
    pub fn shown_loop(&self, clip: ClipId) -> Option<ClipLoop> {
        if let Some(d) = self.arrange.drag.as_ref().filter(|d| d.started)
            && let Spot::Clip {
                track,
                clip: c,
                grip: Grip::Loop,
            } = d.spot
            && c == clip
            && d.tool == Tool::Select
        {
            let p = self.project();
            if let Ok((_, k)) = p.clip(clip)
                && let Some(Plan {
                    preview: Preview::ClipLoop { looped, .. },
                    ..
                }) = self.plan_clip_loop(d, track, k)
            {
                return Some(looped);
            }
        }
        self.project().clip_loop(clip)
    }
}

/// The passes of `clip` as drawn: (where in the clip, from where in it, how long). One pass for a
/// clip without a loop.
pub fn shown_pieces(studio: &Studio, clip: &Clip) -> Vec<(Tick, Tick, Tick)> {
    pieces(studio.shown_loop(clip.id), clip.length)
}

/// What a clip holds, drawn once for each pass: `body` paints into the rect and inner rect it is
/// given, which are shifted so the pass's part of the clip lands where the pass plays.
pub fn paint_passes(
    pen: &mut Pen,
    studio: &Studio,
    clip: &Clip,
    r: Rect,
    inner: Rect,
    mut body: impl FnMut(&mut Pen, Rect, Rect),
) {
    let parts = shown_pieces(studio, clip);
    if parts.len() < 2 {
        body(pen, r, inner);
        return;
    }
    for (at, from, len) in parts {
        let x = r.x + studio.ticks_to_px(at);
        let dx = studio.ticks_to_px(at) - studio.ticks_to_px(from);
        let pass = Rect::new(x, r.y, studio.ticks_to_px(len), r.h);
        pen.mask(pass, |pen| {
            body(
                pen,
                Rect::new(r.x + dx, r.y, r.w, r.h),
                Rect::new(inner.x + dx, inner.y, inner.w, inner.h),
            );
        });
    }
}

/// The marks of a looped clip in `r`: a line where each pass starts again and a bar under the
/// loop.
pub fn paint_mark(pen: &mut Pen, studio: &Studio, clip: &Clip, r: Rect) {
    if let Some(l) = studio.shown_loop(clip.id) {
        pen.mask(r, |pen| {
            let mark = pen.c.line[2];
            for (at, ..) in pieces(Some(l), clip.length).into_iter().skip(1) {
                let x = r.x + studio.ticks_to_px(at);
                pen.fill(Rect::new(x - 0.5, r.y, 1.0, r.h), mark);
            }
            let x0 = r.x + studio.ticks_to_px(l.start);
            let x1 = r.x + studio.ticks_to_px(l.end().min(clip.length));
            pen.round(
                Rect::new(x0, r.bottom() - BAR_H - 1.0, (x1 - x0).max(2.0), BAR_H),
                mark,
                Radius::Pill,
            );
        });
    }
}
