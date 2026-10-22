//! `--script bartools`: the ▾ right of the arrow and the shape button right of the pencil
//! (UI-128), with their menus open, at whatever width `--size` gives.
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "bartools-01-bar",
        acts: &[Act::Reset, staged(0)],
    },
    Shot {
        name: "bartools-02-alt-menu",
        acts: &[staged(1)],
    },
    Shot {
        name: "bartools-03-shape-menu",
        acts: &[staged(2)],
    },
    Shot {
        name: "bartools-04-sine",
        acts: &[staged(3)],
    },
];

/// Stage 0 shuts the drawer, so the arrangement's bar (with its ▾) shows; 1 opens the ▾ menu; 2 shuts it and opens the shape menu;
/// 3 picks the sine wave, so the button shows another shape.
fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.popup = None;
            s.drawer_open = false;
            s.choose_editor();
        }
        1 => s.open_alt_menu(g),
        2 => {
            s.popup = None;
            s.open_shape_menu(g);
        }
        _ => {
            s.popup = None;
            s.song
                .set("tool.draw.shape", serde_json::json!({ "shape": "sine" }))
                .expect("the shape is set");
        }
    }
}
