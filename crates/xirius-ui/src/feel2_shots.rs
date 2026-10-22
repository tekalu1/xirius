//! `--script feel2`: the second round of the editing feel. Every stage goes in through the
//! screen's own press, move and release entry points; silent, no OS input.
//! EDT-051: a note muted by Alt+click on its upper part, grey beside the two that play.
//! EDT-032: the pencil's shape turned by the wheel on the bar; a triangle drawn and one held,
//! and the parabola with Alt held drawn as a straight line.
//! EDT-041: the bass's clip looped over its first two bars by a drag on its lower right corner,
//! the pointer on the corner; and the loop's head held one bar later with Alt.
//! EDT-062: the keys' part and a part on a new Lead track picked together in the arrangement;
//! the roll shows the lead's notes with its own, and Ctrl+Shift+A has selected all of them.
//! EDT-046: a clip held at the right edge with Space down: the view has gone past the song's end
//! and the clip with it.
//! ENG-033: the drums' bounce rendering, and the bass's asked for after it waiting in the queue.
//! UI-063: the bass's fader, played by its volume lane in Read, under the pointer: the tooltip
//! says why it does not move; and the same for the sampler's first knob in its window.
//! UI-088: the first note picked by → and moved two sixteenths later by → twice.
use crate::arrange::probe::{clip_mid, hold, two_lanes};

use crate::roll::ids_json;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::{Mods, Tool};
use crate::topbar::BarButton;
use crate::tree::RowKind;
use serde_json::json;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // EDT-032: the parabola, with Alt held: the stroke held is a straight line.
    Shot {
        name: "edt032-alt-line",
        acts: &[Act::Reset, staged(2)],
    },
    // EDT-032: the wheel turned the pencil to the triangle; one drawn, a second held.
    Shot {
        name: "edt032-triangle",
        acts: &[Act::Reset, staged(1)],
    },
    // EDT-041: the clip looped over its first two bars, the pointer on its corner.
    Shot {
        name: "edt041-loop",
        acts: &[Act::Reset, staged(4)],
    },
    // EDT-041: the loop's head dragged a bar later with Alt, held.
    Shot {
        name: "edt041-loop-head",
        acts: &[Act::Reset, staged(5)],
    },
    // EDT-046: a clip held at the edge with Space: the view went past the song's end.
    Shot {
        name: "edt046-edge-scroll",
        acts: &[Act::Reset, staged(3)],
    },
    // EDT-051: three notes; the middle one Alt+clicked on its upper part, grey.
    Shot {
        name: "edt051-muted",
        acts: &[Act::Reset, staged(0)],
    },
    // EDT-062: two parts on two tracks in the roll, every note of both selected.
    Shot {
        name: "edt062-parts",
        acts: &[Act::Reset, staged(7)],
    },
    // ENG-033: a bounce renders; the one asked for after it waits, its clips saying so.
    Shot {
        name: "eng033-queued",
        acts: &[Act::Reset, staged(10)],
    },
    // UI-063: the pointer on a fader its lane plays: the reason in the tooltip.
    Shot {
        name: "ui063-locked-fader",
        acts: &[Act::Reset, staged(8)],
    },
    // UI-063: the pointer on a sampler's knob its lane plays, the lane's mark on the knob.
    Shot {
        name: "ui063-locked-knob",
        acts: &[Act::Reset, staged(9)],
    },
    // UI-088: the first note, picked and moved by the arrows, still selected.
    Shot {
        name: "ui088-arrows",
        acts: &[Act::Reset, staged(6)],
    },
];

/// The roll with three notes of the clip it shows, and nothing else.
fn three_notes(s: &mut Studio, g: &Geometry) {
    let ctx = s.roll_ctx(g).expect("the roll");
    let _ = s.song.run(
        "note.delete",
        json!({"ids":ids_json(ctx.notes().iter().map(|n| &n.id))}),
    );
    let _ = s.song.run(
        "note.put",
        json!({"clip":ctx.clip.id.to_string(),"notes":[
            {"at":960,"pitch":60,"length":960,"velocity":80},
            {"at":2880,"pitch":62,"length":1440,"velocity":90},
            {"at":4800,"pitch":64,"length":480,"velocity":100}
        ]}),
    );
    s.resized(g);
}

/// The upper part of note `i` of the roll.
fn upper(s: &Studio, g: &Geometry, i: usize) -> (f32, f32) {
    let c = s.roll_ctx(g).expect("the roll");
    let r = s.roll.note_box(&c, i).expect("the note");
    (r.x + r.w / 2.0, r.y + 1.0)
}

/// A click as the screen delivers it: its press, then its release.
fn click(s: &mut Studio, g: &Geometry, p: (f32, f32), m: Mods) {
    s.roll_press(g, p.0, p.1, m, 1);
    if let Some(c) = s.roll_ctx(g) {
        let out = s.roll.up(&c, p.0, p.1, m);
        s.apply_roll(g, out);
    }
}

