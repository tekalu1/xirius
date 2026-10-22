//! Painting what the arrangement's editing adds: the loop band, the tempo
//! line, automation lanes and their heads, and, over everything, what a drag
//! would do (ghost clips, the drop mark, the marquee) with its value in a
//! balloon next to the pointer.
//!
//! Points, curves, the tempo line and the loop band show a drag's preview in
//! place (the cached picture is redrawn while they move); the rest is painted
//! in [`overlay`].

use gpui::{Hsla, PathBuilder, point, px};
use xirius_core::auto::{AutoLane, AutoMode};
use xirius_core::model::{Project, Track};
use xirius_core::time::Tick;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};

use super::clipfx::FxPart;
use super::{Grip, LaneButton, LaneParts, LoopGrip, Preview, Spot, lane_value_y};
use crate::draw::{Align, Pen, track_color};
use crate::roll::Cursor;
use crate::studio::{Geometry, Rect, RowParts, Studio};
use crate::tools::Tool;
use crate::tree::{LANE_H, Row, RowKind};

/// How thick lines (automation, tempo) are drawn.
const LINE_W: f32 = 1.5;
/// An automation point, and the ring that keeps it apart from the line.
const POINT: f32 = 9.0;
const POINT_RING: f32 = 2.0;
const TEMPO_POINT: f32 = 7.0;
/// How many pieces a curved segment is drawn with.
const CURVE_STEPS: usize = 16;

fn alpha(c: Hsla, a: f32) -> Hsla {
    c.opacity(a)
}

fn track_ink(pen: &Pen, track: &Track) -> Hsla {
    pen.c.tracks[track_color(track) % pen.c.tracks.len()]
}

/// What the drag in progress previews, if anything.
fn preview(studio: &Studio, g: &Geometry) -> Option<Preview> {
    let d = studio.arrange.drag.as_ref()?;
    studio.plan(g, d).map(|p| p.preview)
}

/// A dot with a ring of `ring` around it, centred on (x, y).
fn dot(pen: &mut Pen, x: f32, y: f32, size: f32, color: Hsla, ring: Hsla) {
    let o = size + POINT_RING * 2.0;
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

/// A line through window points.
pub(super) fn polyline(pen: &mut Pen, pts: &[(f32, f32)], color: Hsla) {
    let Some(first) = pts.first() else {
        return;
    };
    let (ox, oy) = pen.origin;
    let mut path = PathBuilder::stroke(px(LINE_W));
    path.move_to(point(px(ox + first.0), px(oy + first.1)));
    for p in &pts[1..] {
        path.line_to(point(px(ox + p.0), px(oy + p.1)));
    }
    if let Ok(path) = path.build() {
        pen.window.paint_path(path, color);
    }
}

// ---------------------------------------------------------------- track column

/// The automation button in a track's head, and the heads of its lanes.
pub fn head(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry, row: &Row, top: f32) {
    let RowKind::Track(t) = row.kind else {
        return;
    };
    let Some(track) = project.tracks.get(&t) else {
        return;
    };
    let c = pen.c;
    let at = |r: Rect, y: f32| Rect::new(g.column.x + r.x, y + r.y, r.w, r.h);
    if let Some(r) = RowParts::of(row).auto {
        let open = studio.arrange.open.contains(&t);
        let used = track.automation.iter().any(|l| !l.points.is_empty());
        let ink = if studio.arrange.hover == Spot::AutoToggle(t) {
            c.ink_strong
        } else if used {
            c.ink_blue
        } else {
            c.ink
        };
        let name = if open {
            "chevron-down"
        } else {
            "chevron-right"
        };
        // Open, it reads as pressed (the mock's lit chevron).
        if open {
            pen.round(at(r, top), c.surface_3, Radius::Clip);
        }
        pen.icon(name, at(r, top), ink);
    }
    crate::automation_track_mode::paint(pen, studio, row, t, track.auto_mode, g.column.x, top);
    for (i, lane) in studio.arrange.lanes_of(track).iter().enumerate() {
        let lane_top = top + row.h + i as f32 * LANE_H;
        let parts = LaneParts::of(row, i == 0);
        let hot = |b: LaneButton| {
            studio.arrange.hover
                == Spot::LaneHead {
                    track: t,
                    target: lane.target.clone(),
                    button: b,
                }
        };
        // What the lane moves: a button with the name and a chevron.
        let r = at(parts.param, lane_top);
        let face = if hot(LaneButton::Param) {
            c.hover
        } else {
            c.surface_3
        };
        pen.round(r, face, Radius::S);
        let s = Layout::ICON_SIZE;
        let text = Rect::new(
            r.x + Space::X2.px(),
            r.y,
            (r.w - s - Space::X3.px()).max(0.0),
            r.h,
        );
        let name = crate::automation_targets::lane_label(project, track, &lane.target);
        pen.mask(text, |pen| {
            pen.text(
                &name,
                FontSize::Micro,
                Weight::Medium,
                c.ink,
                text,
                Align::Left,
            );
        });
        pen.icon(
            "chevron-down",
            Rect::new(
                r.right() - s - Space::X1.px() / 2.0,
                r.y + (r.h - s) / 2.0,
                s,
                s,
            ),
            c.ink,
        );
        for (rect, icon, b) in [
            (parts.add, "plus", LaneButton::Add),
            (parts.remove, "close", LaneButton::Remove),
        ] {
            let r = at(rect, lane_top);
            let ink = if hot(b) {
                pen.round(r, c.hover, Radius::Clip);
                c.ink_strong
            } else {
                c.ink
            };
            pen.icon(icon, centred(r, s), ink);
        }
    }
}

/// The icon of an automation mode: each a different shape.
pub fn mode_icon(m: AutoMode) -> &'static str {
    match m {
        AutoMode::Off | AutoMode::Read => "am-read",
        AutoMode::Touch => "am-touch",
        AutoMode::Latch => "am-latch",
        AutoMode::Write => "am-write",
    }
}

