//! Painting the AI's pop-over, its stars in the top bar, and what runs over
//! the arrangement while it works.

use xirius_core::AiPermission;
use xirius_core::model::{Clip, ClipBody};
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use super::{AiLook, BarAi};
use crate::draw::{Align, Pen};
use crate::popup::{Action, Cell, Look};
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::RowKind;

/// Where each star of `ai-star.svg` / `ai-spark.svg` has its middle, as a
/// part of the icon's box: they grow around it.
const STAR_AT: (f32, f32) = (11.0 / 24.0, 10.5 / 24.0);
const SPARK_AT: (f32, f32) = (18.0 / 24.0, 18.0 / 24.0);
/// The band that runs over the AI's work: its width, as a part of what it
/// runs over, and its strength.
const SCAN_W: f32 = 0.12;
const SCAN_ALPHA: f32 = 0.14;
/// The top of a clip that holds its name.
const CLIP_NAME_H: f32 = 17.0;

/// `r` grown by `s` around the point `at` (a part of its box).
fn grow(r: Rect, at: (f32, f32), s: f32) -> Rect {
    let (px, py) = (r.x + at.0 * r.w, r.y + at.1 * r.h);
    Rect::new(px - at.0 * r.w * s, py - at.1 * r.h * s, r.w * s, r.h * s)
}

/// The two stars in the top bar's AI button.
pub fn paint_bar_button(pen: &mut Pen, r: Rect, v: &BarAi) {
    let c = pen.c;
    let ink = if v.level == AiPermission::Off {
        c.ink_on_toggle
    } else if v.working {
        c.ink_ai
    } else {
        c.ink_strong
    };
    pen.icon("ai-star", grow(r, STAR_AT, v.star.0), ink.opacity(v.star.1));
    pen.icon(
        "ai-spark",
        grow(r, SPARK_AT, v.spark.0),
        ink.opacity(v.spark.1),
    );
}

fn icon_at(r: Rect, x: f32) -> Rect {
    Rect::new(
        x,
        r.y + (r.h - Layout::ICON_SIZE) / 2.0,
        Layout::ICON_SIZE,
        Layout::ICON_SIZE,
    )
}

/// One line of text cut at the box's right edge.
fn line(pen: &mut Pen, s: &str, size: FontSize, weight: Weight, ink: gpui::Hsla, r: Rect) {
    pen.mask(r, |pen| {
        pen.text(s, size, weight, ink, r, Align::Left);
    });
}

/// Paints one of the pop-over's own cells.
pub fn paint_cell(pen: &mut Pen, cell: &Cell, look: &AiLook, hot: bool) {
    let c = pen.c;
    let r = cell.rect;
    let hot = hot && cell.live();
    let gap = Space::X2.px();
    match look {
        AiLook::Head => {
            pen.icon("ai", icon_at(r, r.x), c.ink_ai);
            let x = r.x + Layout::ICON_SIZE + gap;
            pen.text(
                &cell.text,
                FontSize::Small,
                Weight::Semibold,
                c.ink_strong,
                Rect::new(x, r.y, r.right() - x, r.h),
                Align::Left,
            );
        }
        AiLook::Tray => pen.round(r, c.surface_2, Radius::S),
        AiLook::Seg => {
            let ink = if cell.on {
                pen.round(r, c.surface_3, Radius::S);
                c.ink_strong
            } else if hot {
                c.ink_strong
            } else {
                c.ink
            };
            if let Some(icon) = cell.icon {
                let s = Layout::ICON_SIZE;
                let at = Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s);
                pen.icon(icon, at, ink);
            }
        }
        AiLook::About => {
            let half = r.h / 2.0;
            line(
                pen,
                &cell.text,
                FontSize::Body,
                Weight::Semibold,
                c.ink_strong,
                Rect::new(r.x, r.y, r.w, half),
            );
            line(
                pen,
                &cell.hint,
                FontSize::Small,
                Weight::Regular,
                c.ink,
                Rect::new(r.x, r.y + half, r.w, half),
            );
        }
        AiLook::Url { down } => {
            let ink = if *down { c.ink } else { c.ink_strong };
            pen.mask(r, |pen| {
                pen.digits(
                    &cell.text,
                    FontSize::Small,
                    Weight::Regular,
                    ink,
                    r,
                    Align::Left,
                );
            });
        }
        AiLook::Last { refused } => {
            let half = r.h / 2.0;
            pen.icon("ai", icon_at(Rect::new(r.x, r.y, r.w, half), r.x), c.ink_ai);
            let x = r.x + Layout::ICON_SIZE + gap;
            line(
                pen,
                &cell.text,
                FontSize::Small,
                Weight::Medium,
                c.ink_strong,
                Rect::new(x, r.y, r.right() - x, half),
            );
            line(
                pen,
                &cell.hint,
                FontSize::Small,
                Weight::Regular,
                if *refused { c.alert } else { c.ink },
                Rect::new(x, r.y + half, r.right() - x, half),
            );
        }
    }
}

