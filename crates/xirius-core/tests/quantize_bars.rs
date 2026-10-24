//! The quantize, AQ and the grid's splits count the grid from the start of each
//! bar, as the lines and the snap do (UI-339): in 7/8 a 1/4 note grid starts
//! again at every bar. A 4/4 song on a grid that divides the bar keeps every
//! result it had.

use serde_json::{Value, json};
use xirius_core::session::{RunOptions, Session};
use xirius_core::time::{Tick, parse_grid, quantize_tick};

fn run(s: &mut Session, name: &str, args: Value) -> Value {
    s.run_value(name, args.clone(), &RunOptions::human())
        .unwrap_or_else(|e| panic!("{name} {args}: {e:?}"))
        .data
}

fn id(v: &Value) -> String {
    v["id"]
        .as_str()
        .or_else(|| v["ids"][0].as_str())
        .unwrap()
        .to_string()
}

/// A song in `num`/`den` with an instrument track.
fn song(num: u32, den: u32) -> (Session, String) {
    let mut s = Session::new("q");
    if (num, den) != (4, 4) {
        run(&mut s, "timesig.set", json!({ "num": num, "den": den }));
    }
    let t = id(&run(
        &mut s,
        "track.add.instrument",
        json!({ "name": "Keys" }),
    ));
    (s, t)
}

/// A MIDI clip from `at` for `length` ticks with a note at each song tick of `starts`.
fn clip(s: &mut Session, track: &str, at: Tick, length: Tick, starts: &[Tick]) -> String {
    let c = id(&run(
        s,
        "clip.add.midi",
        json!({ "track": track, "at": at, "length": length }),
    ));
    for t in starts {
        note(s, &c, *t, 100);
    }
    c
}

fn note(s: &mut Session, c: &str, at: Tick, length: Tick) {
    run(
        s,
        "note.add",
        json!({ "clip": c, "at": at, "pitch": 60, "length": length, "velocity": 90 }),
    );
}

/// The notes' (start, length), in song ticks, in order.
fn notes(s: &Session, c: &str) -> Vec<(Tick, Tick)> {
    let (_, c) = s.project().clip(c.parse().unwrap()).unwrap();
    let mut v: Vec<_> = c
        .notes()
        .iter()
        .map(|n| (c.start + n.start, n.length))
        .collect();
    v.sort_unstable();
    v
}

fn starts(s: &Session, c: &str) -> Vec<Tick> {
    notes(s, c).iter().map(|n| n.0).collect()
}

/// Every clip's (start, length), in order.
fn clips(s: &Session) -> Vec<(Tick, Tick)> {
    let mut v: Vec<_> = s
        .project()
        .tracks
        .values()
        .flat_map(|t| t.clips.iter().map(|c| (c.start, c.length)))
        .collect();
    v.sort_unstable();
    v
}

// In 7/8 a bar is 3360 ticks: the 1/4 note lines of bar 2 are 3360, 4320, 5280,
// 6240 and the bar at 6720, where the song's own multiples of 960 are 3840,
// 4800, 5760 and 6720.

