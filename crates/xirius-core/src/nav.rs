//! Where the play position can jump to (KEY-001, OPS-014): the start and end of the song, the
//! loop, the markers, the beat and bar lines, and the edges of clips. One resolver, so the keys,
//! the palette, the CLI and the AI all land on the same tick.

use crate::error::{CmdError, CmdResult};
use crate::model::Project;
use crate::time::Tick;

/// The names `--to` takes; `marker-N` (the Nth marker from the start) is read besides these.
pub const TARGETS: [&str; 12] = [
    "start",
    "end",
    "loop-start",
    "loop-end",
    "next-marker",
    "prev-marker",
    "next-beat",
    "prev-beat",
    "next-bar",
    "prev-bar",
    "next-event",
    "prev-event",
];

/// Where the end of the song is: the end of its last clip (0 when it has none).
pub fn song_end(p: &Project) -> Tick {
    p.tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| c.end()))
        .max()
        .unwrap_or(0)
}

/// Every clip edge on the song, sorted, without repeats.
fn event_ticks(p: &Project) -> Vec<Tick> {
    let mut ticks: Vec<Tick> = p
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().flat_map(|c| [c.start, c.end()]))
        .collect();
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

fn after(ticks: &[Tick], from: Tick, what: &str) -> CmdResult<Tick> {
    ticks
        .iter()
        .copied()
        .find(|t| *t > from)
        .ok_or_else(|| CmdError::not_found(format!("これより後ろに{what}がありません")))
}

fn before(ticks: &[Tick], from: Tick, what: &str) -> CmdResult<Tick> {
    ticks
        .iter()
        .rev()
        .copied()
        .find(|t| *t < from)
        .ok_or_else(|| CmdError::not_found(format!("これより前に{what}がありません")))
}

/// The tick `to` names, seen from `from`.
pub fn destination(p: &Project, from: Tick, to: &str) -> CmdResult<Tick> {
    let map = p.time_map();
    let from = from.max(0);
    let (bar, beat, into_beat) = map.to_bbt(from);
    let markers: Vec<Tick> = p.markers.iter().map(|m| m.tick).collect();
    if let Some(n) = to.strip_prefix("marker-") {
        let n: usize = n
            .parse()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| CmdError::invalid(format!("--to `{to}` を読み取れません")))?;
        return markers
            .get(n - 1)
            .copied()
            .ok_or_else(|| CmdError::not_found(format!("マーカー {n} がありません")));
    }
    match to {
        "start" => Ok(0),
        "end" => Ok(song_end(p)),
        "loop-start" | "loop-end" => {
            let range = p
                .loop_range
                .ok_or_else(|| CmdError::not_found("ループ範囲がありません"))?;
            Ok(if to == "loop-start" {
                range.start
            } else {
                range.end
            })
        }
        "next-marker" => after(&markers, from, "マーカー"),
        "prev-marker" => before(&markers, from, "マーカー"),
        "next-event" => after(&event_ticks(p), from, "クリップの端"),
        "prev-event" => before(&event_ticks(p), from, "クリップの端"),
        "next-beat" => Ok(map.from_bbt(bar, beat + 1, 0)),
        "prev-beat" => {
            let here = map.from_bbt(bar, beat, 0);
            if into_beat > 0 || here == 0 {
                Ok(here)
            } else {
                let (b, e, _) = map.to_bbt(here - 1);
                Ok(map.from_bbt(b, e, 0))
            }
        }
        "next-bar" => Ok(map.bar_start(bar + 1)),
        "prev-bar" => {
            if beat > 1 || into_beat > 0 || bar == 1 {
                Ok(map.bar_start(bar))
            } else {
                Ok(map.bar_start(bar - 1))
            }
        }
        other => Err(CmdError::invalid(format!(
            "--to `{other}` は使えません（{}、marker-1 のような番号のいずれか）",
            TARGETS.join("・")
        ))),
    }
}

/// Where the selection moves with an arrow key or Home / End (KEY-001, OPS-014).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
    Start,
    End,
}

impl Step {
    pub const NAMES: [&'static str; 6] = ["left", "right", "up", "down", "start", "end"];

    pub fn parse(name: &str) -> Option<Step> {
        Some(match name {
            "left" => Step::Left,
            "right" => Step::Right,
            "up" => Step::Up,
            "down" => Step::Down,
            "start" => Step::Start,
            "end" => Step::End,
            _ => return None,
        })
    }
}

/// Something that can be selected, placed by where it starts and which row it sits on (the
/// track's place from the top, or a note's pitch counted from the top).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot<T> {
    pub id: T,
    pub x: Tick,
    pub row: i64,
}

