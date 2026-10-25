//! `--script browser-files-menu`: the new name, the new folder and the recycle bin in the
//! menus of a file (S41) and a folder (S42) of the files tab (CTX-059), pressed through the
//! window's pointer path. The bin is asked for and never chosen; the files are a fixture
//! under `target`. Silent; no OS input.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_design::Layout;

use crate::browser::{BRow, BrowserHit, RailTab};
use crate::shot::{Act, Play, Shot};
use crate::studio::Studio;

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    // A file's menu: the new name and the recycle bin after the explorer.
    Shot {
        name: "bfm01-file-menu",
        acts: &[Act::Reset, stage(0), stage(1)],
    },
    // The new name asked for in the menu's place, the name without its extension.
    Shot {
        name: "bfm02-rename-box",
        acts: &[stage(2)],
    },
    // Entered: the list shows the new name, still selected.
    Shot {
        name: "bfm03-renamed",
        acts: &[stage(3)],
    },
    // The bin asks once.
    Shot {
        name: "bfm04-bin-asks",
        acts: &[stage(4)],
    },
    // A file the song uses: the bin is dim and says why.
    Shot {
        name: "bfm05-used-file-dim",
        acts: &[stage(5)],
    },
    // A folder's menu: a new folder, the new name, and the bin dim for what is in use.
    Shot {
        name: "bfm06-folder-menu",
        acts: &[stage(6)],
    },
    // The new folder asked for, with a name the folder does not have.
    Shot {
        name: "bfm07-new-folder-box",
        acts: &[stage(7)],
    },
];

fn top() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/browser-files-menu-fixture/素材")
}

pub(crate) fn dispatch(
    p: (f32, f32),
    button: Option<MouseButton>,
    window: &mut Window,
    cx: &mut App,
) {
    let p = point(px(p.0), px(p.1 + Layout::TOP_BAR_H));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: p,
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
    let Some(button) = button else {
        return;
    };
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: p,
            button,
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// Right-presses the row of the list named `name` (a file or a folder).
pub(crate) fn press_row(
    studio: &Rc<RefCell<Studio>>,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let p = {
        let mut s = studio.borrow_mut();
        s.popup = None;
        s.ctx = None;
        let g = crate::screen::geometry(&s, window);
        let l = s.browser.layout(g.panel);
        let hit = l
            .rows
            .iter()
            .enumerate()
            .find_map(|(i, p)| match &p.row {
                BRow::File { name: n, .. } if n == name => Some(BrowserHit::File(i)),
                BRow::Dir { name: n, .. } if n == name => Some(BrowserHit::Dir(i)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no row {name}"));
        s.browser.center_of(&l, hit).unwrap()
    };
    dispatch(p, Some(MouseButton::Right), window, cx);
    assert!(studio.borrow().context_open(), "no menu on {name}");
}

/// Where the menu's row `label` is, and whether it can be chosen.
fn row(studio: &Rc<RefCell<Studio>>, label: &str, window: &Window) -> ((f32, f32), bool) {
    let s = studio.borrow();
    let g = crate::screen::geometry(&s, window);
    let layout = s.popup.as_ref().expect("menu").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label && !matches!(c.look, crate::popup::Look::Input))
        .unwrap_or_else(|| panic!("missing row {label}"));
    (
        (
            cell.rect.x + cell.rect.w / 2.0,
            cell.rect.y + cell.rect.h / 2.0,
        ),
        cell.live(),
    )
}

pub(crate) fn choose(studio: &Rc<RefCell<Studio>>, label: &str, window: &mut Window, cx: &mut App) {
    let (p, live) = row(studio, label, window);
    assert!(live, "disabled row {label}");
    dispatch(p, Some(MouseButton::Left), window, cx);
}

fn play(n: u8, studio: &Rc<RefCell<Studio>>, window: &mut Window, cx: &mut App) {
    let top = top();
    match n {
        0 => {
            crate::import_shots::make_place(&top);
            crate::browser_files::forget();
            let mut s = studio.borrow_mut();
            s.drawer_open = false;
            s.browser.press_rail(RailTab::Files);
            s.browser.add_place_fixture(&top).unwrap();
            for dir in ["Drums", "Loops"] {
                s.browser
                    .open_dirs
                    .insert(top.join(dir).to_string_lossy().into_owned());
            }
            let g = crate::screen::geometry(&s, window);
            s.resized(&g);
            drop(s);
            // The song uses the first kick.
            press_row(studio, "kick_01.wav", window, cx);
            choose(studio, "新しいトラックに置く", window, cx);
        }
        1 => press_row(studio, "loop_120bpm.wav", window, cx),
        2 => {
            choose(studio, "名前を変更", window, cx);
            assert_eq!(
                studio.borrow().ask_field().map(|f| f.value.clone()),
                Some("loop_120bpm".into())
            );
        }
        3 => {
            let mut s = studio.borrow_mut();
            for _ in 0.."loop_120bpm".len() {
                s.ask_key("backspace");
            }
            s.ask_text(None, "groove", false);
            s.ask_key("enter");
            assert_eq!(s.error, None);
            assert!(top.join("Loops/groove.wav").is_file());
        }
        4 => {
            press_row(studio, "groove.wav", window, cx);
            choose(studio, "ごみ箱へ", window, cx);
            assert_eq!(
                studio
                    .borrow()
                    .context_rows()
                    .iter()
                    .filter(|r| !r.gap)
                    .map(|r| r.label.clone())
                    .collect::<Vec<_>>(),
                ["戻る", "ファイルをごみ箱へ"]
            );
        }
        5 => {
            press_row(studio, "kick_01.wav", window, cx);
            let (p, live) = row(studio, "ごみ箱へ", window);
            assert!(!live, "a file the song uses is not binned");
            dispatch(p, None, window, cx);
        }
        6 => {
            press_row(studio, "Drums", window, cx);
            let (_, live) = row(studio, "ごみ箱へ", window);
            assert!(!live, "nor the folder it is in");
        }
        _ => {
            choose(studio, "新しいフォルダー", window, cx);
            assert_eq!(
                studio.borrow().ask_field().map(|f| f.value.clone()),
                Some("新しいフォルダー".into())
            );
        }
    }
}
