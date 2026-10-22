//! The visible time grid shared by arrangement and piano roll.

use crate::tools::snap_floor;
use std::collections::BTreeSet;
use xirius_core::time::{Tick, TimeMap};

/// The least width a grid line may be spaced from the next one; closer lines
/// are thinned to every second or fourth step. Not the snap's minimum
/// ([`AUTO_MIN_PX`]): a line only has to be told apart, not aimed at.
pub const MIN_LINE_PX: f32 = 4.0;

/// How strong a grid line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rank {
    Bar,
    Beat,
    Fine,
}

/// The spacing of the lines between the beats: the step, doubled while the
/// lines would sit closer than [`MIN_LINE_PX`]. None when it reaches a beat
/// (the beat lines say it already).
pub fn fine_spacing(step: Tick, ppq: u32, beat_px: f32) -> Option<Tick> {
    let ppq = Tick::from(ppq);
    let mut spacing = step.max(1);
    while spacing < ppq && (spacing as f32 / ppq as f32 * beat_px) < MIN_LINE_PX {
        spacing *= 2;
    }
    (spacing < ppq).then_some(spacing)
}

pub fn ticks(
    tm: &TimeMap,
    first: Tick,
    last: Tick,
    step: Tick,
    ppq: u32,
    beat_px: f32,
) -> Vec<Tick> {
    let bars = crate::ruler::bars_in(tm, first, last);
    let mut out: BTreeSet<Tick> = bars.iter().map(|(_, t)| *t).collect();
    if let Some(spacing) = fine_spacing(step, ppq, beat_px) {
        out.extend((snap_floor(first, spacing)..=last).step_by(spacing as usize));
    }
    for (bar, start) in bars {
        let beat = tm.from_bbt(bar, 2, 0) - start;
        // Beats thin to every second, fourth... one when they are tight; the bar's own line stays.
        let mut every = beat.max(1);
        while (every as f32 / ppq as f32 * beat_px) < MIN_LINE_PX && every < beat * 64 {
            every *= 2;
        }
        out.extend((start..tm.bar_start(bar + 1).min(last + 1)).step_by(every as usize));
    }
    out.into_iter().collect()
}

pub fn rank(tick: Tick, tm: &TimeMap) -> Rank {
    let (bar, _, sub) = tm.to_bbt(tick.max(0));
    if tick == tm.bar_start(bar) {
        Rank::Bar
    } else if sub == 0 {
        Rank::Beat
    } else {
        Rank::Fine
    }
}

pub fn tone(tick: Tick, tm: &TimeMap, c: &crate::paint::Colors) -> gpui::Hsla {
    match rank(tick, tm) {
        Rank::Bar => c.grid[0],
        Rank::Beat => c.grid[1],
        Rank::Fine => c.grid[2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_design::{Overlay, Surface, Theme, contrast_ratio};

    #[test]
    fn fixed_grid_thins_out_but_bars_and_beats_remain() {
        let tm = TimeMap::new(960, &[]);
        let ppq = 960;
        // 16ths at 8 px a beat would be 0.5 px apart: they thin to the half beat (4 px).
        let dense = ticks(&tm, 0, ppq * 8, ppq / 16, 960, 8.0);
        assert!(dense.iter().all(|t| t % (ppq / 2) == 0));
        assert!(dense.contains(&tm.bar_start(2)));
        assert!(dense.contains(&ppq));
        let fine = ticks(&tm, 0, ppq * 4, ppq / 4, 960, 48.0);
        assert!(fine.contains(&(ppq / 4)));
    }

    #[test]
    fn lines_never_sit_closer_than_the_least_and_a_bar_always_stays() {
        let tm = TimeMap::new(960, &[]);
        let ppq = 960;
        for beat_px in [1.0_f32, 2.0, 3.0, 4.0, 5.0, 7.0, 8.0, 16.0, 40.0, 200.0] {
            for step in [ppq / 16, ppq / 8, ppq / 4, ppq / 2, ppq, ppq * 4] {
                let lines = ticks(&tm, 0, ppq * 40, step, 960, beat_px);
                for bar in 1..10 {
                    assert!(lines.contains(&tm.bar_start(bar)), "{beat_px} {step}");
                }
                for w in lines.windows(2) {
                    let px = (w[1] - w[0]) as f32 / ppq as f32 * beat_px;
                    // A bar at the end of a thinned run may sit closer; nothing else may.
                    assert!(
                        px >= MIN_LINE_PX || tm.bar_start(tm.to_bbt(w[1]).0) == w[1],
                        "{beat_px} {step}: {px}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_tight_beat_thins_to_every_second_or_fourth_beat() {
        let tm = TimeMap::new(960, &[]);
        let ppq = 960;
        let two = ticks(&tm, 0, ppq * 8, ppq, 960, 3.0);
        assert!(two.iter().all(|t| t % (ppq * 2) == 0) && two.contains(&(ppq * 2)));
        let four = ticks(&tm, 0, ppq * 8, ppq, 960, 1.0);
        assert!(four.iter().all(|t| t % (ppq * 4) == 0));
    }

    #[test]
    fn a_finer_step_than_the_snaps_minimum_is_still_drawn() {
        // 1/16 at 20 px a beat is 5 px: under the snap's 7 px, over the line's 4.
        assert_eq!(fine_spacing(240, 960, 20.0), Some(240));
        assert_eq!(fine_spacing(240, 960, 8.0), Some(480));
        assert_eq!(fine_spacing(240, 960, 3.0), None);
    }

    #[test]
    fn bar_beat_and_subdivision_use_three_descending_strengths() {
        let tm = TimeMap::new(960, &[]);
        let c = crate::paint::Colors::new(crate::theme::THEME);
        assert_eq!(rank(0, &tm), Rank::Bar);
        assert_eq!(rank(960, &tm), Rank::Beat);
        assert_eq!(rank(240, &tm), Rank::Fine);
        assert_eq!(tone(0, &tm, &c), c.grid[0]);
        assert_eq!(tone(960, &tm, &c), c.grid[1]);
        assert_eq!(tone(240, &tm, &c), c.grid[2]);
    }

    #[test]
    fn the_three_strengths_read_in_order_against_the_lane_in_both_themes() {
        // The mock measured dark 1.98 / 1.43 / 1.20 and light 2.21 / 1.59 / 1.34.
        for (theme, want) in [
            (Theme::Dark, [1.98, 1.43, 1.20]),
            (Theme::Light, [2.21, 1.59, 1.34]),
        ] {
            let paper = Surface::Paper.color(theme);
            let lines = [
                Overlay::ScrollThumbHover.color(theme),
                Overlay::ScrollThumb.color(theme),
                Surface::S3.color(theme),
            ];
            let mut last = f64::MAX;
            for (i, (line, want)) in lines.iter().zip(want).enumerate() {
                let ratio = contrast_ratio(line.over(paper), paper);
                assert!(
                    (ratio - want).abs() < 0.1,
                    "{theme:?} line {i}: {ratio:.2} (want {want})"
                );
                assert!(ratio < last);
                last = ratio;
            }
        }
    }
}
