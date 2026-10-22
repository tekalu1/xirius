//! What the record panel makes of a recording (REC-007, REC-009): where the transport starts
//! (the pre-roll), which stretch of what is captured is kept (the auto-punch range), where it
//! stops by itself (the post-roll) and how played notes are snapped (input quantize).
//!
//! The engine captures everything from the first frame to the stop; the takes are cut down to
//! the stretch kept afterwards, so a take lies over the original only where the punch was open.
//! The files hold the whole capture; a clip names the part of it that it uses.

use serde_json::{Value, json};
use xirius_core::model::Project;
use xirius_core::time::Tick;
use xirius_prefs::Prefs;

/// The record panel's settings, read from the preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    pub punch: bool,
    pub preroll: Option<u32>,
    pub postroll: Option<u32>,
    /// The grid note heads snap to, in ticks.
    pub quantize: Option<Tick>,
}

impl Panel {
    pub fn from_prefs(prefs: &Prefs, ppq: u32) -> Panel {
        let bars = |on: &str, n: &str| {
            prefs
                .bool(on)
                .then(|| prefs.number(n).clamp(1.0, 8.0) as u32)
        };
        let punch = prefs.bool("record.punch");
        Panel {
            punch,
            preroll: bars("record.preroll", "record.preroll-bars"),
            postroll: bars("record.postroll", "record.postroll-bars").filter(|_| punch),
            quantize: prefs
                .bool("edit.record.input-quantize")
                .then(|| grid_ticks(&prefs.text("edit.record.input-quantize-value"), ppq)),
        }
    }
}

/// The ticks of an input quantize value: `1/4`, `1/8`, `1/16` or `1/8T` (a third of a quarter).
pub fn grid_ticks(value: &str, ppq: u32) -> Tick {
    let q = Tick::from(ppq);
    match value {
        "1/4" => q,
        "1/8" => q / 2,
        "1/8T" => q / 3,
        _ => q / 4,
    }
    .max(1)
}

/// What the ruler shows of the auto-punch: the range (the loop range) and the grey stretches the
/// pre-roll and the post-roll add before and after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bands {
    pub range: (Tick, Tick),
    pub pre: Option<(Tick, Tick)>,
    pub post: Option<(Tick, Tick)>,
}

/// The bands of the panel's auto-punch, or none while it is off or there is no range.
pub fn bands(p: &Project, panel: &Panel) -> Option<Bands> {
    let l = p.loop_range.filter(|l| panel.punch && l.end > l.start)?;
    Some(Bands {
        range: (l.start, l.end),
        pre: panel.preroll.map(|n| (before(p, l.start, n), l.start)),
        post: panel.postroll.map(|n| (l.end, after(p, l.end, n))),
    })
}

/// One recording as the panel plans it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Play from here (`None`: from where the transport is, or on if it plays).
    pub at: Option<Tick>,
    /// The stretch kept: from, and to (`None`: until the stop).
    pub keep: Option<(Tick, Option<Tick>)>,
    /// Stop by itself at this tick (`None`: only when asked to).
    pub stop_at: Option<Tick>,
    pub quantize: Option<Tick>,
}

fn bar_of(p: &Project, tick: Tick) -> u32 {
    p.time_map().to_bbt(tick).0
}

/// `bars` bars before `tick`, from the start of the bar it lies in, not before the song.
fn before(p: &Project, tick: Tick, bars: u32) -> Tick {
    let tm = p.time_map();
    tm.bar_start(bar_of(p, tick).saturating_sub(bars).max(1))
}

/// `bars` bars after `tick`, at the same place in its bar.
fn after(p: &Project, tick: Tick, bars: u32) -> Tick {
    let tm = p.time_map();
    let bar = bar_of(p, tick);
    tm.bar_start(bar + bars) + (tick - tm.bar_start(bar))
}

/// The plan of a recording pressed at `from` (the playhead), the transport `playing` or not and
/// the loop `looping` or not. The auto-punch range is the loop range (one band on the ruler).
pub fn plan(p: &Project, panel: &Panel, from: Tick, playing: bool, looping: bool) -> Plan {
    let range = p
        .loop_range
        .filter(|l| panel.punch && l.end > l.start)
        .map(|l| (l.start, l.end));
    let mut out = Plan {
        quantize: panel.quantize,
        ..Plan::default()
    };
    match range {
        Some((a, b)) => {
            out.keep = Some((a, Some(b)));
            if !playing {
                out.at = Some(panel.preroll.map_or(a, |n| before(p, a, n)));
            }
            // A loop goes on lap after lap until it is stopped.
            if !looping {
                out.stop_at = Some(panel.postroll.map_or(b, |n| after(p, b, n)));
            }
        }
        None => {
            if playing {
                return out;
            }
            out.at = Some(panel.preroll.map_or(from, |n| before(p, from, n)));
            if panel.preroll.is_some() {
                out.keep = Some((from, None));
            }
        }
    }
    out
}

