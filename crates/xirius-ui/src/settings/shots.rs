//! The settings' run of pictures (`--script settings`): each section as the
//! gear opens it, the mouse behind its seg, a key taken twice, the preset's
//! dropdown, and the toast after a preset.

use super::{Do, MenuKind, Section, Sub};
use crate::studio::{Geometry, Hit, Rect, Studio};
use crate::tools::Mods;

pub const SHOTS: &[crate::shot::Shot] = {
    use crate::shot::{Act, Shot};
    &[
        Shot {
            name: "01-general",
            acts: &[Act::Settings(0)],
        },
        Shot {
            name: "02-audio",
            acts: &[Act::Settings(1)],
        },
        Shot {
            name: "03-midi",
            acts: &[Act::Settings(2)],
        },
        Shot {
            name: "04-plugins",
            acts: &[Act::Settings(3)],
        },
        Shot {
            name: "05-edit",
            acts: &[Act::Settings(4)],
        },
        Shot {
            name: "06-keys",
            acts: &[Act::Settings(5)],
        },
        Shot {
            name: "07-mouse",
            acts: &[Act::Settings(6)],
        },
        Shot {
            name: "08-keys-conflict",
            acts: &[Act::Settings(7)],
        },
        Shot {
            name: "09-preset-dropdown",
            acts: &[Act::Settings(8)],
        },
        Shot {
            name: "10-preset-applied",
            acts: &[Act::Settings(9)],
        },
    ]
};

/// Presses the item that does `act` at its middle, as a person would.
fn press(s: &mut Studio, g: &Geometry, act: &Do) {
    let scene = s.settings_scene(g);
    let hit = scene.items.iter().filter(|i| i.act == *act).find_map(|i| {
        let (x, y) = (i.r.x + i.r.w / 2.0, i.r.y + i.r.h / 2.0);
        (scene.item_at(x, y).map(|h| &h.act) == Some(act)).then_some((x, y))
    });
    if let Some((x, y)) = hit {
        s.settings_press(g, x, y);
    }
}

/// Plays stage `stage` of [`SHOTS`].
pub fn stage(s: &mut Studio, g: &Geometry, stage: u8) {
    let section = |n: u8| Section::ALL[usize::from(n)];
    match stage {
        0..=5 => {
            s.close_settings();
            if stage == 0 {
                // The gear at the bottom of the rail.
                s.press(g, Hit::RailSettings);
            } else {
                s.open_settings(section(stage));
            }
        }
        6 => {
            s.open_settings(Section::Keys);
            press(s, g, &Do::Sub(Sub::Mouse));
        }
        7 => {
            s.open_settings(Section::Keys);
            press(s, g, &Do::Sub(Sub::Keyboard));
            let from = s.with_settings(|p| p.keymap.keys_of("roll", "note.delete"));
            s.settings_do(
                g,
                Do::Capture {
                    ctx: "roll".into(),
                    cmd: "note.delete".into(),
                    from: from.first().cloned(),
                },
                Rect::default(),
            );
            let none = Mods::default();
            s.settings_key("q", none);
            // The search by Q shows both rows.
            s.settings_key("q", none);
        }
        8 => {
            s.close_settings();
            s.open_settings(Section::Keys);
            press(s, g, &Do::Menu(MenuKind::Preset));
            s.settings_key("down", Mods::default());
        }
        _ => {
            s.settings_key("down", Mods::default());
            s.settings_key("enter", Mods::default());
            // The pointer has left the button, so its tip is gone.
            s.settings_hover(0.0, 0.0);
        }
    }
}
