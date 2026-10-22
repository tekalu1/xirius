//! `--script range-trim`: AUT-010's range over a lane and over the tempo row, taken hold of. The
//! upper half of the range under the pointer, its values dragged down with the value in the
//! balloon, the frame Alt+T puts round them, and its two handles dragged.
use crate::arrange::probe::{hold, track, two_lanes};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;
use serde_json::json;
use xirius_core::auto::AutoTarget;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "01-hover",
        acts: &[Act::Reset, staged(0)],
    },
    Shot {
        name: "02-trim",
        acts: &[staged(1)],
    },
    Shot {
        name: "03-frame",
        acts: &[staged(2)],
    },
    Shot {
        name: "04-scale",
        acts: &[staged(3)],
    },
    Shot {
        name: "05-tilt",
        acts: &[staged(4)],
    },
    Shot {
        name: "06-tempo",
        acts: &[staged(5)],
    },
];

const BAR: i64 = 3840;

/// A point in the upper quarter of Bass's volume lane, half a bar into the range.
fn upper(s: &Studio, g: &Geometry) -> (f32, f32) {
    let t = track(s, "Bass");
    let top = s
        .lane_top_of(g, t, &AutoTarget::Volume)
        .expect("the lane on screen");
    (s.tick_x(g, 4 * BAR + BAR / 2), top + 10.0)
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.popup = None;
            s.drawer_open = false;
            s.resized(g);
            let t = two_lanes(s, g, "Bass");
            s.scroll.1 = s.tree().row_of(RowKind::Track(t)).expect("Bass").y - 40.0;
            s.arrange.range = Some((4 * BAR, 8 * BAR));
            s.arrange.ranges = vec![(4 * BAR, 8 * BAR)];
            s.rebuild(g);
            let at = upper(s, g);
            s.arrange_move(g, at.0, at.1, Mods::NONE);
        }
        1 => {
            let at = upper(s, g);
            hold(s, g, at, (at.0, at.1 + 8.0), Mods::NONE);
        }
        2 => {
            let at = upper(s, g);
            s.arrange_up(g, at.0, at.1 + 8.0, Mods::NONE);
            s.arrange_move(g, at.0, at.1, Mods::NONE);
            s.mapped_key(g, "t", Some("t"), Mods::alt());
            let b = s.frame_box(g, None).expect("the frame");
            s.arrange_move(g, b.scale.x + 4.0, b.scale.y + 4.0, Mods::NONE);
        }
        3 => {
            let b = s.frame_box(g, None).expect("the frame");
            let grip = (b.scale.x + 4.0, b.scale.y + 4.0);
            hold(s, g, grip, (grip.0, grip.1 - 4.0), Mods::NONE);
        }
        4 => {
            s.arrange.drag = None;
            let b = s.frame_box(g, None).expect("the frame");
            let grip = (b.tilt.x + 4.0, b.tilt.y + 4.0);
            hold(s, g, grip, (grip.0, grip.1 + 12.0), Mods::NONE);
        }
        _ => {
            s.arrange.drag = None;
            s.arrange.frame = None;
            s.run_command("tempo.set", json!({"at": 6 * BAR, "bpm": 132}));
            let at = (s.tick_x(g, 4 * BAR + BAR / 2), g.tempo.y + 3.0);
            hold(s, g, at, (at.0, at.1 - 16.0), Mods::NONE);
        }
    }
}
