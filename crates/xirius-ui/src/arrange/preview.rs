//! What the cut and mute tools show before a press (the feel pass, item 06):
//! a line where the cut would land, and the look the clip is about to have.

use super::*;

impl Studio {
    /// The tool the pointer acts with in the arrangement now.
    fn arrange_tool(&self) -> Tool {
        self.holds()
            .tool(Area::Arrange, self.tool, self.arrange.mods)
    }

    /// Where a press with the cut tool would split the clip under the pointer
    /// (on the grid), if it would split one.
    pub fn cut_preview(&self, g: &Geometry) -> Option<(TrackId, ClipId, Tick)> {
        if self.arrange.drag.is_some() || self.arrange_tool() != Tool::Cut {
            return None;
        }
        let (x, _) = self.arrange.pointer?;
        let (track, clip) = match self.arrange.hover {
            Spot::Clip { track, clip, .. } | Spot::Fx { track, clip, .. } => (track, clip),
            _ => return None,
        };
        let project = self.project();
        let (_, c) = project.clip(clip).ok()?;
        let at = self.snap_tick(self.tick_at(g, x).max(0), self.arrange.mods.shift);
        (at > c.start && at < c.start + c.length).then_some((track, clip, at))
    }

    /// The clips the mute tool would act on now: the ones swept over while a
    /// drag is under way, else the one under the pointer.
    fn mute_targets(&self) -> Vec<ClipId> {
        match &self.arrange.drag {
            Some(d) if d.tool == Tool::Mute => self.arrange.swept.iter().map(|(_, c)| *c).collect(),
            Some(_) => Vec::new(),
            None if self.arrange_tool() == Tool::Mute => match self.arrange.hover {
                Spot::Clip { clip, .. } | Spot::Fx { clip, .. } => vec![clip],
                _ => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// Whether clip `id` is drawn muted: as it is in the song, except that the
    /// clips the mute tool is about to act on are drawn as they will be after
    /// it (`clip.mute` leaves all of them in the state the first one is not in).
    pub fn clip_muted_shown(&self, id: ClipId, muted: bool) -> bool {
        let targets = self.mute_targets();
        if !targets.contains(&id) {
            return muted;
        }
        let first = self
            .project()
            .clip(targets[0])
            .map_or(muted, |(_, c)| c.muted);
        !first
    }
}
