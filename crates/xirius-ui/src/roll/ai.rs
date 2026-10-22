//! The AI's notes in the roll (AI-030, AI-036): a purple edge on what it
//! changed, a flicker and a running band while it works, and a glide from
//! where a note was to where the AI put it.

use gpui::Hsla;
use xirius_core::id::ClipId;
use xirius_core::model::Note;

use super::NoteView;
use super::notes::note_rect;
use crate::ai::GlideAt;
use crate::draw::{Pen, inset};
use crate::studio::{Rect, Studio};

/// How wide the AI's edge on a note is.
const EDGE: f32 = 1.0;
/// The band inside a note the AI works on: its width (a part of the note) and strength.
const BAND_W: f32 = 0.2;
const BAND_ALPHA: f32 = 0.16;
/// How much later each beat's notes flicker, so the flicker runs along the clip.
const STAGGER_MS_PER_BEAT: i64 = 45;

/// What the roll asks for each note.
pub(super) struct Notes<'a> {
    studio: &'a Studio,
    clip: ClipId,
    ppq: i64,
    working: bool,
    band: f32,
}

impl<'a> Notes<'a> {
    pub(super) fn new(studio: &'a Studio, clip: ClipId, ppq: i64) -> Notes<'a> {
        Notes {
            studio,
            clip,
            ppq: ppq.max(1),
            working: studio.ai_working() && !studio.ai.motion.reduced,
            band: studio.ai.scan(BAND_W),
        }
    }

    /// Something moves: the notes may cross, so they are not batched.
    pub(super) fn moving(&self) -> bool {
        self.studio.ai_moving()
    }

    /// One note, with the AI's edge, flicker and glide. `shown` is the
    /// note as the roll would draw it.
    pub(super) fn paint(
        &self,
        pen: &mut Pen,
        v: &super::frame::View,
        n: &Note,
        shown: NoteView,
        color: Hsla,
    ) {
        let to = note_rect(v, &shown);
        let r = match self.studio.ai.glide(self.clip, n.id, shown) {
            GlideAt::Still => to,
            GlideAt::Moving(from, t) => lerp(note_rect(v, &from), to, t),
        };
        if r.right() < v.grid.x
            || r.x >= v.grid.right()
            || r.bottom() < v.grid.y
            || r.y >= v.grid.bottom()
        {
            return;
        }
        let mut body = Rect::new(r.x, r.y + 1.0, r.w, r.h - 2.0);
        let mut face = if shown.muted { pen.c.surface_3 } else { color };
        let editing = self.working && self.studio.ai_editing(&n.id.to_string());
        if editing {
            let beat = n.start.max(0) / self.ppq;
            let (alpha, tall) = self.studio.ai.flicker((beat * STAGGER_MS_PER_BEAT) as u64);
            let h = body.h * tall;
            body = Rect::new(body.x, body.y + (body.h - h) / 2.0, body.w, h);
            face = face.opacity(alpha);
        }
        if n.ai || self.studio.ai_marked(&n.id.to_string()) {
            super::paint::blob(pen, v, body, pen.c.ai_line);
            super::paint::blob(pen, v, inset(body, EDGE), face);
        } else {
            super::paint::blob(pen, v, body, face);
        }
        if editing {
            let band = pen.c.ai_line.opacity(BAND_ALPHA);
            pen.mask(body, |pen| {
                pen.fill(
                    Rect::new(body.x + self.band * body.w, body.y, body.w * BAND_W, body.h),
                    band,
                );
            });
        }
    }
}

fn lerp(a: Rect, b: Rect, t: f32) -> Rect {
    let m = |x: f32, y: f32| x + (y - x) * t;
    Rect::new(m(a.x, b.x), m(a.y, b.y), m(a.w, b.w), m(a.h, b.h))
}