fn centred(r: Rect, s: f32) -> Rect {
    Rect::new(r.x + (r.w - s) / 2.0, r.y + (r.h - s) / 2.0, s, s)
}

// ---------------------------------------------------------------- lanes

/// A track's automation lanes, under its clips: the line and its points.
pub fn lanes(
    pen: &mut Pen,
    studio: &Studio,
    _project: &Project,
    g: &Geometry,
    row: &Row,
    track: &Track,
    top: f32,
) {
    let shown = studio.arrange.lanes_of(track);
    if shown.is_empty() {
        return;
    }
    let pv = preview(studio, g);
    let dragged = studio
        .arrange
        .drag
        .as_ref()
        .filter(|d| d.started)
        .map(|d| d.spot.clone());
    let color = track_ink(pen, track);
    let color = if track.auto_mode.plays() {
        color
    } else {
        alpha(color, 0.5)
    };
    for (i, lane) in shown.iter().enumerate() {
        let lane_top = top + row.h + i as f32 * LANE_H;
        if lane_top > g.lanes.bottom() || lane_top + LANE_H < g.lanes.y {
            continue;
        }
        let lane = with_preview(lane, track, pv.as_ref());
        let y = |v: f64| lane_top + lane_value_y(&lane.target, v);
        let line = lane_line(studio, g, &lane, track, y);
        polyline(pen, &line, color);
        for (index, p) in lane.points.iter().enumerate() {
            let x = studio.tick_x(g, p.tick);
            if x < g.lanes.x - POINT || x > g.lanes.right() + POINT {
                continue;
            }
            let spot = Spot::Point {
                track: track.id,
                target: lane.target.clone(),
                index,
            };
            let hot = studio.arrange.hover == spot || dragged.as_ref() == Some(&spot);
            let chosen = studio.arrange.points.has(track.id, &lane.target, p.tick)
                || (dragged.as_ref() == Some(&spot) && !studio.arrange.points.is_empty());
            let face = if hot { pen.c.ink_strong } else { color };
            if chosen {
                // A selected point is larger and takes the selection's color.
                dot(pen, x, y(p.value), POINT + 4.0, pen.c.ring, pen.c.surface_1);
            } else {
                dot(pen, x, y(p.value), POINT, face, pen.c.surface_1);
            }
        }
    }
}

