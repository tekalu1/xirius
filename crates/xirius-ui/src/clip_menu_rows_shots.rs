//! `--script clip-menu-rows`: the 1.0 rows of the clip menus (CTX-057): 処理 › and バウンス › of
//! an audio clip (S02), バウンス › and the MIDI file of a MIDI clip (S03), and バウンス › of
//! several clips (S04). ARA is after 1.0 (EDT-023) and has no row. Nothing is chosen but the
//! submenus. Silent; no OS input.

use crate::arrange::probe::{clip_mid, track};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Studio};

const fn laid(n: u8) -> Act {
    Act::Stage(Play::Laid(play), n)
}

pub const SHOTS: &[Shot] = &[
    // An audio clip: 処理 › with ノーマライズ, 逆再生 and テンポを検出.
    Shot {
        name: "cmr01-audio-process",
        acts: &[Act::Reset, laid(0), laid(1)],
    },
    // An audio clip: バウンス › in place or onto a new track.
    Shot {
        name: "cmr02-audio-bounce",
        acts: &[laid(2)],
    },
    // A MIDI clip: バウンス › and MIDI ファイルとして書き出す… in the menu.
    Shot {
        name: "cmr03-midi-menu",
        acts: &[laid(3)],
    },
    // A MIDI clip and an audio clip: バウンス › for both.
    Shot {
        name: "cmr04-several-bounce",
        acts: &[laid(4)],
    },
];

/// Right-clicks the first clip of the track `name`.
fn menu(s: &mut Studio, g: &Geometry, name: &str) {
    s.popup = None;
    s.ctx = None;
    let (x, y) = clip_mid(s, g, name, 0);
    assert!(s.scene_context(g, x, y), "no menu on {name}");
}

fn labels(s: &Studio) -> Vec<String> {
    s.context_rows()
        .into_iter()
        .filter(|r| !r.gap)
        .map(|r| r.label)
        .collect()
}

fn play(s: &mut Studio, g: &Geometry, n: u8) {
    if n == 0 {
        s.drawer_open = false;
        let g = &s.geometry(g.size.0, g.size.1);
        s.resized(g);
        return;
    }
    let g = &s.geometry(g.size.0, g.size.1);
    match n {
        1 => {
            menu(s, g, "Vocal");
            assert!(s.context_choose(g, "処理"));
            assert_eq!(
                labels(s),
                ["戻る", "ノーマライズ", "逆再生", "テンポを検出"]
            );
        }
        2 => {
            menu(s, g, "Vocal");
            assert!(s.context_choose(g, "バウンス"));
        }
        3 => {
            menu(s, g, "Keys");
            let rows = labels(s);
            assert!(rows.iter().any(|r| r == "バウンス"), "{rows:?}");
            assert!(rows.iter().any(|r| r == "MIDI ファイルとして書き出す…"));
        }
        _ => {
            menu(s, g, "Keys");
            s.popup = None;
            s.ctx = None;
            let vt = track(s, "Vocal");
            let vc = s.project().tracks[&vt].clips[0].id;
            s.arrange.clips.insert((vt, vc));
            menu(s, g, "Keys");
            assert_eq!(s.arrange.clips.len(), 2);
            assert!(s.context_choose(g, "バウンス"));
            assert_eq!(
                labels(s),
                ["戻る", "クリップをバウンス", "新しいトラックにバウンス"]
            );
        }
    }
}
