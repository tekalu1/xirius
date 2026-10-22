//! The tracks a running bounce makes (EXP-047): while 「新しいトラックにバウンス」, a range or a
//! track bounce runs, each new track already has its place below the track it goes under, with
//! its clips where they will be. They fill from the left as the bounce goes, carry how far it is
//! and the square that stops it. Stopped, the places close and the song is as it was.

use std::collections::HashMap;

use xirius_core::id::TrackId;
use xirius_core::model::Project;
use xirius_design::{FontSize, Layout, Radius, Space, Weight};
use xirius_session::bounce::Planned;

use crate::clip_marks::Status;
use crate::draw::{Align, Pen};
use crate::studio::{Geometry, Rect, RowParts, Studio};
use crate::tree::{Row, RowKind};

/// What the chip on a clip being made says.
pub const WORKING: &str = "バウンス中";

/// How high the area under each track is: a row of the song's track height for each track
/// the bounce puts below it.
pub fn heights(studio: &Studio) -> HashMap<TrackId, f32> {
    let mut out = HashMap::new();
    if let Some(job) = &studio.job {
        for t in &job.planned {
            *out.entry(t.after).or_insert(0.0) += studio.arrange.track_h;
        }
    }
    out
}

/// One new track in its place: what it is, and its row as the screen lays it out.
pub struct Place<'a> {
    pub planned: &'a Planned,
    /// A row of its own height at its place (relative to the top of the track column).
    pub row: Row,
}

/// The new tracks that go below `row`, in order.
pub fn places<'a>(studio: &'a Studio, row: &Row) -> Vec<Place<'a>> {
    let (Some(job), RowKind::Track(t)) = (&studio.job, row.kind) else {
        return Vec::new();
    };
    if row.job_h <= 0.0 {
        return Vec::new();
    }
    let h = studio.arrange.track_h;
    job.planned
        .iter()
        .filter(|p| p.after == t)
        .enumerate()
        .map(|(i, planned)| Place {
            planned,
            row: Row {
                y: row.job_y() + i as f32 * h,
                h,
                tall: false,
                lanes: 0,
                rec_h: 0.0,
                job_h: 0.0,
                ..*row
            },
        })
        .collect()
}

/// Where the clips of a new track go on the screen.
pub fn clip_rects(studio: &Studio, g: &Geometry, place: &Place) -> Vec<Rect> {
    let inset = Space::X1.px();
    let top = studio.row_y(g, &place.row);
    place
        .planned
        .spans
        .iter()
        .map(|&(from, to)| {
            Rect::new(
                studio.tick_x(g, from),
                top + inset,
                studio.ticks_to_px(to - from),
                place.row.h - inset * 2.0,
            )
        })
        .collect()
}

/// The squares that stop the bounce, on the screen.
pub fn stop_rects(studio: &Studio, g: &Geometry) -> Vec<Rect> {
    studio
        .tree()
        .rows
        .iter()
        .flat_map(|row| places(studio, row))
        .flat_map(|place| clip_rects(studio, g, &place))
        .filter_map(crate::clip_marks::stop_rect)
        .collect()
}

/// The colour a new track takes: the one of the track it is made from, else none of its own.
fn color(pen: &Pen, project: &Project, planned: &Planned) -> gpui::Hsla {
    let c = pen.c;
    match planned.like.and_then(|t| project.tracks.get(&t)) {
        Some(t) => crate::draw::color_of(c, t),
        None => c.ink,
    }
}

/// The track column's part: the colour, the kind and the name of each new track, faint.
pub fn paint_head(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry, row: &Row) {
    for place in places(studio, row) {
        let parts = RowParts::of(&place.row);
        let top = studio.row_y(g, &place.row);
        let at = |r: Rect| Rect::new(g.column.x + r.x, top + r.y, r.w, r.h);
        let c = pen.c;
        let faint = 0.6;
        if let Some(cell) = parts.swatch {
            let cell = at(cell);
            let m = RowParts::SWATCH_MARK;
            let mark = Rect::new(
                cell.x + (cell.w - m) / 2.0,
                cell.y + (cell.h - m) / 2.0,
                m,
                m,
            );
            pen.round(
                mark,
                color(pen, project, place.planned).opacity(faint),
                Radius::Xs,
            );
        }
        pen.icon("audio", at(parts.icon), c.ink.opacity(faint));
        let name_r = Rect::new(
            g.column.x + parts.name_x,
            top + RowParts::LINE_1_Y,
            parts.name_right - parts.name_x,
            Layout::ROW_ICON,
        );
        pen.mask(name_r, |pen| {
            pen.text(
                &place.planned.name,
                FontSize::Body,
                Weight::Medium,
                pen.c.ink,
                name_r,
                Align::Left,
            );
        });
    }
}

/// The lanes' part: the clips of each new track, filled from the left as far as the bounce
/// has gone, with how far it is and the square that stops it.
pub fn paint_lanes(pen: &mut Pen, studio: &Studio, project: &Project, g: &Geometry, row: &Row) {
    let Some(job) = &studio.job else {
        return;
    };
    let fraction = job.fraction().clamp(0.0, 1.0);
    let status = Status::Working {
        label: Some(WORKING),
        percent: (fraction * 100.0).floor() as u32,
        stop: true,
    };
    for place in places(studio, row) {
        let rects = clip_rects(studio, g, &place);
        let (Some(x0), Some(x1)) = (
            rects.iter().map(|r| r.x).reduce(f32::min),
            rects.iter().map(|r| r.right()).reduce(f32::max),
        ) else {
            continue;
        };
        let done = x0 + (x1 - x0) * fraction;
        let color = color(pen, project, place.planned);
        for r in rects {
            if r.right() < g.lanes.x || r.x > g.lanes.right() {
                continue;
            }
            pen.round(r, pen.c.clip, Radius::Clip);
            let w = (done - r.x).clamp(0.0, r.w);
            if w > 0.0 {
                pen.mask(Rect::new(r.x, r.y, w, r.h), |pen| {
                    pen.round(r, color.opacity(0.35), Radius::Clip);
                });
            }
            crate::clip_marks::paint_status(pen, status, r);
        }
    }
}

impl Studio {
    /// A press on the square of a new track's clip stops the bounce. True when it did.
    pub(crate) fn job_row_stop_press(&mut self, g: &Geometry, x: f32, y: f32) -> bool {
        g.lanes.contains(x, y)
            && stop_rects(self, g).iter().any(|r| r.contains(x, y))
            && self.job_cancel()
    }
}
