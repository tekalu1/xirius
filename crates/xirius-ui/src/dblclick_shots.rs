//! `--script dblclick`: the two double clicks of UI-182 and UI-183. The presses go the way the
//! window sends them (the name, then the arrangement's open, then its press, then the row), so
//! the click path is the real one and no OS input is sent.

use crate::arrange::probe::track;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, RowParts, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "dblclick-01-before",
        acts: &[staged(0)],
    },
    Shot {
        name: "dblclick-02-part-made",
        acts: &[staged(1)],
    },
    Shot {
        name: "dblclick-03-mixer-lit",
        acts: &[staged(2)],
    },
];

fn press(s: &mut Studio, g: &Geometry, (x, y): (f32, f32), clicks: u32) {
    if s.name_press(g, x, y, clicks) {
        return;
    }
    if clicks == 2 && s.arrange_open(g, x, y, Mods::NONE) {
        return;
    }
    if s.arrange_down(g, x, y, Mods::NONE) {
        if clicks == 2 {
            s.arrange_open(g, x, y, Mods::NONE);
        }
        return;
    }
    let hit = s.hit(g, x, y);
    s.press(g, hit);
}

fn double_click(s: &mut Studio, g: &Geometry, at: (f32, f32)) {
    for clicks in 1..=2 {
        press(s, g, at, clicks);
        s.arrange_up(g, at.0, at.1, Mods::NONE);
    }
}

/// Stage 0 shuts the drawer; stage 1 double-clicks the middle of bar 2 on the empty Bass lane;
/// stage 2 double-clicks the empty part of the Vocal track's head.
fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => s.drawer_open = false,
        1 => {
            let row = *s
                .tree()
                .row_of(RowKind::Track(track(s, "Bass")))
                .expect("row");
            let bar = s.project().time_map().bar_start(2);
            let at = (
                g.lanes.x - s.scroll.0 + s.ticks_to_px(bar + bar / 2),
                s.row_y(g, &row) + row.h / 2.0,
            );
            double_click(s, g, at);
        }
        _ => {
            let row = *s
                .tree()
                .row_of(RowKind::Track(track(s, "Vocal")))
                .expect("row");
            let parts = RowParts::of(&row);
            let at = (
                g.column.x + parts.name_right - 4.0,
                s.row_y(g, &row) + RowParts::LINE_1_Y + 6.0,
            );
            double_click(s, g, at);
        }
    }
}
