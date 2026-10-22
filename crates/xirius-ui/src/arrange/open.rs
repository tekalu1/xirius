//! Opening a clip's editor from the arrangement: a double click on a clip, or
//! F2 on the selected one (EDT-042). The drawer opens on the piano roll with
//! the clip in it; Alt also zooms the roll to the clip.
//!
//! An audio clip has no editor yet: it is selected and the drawer is left as
//! it is.

use xirius_core::id::{ClipId, TrackId};
use xirius_core::model::ClipBody;

use super::*;
use crate::roll::{BEAT_PX_MAX, BEAT_PX_MIN};
use crate::studio::DrawerTab;

impl Studio {
    /// The second press of a double click at (`x`, `y`): the editor of the
    /// clip under it. Only the Arrow tool opens (the Cut, Eraser and Mute tools
    /// already acted on the first press).
    pub fn arrange_open(&mut self, g: &Geometry, x: f32, y: f32, mods: Mods) -> bool {
        let spot = self.arrange_spot(g, x, y);
        // A double click on a section is its name box (the edge is the length's).
        if let Spot::Section {
            id, resize: false, ..
        } = spot
        {
            self.arrange.drag = None;
            return self.section_rename_box(id, x, y);
        }
        if mods.shift
            && let Spot::Clip { track, .. }
            | Spot::Fx { track, .. }
            | Spot::Space(RowKind::Track(track)) = spot
        {
            self.arrange.clips = self.project().tracks[&track]
                .clips
                .iter()
                .map(|c| (track, c.id))
                .collect();
            self.arrange.drag = None;
            self.arrange.touched();
            return true;
        }
        if self.holds().tool(Area::Arrange, self.tool, mods) == Tool::Range {
            let at = self.snap_tick(self.tick_at(g, x).max(0), mods.shift);
            let p = self.project();
            let ids: Vec<_> = self
                .arrange
                .clips
                .iter()
                .map(|(_, c)| c.to_string())
                .collect();
            let range = self
                .arrange
                .ranges
                .iter()
                .copied()
                .find(|(start, end)| *start <= at && at <= *end)
                .or(self.arrange.range);
            if let Some((start, end)) = range
                && start <= at
                && at <= end
                && !ids.is_empty()
            {
                self.arrange_run(
                    "clip.split.range",
                    json!({"ids":ids,"start":start,"end":end}),
                );
            } else if let Spot::Clip { clip, .. } | Spot::Fx { clip, .. } = spot
                && p.clip(clip).is_ok()
            {
                self.arrange_run("clip.split", json!({"ids":[clip.to_string()],"at":at}));
            }
            self.arrange.drag = None;
            return true;
        }
        if self.holds().tool(Area::Arrange, self.tool, mods) != Tool::Select || mods.shift {
            return false;
        }
        match spot {
            Spot::Clip { track, clip, .. } => self.open_clip(g, track, clip, mods.alt),
            Spot::Space(RowKind::Track(track)) => self.make_part(g, track, x),
            _ => false,
        }
    }

    /// A double click on the empty lane of a track: an instrument track gets an empty part of
    /// one bar from the head of the bar under the pointer, cut before the next part, named as
    /// the track, and it is the selection. An audio track has nothing to make there (the press
    /// is still answered, so the playhead the first press moved stays).
    fn make_part(&mut self, g: &Geometry, track: TrackId, x: f32) -> bool {
        self.arrange.drag = None;
        let p = self.project();
        let Ok(t) = p.track(track) else {
            return false;
        };
        if t.kind != TrackKind::Instrument {
            return true;
        }
        let tm = p.time_map();
        let at = self.tick_at(g, x).max(0);
        let (bar, _, _) = tm.to_bbt(at);
        let before = t
            .clips
            .iter()
            .map(|c| c.start + c.length)
            .filter(|end| *end <= at)
            .max()
            .unwrap_or(0);
        let after = t
            .clips
            .iter()
            .map(|c| c.start)
            .filter(|start| *start >= at)
            .min()
            .unwrap_or(Tick::MAX);
        let start = tm.bar_start(bar).max(before);
        let end = tm.bar_start(bar + 1).min(after);
        if end <= start {
            return true;
        }
        let made = self.arrange_run(
            "clip.add.midi",
            json!({"track": track.to_string(), "at": start, "length": end - start,
                   "name": t.shown_name()}),
        );
        if let Some(id) = made.as_ref().and_then(|d| d["id"].as_str()) {
            self.select_made(&json!({"ids": [id]}));
            self.select_row(Some(RowKind::Track(track)));
        }
        true
    }

    /// F2: the editor of the selected clip.
    pub fn open_selected_clip(&mut self, g: &Geometry, fit: bool) -> bool {
        let Some((track, clip)) = self.selected_clip else {
            return false;
        };
        self.open_clip(g, track, clip, fit)
    }

    fn open_clip(&mut self, g: &Geometry, track: TrackId, clip: ClipId, fit: bool) -> bool {
        let Some(c) = self
            .project()
            .tracks
            .get(&track)
            .and_then(|t| t.clips.iter().find(|c| c.id == clip))
            .cloned()
        else {
            return false;
        };
        let key = (track, clip);
        let changed = self.selected_clip != Some(key);
        self.selected_clip = Some(key);
        if !self.arrange.clips.contains(&key) {
            self.arrange.clips = BTreeSet::from([key]);
        }
        self.select_row(Some(RowKind::Track(track)));
        self.arrange.touched();
        let ClipBody::Midi { notes, .. } = &c.body else {
            return true;
        };
        self.tab = DrawerTab::Roll;
        self.drawer_open = true;
        self.arrange.focus = false;
        self.choose_editor();
        self.rebuild(g);
        // A shut drawer slides open; the roll is fitted to the drawer it opens to.
        let g = self.settled_geometry(g.size.0, g.size.1);
        let Some(ctx) = self.roll_ctx(&g) else {
            return true;
        };
        let grid = self.roll.view(&ctx).grid;
        if changed || fit {
            self.roll.scroll.0 = 0.0;
        }
        if fit {
            let beats = (c.length as f32 / ctx.project.ppq as f32).max(1.0 / 64.0);
            self.roll.beat_px = (grid.w / beats).clamp(BEAT_PX_MIN, BEAT_PX_MAX);
        }
        if changed || fit {
            // Bring the notes into view: their pitch range in the middle.
            let (lo, hi) = notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
                (lo.min(n.pitch), hi.max(n.pitch))
            });
            if lo <= hi {
                let mid = (f32::from(lo) + f32::from(hi)) / 2.0;
                let y = (127.0 - mid + 0.5) * self.roll.row_h - grid.h / 2.0;
                let max = self.roll.max_scroll(&ctx).1;
                self.roll.scroll.1 = y.clamp(0.0, max);
            }
        }
        let max = self.roll.max_scroll(&ctx);
        self.roll.scroll.0 = self.roll.scroll.0.clamp(0.0, max.0);
        true
    }
}