/// The lane as it would be if the drag in progress were let go.
fn with_preview(lane: &AutoLane, track: &Track, pv: Option<&Preview>) -> AutoLane {
    let mut lane = lane.clone();
    match pv {
        Some(Preview::Point {
            track: t,
            target,
            index,
            tick,
            value,
        }) if *t == track.id && *target == lane.target => {
            lane.push_point(*index, *tick, *value);
        }
        Some(Preview::Curve {
            track: t,
            target,
            index,
            curve,
        }) if *t == track.id && *target == lane.target => {
            let mut pts = lane.points.to_vec();
            if let Some(p) = pts.get_mut(*index) {
                p.curve = *curve;
            }
            lane.points = pts.into();
        }
        Some(Preview::RangeMove {
            place: Some((t, target)),
            from,
            to,
            how,
        }) if *t == track.id && *target == lane.target => {
            let fallback = target.stored_value(track);
            if let Some(moved) =
                xirius_core::commands::moved_lane(&lane, *from, *to, *how, fallback)
            {
                lane = moved;
            }
        }
        _ => {}
    }
    lane
}

/// The window points of a lane's line, across the visible lanes.
fn lane_line(
    studio: &Studio,
    g: &Geometry,
    lane: &AutoLane,
    track: &Track,
    y: impl Fn(f64) -> f32,
) -> Vec<(f32, f32)> {
    let (left, right) = (g.lanes.x, g.lanes.right());
    let pts = &*lane.points;
    let Some(first) = pts.first() else {
        let v = lane
            .target
            .current(track)
            .unwrap_or_else(|| lane.target.range().0);
        return vec![(left, y(v)), (right, y(v))];
    };
    let x = |t: Tick| studio.tick_x(g, t);
    let mut out = vec![(left.min(x(first.tick)), y(first.value))];
    out.push((x(first.tick), y(first.value)));
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (xa, xb) = (x(a.tick), x(b.tick));
        if lane.target.stepped() || a.step {
            out.push((xb, y(a.value)));
            out.push((xb, y(b.value)));
        } else if a.curve == 0.0 || xb < left || xa > right {
            out.push((xb, y(b.value)));
        } else {
            for k in 1..=CURVE_STEPS {
                let f = k as f64 / CURVE_STEPS as f64;
                let tick = a.tick as f64 + (b.tick - a.tick) as f64 * f;
                let v = lane.curve_at(tick).unwrap_or(b.value);
                out.push((xa + (xb - xa) * f as f32, y(v)));
            }
        }
    }
    let last = pts[pts.len() - 1];
    out.push((right.max(x(last.tick)), y(last.value)));
    out
}

// ---------------------------------------------------------------- loop and tempo

/// The loop's range, as the drag in progress would leave it.
fn loop_now(studio: &Studio, g: &Geometry, project: &Project) -> Option<(Tick, Tick)> {
    if let Some(Preview::Loop(a, b)) = preview(studio, g) {
        return Some((a, b));
    }
    project.loop_range.map(|l| (l.start, l.end))
}

/// The loop range shades the lanes while it repeats (or is being dragged).
fn loop_lit(studio: &Studio) -> bool {
    studio.transport.looping
        || studio
            .arrange
            .drag
            .as_ref()
            .is_some_and(|d| d.started && matches!(d.spot, Spot::Loop(_)))
}

/// The loop's stretch, faintly, over the lanes.
pub fn loop_shade(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry) {
    let Some((a, b)) = loop_now(studio, g, project) else {
        return;
    };
    if !loop_lit(studio) {
        return;
    }
    let scale = studio.ruler_scale(g);
    crate::ruler::shade(pen, scale.x(a), scale.x(b), g.lanes);
}

/// The auto-punch range and its pre-roll and post-roll stretches, while auto-punch is on.
pub fn punch_bands(studio: &Studio, project: &Project) -> Option<xirius_session::punch::Bands> {
    let settings = studio.song.settings().lock().ok()?;
    let panel = xirius_session::punch::Panel::from_prefs(&settings.prefs, project.ppq);
    xirius_session::punch::bands(project, &panel)
}

