//! Several parts in the roll at once (EDT-062): the MIDI clips selected in the arrangement with
//! the roll's clip, on its track or on others, are drawn with it where they sit in the song, and
//! what the keys do to the selected notes (move, transpose, delete, quantize, mute) reaches the
//! selected notes of every one of them in one step. Mod+Shift+A selects every note of every part
//! on the tracks of those parts.

use std::collections::BTreeSet;

use serde_json::json;
use xirius_core::id::{NoteId, TrackId};
use xirius_core::model::{Clip, ClipBody, Note};
use xirius_core::time::Tick;
use xirius_design::GridTier;

use super::frame::{Frame, View};
use super::{Ctx, NoteView, Roll};
use crate::draw::Pen;
use crate::grid_lines::Line;
use crate::studio::{Geometry, Rect, Studio};

/// A part edited with the roll's clip.
#[derive(Clone, Debug)]
pub struct Part {
    pub track: TrackId,
    pub clip: Clip,
}

impl Part {
    /// Where the part starts, from the head of the roll's clip.
    pub fn offset(&self, ctx: &Ctx) -> Tick {
        self.clip.start - ctx.clip.start
    }
}

impl Ctx {
    /// Every note of the other parts, with its part.
    pub fn part_notes(&self) -> impl Iterator<Item = (&Part, &Note)> {
        self.others
            .iter()
            .flat_map(|p| p.clip.notes().iter().map(move |n| (p, n)))
    }
}

impl Roll {
    /// The selected notes of the other parts.
    pub fn selected_elsewhere<'a>(&self, ctx: &'a Ctx) -> Vec<(&'a Part, &'a Note)> {
        ctx.part_notes()
            .filter(|(_, n)| self.selected.contains(&n.id))
            .collect()
    }

    /// The ids of the selected notes of the other parts.
    pub fn ids_elsewhere(&self, ctx: &Ctx) -> Vec<NoteId> {
        self.selected_elsewhere(ctx)
            .into_iter()
            .map(|(_, n)| n.id)
            .collect()
    }

    /// The notes a key moves by `by` ticks, each held inside its own part: (head room, tail
    /// room) over every selected note of every part, as the most each may go back and forth.
    pub(super) fn room(&self, ctx: &Ctx) -> Option<(Tick, Tick)> {
        let mut spans: Vec<(Tick, Tick)> = self
            .selected_notes(ctx)
            .iter()
            .map(|n| (n.start, ctx.clip_len() - 1 - n.start))
            .collect();
        spans.extend(
            self.selected_elsewhere(ctx)
                .iter()
                .map(|(p, n)| (n.start, p.clip.length - 1 - n.start)),
        );
        if spans.is_empty() {
            return None;
        }
        let back = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let ahead = spans.iter().map(|s| s.1).min().unwrap_or(0);
        Some((back, ahead.max(-back)))
    }
}

impl Studio {
    /// The MIDI clips selected in the arrangement with the roll's clip (EDT-062).
    pub fn roll_parts(&self) -> Vec<Part> {
        let Some((_, target)) = self.selected_clip else {
            return Vec::new();
        };
        if !self.arrange.clips.iter().any(|(_, c)| *c == target) {
            return Vec::new();
        }
        let p = self.project();
        self.arrange
            .clips
            .iter()
            .filter(|(_, c)| *c != target)
            .filter_map(|(t, c)| {
                let clip = p.tracks.get(t)?.clips.iter().find(|k| k.id == *c)?;
                matches!(clip.body, ClipBody::Midi { .. }).then(|| Part {
                    track: *t,
                    clip: clip.clone(),
                })
            })
            .collect()
    }

    /// Mod+Shift+A in the roll: every part on the track of the roll's clip, and on the tracks of
    /// the parts edited with it, is selected in the arrangement, and every note in them.
    pub(crate) fn roll_select_on_tracks(&mut self, g: &Geometry) -> bool {
        let Some(ctx) = self.roll_ctx(g) else {
            return false;
        };
        let mut ids = vec![ctx.clip.id.to_string()];
        ids.extend(ctx.others.iter().map(|p| p.clip.id.to_string()));
        if self
            .arrange_run("select.on-tracks", json!({ "ids": ids }))
            .is_none()
        {
            return true;
        }
        self.take_clips();
        self.arrange.touched();
        if let Some(ctx) = self.roll_ctx(g) {
            let mut all: BTreeSet<NoteId> = ctx.notes().iter().map(|n| n.id).collect();
            all.extend(ctx.part_notes().map(|(_, n)| n.id));
            self.roll.selected = all;
            self.roll.points.clear();
        }
        true
    }
}

/// The lines of the roll's grid with the other parts' edges among them, in the roll's ticks.
/// An edge is a bar line (UI-339). On a line of the grid it takes that line's place, and parts
/// meeting at one time share one: the color is translucent, so a time painted twice is darker.
pub(super) fn edged(mut lines: Vec<Line>, ctx: &Ctx) -> Vec<Line> {
    for p in &ctx.others {
        let at = p.offset(ctx);
        for tick in [at, at + p.clip.length] {
            let edge = Line {
                tick,
                tier: GridTier::Bar,
                fade: 1.0,
            };
            match lines.binary_search_by_key(&tick, |l| l.tick) {
                Ok(i) => lines[i] = edge,
                Err(i) => lines.insert(i, edge),
            }
        }
    }
    lines
}

/// The other parts behind the roll's clip: a tint over each one's span between its edges (bar
/// lines among the grid's, [`edged`]), and its notes in its track's color, faint unless selected.
pub(super) fn paint(pen: &mut Pen, roll: &Roll, ctx: &Ctx, v: &View, f: &Frame) {
    for part in &ctx.others {
        let color = ctx
            .project
            .tracks
            .get(&part.track)
            .map_or(pen.c.tracks[0], |t| {
                pen.c.tracks[crate::draw::track_color(t)]
            });
        let at = part.offset(ctx);
        let x0 = v.x(at).max(f.grid.x);
        let x1 = v.x(at + part.clip.length).min(f.grid.right());
        if x1 <= f.grid.x || x0 >= f.grid.right() {
            continue;
        }
        pen.fill(
            Rect::new(x0, f.grid.y, x1 - x0, f.grid.h),
            color.opacity(0.08),
        );
        for n in part.clip.notes() {
            let view = NoteView {
                start: n.start + at,
                ..NoteView::from(n)
            };
            let face = if roll.selected.contains(&n.id) {
                pen.c.ink_strong
            } else {
                color.opacity(0.55)
            };
            super::paint::note(pen, v, view, face);
        }
    }
}
