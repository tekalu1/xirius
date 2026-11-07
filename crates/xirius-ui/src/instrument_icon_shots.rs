//! Approved front-facing keyboard, in the track head, add list and both strip widths.
//! Run `--script instrument-icon` in either theme; all input stays inside the window.

use crate::arrange::probe::track;
use crate::browser::RailTab;
use crate::shot::{Act, Play, Shot, Target};
use crate::studio::{DrawerTab, Studio};

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "icon01-track",
        acts: &[
            Act::Reset,
            Act::Press(Target::Chevron("Rhythm")),
            Act::Stage(Play::Studio(prepare), 0),
            Act::Press(Target::Rail(RailTab::Instruments)),
            Act::Press(Target::Track("Keys")),
        ],
    },
    Shot {
        name: "icon02-add",
        acts: &[Act::Press(Target::AddTrack)],
    },
    Shot {
        name: "icon03-strip-116",
        acts: &[Act::Key("escape"), Act::Stage(Play::Studio(prepare), 1)],
    },
    Shot {
        name: "icon04-strip-252",
        acts: &[Act::Stage(Play::Studio(prepare), 2)],
    },
];

fn prepare(s: &mut Studio, n: u8) {
    s.mixer.scan.auto = false;
    s.drawer_open = n != 0;
    if n != 0 {
        s.tab = DrawerTab::Mixer;
        s.drawer_size.mixer = Some(520.0);
        if n == 2 {
            s.mixer.wide.insert(track(s, "Keys"));
        }
    }
}