/// Over a history row: the AI's newest step glows once as it is written
/// (AI-036). `fresh` is the step and how far (0..1) its glow is.
pub fn paint_fresh(pen: &mut Pen, cell: &Cell, fresh: Option<(u64, f32)>) {
    let Some((n, t)) = fresh else {
        return;
    };
    let Look::Step { ai: true, .. } = cell.look else {
        return;
    };
    if cell.action != Some(Action::Jump(n)) {
        return;
    }
    let c = pen.c;
    let badge = crate::popup::step_badge(cell.rect);
    let k = 1.0 - super::ease(xirius_design::Easing::OUT, t);
    let s = 1.0 + 0.25 * (std::f32::consts::PI * t).sin();
    pen.round(badge, c.ai_line.opacity(k), Radius::Pill);
    let icon = grow(
        Rect::new(badge.x + 2.0, badge.y + 2.0, 16.0, 16.0),
        (0.5, 0.5),
        s,
    );
    pen.icon("ai", icon, c.ink_ai);
    pen.icon("ai", icon, c.paper.opacity(k));
}

/// Is this clip the AI's: changed by it and not touched since (AI-030)?
pub(crate) fn clip_marked(studio: &Studio, clip: &Clip) -> bool {
    let notes = match &clip.body {
        ClipBody::Midi { notes, .. } => notes.iter().any(|n| n.ai),
        _ => false,
    };
    notes || studio.ai_marked(&clip.id.to_string())
}

/// Is the AI at work on this clip now (it, or a note in it)?
fn clip_editing(studio: &Studio, clip: &Clip) -> bool {
    if studio.ai_editing(&clip.id.to_string()) {
        return true;
    }
    match &clip.body {
        ClipBody::Midi { notes, .. } => notes.iter().any(|n| studio.ai_editing(&n.id.to_string())),
        _ => false,
    }
}

/// The band that runs over the clips the AI works on (AI-036). Under the
/// pop-over, over the cached body.
pub fn overlay(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    if studio.ai.motion.reduced || !studio.ai_working() {
        return;
    }
    let project = studio.project();
    let tree = studio.tree();
    let band = pen.c.ai_line.opacity(SCAN_ALPHA);
    let left = studio.ai.scan(SCAN_W);
    pen.mask(crate::draw::region(g, g.lanes), |pen| {
        for row in &tree.rows {
            let RowKind::Track(t) = row.kind else {
                continue;
            };
            let Some(track) = project.tracks.get(&t) else {
                continue;
            };
            for clip in &track.clips {
                if !clip_editing(studio, clip) {
                    continue;
                }
                // Over the notes, under the name.
                let r = studio.clip_rect(g, row, clip);
                let r = Rect::new(r.x, r.y + CLIP_NAME_H, r.w, r.h - CLIP_NAME_H);
                pen.mask(r, |pen| {
                    let w = r.w * SCAN_W;
                    pen.fill(Rect::new(r.x + left * r.w, r.y, w, r.h), band);
                });
            }
        }
    });
}
