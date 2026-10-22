//! CTX-008: the right-click menus whose row names were made to agree (追加・削除・ポイント・… の
//! 言い回し), each one opened on the sample song and left open for the picture. The arrangement
//! alone for the track, the folder and the ruler; the mixer for a channel's effect and its send.
use crate::arrange::probe::{folder, row_mid, track};
use crate::mixer::MixerHit;
use crate::shot::{Act, Play, Shot};
use crate::studio::{DrawerTab, Geometry, Studio};
use crate::tree::RowKind;
use serde_json::json;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const CTXNAMES: &[Shot] = &[
    Shot {
        name: "n01-track-menu",
        acts: &[Act::Reset, stage(0)],
    },
    Shot {
        name: "n02-track-add-rows",
        acts: &[stage(1)],
    },
    Shot {
        name: "n03-folder-menu",
        acts: &[stage(2)],
    },
    Shot {
        name: "n04-ruler-menu",
        acts: &[stage(3)],
    },
    Shot {
        name: "n05-mixer-effect",
        acts: &[stage(4)],
    },
    Shot {
        name: "n06-mixer-send",
        acts: &[stage(5)],
    },
];

/// Chooses the row named `label` of the open menu, as a press on its cell does.
fn choose(s: &mut Studio, g: &Geometry, label: &str) {
    let layout = s.popup.as_ref().expect("a menu").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
        .unwrap_or_else(|| panic!("missing row {label}"));
    assert!(cell.live(), "disabled row {label}");
    let (x, y) = (
        cell.rect.x + cell.rect.w / 2.0,
        cell.rect.y + cell.rect.h / 2.0,
    );
    let hit = s.hit(g, x, y);
    s.press(g, hit);
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    s.popup = None;
    // The arrangement alone, tall enough for every row.
    s.drawer_open = n >= 4;
    s.tab = DrawerTab::Mixer;
    let g = &s.geometry(g.size.0, g.size.1);
    s.resized(g);
    match n {
        0 | 1 => {
            let t = track(s, "Vocal");
            let (x, y) = row_mid(s, g, RowKind::Track(t));
            assert!(s.scene_context(g, x, y));
            if n == 1 {
                choose(s, g, "下にトラックを追加");
            }
        }
        2 => {
            let f = folder(s, "Rhythm");
            let (x, y) = row_mid(s, g, RowKind::Folder(f));
            assert!(s.scene_context(g, x, y));
        }
        3 => {
            let bar = i64::from(s.project().ppq) * 4;
            let x = s.tick_x(g, 2 * bar) + 2.0;
            assert!(s.scene_context(g, x, g.ruler.y + g.ruler.h - 4.0));
        }
        _ => {
            let t = track(s, "Vocal");
            let hit = if n == 4 {
                assert!(s.run_command(
                    "plugin.add",
                    json!({ "track": t.to_string(), "name": "EQ", "kind": "effect",
                            "format": "builtin", "uid": "EQ" }),
                ));
                let id = s.project().tracks[&t].inserts.last().expect("added").id;
                MixerHit::Open(t, id)
            } else {
                assert!(
                    s.run_command("track.add.bus", json!({ "name": "Reverb", "into": "root" }))
                );
                let bus = track(s, "Reverb");
                assert!(s.run_command(
                    "send.add",
                    json!({ "ids": [t.to_string()], "to": bus.to_string() })
                ));
                let id = s.project().tracks[&t].sends.last().expect("sent").id;
                MixerHit::SendPre(t, id)
            };
            let g = &s.geometry(g.size.0, g.size.1);
            let (x, y) = s.mixer_center(g, hit).expect("on screen");
            assert!(s.scene_context(g, x, y));
        }
    }
}
