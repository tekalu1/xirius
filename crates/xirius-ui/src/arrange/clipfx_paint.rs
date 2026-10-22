//! Painting the fades, the gain line and the crossfades over a track's clips
//! (EDT-002, 017, 039, 040), and the handles that take hold of them.
//!
//! A fade darkens the part of the clip it silences; the gain line is a thin
//! line with its points; a crossfade is the pair of curves over the stretch
//! where the two clips overlap. Nothing is outlined: shapes are told apart by
//! brightness. The handles show on the clips that are selected or under the
//! pointer, the lines on any clip that has something to show. While a drag is
//! under way the clip is drawn with the edit applied, so what the number says
//! is what the clip looks like before the button comes up.

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::id::ClipId;
use xirius_core::model::{ClipBody, Project, Track};
use xirius_design::Radius;

use super::clipfx::{FxEdit, FxPart, FxView, Handles, MIN_W, SQUARE, Side, fade_xy};
use super::paint::polyline;
use super::{Preview, Spot};
use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::Row;

/// How many pieces a curve is drawn with.
const STEPS: usize = 14;
/// A point of the gain line.
const POINT: f32 = 7.0;
const RING: f32 = 2.0;

/// What the fade or gain drag under way does to the clips it holds, so they are drawn with it
/// (the waveform too, UI-310) before the button comes up.
pub(crate) fn drag_edits(studio: &Studio, g: &Geometry) -> Vec<(ClipId, FxEdit)> {
    studio
        .arrange
        .drag
        .as_ref()
        .filter(|d| d.started && matches!(d.spot, Spot::Fx { .. }))
        .and_then(|d| studio.plan(g, d))
        .and_then(|p| match p.preview {
            Preview::Fx(edits) => Some(edits),
            _ => None,
        })
        .unwrap_or_default()
}

/// The curves, line and handles of every audio clip in a track's row.
pub fn paint_track(
    pen: &mut Pen,
    studio: &Studio,
    project: &Project,
    g: &Geometry,
    row: &Row,
    track: &Track,
) {
    if studio.clips_locked(track.id) {
        return;
    }
    let edits = drag_edits(studio, g);
    let spot_clip = |s: &Spot| match s {
        Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } => Some((*track, *clip)),
        _ => None,
    };
    let hovered = spot_clip(&studio.arrange.hover);
    let dragged = studio
        .arrange
        .drag
        .as_ref()
        .and_then(|d| spot_clip(&d.spot));
    let hot_part = match (&studio.arrange.drag, &studio.arrange.hover) {
        (Some(d), _) => &d.spot,
        (None, h) => h,
    };
    for clip in &track.clips {
        if !matches!(clip.body, ClipBody::Audio { .. }) {
            continue;
        }
        let r = studio.clip_rect(g, row, clip);
        if r.right() < g.lanes.x || r.x > g.lanes.right() {
            continue;
        }
        let Some(mut view) = FxView::of(project, track, clip) else {
            continue;
        };
        for (id, edit) in &edits {
            if *id == clip.id {
                view.apply(edit);
            }
        }
        let key = (track.id, clip.id);
        let active = studio.selected_clip == Some(key)
            || studio.arrange.clips.contains(&key)
            || hovered == Some(key)
            || dragged == Some(key);
        let h = view.handles(studio, r);
        let has_line = !view.points.is_empty() || view.gain_db != 0.0;
        let c = pen.c;
        pen.mask(r, |pen| {
            shade(pen, studio, &view, r, pen.c.surface_1);
            if view.crossed_in {
                cross_curves(pen, studio, &view, r);
            }
            if active || has_line {
                let ink = if active {
                    c.ink_strong.opacity(0.85)
                } else {
                    c.ink_strong.opacity(0.4)
                };
                polyline(pen, &h.line, ink);
                for xy in &h.points {
                    dot(pen, xy.0, xy.1, POINT, ink, c.clip);
                }
            }
            if active && r.w >= MIN_W {
                handles(pen, &h, hot_part, key, c.ink_strong, c.fill);
            }
        });
    }
}

