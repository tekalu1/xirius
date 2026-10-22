//! `--script clipedit`: the fades, the gain line, a crossfade, the join of two
//! clips and the slip, each as the pointer takes hold of it (EDT-002, 017, 036,
//! 037, 039, 040). Stage 0 gives the song something to edit through the core
//! commands; every later stage presses a handle and leaves the drag half done,
//! so the picture shows the clip as the drag would leave it and the value next
//! to the pointer.

use serde_json::json;
use xirius_core::id::{ClipId, TrackId};

use crate::arrange::clipfx::FxView;
use crate::arrange::probe::{hold, track};
use crate::shot::{Act, Shot};
use crate::studio::{Geometry, Rect, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

pub const CLIPEDIT: &[Shot] = &[
    // A vocal clip with both fades curved and two points on its gain line; the
    // guitar clips crossfade where they overlap.
    Shot {
        name: "c01-fades-gain-crossfade",
        acts: &[Act::ClipEdit(0)],
    },
    // The fade-in flag dragged a bar: the clip fades in over it.
    Shot {
        name: "c02-fade-in-drag",
        acts: &[Act::ClipEdit(1)],
    },
    // The volume square dragged up: the whole line rises, the balloon says by how much.
    Shot {
        name: "c03-gain-drag",
        acts: &[Act::ClipEdit(2)],
    },
    // The crossfade's centre dragged later.
    Shot {
        name: "c04-crossfade-drag",
        acts: &[Act::ClipEdit(3)],
    },
    // The join of two touching clips dragged: both ends move, the right one's end stays.
    Shot {
        name: "c05-join-drag",
        acts: &[Act::ClipEdit(4)],
    },
    // Ctrl + Alt on a clip: what it plays slips, the clip stays.
    Shot {
        name: "c06-slip-drag",
        acts: &[Act::ClipEdit(5)],
    },
];

const VOCAL: &str = "Vocal";
const GUITAR: &str = "Guitar";

fn none() -> Mods {
    Mods::default()
}

/// The `i`-th clip of a track, by start.
fn clip_at(s: &Studio, t: TrackId, i: usize) -> ClipId {
    let p = s.project();
    let mut v: Vec<_> = p.tracks[&t].clips.iter().collect();
    v.sort_by_key(|c| c.start);
    v[i].id
}

fn rect_of(s: &Studio, g: &Geometry, t: TrackId, c: ClipId) -> Rect {
    let p = s.project();
    let clip = p.tracks[&t].clips.iter().find(|k| k.id == c).unwrap();
    let row = *s.tree().row_of(RowKind::Track(t)).unwrap();
    s.clip_rect(g, &row, clip)
}

fn handles(s: &Studio, g: &Geometry, t: TrackId, c: ClipId) -> crate::arrange::clipfx::Handles {
    let p = s.project();
    let track = &p.tracks[&t];
    let clip = track.clips.iter().find(|k| k.id == c).unwrap();
    FxView::of(&p, track, clip)
        .expect("an audio clip")
        .handles(s, rect_of(s, g, t, c))
}

fn bar(s: &Studio) -> f32 {
    s.ticks_to_px(i64::from(s.project().ppq) * 4)
}

/// Plays stage `stage` of [`CLIPEDIT`].
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    s.arrange.drag = None;
    s.arrange.hover = crate::arrange::Spot::None;
    s.drawer_open = false;
    let (vocal, guitar) = (track(s, VOCAL), track(s, GUITAR));
    if stage == 0 {
        setup(s, g, vocal, guitar);
    }
    let g = &s.geometry(g.size.0, g.size.1);
    s.rebuild(g);
    let (v0, v1) = (clip_at(s, vocal, 0), clip_at(s, vocal, 1));
    let g1 = clip_at(s, guitar, 1);
    match stage {
        0 => {
            s.arrange.clips = [(vocal, v0)].into();
            s.selected_clip = Some((vocal, v0));
            s.arrange.touched();
        }
        1 => {
            let from = centre(handles(s, g, vocal, v0).flag_in.unwrap());
            hold(s, g, from, (from.0 + bar(s), from.1), none());
        }
        2 => {
            let from = handles(s, g, vocal, v0).gain;
            hold(s, g, from, (from.0, from.1 - 14.0), none());
        }
        3 => {
            let from = handles(s, g, guitar, g1).cross.expect("a crossfade");
            hold(s, g, from, (from.0 + bar(s) * 0.4, from.1), none());
        }
        4 => {
            let r = rect_of(s, g, vocal, v0);
            let from = (r.right(), r.y + r.h * 0.75);
            hold(s, g, from, (from.0 - bar(s) * 0.5, from.1), none());
        }
        5 => {
            let r = rect_of(s, g, vocal, v1);
            let from = (r.x + r.w * 0.4, r.y + r.h * 0.6);
            let mods = Mods {
                ctrl: true,
                alt: true,
                ..Mods::default()
            };
            hold(s, g, from, (from.0 + bar(s) * 0.5, from.1), mods);
        }
        _ => {}
    }
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// Fades, a gain line, a crossfade and a join, made by the commands the pointer runs.
fn setup(s: &mut Studio, g: &Geometry, vocal: TrackId, guitar: TrackId) {
    let ppq = i64::from(s.project().ppq);
    let bar = ppq * 4;
    // The whole picture on screen: bar 1 to bar 18 across the lanes.
    s.beat_w = (g.lanes.w / 18.0 / 4.0).max(8.0);
    s.scroll.0 = 0.0;
    let v0 = clip_at(s, vocal, 0);
    s.arrange_run(
        "clip.fade.set",
        json!({ "ids": [v0.to_string()], "in": bar, "out": bar * 3 / 2,
                "in_curve": 0.4, "out_curve": -0.3 }),
    );
    for (at, db) in [(bar * 2, -6.0), (bar * 4, 3.0)] {
        s.arrange_run(
            "clip.gain.point.add",
            json!({ "id": v0.to_string(), "at": at, "db": db }),
        );
    }
    // The vocal's second take starts where the first ends, with material before it.
    let first = s.project().tracks[&vocal]
        .clips
        .iter()
        .find(|c| c.id == v0)
        .map(|c| c.start + c.length)
        .expect("the first take");
    let made = s
        .arrange_run(
            "clip.add.audio",
            json!({ "track": vocal.to_string(), "at": first, "length": bar * 3,
                    "file": "vocal_take2.wav" }),
        )
        .expect("a second take");
    let v1: ClipId = made["id"]
        .as_str()
        .or_else(|| made["ids"][0].as_str())
        .and_then(|x| x.parse().ok())
        .expect("its id");
    s.arrange_run("clip.slip", json!({ "ids": [v1.to_string()], "by": -bar }));
    // The guitar's second clip runs a bar into the first.
    let made = s
        .arrange_run(
            "clip.add.audio",
            json!({ "track": guitar.to_string(), "at": 12 * bar, "length": bar * 4,
                    "file": "guitar_di2.wav" }),
        )
        .expect("a second clip");
    let g1: ClipId = made["id"]
        .as_str()
        .or_else(|| made["ids"][0].as_str())
        .and_then(|x| x.parse().ok())
        .expect("its id");
    s.arrange_run("clip.fade.cross", json!({ "ids": [g1.to_string()] }));
}
