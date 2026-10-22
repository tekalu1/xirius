//! The notes the grid can show (UI-036): found through the clip's span index, not by looking at
//! every note of a long clip each frame.

use super::frame::View;
use super::{Ctx, Roll};
use xirius_core::model::{ClipBody, Note};
use xirius_core::time::Tick;

impl Roll {
    /// The notes of `ctx.clip` that can be on screen in `v`, in the order of the clip, and the
    /// ones a drag shows somewhere else. Some of them may still be just off the grid.
    pub fn visible_notes<'a>(&self, ctx: &'a Ctx, v: &View) -> Vec<&'a Note> {
        let ClipBody::Midi { notes: shared, .. } = &ctx.clip.body else {
            return Vec::new();
        };
        let notes: &[Note] = shared;
        let index = self.spatial.of(shared);
        // A note is drawn at least two pixels wide, so one starting just before the grid shows.
        // A hit of the drum view is a square centered on its start.
        let pad = if v.drums() { v.row_h / 2.0 + 2.0 } else { 2.0 };
        let from = v.tick_f(v.grid.x - pad).floor() as Tick - 1;
        let to = v.tick_f(v.grid.right() + pad - 2.0).ceil() as Tick + 1;
        index.visible(notes, from, to, self.preview.changed.keys())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use serde_json::json;

    use super::*;
    use crate::roll::NoteView;
    use crate::roll::notes::note_rect;
    use crate::studio::{Geometry, Studio};
    use xirius_core::id::NoteId;

    /// A song whose open clip holds `count` notes over `beats` beats (some long, most short).
    fn song_with(count: usize, beats: i64) -> (Studio, Geometry) {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        let ctx = s.roll_ctx(&g).unwrap();
        let ppq = ctx.ppq();
        s.song
            .run(
                "clip.resize",
                json!({"ids": [ctx.clip.id.to_string()], "length": (beats + 8) * ppq}),
            )
            .unwrap();
        let notes: Vec<_> = (0..count as i64)
            .map(|i| {
                let at = i * beats * ppq / count as i64;
                let length = if i % 17 == 0 { ppq * 12 } else { ppq / 4 };
                json!({"at": at, "pitch": 36 + (i * 7) % 60, "length": length, "velocity": 90})
            })
            .collect();
        s.song
            .run(
                "note.put",
                json!({"clip": ctx.clip.id.to_string(), "notes": notes}),
            )
            .unwrap();
        s.resized(&g);
        (s, g)
    }

    /// What the grid draws of `notes`: the rule `Notes::paint` culls by.
    fn drawn<'a>(roll: &Roll, v: &View, notes: impl Iterator<Item = &'a Note>) -> Vec<NoteId> {
        notes
            .filter(|n| {
                let r = note_rect(v, &roll.shown(n));
                !(r.right() < v.grid.x
                    || r.x >= v.grid.right()
                    || r.bottom() < v.grid.y
                    || r.y >= v.grid.bottom())
            })
            .map(|n| n.id)
            .collect()
    }

    #[test]
    fn what_is_drawn_is_the_same_as_with_a_full_scan() {
        let (mut s, g) = song_with(20_000, 4_000);
        for (beat_px, scroll_x) in [
            (40.0, 0.0),
            (40.0, 30_000.0),
            (700.0, 90_000.0),
            (4.0, 5_000.0),
        ] {
            s.roll.beat_px = beat_px;
            s.roll.scroll.0 = scroll_x;
            let ctx = s.roll_ctx(&g).unwrap();
            let v = s.roll.view(&ctx);
            let fast = s.roll.visible_notes(&ctx, &v);
            assert!(fast.len() <= ctx.notes().len());
            assert_eq!(
                drawn(&s.roll, &v, fast.iter().copied()),
                drawn(&s.roll, &v, ctx.notes().iter()),
                "beat_px {beat_px}, scroll {scroll_x}"
            );
        }
    }

    #[test]
    fn a_zoomed_in_grid_looks_at_few_notes() {
        let (mut s, g) = song_with(20_000, 4_000);
        s.roll.beat_px = 120.0;
        s.roll.scroll.0 = 60_000.0;
        let ctx = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&ctx);
        let fast = s.roll.visible_notes(&ctx, &v);
        assert!(
            !fast.is_empty() && fast.len() < 400,
            "{} of {}",
            fast.len(),
            ctx.notes().len()
        );
    }

    #[test]
    fn a_dragged_note_is_found_where_the_drag_shows_it() {
        let (mut s, g) = song_with(20_000, 4_000);
        s.roll.beat_px = 120.0;
        s.roll.scroll.0 = 0.0;
        let ctx = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&ctx);
        let far = ctx.notes().last().unwrap();
        let mut view = NoteView::from(far);
        view.start = 480;
        s.roll.preview.changed.insert(far.id, view);
        let fast = s.roll.visible_notes(&ctx, &v);
        assert!(fast.iter().any(|n| n.id == far.id));
        assert_eq!(
            drawn(&s.roll, &v, fast.iter().copied()),
            drawn(&s.roll, &v, ctx.notes().iter())
        );
    }

    #[test]
    fn the_index_is_built_once_for_one_song_and_again_after_an_edit() {
        let (s, g) = song_with(2_000, 400);
        let ctx = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&ctx);
        for _ in 0..5 {
            s.roll.visible_notes(&ctx, &v);
        }
        assert_eq!(s.roll.spatial.builds(), 1);
        let mut s = s;
        // An edit of another track leaves the clip's notes (and so the index) as they were.
        let other = ctx
            .project
            .tracks
            .keys()
            .find(|t| **t != s.selected_clip.unwrap().0)
            .unwrap()
            .to_string();
        s.song.run("track.mute", json!({"ids": [other]})).unwrap();
        let ctx2 = s.roll_ctx(&g).unwrap();
        assert!(!Arc::ptr_eq(&ctx.project, &ctx2.project));
        s.roll.visible_notes(&ctx2, &s.roll.view(&ctx2));
        assert_eq!(s.roll.spatial.builds(), 1);
        s.song
            .run(
                "note.put",
                json!({"clip": ctx.clip.id.to_string(), "notes": [
                    {"at": 10, "pitch": 60, "length": 100, "velocity": 80}
                ]}),
            )
            .unwrap();
        let ctx = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&ctx);
        let fast = s.roll.visible_notes(&ctx, &v);
        assert_eq!(s.roll.spatial.builds(), 2);
        assert!(fast.iter().any(|n| n.start == 10 && n.length == 100));
    }

    /// The query against looking at every note, on a large song (UI-036). Prints the numbers.
    #[test]
    fn the_query_is_faster_than_a_full_scan_on_a_large_song() {
        let (mut s, g) = song_with(200_000, 40_000);
        s.roll.beat_px = 60.0;
        let frames = 40;
        let ctx = s.roll_ctx(&g).unwrap();
        let v = s.roll.view(&ctx);
        let t = Instant::now();
        s.roll.visible_notes(&ctx, &v);
        let build = t.elapsed();
        let mut query = std::time::Duration::ZERO;
        let mut scan = std::time::Duration::ZERO;
        let mut shown = 0;
        for f in 0..frames {
            s.roll.scroll.0 = 1_000.0 + f as f32 * 90_000.0;
            let ctx = s.roll_ctx(&g).unwrap();
            let v = s.roll.view(&ctx);
            let t = Instant::now();
            let fast = drawn(&s.roll, &v, s.roll.visible_notes(&ctx, &v).into_iter());
            query += t.elapsed();
            let t = Instant::now();
            let slow = drawn(&s.roll, &v, ctx.notes().iter());
            scan += t.elapsed();
            assert_eq!(fast, slow);
            shown += slow.len();
        }
        let per = |d: std::time::Duration| d.as_secs_f64() * 1000.0 / f64::from(frames);
        println!(
            "UI-036: 200000 notes, {frames} frames, {} drawn per frame; \
             built once in {build:?}; index {:.4} ms/frame, full scan {:.4} ms/frame ({:.0}x)",
            shown / frames as usize,
            per(query),
            per(scan),
            scan.as_secs_f64() / query.as_secs_f64().max(1e-9)
        );
        assert!(query * 4 < scan, "index {query:?}, scan {scan:?}");
        assert!(per(query) < 16.0);
    }
}
