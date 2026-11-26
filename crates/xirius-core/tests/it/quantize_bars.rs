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

/// What a row looks at after its command: the clip's notes or the song's clips.
#[derive(Clone, Copy)]
enum Look {
    Notes,
    Clips,
}

struct BarRow {
    name: &'static str,
    /// Meter changes after the first bar's 7/8, as (bar, num, den).
    meters: &'static [(u32, u32, u32)],
    /// The clip, as (start, length), and its notes as (song tick, length).
    clip: (Tick, Tick),
    notes: &'static [(Tick, Tick)],
    cmd: &'static str,
    args: fn(&str) -> Value,
    look: Look,
    want: &'static [(Tick, Tick)],
}

#[test]
fn outside_four_four_every_grid_edit_counts_from_the_start_of_each_bar() {
    let rows = [
        BarRow {
            name: "in 7/8 a quantize puts notes on the lines of their bar",
            meters: &[],
            clip: (0, 6720),
            notes: &[(3360, 100), (4320, 100), (3400, 100), (5300, 100)],
            cmd: "note.quantize",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Notes,
            want: &[(3360, 100), (3360, 100), (4320, 100), (5280, 100)],
        },
        BarRow {
            // AQ runs the same command with the panel's settings. 6300 is 60 from 6240 and
            // 420 from the next bar.
            name: "in 7/8 the panel and AQ count from the bar too",
            meters: &[],
            clip: (0, 6720),
            notes: &[(3400, 100), (4300, 100), (6300, 100)],
            cmd: "note.quantize",
            args: |c| json!({ "ids": [c], "grid": "1/4", "range": 100 }),
            look: Look::Notes,
            want: &[(3360, 100), (4320, 100), (6240, 100)],
        },
        BarRow {
            // A clip from 3400: its note at 4340 goes to the song's 4320, not to the clip's
            // own 960 (4360).
            name: "a clip inside a bar quantizes its notes on the song's lines",
            meters: &[],
            clip: (3400, 3000),
            notes: &[(4340, 100)],
            cmd: "note.quantize",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Notes,
            want: &[(4320, 100)],
        },
        BarRow {
            name: "a clip inside a bar quantizes its own start on the song's lines",
            meters: &[],
            clip: (3400, 3000),
            notes: &[],
            cmd: "clip.quantize",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Clips,
            want: &[(3360, 3000)],
        },
        BarRow {
            // At 6240 and at the next bar, 6720.
            name: "in 7/8 the grid splits notes on the lines of their bar",
            meters: &[],
            clip: (3360, 3360),
            notes: &[(5860, 1500)],
            cmd: "note.split.grid",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Notes,
            want: &[(5860, 380), (6240, 480), (6720, 640)],
        },
        BarRow {
            name: "in 7/8 the grid splits clips on the lines of their bar",
            meters: &[],
            clip: (2000, 3000),
            notes: &[],
            cmd: "clip.split.grid",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Clips,
            want: &[(2000, 880), (2880, 480), (3360, 960), (4320, 680)],
        },
        BarRow {
            // Bar 1 in 7/8, then 4/4 from 3360: the 1/4 lines go on at 3360 + 960 k.
            name: "a meter change starts the grid again at its bar",
            meters: &[(2, 4, 4)],
            clip: (0, 7200),
            notes: &[(4300, 100), (6200, 100)],
            cmd: "note.quantize",
            args: |c| json!({ "ids": [c], "grid": "1/4" }),
            look: Look::Notes,
            want: &[(4320, 100), (6240, 100)],
        },
    ];
    let mut failures = Vec::new();
    for r in &rows {
        let (mut s, t) = song(7, 8);
        for &(bar, num, den) in r.meters {
            run(
                &mut s,
                "timesig.set",
                json!({ "bar": bar, "num": num, "den": den }),
            );
        }
        let c = clip(&mut s, &t, r.clip.0, r.clip.1, &[]);
        for &(at, length) in r.notes {
            note(&mut s, &c, at, length);
        }
        run(&mut s, r.cmd, (r.args)(&c));
        let got = match r.look {
            Look::Notes => notes(&s, &c),
            Look::Clips => clips(&s),
        };
        if got != r.want {
            failures.push(format!("{}: got {got:?}, want {:?}", r.name, r.want));
        }
    }
    assert!(failures.is_empty(), "grid edits by the bar: {failures:#?}");
}

/// The grids of the list that divide a 4/4 bar.
const DIVIDING: [&str; 13] = [
    "1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/2t", "1/4t", "1/8t", "1/16t", "1/32t",
    "1/64t",
];

/// In a 4/4 song a grid that divides the bar has a line on every multiple of itself, so
/// counting from each bar gives what counting from the song's start gave before UI-339. Each
/// row is one grid and one setting, against what that older rule gives.
#[test]
fn a_four_four_song_on_a_dividing_grid_keeps_every_result_it_had() {
    let mut failures = Vec::new();
    // Notes: each start, in clip time, went to the nearest multiple of the grid, pulled by
    // the strength; the length too with `lengths`. The clips start on the grid, where clip
    // time and song time agree. All the notes go in with one command.
    let at: Vec<Tick> = (0..90).map(|k| k * 97 + (k % 7) * 13).collect();
    let length = |k: usize| 37 + k as Tick * 11;
    let put: Vec<Value> = at
        .iter()
        .enumerate()
        .map(|(k, a)| json!({ "at": a, "pitch": 60, "length": length(k), "velocity": 90 }))
        .collect();
    for grid in DIVIDING {
        let g = parse_grid(grid, 960).unwrap();
        for strength in [100_u32, 50, 33] {
            for lengths in [false, true] {
                for head in [0, 3840, 7 * g] {
                    let (mut s, t) = song(4, 4);
                    let c = clip(&mut s, &t, head, 9000, &[]);
                    run(&mut s, "note.put", json!({ "clip": c, "notes": put }));
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
                    if notes(&s, &c) != want {
                        failures.push(format!(
                            "note.quantize {grid} strength {strength} lengths {lengths} head {head}"
                        ));
                    }
                }
            }
        }
    }
    // Clips, two bars apart so no two land on one place.
    let clip_at = [100, 8980, 17279, 25940, 34559];
    for grid in DIVIDING {
        let g = parse_grid(grid, 960).unwrap();
        for strength in [100_u32, 50] {
            let (mut s, t) = song(4, 4);
            let ids: Vec<_> = clip_at
                .iter()
                .map(|a| clip(&mut s, &t, *a, 700, &[]))
                .collect();
            run(
                &mut s,
                "clip.quantize",
                json!({ "ids": ids, "grid": grid, "strength": strength }),
            );
            let want: Vec<_> = clip_at
                .iter()
                .map(|a| (quantize_tick(*a, g, strength).max(0), 700))
                .collect();
            let got = clips(&s);
            if got != want {
                failures.push(format!(
                    "clip.quantize {grid} strength {strength}: got {got:?}, want {want:?}"
                ));
            }
        }
    }
    // Splits: the cuts were every multiple of the grid inside.
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
        if have != want {
            failures.push(format!(
                "clip.split.grid {grid}: got {have:?}, want {want:?}"
            ));
        }

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
        let have = starts(&s, &c);
        if have != want {
            failures.push(format!(
                "note.split.grid {grid}: got {have:?}, want {want:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "4/4 grid edits as before: {failures:#?}"
    );
}
