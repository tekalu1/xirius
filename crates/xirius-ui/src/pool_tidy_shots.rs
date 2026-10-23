//! `--script pool-tidy`: the order of the song-files tab and 「使われていない録音を整理」 (BRW-004)
//! through the real (parked, silent) window's presses and keys, on the song of `--script pool`
//! with two more recordings no clip uses. The song is made in `XIRIUS_POOL_DIR`; what goes to
//! the bin goes to `XIRIUS_TRASH_DIR`, which must be set (else it would be the PC's recycle
//! bin). Each stage prints `pool-tidy-check stage=N ok=true` (`tests/pool_window.rs` reads the
//! lines).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
    Window, point, px,
};
use xirius_design::Layout;

use crate::browser::{BRow, RailTab, View};
use crate::cleanup_dialog::{Act as Clean, CleanLook};
use crate::pool_shots::{VOCAL_1, VOCAL_2, make_song};
use crate::popup::{Look, PopupKind};
use crate::shot::{Act, Play, Shot};
use crate::studio::{Geometry, Rect, Studio};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Window(play), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "pool-tidy-00-orders",
        acts: &[stage(0)],
    },
    Shot {
        name: "pool-tidy-01-by-name",
        acts: &[stage(1)],
    },
    Shot {
        name: "pool-tidy-02-cleanup",
        acts: &[stage(2)],
    },
    Shot {
        name: "pool-tidy-03-keys",
        acts: &[stage(3)],
    },
    Shot {
        name: "pool-tidy-04-trashed",
        acts: &[stage(4)],
    },
];

/// The recordings no clip uses, besides `VOCAL_2`.
const MORE: [&str; 2] = [
    "Media/Recordings/Vocal_03.wav",
    "Media/Recordings/Gtr_take2.wav",
];

fn root() -> PathBuf {
    std::env::var_os("XIRIUS_POOL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xirius-pool-tidy-shots"))
}

