//! `--script drum-view`: the roll shown as the keyboard and as the rows of a drum map, the
//! switch and the map menu in the drawer's bar (EDT-006, EDT-058), and the rows hidden, moved
//! and renamed for the track. The menus of the rows are in `--script drum-rows`. Silent; no OS
//! input.

use serde_json::json;

use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Hit, Studio};

pub const SHOTS: &[Shot] = &[
    // A drum part opens as the drum rows: the names at the left, a diamond per hit.
    Shot {
        name: "dv01-drums",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 0)],
    },
    // The same part as the keyboard.
    Shot {
        name: "dv02-keys",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 1),
        ],
    },
    // The map button pressed: the maps, the one in use marked.
    Shot {
        name: "dv03-map-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 2),
        ],
    },
    // The kit's own rows.
    Shot {
        name: "dv04-kit-map",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 3),
        ],
    },
    // The rows changed for the track: two cymbals hidden, the hi-hat on top, the kick renamed.
    Shot {
        name: "dv05-rows-edited",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 4),
        ],
    },
    // The menu of a row's name.
    Shot {
        name: "dv06-name-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(play), 0),
            Act::Stage(Play::Laid(play), 4),
            Act::Stage(Play::Laid(play), 5),
        ],
    },
];

/// 0 opens the drum part, 4 changes its rows for the track (also used by `--script drum-rows`).
pub(crate) fn play(s: &mut Studio, g: &Geometry, n: u8) {
    match n {
        0 => {
            s.select_clip_of("Drums");
            s.drawer_open = true;
            s.tab = DrawerTab::Roll;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let Some(c) = s.roll_ctx(g) else { return };
            let ids: Vec<String> = c.notes().iter().map(|n| n.id.to_string()).collect();
            if !ids.is_empty() {
                let _ = s.song.run("note.delete", json!({ "ids": ids }));
            }
            let mut notes = Vec::new();
            for beat in 0..8 {
                let at = beat * 960;
                if beat % 4 == 0 || beat % 4 == 2 {
                    notes.push(json!({ "at": at, "pitch": 36, "length": 240, "velocity": 110 }));
                }
                if beat % 4 == 1 || beat % 4 == 3 {
                    notes.push(json!({ "at": at, "pitch": 38, "length": 240, "velocity": 100 }));
                }
                for half in 0..2 {
                    notes.push(
                        json!({ "at": at + half * 480, "pitch": 42, "length": 240, "velocity": 70 }),
                    );
                }
            }
            let _ = s.song.run(
                "note.put",
                json!({ "clip": c.clip.id.to_string(), "notes": notes }),
            );
            let _ = s.song.run(
                "roll.view",
                json!({ "clip": c.clip.id.to_string(), "mode": "drums" }),
            );
            s.after_edit();
            s.resized(g);
            if let Some(c) = s.roll_ctx(g) {
                s.roll.sync(&c);
                s.roll.center(&c);
            }
        }
        1 => {
            let g = &s.geometry(g.size.0, g.size.1);
            s.press(g, Hit::RollView(false));
        }
        2 => {
            let g = &s.geometry(g.size.0, g.size.1);
            s.press(g, Hit::RollMap);
        }
        3 => {
            let g = &s.geometry(g.size.0, g.size.1);
            s.press(g, Hit::RollMap);
            s.context_choose(g, "ドラムキット");
        }
        4 => {
            let g = &s.geometry(g.size.0, g.size.1);
            let Some(c) = s.roll_ctx(g) else { return };
            let clip = c.clip.id.to_string();
            for pitch in [51, 49] {
                let _ = s
                    .song
                    .run("roll.drum.hide", json!({ "clip": clip, "pitch": pitch }));
            }
            let _ = s.song.run(
                "roll.drum.move",
                json!({ "clip": clip, "pitch": 42, "by": -20 }),
            );
            let _ = s.song.run(
                "roll.drum.rename",
                json!({ "clip": clip, "pitch": 36, "name": "808 Kick" }),
            );
            s.resized(g);
            if let Some(c) = s.roll_ctx(g) {
                s.roll.sync(&c);
                s.roll.center(&c);
            }
        }
        _ => {
            let g = &s.geometry(g.size.0, g.size.1);
            let Some(c) = s.roll_ctx(g) else { return };
            let f = s.roll.frame(c.body);
            let v = s.roll.view(&c);
            s.scene_context(g, f.keys.x + f.keys.w / 2.0, v.y(38) + v.row_h / 2.0);
        }
    }
}
