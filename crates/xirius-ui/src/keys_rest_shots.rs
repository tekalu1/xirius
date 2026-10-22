//! `--script keys-rest`: the key settings' table with the rest of the Studio Pro key table
//! (KEY-012): the keys the table looks up, and the new rows by name. Run with `--light` for the
//! light theme. No OS input.

use xirius_prefs::chord::{MOD, SHIFT};

use crate::settings::{Do, Section};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Rect, Studio};

pub const SHOTS: &[Shot] = &[
    // F6: the browser's instruments.
    Shot {
        name: "k301-f6",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 0)],
    },
    // Tab: the next part of the window.
    Shot {
        name: "k302-tab",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 1)],
    },
    // Ctrl+T: go to a time.
    Shot {
        name: "k303-ctrl-t",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 2)],
    },
    // Ctrl+Shift+S: every solo off.
    Shot {
        name: "k304-ctrl-shift-s",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 3)],
    },
    // The rows named 「表示」: the browser's tabs, and finding a track or a channel.
    Shot {
        name: "k305-name-show",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 4)],
    },
    // The rows named 「ページ」.
    Shot {
        name: "k306-name-page",
        acts: &[Act::Reset, Act::Stage(Play::Laid(play), 5)],
    },
];

fn act(s: &mut Studio, g: &Geometry, act: Do) {
    s.settings_do(g, act, Rect::default());
}

fn by_key(s: &mut Studio, g: &Geometry, mods: u8, key: &str) {
    for bit in [MOD, SHIFT] {
        if mods & bit != 0 {
            act(s, g, Do::KbMod(bit));
        }
    }
    act(s, g, Do::KbKey(key.into()));
}

fn by_name(s: &mut Studio, g: &Geometry, query: &str) {
    act(s, g, Do::KeyMode(false));
    // As typed into the search field.
    let ui = s.settings.as_mut().unwrap();
    ui.field.as_mut().unwrap().set(query.into());
    ui.query = query.into();
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    s.open_settings(Section::Keys);
    match n {
        0 => by_key(s, g, 0, "F6"),
        1 => by_key(s, g, 0, "Tab"),
        2 => by_key(s, g, MOD, "T"),
        3 => by_key(s, g, MOD | SHIFT, "S"),
        4 => by_name(s, g, "表示"),
        _ => by_name(s, g, "ページ"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stage_opens_the_key_table_with_its_search() {
        for n in 0..6 {
            let mut s = Studio::sample();
            let g = s.geometry(1440.0, 860.0);
            play(&mut s, &g, n);
            let ui = s.settings.as_ref().expect("the settings are open");
            assert_eq!(ui.section, Section::Keys, "{n}");
            if n < 4 {
                assert!(ui.key_mode && ui.q_key.is_some(), "{n}");
            } else {
                assert!(!ui.key_mode && !ui.query.is_empty(), "{n}");
                assert_eq!(ui.field.as_ref().unwrap().text(), ui.query, "{n}");
            }
        }
    }
}
