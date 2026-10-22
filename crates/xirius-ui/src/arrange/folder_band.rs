//! A folded folder in the lanes (ARR-003): everything its tracks hold, drawn as one band per
//! run of clips, so a closed folder still shows where its music is.

use xirius_core::id::FolderId;
use xirius_core::model::Project;
use xirius_core::time::Tick;
use xirius_design::{Radius, Space};

use crate::draw::Pen;
use crate::studio::{Geometry, Rect, Studio};
use crate::tree::Row;

/// The stretches the folder's clips cover (at any depth), in time order.
pub fn spans(project: &Project, folder: FolderId) -> Vec<(Tick, Tick)> {
    merge(
        project
            .tracks_in(folder)
            .into_iter()
            .filter_map(|t| project.tracks.get(&t))
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.length > 0)
            .map(|c| (c.start, c.start + c.length))
            .collect(),
    )
}

/// Clips that touch or overlap are one stretch.
fn merge(mut all: Vec<(Tick, Tick)>) -> Vec<(Tick, Tick)> {
    all.sort_unstable();
    let mut out: Vec<(Tick, Tick)> = Vec::new();
    for (start, end) in all {
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => out.push((start, end)),
        }
    }
    out
}

/// Draws the bands of a folded folder's row; an open folder (its tracks show) draws nothing.
pub fn paint(
    pen: &mut Pen,
    studio: &Studio,
    project: &Project,
    g: &Geometry,
    row: &Row,
    f: FolderId,
) {
    if !studio.is_collapsed(f) {
        return;
    }
    let inset = Space::X1.px();
    let top = studio.row_y(g, row) + inset;
    let h = row.h - inset * 2.0;
    for (start, end) in spans(project, f) {
        let x = studio.tick_x(g, start);
        let w = studio.ticks_to_px(end - start);
        if x + w < g.lanes.x || x > g.lanes.right() {
            continue;
        }
        pen.round(Rect::new(x, top, w, h), pen.c.clip, Radius::Clip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arrange::probe::folder;

    #[test]
    fn clips_that_touch_or_overlap_make_one_band() {
        assert_eq!(
            merge(vec![
                (960, 1920),
                (0, 960),
                (1000, 1100),
                (3000, 4000),
                (3500, 3600)
            ]),
            [(0, 1920), (3000, 4000)]
        );
        assert!(merge(Vec::new()).is_empty());
    }

    #[test]
    fn a_folder_band_lies_over_the_clips_of_every_depth() {
        let s = Studio::sample();
        let p = s.project();
        let rhythm = folder(&s, "Rhythm");
        let bands = spans(&p, rhythm);
        let tracks = p.tracks_in(rhythm);
        assert!(tracks.len() > 1, "the folder holds several tracks");
        let mut clips = 0;
        for t in &tracks {
            for c in &p.tracks[t].clips {
                clips += 1;
                assert!(
                    bands
                        .iter()
                        .any(|(a, b)| *a <= c.start && c.start + c.length <= *b),
                    "{} lies under a band",
                    c.name
                );
            }
        }
        assert!(clips > 0, "the sample folder holds clips");
        assert!(bands.windows(2).all(|w| w[0].1 < w[1].0));
    }
}
