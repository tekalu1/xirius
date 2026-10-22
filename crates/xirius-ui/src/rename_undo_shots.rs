//! `--script rename-undo`: a file the song uses renamed from the menu of the files tab, then
//! Ctrl+Z and Ctrl+Y through the window's key path rename it back and again with its clip
//! (BRW-013); a name taken in between keeps the clip on the new name and the error is shown.
//! The files are a fixture under `target`. Silent; no OS input.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{App, Window};
use xirius_core::model::ClipBody;

use crate::browser::RailTab;
use crate::browser_files_menu_shots::{choose, press_row};
use crate::import_shots::file_row;
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // The kick the song uses, renamed from its menu: the list shows the new name.
    Shot {
        name: "ru01-renamed",
        acts: &[Act::Reset, stage(0)],
    },
    // Ctrl+Z: the name on the disk and the clip go back.
    Shot {
        name: "ru02-undone",
        acts: &[Act::Key("ctrl-z"), stage(1)],
    },
    // Ctrl+Y: renamed again.
    Shot {
        name: "ru03-redone",
        acts: &[Act::Key("ctrl-y"), stage(2)],
    },
    // The old name is taken in between: Ctrl+Z changes nothing and says why.
    Shot {
        name: "ru04-cannot-undo",
        acts: &[stage(3), Act::Key("ctrl-z"), stage(4)],
    },
];

fn top() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/rename-undo-fixture/素材")
}

/// How many clips of the song name `file`.
fn uses(studio: &Rc<RefCell<Studio>>, file: &Path) -> usize {
    let p = studio.borrow().project();
    p.tracks
        .values()
        .flat_map(|t| t.clips.iter())
        .filter(|c| matches!(&c.body, ClipBody::Audio { file: f, .. } if Path::new(f) == file))
        .count()
}

fn listed(studio: &Rc<RefCell<Studio>>, name: &str, window: &Window) -> bool {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    file_row(&s, &g, name).is_some()
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let top = top();
    let old = top.join("Drums/kick_01.wav");
    let new = top.join("Drums/Kick A.wav");
    match n {
        0 => {
            crate::import_shots::make_place(&top);
            crate::browser_files::forget();
            {
                let mut s = studio.borrow_mut();
                s.drawer_open = false;
                s.browser.press_rail(RailTab::Files);
                s.browser.add_place(&top).unwrap();
                s.browser
                    .open_dirs
                    .insert(top.join("Drums").to_string_lossy().into_owned());
                let g = crate::screen::geometry(&s, window);
                s.resized(&g);
            }
            press_row(studio, "kick_01.wav", window, cx);
            choose(studio, "新しいトラックに置く", window, cx);
            assert_eq!(uses(studio, &old), 1);
            press_row(studio, "kick_01.wav", window, cx);
            choose(studio, "名前を変更", window, cx);
            let mut s = studio.borrow_mut();
            for _ in 0.."kick_01".len() {
                s.ask_key("backspace");
            }
            s.ask_text(None, "Kick A", false);
            s.ask_key("enter");
            assert_eq!(s.error, None);
            drop(s);
            assert!(new.is_file() && !old.exists());
            assert_eq!(uses(studio, &new), 1);
            assert!(listed(studio, "Kick A.wav", window));
        }
        1 => {
            assert_eq!(studio.borrow().error, None);
            assert!(old.is_file() && !new.exists(), "the name went back");
            assert_eq!(uses(studio, &old), 1);
            assert!(listed(studio, "kick_01.wav", window));
            assert!(!listed(studio, "Kick A.wav", window));
        }
        2 => {
            assert!(new.is_file() && !old.exists(), "renamed again");
            assert_eq!(uses(studio, &new), 1);
            assert!(listed(studio, "Kick A.wav", window));
        }
        3 => std::fs::write(&old, b"taken").unwrap(),
        _ => {
            let error = studio.borrow().error.clone().expect("the error is shown");
            assert!(error.contains("取り消していません"), "{error}");
            assert_eq!(uses(studio, &new), 1, "the clip stays on the new name");
            assert!(new.is_file());
            assert_eq!(std::fs::read(&old).unwrap(), b"taken");
        }
    }
}