fn snap(tick: Tick, grid: Tick) -> Tick {
    (tick + grid / 2).div_euclid(grid) * grid
}

/// Cuts the takes down to `keep` and snaps the heads of the notes to `grid`. A take that nothing
/// is left of goes; the second is how many did.
///
/// An audio take keeps the part of its file inside: its clip starts later and names where in the
/// file with `offset`. A MIDI take keeps the notes whose heads are inside (a note held past the
/// end is cut there) and, snapped, takes the heads' places; the lengths stay.
pub fn shape(
    takes: Vec<Value>,
    keep: Option<(Tick, Option<Tick>)>,
    grid: Option<Tick>,
) -> (Vec<Value>, usize) {
    let mut out = Vec::new();
    let mut gone = 0;
    for mut t in takes {
        let (Some(start), Some(length)) = (t["start"].as_i64(), t["length"].as_i64()) else {
            out.push(t);
            continue;
        };
        let end = start + length;
        let from = keep.map_or(start, |k| k.0.max(start));
        let to = keep.and_then(|k| k.1).map_or(end, |to| to.min(end));
        if t.get("notes").is_none() {
            if keep.is_none() {
                out.push(t);
            } else if to > from {
                let before = t["offset"].as_i64().unwrap_or(0);
                t["start"] = json!(from);
                t["length"] = json!(to - from);
                t["offset"] = json!(before + (from - start));
                out.push(t);
            } else {
                gone += 1;
            }
            continue;
        }
        let notes: Vec<(Tick, Tick, i64, i64)> = t["notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| {
                let head = start + n["start"].as_i64()?;
                Some((
                    grid.map_or(head, |g| snap(head, g)),
                    n["length"].as_i64()?,
                    n["pitch"].as_i64()?,
                    n["velocity"].as_i64().unwrap_or(100),
                ))
            })
            .filter(|n| match keep {
                Some((a, b)) => n.0 >= a && b.is_none_or(|b| n.0 < b),
                None => true,
            })
            .collect();
        let Some(first) = notes.iter().map(|n| n.0).min() else {
            gone += 1;
            continue;
        };
        let limit = keep.and_then(|k| k.1);
        let notes: Vec<_> = notes
            .into_iter()
            .map(|(head, len, pitch, vel)| {
                (
                    head,
                    limit.map_or(len, |b| len.min(b - head)).max(1),
                    pitch,
                    vel,
                )
            })
            .collect();
        let new_start = first.min(from);
        let last = notes.iter().map(|n| n.0 + n.1).max().unwrap_or(to);
        let new_end = to.max(last).max(new_start + 1);
        t["start"] = json!(new_start);
        t["length"] = json!(new_end - new_start);
        t["notes"] = json!(
            notes
                .iter()
                .map(|&(head, len, pitch, vel)| json!({
                    "start": head - new_start,
                    "length": len,
                    "pitch": pitch,
                    "velocity": vel,
                }))
                .collect::<Vec<_>>()
        );
        out.push(t);
    }
    (out, gone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xirius_core::model::LoopRange;

    fn song() -> Project {
        Project::new("t")
    }

    fn panel() -> Panel {
        Panel {
            punch: false,
            preroll: None,
            postroll: None,
            quantize: None,
        }
    }

    const BAR: Tick = 4 * xirius_core::time::DEFAULT_PPQ as Tick;

    #[test]
    fn the_panel_is_what_the_settings_say() {
        let ppq = xirius_core::time::DEFAULT_PPQ;
        let mut prefs = Prefs::default();
        assert_eq!(Panel::from_prefs(&prefs, ppq).quantize, None);
        prefs
            .set("edit.record.input-quantize", &serde_json::json!(true))
            .unwrap();
        prefs
            .set(
                "edit.record.input-quantize-value",
                &serde_json::json!("1/8"),
            )
            .unwrap();
        prefs.set("record.punch", &serde_json::json!(true)).unwrap();
        prefs
            .set("record.preroll", &serde_json::json!(true))
            .unwrap();
        prefs
            .set("record.preroll-bars", &serde_json::json!(3))
            .unwrap();
        let panel = Panel::from_prefs(&prefs, ppq);
        assert_eq!(panel.quantize, Some(Tick::from(ppq / 2)));
        assert!(panel.punch);
        assert_eq!(panel.preroll, Some(3));
    }

    #[test]
    fn grids_are_the_note_values() {
        assert_eq!(grid_ticks("1/4", 480), 480);
        assert_eq!(grid_ticks("1/8", 480), 240);
        assert_eq!(grid_ticks("1/16", 480), 120);
        assert_eq!(grid_ticks("1/8T", 480), 160);
    }

    #[test]
    fn nothing_set_records_from_where_it_is() {
        let p = song();
        assert_eq!(
            plan(&p, &panel(), 700, false, false),
            Plan {
                at: Some(700),
                ..Plan::default()
            }
        );
        assert_eq!(plan(&p, &panel(), 700, true, false), Plan::default());
    }

    #[test]
    fn the_preroll_starts_bars_before_and_keeps_from_the_playhead() {
        let p = song();
        let pa = Panel {
            preroll: Some(1),
            ..panel()
        };
        let r = plan(&p, &pa, 2 * BAR, false, false);
        assert_eq!(
            (r.at, r.keep, r.stop_at),
            (Some(BAR), Some((2 * BAR, None)), None)
        );
        // Not before the song.
        let r = plan(&p, &pa, 100, false, false);
        assert_eq!(r.at, Some(0));
    }

    #[test]
    fn the_punch_range_is_the_loop_range_and_the_postroll_stops_after_it() {
        let mut p = song();
        p.loop_range = Some(LoopRange {
            start: 2 * BAR,
            end: 3 * BAR,
        });
        let pa = Panel {
            punch: true,
            preroll: Some(1),
            postroll: Some(2),
            ..panel()
        };
        let r = plan(&p, &pa, 0, false, false);
        assert_eq!(r.at, Some(BAR));
        assert_eq!(r.keep, Some((2 * BAR, Some(3 * BAR))));
        assert_eq!(r.stop_at, Some(5 * BAR));
        // Without the post-roll it stops at the end of the range; a loop does not stop by itself.
        let r = plan(
            &p,
            &Panel {
                postroll: None,
                ..pa
            },
            0,
            false,
            false,
        );
        assert_eq!(r.stop_at, Some(3 * BAR));
        assert_eq!(plan(&p, &pa, 0, false, true).stop_at, None);
        // A playing transport goes on.
        assert_eq!(plan(&p, &pa, 0, true, false).at, None);
        // With no range the punch does nothing.
        p.loop_range = None;
        assert_eq!(plan(&p, &pa, 0, false, false).keep, Some((0, None)));
    }

    #[test]
    fn an_audio_take_is_cut_to_the_range_and_names_where_in_its_file() {
        let take = json!({"track": "t1", "start": 100, "length": 2000, "file": "a.wav"});
        let (out, gone) = shape(vec![take.clone()], Some((500, Some(1500))), None);
        assert_eq!(gone, 0);
        assert_eq!(
            (
                out[0]["start"].clone(),
                out[0]["length"].clone(),
                out[0]["offset"].clone()
            ),
            (json!(500), json!(1000), json!(400))
        );
        // Outside the range altogether: nothing is left.
        let (out, gone) = shape(vec![take.clone()], Some((3000, Some(4000))), None);
        assert_eq!((out.len(), gone), (0, 1));
        // No range: untouched.
        assert_eq!(shape(vec![take.clone()], None, None).0, vec![take]);
    }

    #[test]
    fn a_midi_take_keeps_the_notes_whose_heads_are_inside_and_cuts_the_one_held_past_the_end() {
        let take = json!({"track": "t1", "start": 0, "length": 2000, "notes": [
            {"start": 100, "length": 100, "pitch": 60, "velocity": 90},
            {"start": 600, "length": 300, "pitch": 62, "velocity": 90},
            {"start": 1400, "length": 100, "pitch": 64, "velocity": 90},
        ]});
        let (out, _) = shape(vec![take], Some((500, Some(800))), None);
        assert_eq!(
            (out[0]["start"].clone(), out[0]["length"].clone()),
            (json!(500), json!(300))
        );
        let notes = out[0]["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(
            (
                notes[0]["start"].clone(),
                notes[0]["length"].clone(),
                notes[0]["pitch"].clone()
            ),
            (json!(100), json!(200), json!(62))
        );
    }

    #[test]
    fn quantize_snaps_the_heads_and_keeps_the_lengths() {
        let take = json!({"track": "t1", "start": 0, "length": 1000, "notes": [
            {"start": 10, "length": 90, "pitch": 60, "velocity": 90},
            {"start": 250, "length": 70, "pitch": 62, "velocity": 90},
            {"start": 481, "length": 33, "pitch": 64, "velocity": 90},
        ]});
        let (out, _) = shape(vec![take], None, Some(120));
        let notes = out[0]["notes"].as_array().unwrap();
        let heads: Vec<_> = notes.iter().map(|n| n["start"].as_i64().unwrap()).collect();
        let lens: Vec<_> = notes
            .iter()
            .map(|n| n["length"].as_i64().unwrap())
            .collect();
        assert_eq!(heads, vec![0, 240, 480]);
        assert_eq!(lens, vec![90, 70, 33]);
    }

    #[test]
    fn a_head_snapped_before_the_take_moves_the_clip_back() {
        let take = json!({"track": "t1", "start": 960, "length": 480, "notes": [
            {"start": 0, "length": 50, "pitch": 60, "velocity": 90},
        ]});
        // The head is at 960 here, which is on the grid; shift it off by one.
        let mut take = take;
        take["start"] = json!(961);
        let (out, _) = shape(vec![take], None, Some(120));
        assert_eq!(out[0]["start"], 960);
        assert_eq!(out[0]["notes"][0]["start"], 0);
    }
}