#[test]
fn in_seven_eight_a_quantize_puts_notes_on_the_lines_of_their_bar() {
    let (mut s, t) = song(7, 8);
    let c = clip(&mut s, &t, 0, 6720, &[3360, 4320, 3400, 5300]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    assert_eq!(starts(&s, &c), [3360, 3360, 4320, 5280]);
}

#[test]
fn in_seven_eight_the_panel_and_aq_count_from_the_bar_too() {
    // AQ runs the same command with the panel's settings.
    let (mut s, t) = song(7, 8);
    let c = clip(&mut s, &t, 0, 6720, &[3400, 4300, 6300]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/4", "range": 100 }),
    );
    // 6300 is 60 from 6240 and 420 from the next bar.
    assert_eq!(starts(&s, &c), [3360, 4320, 6240]);
}

#[test]
fn a_clip_inside_a_bar_quantizes_on_the_songs_lines() {
    // A clip from 3400: its note at 4340 goes to the song's 4320, not to the
    // clip's own 960 (4360).
    let (mut s, t) = song(7, 8);
    let c = clip(&mut s, &t, 3400, 3000, &[4340]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    assert_eq!(starts(&s, &c), [4320]);
    // The clip's own start, too.
    run(
        &mut s,
        "clip.quantize",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    assert_eq!(clips(&s)[0].0, 3360);
}

#[test]
fn in_seven_eight_the_grid_splits_notes_on_the_lines_of_their_bar() {
    let (mut s, t) = song(7, 8);
    let c = clip(&mut s, &t, 3360, 3360, &[]);
    note(&mut s, &c, 5860, 1500);
    run(
        &mut s,
        "note.split.grid",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    // At 6240 and at the next bar, 6720.
    assert_eq!(notes(&s, &c), [(5860, 380), (6240, 480), (6720, 640)]);
}

#[test]
fn in_seven_eight_the_grid_splits_clips_on_the_lines_of_their_bar() {
    let (mut s, t) = song(7, 8);
    let c = clip(&mut s, &t, 2000, 3000, &[]);
    run(
        &mut s,
        "clip.split.grid",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    assert_eq!(
        clips(&s),
        [(2000, 880), (2880, 480), (3360, 960), (4320, 680)]
    );
}

#[test]
fn a_meter_change_starts_the_grid_again_at_its_bar() {
    // Bar 1 in 7/8, then 4/4 from 3360: the 1/4 lines go on at 3360 + 960 k.
    let (mut s, t) = song(7, 8);
    run(
        &mut s,
        "timesig.set",
        json!({ "bar": 2, "num": 4, "den": 4 }),
    );
    let c = clip(&mut s, &t, 0, 7200, &[4300, 6200]);
    run(
        &mut s,
        "note.quantize",
        json!({ "ids": [c], "grid": "1/4" }),
    );
    assert_eq!(starts(&s, &c), [4320, 6240]);
}

/// The grids of the list that divide a 4/4 bar.
const DIVIDING: [&str; 13] = [
    "1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/2t", "1/4t", "1/8t", "1/16t", "1/32t",
    "1/64t",
];

#[test]
fn a_four_four_song_quantizes_notes_as_before() {
    // Before UI-339 each start, in clip time, went to the nearest multiple of
    // the grid, pulled by the strength; the length too with `lengths`. The
    // clips start on the grid, where clip time and song time agree.
    let at: Vec<Tick> = (0..90).map(|k| k * 97 + (k % 7) * 13).collect();
    let length = |k: usize| 37 + k as Tick * 11;
    for grid in DIVIDING {
        let g = parse_grid(grid, 960).unwrap();
        for strength in [100_u32, 50, 33] {
            for lengths in [false, true] {
                for head in [0, 3840, 7 * g] {
                    let (mut s, t) = song(4, 4);
                    let c = clip(&mut s, &t, head, 9000, &[]);
                    for (k, a) in at.iter().enumerate() {
                        note(&mut s, &c, head + a, length(k));
                    }
                    let mut want: Vec<(Tick, Tick)> = at
                        .iter()
                        .enumerate()
                        .map(|(k, &a)| {
                            let len = if lengths {
                                let floor = if strength == 100 { g } else { 1 };
                                quantize_tick(length(k), g, strength).max(floor)
                            } else {
                                length(k)
                            };
                            (head + quantize_tick(a, g, strength).max(0), len)
                        })
                        .collect();
                    want.sort_unstable();
                    run(
                        &mut s,
                        "note.quantize",
                        json!({ "ids": [c], "grid": grid, "strength": strength,
                                "lengths": lengths }),
                    );
                    assert_eq!(notes(&s, &c), want, "{grid} {strength} {lengths} {head}");
                }
            }
        }
    }
}

#[test]
fn a_four_four_song_quantizes_clips_as_before() {
    // Two bars apart, so no two land on one place.
    let at = [100, 8980, 17279, 25940, 34559];
    for grid in DIVIDING {
        let g = parse_grid(grid, 960).unwrap();
        for strength in [100_u32, 50] {
            let (mut s, t) = song(4, 4);
            let ids: Vec<_> = at.iter().map(|a| clip(&mut s, &t, *a, 700, &[])).collect();
            run(
                &mut s,
                "clip.quantize",
                json!({ "ids": ids, "grid": grid, "strength": strength }),
            );
            let want: Vec<_> = at
                .iter()
                .map(|a| (quantize_tick(*a, g, strength).max(0), 700))
                .collect();
            assert_eq!(clips(&s), want, "{grid} {strength}");
        }
    }
}

#[test]
fn a_four_four_song_splits_on_the_grid_as_before() {
    // Before UI-339 the cuts were every multiple of the grid inside.
    for grid in DIVIDING {
        let g = parse_grid(grid, 960).unwrap();
        let multiples = |from: Tick, to: Tick| -> Vec<Tick> {
            ((from / g + 1) * g..to).step_by(g as usize).collect()
        };
        let (mut s, t) = song(4, 4);
        let c = clip(&mut s, &t, 1234, 5000, &[]);
        run(
            &mut s,
            "clip.split.grid",
            json!({ "ids": [c], "grid": grid }),
        );
        let mut want = vec![1234];
        want.extend(multiples(1234, 6234));
        let have: Vec<_> = clips(&s).iter().map(|c| c.0).collect();
        assert_eq!(have, want, "{grid}");

        let (mut s, t) = song(4, 4);
        let c = clip(&mut s, &t, 20_000, 9000, &[]);
        note(&mut s, &c, 20_333, 4000);
        run(
            &mut s,
            "note.split.grid",
            json!({ "ids": [c], "grid": grid }),
        );
        let mut want = vec![20_333];
        want.extend(multiples(20_333, 24_333));
        assert_eq!(starts(&s, &c), want, "{grid}");
    }
}
