//! `--script drum-rows`: the menus of the drum rows (EDT-006, approved 2026-10-15). The map
//! button's menu hides and renames the rows; the menu of a row's name renames, hides and moves
//! that row. Silent; no OS input.

use crate::drum_view_shots::play as part;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Hit, Studio};

pub const SHOTS: &[Shot] = &[
    // The map button pressed: the maps, then 隠す ›, 行名を変更 › and 既定に戻す.
    Shot {
        name: "dr01-map-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 0),
        ],
    },
    // The map menu's 隠す: a row per sound, the hidden ones checked.
    Shot {
        name: "dr02-hide-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 1),
        ],
    },
    // The menu of the snare's name: its name, hiding it and its place.
    Shot {
        name: "dr03-row-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 2),
        ],
    },
    // 上へ移動 chosen there: the snare one row up, its notes with it.
    Shot {
        name: "dr04-row-moved",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 3),
        ],
    },
    // The menu of the top row: 上へ移動 is off.
    Shot {
        name: "dr05-top-row-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 4),
        ],
    },
    // 名前を変更 chosen from the menu of the kick's name (scrolled into sight): the name's field.
    Shot {
        name: "dr06-rename",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 4),
            Act::Stage(Play::Laid(play), 5),
        ],
    },
];

/// Right-clicks the name of the row of `pitch`.
fn name_menu(s: &mut Studio, g: &Geometry, pitch: u8) {
    let Some(c) = s.roll_ctx(g) else { return };
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    s.scene_context(g, f.keys.x + f.keys.w / 2.0, v.y(pitch) + v.row_h / 2.0);
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            s.press(g, Hit::RollMap);
        }
        1 => {
            s.press(g, Hit::RollMap);
            s.context_choose(g, "隠す");
        }
        2 => name_menu(s, g, 38),
        3 => {
            name_menu(s, g, 38);
            s.context_choose(g, "上へ移動");
        }
        4 => {
            let Some(c) = s.roll_ctx(g) else { return };
            let Some(top) = c.rows().pitches().next() else {
                return;
            };
            s.roll.scroll_to_pitch(&c, top);
            name_menu(s, g, top);
        }
        _ => {
            let Some(c) = s.roll_ctx(g) else { return };
            s.roll.scroll_to_pitch(&c, 36);
            name_menu(s, g, 36);
            s.context_choose(g, "名前を変更");
        }
    }
}
