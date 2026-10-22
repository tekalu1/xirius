//! The shots of the instrument's screens (`xirius-ui --shot <dir> --script
//! instrument`). Each stage is an [`Act::Stage`] played by [`stage`].

use crate::shot::{Act, Play, Shot};
use crate::song::Song;
use crate::start::Entry;
use crate::studio::{Geometry, Hit, Studio};

use super::window::Part;
use super::{DEFAULT, Pick, current};

/// Stage `n` of this script, played by [`stage`].
const fn staged(n: u8) -> Act {
    Act::Stage(Play::Laid(stage), n)
}

pub const SHOTS: &[Shot] = &[
    // UI-098: the keys button at the head of Keys opens its instrument's window.
    Shot {
        name: "i01-instrument-window",
        acts: &[Act::Reset, staged(0)],
    },
    // UI-098: a knob turned and the preset list open.
    Shot {
        name: "i02-instrument-preset",
        acts: &[staged(1)],
    },
    // UI-074: a new song shows where to start; the pointer is on 「MIDI を演奏」.
    Shot {
        name: "i03-start-empty",
        acts: &[staged(2)],
    },
    // UI-074: after 「MIDI を演奏」, an armed instrument track and A held down.
    Shot {
        name: "i04-start-midi-played",
        acts: &[staged(3)],
    },
    // UI-076: a file dragged over a new song lights the place to drop it.
    Shot {
        name: "i05-drop-over",
        acts: &[staged(4)],
    },
    // UI-076: a dropped .mid is a Sampler track with its clip, open in the roll.
    Shot {
        name: "i06-drop-mid",
        acts: &[staged(5)],
    },
    // CTX-040: a right press on the instrument row in the mixer.
    Shot {
        name: "i07-instrument-row-menu",
        acts: &[Act::Reset, staged(6)],
    },
    // CTX-048: a right press on an instrument of the browser, Keys selected.
    Shot {
        name: "i08-browser-instrument-menu",
        acts: &[Act::Reset, staged(7)],
    },
    // CTX-048: Drum Kit dragged from the browser over Keys.
    Shot {
        name: "i09-browser-instrument-drag",
        acts: &[Act::Reset, staged(8)],
    },
];

/// Two bars for the drop shot: a line over C and G in the bass.
const MELODY: &[u8] = &[
    0x4d, 0x54, 0x68, 0x64, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x01, 0x01, 0xe0, 0x4d, 0x54,
    0x72, 0x6b, 0x00, 0x00, 0x00, 0x6c, 0x00, 0x90, 0x3c, 0x60, 0x00, 0x90, 0x30, 0x50, 0x00, 0x90,
    0x37, 0x50, 0x81, 0x5c, 0x80, 0x3c, 0x00, 0x14, 0x90, 0x40, 0x60, 0x81, 0x5c, 0x80, 0x40, 0x00,
    0x14, 0x90, 0x43, 0x60, 0x81, 0x5c, 0x80, 0x43, 0x00, 0x14, 0x90, 0x48, 0x60, 0x81, 0x5c, 0x80,
    0x48, 0x00, 0x14, 0x80, 0x30, 0x00, 0x00, 0x80, 0x37, 0x00, 0x00, 0x90, 0x47, 0x60, 0x00, 0x90,
    0x2b, 0x50, 0x00, 0x90, 0x32, 0x50, 0x81, 0x5c, 0x80, 0x47, 0x00, 0x14, 0x90, 0x43, 0x60, 0x81,
    0x5c, 0x80, 0x43, 0x00, 0x14, 0x90, 0x40, 0x60, 0x81, 0x5c, 0x80, 0x40, 0x00, 0x14, 0x90, 0x3e,
    0x60, 0x81, 0x5c, 0x80, 0x3e, 0x00, 0x14, 0x80, 0x2b, 0x00, 0x00, 0x80, 0x32, 0x00, 0x00, 0xff,
    0x2f, 0x00,
];

fn track_named(s: &Studio, name: &str) -> Option<xirius_core::id::TrackId> {
    let p = s.project();
    p.tracks.values().find(|t| t.name == name).map(|t| t.id)
}