fn trashed() -> Vec<String> {
    let dir = std::env::var_os("XIRIUS_TRASH_DIR").expect("XIRIUS_TRASH_DIR");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

type Studio1 = Rc<RefCell<Studio>>;

fn at(p: (f32, f32)) -> gpui::Point<gpui::Pixels> {
    point(px(p.0), px(p.1 + Layout::TOP_BAR_H))
}

fn hover(p: (f32, f32), w: &mut Window, cx: &mut App) {
    w.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: at(p),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn click(p: (f32, f32), w: &mut Window, cx: &mut App) {
    hover(p, w, cx);
    w.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    w.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: at(p),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

fn keys(text: &str, w: &mut Window, cx: &mut App) {
    for key in text.split(' ') {
        w.dispatch_keystroke(gpui::Keystroke::parse(key).unwrap(), cx);
    }
}

fn geometry(s: &Studio1, w: &Window) -> Geometry {
    crate::screen::geometry(&s.borrow(), w)
}

fn mid(r: Rect) -> (f32, f32) {
    (r.x + r.w / 2.0, r.y + r.h / 2.0)
}

/// The middle of the cell `label` of the popup that is open.
fn cell(s: &Studio1, g: &Geometry, label: &str) -> (f32, f32) {
    let s = s.borrow();
    let layout = s.popup.as_ref().expect("a popup is open").layout(g.size);
    let cell = layout
        .cells
        .iter()
        .find(|c| c.text == label)
        .unwrap_or_else(|| panic!("no cell {label}"));
    mid(cell.rect)
}

/// The files of the list in its order, each with its icon.
fn files(s: &Studio1, g: &Geometry) -> Vec<(String, &'static str)> {
    s.borrow()
        .browser
        .layout(g.panel)
        .rows
        .iter()
        .filter_map(|p| match &p.row {
            BRow::Pool { name, icon, .. } => Some((name.clone(), *icon)),
            _ => None,
        })
        .collect()
}

/// The files the dialog lists, and its count.
fn listed(s: &Studio1, g: &Geometry) -> (Vec<String>, String) {
    let s = s.borrow();
    let p = s.popup.as_ref().expect("the dialog");
    assert_eq!(p.kind, PopupKind::Cleanup);
    let l = p.layout(g.size);
    let names = l
        .cells
        .iter()
        .filter(|c| c.look == Look::Clean(CleanLook::File))
        .map(|c| c.text.clone())
        .collect();
    let count = l
        .cells
        .iter()
        .find(|c| c.look == Look::Clean(CleanLook::Total))
        .map(|c| c.text.clone())
        .unwrap_or_default();
    (names, count)
}

/// The press on 「…」 and on 「使われていない録音を整理…」.
fn open_cleanup(s: &Studio1, w: &mut Window, cx: &mut App) {
    let g = geometry(s, w);
    let more = s.borrow().browser.layout(g.panel).more.expect("the 「…」");
    click(mid(more), w, cx);
    let at = cell(s, &g, "使われていない録音を整理…");
    click(at, w, cx);
}

fn play(n: u8, s: &Studio1, w: &mut Window, cx: &mut App) {
    let root = root();
    let song = root.join("曲");
    match n {
        0 => {
            assert!(
                std::env::var_os("XIRIUS_TRASH_DIR").is_some(),
                "the cleanup would go to the PC's recycle bin"
            );
            if root.exists() {
                std::fs::remove_dir_all(&root).unwrap();
            }
            let dir = make_song(&root);
            for f in MORE {
                std::fs::copy(dir.join(VOCAL_2), dir.join(f)).unwrap();
            }
            assert!(s.borrow_mut().command(|song| song.open(&dir)));
            s.borrow_mut().drawer_open = false;
            let g = geometry(s, w);
            click(
                mid(crate::browser::rail_button(g.rail, RailTab::SongFiles)),
                w,
                cx,
            );
            s.borrow_mut().follow_browser();
            let g = geometry(s, w);
            let view = s.borrow().browser.layout(g.panel).view.expect("the order");
            click(mid(view), w, cx);
            let st = s.borrow();
            let p = st.popup.as_ref().expect("the list of orders");
            let texts: Vec<String> = p
                .layout(g.size)
                .cells
                .iter()
                .filter(|c| !matches!(c.look, Look::Input))
                .map(|c| c.text.clone())
                .collect();
            assert_eq!(texts, ["種類別", "名前順"]);
        }
        1 => {
            let g = geometry(s, w);
            let at = cell(s, &g, "名前順");
            click(at, w, cx);
            assert!(s.borrow().popup.is_none());
            assert_eq!(s.borrow().browser.pool_view, View::Name);
            let list = files(s, &g);
            let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
            let mut sorted = names.clone();
            sorted.sort_by_key(|n| n.to_lowercase());
            assert_eq!(names, sorted);
            let icon = |n: &str| list.iter().find(|(m, _)| m == n).unwrap().1;
            assert_eq!(icon("Vocal_01.wav"), "mic");
            assert_eq!(icon("Kick.wav"), "file");
            assert_eq!(icon("Bass.wav"), "alert");
            // The pointer rests on the list, clear of the buttons' tips.
            hover(
                (g.panel.x + g.panel.w / 2.0, g.panel.bottom() - 40.0),
                w,
                cx,
            );
        }
        2 => {
            open_cleanup(s, w, cx);
            let g = geometry(s, w);
            assert!(!s.borrow().context_open());
            let (names, count) = listed(s, &g);
            assert_eq!(names, ["Gtr_take2.wav", "Vocal_02.wav", "Vocal_03.wav"]);
            assert_eq!(count, "3 個");
            let focus = s.borrow().popup.as_ref().unwrap().cleanup.clone().unwrap();
            assert_eq!(focus.focus, Clean::Trash);
            assert!(!focus.keyed);
            // The pointer rests on the title, so no button looks hovered.
            let at = cell(s, &g, crate::cleanup_dialog::TITLE);
            hover(at, w, cx);
        }
        3 => {
            keys("tab", w, cx);
            let v = s.borrow().popup.as_ref().unwrap().cleanup.clone().unwrap();
            assert_eq!(v.focus, Clean::Cancel);
            assert!(v.keyed, "the ring shows");
        }
        4 => {
            // Enter on キャンセル closes it and keeps the files.
            keys("enter", w, cx);
            assert!(s.borrow().popup.is_none());
            assert!(song.join(VOCAL_2).is_file());
            assert!(trashed().is_empty());
            // Esc too.
            open_cleanup(s, w, cx);
            keys("escape", w, cx);
            assert!(s.borrow().popup.is_none());
            open_cleanup(s, w, cx);
            let g = geometry(s, w);
            let at = cell(s, &g, crate::cleanup_dialog::TRASH);
            click(at, w, cx);
            assert!(s.borrow().popup.is_none());
            assert_eq!(s.borrow().error, None);
            assert_eq!(
                s.borrow().notice_text(),
                Some("使われていない録音 3 個をごみ箱に移動しました")
            );
            assert_eq!(trashed(), ["Gtr_take2.wav", "Vocal_02.wav", "Vocal_03.wav"]);
            assert!(song.join(VOCAL_1).is_file(), "a used take stays");
            s.borrow_mut().follow_browser();
            let g = geometry(s, w);
            assert!(
                !files(s, &g)
                    .iter()
                    .any(|(n, _)| n == "Vocal_02.wav" || n == "Vocal_03.wav")
            );
            hover(
                (g.panel.x + g.panel.w / 2.0, g.panel.bottom() - 40.0),
                w,
                cx,
            );
        }
        _ => unreachable!(),
    }
    let error = s.borrow_mut().error.take();
    assert_eq!(error, None);
    eprintln!("pool-tidy-check stage={n} ok=true");
}
