//! `--script draw-length`: the length the pencil draws with, in the drawer's bar and right of
//! the grid in the top bar (EDT-131). Silent; no OS input.

use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};

pub const SHOTS: &[Shot] = &[
    // The grid's value: the switch shows it, the top bar has no chip.
    Shot {
        name: "dl01-grid",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 0)],
    },
    // The note before: the chip right of the grid shows the length a click will draw.
    Shot {
        name: "dl02-last",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 1),
        ],
    },
    // A note in hand: the chip is as long as it.
    Shot {
        name: "dl03-selected",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 1),
            Act::Stage(Play::Laid(play), 2),
        ],
    },
    // The drum view has no length to choose: no switch, no chip.
    Shot {
        name: "dl04-drums",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 1),
            Act::Stage(Play::Laid(play), 3),
        ],
    },
];

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.select_clip_of("Keys");
            s.drawer_open = true;
            s.tab = DrawerTab::Roll;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            s.choose_editor();
            if let Some(c) = s.roll_ctx(g) {
                s.roll.sync(&c);
                s.roll.center(&c);
            }
        }
        1 => {
            let g = &s.geometry(g.size.0, g.size.1);
            s.press(g, Hit::RollLength(true));
        }
        2 => {
            let g = &s.geometry(g.size.0, g.size.1);
            if let Some(c) = s.roll_ctx(g) {
                let Some(note) = c.notes().iter().max_by_key(|n| n.length).map(|n| n.id) else {
                    return;
                };
                s.roll.selected = [note].into_iter().collect();
                s.roll.next_length = c.notes().iter().map(|n| n.length).max().unwrap_or(0);
            }
        }
        _ => {
            s.select_clip_of("Drums");
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            s.choose_editor();
        }
    }
}