/// The loop band along the top of the ruler (ARR-001): filled while the
/// range repeats, grey while it does not.
pub fn loop_band(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry) {
    let Some((a, b)) = loop_now(studio, g, project) else {
        return;
    };
    let scale = studio.ruler_scale(g);
    // Auto-punch makes the loop range the punch range: red, with the pre-roll and the
    // post-roll as grey stretches beside it (REC-007).
    let punch = punch_bands(studio, project);
    if let Some(bands) = punch {
        let x = |r: Option<(Tick, Tick)>| r.map(|(p, q)| (scale.x(p), scale.x(q)));
        crate::ruler::paint_rolls(pen, g.ruler, [x(bands.pre), x(bands.post)]);
    }
    let band = crate::ruler::Band {
        x0: scale.x(a),
        x1: scale.x(b),
        punch: punch.is_some(),
        lit: loop_lit(studio),
        held: studio
            .arrange
            .drag
            .as_ref()
            .is_some_and(|d| matches!(d.spot, Spot::Loop(_))),
        hover: match studio.arrange.hover {
            Spot::Loop(grip) => Some(grip),
            _ => None,
        },
    };
    let below = g.ruler.y + Arrange::LOOP_H;
    let shade_to = Rect::new(0.0, below, 0.0, g.header_h() - Arrange::LOOP_H);
    crate::ruler::paint_band(pen, &band, g.ruler, Some(shade_to));
}

use super::Arrange;

/// The tempo line: a step at every change, its points and their values.
pub fn tempo(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry) {
    let preview = preview(studio, g);
    let pv = match preview {
        Some(Preview::Tempo { index, tick, bpm }) => Some((index, tick, bpm)),
        _ => None,
    };
    // A drag on the range over the row shows the tempo as it would land (AUT-010).
    let moved = match preview {
        Some(Preview::RangeMove {
            place: None,
            from,
            to,
            how,
        }) => Some(xirius_core::commands::moved_tempo(
            &project.tempo,
            from,
            to,
            how,
        )),
        _ => None,
    };
    let shown = moved.as_deref().unwrap_or(&project.tempo);
    let pts: Vec<(Tick, f64)> = shown
        .iter()
        .enumerate()
        .map(|(i, t)| match pv {
            Some((k, tick, bpm)) if k == i => (tick, bpm),
            _ => (t.tick, t.bpm),
        })
        .collect();
    let Some(first) = pts.first() else {
        return;
    };
    let c = pen.c;
    let xy = |(t, bpm): (Tick, f64)| studio.tempo_xy(g, t, bpm);
    let (x0, y0) = xy(*first);
    let mut line = vec![(g.tempo.x.min(x0), y0), (x0, y0)];
    for (i, w) in pts.windows(2).enumerate() {
        let (_, ya) = xy(w[0]);
        let (xb, yb) = xy(w[1]);
        if !shown[i].ramp {
            line.push((xb, ya));
        }
        line.push((xb, yb));
    }
    let (xl, yl) = xy(pts[pts.len() - 1]);
    line.push((g.tempo.right().max(xl), yl));
    polyline(pen, &line, c.fill_hover);
    let held = studio
        .arrange
        .drag
        .as_ref()
        .filter(|d| d.started)
        .and_then(|d| match d.spot {
            Spot::Tempo(i) => Some(i),
            _ => None,
        });
    for (i, p) in pts.iter().enumerate() {
        let (x, y) = xy(*p);
        if x < g.tempo.x - 40.0 || x > g.tempo.right() {
            continue;
        }
        let hot = held == Some(i) || studio.arrange.hover == Spot::Tempo(i);
        let face = if hot { c.ink_strong } else { c.fill_hover };
        dot(pen, x, y, TEMPO_POINT, face, c.surface_2);
        // A jump (two points at one place, as the edges of a moved range) shows the value after.
        if pts.get(i + 1).is_some_and(|q| xy(*q).0 - x < 4.0) {
            continue;
        }
        let label = Rect::new(x + Space::X2.px(), y - 9.0, 64.0, 18.0);
        let label = if label.bottom() > g.tempo.bottom() {
            Rect::new(label.x, g.tempo.bottom() - label.h, label.w, label.h)
        } else {
            label
        };
        pen.digits(
            &format!("{}", p.1),
            FontSize::Micro,
            Weight::Medium,
            if hot { c.ink_strong } else { c.ink },
            label,
            Align::Left,
        );
    }
}

