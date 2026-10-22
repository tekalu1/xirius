//! `--script wave-mod`: AUT-010's wave drawn with the pencil in a lane, and the modifiers it reads
//! on the way (approved 2026-10-15). Out to the end, then with Mod the phase moves and the ends
//! stay, with Mod+Alt the whole stroke moves in time, and the wave let go. Silent; no OS input.
use crate::arrange::probe::{track, two_lanes};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};
use crate::tools::{Mods, Tool};
use crate::topbar::BarButton;
use crate::tree::{LANE_H, RowKind};
use serde_json::json;
use xirius_core::auto::AutoTarget;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // The pencil from the top bar, a sine drawn from bar 10 to bar 12, held.
    Shot {
        name: "wm01-drawn",
        acts: &[Act::Reset, staged(0)],
    },
    // Then a quarter note on with Mod: the same two bars, the wave a quarter later.
    Shot {
        name: "wm02-phase",
        acts: &[staged(1)],
    },
    // Then a bar on with Mod+Alt: the whole stroke a bar later.
    Shot {
        name: "wm03-shift",
        acts: &[staged(2)],
    },
    // Let go: the points laid.
    Shot {
        name: "wm04-laid",
        acts: &[staged(3)],
    },
];

const BAR: i64 = 3840;

/// The middle of Bass's volume lane at `tick`.
fn at(s: &Studio, g: &Geometry, tick: i64) -> (f32, f32) {
    let t = track(s, "Bass");
    let top = s
        .lane_top_of(g, t, &AutoTarget::Volume)
        .expect("the lane on screen");
    (s.tick_x(g, tick), top + LANE_H / 2.0)
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    let ctrl = Mods {
        ctrl: true,
        ..Mods::NONE
    };
    match n {
        0 => {
            s.popup = None;
            s.drawer_open = false;
            s.resized(g);
            let t = two_lanes(s, g, "Bass");
            s.scroll.1 = s.tree().row_of(RowKind::Track(t)).expect("Bass").y - 40.0;
            s.rebuild(g);
            let _ = s.song.set("tool.draw.shape", json!({ "shape": "sine" }));
            s.press(g, Hit::Bar(BarButton::Tool(Tool::Draw)));
            let (from, to) = (at(s, g, 9 * BAR), at(s, g, 11 * BAR));
            s.arrange_down(g, from.0, from.1, Mods::NONE);
            s.arrange_move(g, (from.0 + to.0) / 2.0, from.1 - 6.0, Mods::NONE);
            s.arrange_move(g, to.0, to.1, Mods::NONE);
        }
        1 => {
            let p = at(s, g, 11 * BAR + 240);
            s.arrange_move(g, p.0, p.1, ctrl);
        }
        2 => {
            let p = at(s, g, 12 * BAR + 240);
            s.arrange_move(g, p.0, p.1, Mods { alt: true, ..ctrl });
        }
        _ => {
            let p = at(s, g, 12 * BAR + 240);
            s.arrange_up(g, p.0, p.1, Mods { alt: true, ..ctrl });
            s.arrange_move(g, p.0, p.1 + 30.0, Mods::NONE);
        }
    }
}
