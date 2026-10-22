//! KEY-012・KEY-015・KEY-019・OPS-004・SET-001: the macros in the shortcut list, the mouse table with
//! its holds, the key presets, and the search by key. The pointer goes through the offscreen
//! window; the settings live in a folder of the script's own.
use crate::keymap_shots::{click, press, read};
use crate::settings::{Do, MenuKind, Section, Sub};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Hit, Studio};
use gpui::{App, Window};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

const fn macros(n: u8) -> Act {
    Act::Stage(Play::Window(play_macros), n)
}
const fn mouse(n: u8) -> Act {
    Act::Stage(Play::Window(play_mouse), n)
}
const fn presets(n: u8) -> Act {
    Act::Stage(Play::Window(play_presets), n)
}
const fn search(n: u8) -> Act {
    Act::Stage(Play::Window(play_search), n)
}

pub const MACROS: &[Shot] = &[
    Shot {
        name: "01-list",
        acts: &[macros(0)],
    },
    Shot {
        name: "02-steps",
        acts: &[macros(1)],
    },
    Shot {
        name: "03-add-step",
        acts: &[macros(2)],
    },
];

pub const MOUSE: &[Shot] = &[
    Shot {
        name: "01-roll",
        acts: &[mouse(0)],
    },
    Shot {
        name: "02-arrange",
        acts: &[mouse(1)],
    },
    Shot {
        name: "03-hold-hides-click",
        acts: &[mouse(2)],
    },
    Shot {
        name: "04-modifier-taken",
        acts: &[mouse(3)],
    },
    Shot {
        name: "05-add-hold",
        acts: &[mouse(4)],
    },
];

pub const PRESETS: &[Shot] = &[
    Shot {
        name: "01-own-row",
        acts: &[presets(0)],
    },
    Shot {
        name: "02-preset-menu",
        acts: &[presets(1)],
    },
    Shot {
        name: "03-cubase-applied",
        acts: &[presets(2)],
    },
    Shot {
        name: "04-more-menu",
        acts: &[presets(3)],
    },
    Shot {
        name: "05-reset-all",
        acts: &[presets(4)],
    },
];

pub const SEARCH: &[Shot] = &[
    Shot {
        name: "01-q",
        acts: &[search(0)],
    },
    Shot {
        name: "02-d",
        acts: &[search(1)],
    },
];

/// The settings of the script's own folder, open on 編集 → ショートカット.
fn open_keys(tag: &str, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let dir = std::env::temp_dir().join(format!("xirius-controls-{tag}-{}", std::process::id()));
    studio.borrow_mut().song.load_settings(&dir);
    let at = read(studio, window, |s, g| {
        s.center_of(g, Hit::RailSettings).unwrap()
    });
    click(at, 1, window, cx);
    press(studio, Do::Nav(Section::Keys), window, cx);
}

/// A row of the open settings menu, by the value it picks.
fn pick(studio: &Rc<RefCell<Studio>>, value: &str, window: &mut Window, cx: &mut App) {
    let at = read(studio, window, |s, _| {
        let menu = s.settings.as_ref().unwrap().menu.as_ref().expect("a menu");
        let cell = menu
            .menu
            .layout
            .cells
            .iter()
            .find(|c| c.action == Some(crate::popup::Action::Pick(value.into())))
            .unwrap_or_else(|| panic!("missing choice {value}"));
        assert!(cell.live(), "disabled choice {value}");
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        )
    });
    click(at, 1, window, cx);
}

fn set(studio: &Rc<RefCell<Studio>>, command: &str, args: serde_json::Value) {
    studio
        .borrow_mut()
        .song
        .set(command, args)
        .unwrap_or_else(|e| panic!("{command}: {e}"));
}

fn play_macros(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            open_keys("macros", studio, window, cx);
            set(
                studio,
                "macro.set",
                json!({"name": "仕上げ", "steps": [
                    {"command": "note.quantize --grid 1/16"},
                    {"command": "note.humanize"},
                    {"command": "note.mute --off"},
                ]}),
            );
            set(
                studio,
                "keymap.set",
                json!({"key": "Alt+M", "command": "macro.run --name 仕上げ", "context": "roll"}),
            );
        }
        1 => press(studio, Do::MacroOpen("仕上げ".into()), window, cx),
        _ => press(studio, Do::MacroPick, window, cx),
    }
}

fn play_mouse(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            open_keys("mouse", studio, window, cx);
            press(studio, Do::Sub(Sub::Mouse), window, cx);
        }
        1 => press(studio, Do::Area("arrange".into()), window, cx),
        2 => set(
            studio,
            "mousemap.set",
            json!({"context": "arrange.select.clip-body.click", "modifier": "Ctrl",
                   "action": "select.toggle"}),
        ),
        3 => {
            press(studio, Do::Area("roll".into()), window, cx);
            press(
                studio,
                Do::Menu(MenuKind::HoldMod("roll.select.hold".into(), "Mod".into())),
                window,
                cx,
            );
            pick(studio, "Mod+Shift", window, cx);
        }
        _ => press(
            studio,
            Do::Menu(MenuKind::HoldAdd("roll".into())),
            window,
            cx,
        ),
    }
}

fn play_presets(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    match n {
        0 => {
            open_keys("presets", studio, window, cx);
            set(
                studio,
                "keymap.set",
                json!({"key": "Ctrl+Alt+Q", "command": "note.quantize", "context": "roll"}),
            );
        }
        1 => press(studio, Do::Menu(MenuKind::Preset), window, cx),
        2 => pick(studio, "cubase", window, cx),
        3 => press(studio, Do::Menu(MenuKind::More), window, cx),
        _ => pick(studio, "reset-all", window, cx),
    }
}

fn play_search(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    if n == 0 {
        open_keys("search", studio, window, cx);
        press(studio, Do::KbKey("Q".into()), window, cx);
    } else {
        press(studio, Do::KbKey("D".into()), window, cx);
    }
}