// ---------------------------------------------------------------- over everything

/// What the drag in progress would do, and its value next to the pointer.
pub fn overlay(pen: &mut Pen, studio: &Studio, g: &Geometry) {
    let c = pen.c;
    let lanes_region = Rect::new(
        g.lanes.x,
        g.lanes.y,
        g.lanes.w,
        (g.drawer.y - Layout::REGION_GAP - g.lanes.y).max(0.0),
    );
    let drag = studio.arrange.drag.as_ref().filter(|d| d.started);
    // The range stays while its values are dragged.
    let on_range =
        drag.is_some_and(|d| matches!(d.spot, Spot::RangeTrim(_) | Spot::FrameGrip { .. }));
    if drag.is_none() || on_range {
        let ranges = if studio.arrange.ranges.is_empty() {
            studio.arrange.range.into_iter().collect::<Vec<_>>()
        } else {
            studio.arrange.ranges.clone()
        };
        for (a, b) in ranges {
            let (x0, x1) = (studio.tick_x(g, a), studio.tick_x(g, b));
            pen.mask(lanes_region, |pen| {
                pen.fill(
                    Rect::new(x0, g.lanes.y, x1 - x0, g.lanes.h),
                    alpha(c.fill, 0.12),
                );
            });
        }
    }
    range_frame(pen, studio, g, lanes_region);
    let Some(d) = drag else {
        cut_line(pen, studio, g, lanes_region);
        if let Some((at, tip)) = studio.point_tip(g) {
            balloon(pen, g, at, &tip);
        }
        return;
    };
    let Some(plan) = studio.plan(g, d) else {
        return;
    };
    let tree = studio.tree();
    match &plan.preview {
        Preview::Clips(ghosts) => pen.mask(lanes_region, |pen| {
            let inset = Space::X1.px();
            for ghost in ghosts {
                let Some(row) = tree.row_of(RowKind::Track(ghost.track)) else {
                    continue;
                };
                let r = Rect::new(
                    studio.tick_x(g, ghost.start),
                    studio.row_y(g, row) + inset,
                    studio.ticks_to_px(ghost.length),
                    row.h - inset * 2.0,
                );
                let color = c.tracks[ghost.color % c.tracks.len()];
                pen.round(r, alpha(color, 0.45), Radius::Clip);
            }
        }),
        Preview::Marquee(r) => pen.round(*r, alpha(c.fill, 0.14), Radius::Xs),
        Preview::Stroke(line) => polyline(pen, line, c.ink_strong),
        Preview::Range {
            start,
            end,
            top,
            bottom,
        } => {
            let (x0, x1) = (studio.tick_x(g, *start), studio.tick_x(g, *end));
            pen.mask(lanes_region, |pen| {
                pen.fill(
                    Rect::new(x0, *top, x1 - x0, bottom - top),
                    alpha(c.fill, 0.18),
                );
            });
        }
        Preview::Section { id, dx, slot } => {
            let header = Rect::new(g.ruler.x, g.ruler.y, g.ruler.w, g.header_h());
            if let Some(s) = studio.project().sections.iter().find(|s| s.id == *id) {
                // What goes with it, in every row: from its head to the next section's,
                // and the line where it will land through all of them (ARR-002).
                let p = studio.project();
                let till = p
                    .sections
                    .iter()
                    .filter(|o| o.start > s.start)
                    .map(|o| o.start)
                    .min()
                    .unwrap_or(s.start + s.length);
                let (a, b) = (studio.tick_x(g, s.start), studio.tick_x(g, till));
                pen.mask(lanes_region, |pen| {
                    pen.fill(
                        Rect::new(a, lanes_region.y, b - a, lanes_region.h),
                        alpha(c.fill, 0.12),
                    );
                    let x = studio.tick_x(g, *slot);
                    pen.round(
                        Rect::new(x - 1.0, lanes_region.y, 2.0, lanes_region.h),
                        c.ink_strong,
                        Radius::Pill,
                    );
                });
                let x0 = studio.tick_x(g, s.start) + dx;
                let w = studio.ticks_to_px(s.length);
                let band = g.section_band;
                pen.mask(header, |pen| {
                    let r = Rect::new(x0, band.y + 2.0, w, band.h - 4.0);
                    pen.round(r, c.clip_sel, Radius::Clip);
                    pen.mask(r, |pen| {
                        let at = Rect::new(r.x + Space::X2.px(), r.y, r.w, r.h);
                        pen.text(
                            &s.name,
                            FontSize::Caption,
                            Weight::Semibold,
                            c.ink_strong,
                            at,
                            Align::Left,
                        );
                    });
                    let x = studio.tick_x(g, *slot);
                    pen.round(
                        Rect::new(x - 1.0, band.y, 2.0, band.h),
                        c.ink_strong,
                        Radius::Pill,
                    );
                });
            }
        }
        Preview::NewSection { start, end } => {
            let (x0, x1) = (studio.tick_x(g, *start), studio.tick_x(g, *end));
            let band = g.section_band;
            pen.round(
                Rect::new(x0, band.y + 2.0, x1 - x0, band.h - 4.0),
                c.clip_sel,
                Radius::Clip,
            );
        }
        Preview::SectionLength { id, length } => {
            if let Some(s) = studio.project().sections.iter().find(|s| s.id == *id) {
                let x0 = studio.tick_x(g, s.start);
                let x1 = studio.tick_x(g, s.start + length);
                let band = g.section_band;
                pen.round(
                    Rect::new(x0, band.y + 2.0, x1 - x0, band.h - 4.0),
                    c.clip_sel,
                    Radius::Clip,
                );
            }
        }
        Preview::Drop { mark, moving } => {
            let column = Rect::new(g.column.x, g.column.y, g.column.w, lanes_region.h);
            let both = Rect::new(
                column.x,
                column.y,
                lanes_region.right() - column.x,
                column.h,
            );
            pen.mask(both, |pen| {
                for kind in moving {
                    let Some(row) = tree.row_of(*kind) else {
                        continue;
                    };
                    let top = studio.row_y(g, row);
                    for x in [column, lanes_region] {
                        pen.fill(
                            Rect::new(x.x, top, x.w, row.full_h()),
                            alpha(c.surface_2, 0.5),
                        );
                    }
                }
                match mark {
                    super::DropMark::Line { y, depth } => {
                        let y = g.column.y + y - studio.scroll.1;
                        let x = g.column.x
                            + Space::X2.px()
                            + (*depth).min(Layout::INDENT_MAX_DEPTH) as f32 * Layout::INDENT;
                        pen.round(
                            Rect::new(x, y - 1.5, column.right() - Space::X2.px() - x, 3.0),
                            c.fill,
                            Radius::Pill,
                        );
                    }
                    super::DropMark::Into(f) => {
                        if let Some(row) = tree.row_of(RowKind::Folder(*f)) {
                            let top = studio.row_y(g, row);
                            for x in [column, lanes_region] {
                                pen.round(
                                    Rect::new(x.x, top, x.w, row.h),
                                    alpha(c.sel, 0.75),
                                    Radius::S,
                                );
                            }
                        }
                    }
                }
            });
        }
        Preview::Loop(..)
        | Preview::Fx(_)
        | Preview::Speed { .. }
        | Preview::Tempo { .. }
        | Preview::Point { .. }
        | Preview::Curve { .. }
        | Preview::RangeMove { .. }
        | Preview::ClipLoop { .. } => {}
    }
    balloon(pen, g, d.at, studio.drag_tip(&plan));
}

