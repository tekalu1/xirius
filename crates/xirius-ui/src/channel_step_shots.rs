//! `--script channel-step`: KEY-012's next and previous channel and preset (approved 2026-10-15),
//! pressed as keys through the window. In the mixer `]` selects the first channel and then the
//! next, `[` the one before; with a plugin window on top Alt+PageDown loads the next preset and
//! Alt+PageUp, from the first, the last. Silent; no OS input.
use crate::arrange::probe::track;
use crate::instrument::{DEFAULT, Pick, current};
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Studio};

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // The mixer open with nothing selected, then `]`: the first channel.
    Shot {
        name: "cs01-first",
        acts: &[Act::Reset, staged(0), Act::Key("]")],
    },
    // Three more `]` and one `[`: the third channel, its strip in sight.
    Shot {
        name: "cs02-next",
        acts: &[Act::Key("]"), Act::Key("]"), Act::Key("]"), Act::Key("[")],
    },
    // Keys on the built-in sampler, its window on top, then Alt+PageDown: the next preset.
    Shot {
        name: "cs03-preset-next",
        acts: &[staged(1), Act::Key("alt-pagedown")],
    },
    // Alt+PageUp twice: back to the first, and round to the last.
    Shot {
        name: "cs04-preset-round",
        acts: &[Act::Key("alt-pageup"), Act::Key("alt-pageup")],
    },
];

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.popup = None;
            s.drawer_open = true;
            s.tab = DrawerTab::Mixer;
            s.selected_row = None;
            s.resized(g);
            s.rebuild(g);
        }
        _ => {
            let t = track(s, "Keys");
            if s.set_instrument(t, &Pick::built_in(DEFAULT)) {
                let id = current(&s.project().tracks[&t]).expect("the sampler").id;
                s.show_instrument(t, id);
            }
        }
    }
}