/// Plays stage `stage` of [`SHOTS`].
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    match stage {
        0 => {
            s.drawer_open = false;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let Some(t) = track_named(s, "Keys") else {
                return;
            };
            s.set_instrument(t, &Pick::built_in(DEFAULT));
            s.press(g, Hit::Keys(t));
        }
        1 => {
            let Some(t) = track_named(s, "Keys") else {
                return;
            };
            let Some(id) = current(&s.project().tracks[&t]).map(|x| x.id) else {
                return;
            };
            if let Some((x, y)) = s.instrument_center(g, id, Part::Knob(4)) {
                s.instrument_down(g, x, y);
                s.instrument_moved(g, x, y - 40.0, false);
                s.instrument_up();
            }
            if let Some((x, y)) = s.instrument_center(g, id, Part::Preset) {
                s.instrument_down(g, x, y);
            }
        }
        2 => {
            let mode = s.transport.mode;
            *s = Studio::new(Song::empty());
            s.transport.mode = mode;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            s.hover = Hit::Start(Entry::Midi);
        }
        3 => {
            s.hover = Hit::None;
            s.press(g, Hit::Start(Entry::Midi));
            s.pc_key_down("a", false);
        }
        4 => {
            self::stage(s, g, 2);
            s.hover = Hit::None;
            s.drop_hover(true);
        }
        5 => {
            let dir = std::env::temp_dir().join(format!("xirius-shot-{}", std::process::id()));
            let path = dir.join("melody.mid");
            if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, MELODY).is_err() {
                return;
            }
            let z = crate::start::zone(g);
            s.drop_files(g, z.x + z.w / 2.0, z.y + z.h / 2.0, &[path]);
            let _ = std::fs::remove_dir_all(&dir);
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Roll;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            if let Some(c) = s.roll_ctx(g) {
                s.roll.sync(&c);
                s.roll.center(&c);
            }
        }
        6 => {
            s.drawer_open = true;
            s.tab = crate::studio::DrawerTab::Mixer;
            let g = &s.geometry(g.size.0, g.size.1);
            s.resized(g);
            let Some(t) = track_named(s, "Keys") else {
                return;
            };
            s.set_instrument(t, &Pick::built_in(DEFAULT));
            let Some(id) = current(&s.project().tracks[&t]).map(|x| x.id) else {
                return;
            };
            if let Some((x, y)) = s.mixer_center(g, crate::mixer::MixerHit::Open(t, id)) {
                s.context_press(g, x, y);
            }
        }
        7 => {
            let Some(t) = track_named(s, "Keys") else {
                return;
            };
            s.selected_row = Some(crate::tree::RowKind::Track(t));
            if let Some((x, y)) = browser_item(s, g, "Sampler") {
                s.lib_context(g, x, y);
            }
        }
        8 => {
            let Some(t) = track_named(s, "Keys") else {
                return;
            };
            // Keys whole on screen.
            s.scroll_by(g, 0.0, -240.0, false);
            let Some(row) = s.tree().row_of(crate::tree::RowKind::Track(t)).copied() else {
                return;
            };
            let to = (
                g.lanes.x + g.lanes.w * 0.3,
                g.column.y + row.y - s.scroll.1 + row.h / 2.0,
            );
            let Some((x, y)) = browser_item(s, g, "Drum Kit") else {
                return;
            };
            let hit = s.hit(g, x, y);
            s.press(g, hit);
            s.lib_grab(g, hit, x, y, 1);
            s.lib_move(to.0, to.1);
        }
        _ => {}
    }
}

/// The middle of an item of the browser's list.
fn browser_item(s: &Studio, g: &Geometry, name: &str) -> Option<(f32, f32)> {
    use crate::browser::{BRow, BrowserHit};
    let l = s.browser.layout(g.panel);
    let i = l
        .rows
        .iter()
        .position(|p| matches!(&p.row, BRow::Item { item, .. } if item.name == name))?;
    s.browser.center_of(&l, BrowserHit::Item(i))
}