/// The transform frame round the range (AUT-010): a line along its top and down both sides, with
/// the handle that scales on the top edge and the one that tilts on the corner, as the velocity
/// lane's frame has them. It follows the values while a handle is dragged.
fn range_frame(pen: &mut Pen, studio: &Studio, g: &Geometry, lanes_region: Rect) {
    let how = studio
        .arrange
        .drag
        .as_ref()
        .filter(|d| d.started && matches!(d.spot, Spot::FrameGrip { .. }))
        .and_then(|d| studio.plan(g, d))
        .and_then(|p| match p.preview {
            Preview::RangeMove { how, .. } => Some(how),
            _ => None,
        });
    let Some(b) = studio.frame_box(g, how) else {
        return;
    };
    let tempo = studio
        .arrange
        .frame
        .as_ref()
        .is_some_and(|f| f.place.is_none());
    let region = if tempo { g.tempo } else { lanes_region };
    let region = Rect::new(region.x, region.y - 6.0, region.w, region.h + 12.0);
    pen.mask(region, |pen| {
        let mut line = pen.c.ring;
        line.a = 0.5;
        let f = b.frame;
        pen.fill(Rect::new(f.x, f.y, f.w, 1.0), line);
        pen.fill(Rect::new(f.x, f.y, 1.0, f.h), line);
        pen.fill(Rect::new(f.right() - 1.0, f.y, 1.0, f.h), line);
        for handle in [b.scale, b.tilt] {
            pen.round(handle, pen.c.ring, Radius::Xs);
        }
    });
}

