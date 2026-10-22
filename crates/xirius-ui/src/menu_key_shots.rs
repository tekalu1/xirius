//! `--script menu-key`: Shift+F10 in the piano roll opens the selected note's menu at the note,
//! and in the mixer the selected channel's menu at its name (KEY-046). Silent; no OS input.

use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tools::Mods;
use crate::tree::RowKind;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "mk01-roll",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 0)],
    },
    Shot {
        name: "mk02-mixer",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 1)],
    },
];

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    s.drawer_open = true;
    if n == 0 {
        s.select_clip_of("Keys");
        s.tab = DrawerTab::Roll;
        let g = &s.geometry(g.size.0, g.size.1);
        s.resized(g);
        let Some(c) = s.roll_ctx(g) else { return };
        s.roll.sync(&c);
        s.roll.center(&c);
        let Some(note) = c.notes().get(1) else { return };
        s.roll.selected = [note.id].into();
        s.arrange.focus = false;
        s.mapped_key(g, "f10", None, Mods::shift());
    } else {
        s.tab = DrawerTab::Mixer;
        let g = &s.geometry(g.size.0, g.size.1);
        s.resized(g);
        let vocal = crate::arrange::probe::track(s, "Vocal");
        s.selected_row = Some(RowKind::Track(vocal));
        s.arrange.focus = false;
        s.mapped_key(g, "f10", None, Mods::shift());
    }
}