/// The arrangement alone with the pencil, the wheel turned `notches` down on it, and the bass's
/// volume lane open; the lane's top and bottom, and the screen x of a bar.
fn pencil(
    s: &mut Studio,
    g: &Geometry,
    notches: i32,
) -> (Geometry, f32, f32, impl Fn(f32) -> f32 + use<>) {
    s.popup = None;
    s.drawer_open = false;
    let g = s.geometry(g.size.0, g.size.1);
    s.rebuild(&g);
    let bass = two_lanes(s, &g, "Bass");
    s.press(&g, Hit::Bar(BarButton::Tool(Tool::Draw)));
    if let Some((x, y)) = s.center_of(&g, Hit::Bar(BarButton::Tool(Tool::Draw))) {
        for _ in 0..notches {
            s.bar_shape_wheel(&g, x, y, -1.0);
        }
    }
    let row = *s.tree().row_of(RowKind::Track(bass)).expect("the bass");
    let top = s.lane_top(&g, &row, 0);
    let (x0, bar) = (s.tick_x(&g, 0), s.ticks_to_px(3840));
    (
        g,
        top + 12.0,
        top + crate::tree::LANE_H - 12.0,
        move |b: f32| x0 + b * bar,
    )
}

/// The arrangement alone with the arrow; the lower right corner of the bass's first clip (just
/// inside it) and the clip's length.
fn bass_corner(s: &mut Studio, g: &Geometry) -> (Geometry, (f32, f32), i64) {
    s.popup = None;
    s.drawer_open = false;
    let g = s.geometry(g.size.0, g.size.1);
    s.rebuild(&g);
    s.tool = Tool::Select;
    let t = crate::arrange::probe::track(s, "Bass");
    let c = s.project().tracks[&t].clips[0].clone();
    let row = *s.tree().row_of(RowKind::Track(t)).expect("the bass");
    let r = s.clip_rect(&g, &row, &c);
    (g, (r.right() - 2.0, r.bottom() - 2.0), c.length)
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.popup = None;
            three_notes(s, g);
            s.tool = Tool::Select;
            let p = upper(s, g, 1);
            click(s, g, p, Mods::alt());
            // The pointer off the notes, so the grey is the note's own and not a hover.
            if let Some(c) = s.roll_input(g) {
                let out = s.roll.moved(&c, p.0, p.1 + 120.0, Mods::NONE);
                s.apply_roll(g, out);
            }
        }
        1 => {
            let (g, lo, hi, x) = pencil(s, g, 4);
            hold(s, &g, (x(3.0), lo), (x(5.0), hi), Mods::NONE);
            s.arrange_up(&g, x(5.0), hi, Mods::NONE);
            hold(s, &g, (x(7.0), lo), (x(9.0), hi), Mods::NONE);
            s.input_map.pointer = (x(9.0), hi);
        }
        2 => {
            let (g, lo, hi, x) = pencil(s, g, 2);
            hold(s, &g, (x(3.0), lo), (x(5.0), hi), Mods::alt());
            s.input_map.pointer = (x(5.0), hi);
        }
        3 => {
            s.popup = None;
            s.drawer_open = false;
            let g = s.geometry(g.size.0, g.size.1);
            s.rebuild(&g);
            s.tool = Tool::Select;
            let (x, y) = clip_mid(s, &g, "Bass", 0);
            let edge = g.lanes.right() - 2.0;
            s.input_map.pointer = (x, y);
            hold(s, &g, (x, y), (edge, y), Mods::NONE);
            s.input_map.pointer = (edge, y);
            s.edit_key(&g, "space", Mods::NONE);
            for _ in 0..30 {
                s.drag_edge_step(&g, 0.05);
                s.resized(&g);
            }
        }
        4 | 5 => {
            let (g, at, length) = bass_corner(s, g);
            // The loop's end two bars into the clip.
            let to = (at.0 - s.ticks_to_px(length - 2 * 3840), at.1);
            hold(s, &g, at, to, Mods::NONE);
            s.arrange_up(&g, to.0, to.1, Mods::NONE);
            if n == 4 {
                s.arrange_move(&g, at.0, at.1, Mods::NONE);
                s.input_map.pointer = at;
            } else {
                let to = (at.0 + s.ticks_to_px(3840), at.1);
                hold(s, &g, at, to, Mods::alt());
                s.input_map.pointer = to;
            }
        }
        6 => {
            s.popup = None;
            three_notes(s, g);
            s.tool = Tool::Select;
            s.roll.selected.clear();
            // → picks the first note; → twice more moves it two sixteenths later.
            for _ in 0..3 {
                s.edit_key(g, "right", Mods::NONE);
            }
            let c = s.roll_ctx(g).expect("the roll");
            if let Some(r) = s.roll.note_box(&c, 0) {
                s.input_map.pointer = (r.x, r.y + 160.0);
            }
        }
        7 => {
            s.popup = None;
            three_notes(s, g);
            s.tool = Tool::Select;
            let (keys, first) = s.selected_clip.expect("the keys' part");
            let at = s.project().tracks[&keys]
                .clips
                .iter()
                .find(|c| c.id == first)
                .map_or(0, |c| c.start);
            let made = |v: Result<serde_json::Value, String>| {
                v.ok()
                    .and_then(|v| {
                        v["id"]
                            .as_str()
                            .or_else(|| v["ids"][0].as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default()
            };
            let lead = made(s.song.run("track.add.instrument", json!({"name": "Lead"})));
            let part = made(
                s.song
                    .run("clip.add.midi", json!({"track": lead, "at": at, "bars": 2})),
            );
            let _ = s.song.run(
                "note.put",
                json!({"clip": part, "notes": [
                    {"at": 480, "pitch": 65, "length": 480, "velocity": 90},
                    {"at": 1920, "pitch": 59, "length": 960, "velocity": 90},
                    {"at": 4320, "pitch": 63, "length": 480, "velocity": 90},
                    {"at": 5760, "pitch": 61, "length": 960, "velocity": 90}
                ]}),
            );
            s.rebuild(g);
            let (Ok(lead), Ok(part)) = (lead.parse(), part.parse()) else {
                return;
            };
            // The lead's part, then Shift and the keys' part: the keys' part goes in the roll.
            for (t, c, m) in [(lead, part, Mods::NONE), (keys, first, Mods::shift())] {
                let Some(row) = s.tree().row_of(RowKind::Track(t)).copied() else {
                    return;
                };
                s.scroll.1 = (row.y - 8.0).max(0.0);
                s.clamp_scroll(g);
                let Some(k) = s.project().tracks[&t]
                    .clips
                    .iter()
                    .find(|k| k.id == c)
                    .cloned()
                else {
                    return;
                };
                let r = s.clip_rect(g, &row, &k);
                let p = (r.x + 4.0, r.y + r.h / 2.0);
                s.arrange_down(g, p.0, p.1, m);
                s.arrange_up(g, p.0, p.1, m);
            }
            // The two tracks in view, the keys at the top.
            if let Some(row) = s.tree().row_of(RowKind::Track(keys)).copied() {
                s.scroll.1 = (row.y - 8.0).max(0.0);
                s.clamp_scroll(g);
            }
            // Into the roll, then every note of every part on the two tracks.
            if let Some(c) = s.roll_ctx(g) {
                let v = s.roll.view(&c);
                click(s, g, (v.grid.x + 4.0, v.grid.y + 2.0), Mods::NONE);
            }
            s.edit_key(g, "a", Mods::ctrl_shift());
            s.input_map.pointer = (0.0, 0.0);
        }
        8 => {
            s.popup = None;
            let t = crate::arrange::probe::track(s, "Bass");
            let bar = i64::from(s.project().ppq) * 4;
            for (at, v) in [(0, -12.0), (4 * bar, -3.0), (8 * bar, -9.0)] {
                s.run_command(
                    "auto.point.add",
                    json!({"track": t.to_string(), "target": "volume", "at": at, "value": v}),
                );
            }
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Mixer;
            let g = s.geometry(g.size.0, g.size.1);
            s.rebuild(&g);
            let hit = crate::mixer::MixerHit::Fader(t);
            if let Some((x, y)) = s.mixer_center(&g, hit) {
                s.input_map.pointer = (x, y);
                s.hover = s.hit(&g, x, y);
            }
        }
        9 => {
            s.popup = None;
            let t = crate::arrange::probe::track(s, "Keys");
            let pick = crate::instrument::Pick::built_in(crate::instrument::DEFAULT);
            if !s.set_instrument(t, &pick) {
                return;
            }
            let Some(plugin) = crate::instrument::current(&s.project().tracks[&t]).cloned() else {
                return;
            };
            let Some(def) = plugin.builtin() else {
                return;
            };
            let target = format!("param:{}:{}", plugin.id, def.params[0].id);
            for (at, v) in [(0, 0.25), (7680, 0.75)] {
                s.run_command(
                    "auto.point.add",
                    json!({"track": t.to_string(), "target": target, "at": at, "value": v}),
                );
            }
            s.press(g, Hit::Keys(t));
            let knob = crate::instrument::window::Part::Knob(0);
            if let Some((x, y)) = s.instrument_center(g, plugin.id, knob) {
                s.input_map.pointer = (x, y);
                s.hover = s.hit(g, x, y);
            }
        }
        10 => {
            s.popup = None;
            let bars = i64::from(s.project().ppq) * 4 * 400;
            for name in ["Drums", "Bass"] {
                let t = crate::arrange::probe::track(s, name);
                s.job_start(
                    "range.bounce",
                    json!({ "from": 0, "to": bars, "tracks": [t.to_string()], "tail": 0 }),
                );
            }
            s.input_map.pointer = (0.0, 0.0);
        }
        _ => {}
    }
}