/// The cut tool's line over the clip under the pointer, where a press would split it.
fn cut_line(pen: &mut Pen, studio: &Studio, g: &Geometry, region: Rect) {
    let Some((track, clip, at)) = studio.cut_preview(g) else {
        return;
    };
    let tree = studio.tree();
    let Some(row) = tree.row_of(RowKind::Track(track)) else {
        return;
    };
    let project = studio.project();
    let Ok((_, c)) = project.clip(clip) else {
        return;
    };
    let r = studio.clip_rect(g, row, c);
    let x = studio.tick_x(g, at).round();
    let ink = pen.c.ink_strong;
    pen.mask(region, |pen| pen.fill(Rect::new(x, r.y, 1.0, r.h), ink));
}

/// The value of a drag, in a small box below and right of the pointer.
fn balloon(pen: &mut Pen, g: &Geometry, at: (f32, f32), tip: &str) {
    if tip.is_empty() {
        return;
    }
    let width: f32 = tip
        .chars()
        .map(|ch| if ch.is_ascii() { 7.0 } else { 12.5 })
        .sum::<f32>()
        + Space::X2.px() * 2.0;
    let h = 24.0;
    let x = (at.0 + 14.0).min(g.size.0 - width - Space::X1.px());
    let y = (at.1 + 16.0).min(g.size.1 - h - Space::X1.px());
    let r = Rect::new(x, y, width, h);
    pen.round(r, pen.c.surface_3, Radius::S);
    pen.digits(
        tip,
        FontSize::Caption,
        Weight::Medium,
        pen.c.ink_strong,
        Rect::new(r.x + Space::X2.px(), r.y, r.w, r.h),
        Align::Left,
    );
}

/// Whether `track` is an instrument track (the one a part of notes can be drawn on).
fn instrument(studio: &Studio, track: xirius_core::id::TrackId) -> bool {
    studio
        .project()
        .tracks
        .get(&track)
        .is_some_and(|t| t.kind == xirius_core::model::TrackKind::Instrument)
}

