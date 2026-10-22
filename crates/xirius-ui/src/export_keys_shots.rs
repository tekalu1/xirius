//! Export keys use the same input map as the window, including the current panel mode.
use crate::{
    shot::{Act, Play, Shot, Target},
    studio::Studio,
};
use serde_json::json;

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "keys01-mixdown",
        acts: &[
            Act::Reset,
            staged(0),
            Act::Key("ctrl-shift-e"),
            Act::Key("ctrl-shift-e"),
            Act::Key("ctrl-shift-e"),
            Act::Press(Target::PopupText("ミックスダウン")),
            Act::Press(Target::PopupText("ループ範囲")),
            Act::Key("ctrl-enter"),
            staged(1),
        ],
    },
    Shot {
        name: "keys02-stems",
        acts: &[
            Act::Press(Target::PopupText("ステム")),
            Act::Key("ctrl-enter"),
            staged(1),
        ],
    },
    Shot {
        name: "keys03-closed",
        acts: &[Act::Key("escape"), staged(2)],
    },
];

const fn staged(n: u8) -> Act {
    Act::Stage(Play::Studio(stage), n)
}

fn stage(s: &mut Studio, n: u8) {
    match n {
        0 => {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../target/export-keys-{}", std::process::id()));
            s.command(|song| song.save_as(&dir));
            s.run_command("loop.set", json!({"start":0,"end":1920}));
        }
        1 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let cards = s.export_view().cards;
            assert_eq!(
                cards[0].state,
                crate::export::State::Done,
                "{}",
                cards[0].error
            );
            assert_eq!(cards[0].stems, s.export.stems);
        }
        2 => assert!(s.popup.is_none()),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{popup::PopupKind, tools::Mods};

    fn key(s: &mut Studio, key: &str, shift: bool) -> bool {
        let g = s.geometry(1600.0, 900.0);
        s.mapped_key(
            &g,
            key,
            None,
            Mods {
                ctrl: true,
                shift,
                alt: false,
            },
        )
    }

    #[test]
    fn keys_toggle_the_panel_export_its_mode_and_never_start_an_export_outside_it() {
        let mut s = Studio::sample();
        stage(&mut s, 0);
        assert!(!key(&mut s, "enter", false));
        assert!(s.export.jobs.is_empty());
        assert!(key(&mut s, "e", true));
        assert_eq!(s.popup.as_ref().unwrap().kind, PopupKind::Export);
        assert!(key(&mut s, "e", true));
        assert!(s.popup.is_none());
        assert!(key(&mut s, "e", true));
        s.export_action(crate::export::Act::Range(crate::export::Range::Loop));
        for stems in [false, true] {
            s.export_action(crate::export::Act::Mode(stems));
            if !stems {
                s.popup.as_mut().unwrap().naming = Some("Short mix".into());
            }
            assert!(key(&mut s, "enter", false));
            stage(&mut s, 1);
        }
        assert_eq!(s.export.jobs.len(), 2);
        let g = s.geometry(1600.0, 900.0);
        assert!(s.mapped_key(&g, "escape", None, Mods::default()));
        assert!(s.popup.is_none());
        let dir = s.song.dir().unwrap();
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