/// The selection after one step, or `None` when there is nothing further that way. `extend` keeps
/// what was selected and adds the target; otherwise the target alone is selected.
pub fn step<T: Clone + PartialEq>(
    spots: &[Spot<T>],
    picked: &[T],
    to: Step,
    extend: bool,
) -> Option<Vec<T>> {
    fn key<T>(s: &Spot<T>) -> (Tick, i64) {
        (s.x, s.row)
    }
    let held: Vec<&Spot<T>> = spots.iter().filter(|s| picked.contains(&s.id)).collect();
    let target = match (to, held.is_empty()) {
        (Step::Start, _) | (Step::Right | Step::Down, true) => spots.iter().min_by_key(|s| key(s)),
        (Step::End, _) | (Step::Left | Step::Up, true) => spots.iter().max_by_key(|s| key(s)),
        (Step::Right, false) => {
            let from = held.iter().max_by_key(|s| key(s))?;
            spots
                .iter()
                .filter(|s| s.row == from.row && s.x > from.x)
                .min_by_key(|s| s.x)
        }
        (Step::Left, false) => {
            let from = held.iter().min_by_key(|s| key(s))?;
            spots
                .iter()
                .filter(|s| s.row == from.row && s.x < from.x)
                .max_by_key(|s| s.x)
        }
        (Step::Down, false) => {
            let from = held.iter().max_by_key(|s| (s.row, s.x))?;
            let row = spots
                .iter()
                .map(|s| s.row)
                .filter(|r| *r > from.row)
                .min()?;
            spots
                .iter()
                .filter(|s| s.row == row)
                .min_by_key(|s| ((s.x - from.x).abs(), s.x))
        }
        (Step::Up, false) => {
            let from = held.iter().min_by_key(|s| (s.row, s.x))?;
            let row = spots
                .iter()
                .map(|s| s.row)
                .filter(|r| *r < from.row)
                .max()?;
            spots
                .iter()
                .filter(|s| s.row == row)
                .min_by_key(|s| ((s.x - from.x).abs(), s.x))
        }
    }?;
    let mut out: Vec<T> = if extend { picked.to_vec() } else { Vec::new() };
    if !out.contains(&target.id) {
        out.push(target.id.clone());
    }
    if out.len() == picked.len() && out.iter().all(|i| picked.contains(i)) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot(id: u32, x: Tick, row: i64) -> Spot<u32> {
        Spot { id, x, row }
    }

    fn grid() -> Vec<Spot<u32>> {
        // row 0: ids 1 (x 0), 2 (x 960), 3 (x 1920); row 1: id 4 (x 900); row 3: id 5 (x 0)
        vec![
            spot(1, 0, 0),
            spot(2, 960, 0),
            spot(3, 1920, 0),
            spot(4, 900, 1),
            spot(5, 0, 3),
        ]
    }

    #[test]
    fn with_nothing_selected_each_direction_starts_from_the_matching_end() {
        let g = grid();
        for (to, id) in [
            (Step::Right, 1),
            (Step::Down, 1),
            (Step::Start, 1),
            (Step::Left, 3),
            (Step::Up, 3),
            (Step::End, 3),
        ] {
            assert_eq!(step(&g, &[], to, false), Some(vec![id]), "{to:?}");
        }
    }

    #[test]
    fn left_and_right_stay_on_the_row_and_stop_at_its_end() {
        let g = grid();
        assert_eq!(step(&g, &[1], Step::Right, false), Some(vec![2]));
        assert_eq!(step(&g, &[2], Step::Right, false), Some(vec![3]));
        assert_eq!(step(&g, &[3], Step::Right, false), None);
        assert_eq!(step(&g, &[2], Step::Left, false), Some(vec![1]));
        assert_eq!(step(&g, &[1], Step::Left, false), None);
        // The only one on its row has nowhere to go sideways.
        assert_eq!(step(&g, &[4], Step::Right, false), None);
    }

    #[test]
    fn up_and_down_take_the_nearest_row_and_the_nearest_start() {
        let g = grid();
        assert_eq!(step(&g, &[2], Step::Down, false), Some(vec![4]));
        // A row without anything is jumped over.
        assert_eq!(step(&g, &[4], Step::Down, false), Some(vec![5]));
        assert_eq!(step(&g, &[5], Step::Down, false), None);
        assert_eq!(step(&g, &[4], Step::Up, false), Some(vec![2]));
        assert_eq!(step(&g, &[1], Step::Up, false), None);
    }

    #[test]
    fn extend_keeps_what_was_selected_and_a_step_that_adds_nothing_is_none() {
        let g = grid();
        assert_eq!(step(&g, &[1], Step::Right, true), Some(vec![1, 2]));
        assert_eq!(step(&g, &[1, 2], Step::Right, true), Some(vec![1, 2, 3]));
        // Moving on from the left end of the selection, with nothing further left, goes nowhere.
        assert_eq!(step(&g, &[1, 3], Step::Left, true), None);
        assert_eq!(step(&g, &[3], Step::Start, true), Some(vec![3, 1]));
        assert_eq!(step(&g, &[1], Step::Start, true), None);
        assert_eq!(step::<u32>(&[], &[], Step::Right, false), None);
    }
}