/// The pointer's shape over the arrangement; None leaves it to the roll.
pub fn cursor(studio: &Studio) -> Option<Cursor> {
    let (spot, tool, started) = match &studio.arrange.drag {
        Some(d) => (&d.spot, d.tool, d.started),
        None => (
            &studio.arrange.hover,
            studio.holds().tool(
                crate::tools::Area::Arrange,
                studio.tool,
                studio.arrange.mods,
            ),
            false,
        ),
    };
    Some(match spot {
        Spot::Clip { grip, track, .. } => match tool {
            Tool::Cut => Cursor::Split,
            Tool::Eraser => Cursor::Erase,
            Tool::Draw if instrument(studio, *track) => Cursor::Pencil,
            Tool::Draw => Cursor::Blocked,
            Tool::Mute => Cursor::Mute,
            Tool::Audition => Cursor::Audition,
            Tool::Range => Cursor::Range,
            _ => match grip {
                Grip::Left | Grip::Right | Grip::Corner => Cursor::ResizeH,
                Grip::Body if started => Cursor::Move,
                Grip::Body => Cursor::Arrow,
            },
        },
        Spot::Fx { part, .. } => match part {
            FxPart::Flag(_) | FxPart::Boundary(_) => Cursor::ResizeH,
            FxPart::Mid(_) | FxPart::Gain => Cursor::ResizeV,
            FxPart::Cross | FxPart::Point(_) | FxPart::Slip => Cursor::Move,
            FxPart::Line { .. } if started => Cursor::Move,
            FxPart::Line { .. } => Cursor::ResizeV,
        },
        Spot::Loop(LoopGrip::Start | LoopGrip::End) => Cursor::ResizeH,
        Spot::Loop(LoopGrip::Body) => Cursor::Move,
        Spot::Section { resize: true, .. } => Cursor::ResizeH,
        Spot::Section { .. } if started => Cursor::Move,
        // Split and mute have nothing to act on in the tempo row and the lanes.
        Spot::Tempo(_)
        | Spot::TempoGap
        | Spot::Point { .. }
        | Spot::Line { .. }
        | Spot::Lane { .. }
            if matches!(tool, Tool::Cut | Tool::Mute | Tool::Audition) =>
        {
            Cursor::Blocked
        }
        Spot::TempoGap => match tool {
            Tool::Draw => Cursor::Pencil,
            Tool::Eraser => Cursor::Blocked,
            Tool::Range => Cursor::Range,
            _ => Cursor::Arrow,
        },
        Spot::Tempo(_) | Spot::Line { .. } => Cursor::ResizeV,
        Spot::RangeTrim(_) | Spot::FrameGrip { .. } => Cursor::ResizeV,
        Spot::Point { .. } => Cursor::Move,
        Spot::Lane { .. } => match tool {
            Tool::Draw => Cursor::Pencil,
            Tool::Eraser => Cursor::Blocked,
            _ => Cursor::Cross,
        },
        Spot::Row(_) if started => Cursor::Move,
        // Where there is no part: the tools that act on one have nothing to act on, and the
        // pencil makes one only on an instrument track.
        Spot::Space(row) => match tool {
            Tool::Draw if matches!(row, RowKind::Track(t) if instrument(studio, *t)) => {
                Cursor::Pencil
            }
            Tool::Draw | Tool::Eraser | Tool::Cut | Tool::Mute | Tool::Audition => Cursor::Blocked,
            Tool::Range => Cursor::Range,
            Tool::Select => Cursor::Arrow,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_its_own_shape() {
        let shapes: std::collections::HashSet<_> = [
            AutoMode::Read,
            AutoMode::Touch,
            AutoMode::Latch,
            AutoMode::Write,
        ]
        .into_iter()
        .map(mode_icon)
        .collect();
        assert_eq!(shapes.len(), 4);
        for s in shapes {
            use gpui::AssetSource;
            let path = crate::assets::icon(s);
            assert!(crate::assets::Assets.load(&path).unwrap().is_some(), "{s}");
        }
    }

    #[test]
    fn a_higher_value_is_higher_in_the_lane() {
        use xirius_core::auto::AutoTarget;
        for t in [AutoTarget::Volume, AutoTarget::Pan] {
            let (lo, hi) = t.range();
            assert!(lane_value_y(&t, hi) < lane_value_y(&t, lo));
        }
    }

    #[test]
    fn a_volume_lane_goes_in_even_steps_of_db() {
        use super::super::{lane_norm, lane_value};
        use xirius_core::auto::AutoTarget;
        let t = AutoTarget::Volume;
        let y = |v: f64| lane_value_y(&t, v);
        // Every 10 dB from -40 to 0 is the same few pixels apart, enough to
        // tell apart in the lane's height.
        let steps: Vec<f32> = [-40.0, -30.0, -20.0, -10.0, 0.0]
            .windows(2)
            .map(|w| y(w[0]) - y(w[1]))
            .collect();
        for s in &steps {
            assert!((s - steps[0]).abs() < 0.01, "{steps:?}");
            assert!(*s >= 5.0, "{steps:?}");
        }
        // Silence is at the bottom, and the place gives the value back.
        assert_eq!(lane_norm(&t, xirius_core::model::MIN_DB), 0.0);
        for v in [-90.0, -60.0, -40.0, -6.5, 0.0, 12.0] {
            assert!((lane_value(&t, lane_norm(&t, v)) - v).abs() < 1e-9, "{v}");
        }
    }
}