/// The part of the clip a fade silences, a step darker.
fn shade(pen: &mut Pen, studio: &Studio, view: &FxView, r: Rect, ink: Hsla) {
    let tint = ink.opacity(0.8);
    if view.fade_in.length > 0 && !view.crossed_in {
        let w = studio.ticks_to_px(view.fade_in.length).min(r.w).max(1.0);
        let mut pts: Vec<(f32, f32)> = (0..=STEPS)
            .map(|i| fade_xy(r, w, Side::In, i as f32 / STEPS as f32, view.fade_in.curve))
            .collect();
        pts.push((r.x, r.y));
        fill_poly(pen, &pts, tint);
    }
    if view.fade_out.length > 0 && !view.crossed_out {
        let w = studio.ticks_to_px(view.fade_out.length).min(r.w).max(1.0);
        let mut pts: Vec<(f32, f32)> = (0..=STEPS)
            .map(|i| {
                fade_xy(
                    r,
                    w,
                    Side::Out,
                    i as f32 / STEPS as f32,
                    view.fade_out.curve,
                )
            })
            .collect();
        pts.push((r.right(), r.y));
        fill_poly(pen, &pts, tint);
    }
}

/// The two curves of a crossfade over the stretch of the right clip it lies
/// on: this clip rising, the one it follows falling.
fn cross_curves(pen: &mut Pen, studio: &Studio, view: &FxView, r: Rect) {
    let w = studio.ticks_to_px(view.fade_in.length).min(r.w);
    if w < 1.0 {
        return;
    }
    let ink = pen.c.ink_strong.opacity(0.85);
    let curve = view.fade_in.curve;
    let rising: Vec<(f32, f32)> = (0..=STEPS)
        .map(|i| fade_xy(r, w, Side::In, i as f32 / STEPS as f32, curve))
        .collect();
    // The falling one is the rising one turned round: it is the left clip's
    // share of the same stretch.
    let falling: Vec<(f32, f32)> = (0..=STEPS)
        .map(|i| {
            let f = i as f32 / STEPS as f32;
            let (_, y) = fade_xy(r, w, Side::In, 1.0 - f, curve);
            (r.x + f * w, y)
        })
        .collect();
    let veil = pen.c.surface_1.opacity(0.55);
    let mut above_rise = rising.clone();
    above_rise.push((r.x, r.y));
    fill_poly(pen, &above_rise, veil);
    polyline(pen, &rising, ink);
    polyline(pen, &falling, ink);
}

fn handles(
    pen: &mut Pen,
    h: &Handles,
    hot: &Spot,
    key: (xirius_core::id::TrackId, xirius_core::id::ClipId),
    ink: Hsla,
    lit: Hsla,
) {
    let part = match hot {
        Spot::Fx {
            track, clip, part, ..
        } if (*track, *clip) == key => Some(*part),
        _ => None,
    };
    let face = |p: FxPart| if part == Some(p) { lit } else { ink };
    for (flag, side) in [(h.flag_in, Side::In), (h.flag_out, Side::Out)] {
        if let Some(f) = flag {
            pen.round(f, face(FxPart::Flag(side)), Radius::Xs);
        }
    }
    let square = |pen: &mut Pen, xy: (f32, f32), color: Hsla| {
        pen.round(
            Rect::new(xy.0 - SQUARE / 2.0, xy.1 - SQUARE / 2.0, SQUARE, SQUARE),
            color,
            Radius::Xs,
        );
    };
    if let Some(m) = h.mid_in {
        square(pen, m, face(FxPart::Mid(Side::In)));
    }
    if let Some(m) = h.mid_out {
        square(pen, m, face(FxPart::Mid(Side::Out)));
    }
    if let Some(m) = h.cross {
        square(pen, m, face(FxPart::Cross));
    }
    square(pen, h.gain, face(FxPart::Gain));
    if let Some(FxPart::Point(i)) = part
        && let Some(xy) = h.points.get(i)
    {
        dot(pen, xy.0, xy.1, POINT + 2.0, lit, pen.c.clip);
    }
}

fn dot(pen: &mut Pen, x: f32, y: f32, size: f32, color: Hsla, ring: Hsla) {
    let o = size + RING * 2.0;
    pen.round(
        Rect::new(x - o / 2.0, y - o / 2.0, o, o),
        ring,
        Radius::Pill,
    );
    pen.round(
        Rect::new(x - size / 2.0, y - size / 2.0, size, size),
        color,
        Radius::Pill,
    );
}

/// A filled polygon through window points.
fn fill_poly(pen: &mut Pen, pts: &[(f32, f32)], color: Hsla) {
    let Some(first) = pts.first() else {
        return;
    };
    let (ox, oy) = pen.origin;
    let mut path = PathBuilder::fill();
    path.move_to(point(px(ox + first.0), px(oy + first.1)));
    for p in &pts[1..] {
        path.line_to(point(px(ox + p.0), px(oy + p.1)));
    }
    path.close();
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}
