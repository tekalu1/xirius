//! `--script keys-view-menu`: 鍵盤の表示 in the menu of the roll's keys (CTX-058), the switch of
//! the drawer's bar between the note names and the drum names. Silent; no OS input.

use crate::drum_view_shots::play as part;
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};

pub const SHOTS: &[Shot] = &[
    // A key of the piano: 鍵盤の表示 › with 音名 checked.
    Shot {
        name: "kvm01-keys-menu",
        acts: &[
            Act::Reset,
            Act::Stage(Play::Laid(part), 0),
            Act::Stage(Play::Laid(part), 1),
            Act::Stage(Play::Laid(play), 0),
        ],
    },
    // ドラムの名前 chosen there: the keys show the drum names.
    Shot {
        name: "kvm02-drum-names",
        acts: &[Act::Stage(Play::Laid(play), 1)],
    },
    // A drum row's name: 鍵盤の表示 › with ドラムの名前 checked, above the row's own rows.
    Shot {
        name: "kvm03-drum-row-menu",
        acts: &[Act::Stage(Play::Laid(play), 2)],
    },
];

/// Right-clicks the key or the row's name of `pitch` and opens 鍵盤の表示.
fn view_menu(s: &mut Studio, g: &Geometry, pitch: u8) {
    let Some(c) = s.roll_ctx(g) else { return };
    s.roll.scroll_to_pitch(&c, pitch);
    let f = s.roll.frame(c.body);
    let v = s.roll.view(&c);
    s.scene_context(g, f.keys.x + f.keys.w / 2.0, v.y(pitch) + v.row_h / 2.0);
    assert!(s.context_choose(g, "鍵盤の表示"));
}

fn on(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| r.on)
        .map(|r| r.label)
        .collect()
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        0 => {
            view_menu(s, g, 38);
            assert_eq!(on(s), ["音名"]);
        }
        1 => {
            assert!(s.context_choose(g, "ドラムの名前"));
            assert_eq!(s.roll_ctx(g).and_then(|c| c.drum).as_deref(), Some("gm"));
        }
        _ => {
            view_menu(s, g, 38);
            assert_eq!(on(s), ["ドラムの名前"]);
        }
    }
}
