//! The 1.0 export panel (EXP-019..023, EXP-026, CTX-060, UI-280, UI-281): stems that go stale
//! track by track with the dots of what changed, writing again only those, the same name written
//! over its row, a MIDI clip's "MIDI ファイルとして書き出す…" and the file brought back at the
//! head of its range. The files a replaced export sends to the recycle bin go to
//! `XIRIUS_TRASH_DIR`, which the run must name.
use crate::{
    arrange::probe::{clip_mid, track},
    ctxfolder_shots::{choose, press},
    export::State,
    shot::{Act, Play, Shot, Target},
    studio::Studio,
    topbar::BarButton,
};
use gpui::{App, MouseButton, Window};
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}

const fn window(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "v1-01-stale",
        acts: &[
            Act::Reset,
            stage(0),
            Act::Press(Target::Top(BarButton::Export)),
            Act::Press(Target::PopupText("ステム")),
            Act::Press(Target::PopupText("ループ範囲")),
            Act::Press(Target::PopupText("Reverb")),
            Act::Press(Target::PopupText("書き出す")),
            stage(1),
        ],
    },
    Shot {
        name: "v1-02-dot-name",
        acts: &[Act::Hover(Target::PopupTip("Vocal"))],
    },
    Shot {
        name: "v1-03-written-again",
        acts: &[Act::Press(Target::PopupText("更新")), stage(2)],
    },
    Shot {
        name: "v1-04-same-name",
        acts: &[Act::Press(Target::PopupText("書き出す")), stage(3)],
    },
    Shot {
        name: "v1-05-midi-clip-menu",
        acts: &[Act::Key("escape"), window(0)],
    },
    Shot {
        name: "v1-06-midi-panel",
        acts: &[window(1)],
    },
    Shot {
        name: "v1-07-midi-written",
        acts: &[Act::Press(Target::PopupText("書き出す")), stage(4)],
    },
    Shot {
        name: "v1-08-imported",
        acts: &[
            Act::Press(Target::PopupText("曲に取り込む")),
            Act::Key("escape"),
            stage(5),
        ],
    },
];

fn dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/export-v1-ui-{}", std::process::id()))
}

fn trashed() -> usize {
    std::env::var_os("XIRIUS_TRASH_DIR")
        .and_then(|d| std::fs::read_dir(d).ok())
        .map_or(0, Iterator::count)
}

fn prepare(s: &mut Studio, n: u8) {
    let ppq = i64::from(s.project().ppq);
    match n {
        0 => {
            assert!(
                std::env::var_os("XIRIUS_TRASH_DIR").is_some(),
                "a replaced export would go to the PC's recycle bin"
            );
            s.command(|song| song.save_as(&dir()));
            s.drawer_open = false;
            s.run_command("loop.set", json!({"start": ppq * 16, "end": ppq * 32}));
            s.run_command("track.add.bus", json!({"name": "Reverb"}));
            let bus = track(s, "Reverb");
            let vocal = track(s, "Vocal");
            s.run_command("send.add", json!({"ids": [vocal], "to": bus, "level": -6}));
            assert!(s.error.is_none(), "{:?}", s.error);
        }
        1 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert_eq!(s.export_view().cards[0].state, State::Done);
            // Bass and Vocal change; Vocal sends to Reverb, so its stem is stale too.
            for name in ["Bass", "Vocal"] {
                let t = track(s, name);
                s.run_command("track.volume.set", json!({"ids": [t], "db": -3}));
            }
            assert!(s.error.is_none(), "{:?}", s.error);
            let card = &s.export_view().cards[0];
            assert_eq!(card.stale, Some(true));
            assert_eq!(card.old, 3, "{:?}", card.kids);
            let dots: Vec<_> = card.dots.iter().map(|d| d.name.as_str()).collect();
            assert_eq!(dots, ["Bass", "Vocal"]);
            let reverb = card.kids.iter().find(|k| k.name == "Reverb").unwrap();
            assert_eq!(reverb.stale, Some(true));
            assert_eq!(reverb.dots.len(), 1);
        }
        2 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let card = &s.export_view().cards[0];
            assert_eq!(card.state, State::Done);
            assert_eq!(card.stale, Some(false));
            assert_eq!(card.old, 0);
        }
        3 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let v = s.export_view();
            assert_eq!(v.cards.len(), 1, "the same name takes the row's place");
            assert_eq!(v.cards[0].state, State::Done);
            assert!(trashed() > 0, "the files it replaced");
        }
        4 => {
            crate::shot::run_for(s, 60.0, |s| !s.export.running());
            assert!(s.error.is_none(), "{:?}", s.error);
            let card = &s.export_view().cards[0];
            assert!(card.midi);
            assert_eq!(card.state, State::Done);
        }
        5 => {
            assert!(s.error.is_none(), "{:?}", s.error);
            let project = s.project();
            let keys = &project.tracks[&track(s, "Keys")];
            let new = project
                .tracks
                .values()
                .find(|t| t.kind == keys.kind && t.name != "Keys" && t.clips.len() == 1)
                .expect("the imported track");
            assert_eq!(
                new.clips[0].start, keys.clips[0].start,
                "the head of its range"
            );
            let last = s.tree().rows.last().unwrap();
            s.scroll.1 = last.y;
        }
        _ => unreachable!(),
    }
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            let at = {
                let s = studio.borrow();
                let g = crate::screen::geometry(&s, window);
                clip_mid(&s, &g, "Keys", 0)
            };
            press(at, MouseButton::Right, window, cx);
            assert!(studio.borrow().context_open());
        }
        _ => {
            choose(studio, "MIDI ファイルとして書き出す…", window, cx);
            let s = studio.borrow();
            assert!(s.export.midi && !s.export.stems);
            assert_eq!(s.export.midi_picked, Some(vec![track(&s, "Keys")]));
        }
    }
}
